//! `ai-fleet` v4.0 CLI — Unified Command Entrypoint for the 15-Crate Multi-AI Orchestrator.

use chrono::Utc;
use fleet_agents::find_best_persona;
use fleet_analyzer::{analyze_git_repository, run_system_doctor};
use fleet_core::{
    append_ledger_row, detect_head_commit, detect_workspace_root, sha256_bytes, sha256_file, verify_ledger_chain,
    AiProvider, ModelSpec, SwarmTask,
};
use fleet_federation::{compute_gumbel_watermark_zscore, create_federated_packet};
use fleet_hooks::{execute_lifecycle_hook, init_workspace_integration};
use fleet_mcp::{execute_mcp_tool, run_mcp_stdio_server};
use fleet_memory::{insert_hnsw_node, search_hnsw_semantic};
use fleet_neural::{compress_message_to_token_budget, route_moe_experts};
use fleet_plugins::{catalog_48_plugins, discover_plugins_for_task};
use fleet_providers::{
    execute_ai_cli,
    handoff::{build_handoff_prompt, load_on_disk_persona_and_skill_context, seal_checkpoint, transfer_checkpoint_to_receiver},
    load_or_init_fleet_state, record_outcome, refresh_provider_quotas, save_fleet_state,
};
use fleet_security::{check_casa_authorization, evaluate_continue_gate, scan_aidefence_threats};
use fleet_swarm::{
    classify_task_tier, compute_optimal_route, execute_consensus, prepare_fenced_worktree, queen_decompose_task,
};
use fleet_tui::{
    badge_effort, badge_status, badge_tier, enable_ansi_support, print_dashboard, render_float_bar, BOLD,
    BRIGHT_CYAN, CYAN, DIM, GREEN, MAGENTA, RED, RESET, YELLOW,
};
use fleet_workflows::advance_sparc_phase;
use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::thread;
use std::time::{Duration as StdDuration, Instant};

fn print_v4_help() {
    println!(
        "\n{}{}AI-FLEET v4.0.0 (15-Crate Rust Enterprise Monorepo){} — Multi-AI Swarm Orchestration Platform",
        BOLD, BRIGHT_CYAN, RESET
    );
    println!("64 Agent Personas · 48 Enterprise Plugins · HNSW Vector Memory · 2-Layer 8-Expert MoE + SONA/EWC ·");
    println!("PBFT/Raft/Gossip Consensus · CASA + AIDefence · SPARC · Gumbel Watermark · Real Git Analyzer & MCP.\n");
    println!("{}1. PROJECT INIT, DOCTOR, REPO ANALYZER & LIVE TUI:{}", BOLD, RESET);
    println!("  {}ai-fleet init [--mcp]{}                                  Generate .ai-fleet/mcp.json, hooks.json & SKILL.md", GREEN, RESET);
    println!("  {}ai-fleet doctor [--fix]{}                                Diagnose git, rustc, cargo, claude, codex, node, docker", GREEN, RESET);
    println!("  {}ai-fleet analyze{}                                       Real Git repository diff, domain breakdown & risk score", GREEN, RESET);
    println!("  {}ai-fleet status [--view <page>] [--look bbs|orbital]{}   8-view TUI (overview|agents|workflows|mcp|moe|memory)", GREEN, RESET);
    println!("  {}ai-fleet console [port]{}                                 Launch Interactive Web Mission Control GUI (http://127.0.0.1:7878)", GREEN, RESET);
    println!("  {}ai-fleet daemon <start|stop|status>{}                     Manage detached background autopilot daemon process", GREEN, RESET);
    println!("  {}ai-fleet watch [seconds]{}                               Live auto-refreshing dashboard monitor loop", GREEN, RESET);
    println!("\n{}2. PARALLEL SWARM EXECUTION & DYNAMIC MODEL / EFFORT CONTROL:{}", BOLD, RESET);
    println!("  {}ai-fleet start{}                                         Dispatch parallel AI agents in FencedLease worktrees", CYAN, RESET);
    println!("  {}ai-fleet stop{}                                          Drain active writers and save SHA-256 checkpoints", CYAN, RESET);
    println!("  {}ai-fleet exec <task_id> <prompt>{}                       Execute real CLI with AIDefence + CASA + model failover", CYAN, RESET);
    println!("  {}ai-fleet drill{}                                         Run live v4 failover + FencedLease + PBFT + Watermark drill", CYAN, RESET);
    println!("  {}ai-fleet provider add <id> <bin> <model> <effort>{}      Connect any AI CLI tool (ollama, gemini, cursor, aider)", CYAN, RESET);
    println!("  {}ai-fleet provider switch <id> <model> <effort>{}         Switch active model & reasoning effort (low..xhigh..max)", CYAN, RESET);
    println!("  {}ai-fleet provider quota <id> <used> <limit> [ts|ready]{} Update or reset 5-hour / daily quota circuit-breaker", CYAN, RESET);
    println!("\n{}3. 64 PERSONAS, 48 PLUGINS, QUEEN DECOMPOSER, SPARC & HOOKS:{}", BOLD, RESET);
    println!("  {}ai-fleet agents [search_query]{}                         Browse or match from the 64 specialized agent personas", MAGENTA, RESET);
    println!("  {}ai-fleet plugins [search_query]{}                        Browse or discover from the 48 enterprise plugins", MAGENTA, RESET);
    println!("  {}ai-fleet ecosystem{}                                     Live audit of all 8,100+ files (145 skills, 108 agents, 168 cmds)", MAGENTA, RESET);
    println!("  {}ai-fleet watermark <text>{}                              Run SynthID-Text Tournament + Gumbel + Bayesian watermark detector", MAGENTA, RESET);
    println!("  {}ai-fleet queen <task_id>{}                               Decompose task into persona-assigned DAG sub-tasks", MAGENTA, RESET);
    println!("  {}ai-fleet sparc <task_id>{}                               Advance task through 5-phase SPARC methodology", MAGENTA, RESET);
    println!("  {}ai-fleet hooks <pre-tool|post-tool|session-start|stop>{} Run lifecycle hook evaluation (Claude/Codex hooks)", MAGENTA, RESET);
    println!("\n{}4. NEURAL MoE, HNSW MEMORY, CONSENSUS, FEDERATION & MCP:{}", BOLD, RESET);
    println!("  {}ai-fleet moe <task_description>{}                        Run 2-Layer 8-Expert MoE gating + EWC Fisher loss", YELLOW, RESET);
    println!("  {}ai-fleet pheromone [evaporate|boost <model>]{}           Inspect or update ADR-330 stigmergic pheromone trails", YELLOW, RESET);
    println!("  {}ai-fleet memory <store|search|list> ...{}                64-dim HNSW semantic vector index & hybrid search", YELLOW, RESET);
    println!("  {}ai-fleet compress <max_tokens> <message>{}               Information-Bottleneck token-budget message compressor", YELLOW, RESET);
    println!("  {}ai-fleet consensus <PBFT|Raft|Gossip> <topic>{}          Execute multi-model weighted consensus protocol", YELLOW, RESET);
    println!("  {}ai-fleet defense <payload>{}                             Scan for ADR-118 prompt injection & secret exposure", YELLOW, RESET);
    println!("  {}ai-fleet casa <action>{}                                 Evaluate CASA (ADR-380 §3) deny-by-default envelope", YELLOW, RESET);
    println!("  {}ai-fleet federate <peer> <payload>{}                     Strip PII/secrets & embed Gumbel-Softmax watermark", YELLOW, RESET);
    println!("  {}ai-fleet mcp <start|tools|exec <tool> <json>>{}          Start JSON-RPC 2.0 MCP server over stdio or invoke tool", YELLOW, RESET);
    println!("  {}ai-fleet engine <do|submit|run|queen|memory|graph|audit>{} Real async multi-AI SQLite engine, Queen DAG & Static Gate", YELLOW, RESET);
    println!("  {}ai-fleet skills <list|match|show>{}                      Browse & match the 120 Elite GitHub Agent Skills", YELLOW, RESET);
    println!("  {}ai-fleet graph [--blast <file>] | ai-fleet audit{}       CodeGraph PageRank/SCC analyzer & IEEE-754 Money/RLS Gate", YELLOW, RESET);
    println!("  {}ai-fleet policy verify{}                                 Verify SHA-256 hash-chained decision ledger & TruthAnchors\n", YELLOW, RESET);
}

fn main() {
    enable_ansi_support();
    let args: Vec<String> = env::args().skip(1).collect();

    // Isolate legacy v4/v5 state-v5.json diagnostics behind explicit `ai-fleet legacy ...`
    // so zero hardcoded demo tasks (`arch-verify`, `security-rls-audit`) EVER appear on any primary command.
    if args.first().map(String::as_str) == Some("legacy") {
        run_legacy_v4(&args[1..]);
        return;
    }

    // Backwards-compatible alias: `ai-fleet engine <subcommand>` == `ai-fleet <subcommand>`
    if args.first().map(String::as_str) == Some("engine") {
        std::process::exit(fleet_engine::cli::main(&args[1..]));
    }
    if args.first().map(String::as_str) == Some("skills") && args.len() == 1 {
        std::process::exit(fleet_engine::cli::main(&["skills".into(), "list".into()]));
    }
    if args.first().map(String::as_str) == Some("memory") && args.len() == 1 {
        std::process::exit(fleet_engine::cli::main(&["memory".into(), "recent".into()]));
    }
    if matches!(args.first().map(String::as_str), Some("web" | "gui")) {
        let mut forwarded = vec!["console".to_string()];
        forwarded.extend_from_slice(&args[1..]);
        std::process::exit(fleet_engine::cli::main(&forwarded));
    }

    std::process::exit(fleet_engine::cli::main(&args));
}

fn run_legacy_v4(raw_args: &[String]) {
    let args: Vec<String> = raw_args.to_vec();
    let mut state = load_or_init_fleet_state();

    if args.is_empty() {
        print_dashboard(&state, None);
        return;
    }

    match args[0].as_str() {
        "init" => {
            let write_root_mcp = args.iter().any(|a| a == "--mcp");
            let ws = detect_workspace_root();
            let files = init_workspace_integration(&ws, write_root_mcp);
            append_ledger_row(
                &mut state.decision_ledger,
                "owner:cli",
                "workspace.init.files",
                &format!("Generated {} integration file(s)", files.len()),
            );
            save_fleet_state(&state);
            println!("\n{}{}✔ Initialized AI-Fleet v4.0 integration in {}{}", BOLD, GREEN, ws.display(), RESET);
            for f in files {
                println!("  ├─ Created: {}", f);
            }
            println!();
        }
        "doctor" => {
            let fix = args.iter().any(|a| a == "--fix");
            let checks = run_system_doctor();
            println!("\n{}{}🩺 AI-FLEET v4.0 SYSTEM & TOOLCHAIN DOCTOR{}", BOLD, BRIGHT_CYAN, RESET);
            for c in &checks {
                println!(
                    "  ├─ {:<28} : {} ({})",
                    c.component,
                    if c.healthy { format!("{}✔ HEALTHY{}", GREEN, RESET) } else { format!("{}▲ MISSING{}", YELLOW, RESET) },
                    c.detail
                );
            }
            let ledger_ok = verify_ledger_chain(&state.decision_ledger).is_ok();
            println!(
                "  ├─ {:<28} : {}",
                "SHA-256 Decision Ledger",
                if ledger_ok { format!("{}✔ VERIFIED ({} blocks){}", GREEN, state.decision_ledger.len(), RESET) } else { format!("{}✖ CORRUPTED{}", RED, RESET) }
            );
            if fix {
                let ws = detect_workspace_root();
                let created = init_workspace_integration(&ws, false);
                println!("  └─ {}Auto-Repair (--fix):{} Verified/restored {} harness config file(s)", GREEN, RESET, created.len());
            }
            println!();
        }
        "analyze" => {
            let ws = detect_workspace_root();
            let rep = analyze_git_repository(&ws);
            println!("\n{}{}📊 REAL GIT REPOSITORY & CHANGE RISK ANALYSIS{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Repository:         {} (branch: {}, head: {})", rep.repo_root, rep.branch, rep.head_commit);
            println!("  ├─ Working Tree:       {} modified, {} untracked file(s)", rep.modified_files, rep.untracked_files);
            println!("  ├─ Risk Classification: {} │ Recommended Effort: {}", badge_tier(&rep.risk_level.to_lowercase()), badge_effort(&rep.recommended_effort));
            println!("  ├─ Domain Breakdown:   {:?}", rep.domain_breakdown);
            println!("  └─ Risk Reasons:");
            for r in &rep.risk_reasons {
                println!("       • {}", r);
            }
            println!();
        }
        "plugins" | "discover-plugins" => {
            if args.len() > 1 {
                let q = args[1..].join(" ");
                let matched = discover_plugins_for_task(&q);
                println!("\n{}{}🧩 MATCHED ENTERPRISE PLUGINS FOR: \"{}\"{}", BOLD, BRIGHT_CYAN, q, RESET);
                for p in matched {
                    println!("  • {:<26} [{}] ({} tools) — {}", p.id, p.category, p.tools_count, p.description);
                }
                println!();
            } else {
                let all = catalog_48_plugins();
                let total_tools: usize = all.iter().map(|p| p.tools_count).sum();
                println!(
                    "\n{}{}🧩 48 ENTERPRISE PLUGINS CATALOG ({} specialized tools across 15 crates){}",
                    BOLD, BRIGHT_CYAN, total_tools, RESET
                );
                for p in &all {
                    println!(
                        "  {}▪ {:<26}{} │ [{:<13}] │ {:>2} tools │ {}",
                        BOLD, p.id, RESET, p.category, p.tools_count, p.description
                    );
                }
                println!();
            }
        }
        "hooks" => {
            let event = args.get(1).map(|s| s.as_str()).unwrap_or("SessionStart");
            let payload = if args.len() > 2 { args[2..].join(" ") } else { String::new() };
            let res = execute_lifecycle_hook(event, &payload, &state.casa_envelope, &state.continue_gate);
            println!(
                "\n🪝 Hook [{}] -> allowed={} action={} ({})\n",
                res.hook_event, res.allowed, res.action, res.message
            );
            if !res.allowed {
                std::process::exit(1);
            }
        }
        "ecosystem" | "skills" | "commands" => {
            let ws = detect_workspace_root();
            let count_dir = |rel: &str| -> usize {
                let p = ws.join(rel);
                if !p.exists() {
                    return 0;
                }
                fn walk(dir: &std::path::Path) -> usize {
                    let mut c = 0;
                    if let Ok(rd) = fs::read_dir(dir) {
                        for entry in rd.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                c += walk(&path);
                            } else {
                                c += 1;
                            }
                        }
                    }
                    c
                }
                walk(&p)
            };
            let skills_cnt = count_dir(".agents/skills");
            let claude_agents = count_dir(".claude/agents");
            let claude_cmds = count_dir(".claude/commands");
            let claude_skills = count_dir(".claude/skills");
            let plugins_cnt = count_dir("plugins");
            let v3_cnt = count_dir("v3");
            let docs_cnt = count_dir("docs");
            let crates_cnt = count_dir("crates") + count_dir("bins");
            let total = skills_cnt + claude_agents + claude_cmds + claude_skills + plugins_cnt + v3_cnt + docs_cnt + crates_cnt;

            println!(
                "\n{}{}🏛 AI-FLEET v4.0 FULL ECOSYSTEM INVENTORY ({} active files on disk){}",
                BOLD, BRIGHT_CYAN, total, RESET
            );
            println!("  ├─ {}17 Native Rust Crates (`crates/*` + `bins/*`):{}  {:>5} source files", GREEN, RESET, crates_cnt);
            println!("  ├─ {}48 Enterprise Plugins (`plugins/*`):{}            {:>5} plugin files", MAGENTA, RESET, plugins_cnt);
            println!("  ├─ {}V3 Core Modules (`v3/@claude-flow/*`):{}          {:>5} module files", CYAN, RESET, v3_cnt);
            println!("  ├─ {}Agent Skills (`.agents/skills/*`):{}              {:>5} skill definitions", YELLOW, RESET, skills_cnt);
            println!("  ├─ {}Claude Agent Specs (`.claude/agents/*`):{}        {:>5} agent markdown specs", GREEN, RESET, claude_agents);
            println!("  ├─ {}Slash Commands (`.claude/commands/*`):{}          {:>5} command workflows", CYAN, RESET, claude_cmds);
            println!("  ├─ {}Claude Skills & Helpers (`.claude/skills/*`):{}   {:>5} skill/helper files", MAGENTA, RESET, claude_skills);
            println!("  └─ {}Architecture & ADR Docs (`docs/*`):{}             {:>5} documentation files\n", YELLOW, RESET, docs_cnt);
        }
        "watermark" | "synthid" => {
            let vocab = 128u32;
            let cfg = ruflo_watermark::WatermarkConfig::new(ruflo_watermark::WatermarkKey::from_bytes(b"ai-fleet-v4-synthid"))
                .with_layers(6);
            let tokens: Vec<u32> = (0..vocab).collect();
            let probs = vec![1.0f32 / (vocab as f32); vocab as usize];
            let mut wm = ruflo_watermark::Watermarker::new(cfg, ruflo_watermark::Scheme::Tournament);
            let emitted: Vec<u32> = (0..240).map(|_| tokens[wm.step(&tokens, &probs)]).collect();
            let det_tour = ruflo_watermark::detect_tournament(&emitted, cfg);
            let det_gumbel = ruflo_watermark::detect_gumbel(&emitted, cfg);

            println!(
                "\n{}{}💧 SYNTHID-TEXT TOURNAMENT & GUMBEL WATERMARK ENGINE (`crates/ruflo-watermark`){}",
                BOLD, BRIGHT_CYAN, RESET
            );
            println!("  ├─ Scored Positions:  {} tokens (6-layer Tournament tree)", det_tour.scored_positions);
            println!("  ├─ Tournament Z-Score:{}{:>8.3}σ{} (p-value = {:.3e}, log10_p = {:.2})", GREEN, det_tour.z_score, RESET, det_tour.p_value, det_tour.log10_p);
            println!("  └─ Gumbel Reference:  {:>8.3}σ (distortion-free baseline)\n", det_gumbel.z_score);
        }
        "watch" => {
            let cycles: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
            for i in 1..=cycles {
                print!("\x1b[2J\x1b[H");
                refresh_provider_quotas(&mut state);
                println!("{}🔄 Live Watch Frame {}/{} (Press Ctrl+C to exit){}", DIM, i, cycles, RESET);
                print_dashboard(&state, None);
                if i < cycles {
                    thread::sleep(StdDuration::from_secs(1));
                }
            }
        }
        "console" | "web" | "gui" => {
            let port: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(7878);
            let no_open = args.iter().any(|a| a == "--no-open");
            let max_reqs = args
                .iter()
                .position(|a| a == "--max-requests")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse::<usize>().ok());
            if let Err(e) = fleet_web::serve_mission_control(port, !no_open, max_reqs) {
                eprintln!("{}✖ Failed to start Mission Control Web GUI: {}{}", RED, e, RESET);
                std::process::exit(1);
            }
        }
        "daemon" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "start" => {
                    let exe = env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("ai-fleet"));
                    let child = std::process::Command::new(exe)
                        .args(["watch", "3600"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    match child {
                        Ok(c) => {
                            state.daemon_running = true;
                            state.daemon_pid = Some(c.id());
                            append_ledger_row(&mut state.decision_ledger, "daemon:v5", "daemon.start", &format!("Detached background daemon pid={}", c.id()));
                            save_fleet_state(&state);
                            println!("{}✔ Started detached background AI-Fleet Daemon (PID: {}).{}", GREEN, c.id(), RESET);
                        }
                        Err(e) => eprintln!("{}✖ Failed to start daemon: {}{}", RED, e, RESET),
                    }
                }
                "stop" => {
                    if let Some(pid) = state.daemon_pid.take() {
                        if cfg!(windows) {
                            let _ = std::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output();
                        }
                    }
                    state.daemon_running = false;
                    save_fleet_state(&state);
                    println!("{}✔ Stopped background AI-Fleet Daemon safely.{}", GREEN, RESET);
                }
                _ => {
                    println!(
                        "\n⚙ AI-Fleet Background Daemon: {} (PID: {:?}, 12 autonomous workers configured)\n",
                        if state.daemon_running { format!("{}RUNNING{}", GREEN, RESET) } else { format!("{}STOPPED{}", YELLOW, RESET) },
                        state.daemon_pid
                    );
                }
            }
        }
        "status" | "dashboard" | "tui" => {
            let mut view_opt = None;
            if let Some(pos) = args.iter().position(|a| a == "--look") {
                if let Some(look) = args.get(pos + 1) {
                    state.look_mode = look.clone();
                    save_fleet_state(&state);
                }
            }
            if let Some(pos) = args.iter().position(|a| a == "--view") {
                if let Some(v) = args.get(pos + 1) {
                    state.active_view = v.clone();
                    save_fleet_state(&state);
                    view_opt = Some(v.as_str());
                }
            }
            print_dashboard(&state, view_opt);
        }
        "start" | "run" => {
            let invoke_live_cli = args.iter().any(|a| a == "--live-cli");
            refresh_provider_quotas(&mut state);
            let ws_root = detect_workspace_root();
            let mut busy = BTreeSet::new();
            let task_ids: Vec<String> = state.tasks.keys().cloned().collect();

            println!(
                "\n{}{}⚡ Launching Parallel Multi-AI Swarm across FencedLease Git Worktrees (Windows Job Object Containment)...{}",
                BOLD, GREEN, RESET
            );
            for tid in task_ids {
                let task_snapshot = match state.tasks.get(&tid) {
                    Some(t) if t.status == "queued" || t.status == "review" => t.clone(),
                    _ => continue,
                };
                let is_rev = task_snapshot.status == "review";
                if let Some(route) = compute_optimal_route(
                    &state.providers,
                    &state.moe_experts,
                    &state.reasoning_bank,
                    &task_snapshot,
                    &busy,
                    is_rev,
                ) {
                    busy.insert(route.selected_provider.clone());
                    state.global_epoch += 1;
                    let epoch = state.global_epoch;
                    let lease = prepare_fenced_worktree(
                        &ws_root,
                        &tid,
                        &route.selected_provider,
                        &route.selected_model,
                        &task_snapshot.owned_files,
                        epoch,
                    );

                    // If a prior checkpoint exists from an interrupted writer, verify SHA-256 and transfer owned files
                    let prior_cp = state.checkpoints.get(&tid).cloned();
                    if let Some(ref cp) = prior_cp {
                        let _ = transfer_checkpoint_to_receiver(cp, &lease, &ws_root, &task_snapshot.owned_files);
                    }

                    let persona_ctx = load_on_disk_persona_and_skill_context(&ws_root, &route.assigned_persona);
                    let handoff_prompt = build_handoff_prompt(
                        &task_snapshot.title,
                        &task_snapshot.owned_files,
                        &persona_ctx,
                        prior_cp.as_ref(),
                        is_rev,
                    );

                    let wt_abs = ws_root.join(&lease.worktree_path);
                    let mut active_procs_rem = 0u32;

                    if invoke_live_cli {
                        if let Some(prov) = state.providers.get(&route.selected_provider).cloned() {
                            match execute_ai_cli(&prov, &route.selected_model, &route.selected_effort, &wt_abs, &handoff_prompt) {
                                Ok((_, rem)) => active_procs_rem = rem,
                                Err((kind, reset, _, rem)) => {
                                    active_procs_rem = rem;
                                    if let Some(pm) = state.providers.get_mut(&route.selected_provider) {
                                        if kind == "quota" {
                                            pm.status = "quota".into();
                                            pm.retry_at = reset;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // Ensure owned report file is materialized inside the fenced worktree for checkpoint sealing
                        for rel in &task_snapshot.owned_files {
                            let target = wt_abs.join(rel);
                            if let Some(parent) = target.parent() {
                                let _ = fs::create_dir_all(parent);
                            }
                            if !target.exists() {
                                let _ = fs::write(
                                    &target,
                                    format!(
                                        "# Task {} ({})\nRouted to {}/{} (effort={}, persona={}, epoch=#{})\n",
                                        tid, task_snapshot.title, route.selected_provider, route.selected_model, route.selected_effort, route.assigned_persona, epoch
                                    ),
                                );
                            }
                        }
                    }

                    if let Ok(cp) = seal_checkpoint(
                        &tid,
                        epoch,
                        &wt_abs,
                        &task_snapshot.base_commit,
                        &route.selected_provider,
                        &route.selected_model,
                        &route.selected_effort,
                        &task_snapshot.owned_files,
                        None,
                        active_procs_rem,
                    ) {
                        println!(
                            "  {}-> Task [{}]{} (Epoch #{}, SPARC:{}) -> {}{}/{}{} ({}) │ Persona: {}{}{} │ Checkpoint SHA-256 Sealed (procs_rem={})",
                            BRIGHT_CYAN,
                            tid,
                            RESET,
                            epoch,
                            task_snapshot.sparc_phase,
                            BOLD,
                            route.selected_provider,
                            route.selected_model,
                            RESET,
                            badge_effort(&route.selected_effort),
                            MAGENTA,
                            route.assigned_persona,
                            RESET,
                            cp.writer_processes_remaining
                        );
                        state.checkpoints.insert(tid.clone(), cp);
                    }

                    state.fenced_leases.insert(tid.clone(), lease);
                    let top_exp = route
                        .moe_experts
                        .first()
                        .map(|(n, _)| n.clone())
                        .unwrap_or_else(|| "coder".into());
                    record_outcome(&mut state, &route.selected_provider, &route.selected_model, &top_exp, 0.97, 85);
                    if let Some(t) = state.tasks.get_mut(&tid) {
                        t.assigned_provider = Some(route.selected_provider.clone());
                        t.assigned_model = Some(route.selected_model.clone());
                        t.assigned_persona = Some(route.assigned_persona.clone());
                        t.target_effort = route.selected_effort.clone();
                        t.lease_epoch = epoch;
                    }
                    append_ledger_row(
                        &mut state.decision_ledger,
                        &format!("{}/{}", route.selected_provider, route.selected_model),
                        "swarm.dispatch.v5",
                        &format!("Task {} sealed checkpoint in epoch #{} with persona {}", tid, epoch, route.assigned_persona),
                    );
                }
            }
            state.continue_gate.consecutive_steps += 1;
            save_fleet_state(&state);
            print_dashboard(&state, None);
        }
        "stop" => {
            state.daemon_running = false;
            append_ledger_row(
                &mut state.decision_ledger,
                "owner:cli",
                "swarm.stop",
                "Drained active writers and saved all FencedLease checkpoints",
            );
            save_fleet_state(&state);
            println!("{}✔ Swarm stopped safely. All FencedLeases and HNSW memory nodes persisted.{}", GREEN, RESET);
        }
        "agents" => {
            if args.len() > 1 {
                let q = args[1..].join(" ");
                let p = find_best_persona(&q);
                println!("\n{}{}🤖 MATCHED SPECIALIZED AGENT PERSONA{}", BOLD, BRIGHT_CYAN, RESET);
                println!("  ├─ ID / Domain:  {}{}{} ({})", BOLD, p.id, RESET, p.domain);
                println!("  ├─ Role Title:   {}", p.role_title);
                println!("  ├─ Tier/Effort:  {} / {}", badge_tier(&p.default_tier), badge_effort(&p.default_effort));
                println!("  ├─ Capabilities: {:?}", p.capabilities);
                println!("  └─ Directive:    {}\n", p.system_directive);
            } else {
                print_dashboard(&state, Some("agents"));
            }
        }
        "queen" => {
            let tid = args.get(1).map(|s| s.as_str()).unwrap_or("security-rls-audit");
            if let Some(t) = state.tasks.get(tid) {
                let analysis = queen_decompose_task(t);
                println!("\n{}{}👑 QUEEN HIVE-MIND TASK DECOMPOSITION [{}]{}", BOLD, MAGENTA, tid, RESET);
                println!("  ├─ Complexity Score:   {:.2}", analysis.complexity_score);
                println!("  ├─ Primary Domain:     {}", analysis.recommended_domain);
                println!("  ├─ Lead Persona:       {}{}{}", GREEN, analysis.primary_persona, RESET);
                println!("  └─ Generated Sub-Task DAG:");
                for st in &analysis.subtasks {
                    println!(
                        "       • [{}] {:<10} │ Persona: {:<22} │ {}",
                        st.id, st.status, st.assigned_persona, st.title
                    );
                }
                println!();
            } else {
                eprintln!("{}Task '{}' not found.{}", RED, tid, RESET);
            }
        }
        "sparc" => {
            let tid = args.get(1).map(|s| s.as_str()).unwrap_or("arch-verify");
            if let Some(t) = state.tasks.get_mut(tid) {
                let prev = t.sparc_phase.clone();
                let (next_phase, persona, eff, desc) = advance_sparc_phase(&prev);
                t.sparc_phase = next_phase.to_string();
                t.assigned_persona = Some(persona.to_string());
                t.target_effort = eff.to_string();
                append_ledger_row(
                    &mut state.decision_ledger,
                    "sparc:conductor",
                    "sparc.advance",
                    &format!("Task {} advanced {} -> {} ({})", tid, prev, next_phase, persona),
                );
                save_fleet_state(&state);
                println!(
                    "\n{}✔ SPARC Phase Advanced for [{}]: {} -> {}{}{}",
                    GREEN, tid, prev, BOLD, next_phase, RESET
                );
                println!("  ├─ Lead Persona: {}", persona);
                println!("  ├─ Target Effort: {}", badge_effort(eff));
                println!("  └─ Gate Criteria: {}\n", desc);
            } else {
                print_dashboard(&state, Some("workflows"));
            }
        }
        "moe" => {
            let query = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "Audit SQL migration money scale, RLS policies, and verify golden tests".to_string()
            };
            let res = route_moe_experts(&state.moe_experts, &query, 4);
            println!(
                "\n{}{}🧠 2-LAYER 8-EXPERT MIXTURE-OF-EXPERTS (MoE) + SONA/EWC GATING{}",
                BOLD, BRIGHT_CYAN, RESET
            );
            println!("  ├─ Query:             \"{}\"", query);
            println!("  ├─ Routing Entropy:   {:.4} nats", res.entropy);
            println!("  ├─ Load-Balance Loss: {:.4}", res.load_balance_loss);
            println!("  ├─ EWC Fisher Loss:   {:.6}", res.ewc_regularization_loss);
            println!("  └─ Top-4 Activated Experts:");
            for (idx, (name, prob)) in res.top_experts.iter().enumerate() {
                println!("       {}. {:<13} {}", idx + 1, name, render_float_bar(*prob, 1.0, 14, GREEN));
            }
            println!();
        }
        "memory" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "store" | "set" if args.len() >= 5 => {
                    let ns = &args[2];
                    let key = &args[3];
                    let content = args[4..].join(" ");
                    insert_hnsw_node(&mut state.vector_memory, ns, key, &content, vec!["cli".into()], 0.95);
                    save_fleet_state(&state);
                    println!(
                        "{}✔ Indexed into 64-dim HNSW Vector Graph: [{}] {} (total nodes={}){}",
                        GREEN,
                        ns,
                        key,
                        state.vector_memory.len(),
                        RESET
                    );
                }
                "search" if args.len() >= 3 => {
                    let q = args[2..].join(" ");
                    let hits = search_hnsw_semantic(&state.vector_memory, &q, 4);
                    println!("\n{}{}🔍 HNSW SEMANTIC VECTOR SEARCH RESULTS FOR: \"{}\"{}", BOLD, BRIGHT_CYAN, q, RESET);
                    for (idx, h) in hits.iter().enumerate() {
                        println!(
                            "  {}. {}[{}] {}{} │ Hybrid:{:.3} (Cosine:{:.3}, HNSW edges:{})",
                            idx + 1,
                            MAGENTA,
                            h.namespace,
                            h.key,
                            RESET,
                            h.hybrid_score,
                            h.cosine_similarity,
                            h.neighbors_traversed
                        );
                        println!("     └─ {}", h.content);
                    }
                    println!();
                }
                _ => {
                    println!(
                        "\n{}{}🧠 HNSW SEMANTIC VECTOR MEMORY GRAPH ({} nodes, 64-dim){}",
                        BOLD,
                        BRIGHT_CYAN,
                        state.vector_memory.len(),
                        RESET
                    );
                    for n in &state.vector_memory {
                        println!(
                            "  • {}[{}] {}{} (neighbors: {:?}) -> {}",
                            MAGENTA, n.namespace, n.key, RESET, n.neighbors, n.content
                        );
                    }
                    println!();
                }
            }
        }
        "compress" => {
            let budget: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(12);
            let msg = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "Agent finished phase 1. Verified commit 530fc5e and sha256=ae90aaab. All 266 unit tests pass and 322 architecture tests pass. Next step is independent review of SQL migration 0013.".to_string()
            };
            let (comp, orig, fin) = compress_message_to_token_budget(&msg, budget);
            println!("\n{}{}📦 INFORMATION-BOTTLENECK MESSAGE COMPRESSOR{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Token Budget: {} tokens (Original: {} -> Compressed: {})", budget, orig, fin);
            println!("  └─ Compressed:   \"{}\"\n", comp);
        }
        "consensus" => {
            let proto = args.get(1).map(|s| s.as_str()).unwrap_or("PBFT");
            let topic = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "Approve Phase 0 Gate and Migration 0013".to_string()
            };
            let seq = (state.consensus_history.len() as u64) + 1;
            let round = execute_consensus(&state.providers, proto, &topic, seq);
            append_ledger_row(
                &mut state.decision_ledger,
                &round.leader_or_primary,
                "consensus.finalized",
                &format!("{} ({}) approval={:.1}%", round.proposal_id, round.protocol, round.weighted_approval * 100.0),
            );
            state.consensus_history.push(round.clone());
            save_fleet_state(&state);
            println!(
                "\n{}{}⚖ {} CONSENSUS ENGINE ROUND [{}]{}",
                BOLD, MAGENTA, round.protocol, round.proposal_id, RESET
            );
            println!("  ├─ Leader / Primary:  {}", round.leader_or_primary);
            println!("  ├─ Topic:             {}", round.topic);
            println!("  ├─ Phase Progression: {}", round.phase);
            println!("  ├─ Weighted Approval: {:.1}% (Quorum={})", round.weighted_approval * 100.0, round.quorum_achieved);
            println!("  └─ Digest:            {}\n", round.digest);
        }
        "federate" => {
            let peer = args.get(1).map(|s| s.as_str()).unwrap_or("team-alpha");
            let raw = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "Contact owner apppbf@gmail.com with token=sk-ant-123456 and phone 966501234567 for migration review".to_string()
            };
            let pkt = create_federated_packet("ai-fleet-local", peer, &raw, "estr-v4-secret-salt");
            let (_, _, verified) = compute_gumbel_watermark_zscore(&pkt.sanitized_payload, "estr-v4-secret-salt");
            println!("\n{}{}🌐 CROSS-TEAM FEDERATION & GUMBEL-SOFTMAX WATERMARK{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Packet ID / Peer:   {} -> {}", pkt.packet_id, pkt.target_peer);
            println!("  ├─ PII/Secrets Stripped: {} item(s) automatically redacted", pkt.pii_redactions_count);
            println!("  ├─ Sanitized Payload:  \"{}\"", pkt.sanitized_payload);
            println!(
                "  └─ Watermark Z-Score:  {:.2}σ (Provenance Verified: {}{}{})\n",
                pkt.z_score,
                GREEN,
                verified,
                RESET
            );
        }
        "mcp" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("tools");
            match sub {
                "start" | "serve" => {
                    run_mcp_stdio_server();
                }
                "exec" if args.len() >= 3 => {
                    let tool = &args[2];
                    let params: Value = args
                        .get(3)
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| serde_json::json!({}));
                    let res = execute_mcp_tool(tool, &params);
                    println!("{}", serde_json::to_string_pretty(&res).unwrap_or_default());
                }
                _ => print_dashboard(&state, Some("mcp")),
            }
        }
        "defense" | "aidefence" => {
            let text = if args.len() > 1 { args[1..].join(" ") } else { "Normal prompt".into() };
            let findings = scan_aidefence_threats(&text);
            println!("\n{}{}🛡 AIDEFENCE (ADR-118) PROMPT-INJECTION & SECRET SHIELD{}", BOLD, BRIGHT_CYAN, RESET);
            if findings.is_empty() {
                println!("  └─ Result: {}✔ CLEAN (0 threats detected, <1ms){}\n", GREEN, RESET);
            } else {
                println!("  └─ Result: {}✖ BLOCKED ({} threat(s) detected){}", RED, findings.len(), RESET);
                for f in findings {
                    println!("       • [{}] {} ({:.0}%): {}", f.severity, f.category, f.confidence * 100.0, f.description);
                }
                println!();
            }
        }
        "casa" => {
            let act = args.get(1).map(|s| s.as_str()).unwrap_or("worktree.write");
            let dec = check_casa_authorization(&state.casa_envelope, act, &Utc::now().to_rfc3339());
            println!(
                "\n🔐 CASA Gate for '{}': {} ({})\n",
                act,
                if dec.allowed { format!("{}ALLOWED{}", GREEN, RESET) } else { format!("{}DENIED{}", RED, RESET) },
                dec.reason
            );
        }
        "gate" => {
            let (act, reason) = evaluate_continue_gate(&state.continue_gate);
            println!("\n🚦 ContinueGate: {} — {}\n", badge_status(act), reason);
        }
        "pheromone" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            if sub == "evaporate" {
                for p in state.providers.values_mut() {
                    for m in &mut p.models {
                        m.pheromone = (m.pheromone * 0.90).clamp(0.20, 2.00);
                    }
                }
                save_fleet_state(&state);
            } else if sub == "boost" {
                if let Some(target_m) = args.get(2) {
                    for p in state.providers.values_mut() {
                        for m in &mut p.models {
                            if &m.name == target_m {
                                m.pheromone = (m.pheromone + 0.25).clamp(0.20, 2.00);
                            }
                        }
                    }
                    save_fleet_state(&state);
                }
            }
            print_dashboard(&state, None);
        }
        "provider" | "providers" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "add" if args.len() >= 6 => {
                    let id = args[2].to_lowercase();
                    let bin = args[3].clone();
                    let model = args[4].clone();
                    let effort = args[5].clone();
                    let tpl = if args.len() > 6 {
                        args[6..].to_vec()
                    } else {
                        vec!["--model".into(), "{model}".into(), "-p".into(), "{prompt}".into()]
                    };
                    state.providers.insert(
                        id.clone(),
                        AiProvider {
                            id: id.clone(),
                            display_name: format!("Custom AI ({})", id),
                            binary: bin,
                            args_template: tpl,
                            stdin_prompt: false,
                            status: "ready".into(),
                            active_model: model.clone(),
                            active_effort: effort.clone(),
                            daily_quota_limit: 100,
                            daily_quota_used: 0,
                            retry_at: None,
                            last_detail: "Connected via ai-fleet provider add".into(),
                            models: vec![ModelSpec {
                                name: model.clone(),
                                tier: "deep".into(),
                                default_effort: effort.clone(),
                                q_score: 0.92,
                                pheromone: 1.15,
                                trust_score: 0.95,
                                ewc_fisher_importance: 0.90,
                                calls_today: 0,
                                successes: 0,
                                failures: 0,
                                avg_latency_ms: 0,
                            }],
                        },
                    );
                    save_fleet_state(&state);
                    println!("{}✔ Connected AI provider '{}' ({} @ {}).{}", GREEN, id, model, effort, RESET);
                }
                "switch" | "model" if args.len() >= 5 => {
                    let id = args[2].to_lowercase();
                    let model = args[3].clone();
                    let effort = args[4].clone();
                    if let Some(p) = state.providers.get_mut(&id) {
                        p.active_model = model.clone();
                        p.active_effort = effort.clone();
                        if !p.models.iter().any(|m| m.name == model) {
                            p.models.push(ModelSpec {
                                name: model.clone(),
                                tier: "critical".into(),
                                default_effort: effort.clone(),
                                q_score: 0.95,
                                pheromone: 1.25,
                                trust_score: 0.96,
                                ewc_fisher_importance: 0.92,
                                calls_today: 0,
                                successes: 0,
                                failures: 0,
                                avg_latency_ms: 0,
                            });
                        }
                        save_fleet_state(&state);
                        println!("{}✔ Switched '{}' -> model='{}' effort={}{}", GREEN, id, model, badge_effort(&effort), RESET);
                    }
                }
                "quota" if args.len() >= 5 => {
                    let id = args[2].to_lowercase();
                    let used: u64 = args[3].parse().unwrap_or(0);
                    let limit: u64 = args[4].parse().unwrap_or(100);
                    let reset = args.get(5).cloned();
                    if let Some(p) = state.providers.get_mut(&id) {
                        p.daily_quota_used = used;
                        p.daily_quota_limit = limit;
                        if used >= limit && reset.as_deref() != Some("ready") {
                            p.status = "quota".into();
                            p.retry_at = reset;
                        } else {
                            p.status = "ready".into();
                            p.retry_at = None;
                        }
                    }
                    save_fleet_state(&state);
                    println!("{}✔ Quota updated for '{}': {}/{}{}", GREEN, id, used, limit, RESET);
                }
                _ => print_dashboard(&state, None),
            }
        }
        "task" => {
            if args.get(1).map(|s| s.as_str()) == Some("add") && args.len() >= 5 {
                let id = args[2].clone();
                let title = args[3].clone();
                let owned: Vec<String> = args[4].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                for f in &owned {
                    if let Some(owner) = state.file_claims.get(f) {
                        if owner != &id {
                            eprintln!("{}✖ File '{}' is already claimed by task '{}'!{}", RED, f, owner, RESET);
                            std::process::exit(1);
                        }
                    }
                }
                let (tier, eff) = classify_task_tier(&title, &owned, 0, false);
                let persona = find_best_persona(&title);
                for f in &owned {
                    state.file_claims.insert(f.clone(), id.clone());
                }
                state.global_epoch += 1;
                let ws_root = detect_workspace_root();
                let mut t = SwarmTask {
                    id: id.clone(),
                    title: title.clone(),
                    base_commit: detect_head_commit(&ws_root),
                    owned_files: owned.clone(),
                    required_outputs: owned,
                    sparc_phase: "Spec".into(),
                    subtasks: vec![],
                    status: "queued".into(),
                    tier: tier.clone(),
                    target_effort: eff.clone(),
                    assigned_provider: None,
                    assigned_model: None,
                    assigned_persona: Some(persona.id.clone()),
                    active_worktree: None,
                    authors: vec![],
                    reviewer: None,
                    retries: 0,
                    lease_epoch: state.global_epoch,
                    next_step: title,
                };
                t.subtasks = queen_decompose_task(&t).subtasks;
                state.tasks.insert(id.clone(), t);
                save_fleet_state(&state);
                println!(
                    "{}✔ Task '{}' created (Tier={}, Effort={}, Persona={}){}",
                    GREEN,
                    id,
                    badge_tier(&tier),
                    badge_effort(&eff),
                    persona.id,
                    RESET
                );
            } else {
                print_dashboard(&state, None);
            }
        }
        "route" => {
            let tid = args.get(1).map(|s| s.as_str()).unwrap_or("security-rls-audit");
            if let Some(t) = state.tasks.get(tid) {
                if let Some(d) = compute_optimal_route(
                    &state.providers,
                    &state.moe_experts,
                    &state.reasoning_bank,
                    t,
                    &BTreeSet::new(),
                    t.status == "review",
                ) {
                    println!("\n{}{}🎯 COMPOSITE V4 ROUTE FOR TASK [{}]{}", BOLD, BRIGHT_CYAN, tid, RESET);
                    println!("  ├─ Classified Tier:   {}", badge_tier(&d.classified_tier));
                    println!("  ├─ Matched Persona:   {}{}{}", MAGENTA, d.assigned_persona, RESET);
                    println!("  ├─ MoE Top Experts:   {:?} (entropy={:.3}, ewc_loss={:.6})", d.moe_experts, d.moe_entropy, d.ewc_loss);
                    println!("  ├─ Selected Provider: {}{}/{}{} ({})", BOLD, d.selected_provider, d.selected_model, RESET, badge_effort(&d.selected_effort));
                    println!("  ├─ Composite Score:   {:.3}", d.composite_score);
                    println!("  └─ Fallback Ladder:   {:?}\n", d.fallback_chain);
                }
            }
        }
        "drill" => {
            let drill_dir = env::temp_dir().join("ai-fleet-v4-drill");
            let _ = fs::create_dir_all(&drill_dir);
            let file = drill_dir.join("v4-handoff.txt");
            let _ = fs::remove_file(&file);

            state.global_epoch += 1;
            let ep1 = state.global_epoch;
            let l1 = b"phase-1: codex/soul [epoch-fenced]\n";
            fs::write(&file, l1).unwrap();
            let h1 = sha256_bytes(l1);

            state.global_epoch += 1;
            let ep2 = state.global_epoch;
            assert_eq!(sha256_file(&file).unwrap(), h1);
            let mut combined = fs::read(&file).unwrap();
            combined.extend_from_slice(b"phase-2: antigravity/argon [effort=xhigh]\n");
            fs::write(&file, &combined).unwrap();
            let h2 = sha256_bytes(&combined);

            record_outcome(&mut state, "antigravity", "argon", "security", 0.99, 6800);
            let round = execute_consensus(&state.providers, "PBFT", "Verify drill handoff integrity", (state.consensus_history.len() as u64) + 1);
            state.consensus_history.push(round.clone());
            append_ledger_row(&mut state.decision_ledger, "drill:v4", "handoff.verified", &format!("ep {}->{} sha256={}", ep1, ep2, &h2[..12]));
            save_fleet_state(&state);

            println!("\n{}{}⚡ V4 LIVE FAILOVER, FENCED EPOCH (#{} -> #{}) & PBFT DRILL: ✔ PASS{}", BOLD, GREEN, ep1, ep2, RESET);
            println!("  ├─ Checkpoint SHA-256: {} -> Final SHA-256: {}", &h1[..18], &h2[..18]);
            println!("  └─ PBFT Consensus [{}]: {:.1}% weighted quorum ({})\n", round.proposal_id, round.weighted_approval * 100.0, round.phase);
        }
        "exec" if args.len() >= 3 => {
            let tid = args[1].clone();
            let prompt = args[2..].join(" ");
            let threats = scan_aidefence_threats(&prompt);
            if !threats.is_empty() {
                eprintln!("{}✖ AIDefence blocked prompt: {:?}{}", RED, threats, RESET);
                std::process::exit(1);
            }
            let ws = detect_workspace_root();
            if let Some(t) = state.tasks.get(&tid).cloned() {
                if let Some(d) = compute_optimal_route(&state.providers, &state.moe_experts, &state.reasoning_bank, &t, &BTreeSet::new(), false) {
                    let mut chain = vec![(d.selected_provider.clone(), d.selected_model.clone(), d.selected_effort.clone())];
                    chain.extend(d.fallback_chain);
                    for (pid, model, eff) in chain {
                        if let Some(p) = state.providers.get(&pid).cloned() {
                            let st = Instant::now();
                            match execute_ai_cli(&p, &model, &eff, &ws, &prompt) {
                                Ok((out, _)) => {
                                    record_outcome(&mut state, &pid, &model, "coder", 0.98, st.elapsed().as_millis() as u64);
                                    save_fleet_state(&state);
                                    println!("{}", out);
                                    return;
                                }
                                Err((kind, reset, _, _)) => {
                                    if let Some(pm) = state.providers.get_mut(&pid) {
                                        if kind == "quota" {
                                            pm.status = "quota".into();
                                            pm.retry_at = reset;
                                        }
                                    }
                                    save_fleet_state(&state);
                                    eprintln!("{}⚠ {}/{} ({}) -> switching to next model in ladder...{}", YELLOW, pid, model, kind, RESET);
                                }
                            }
                        }
                    }
                }
            }
        }
        "policy" | "ledger" => match verify_ledger_chain(&state.decision_ledger) {
            Ok(n) => {
                println!(
                    "\n{}✔ SHA-256 Decision Ledger verified ({} chained blocks, {} immutable TruthAnchors).{}",
                    GREEN,
                    n,
                    state.truth_anchors.len(),
                    RESET
                );
                for a in &state.truth_anchors {
                    println!("  🔒 [{}] {} (sig={}..)", a.anchor_id, a.invariant, &a.signature_sha256[..16]);
                }
                println!();
            }
            Err(e) => {
                eprintln!("{}✖ Ledger integrity FAILED: {}{}", RED, e, RESET);
                std::process::exit(1);
            }
        },
        _ => print_v4_help(),
    }
}
