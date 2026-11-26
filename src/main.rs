use ai_fleet::consensus::{append_ledger, run_pbft_consensus, verify_ledger};
use ai_fleet::domain::{
    detect_head_commit, detect_workspace_root, load_or_init_state, save_state, AiProvider, MemoryEntry, ModelSpec,
    SwarmTask,
};
use ai_fleet::executor::{execute_provider_cli, run_failover_simulation, run_parallel_cycle};
use ai_fleet::router::{
    classify_task_tier, compute_optimal_route, route_moe_experts, update_learning_and_pheromones,
};
use ai_fleet::security::{check_casa_authorization, evaluate_continue_gate, scan_aidefence_threats};
use ai_fleet::tui::{
    badge_effort, badge_status, badge_tier, enable_ansi_support, print_dashboard, print_help, render_float_bar, BOLD,
    BRIGHT_CYAN, GREEN, MAGENTA, RED, RESET, WHITE, YELLOW,
};
use chrono::Utc;
use std::collections::BTreeSet;
use std::env;
use std::time::Instant;

fn main() {
    enable_ansi_support();
    let args: Vec<String> = env::args().skip(1).collect();
    let mut state = load_or_init_state();

    if args.is_empty() {
        print_dashboard(&state);
        return;
    }

    match args[0].as_str() {
        "status" | "dashboard" | "tui" => {
            if let Some(pos) = args.iter().position(|a| a == "--look") {
                if let Some(look) = args.get(pos + 1) {
                    state.look_mode = look.clone();
                    save_state(&state);
                }
            }
            print_dashboard(&state);
        }
        "start" | "run" => {
            state.daemon_running = true;
            state.daemon_pid = Some(std::process::id());
            append_ledger(&mut state, "owner:cli", "fleet.start", "Started parallel multi-AI orchestrator v2");
            save_state(&state);
            run_parallel_cycle(&mut state);
            print_dashboard(&state);
        }
        "stop" => {
            state.daemon_running = false;
            state.daemon_pid = None;
            append_ledger(
                &mut state,
                "owner:cli",
                "fleet.stop",
                "Drained active writers and stopped orchestrator safely",
            );
            save_state(&state);
            println!(
                "{}✔ Fleet stopped safely. All FencedLeases, checkpoints, and file claims preserved.{}",
                GREEN, RESET
            );
        }
        "drill" | "test-failover" => {
            run_failover_simulation(&mut state);
        }
        "moe" => {
            let query = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "Audit SQL migration money scale, RLS policies, and verify golden tests".to_string()
            };
            let res = route_moe_experts(&state.moe_experts, &query, 4);
            println!("\n{}{}🧠 8-EXPERT MIXTURE-OF-EXPERTS (MoE) GATING RESULT{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Query:             \"{}\"", query);
            println!("  ├─ Routing Entropy:   {:.4} nats", res.entropy);
            println!("  ├─ Load-Balance Loss: {:.4}", res.load_balance_loss);
            println!("  └─ Top-4 Activated Experts:");
            for (idx, (name, prob)) in res.top_experts.iter().enumerate() {
                println!(
                    "       {}. {:<13} {}",
                    idx + 1,
                    name,
                    render_float_bar(*prob, 1.0, 14, GREEN)
                );
            }
            println!();
        }
        "pheromone" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            if sub == "evaporate" {
                for p in state.providers.values_mut() {
                    for m in &mut p.models {
                        m.pheromone = (m.pheromone * 0.90).clamp(0.20, 2.00);
                    }
                }
                save_state(&state);
                println!("{}✔ Applied ADR-330 pheromone evaporation cycle (0.90x) across all models.{}", GREEN, RESET);
            } else if sub == "boost" {
                if let Some(target_m) = args.get(2) {
                    for p in state.providers.values_mut() {
                        for m in &mut p.models {
                            if &m.name == target_m {
                                m.pheromone = (m.pheromone + 0.25).clamp(0.20, 2.00);
                            }
                        }
                    }
                    save_state(&state);
                    println!("{}✔ Boosted pheromone trail for model '{}'.{}", GREEN, target_m, RESET);
                }
            }
            println!("\n{}{}🐜 ADR-330 PHEROMONE-ADAPTIVE SCHEDULING TRAILS{}", BOLD, MAGENTA, RESET);
            for (pid, p) in &state.providers {
                for m in &p.models {
                    println!(
                        "  ├─ {:<12} / {:<18} │ Pheromone: {} │ Q-Score: {:.3} │ Trust: {:.2}",
                        pid,
                        m.name,
                        render_float_bar(m.pheromone, 2.0, 12, MAGENTA),
                        m.q_score,
                        m.trust_score
                    );
                }
            }
            println!();
        }
        "defense" | "aidefence" => {
            let text = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "Normal task prompt".to_string()
            };
            let findings = scan_aidefence_threats(&text);
            println!("\n{}{}🛡 AIDEFENCE (ADR-118) PROMPT-INJECTION & SECRET SHIELD{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Scanned Payload: \"{}\"", text);
            if findings.is_empty() {
                println!("  └─ Result:          {}✔ CLEAN (0 threats detected, <1ms){}\n", GREEN, RESET);
            } else {
                println!("  └─ Result:          {}✖ BLOCKED ({} threat(s) detected){}", RED, findings.len(), RESET);
                for f in findings {
                    println!(
                        "       • [{}] {} (confidence={:.0}%): {}",
                        f.severity,
                        f.category,
                        f.confidence * 100.0,
                        f.description
                    );
                }
                println!();
            }
        }
        "casa" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("worktree.write");
            let dec = check_casa_authorization(&state.casa_envelope, action, &Utc::now().to_rfc3339());
            println!("\n{}{}🔐 CASA (ADR-380 §3) DETERMINISTIC AUTHORITY GATE{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Requested Action: {}", action);
            println!(
                "  ├─ Decision:         {}",
                if dec.allowed {
                    format!("{}✔ ALLOWED{}", GREEN, RESET)
                } else {
                    format!("{}✖ DENIED{}", RED, RESET)
                }
            );
            println!("  └─ Reason:           {}\n", dec.reason);
        }
        "consensus" => {
            let topic = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "Approve architecture & migration safety gate".to_string()
            };
            let round = run_pbft_consensus(&mut state, &topic);
            println!(
                "\n{}{}⚖ BYZANTINE FAULT TOLERANT (PBFT) CONSENSUS ROUND [{}]{}",
                BOLD, MAGENTA, round.proposal_id, RESET
            );
            println!("  ├─ Topic:             {}", round.topic);
            println!("  ├─ Phase Progression: pre-prepare -> prepare -> {}", round.phase);
            println!(
                "  ├─ Weighted Approval: {:.1}% (Quorum threshold: 66.7%)",
                round.weighted_approval * 100.0
            );
            println!("  ├─ Digest:            {}", round.digest);
            println!("  └─ Model Votes:");
            for (voter, (ok, w)) in &round.votes {
                println!(
                    "       • {:<32} : {} (weight={:.3})",
                    voter,
                    if *ok {
                        format!("{}APPROVE{}", GREEN, RESET)
                    } else {
                        format!("{}REJECT{}", RED, RESET)
                    },
                    w
                );
            }
            println!();
        }
        "gate" => {
            let (act, reason) = evaluate_continue_gate(&state.continue_gate);
            println!("\n{}{}🚦 CONTINUE-GATE LONG-RUNNING AGENT CONTROLLER{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Decision:          {}", badge_status(act));
            println!("  ├─ Reason:            {}", reason);
            println!(
                "  └─ Invariants:        steps={}/{} | coherence={:.2} | uncertainty={:.2} | rework={:.2} | slope={:.3}\n",
                state.continue_gate.consecutive_steps,
                state.continue_gate.max_consecutive_steps,
                state.continue_gate.coherence_score,
                state.continue_gate.uncertainty_score,
                state.continue_gate.rework_ratio,
                state.continue_gate.budget_slope
            );
        }
        "patterns" => {
            println!("\n{}{}📚 REASONING-BANK & SONA LEARNED PATTERNS{}", BOLD, BRIGHT_CYAN, RESET);
            for p in &state.reasoning_bank {
                println!(
                    "  {}◆ [{}]{} Domain: {}{}{} │ Success: {:.0}% ({} runs) │ Effort: {}",
                    BOLD,
                    p.pattern_id,
                    RESET,
                    WHITE,
                    p.domain,
                    RESET,
                    p.success_rate * 100.0,
                    p.uses,
                    badge_effort(&p.recommended_effort)
                );
                println!("    ├─ Strategy: {}", p.strategy);
                for l in &p.key_learnings {
                    println!("    └─ Learning: {}", l);
                }
            }
            println!();
        }
        "exec" => {
            if args.len() < 3 {
                eprintln!("{}Usage: ai-fleet exec <task_id> <prompt>{}", RED, RESET);
                std::process::exit(1);
            }
            let tid = args[1].clone();
            let prompt = args[2..].join(" ");

            let threats = scan_aidefence_threats(&prompt);
            if !threats.is_empty() {
                eprintln!(
                    "{}✖ AIDefence blocked prompt due to {} threat finding(s):{}",
                    RED,
                    threats.len(),
                    RESET
                );
                for t in threats {
                    eprintln!("  • [{}] {}: {}", t.severity, t.category, t.description);
                }
                std::process::exit(1);
            }

            let casa_dec = check_casa_authorization(&state.casa_envelope, "worktree.write", &Utc::now().to_rfc3339());
            if !casa_dec.allowed {
                eprintln!("{}✖ CASA denied execution: {}{}", RED, casa_dec.reason, RESET);
                std::process::exit(1);
            }

            let ws = detect_workspace_root();
            let busy = BTreeSet::new();
            let route = state
                .tasks
                .get(&tid)
                .and_then(|t| compute_optimal_route(&state, t, &busy, t.status == "review"));
            if let Some(d) = route {
                let mut chain = vec![(
                    d.selected_provider.clone(),
                    d.selected_model.clone(),
                    d.selected_effort.clone(),
                )];
                chain.extend(d.fallback_chain.clone());
                for (pid, model, effort) in chain {
                    if let Some(p) = state.providers.get(&pid).cloned() {
                        println!(
                            "{}⚡ Executing on {}/{} (effort={})...{}",
                            BRIGHT_CYAN, pid, model, effort, RESET
                        );
                        let started = Instant::now();
                        match execute_provider_cli(&p, &model, &effort, &ws, &prompt) {
                            Ok(out) => {
                                let ms = started.elapsed().as_millis() as u64;
                                update_learning_and_pheromones(&mut state, &pid, &model, 0.98, ms);
                                append_ledger(
                                    &mut state,
                                    &format!("{}/{}", pid, model),
                                    "task.exec.ok",
                                    &format!("Task {} completed in {}ms", tid, ms),
                                );
                                save_state(&state);
                                println!("{}\n✔ Completed by {}/{} in {}ms{}", GREEN, pid, model, ms, RESET);
                                println!("{}", out);
                                return;
                            }
                            Err((kind, reset, err_msg)) => {
                                update_learning_and_pheromones(&mut state, &pid, &model, 0.25, 1000);
                                if let Some(pm) = state.providers.get_mut(&pid) {
                                    if kind == "quota" || kind == "auth" || kind == "network" {
                                        pm.status = kind.clone();
                                        pm.retry_at = reset;
                                        pm.last_detail = err_msg.lines().next().unwrap_or("CLI error").to_string();
                                    }
                                }
                                save_state(&state);
                                eprintln!(
                                    "{}⚠ {}/{} failed ({}): switching automatically to next model/provider in ladder...{}",
                                    YELLOW, pid, model, kind, RESET
                                );
                            }
                        }
                    }
                }
            } else {
                eprintln!("{}No eligible route for task '{}'.{}", RED, tid, RESET);
            }
        }
        "provider" | "providers" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "models" | "usage" => {
                    print_dashboard(&state);
                }
                "add" => {
                    if args.len() < 6 {
                        eprintln!(
                            "{}Usage: ai-fleet provider add <id> <binary> <default_model> <effort>{}",
                            RED, RESET
                        );
                        std::process::exit(1);
                    }
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
                            status: "ready".to_string(),
                            active_model: model.clone(),
                            active_effort: effort.clone(),
                            daily_quota_limit: 100,
                            daily_quota_used: 0,
                            retry_at: None,
                            last_detail: "Connected via ai-fleet provider add".to_string(),
                            models: vec![ModelSpec {
                                name: model.clone(),
                                tier: "deep".to_string(),
                                default_effort: effort.clone(),
                                q_score: 0.92,
                                pheromone: 1.15,
                                trust_score: 0.95,
                                calls_today: 0,
                                successes: 0,
                                failures: 0,
                                avg_latency_ms: 0,
                            }],
                        },
                    );
                    append_ledger(
                        &mut state,
                        "owner:cli",
                        "provider.add",
                        &format!("Added AI provider {} ({})", id, model),
                    );
                    save_state(&state);
                    println!(
                        "{}✔ Connected AI provider '{}' with model '{}' (effort={}).{}",
                        GREEN, id, model, effort, RESET
                    );
                }
                "switch" | "model" => {
                    if args.len() < 5 {
                        eprintln!(
                            "{}Usage: ai-fleet provider switch <provider_id> <model_name> <effort>{}",
                            RED, RESET
                        );
                        std::process::exit(1);
                    }
                    let id = args[2].to_lowercase();
                    let model = args[3].clone();
                    let effort = args[4].clone();
                    if let Some(p) = state.providers.get_mut(&id) {
                        p.active_model = model.clone();
                        p.active_effort = effort.clone();
                        if !p.models.iter().any(|m| m.name == model) {
                            p.models.push(ModelSpec {
                                name: model.clone(),
                                tier: "critical".to_string(),
                                default_effort: effort.clone(),
                                q_score: 0.95,
                                pheromone: 1.25,
                                trust_score: 0.96,
                                calls_today: 0,
                                successes: 0,
                                failures: 0,
                                avg_latency_ms: 0,
                            });
                        }
                        append_ledger(
                            &mut state,
                            "owner:cli",
                            "provider.switch",
                            &format!("Switched {} to model={} effort={}", id, model, effort),
                        );
                        save_state(&state);
                        println!(
                            "{}✔ Switched '{}' -> model='{}' | effort={}{}",
                            GREEN,
                            id,
                            model,
                            badge_effort(&effort),
                            RESET
                        );
                    } else {
                        eprintln!("{}Provider '{}' not found.{}", RED, id, RESET);
                        std::process::exit(1);
                    }
                }
                "quota" => {
                    if args.len() < 5 {
                        eprintln!(
                            "{}Usage: ai-fleet provider quota <id> <used> <limit> [reset_iso8601|ready]{}",
                            RED, RESET
                        );
                        std::process::exit(1);
                    }
                    let id = args[2].to_lowercase();
                    let used: u64 = args[3].parse().unwrap_or(0);
                    let limit: u64 = args[4].parse().unwrap_or(100);
                    let reset = args.get(5).cloned();
                    let new_status = if let Some(p) = state.providers.get_mut(&id) {
                        p.daily_quota_used = used;
                        p.daily_quota_limit = limit;
                        if used >= limit && reset.as_deref() != Some("ready") {
                            p.status = "quota".to_string();
                            p.retry_at = reset;
                        } else {
                            p.status = "ready".to_string();
                            p.retry_at = None;
                        }
                        Some(p.status.clone())
                    } else {
                        None
                    };
                    if let Some(st_label) = new_status {
                        save_state(&state);
                        println!(
                            "{}✔ Updated quota for '{}': {}/{} (status={}){}",
                            GREEN, id, used, limit, st_label, RESET
                        );
                    }
                }
                _ => print_help(),
            }
        }
        "task" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "add" || sub == "create" {
                if args.len() < 5 {
                    eprintln!(
                        "{}Usage: ai-fleet task add <id> <title> <comma_separated_owned_files>{}",
                        RED, RESET
                    );
                    std::process::exit(1);
                }
                let id = args[2].clone();
                let title = args[3].clone();
                let owned: Vec<String> = args[4]
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                for f in &owned {
                    if let Some(existing_owner) = state.file_claims.get(f) {
                        if existing_owner != &id {
                            eprintln!(
                                "{}✖ File '{}' is already claimed by task '{}'! FencedLease prevents conflict.{}",
                                RED, f, existing_owner, RESET
                            );
                            std::process::exit(1);
                        }
                    }
                }
                let (tier, effort) = classify_task_tier(&title, &owned, 0, false);
                for f in &owned {
                    state.file_claims.insert(f.clone(), id.clone());
                }
                state.global_epoch += 1;
                let ws_root = detect_workspace_root();
                state.tasks.insert(
                    id.clone(),
                    SwarmTask {
                        id: id.clone(),
                        title: title.clone(),
                        base_commit: detect_head_commit(&ws_root),
                        owned_files: owned.clone(),
                        required_outputs: owned,
                        steps: vec![title.clone()],
                        current_step: 0,
                        status: "queued".to_string(),
                        tier: tier.clone(),
                        target_effort: effort.clone(),
                        assigned_provider: None,
                        assigned_model: None,
                        active_worktree: None,
                        authors: vec![],
                        reviewer: None,
                        retries: 0,
                        lease_epoch: state.global_epoch,
                        checkpoint: None,
                        next_step: title,
                        history: vec![],
                        ruflo_id: None,
                    },
                );
                append_ledger(
                    &mut state,
                    "owner:cli",
                    "task.create",
                    &format!("Created task {} ({}/{})", id, tier, effort),
                );
                save_state(&state);
                println!(
                    "{}✔ Created task '{}' (Epoch #{}) -> Auto-Classified Tier: {} | Target Effort: {}{}",
                    GREEN,
                    id,
                    state.global_epoch,
                    badge_tier(&tier),
                    badge_effort(&effort),
                    RESET
                );
            } else {
                print_dashboard(&state);
            }
        }
        "route" => {
            let tid = args.get(1).map(|s| s.as_str()).unwrap_or("critical-guard");
            if let Some(t) = state.tasks.get(tid) {
                let busy = BTreeSet::new();
                if let Some(d) = compute_optimal_route(&state, t, &busy, t.status == "review") {
                    println!(
                        "\n{}{}🎯 MOE + Q-LEARNING + PHEROMONE ROUTE FOR TASK [{}]{}",
                        BOLD, BRIGHT_CYAN, tid, RESET
                    );
                    println!("  ├─ Classified Tier:   {}", badge_tier(&d.classified_tier));
                    println!(
                        "  ├─ MoE Top Experts:   {} (entropy={:.3})",
                        d.moe_experts
                            .iter()
                            .map(|(n, w)| format!("{} ({:.1}%)", n, w * 100.0))
                            .collect::<Vec<_>>()
                            .join(" + "),
                        d.moe_entropy
                    );
                    if let Some(ref pat) = d.matched_pattern {
                        println!("  ├─ ReasoningBank Hit: {}{}{}", MAGENTA, pat, RESET);
                    }
                    println!("  ├─ Selected Provider: {}{}{}", BOLD, d.selected_provider, RESET);
                    println!("  ├─ Selected Model:    {}{}{}", GREEN, d.selected_model, RESET);
                    println!("  ├─ Reasoning Effort:  {}", badge_effort(&d.selected_effort));
                    println!("  ├─ Composite Score:   {:.3}", d.composite_score);
                    println!("  └─ Automatic Fallback Ladder:");
                    for (idx, (fp, fm, fe)) in d.fallback_chain.iter().enumerate() {
                        println!("       {}. {}/{} (effort={})", idx + 1, fp, fm, badge_effort(fe));
                    }
                    println!();
                } else {
                    println!("{}No ready provider available right now.{}", YELLOW, RESET);
                }
            } else {
                eprintln!("{}Task '{}' not found.{}", RED, tid, RESET);
            }
        }
        "swarm" => {
            if let Some(top) = args.get(1) {
                state.topology = top.clone();
                if let Some(ma) = args.get(2).and_then(|s| s.parse::<usize>().ok()) {
                    state.max_agents = ma;
                }
                let top_note = format!("Topology={}", state.topology);
                append_ledger(&mut state, "owner:cli", "swarm.topology", &top_note);
                save_state(&state);
                println!(
                    "{}✔ Swarm topology updated to '{}' (max_agents={}).{}",
                    GREEN, state.topology, state.max_agents, RESET
                );
            } else {
                print_dashboard(&state);
            }
        }
        "autopilot" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "enable" | "on" => {
                    state.autopilot_enabled = true;
                    save_state(&state);
                    println!("{}✔ Autopilot persistent completion ENABLED.{}", GREEN, RESET);
                }
                "disable" | "off" => {
                    state.autopilot_enabled = false;
                    save_state(&state);
                    println!("{}✔ Autopilot DISABLED.{}", YELLOW, RESET);
                }
                _ => {
                    println!(
                        "Autopilot is currently: {}",
                        if state.autopilot_enabled { "ENABLED" } else { "DISABLED" }
                    );
                }
            }
        }
        "budget" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "pause" => {
                    state.budget_paused = true;
                    save_state(&state);
                    println!("{}⏸ Global AI launch budget PAUSED (circuit-breaker locked).{}", YELLOW, RESET);
                }
                "resume" => {
                    state.budget_paused = false;
                    save_state(&state);
                    println!("{}▶ Global AI launch budget RESUMED.{}", GREEN, RESET);
                }
                _ => print_dashboard(&state),
            }
        }
        "memory" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "set" if args.len() >= 4 => {
                    let key = args[2].clone();
                    let val = args[3..].join(" ");
                    state.memory.insert(
                        key.clone(),
                        MemoryEntry {
                            key: key.clone(),
                            namespace: "global".to_string(),
                            value: val.clone(),
                            tags: vec!["cli".into()],
                            updated_at: Utc::now().to_rfc3339(),
                        },
                    );
                    save_state(&state);
                    println!("{}✔ Stored in shared hive memory: {} = {}{}", GREEN, key, val, RESET);
                }
                "get" if args.len() >= 3 => {
                    if let Some(m) = state.memory.get(&args[2]) {
                        println!("{} = {}", m.key, m.value);
                    } else {
                        println!("Key not found.");
                    }
                }
                _ => {
                    for m in state.memory.values() {
                        println!("[{}] {} = {}", m.namespace, m.key, m.value);
                    }
                }
            }
        }
        "policy" | "ledger" => match verify_ledger(&state) {
            Ok(n) => {
                println!(
                    "{}✔ SHA-256 Decision Ledger integrity verified: {} hash-chained entries, 0 tampering findings.{}",
                    GREEN, n, RESET
                );
                for row in state.decision_ledger.iter().rev().take(6) {
                    println!(
                        "  #{} [{}] {} -> {} ({}) | hash={}..",
                        row.seq,
                        row.timestamp,
                        row.actor,
                        row.action,
                        row.payload_summary,
                        &row.row_hash[..16]
                    );
                }
            }
            Err(e) => {
                eprintln!("{}✖ Ledger integrity FAILED: {}{}", RED, e, RESET);
                std::process::exit(1);
            }
        },
        _ => print_help(),
    }
}
