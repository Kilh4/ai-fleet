//! The scheduler: claims tasks, routes them, supervises agent runs, commits
//! checkpoints, enforces scope, verifies, hands off on quota exhaustion,
//! gates on independent review, and merges into the integration branch.

use crate::error::{EngineError, Result};
use crate::git::{self, MergeResult};
use crate::intelligence::{self, CodeGraph};
use crate::memory::{MemoryKind, MemoryStore, NewMemory};
use crate::model::*;
use crate::proc::{run_process, run_shell, ProcSpec, StreamKind};
use crate::prompts::{self, AuthorContext, Verdict};
use crate::providers::{self, build_invocation, classify, resolve_binary, ParsedOutput, ProviderSpec, StreamParser};
use crate::queen::EpicPlan;
use crate::router::{route, Candidate, Role, Route};
use crate::skills::SkillRegistry;
use crate::store::{now_ts, truncate, Store};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub repo: PathBuf,
    pub state_dir: PathBuf,
    pub providers: Vec<ProviderSpec>,
    pub max_parallel: usize,
    pub run_timeout: Duration,
    pub idle_timeout: Duration,
    pub verify_timeout: Duration,
    pub poll_interval: Duration,
    pub integration_branch: String,
    pub auto_merge: bool,
    pub max_handoffs: u32,
    /// In `run_until_idle`, sleep until a provider's quota resets instead of returning.
    pub wait_for_cooldown: bool,
    pub max_skills_per_run: usize,
    pub skill_budget_bytes: usize,
    /// Enforce deterministic pre-merge security / IEEE-754 money / RLS static gate.
    pub static_audit_gate: bool,
}

impl EngineConfig {
    pub fn new(repo: PathBuf, providers: Vec<ProviderSpec>) -> Self {
        let state_dir = repo.join(".ai-fleet").join("engine");
        EngineConfig {
            repo,
            state_dir,
            providers,
            max_parallel: 4,
            run_timeout: Duration::from_secs(60 * 60),
            idle_timeout: Duration::from_secs(15 * 60),
            verify_timeout: Duration::from_secs(30 * 60),
            poll_interval: Duration::from_millis(500),
            integration_branch: "ai-fleet/integration".into(),
            auto_merge: true,
            max_handoffs: 12,
            wait_for_cooldown: true,
            max_skills_per_run: 3,
            skill_budget_bytes: 12_000,
            static_audit_gate: true,
        }
    }

    /// Configuration for the repository containing `dir`, with providers from
    /// `.ai-fleet/engine/providers.json` (created with defaults if missing).
    pub fn load(dir: &Path) -> Result<Self> {
        let repo = git::repo_root(dir)?;
        let mut cfg = EngineConfig::new(repo, vec![]);
        cfg.providers = providers::load_or_init(&cfg.providers_path())?;
        Ok(cfg)
    }

    pub fn providers_path(&self) -> PathBuf {
        self.state_dir.join("providers.json")
    }

    fn lease(&self) -> chrono::Duration {
        let secs = self.run_timeout.as_secs() + 2 * self.verify_timeout.as_secs() + 300;
        chrono::Duration::seconds(secs as i64)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EngineEvent {
    pub ts: String,
    pub task_id: Option<String>,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
pub struct RunSummary {
    pub runs_started: usize,
    pub done: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub blocked: Vec<(String, String)>,
    /// Still queued, with the reason no provider could take them.
    pub pending: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderReport {
    pub id: String,
    pub display_name: String,
    pub binary: String,
    pub resolved: Option<String>,
    pub enabled: bool,
    pub cooldown_until: Option<String>,
    pub cooldown_reason: Option<String>,
    pub models: Vec<(String, u8, f64, u32)>,
    pub notes: String,
}

struct Inner {
    cfg: EngineConfig,
    store: Store,
    memory: MemoryStore,
    skills: SkillRegistry,
    binaries: HashMap<String, Option<PathBuf>>,
    cancel: CancellationToken,
    merge_lock: tokio::sync::Mutex<()>,
    events: broadcast::Sender<EngineEvent>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct AgentRun {
    run_id: i64,
    outcome: RunOutcome,
    parsed: ParsedOutput,
    exit_code: Option<i32>,
    duration_ms: u64,
}

async fn blocking<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|e| EngineError::Join(e.to_string()))?
}

fn slug(title: &str) -> String {
    let mut s = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
        if s.len() >= 28 {
            break;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "task".into()
    } else {
        s
    }
}

fn new_task_id(title: &str) -> String {
    let mut h = Sha256::new();
    h.update(title.as_bytes());
    h.update(Utc::now().timestamp_nanos_opt().unwrap_or_default().to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    h.update(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_le_bytes());
    let hex = format!("{:x}", h.finalize());
    format!("{}-{}", slug(title), &hex[..6])
}

impl Engine {
    pub fn open(cfg: EngineConfig) -> Result<Self> {
        providers::validate(&cfg.providers)?;
        std::fs::create_dir_all(&cfg.state_dir)?;
        if let Ok(rel) = cfg.state_dir.strip_prefix(&cfg.repo) {
            let pattern = format!("/{}/", rel.to_string_lossy().replace('\\', "/").trim_matches('/'));
            git::ensure_excluded(&cfg.repo, &pattern)?;
        }
        let store = Store::open(&cfg.state_dir.join("engine.db"))?;
        let memory = MemoryStore::open(&cfg.state_dir.join("memory.db"))?;
        let skills = SkillRegistry::load(Some(&cfg.repo));
        let binaries = cfg.providers.iter().map(|p| (p.id.clone(), resolve_binary(&p.binary))).collect();
        let (events, _) = broadcast::channel(1024);
        Ok(Engine {
            inner: Arc::new(Inner {
                cfg,
                store,
                memory,
                skills,
                binaries,
                cancel: CancellationToken::new(),
                merge_lock: tokio::sync::Mutex::new(()),
                events,
            }),
        })
    }

    pub fn config(&self) -> &EngineConfig {
        &self.inner.cfg
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub fn memory(&self) -> &MemoryStore {
        &self.inner.memory
    }

    pub fn skills(&self) -> &SkillRegistry {
        &self.inner.skills
    }

    /// Enqueue every node of a validated Queen [`EpicPlan`], wiring `depends_on`
    /// keys to the newly assigned SQLite task IDs across waves.
    pub fn submit_plan(&self, plan: &EpicPlan, review: bool) -> Result<Vec<String>> {
        let mut key_to_task_id: HashMap<String, String> = HashMap::new();
        let mut ids = Vec::with_capacity(plan.nodes.len());
        for node in &plan.nodes {
            let deps: Vec<String> = node
                .depends_on
                .iter()
                .filter_map(|k| key_to_task_id.get(k).cloned())
                .collect();
            let nt = node.to_new_task(deps, plan.detected_verify_cmd.clone(), review);
            let submitted = self.submit(nt)?;
            key_to_task_id.insert(node.key.clone(), submitted.id.clone());
            ids.push(submitted.id);
        }
        self.inner.emit(
            None,
            "queen.plan_submitted",
            &format!("submitted {} task(s) across {} wave(s) for epic: {}", ids.len(), plan.waves, truncate(&plan.epic, 120)),
        );
        Ok(ids)
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.inner.cancel.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    pub fn submit(&self, nt: NewTask) -> Result<Task> {
        if nt.title.trim().is_empty() || nt.prompt.trim().is_empty() {
            return Err(EngineError::Config("task title and prompt must not be empty".into()));
        }
        for dep in &nt.depends_on {
            if self.inner.store.get_task(dep)?.is_none() {
                return Err(EngineError::NotFound(format!("dependency `{dep}`")));
            }
        }
        if let Some(p) = &nt.preferred_provider {
            if !self.inner.cfg.providers.iter().any(|s| &s.id == p) {
                return Err(EngineError::Config(format!("unknown provider `{p}`")));
            }
        }
        for sid in &nt.skills {
            if self.inner.skills.get(sid).is_none() {
                return Err(EngineError::NotFound(format!("skill `{sid}`")));
            }
        }
        git::scope_matcher(&nt.owned_files)?;
        let base = git::head(&self.inner.cfg.repo)?;
        let complexity = nt.complexity.unwrap_or_else(|| infer_complexity(&format!("{}\n{}", nt.title, nt.prompt)));
        let id = new_task_id(&nt.title);
        let task = self.inner.store.insert_task(&id, &nt, complexity, &base)?;
        self.inner.emit(
            Some(&id),
            "task.submitted",
            &format!("{} (complexity={}, base={})", task.title, complexity.as_str(), &base[..base.len().min(10)]),
        );
        Ok(task)
    }

    pub fn retry(&self, id: &str) -> Result<Task> {
        let mut t = self.inner.store.get_task(id)?.ok_or_else(|| EngineError::NotFound(id.into()))?;
        t.status = if t.head_commit.is_some() && t.review_required && t.last_failure.is_none() {
            TaskStatus::AwaitingReview
        } else {
            TaskStatus::Queued
        };
        t.max_attempts = t.max_attempts.max(t.attempts + 2);
        t.handoffs = 0;
        t.lease_epoch += 1;
        t.lease_expires_at = None;
        self.inner.store.save_task(&t)?;
        self.inner.emit(Some(id), "task.retry", "re-queued by user");
        Ok(t)
    }

    pub fn cancel_task(&self, id: &str) -> Result<Task> {
        let mut t = self.inner.store.get_task(id)?.ok_or_else(|| EngineError::NotFound(id.into()))?;
        t.status = TaskStatus::Cancelled;
        t.lease_epoch += 1; // fences out any run still in flight
        t.lease_expires_at = None;
        self.inner.store.save_task(&t)?;
        self.inner.emit(Some(id), "task.cancelled", "cancelled by user");
        Ok(t)
    }

    pub fn clear_cooldown(&self, provider: &str) -> Result<()> {
        self.inner.store.clear_cooldown(provider)?;
        self.inner.emit(None, "provider.ready", &format!("{provider}: cooldown cleared by user"));
        Ok(())
    }

    pub fn provider_reports(&self) -> Result<Vec<ProviderReport>> {
        let cds = self.inner.store.active_cooldowns(Utc::now())?;
        let stats = self.inner.store.stats()?;
        Ok(self
            .inner
            .cfg
            .providers
            .iter()
            .map(|p| ProviderReport {
                id: p.id.clone(),
                display_name: p.display_name.clone(),
                binary: p.binary.clone(),
                resolved: self.inner.binaries.get(&p.id).cloned().flatten().map(|b| b.display().to_string()),
                enabled: p.enabled,
                cooldown_until: cds.get(&p.id).map(|c| crate::store::ts(c.until)),
                cooldown_reason: cds.get(&p.id).map(|c| c.status.clone()),
                models: p
                    .models
                    .iter()
                    .map(|m| {
                        let st = stats.get(&(p.id.clone(), m.name.clone())).cloned().unwrap_or_default();
                        let n = st.successes + st.failures;
                        let rate = if n == 0 { f64::NAN } else { st.successes as f64 / n as f64 };
                        (m.name.clone(), m.tier, rate, n)
                    })
                    .collect(),
                notes: p.notes.clone(),
            })
            .collect())
    }

    /// Process work until nothing more can be done right now.
    pub async fn run_until_idle(&self) -> Result<RunSummary> {
        self.run(true).await
    }

    /// Keep processing (and picking up newly submitted tasks) until cancelled.
    pub async fn run_forever(&self) -> Result<RunSummary> {
        self.run(false).await
    }

    async fn run(&self, until_idle: bool) -> Result<RunSummary> {
        let inner = self.inner.clone();
        let mut set: JoinSet<()> = JoinSet::new();
        let mut running: HashMap<tokio::task::Id, String> = HashMap::new();
        let mut inflight: HashMap<String, usize> = HashMap::new();
        let mut waiting: BTreeMap<String, String> = BTreeMap::new();
        let mut summary = RunSummary::default();

        loop {
            if inner.cancel.is_cancelled() {
                break;
            }
            let now = Utc::now();
            for id in inner.store.recover_expired_leases(now)? {
                inner.emit(Some(&id), "task.recovered", "lease expired (worker died); re-queued with its committed work");
            }
            inner.block_failed_dependents()?;
            let cooldowns = inner.store.active_cooldowns(now)?;
            let stats = inner.store.stats()?;
            let tasks = inner.store.list_tasks()?;
            let done: std::collections::HashSet<&str> =
                tasks.iter().filter(|t| t.status == TaskStatus::Done).map(|t| t.id.as_str()).collect();

            for task in tasks.iter().filter(|t| match t.status {
                TaskStatus::Queued => t.depends_on.iter().all(|d| done.contains(d.as_str())),
                TaskStatus::AwaitingReview => true,
                _ => false,
            }) {
                if set.len() >= inner.cfg.max_parallel.max(1) {
                    break;
                }
                let role = if task.status == TaskStatus::AwaitingReview { Role::Reviewer } else { Role::Author };
                let cands: Vec<Candidate<'_>> = inner
                    .cfg
                    .providers
                    .iter()
                    .map(|p| Candidate {
                        spec: p,
                        binary_ok: inner.binaries.get(&p.id).is_some_and(|b| b.is_some()),
                        cooldown_until: cooldowns.get(&p.id).map(|c| c.until),
                        inflight: inflight.get(&p.id).copied().unwrap_or(0),
                    })
                    .collect();
                match route(task, role, &cands, &stats, now) {
                    Err(why) => {
                        if waiting.get(&task.id) != Some(&why) {
                            inner.emit(Some(&task.id), "task.waiting", &format!("no {} available: {why}", role.as_str()));
                            waiting.insert(task.id.clone(), why);
                        }
                    }
                    Ok(r) => {
                        let (from, to) = match role {
                            Role::Author => (TaskStatus::Queued, TaskStatus::Running),
                            Role::Reviewer => (TaskStatus::AwaitingReview, TaskStatus::Reviewing),
                        };
                        let Some(epoch) = inner.store.claim(&task.id, from, to, inner.cfg.lease())? else { continue };
                        waiting.remove(&task.id);
                        let fresh = inner.store.get_task(&task.id)?.unwrap_or_else(|| task.clone());
                        *inflight.entry(r.provider.clone()).or_default() += 1;
                        summary.runs_started += 1;
                        let provider = r.provider.clone();
                        let worker = inner.clone();
                        let handle = set.spawn(async move { worker.execute(fresh, role, r, epoch).await });
                        running.insert(handle.id(), provider);
                    }
                }
            }

            if set.is_empty() {
                if until_idle {
                    let pending = inner.store.list_tasks()?.iter().filter(|t| t.status.is_pending()).count();
                    if pending == 0 {
                        break;
                    }
                    let earliest = cooldowns.values().map(|c| c.until).min();
                    match earliest {
                        Some(t) if inner.cfg.wait_for_cooldown => {
                            let wait = (t - Utc::now()).to_std().unwrap_or_default() + Duration::from_secs(1);
                            inner.emit(None, "engine.sleeping", &format!("all eligible providers are cooling down; sleeping {}s", wait.as_secs()));
                            tokio::select! {
                                _ = tokio::time::sleep(wait) => {}
                                _ = inner.cancel.cancelled() => {}
                            }
                            continue;
                        }
                        _ => break,
                    }
                }
                tokio::select! {
                    _ = tokio::time::sleep(inner.cfg.poll_interval) => {}
                    _ = inner.cancel.cancelled() => {}
                }
                continue;
            }

            tokio::select! {
                Some(res) = set.join_next_with_id() => {
                    let id = match &res { Ok((id, _)) => *id, Err(e) => e.id() };
                    if let Err(e) = &res {
                        inner.emit(None, "engine.worker_panic", &e.to_string());
                    }
                    if let Some(p) = running.remove(&id) {
                        if let Some(n) = inflight.get_mut(&p) { *n = n.saturating_sub(1); }
                    }
                }
                _ = tokio::time::sleep(inner.cfg.poll_interval) => {}
                _ = inner.cancel.cancelled() => {}
            }
        }

        while let Some(res) = set.join_next().await {
            if let Err(e) = res {
                inner.emit(None, "engine.worker_panic", &e.to_string());
            }
        }

        for t in inner.store.list_tasks()? {
            let err = t.last_error.clone().unwrap_or_default();
            match t.status {
                TaskStatus::Done => summary.done.push(t.id),
                TaskStatus::Failed => summary.failed.push((t.id, err)),
                TaskStatus::Blocked => summary.blocked.push((t.id, err)),
                TaskStatus::Queued | TaskStatus::AwaitingReview | TaskStatus::Running | TaskStatus::Reviewing => {
                    let why = waiting.get(&t.id).cloned().unwrap_or_else(|| format!("status {}", t.status));
                    summary.pending.push((t.id, why));
                }
                TaskStatus::Cancelled => {}
            }
        }
        Ok(summary)
    }
}

impl Inner {
    fn emit(&self, task_id: Option<&str>, kind: &str, message: &str) {
        let _ = self.store.append_event(task_id, kind, message);
        let _ = self.events.send(EngineEvent {
            ts: now_ts(),
            task_id: task_id.map(str::to_string),
            kind: kind.to_string(),
            message: message.to_string(),
        });
    }

    fn spec(&self, id: &str) -> Result<&ProviderSpec> {
        self.cfg.providers.iter().find(|p| p.id == id).ok_or_else(|| EngineError::NotFound(format!("provider `{id}`")))
    }

    fn worktree_path(&self, task_id: &str) -> PathBuf {
        self.cfg.state_dir.join("worktrees").join(task_id)
    }

    fn select_and_render_skills(&self, task: &Task, role: Role) -> String {
        if !task.auto_skills && task.skills.is_empty() {
            return String::new();
        }
        let limit = if task.auto_skills {
            self.cfg.max_skills_per_run.max(task.skills.len())
        } else {
            task.skills.len()
        };
        if limit == 0 {
            return String::new();
        }
        let query = if task.auto_skills {
            format!("{}\n{}", task.title, task.prompt)
        } else {
            String::new()
        };
        let matched = self.skills.select(&query, &task.owned_files, role, &task.skills, limit);
        if !matched.is_empty() {
            let summary: Vec<String> = matched.iter().map(|m| format!("{}({:.1})", m.id, m.score)).collect();
            self.emit(
                Some(&task.id),
                "skills.activated",
                &format!("{} injected {} skill(s): {}", role.as_str(), matched.len(), summary.join(", ")),
            );
        }
        self.skills.render_prompt_section(&matched, self.cfg.skill_budget_bytes)
    }

    fn block_failed_dependents(&self) -> Result<()> {
        let tasks = self.store.list_tasks()?;
        let by_id: HashMap<&str, &Task> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
        for t in tasks.iter().filter(|t| t.status == TaskStatus::Queued) {
            let bad = t.depends_on.iter().find(|d| {
                by_id.get(d.as_str()).map_or(true, |dep| {
                    matches!(dep.status, TaskStatus::Failed | TaskStatus::Blocked | TaskStatus::Cancelled)
                })
            });
            if let Some(dep) = bad {
                let mut b = (*t).clone();
                b.status = TaskStatus::Blocked;
                b.last_error = Some(format!("dependency `{dep}` did not complete"));
                if self.store.save_task_fenced(&b, t.lease_epoch)? {
                    self.emit(Some(&t.id), "task.blocked", &format!("dependency `{dep}` did not complete"));
                }
            }
        }
        Ok(())
    }

    async fn execute(self: Arc<Self>, task: Task, role: Role, route: Route, epoch: i64) {
        let id = task.id.clone();
        let res = match role {
            Role::Author => self.clone().run_author(task, route, epoch).await,
            Role::Reviewer => self.clone().run_review(task, route, epoch).await,
        };
        if let Err(e) = res {
            self.emit(Some(&id), "task.internal_error", &e.to_string());
            if let Ok(Some(mut t)) = self.store.get_task(&id) {
                if t.lease_epoch == epoch {
                    t.last_failure = Some(FailureKind::Internal.as_str().into());
                    t.last_error = Some(truncate(&e.to_string(), 4000));
                    t.attempts += 1;
                    t.lease_expires_at = None;
                    t.status = if t.attempts >= t.max_attempts {
                        TaskStatus::Failed
                    } else if role == Role::Reviewer {
                        TaskStatus::AwaitingReview
                    } else {
                        TaskStatus::Queued
                    };
                    let _ = self.store.save_task_fenced(&t, epoch);
                }
            }
        }
    }

    /// Launch the provider's CLI and supervise it to completion.
    async fn run_agent(&self, task: &Task, role: Role, route: &Route, wt: &Path, prompt: String, commit_before: &str) -> Result<AgentRun> {
        let spec = self.spec(&route.provider)?.clone();
        let attempt = task.attempts + task.handoffs + 1;
        let safe_model: String = route.model.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
        let log_path = self
            .cfg
            .state_dir
            .join("logs")
            .join(&task.id)
            .join(format!("{:02}-{}-{}-{}.log", attempt, role.as_str(), route.provider, safe_model));
        let run_id = self.store.insert_run(
            &task.id,
            attempt,
            role.as_str(),
            &route.provider,
            &route.model,
            route.effort.as_str(),
            &route.reason,
            &log_path.to_string_lossy(),
            commit_before,
        )?;
        self.emit(
            Some(&task.id),
            "run.started",
            &format!("{} via {}/{} effort={} (attempt {attempt})", role.as_str(), route.provider, route.model, route.effort),
        );

        let started = Instant::now();
        let binary = self.binaries.get(&spec.id).cloned().flatten();
        let (outcome, parsed, exit_code) = match binary {
            None => (
                RunOutcome::Failure {
                    kind: FailureKind::Unavailable,
                    detail: format!("`{}` not found on PATH", spec.binary),
                    retry_at: Some(Utc::now() + chrono::Duration::minutes(30)),
                },
                ParsedOutput::default(),
                None,
            ),
            Some(bin) => {
                let inv = build_invocation(&spec, &bin, &route.model, route.effort, &prompt, wt);
                if let Some(parent) = log_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(
                    &log_path,
                    format!("# {} {}\n# prompt ({} bytes) follows\n{}\n# ---- output ----\n", inv.program.display(), inv.args.join(" "), prompt.len(), prompt),
                )?;
                let mut parser = StreamParser::new(spec.output);
                let ps = ProcSpec {
                    program: inv.program,
                    args: inv.args,
                    cwd: wt.to_path_buf(),
                    env: inv.env,
                    stdin: inv.stdin,
                    timeout: self.cfg.run_timeout,
                    idle_timeout: self.cfg.idle_timeout,
                    log_path: Some(log_path.clone()),
                };
                let res = run_process(
                    ps,
                    |kind, line| {
                        if kind == StreamKind::Stdout {
                            parser.feed(line)
                        }
                    },
                    self.cancel.child_token(),
                )
                .await;
                match res {
                    Ok(pr) => {
                        if pr.lingering_processes > 0 {
                            self.emit(
                                Some(&task.id),
                                "run.process_tree_killed",
                                &format!("{} process(es) outlived the agent and were terminated", pr.lingering_processes),
                            );
                        }
                        let parsed = parser.finish(&pr);
                        let outcome = classify(&spec, &pr, &parsed, Utc::now());
                        (outcome, parsed, pr.exit_code)
                    }
                    Err(e) => (
                        RunOutcome::Failure {
                            kind: FailureKind::Unavailable,
                            detail: format!("failed to launch {}: {e}", bin.display()),
                            retry_at: Some(Utc::now() + chrono::Duration::minutes(30)),
                        },
                        ParsedOutput::default(),
                        None,
                    ),
                }
            }
        };
        Ok(AgentRun { run_id, outcome, parsed, exit_code, duration_ms: started.elapsed().as_millis() as u64 })
    }

    async fn verify(&self, cmd: &str, cwd: &Path, log: PathBuf) -> Result<(bool, String)> {
        let r = run_shell(cmd, cwd, self.cfg.verify_timeout, Some(log), self.cancel.child_token()).await?;
        let tail = truncate(&format!("{}\n{}", r.stdout_tail.trim(), r.stderr_tail.trim()), 6000);
        let ok = r.exit_code == Some(0) && !r.timed_out && !r.cancelled;
        let tail = if r.timed_out { format!("verification timed out\n{tail}") } else { tail };
        Ok((ok, tail))
    }

    /// Record a failure on `t` and decide where it goes next.
    fn apply_failure(&self, t: &mut Task, provider: &str, kind: FailureKind, detail: &str, retry_at: Option<chrono::DateTime<Utc>>, requeue: TaskStatus, counts_attempt: bool) -> Result<()> {
        t.last_failure = Some(kind.as_str().into());
        t.last_error = Some(truncate(detail, 4000));
        if kind.is_provider_fault() {
            let spec_cooldown = self.spec(provider).map(|s| s.default_cooldown_secs).unwrap_or(3600);
            let until = retry_at.unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(spec_cooldown as i64));
            self.store.set_cooldown(provider, until, kind.as_str(), detail)?;
            self.emit(
                Some(&t.id),
                "provider.cooldown",
                &format!("{provider} parked until {} ({kind}); handing the task to another provider", until.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S")),
            );
            t.handoffs += 1;
            t.status = if t.handoffs > self.cfg.max_handoffs { TaskStatus::Failed } else { requeue };
        } else if kind == FailureKind::Cancelled {
            t.status = requeue;
        } else if counts_attempt {
            t.attempts += 1;
            t.status = if t.attempts >= t.max_attempts { TaskStatus::Failed } else { requeue };
        } else {
            t.handoffs += 1;
            t.status = if t.handoffs > self.cfg.max_handoffs { TaskStatus::Failed } else { requeue };
        }
        Ok(())
    }

    async fn run_author(self: Arc<Self>, task: Task, route: Route, epoch: i64) -> Result<()> {
        let branch = format!("ai-fleet/{}", task.id);
        let wt = self.worktree_path(&task.id);
        let base = task.base_commit.clone().ok_or_else(|| EngineError::Config(format!("task {} has no base commit", task.id)))?;

        let pre = {
            let (repo, wt2, br, b) = (self.cfg.repo.clone(), wt.clone(), branch.clone(), base.clone());
            blocking(move || {
                git::ensure_worktree(&repo, &wt2, &br, &b)?;
                // Anything left uncommitted by a crashed run is preserved, not discarded.
                if let Some(c) = git::checkpoint(&wt2, "ai-fleet: preserve uncommitted work from an interrupted run")? {
                    return Ok(c);
                }
                git::head(&wt2)
            })
            .await?
        };

        let (prior_commits, prior_stat, graph_section) = {
            let (wt2, b, p, focus) = (wt.clone(), base.clone(), pre.clone(), task.owned_files.clone());
            blocking(move || {
                let commits = git::log_oneline(&wt2, &b, &p)?;
                let stat = git::diff_stat(&wt2, &b, &p)?;
                let gmap = CodeGraph::scan(&wt2, 350)
                    .map(|g| g.render_prompt_map(&focus, 2800))
                    .unwrap_or_default();
                Ok((commits, stat, gmap))
            })
            .await?
        };
        let mem_query = format!("{}\n{}", task.title, task.prompt);
        let mem_section = self
            .memory
            .search(&mem_query, &task.owned_files, 4)
            .map(|hits| self.memory.render_prompt_section(&hits, 3000))
            .unwrap_or_default();
        let skills_section = self.select_and_render_skills(&task, Role::Author);
        let prompt = prompts::author_prompt(
            &task,
            &AuthorContext {
                branch: &branch,
                prior_commits: &prior_commits,
                prior_diffstat: &prior_stat,
                graph_section: &graph_section,
                memory_section: &mem_section,
                skills_section: &skills_section,
            },
        );

        let run = self.run_agent(&task, Role::Author, &route, &wt, prompt, &pre).await?;
        let mut outcome = run.outcome.clone();

        // Checkpoint whatever the agent produced, success or not: this commit
        // is what the next provider continues from after a quota handoff.
        let msg = format!(
            "ai-fleet({}): attempt by {}/{} effort={} -> {}",
            task.id,
            route.provider,
            route.model,
            route.effort,
            outcome.label()
        );
        let mut after = {
            let (wt2, p) = (wt.clone(), pre.clone());
            blocking(move || Ok(git::checkpoint(&wt2, &msg)?.unwrap_or(p))).await?
        };

        if !task.owned_files.is_empty() && after != pre {
            let (wt2, p, a, owned) = (wt.clone(), pre.clone(), after.clone(), task.owned_files.clone());
            let violations = blocking(move || git::scope_violations(&owned, &git::changed_files(&wt2, &p, &a)?)).await?;
            if !violations.is_empty() {
                let (wt2, p) = (wt.clone(), pre.clone());
                blocking(move || git::reset_hard(&wt2, &p)).await?;
                after = pre.clone();
                outcome = RunOutcome::failure(
                    FailureKind::ScopeViolation,
                    format!(
                        "modified files outside the allowed scope ({}): {}. The attempt was rolled back.",
                        task.owned_files.join(", "),
                        violations.join(", ")
                    ),
                );
            }
        }

        if outcome.is_success() && self.cfg.static_audit_gate && after != pre {
            let (wt2, p, a) = (wt.clone(), pre.clone(), after.clone());
            let audit = blocking(move || intelligence::audit_changes(&wt2, &p, &a)).await?;
            if !audit.passed {
                let details: Vec<String> = audit
                    .findings
                    .iter()
                    .map(|f| format!("{}:{} [{}] {}", f.file, f.line, f.rule, f.message))
                    .collect();
                self.emit(
                    Some(&task.id),
                    "audit.blocked",
                    &format!("pre-merge static gate caught {} issue(s)", audit.findings.len()),
                );
                outcome = RunOutcome::failure(
                    FailureKind::StaticAuditFailed,
                    format!("Static Security/Money/RLS Gate failed:\n{}", details.join("\n")),
                );
            }
        }

        if outcome.is_success() {
            if let Some(cmd) = &task.verify_cmd {
                let log = self.cfg.state_dir.join("logs").join(&task.id).join(format!("verify-{}.log", run.run_id));
                let (ok, tail) = self.verify(cmd, &wt, log).await?;
                self.emit(Some(&task.id), "verify", &format!("`{cmd}` -> {}", if ok { "passed" } else { "FAILED" }));
                if !ok {
                    outcome = RunOutcome::failure(FailureKind::VerifyFailed, format!("`{cmd}` failed:\n{tail}"));
                }
            }
        }

        // Synthesize an automatic Reflexion Lesson whenever a prior failure was overcome!
        if outcome.is_success() && task.attempts > 0 {
            if let Some(prev_kind) = task.last_failure.as_deref() {
                let prev_err = truncate(task.last_error.as_deref().unwrap_or(""), 500);
                let fix_summary = truncate(&run.parsed.final_text, 500);
                let _ = self.memory.store(NewMemory {
                    kind: MemoryKind::Reflexion,
                    title: format!("Resolved `{prev_kind}` on {}", task.title),
                    content: format!("Earlier attempt failed ({prev_kind}): {prev_err} | Fixed by: {fix_summary}"),
                    files: task.owned_files.clone(),
                    tags: vec![prev_kind.to_string(), "reflexion".into()],
                    task_id: Some(task.id.clone()),
                    importance: 2.2,
                });
                self.emit(Some(&task.id), "memory.reflexion_saved", &format!("recorded reflexion lesson for overcoming `{prev_kind}`"));
            }
        }

        self.store.finish_run(run.run_id, &outcome, run.exit_code, run.duration_ms, run.parsed.session_id.as_deref(), &after)?;
        self.emit(Some(&task.id), "run.finished", &format!("{} via {}/{} -> {}", "author", route.provider, route.model, outcome.label()));

        let Some(mut t) = self.store.get_task(&task.id)? else { return Ok(()) };
        if t.lease_epoch != epoch {
            self.emit(Some(&task.id), "run.discarded", "task was re-leased or cancelled while this run was in flight; result ignored");
            return Ok(());
        }
        t.branch = Some(branch);
        t.worktree = Some(wt.to_string_lossy().to_string());
        t.head_commit = Some(after.clone());
        t.last_provider = Some(route.provider.clone());
        t.last_model = Some(route.model.clone());
        t.last_effort = Some(route.effort.as_str().into());
        if (after != pre || outcome.is_success()) && !t.authors.contains(&route.provider) {
            t.authors.push(route.provider.clone());
        }

        let mut integrate = false;
        match &outcome {
            RunOutcome::Success => {
                t.result_summary = Some(truncate(&run.parsed.final_text, 6000));
                t.last_failure = None;
                t.last_error = None;
                t.review_feedback = None;
                t.reviewer = None; // fresh commit requires fresh quorum approvals
                if t.review_required {
                    t.status = TaskStatus::AwaitingReview;
                    t.lease_expires_at = None;
                } else {
                    integrate = true; // stays `running` with active lease until merged
                }
            }
            RunOutcome::Failure { kind, detail, retry_at } => {
                t.lease_expires_at = None;
                self.apply_failure(&mut t, &route.provider, *kind, detail, *retry_at, TaskStatus::Queued, true)?;
                if t.status == TaskStatus::Failed {
                    self.emit(Some(&t.id), "task.failed", &format!("giving up after {} attempts / {} handoffs: {kind}", t.attempts, t.handoffs));
                }
            }
        }
        if !self.store.save_task_fenced(&t, epoch)? {
            self.emit(Some(&task.id), "run.discarded", "lost the lease before saving; result ignored");
            return Ok(());
        }
        if integrate {
            self.integrate(t, epoch).await?;
        }
        Ok(())
    }

    async fn run_review(self: Arc<Self>, task: Task, route: Route, epoch: i64) -> Result<()> {
        let wt = PathBuf::from(task.worktree.clone().ok_or_else(|| EngineError::Config(format!("task {} has no worktree to review", task.id)))?);
        let head = task.head_commit.clone().ok_or_else(|| EngineError::Config(format!("task {} has no commit to review", task.id)))?;
        let base = task.base_commit.clone().unwrap_or_else(|| head.clone());

        let (stat, diff) = {
            let (wt2, b, h) = (wt.clone(), base.clone(), head.clone());
            blocking(move || Ok((git::diff_stat(&wt2, &b, &h)?, git::diff(&wt2, &b, &h, 80_000)?))).await?
        };
        let mem_section = self
            .memory
            .search(&format!("{}\n{}", task.title, task.prompt), &task.owned_files, 3)
            .map(|hits| self.memory.render_prompt_section(&hits, 2400))
            .unwrap_or_default();
        let skills_section = self.select_and_render_skills(&task, Role::Reviewer);
        let prompt = prompts::review_prompt(
            &task,
            &stat,
            &diff,
            task.result_summary.as_deref().unwrap_or(""),
            &mem_section,
            &skills_section,
        );
        let run = self.run_agent(&task, Role::Reviewer, &route, &wt, prompt, &head).await?;

        // Reviewers are read-only: discard anything they touched.
        let tampered = {
            let (wt2, h) = (wt.clone(), head.clone());
            blocking(move || {
                let dirty = !git::status_porcelain(&wt2)?.is_empty() || git::head(&wt2)? != h;
                if dirty {
                    git::reset_hard(&wt2, &h)?;
                }
                Ok(dirty)
            })
            .await?
        };
        if tampered {
            self.emit(Some(&task.id), "review.tamper_reverted", &format!("{} modified the worktree during review; changes discarded", route.provider));
        }

        let verdict = prompts::parse_verdict(&run.parsed.final_text);
        let mut outcome = run.outcome.clone();
        if outcome.is_success() && verdict.is_none() {
            outcome = RunOutcome::failure(FailureKind::AgentError, "reviewer did not end with a `VERDICT:` line");
        }
        self.store.finish_run(run.run_id, &outcome, run.exit_code, run.duration_ms, run.parsed.session_id.as_deref(), &head)?;

        let Some(mut t) = self.store.get_task(&task.id)? else { return Ok(()) };
        if t.lease_epoch != epoch {
            self.emit(Some(&task.id), "run.discarded", "task was re-leased or cancelled during review; verdict ignored");
            return Ok(());
        }
        let mut integrate = false;
        match (&outcome, verdict) {
            (RunOutcome::Success, Some(Verdict::Approve)) => {
                let mut approvers: Vec<String> = t
                    .reviewer
                    .as_deref()
                    .unwrap_or("")
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if !approvers.iter().any(|p| p == &route.provider) {
                    approvers.push(route.provider.clone());
                }
                t.reviewer = Some(approvers.join(","));
                let required = t.review_quorum.max(1) as usize;
                if approvers.len() < required {
                    t.status = TaskStatus::AwaitingReview;
                    t.lease_expires_at = None;
                    self.emit(
                        Some(&t.id),
                        "review.quorum_progress",
                        &format!("approved by {}/{} ({}/{required} quorum votes)", route.provider, route.model, approvers.len()),
                    );
                } else {
                    self.emit(
                        Some(&t.id),
                        "review.approved",
                        &format!("approved by [{}] ({}/{required} quorum reached)", approvers.join(", "), approvers.len()),
                    );
                    integrate = true; // keep lease_expires_at intact while merging
                }
            }
            (RunOutcome::Success, _) => {
                t.lease_expires_at = None;
                t.review_feedback = Some(truncate(&run.parsed.final_text, 6000));
                t.reviewer = None;
                self.emit(Some(&t.id), "review.changes_requested", &format!("changes requested by {}/{}", route.provider, route.model));
                self.apply_failure(&mut t, &route.provider, FailureKind::ReviewRejected, &format!("changes requested by {}", route.provider), None, TaskStatus::Queued, true)?;
            }
            (RunOutcome::Failure { kind, detail, retry_at }, _) => {
                t.lease_expires_at = None;
                self.apply_failure(&mut t, &route.provider, *kind, detail, *retry_at, TaskStatus::AwaitingReview, false)?;
            }
        }
        if !self.store.save_task_fenced(&t, epoch)? {
            return Ok(());
        }
        if integrate {
            self.integrate(t, epoch).await?;
        }
        Ok(())
    }

    /// Merge an accepted branch into the integration branch, re-verifying the
    /// merged result. Serialized so merges never race.
    async fn integrate(&self, mut t: Task, epoch: i64) -> Result<()> {
        if !self.cfg.auto_merge {
            t.status = TaskStatus::Done;
            t.lease_expires_at = None;
            self.store.save_task_fenced(&t, epoch)?;
            self.emit(Some(&t.id), "task.done", &format!("accepted on branch {} (auto-merge disabled)", t.branch.clone().unwrap_or_default()));
            return Ok(());
        }
        let _guard = self.merge_lock.lock().await;
        let int_wt = self.worktree_path("_integration");
        let branch = t.branch.clone().ok_or_else(|| EngineError::Config("task has no branch".into()))?;
        let base = t.base_commit.clone().unwrap_or_default();
        let int_branch = self.cfg.integration_branch.clone();

        let (before, merge) = {
            let (repo, iw, ib, b, br, id) = (self.cfg.repo.clone(), int_wt.clone(), int_branch.clone(), base.clone(), branch.clone(), t.id.clone());
            blocking(move || {
                git::ensure_worktree(&repo, &iw, &ib, &b)?;
                let before = git::head(&iw)?;
                let res = git::merge_into(&iw, &br, &format!("ai-fleet: merge task {id}"))?;
                Ok((before, res))
            })
            .await?
        };

        match merge {
            MergeResult::Conflict(files) => {
                let detail = format!(
                    "merging `{branch}` into `{int_branch}` conflicts in: {}. In your worktree run `git merge {int_branch}`, resolve every conflict, and make sure verification passes.",
                    files.join(", ")
                );
                self.emit(Some(&t.id), "merge.conflict", &detail);
                let lp = t.last_provider.clone().unwrap_or_default();
                self.apply_failure(&mut t, &lp, FailureKind::MergeConflict, &detail, None, TaskStatus::Queued, true)?;
            }
            MergeResult::Merged(_) | MergeResult::UpToDate => {
                let mut ok = true;
                if let Some(cmd) = t.verify_cmd.clone() {
                    let log = self.cfg.state_dir.join("logs").join(&t.id).join("verify-integration.log");
                    let (passed, tail) = self.verify(&cmd, &int_wt, log).await?;
                    if !passed {
                        ok = false;
                        let (iw, b) = (int_wt.clone(), before.clone());
                        blocking(move || git::reset_hard(&iw, &b)).await?;
                        let detail = format!(
                            "after merging into `{int_branch}`, `{cmd}` failed (the merge was reverted). Run `git merge {int_branch}` in your worktree and fix:\n{tail}"
                        );
                        self.emit(Some(&t.id), "merge.verify_failed", "post-merge verification failed; merge reverted");
                        let lp = t.last_provider.clone().unwrap_or_default();
                        self.apply_failure(&mut t, &lp, FailureKind::VerifyFailed, &detail, None, TaskStatus::Queued, true)?;
                    }
                }
                if ok {
                    t.status = TaskStatus::Done;
                    let _ = self.memory.store(NewMemory {
                        kind: MemoryKind::Episode,
                        title: format!("Completed: {}", t.title),
                        content: truncate(t.result_summary.as_deref().unwrap_or(&t.prompt), 900),
                        files: t.owned_files.clone(),
                        tags: vec!["episode".into(), t.complexity.as_str().into()],
                        task_id: Some(t.id.clone()),
                        importance: 1.0,
                    });
                    self.emit(Some(&t.id), "task.done", &format!("merged into {int_branch}"));
                }
            }
        }
        t.lease_expires_at = None;
        self.store.save_task_fenced(&t, epoch)?;
        Ok(())
    }
}
