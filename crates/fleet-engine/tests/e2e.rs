//! End-to-end tests: real git repositories, real child processes (the
//! `fake-ai` binary standing in for Claude/Codex/Antigravity), real SQLite.

use fleet_engine::providers::ModelSpec;
use fleet_engine::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git").args(args).current_dir(dir).output().map(|o| o.status.success()).unwrap_or(false)
}

struct Env {
    repo: tempfile::TempDir,
    scratch: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q"]);
        git(repo.path(), &["checkout", "-q", "-b", "main"]);
        std::fs::write(repo.path().join("README.md"), "demo\n").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "init"]);
        Env { repo, scratch: tempfile::tempdir().unwrap() }
    }

    fn provider(&self, id: &str, plan: &str) -> ProviderSpec {
        let mut p = ProviderSpec::generic(
            id,
            env!("CARGO_BIN_EXE_fake-ai"),
            &["--model", "{model}", "--effort", "{effort}"],
            vec![
                ModelSpec { name: "small".into(), tier: 1, max_effort: Effort::Max },
                ModelSpec { name: "large".into(), tier: 3, max_effort: Effort::Max },
            ],
        );
        p.env.insert("FAKE_AI_NAME".into(), id.into());
        p.env.insert("FAKE_AI_PLAN".into(), plan.into());
        p.env.insert("FAKE_AI_COUNTER_FILE".into(), self.scratch_path(&format!("{id}.count")));
        p.env.insert("FAKE_AI_PROMPT_LOG".into(), self.scratch_path(&format!("{id}.prompts")));
        p.env.insert("FAKE_AI_CHILD_PID_FILE".into(), self.scratch_path(&format!("{id}.childpid")));
        p
    }

    fn scratch_path(&self, name: &str) -> String {
        self.scratch.path().join(name).to_string_lossy().to_string()
    }

    fn prompts(&self, id: &str) -> String {
        std::fs::read_to_string(self.scratch.path().join(format!("{id}.prompts"))).unwrap_or_default()
    }

    fn engine(&self, providers: Vec<ProviderSpec>) -> Engine {
        let mut cfg = EngineConfig::new(self.repo.path().to_path_buf(), providers);
        cfg.run_timeout = Duration::from_secs(60);
        cfg.idle_timeout = Duration::from_secs(30);
        cfg.verify_timeout = Duration::from_secs(60);
        cfg.poll_interval = Duration::from_millis(50);
        cfg.wait_for_cooldown = false;
        Engine::open(cfg).unwrap()
    }

    fn integration_file(&self, path: &str) -> Option<String> {
        let spec = format!("ai-fleet/integration:{path}");
        if !git_ok(self.repo.path(), &["cat-file", "-e", &spec]) {
            return None;
        }
        Some(git(self.repo.path(), &["show", &spec]))
    }
}

fn task(title: &str) -> NewTask {
    let mut t = NewTask::new(title, format!("{title}: please implement it"));
    t.complexity = Some(Complexity::Standard);
    t
}

fn verify_file_exists(name: &str) -> String {
    if cfg!(windows) {
        format!("if exist {name} (exit 0) else (echo {name} missing & exit 1)")
    } else {
        format!("test -f {name} || (echo {name} missing; exit 1)")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn happy_path_with_independent_review_and_merge() {
    let env = Env::new();
    let engine = env.engine(vec![env.provider("alpha", "ok"), env.provider("beta", "approve")]);
    let mut nt = task("add feature");
    nt.preferred_provider = Some("alpha".into());
    let t = engine.submit(nt).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t.id.clone()], "{summary:?}");

    let t = engine.store().get_task(&t.id).unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::Done);
    assert_eq!(t.authors, vec!["alpha".to_string()]);
    assert_eq!(t.reviewer.as_deref(), Some("beta"));
    assert!(env.integration_file("output.txt").unwrap().contains("by alpha"));
    // The user's own checkout is untouched.
    assert!(!env.repo.path().join("output.txt").exists());
    assert!(git(env.repo.path(), &["status", "--porcelain"]).trim().is_empty(), "engine state leaked into git status");
    // Reviewer saw the real diff.
    assert!(env.prompts("beta").contains("+by alpha"));
    assert_eq!(engine.store().verify_events().unwrap(), Ok(engine.store().events(None, 10_000).unwrap().len()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quota_mid_task_hands_off_without_losing_work() {
    let env = Env::new();
    let engine = env.engine(vec![
        env.provider("alpha", "partial-quota"),
        env.provider("beta", "ok"),
        env.provider("gamma", "approve"),
    ]);
    let mut nt = task("long job");
    nt.preferred_provider = Some("alpha".into());
    let t = engine.submit(nt).unwrap();

    let started = chrono::Utc::now();
    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t.id.clone()], "{summary:?}");

    let out = env.integration_file("output.txt").unwrap();
    assert!(out.contains("partial by alpha"), "alpha's partial work was lost: {out}");
    assert!(out.contains("by beta"));

    let beta_prompt = env.prompts("beta");
    assert!(beta_prompt.contains("ran out of subscription quota"), "{beta_prompt}");
    assert!(beta_prompt.contains("Work already on this branch"));

    let runs = engine.store().runs_for_task(&t.id).unwrap();
    assert_eq!(runs[0].provider, "alpha");
    assert_eq!(runs[0].failure_kind.as_deref(), Some("quota"));
    assert_eq!(runs[1].provider, "beta");
    assert_eq!(runs[1].effort, "medium", "quota handoff must not escalate effort");
    assert_eq!(runs[2].provider, "gamma");
    assert_eq!(runs[2].role, "reviewer");

    let t = engine.store().get_task(&t.id).unwrap().unwrap();
    assert_eq!((t.attempts, t.handoffs), (0, 1));
    let cd = engine.store().active_cooldowns(chrono::Utc::now()).unwrap();
    let wait = cd.get("alpha").expect("alpha parked").until - started;
    assert!((120..=126).contains(&wait.num_minutes()), "reset time parsed from 'try again in 2h 5m', got {wait}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_failure_escalates_effort_and_retries() {
    let env = Env::new();
    let engine = env.engine(vec![env.provider("alpha", "ok,touch:verify-ok")]);
    let mut nt = task("make tests pass");
    nt.review = false;
    nt.verify_cmd = Some(verify_file_exists("verify-ok"));
    let t = engine.submit(nt).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t.id.clone()], "{summary:?}");
    let runs = engine.store().runs_for_task(&t.id).unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].failure_kind.as_deref(), Some("verify_failed"));
    assert_eq!((runs[0].model.as_str(), runs[0].effort.as_str()), ("large", "medium"));
    assert_eq!(runs[1].effort, "high", "effort must escalate after a work failure");
    let prompts = env.prompts("alpha");
    assert!(prompts.contains("did not pass verification"));
    assert!(prompts.contains("verify-ok missing"));
    assert!(env.integration_file("verify-ok").is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scope_violation_is_rolled_back() {
    let env = Env::new();
    let engine = env.engine(vec![env.provider("alpha", "out-of-scope,ok:src/lib.txt")]);
    let mut nt = task("scoped change");
    nt.review = false;
    nt.owned_files = vec!["src/**".into()];
    let t = engine.submit(nt).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t.id.clone()], "{summary:?}");
    let runs = engine.store().runs_for_task(&t.id).unwrap();
    assert_eq!(runs[0].failure_kind.as_deref(), Some("scope_violation"));
    assert!(env.integration_file("src/lib.txt").is_some());
    assert!(env.integration_file("outside.txt").is_none(), "out-of-scope file must never reach integration");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn review_rejection_feeds_back_to_author() {
    let env = Env::new();
    let engine = env.engine(vec![env.provider("alpha", "ok"), env.provider("beta", "reject,approve")]);
    let mut nt = task("needs review");
    nt.preferred_provider = Some("alpha".into());
    let t = engine.submit(nt).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t.id.clone()], "{summary:?}");
    let prompts = env.prompts("alpha");
    assert!(prompts.contains("Reviewer feedback you must address"));
    assert!(prompts.contains("add tests for the new behaviour"));
    let roles: Vec<(String, String)> =
        engine.store().runs_for_task(&t.id).unwrap().into_iter().map(|r| (r.role, r.provider)).collect();
    assert_eq!(
        roles,
        vec![
            ("author".into(), "alpha".into()),
            ("reviewer".into(), "beta".into()),
            ("author".into(), "alpha".into()),
            ("reviewer".into(), "beta".into()),
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_provider_cannot_approve_its_own_work() {
    let env = Env::new();
    let engine = env.engine(vec![env.provider("alpha", "ok,approve")]);
    let t = engine.submit(task("solo")).unwrap();
    let summary = engine.run_until_idle().await.unwrap();
    assert!(summary.done.is_empty());
    let (id, why) = &summary.pending[0];
    assert_eq!(id, &t.id);
    assert!(why.contains("authored this change"), "{why}");
    assert_eq!(engine.store().get_task(&t.id).unwrap().unwrap().status, TaskStatus::AwaitingReview);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_agent_and_its_children_are_killed() {
    let env = Env::new();
    let mut cfg_engine_providers = vec![env.provider("alpha", "hang-tree")];
    cfg_engine_providers[0].max_concurrency = 1;
    let mut cfg = EngineConfig::new(env.repo.path().to_path_buf(), cfg_engine_providers);
    cfg.idle_timeout = Duration::from_secs(2);
    cfg.run_timeout = Duration::from_secs(60);
    cfg.poll_interval = Duration::from_millis(50);
    cfg.wait_for_cooldown = false;
    let engine = Engine::open(cfg).unwrap();
    let mut nt = task("hangs");
    nt.review = false;
    nt.max_attempts = 1;
    let t = engine.submit(nt).unwrap();

    let started = Instant::now();
    let summary = engine.run_until_idle().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(30));
    assert_eq!(summary.failed.len(), 1, "{summary:?}");
    let t = engine.store().get_task(&t.id).unwrap().unwrap();
    assert_eq!(t.last_failure.as_deref(), Some("stalled"));

    // The grandchild must be gone too.
    let pid = std::fs::read_to_string(env.scratch.path().join("alpha.childpid")).expect("child started");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!process_alive(pid.trim()), "child process {pid} survived");
}

fn process_alive(pid: &str) -> bool {
    if cfg!(windows) {
        let out = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).contains(pid)
    } else {
        Command::new("kill").args(["-0", pid]).status().map(|s| s.success()).unwrap_or(false)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_dependency_blocks_dependents() {
    let env = Env::new();
    let engine = env.engine(vec![env.provider("alpha", "crash")]);
    let mut a = task("first");
    a.review = false;
    a.max_attempts = 2;
    let a = engine.submit(a).unwrap();
    let mut b = task("second");
    b.depends_on = vec![a.id.clone()];
    let b = engine.submit(b).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.failed.len(), 1);
    assert_eq!(summary.blocked[0].0, b.id);
    assert_eq!(engine.store().runs_for_task(&a.id).unwrap().len(), 2);
    assert!(engine.store().runs_for_task(&b.id).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_tasks_merge_cleanly() {
    let env = Env::new();
    let mut alpha = env.provider("alpha", "ok");
    alpha.max_concurrency = 3;
    // Each task writes its own file so they can run concurrently.
    alpha.env.remove("FAKE_AI_COUNTER_FILE");
    let engine = env.engine(vec![alpha]);
    let mut ids = vec![];
    for i in 0..3 {
        let mut nt = task(&format!("part {i}"));
        nt.review = false;
        let t = engine.submit(nt).unwrap();
        ids.push(t.id);
    }
    // Same output file in every task -> later merges conflict and must be re-queued, then fail
    // after max attempts OR resolve. Here we only assert the engine never corrupts integration.
    let summary = engine.run_until_idle().await.unwrap();
    assert!(!summary.done.is_empty(), "{summary:?}");
    let out = env.integration_file("output.txt").unwrap();
    assert!(!out.contains("<<<<<<<"), "conflict markers leaked into integration");
    for (id, why) in &summary.failed {
        assert!(why.contains("conflicts in") , "{id}: {why}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn engine_restart_recovers_from_crashed_worker() {
    let env = Env::new();
    let providers = vec![env.provider("alpha", "ok"), env.provider("beta", "approve")];
    let id = {
        let engine = env.engine(providers.clone());
        let t = engine.submit(task("survives crash")).unwrap();
        // Simulate a worker that claimed the task and then the process died.
        engine
            .store()
            .claim(&t.id, TaskStatus::Queued, TaskStatus::Running, chrono::Duration::milliseconds(1))
            .unwrap()
            .unwrap();
        t.id
    };
    std::thread::sleep(Duration::from_millis(20));
    let engine = env.engine(providers);
    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![id.clone()], "{summary:?}");
    let events = engine.store().events(Some(&id), 100).unwrap();
    assert!(events.iter().any(|e| e.kind == "task.recovered"));
}

#[test]
fn elite_120_skills_embedded_and_cover_all_12_domains() {
    let reg = SkillRegistry::load(None);
    assert!(reg.len() >= 120, "expected >= 120 bundled elite skills, got {}", reg.len());

    let expected_domains = [
        "software-engineering",
        "architecture",
        "thinking-wisdom",
        "principal-experience",
        "design-ui-ux",
        "languages",
        "accounting-erp",
        "auditing-security",
        "mathematics",
        "physics-simulation",
        "economics-quant",
        "product-features",
    ];
    for dom in expected_domains {
        let in_dom = reg.list(Some(dom));
        assert!(in_dom.len() >= 10, "domain `{dom}` only has {} skills", in_dom.len());
        for spec in in_dom {
            assert!(spec.body.len() >= 400, "skill `{}` body too short ({})", spec.id, spec.body.len());
            assert!(spec.stars >= 30_000, "skill `{}` stars unexpectedly low ({})", spec.id, spec.stars);
        }
    }

    // Domain precision checks (English + Arabic + file extensions).
    let m_acc = reg.select(
        "بناء نظام محاسبة قيد مزدوج ومطابقة دفتر الأستاذ وضريبة القيمة المضافة",
        &["migrations/0014_ledger.sql".into()],
        fleet_engine::router::Role::Author,
        &[],
        3,
    );
    assert!(!m_acc.is_empty());
    assert!(
        m_acc.iter().any(|m| m.domain == "accounting-erp"),
        "expected accounting-erp skill for Arabic ledger prompt, got {m_acc:?}"
    );

    let m_phys = reg.select(
        "Simulate open quantum system Lindblad master equation and Navier-Stokes fluid dynamics",
        &["sim/quantum.py".into()],
        fleet_engine::router::Role::Author,
        &[],
        3,
    );
    assert!(
        m_phys.iter().any(|m| m.domain == "physics-simulation"),
        "expected physics-simulation skill, got {m_phys:?}"
    );

    let m_econ = reg.select(
        "Design SaaS pricing elasticity, auction game-theory incentives, and portfolio VaR risk metrics",
        &[],
        fleet_engine::router::Role::Author,
        &[],
        3,
    );
    assert!(
        m_econ.iter().any(|m| m.domain == "economics-quant"),
        "expected economics-quant skill, got {m_econ:?}"
    );

    let m_math = reg.select(
        "Symbolic Jacobian derivation in SymPy and Bayesian MCMC posterior inference in PyMC",
        &["model.py".into()],
        fleet_engine::router::Role::Author,
        &[],
        3,
    );
    assert!(
        m_math.iter().any(|m| m.domain == "mathematics"),
        "expected mathematics skill, got {m_math:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn engine_auto_injects_domain_skills_into_author_and_reviewer_prompts() {
    let env = Env::new();
    let engine = env.engine(vec![env.provider("alpha", "ok:src/ledger.rs"), env.provider("beta", "approve")]);
    let mut nt = NewTask::new(
        "محاسبة وتدقيق دفتر الأستاذ",
        "Implement double-entry ledger reconciliation in Rust with PostgreSQL Row-Level Security audit",
    );
    nt.owned_files = vec!["src/**".into()];
    nt.skills = vec!["party-ledger-reconciliation".into()];
    nt.preferred_provider = Some("alpha".into());
    let t = engine.submit(nt).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t.id.clone()], "{summary:?}");

    let author_prompts = env.prompts("alpha");
    assert!(author_prompts.contains("## Activated Expert Skills"), "skills section missing from author prompt");
    assert!(author_prompts.contains("party-ledger-reconciliation"), "pinned accounting skill missing");

    let reviewer_prompts = env.prompts("beta");
    assert!(reviewer_prompts.contains("## Activated Expert Skills"), "skills section missing from reviewer prompt");

    let events = engine.store().events(Some(&t.id), 100).unwrap();
    assert!(events.iter().any(|e| e.kind == "skills.activated"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn static_audit_gate_blocks_float_money_and_synthesizes_reflexion_lesson() {
    let env = Env::new();
    let engine = env.engine(vec![
        env.provider("alpha", "bad-float-money,good-decimal-money,ok:src/next.rs"),
    ]);

    let mut t1 = NewTask::new("invoice money model", "Create src/invoice.rs for invoice accounting totals");
    t1.review = false;
    t1.owned_files = vec!["src/**".into()];
    let t1 = engine.submit(t1).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t1.id.clone()], "{summary:?}");

    let runs = engine.store().runs_for_task(&t1.id).unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].failure_kind.as_deref(), Some("static_audit_failed"));
    assert_eq!(runs[1].outcome.as_deref(), Some("success"));
    assert!(env.integration_file("src/invoice.rs").unwrap().contains("total_minor_units: i64"));

    // Reflexion lesson MUST now exist in persistent Hybrid Memory and be injected into future tasks!
    let mem_hits = engine.memory().search("invoice accounting money", &["src/invoice.rs".into()], 5).unwrap();
    assert!(mem_hits.iter().any(|h| h.record.kind == MemoryKind::Reflexion), "expected Reflexion memory, got {mem_hits:?}");

    let mut t2 = NewTask::new("follow-up invoice ledger", "Extend invoice accounting module");
    t2.review = false;
    t2.owned_files = vec!["src/**".into()];
    let t2 = engine.submit(t2).unwrap();
    let summary2 = engine.run_until_idle().await.unwrap();
    assert!(summary2.done.contains(&t2.id));

    let prompts = env.prompts("alpha");
    assert!(prompts.contains("## Institutional Memory & Past Reflexion Lessons"), "reflexion memory was not injected into follow-up prompt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queen_decomposes_epic_into_waves_and_executes_dag() {
    let env = Env::new();
    std::fs::create_dir_all(env.repo.path().join("src")).unwrap();
    std::fs::create_dir_all(env.repo.path().join("web")).unwrap();
    std::fs::write(env.repo.path().join("src/lib.rs"), "pub fn core() {}\n").unwrap();
    std::fs::write(env.repo.path().join("web/app.ts"), "export function ui() {}\n").unwrap();
    git(env.repo.path(), &["add", "."]);
    git(env.repo.path(), &["commit", "-q", "-m", "layout"]);

    let mut p_spec = env.provider("alpha", "ok:docs/specs/spec.md,ok:src/feature.rs,ok:web/view.ts,ok:tests/it.rs");
    p_spec.max_concurrency = 1; // deterministic counter sequence across waves
    let engine = env.engine(vec![p_spec]);

    let graph = CodeGraph::scan(env.repo.path(), 200).unwrap();
    let plan = fleet_engine::queen::decompose_architecturally(
        "Build multi-tenant billing module and dashboard UI",
        env.repo.path(),
        &graph,
        engine.skills(),
        None,
    )
    .unwrap();
    assert!(plan.waves >= 3, "expected >= 3 DAG waves, got {}", plan.waves);
    assert_eq!(plan.nodes.len(), 4);

    let ids = engine.submit_plan(&plan, false).unwrap();
    assert_eq!(ids.len(), 4);

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done.len(), 4, "{summary:?}");
    assert!(env.integration_file("docs/specs/spec.md").is_some());
    assert!(env.integration_file("src/feature.rs").is_some());
    assert!(env.integration_file("web/view.ts").is_some());
    assert!(env.integration_file("tests/it.rs").is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quorum_review_requires_two_distinct_non_author_approvals() {
    let env = Env::new();
    let engine = env.engine(vec![
        env.provider("alpha", "ok:src/critical.rs"),
        env.provider("beta", "approve"),
        env.provider("gamma", "approve"),
    ]);
    let mut nt = NewTask::new("critical authz gate", "Implement critical authorization check");
    nt.review = true;
    nt.review_quorum = 2;
    nt.preferred_provider = Some("alpha".into());
    let t = engine.submit(nt).unwrap();

    let summary = engine.run_until_idle().await.unwrap();
    assert_eq!(summary.done, vec![t.id.clone()], "{summary:?}");

    let final_task = engine.store().get_task(&t.id).unwrap().unwrap();
    let reviewers: Vec<&str> = final_task.reviewer.as_deref().unwrap().split(',').collect();
    assert_eq!(reviewers.len(), 2, "expected 2 distinct quorum reviewers: {final_task:?}");
    assert!(reviewers.contains(&"beta") && reviewers.contains(&"gamma"));
    assert!(!reviewers.contains(&"alpha"));
}

#[test]
fn mcp_jsonrpc_executes_real_sqlite_memory_graph_and_skills_tools() {
    let env = Env::new();
    std::fs::create_dir_all(env.repo.path().join("src")).unwrap();
    std::fs::write(env.repo.path().join("src/core.rs"), "pub struct Ledger;\n").unwrap();
    git(env.repo.path(), &["add", "."]);
    git(env.repo.path(), &["commit", "-q", "-m", "add core"]);

    let init = fleet_engine::mcp::handle_jsonrpc(
        env.repo.path(),
        &serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
    )
    .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "ai-fleet-engine");

    let list = fleet_engine::mcp::handle_jsonrpc(
        env.repo.path(),
        &serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .unwrap();
    assert!(list["result"]["tools"].as_array().unwrap().len() >= 15);

    let mem_res = fleet_engine::mcp::call_tool(
        env.repo.path(),
        "fleet_memory_store",
        &serde_json::json!({ "title": "ZATCA ECDSA invariant", "content": "Always hash canonical XML with SHA-256 before secp256k1 signing" }),
    )
    .unwrap();
    assert!(mem_res["stored_id"].as_i64().unwrap() >= 1);

    let search_res = fleet_engine::mcp::call_tool(
        env.repo.path(),
        "fleet_memory_search",
        &serde_json::json!({ "query": "ZATCA secp256k1 XML signing" }),
    )
    .unwrap();
    assert!(!search_res.as_array().unwrap().is_empty());

    let graph_res = fleet_engine::mcp::call_tool(env.repo.path(), "fleet_code_graph", &serde_json::json!({})).unwrap();
    assert_eq!(graph_res["total_files"], 1);

    let queen_res = fleet_engine::mcp::call_tool(
        env.repo.path(),
        "fleet_queen_decompose",
        &serde_json::json!({ "epic": "Add double-entry journal posting and audit trail", "submit": true }),
    )
    .unwrap();
    assert!(!queen_res["submitted_task_ids"].as_array().unwrap().is_empty());
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {}
