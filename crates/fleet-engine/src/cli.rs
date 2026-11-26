//! `ai-fleet` primary CLI — connected directly to SQLite `engine.db`, `memory.db`,
//! `axum` HTTP+SSE server, `ratatui` interactive TUI, MCP server, and 120 embedded skills.

use crate::engine::{Engine, EngineConfig};
use crate::git;
use crate::intelligence::{audit_file_list, CodeGraph};
use crate::mcp;
use crate::memory::{MemoryKind, NewMemory};
use crate::model::{Complexity, NewTask, Task};
use crate::providers::{default_providers, resolve_binary};
use crate::router::Role;
use crate::skills::SkillRegistry;
use crate::tui;
use crate::web;
use clap::{Args, Parser, Subcommand};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "ai-fleet",
    version,
    about = "Real multi-AI orchestration engine (SQLite WAL queue, Git worktree handoffs, independent review, axum SSE, ratatui TUI, 120 skills)"
)]
struct Cli {
    /// Repository to operate on (defaults to the git repo containing the current directory).
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Real-time status from SQLite (`engine.db`, `memory.db`, providers, event hash chain).
    Status,
    /// Interactive full-screen Terminal UI (`ratatui` + `crossterm`).
    Tui,
    /// Queue a task.
    Submit(TaskArgs),
    /// Queue a task and run the engine until there is nothing left to do.
    Do {
        #[command(flatten)]
        task: TaskArgs,
        #[command(flatten)]
        run: RunArgs,
    },
    /// Process the queue.
    Run(RunArgs),
    /// List tasks.
    List,
    /// Show one task with all of its runs.
    Show { id: String },
    /// Recent runs across all tasks.
    Runs {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Provider health: binaries, cooldowns, measured success rates.
    Providers,
    /// Clear a provider's cooldown (e.g. after you upgraded your plan).
    Ready { provider: String },
    /// Re-queue a failed/blocked task.
    Retry { id: String },
    /// Cancel a task (fences out any in-flight run).
    Cancel { id: String },
    /// Event log; `--verify` checks the hash chain.
    Log {
        #[arg(long)]
        verify: bool,
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
    /// Write the default providers.json (if missing) and print its path.
    Init,
    /// Inspect, search, and test the 120+ Elite Agent Skills pack.
    Skills {
        #[command(subcommand)]
        cmd: SkillsCmd,
    },
    /// Decompose a high-level epic into a cycle-free, file-disjoint multi-wave DAG (Live AI by default, `--offline` for CodeGraph fallback).
    Queen {
        epic: String,
        /// Use deterministic CodeGraph + SkillRegistry decomposition without invoking a live AI CLI.
        #[arg(long)]
        offline: bool,
        /// Enqueue all planned waves into SQLite.
        #[arg(long)]
        submit: bool,
        /// Enqueue and immediately execute all waves to completion.
        #[arg(long)]
        execute: bool,
        #[arg(long)]
        verify: Option<String>,
        #[arg(long)]
        no_review: bool,
        #[command(flatten)]
        run: RunArgs,
    },
    /// Persistent Hybrid FTS5 + 128-D Feature-Hashed Vector + Reflexion Memory.
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// Multi-language Codebase Knowledge Graph (PageRank hubs, blast radius, circular SCCs).
    Graph {
        /// Compute transitive blast radius for these modified files.
        #[arg(long = "blast")]
        blast: Vec<String>,
        #[arg(long, default_value_t = 15)]
        top: usize,
    },
    /// Deterministic pre-merge Security / IEEE-754 Money / PostgreSQL RLS static gate.
    Audit {
        files: Vec<String>,
    },
    /// Start the real 15-tool JSON-RPC 2.0 MCP server over stdio.
    Mcp,
    /// Start the live SQLite-backed `axum` HTTP + SSE Web Console.
    Console {
        #[arg(long, default_value = "127.0.0.1:7878")]
        bind: String,
    },
}

#[derive(Subcommand, Clone)]
enum MemoryCmd {
    /// Store an architectural rule, fact, or lesson.
    Remember {
        title: String,
        content: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "file")]
        files: Vec<String>,
    },
    /// Hybrid FTS5 BM25 + 128-D vector + 1-hop NSW graph search.
    Search {
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// List recent episodes, reflexion lessons, and knowledge entries.
    Recent {
        #[arg(long, default_value_t = 15)]
        limit: usize,
    },
}

#[derive(Subcommand, Clone)]
enum SkillsCmd {
    /// List installed skills (all 120 bundled Elite Skills + any project skills).
    List {
        /// Optional domain filter (e.g. accounting-erp, mathematics, physics-simulation, economics-quant).
        #[arg(long)]
        domain: Option<String>,
    },
    /// Test Okapi BM25 + bilingual Arabic/English skill matching on a prompt.
    Match {
        query: String,
        #[arg(long = "owned")]
        owned: Vec<String>,
        #[arg(long, default_value_t = 5)]
        top: usize,
        #[arg(long)]
        reviewer: bool,
    },
    /// Show full metadata and instructions of a skill.
    Show { id: String },
}

#[derive(Args, Clone)]
struct TaskArgs {
    /// What the AI should do.
    prompt: String,
    #[arg(long)]
    title: Option<String>,
    /// Glob of files the task may modify (repeatable). Default: unrestricted.
    #[arg(long = "owned")]
    owned: Vec<String>,
    /// Command that must exit 0 for the work to be accepted (e.g. "cargo test").
    #[arg(long)]
    verify: Option<String>,
    /// trivial | standard | deep | critical (inferred when omitted).
    #[arg(long)]
    complexity: Option<String>,
    #[arg(long, default_value_t = 0)]
    priority: i64,
    /// Task id that must finish first (repeatable).
    #[arg(long = "after")]
    after: Vec<String>,
    /// Skip independent review.
    #[arg(long)]
    no_review: bool,
    /// Number of distinct non-author AI providers that must approve (`>=2` enables quorum review).
    #[arg(long, default_value_t = 1)]
    quorum: u32,
    /// Preferred provider id for the first attempt.
    #[arg(long)]
    provider: Option<String>,
    /// Explicit skill ID to inject (repeatable). Combined with automatic BM25 skill matching unless --no-auto-skills is set.
    #[arg(long = "skill")]
    skills: Vec<String>,
    /// Disable automatic BM25 domain skill matching.
    #[arg(long)]
    no_auto_skills: bool,
    #[arg(long, default_value_t = 4)]
    max_attempts: u32,
}

#[derive(Args, Clone)]
struct RunArgs {
    /// Keep running and pick up new tasks until Ctrl+C.
    #[arg(long)]
    forever: bool,
    #[arg(long)]
    parallel: Option<usize>,
    /// Return instead of sleeping when every provider is cooling down.
    #[arg(long)]
    no_wait: bool,
    #[arg(long)]
    timeout_mins: Option<u64>,
    #[arg(long)]
    idle_mins: Option<u64>,
    /// Leave accepted work on its task branch instead of merging into the integration branch.
    #[arg(long)]
    no_merge: bool,
}

pub fn main(args: &[String]) -> i32 {
    let argv = std::iter::once("ai-fleet".to_string()).chain(args.iter().cloned());
    let cli = match Cli::try_parse_from(argv) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: cannot start runtime: {e}");
            return 1;
        }
    };
    match rt.block_on(run(cli)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn to_new_task(a: &TaskArgs) -> crate::Result<NewTask> {
    let title = a.title.clone().unwrap_or_else(|| {
        let first = a.prompt.lines().next().unwrap_or("task");
        first.chars().take(72).collect()
    });
    let mut nt = NewTask::new(title, a.prompt.clone());
    nt.owned_files = a.owned.clone();
    nt.verify_cmd = a.verify.clone();
    nt.priority = a.priority;
    nt.depends_on = a.after.clone();
    nt.review = !a.no_review;
    nt.review_quorum = a.quorum.max(1);
    nt.preferred_provider = a.provider.clone();
    nt.skills = a.skills.clone();
    nt.auto_skills = !a.no_auto_skills;
    nt.max_attempts = a.max_attempts;
    nt.complexity = match &a.complexity {
        Some(c) => Some(Complexity::parse(c).ok_or_else(|| crate::EngineError::Config(format!("unknown complexity `{c}`")))?),
        None => None,
    };
    Ok(nt)
}

fn apply_run_args(cfg: &mut EngineConfig, r: &RunArgs) {
    if let Some(p) = r.parallel {
        cfg.max_parallel = p.max(1);
    }
    if let Some(m) = r.timeout_mins {
        cfg.run_timeout = Duration::from_secs(m * 60);
    }
    if let Some(m) = r.idle_mins {
        cfg.idle_timeout = Duration::from_secs(m * 60);
    }
    cfg.wait_for_cooldown = !r.no_wait;
    cfg.auto_merge = !r.no_merge;
}

fn short(s: &str, n: usize) -> String {
    let one_line = s.replace(['\r', '\n'], " ");
    if one_line.chars().count() <= n {
        one_line
    } else {
        format!("{}…", one_line.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

fn print_task_row(t: &Task) {
    println!(
        "{:<36} {:<16} {:<9} att={} hand={} {:<22} {}",
        t.id,
        t.status.as_str(),
        t.complexity.as_str(),
        t.attempts,
        t.handoffs,
        t.last_provider.as_deref().map(|p| format!("{p}/{}", t.last_model.as_deref().unwrap_or("?"))).unwrap_or_else(|| "-".into()),
        short(&t.title, 50)
    );
}

async fn execute(engine: &Engine, r: &RunArgs) -> crate::Result<i32> {
    let token = engine.cancel_token();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\nstopping: terminating running agents, committing their partial work...");
            token.cancel();
        }
    });
    let mut events = engine.subscribe();
    let printer = tokio::spawn(async move {
        while let Ok(ev) = events.recv().await {
            let time = ev.ts.get(11..19).unwrap_or("");
            println!("{time} {:<26} {:<22} {}", ev.task_id.as_deref().unwrap_or("-"), ev.kind, short(&ev.message, 160));
        }
    });
    let summary = if r.forever { engine.run_forever().await? } else { engine.run_until_idle().await? };
    printer.abort();

    println!("\n== summary: {} run(s) started ==", summary.runs_started);
    for id in &summary.done {
        println!("  done     {id}");
    }
    for (id, why) in &summary.failed {
        println!("  FAILED   {id}: {}", short(why, 200));
    }
    for (id, why) in &summary.blocked {
        println!("  blocked  {id}: {}", short(why, 200));
    }
    for (id, why) in &summary.pending {
        println!("  pending  {id}: {}", short(why, 200));
    }
    if !summary.done.is_empty() && engine.config().auto_merge {
        println!(
            "\nAccepted work is on branch `{}`. Inspect with `git log {}` and merge it when you are satisfied.",
            engine.config().integration_branch,
            engine.config().integration_branch
        );
    }
    Ok(if summary.failed.is_empty() && summary.blocked.is_empty() { 0 } else { 3 })
}

async fn run(cli: Cli) -> crate::Result<i32> {
    let dir = match &cli.repo {
        Some(r) => r.clone(),
        None => std::env::current_dir()?,
    };
    let cmd = cli.cmd.unwrap_or(Cmd::Status);

    if let Cmd::Skills { cmd } = &cmd {
        let maybe_repo = git::repo_root(&dir).ok();
        let reg = SkillRegistry::load(maybe_repo.as_deref());
        return match cmd {
            SkillsCmd::List { domain } => {
                let list = reg.list(domain.as_deref());
                println!("total matched skills: {} (registry total: {})\n", list.len(), reg.len());
                println!("{:<38} {:<22} {:>8}  {:<36} {}", "ID", "DOMAIN", "STARS", "SOURCE", "TITLE");
                println!("{}", "-".repeat(132));
                for s in list {
                    println!(
                        "{:<38} {:<22} {:>8}  {:<36} {}",
                        short(&s.id, 37),
                        short(&s.domain, 21),
                        format!("★{}", s.stars),
                        short(&s.source_repo, 35),
                        short(&s.title, 44),
                    );
                }
                Ok(0)
            }
            SkillsCmd::Match { query, owned, top, reviewer } => {
                let role = if *reviewer { Role::Reviewer } else { Role::Author };
                let matches = reg.select(query, owned, role, &[], *top);
                if matches.is_empty() {
                    println!("no skills exceeded the relevance threshold for query: {query}");
                } else {
                    for (i, m) in matches.iter().enumerate() {
                        println!(
                            "{}. {:<36} score={:>5.2}  domain={:<20} ★{:<7} {} [{}]",
                            i + 1,
                            m.id,
                            m.score,
                            m.domain,
                            m.stars,
                            m.source_repo,
                            m.reason
                        );
                    }
                }
                Ok(0)
            }
            SkillsCmd::Show { id } => {
                let s = reg.get(id).ok_or_else(|| crate::EngineError::NotFound(format!("skill `{id}`")))?;
                println!(
                    "# {} ({})\ndomain:  {}\nsource:  {} (★{}, {})\norigin:  {}\ntags:    {}\n\n{}",
                    s.title,
                    s.id,
                    s.domain,
                    s.source_repo,
                    s.stars,
                    s.license,
                    s.origin,
                    s.tags.join(", "),
                    s.body
                );
                Ok(0)
            }
        };
    }

    if let Cmd::Mcp = &cmd {
        mcp::serve_stdio(&dir)?;
        return Ok(0);
    }
    if let Cmd::Console { bind } = &cmd {
        web::serve_http_async(&dir, bind).await?;
        return Ok(0);
    }
    if matches!(cmd, Cmd::Status) && git::repo_root(&dir).is_err() {
        let reg = SkillRegistry::load(None);
        println!("AI-Fleet — Real Execution Engine (not inside a Git repository: {})", dir.display());
        println!("Embedded Skills: {}", reg.len());
        println!("Detected CLI Binaries on PATH:");
        for p in default_providers() {
            let resolved = resolve_binary(&p.binary).map(|b| b.display().to_string()).unwrap_or_else(|| "NOT INSTALLED".into());
            println!("  - {:<14} ({}) -> {}", p.id, p.binary, resolved);
        }
        println!("\nRun inside a Git repository (or pass `--repo <path>`) to open SQLite `engine.db`.");
        return Ok(0);
    }

    let mut cfg = EngineConfig::load(&dir)?;
    match &cmd {
        Cmd::Run(r) | Cmd::Do { run: r, .. } | Cmd::Queen { run: r, .. } => apply_run_args(&mut cfg, r),
        _ => {}
    }
    let engine = Engine::open(cfg)?;

    match cmd {
        Cmd::Status => {
            let repo = engine.config().repo.clone();
            let head = git::head(&repo).unwrap_or_else(|_| "unknown".into());
            let tasks = engine.store().list_tasks()?;
            let runs = engine.store().recent_runs(10)?;
            let mem_count = engine.memory().count().unwrap_or(0);
            let ev_check = engine.store().verify_events()?;
            let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
            for t in &tasks {
                *by_status.entry(t.status.as_str()).or_default() += 1;
            }

            println!("AI-Fleet — Real SQLite Execution Engine");
            println!("  repository:  {} (HEAD {})", repo.display(), &head[..head.len().min(10)]);
            println!("  sqlite db:   {}", engine.config().state_dir.join("engine.db").display());
            println!("  memory db:   {} ({mem_count} record(s))", engine.config().state_dir.join("memory.db").display());
            println!("  skills:      {} embedded skill(s)", engine.skills().len());
            match ev_check {
                Ok(n) => println!("  event chain: intact ({n} SHA-256 chained event(s))"),
                Err(bad) => println!("  event chain: TAMPERED at event #{bad}"),
            }

            println!("\nProviders (`providers.json`):");
            for p in engine.provider_reports()? {
                let state = if !p.enabled {
                    "DISABLED".to_string()
                } else if let Some(u) = &p.cooldown_until {
                    format!("COOLDOWN until {u} ({})", p.cooldown_reason.unwrap_or_default())
                } else if p.resolved.is_some() {
                    "READY".to_string()
                } else {
                    "NOT INSTALLED".to_string()
                };
                println!(
                    "  - {:<13} {:<18} binary={}",
                    p.id,
                    state,
                    p.resolved.as_deref().unwrap_or("not on PATH")
                );
            }

            println!("\nTasks in SQLite (`engine.db`): {} total {:?}", tasks.len(), by_status);
            if tasks.is_empty() {
                println!("  (empty — submit real work with `ai-fleet do \"<prompt>\"` or `ai-fleet queen \"<epic>\" --submit`)");
            } else {
                for t in tasks.iter().take(15) {
                    print_task_row(t);
                }
            }
            if !runs.is_empty() {
                println!("\nRecent runs ({}):", runs.len());
                for r in runs.iter().take(5) {
                    println!(
                        "  #{:<4} {:<28} {:<8} {}/{} effort={} -> {} ({}ms)",
                        r.id,
                        short(&r.task_id, 27),
                        r.role,
                        r.provider,
                        r.model,
                        r.effort,
                        r.failure_kind.as_deref().or(r.outcome.as_deref()).unwrap_or("running"),
                        r.duration_ms.unwrap_or(0)
                    );
                }
            }
            println!("\nInteractive views: `ai-fleet tui` (ratatui) | `ai-fleet console` (axum HTTP+SSE) | `ai-fleet --help`");
            Ok(0)
        }
        Cmd::Tui => {
            tui::run_interactive_tui(engine).await?;
            Ok(0)
        }
        Cmd::Init => {
            println!("providers: {}", engine.config().providers_path().display());
            println!("database:  {}", engine.config().state_dir.join("engine.db").display());
            Ok(0)
        }
        Cmd::Submit(a) => {
            let t = engine.submit(to_new_task(&a)?)?;
            println!("queued {} (complexity={})", t.id, t.complexity.as_str());
            Ok(0)
        }
        Cmd::Do { task, run } => {
            let t = engine.submit(to_new_task(&task)?)?;
            println!("queued {} (complexity={})", t.id, t.complexity.as_str());
            execute(&engine, &run).await
        }
        Cmd::Run(r) => execute(&engine, &r).await,
        Cmd::List => {
            let tasks = engine.store().list_tasks()?;
            if tasks.is_empty() {
                println!("no tasks in engine.db. Queue one with: ai-fleet submit \"...\"");
            }
            for t in &tasks {
                print_task_row(t);
            }
            Ok(0)
        }
        Cmd::Show { id } => {
            let t = engine.store().get_task(&id)?.ok_or_else(|| crate::EngineError::NotFound(id.clone()))?;
            println!("{}", serde_json::to_string_pretty(&t)?);
            println!("\nruns:");
            for r in engine.store().runs_for_task(&id)? {
                println!(
                    "  #{:<4} {:<8} {}/{} effort={} -> {} {} ({} ms)\n        route: {}\n        log: {}",
                    r.id,
                    r.role,
                    r.provider,
                    r.model,
                    r.effort,
                    r.outcome.as_deref().unwrap_or("running"),
                    r.failure_kind.as_deref().unwrap_or(""),
                    r.duration_ms.unwrap_or(0),
                    r.route_reason,
                    r.log_path.as_deref().unwrap_or("")
                );
            }
            Ok(0)
        }
        Cmd::Runs { limit } => {
            for r in engine.store().recent_runs(limit)? {
                println!(
                    "#{:<5} {:<36} {:<8} {:<26} {:<7} {:<16} {}ms",
                    r.id,
                    r.task_id,
                    r.role,
                    format!("{}/{}", r.provider, r.model),
                    r.effort,
                    r.failure_kind.as_deref().or(r.outcome.as_deref()).unwrap_or("running"),
                    r.duration_ms.unwrap_or(0)
                );
            }
            Ok(0)
        }
        Cmd::Providers => {
            println!("config: {}\n", engine.config().providers_path().display());
            for p in engine.provider_reports()? {
                println!(
                    "{} ({}) {}",
                    p.id,
                    p.display_name,
                    if !p.enabled {
                        "DISABLED".to_string()
                    } else if let Some(u) = &p.cooldown_until {
                        format!("COOLING DOWN until {u} ({})", p.cooldown_reason.clone().unwrap_or_default())
                    } else if p.resolved.is_some() {
                        "ready".to_string()
                    } else {
                        "NOT INSTALLED".to_string()
                    }
                );
                println!("  binary: {} -> {}", p.binary, p.resolved.as_deref().unwrap_or("not found on PATH"));
                for (m, tier, rate, n) in &p.models {
                    let rate = if n == &0 { "no history".to_string() } else { format!("{:.0}% success over {n} runs", rate * 100.0) };
                    println!("  model {m:<20} tier {tier}  {rate}");
                }
                if !p.notes.is_empty() {
                    println!("  note: {}", p.notes);
                }
                println!();
            }
            Ok(0)
        }
        Cmd::Ready { provider } => {
            engine.clear_cooldown(&provider)?;
            println!("{provider} is ready");
            Ok(0)
        }
        Cmd::Retry { id } => {
            let t = engine.retry(&id)?;
            println!("{} -> {}", t.id, t.status);
            Ok(0)
        }
        Cmd::Cancel { id } => {
            let t = engine.cancel_task(&id)?;
            println!("{} -> {}", t.id, t.status);
            Ok(0)
        }
        Cmd::Log { verify, task, limit } => {
            if verify {
                match engine.store().verify_events()? {
                    Ok(n) => println!("event log intact: {n} events, hash chain verified"),
                    Err(id) => {
                        println!("EVENT LOG TAMPERED at event #{id}");
                        return Ok(4);
                    }
                }
            }
                for e in engine.store().events(task.as_deref(), limit)? {
                println!("{} {:<26} {:<24} {}", e.ts, e.task_id.as_deref().unwrap_or("-"), e.kind, short(&e.message, 160));
            }
            Ok(0)
        }
        Cmd::Queen { epic, offline, submit, execute: exec_now, verify, no_review, run } => {
            let plan = engine.plan_epic(&epic, offline, verify).await?;
            println!("Queen Plan ({}) — {} task(s) across {} wave(s)", plan.planner_source, plan.nodes.len(), plan.waves);
            if let Some(v) = &plan.detected_verify_cmd {
                println!("  verification gate: `{v}`");
            }
            for (a, b, why) in &plan.auto_serialized_edges {
                println!("  auto-serialized `{a}` -> `{b}` ({why})");
            }
            for n in &plan.nodes {
                println!(
                    "  [wave {}] {:<26} deps={:?} scope={:?} skills={:?}\n           {}",
                    n.wave, n.key, n.depends_on, n.owned_files, n.skills, n.title
                );
            }
            if submit || exec_now {
                let ids = engine.submit_plan(&plan, !no_review)?;
                println!("\nenqueued {} task(s): {}", ids.len(), ids.join(", "));
                if exec_now {
                    return execute(&engine, &run).await;
                }
            }
            Ok(0)
        }
        Cmd::Memory { cmd } => match cmd {
            MemoryCmd::Remember { title, content, tags, files } => {
                let id = engine.memory().store(NewMemory {
                    kind: MemoryKind::Knowledge,
                    title,
                    content,
                    files,
                    tags,
                    task_id: None,
                    importance: 1.5,
                })?;
                println!("stored memory #{id}");
                Ok(0)
            }
            MemoryCmd::Search { query, limit } => {
                for h in engine.memory().search(&query, &[], limit)? {
                    println!(
                        "#{:<4} [{:<9}] score={:.3} vec={:.2} fts={:?}  {}\n      {}",
                        h.record.id,
                        h.record.kind.as_str(),
                        h.score,
                        h.vector_sim,
                        h.lexical_rank,
                        h.record.title,
                        short(&h.record.content, 160)
                    );
                }
                Ok(0)
            }
            MemoryCmd::Recent { limit } => {
                for r in engine.memory().recent(limit)? {
                    println!("#{:<4} [{:<9}] {} — {}", r.id, r.kind.as_str(), r.title, short(&r.content, 120));
                }
                Ok(0)
            }
        },
        Cmd::Graph { blast, top } => {
            let g = CodeGraph::scan(&engine.config().repo, 1500)?;
            println!(
                "CodeGraph: {} source files, {} LOC, {} dependency edges, {} circular SCC(s)",
                g.total_files,
                g.total_lines,
                g.edges.len(),
                g.cycles.len()
            );
            println!("Languages: {:?}", g.languages);
            println!("\nTop {top} architectural hubs by PageRank:");
            for h in g.top_hubs(top) {
                println!(
                    "  PR={:.4}  in={:<3} out={:<3} {:<44} symbols={:?}",
                    h.pagerank,
                    h.in_degree,
                    h.out_degree,
                    h.path,
                    &h.symbols[..h.symbols.len().min(5)]
                );
            }
            if !blast.is_empty() {
                let impacted = g.blast_radius(&blast);
                println!("\nBlast radius ({} transitive dependent file(s)): {:?}", impacted.len(), impacted);
            }
            if !g.cycles.is_empty() {
                println!("\nCircular dependencies: {:?}", g.cycles);
            }
            Ok(0)
        }
        Cmd::Audit { mut files } => {
            let repo = engine.config().repo.clone();
            if files.is_empty() {
                let tracked = git::git(&repo, &["ls-files"]).unwrap_or_default();
                files = tracked
                    .lines()
                    .filter(|l| !l.starts_with("v3/") && !l.starts_with("plugins/") && !l.starts_with("skills/"))
                    .take(1000)
                    .map(str::to_string)
                    .collect();
            }
            let rep = audit_file_list(&repo, &files)?;
            if rep.findings.is_empty() {
                println!("static gate passed: 0 findings across {} file(s)", files.len());
                return Ok(0);
            }
            for f in &rep.findings {
                println!("[{:?}] {}:{} ({}) — {}", f.severity, f.file, f.line, f.rule, f.message);
            }
            Ok(if rep.passed { 0 } else { 5 })
        }
        Cmd::Skills { .. } | Cmd::Mcp | Cmd::Console { .. } => unreachable!(),
    }
}
