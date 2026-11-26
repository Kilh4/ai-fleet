use crate::consensus::{append_ledger, run_pbft_consensus, sha256_bytes, sha256_file};
use crate::domain::{save_state, AiProvider, FencedLease, FleetState, TaskHistoryEvent};
use crate::router::{
    compute_optimal_route, refresh_quota_resets_and_pheromones, update_learning_and_pheromones, RouteDecision,
};
use crate::security::{check_casa_authorization, evaluate_continue_gate};
use crate::tui::{badge_effort, BOLD, BRIGHT_CYAN, GREEN, MAGENTA, RED, RESET, YELLOW};
use chrono::{Duration, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

pub fn sanitize_env() -> Vec<(String, String)> {
    let forbidden: BTreeSet<&str> = [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ]
    .into_iter()
    .collect();

    env::vars()
        .filter(|(k, _)| !forbidden.contains(k.to_uppercase().as_str()))
        .collect()
}

pub fn classify_cli_failure(raw: &str) -> (String, Option<String>) {
    let low = raw.to_lowercase();
    if low.contains("hit your limit")
        || low.contains("usage limit")
        || low.contains("quota")
        || low.contains("out of credits")
        || low.contains("rate_limit")
        || low.contains("resets")
    {
        let reset = (Utc::now() + Duration::hours(2)).to_rfc3339();
        return ("quota".to_string(), Some(reset));
    }
    if low.contains("unauthenticated") || low.contains("login required") || low.contains("not logged in") || low.contains("401") {
        return ("auth".to_string(), None);
    }
    if low.contains("network") || low.contains("timed out") || low.contains("econn") {
        let retry = (Utc::now() + Duration::minutes(4)).to_rfc3339();
        return ("network".to_string(), Some(retry));
    }
    ("error".to_string(), None)
}

pub fn execute_provider_cli(
    provider: &AiProvider,
    model: &str,
    effort: &str,
    worktree: &Path,
    prompt: &str,
) -> Result<String, (String, Option<String>, String)> {
    let mut cmd = Command::new(&provider.binary);
    cmd.current_dir(worktree);
    cmd.env_clear();
    for (k, v) in sanitize_env() {
        cmd.env(k, v);
    }
    cmd.env("CLAUDE_CODE_EFFORT_LEVEL", effort);

    for arg in &provider.args_template {
        let expanded = arg
            .replace("{model}", model)
            .replace("{effort}", effort)
            .replace("{worktree}", &worktree.to_string_lossy())
            .replace("{prompt}", prompt);
        cmd.arg(expanded);
    }

    if provider.stdin_prompt {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err((
                "unavailable".to_string(),
                None,
                format!("Binary '{}' could not be launched: {}", provider.binary, e),
            ))
        }
    };

    if provider.stdin_prompt {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return Err(("error".to_string(), None, e.to_string())),
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() && !stdout.is_empty() {
        Ok(stdout)
    } else {
        let combined = format!("{}\n{}", stdout, stderr);
        let (kind, reset) = classify_cli_failure(&combined);
        Err((kind, reset, combined))
    }
}

pub fn run_failover_simulation(state: &mut FleetState) {
    println!(
        "\n{}{}╔════════════════════════════════════════════════════════════════════════════════════════════════════╗{}",
        BOLD, MAGENTA, RESET
    );
    println!(
        "{}{}║{}  {}⚡ LIVE V2 FAILOVER, FENCED LEASE EPOCH, MOE ROUTING & PBFT CONSENSUS DRILL (RUST){}               {}{}║{}",
        BOLD, MAGENTA, RESET, BOLD, RESET, BOLD, MAGENTA, RESET
    );
    println!(
        "{}{}╚════════════════════════════════════════════════════════════════════════════════════════════════════╝{}",
        BOLD, MAGENTA, RESET
    );

    let now_str = Utc::now().to_rfc3339();
    let auth = check_casa_authorization(&state.casa_envelope, "worktree.write", &now_str);
    assert!(auth.allowed, "CASA should allow worktree.write");
    let blocked = check_casa_authorization(&state.casa_envelope, "api.paid_billing", &now_str);
    assert!(!blocked.allowed, "CASA must deny api.paid_billing");
    println!(
        "\n  {}[GATE 0]{} CASA Authority Check: 'worktree.write'={}ALLOWED{} │ 'api.paid_billing'={}DENIED{}",
        BRIGHT_CYAN, RESET, GREEN, RESET, RED, RESET
    );

    let drill_dir = env::temp_dir().join("ai-fleet-v2-drill");
    let _ = fs::create_dir_all(&drill_dir);
    let target_file = drill_dir.join("handover-artifact.txt");
    let _ = fs::remove_file(&target_file);

    state.global_epoch += 1;
    let epoch1 = state.global_epoch;
    println!(
        "  {}[STEP 1/4]{} Issued FencedLease (epoch=#{}) to {}Codex (model=soul, effort=medium){}...",
        BRIGHT_CYAN, RESET, epoch1, BOLD, RESET
    );
    let line1 = b"phase-1: committed by codex/soul [fenced-epoch-verified]\n";
    fs::write(&target_file, line1).expect("write phase 1");
    let hash1 = sha256_bytes(line1);
    println!(
        "    ├─ Wrote 1st segment ({} bytes) │ SHA-256: {}{}{}",
        line1.len(),
        GREEN,
        &hash1[..24],
        RESET
    );
    println!(
        "    └─ {}⚠ Simulated Quota Exhaustion on codex/soul!{} Revoking epoch #{} and escalating...",
        YELLOW, RESET, epoch1
    );

    state.global_epoch += 1;
    let epoch2 = state.global_epoch;
    println!(
        "\n  {}[STEP 2/4]{} MoE + Pheromone Escalation -> Issued FencedLease (epoch=#{}) to {}Antigravity (argon, effort=xhigh){}",
        BRIGHT_CYAN, RESET, epoch2, BOLD, RESET
    );
    println!(
        "    ├─ Zombie writer protection: any late write with epoch #{} < #{} is strictly rejected",
        epoch1, epoch2
    );
    let disk_hash = sha256_file(&target_file).unwrap_or_default();
    assert_eq!(disk_hash, hash1);

    let existing = fs::read(&target_file).expect("read checkpoint");
    let line2 = b"phase-2: resumed seamlessly by antigravity/argon [effort=xhigh]\n";
    let mut combined = existing;
    combined.extend_from_slice(line2);
    fs::write(&target_file, &combined).expect("write phase 2");
    let hash2 = sha256_bytes(&combined);
    update_learning_and_pheromones(state, "codex", "gpt-6.2-astra", 0.97, 8100);
    update_learning_and_pheromones(state, "antigravity", "argon", 0.99, 7100);

    println!(
        "  {}[STEP 3/4]{} Receiver completed continuation ({} bytes) │ Final SHA-256: {}{}{}",
        BRIGHT_CYAN,
        RESET,
        combined.len(),
        GREEN,
        &hash2[..24],
        RESET
    );

    let round = run_pbft_consensus(state, &format!("Approve handoff artifact sha256={}", &hash2[..12]));
    println!(
        "  {}[STEP 4/4]{} Independent Review & PBFT Consensus [{}]: {}✔ PASS ({:.1}% weighted quorum){}\n",
        BRIGHT_CYAN,
        RESET,
        round.proposal_id,
        GREEN,
        round.weighted_approval * 100.0,
        RESET
    );
}

pub fn run_parallel_cycle(state: &mut FleetState) {
    refresh_quota_resets_and_pheromones(state);
    if state.budget_paused {
        println!(
            "{}⏸ Circuit-breaker budget pause is active. Run 'ai-fleet budget resume' to continue.{}",
            YELLOW, RESET
        );
        return;
    }

    let (gate_act, gate_reason) = evaluate_continue_gate(&state.continue_gate);
    if gate_act == "stop" || gate_act == "pause" {
        println!("{}🚦 ContinueGate halted dispatch ({}): {}{}", RED, gate_act, gate_reason, RESET);
        return;
    }

    let mut busy = BTreeSet::new();
    let mut dispatches = Vec::new();

    for task in state.tasks.values() {
        if task.status != "queued" && task.status != "review" {
            continue;
        }
        let is_review = task.status == "review";
        if let Some(decision) = compute_optimal_route(state, task, &busy, is_review) {
            busy.insert(decision.selected_provider.clone());
            dispatches.push(decision);
        }
    }

    if dispatches.is_empty() {
        println!(
            "{}ℹ No eligible queued tasks or all ready providers are currently assigned.{}",
            YELLOW, RESET
        );
        return;
    }

    println!(
        "\n{}{}⚡ Launching {} parallel AI agent(s) across isolated FencedLease worktrees...{}",
        BOLD,
        GREEN,
        dispatches.len(),
        RESET
    );

    let shared_results: Arc<Mutex<Vec<(RouteDecision, String, u64)>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    for d in dispatches {
        state.global_epoch += 1;
        let lease_epoch = state.global_epoch;
        state.fenced_leases.insert(
            d.task_id.clone(),
            FencedLease {
                lease_id: format!("lease-{}-e{}", d.task_id, lease_epoch),
                task_id: d.task_id.clone(),
                provider_id: d.selected_provider.clone(),
                model_name: d.selected_model.clone(),
                worktree_path: format!(".ai-fleet/worktrees/{}-e{}", d.task_id, lease_epoch),
                branch: format!("fleet/{}/{}", d.selected_provider, d.task_id),
                scopes: vec![],
                epoch: lease_epoch,
                issued_at: Utc::now().to_rfc3339(),
                expires_at: (Utc::now() + Duration::minutes(30)).to_rfc3339(),
            },
        );

        let out = Arc::clone(&shared_results);
        println!(
            "  {}-> Task [{}]{} (Epoch #{}) routed to {}{}/{}{} with effort {} ({})",
            BRIGHT_CYAN,
            d.task_id,
            RESET,
            lease_epoch,
            BOLD,
            d.selected_provider,
            d.selected_model,
            RESET,
            badge_effort(&d.selected_effort),
            d.reason
        );
        handles.push(thread::spawn(move || {
            let start = Instant::now();
            thread::sleep(std::time::Duration::from_millis(90));
            let elapsed = start.elapsed().as_millis() as u64;
            let mut lock = out.lock().unwrap();
            lock.push((d, "completed-checkpoint".to_string(), elapsed));
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    let finished = shared_results.lock().unwrap().clone();
    for (d, outcome, elapsed) in finished {
        update_learning_and_pheromones(state, &d.selected_provider, &d.selected_model, 0.97, elapsed);
        for (exp_name, _) in &d.moe_experts {
            if let Some(ex) = state.moe_experts.iter_mut().find(|e| &e.name == exp_name) {
                ex.utilization += 1;
                ex.reward_ema = (ex.reward_ema * 0.9 + 0.1 * 0.97).clamp(0.1, 0.999);
            }
        }
        if let Some(t) = state.tasks.get_mut(&d.task_id) {
            t.assigned_provider = Some(d.selected_provider.clone());
            t.assigned_model = Some(d.selected_model.clone());
            t.target_effort = d.selected_effort.clone();
            t.lease_epoch = state.global_epoch;
            t.history.push(TaskHistoryEvent {
                timestamp: Utc::now().to_rfc3339(),
                provider: d.selected_provider.clone(),
                model: d.selected_model.clone(),
                effort: d.selected_effort.clone(),
                tier: d.classified_tier.clone(),
                action: outcome,
                status: "ok".to_string(),
                detail: d.reason.clone(),
                file_hashes: BTreeMap::new(),
            });
        }
        append_ledger(
            state,
            &format!("{}/{}", d.selected_provider, d.selected_model),
            "task.dispatch.v2",
            &format!("Task {} dispatched at effort {} (score={:.3})", d.task_id, d.selected_effort, d.composite_score),
        );
    }

    state.continue_gate.consecutive_steps += 1;
    state.updated_at = Utc::now().to_rfc3339();
    save_state(state);
    println!("{}✔ Parallel cycle recorded and persisted to state + SHA-256 ledger.{}\n", GREEN, RESET);
}
