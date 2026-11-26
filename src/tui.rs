use crate::consensus::verify_ledger;
use crate::domain::FleetState;
use crate::router::compute_optimal_route;
use crate::security::evaluate_continue_gate;
use std::collections::BTreeSet;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const CYAN: &str = "\x1b[36m";
pub const BRIGHT_CYAN: &str = "\x1b[96m";
pub const GREEN: &str = "\x1b[92m";
pub const YELLOW: &str = "\x1b[93m";
pub const RED: &str = "\x1b[91m";
pub const MAGENTA: &str = "\x1b[95m";
pub const BLUE: &str = "\x1b[94m";
pub const WHITE: &str = "\x1b[97m";

#[cfg(windows)]
pub fn enable_ansi_support() {
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetStdHandle(nStdHandle: u32) -> *mut core::ffi::c_void;
            fn GetConsoleMode(hConsoleHandle: *mut core::ffi::c_void, lpMode: *mut u32) -> i32;
            fn SetConsoleMode(hConsoleHandle: *mut core::ffi::c_void, dwMode: u32) -> i32;
            fn SetConsoleOutputCP(wCodePageID: u32) -> i32;
        }
        SetConsoleOutputCP(65001);
        let handle = GetStdHandle((-11i32) as u32);
        let mut mode: u32 = 0;
        if GetConsoleMode(handle, &mut mode) != 0 {
            let _ = SetConsoleMode(handle, mode | 0x0004);
        }
    }
}

#[cfg(not(windows))]
pub fn enable_ansi_support() {}

pub fn render_bar(used: u64, limit: u64, width: usize) -> String {
    let ratio = if limit == 0 {
        0.0
    } else {
        ((used as f64) / (limit as f64)).clamp(0.0, 1.0)
    };
    let filled = ((ratio * (width as f64)).round() as usize).min(width);
    let empty = width.saturating_sub(filled);
    let color = if ratio >= 0.95 {
        RED
    } else if ratio >= 0.70 {
        YELLOW
    } else {
        GREEN
    };
    format!(
        "{}{}{}{}{}{} {:>3.0}% ({}/{})",
        color,
        "█".repeat(filled),
        DIM,
        "░".repeat(empty),
        RESET,
        color,
        ratio * 100.0,
        used,
        limit
    )
}

pub fn render_float_bar(val: f64, max_val: f64, width: usize, color: &str) -> String {
    let ratio = (val / max_val.max(1e-6)).clamp(0.0, 1.0);
    let filled = ((ratio * (width as f64)).round() as usize).min(width);
    let empty = width.saturating_sub(filled);
    format!(
        "{}{}{}{}{}{} {:.3}",
        color,
        "▰".repeat(filled),
        DIM,
        "▱".repeat(empty),
        RESET,
        WHITE,
        val
    )
}

pub fn badge_status(status: &str) -> String {
    match status {
        "ready" | "running" | "reviewed" | "pass" | "continue" => format!("{}● {}{}", GREEN, status.to_uppercase(), RESET),
        "quota" | "needs-fix" | "review" | "throttle" | "checkpoint" => format!("{}▲ {}{}", YELLOW, status.to_uppercase(), RESET),
        "quarantine" | "auth" | "error" | "stop" => format!("{}✖ {}{}", RED, status.to_uppercase(), RESET),
        _ => format!("{}○ {}{}", CYAN, status.to_uppercase(), RESET),
    }
}

pub fn badge_effort(effort: &str) -> String {
    match effort {
        "max" => format!("{}{}⚡MAX{}", BOLD, MAGENTA, RESET),
        "xhigh" => format!("{}{}🔥XHIGH{}", BOLD, RED, RESET),
        "high" => format!("{}⚡HIGH{}", YELLOW, RESET),
        "medium" => format!("{}⚙MED{}", CYAN, RESET),
        _ => format!("{}·LOW{}", DIM, RESET),
    }
}

pub fn badge_tier(tier: &str) -> String {
    match tier {
        "critical" => format!("{}{}[CRITICAL]{}", BOLD, RED, RESET),
        "deep" => format!("{}[DEEP]{}", YELLOW, RESET),
        _ => format!("{}[STANDARD]{}", CYAN, RESET),
    }
}

pub fn print_dashboard(state: &FleetState) {
    let ledger_status = match verify_ledger(state) {
        Ok(n) => format!("{}✔ VERIFIED ({} blocks){}", GREEN, n, RESET),
        Err(e) => format!("{}✖ FAILED ({}){}", RED, e, RESET),
    };
    let (gate_act, gate_msg) = evaluate_continue_gate(&state.continue_gate);

    println!();
    println!(
        "{}{}╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════╗{}",
        BOLD, BRIGHT_CYAN, RESET
    );
    println!(
        "{}{}║{} {}🚀 AI-FLEET v{}{} │ {}Topology:{} {}{:<17}{} │ {}Epoch:{} #{:<3} │ {}Autopilot:{} {} {}{}║{}",
        BOLD,
        BRIGHT_CYAN,
        RESET,
        BOLD,
        state.version,
        RESET,
        DIM,
        RESET,
        MAGENTA,
        state.topology,
        RESET,
        DIM,
        RESET,
        state.global_epoch,
        DIM,
        RESET,
        if state.autopilot_enabled {
            format!("{}ON (15-Agent Mesh){}", GREEN, RESET)
        } else {
            format!("{}OFF{}", YELLOW, RESET)
        },
        BOLD,
        BRIGHT_CYAN,
        RESET
    );
    println!(
        "{}{}║{} {}Workspace:{} {:<36} │ {}Ledger:{} {:<22} │ {}Gate:{} {} {}{}║{}",
        BOLD,
        BRIGHT_CYAN,
        RESET,
        DIM,
        RESET,
        state.repo_root,
        DIM,
        RESET,
        ledger_status,
        DIM,
        RESET,
        badge_status(gate_act),
        BOLD,
        BRIGHT_CYAN,
        RESET
    );
    println!(
        "{}{}╚══════════════════════════════════════════════════════════════════════════════════════════════════════════════╝{}",
        BOLD, BRIGHT_CYAN, RESET
    );

    if state.look_mode == "orbital" {
        println!(
            "  {}      .  *  .   ╭─────────────────────────────────────────────────────────╮   .  *  .{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}   *  ╭───╮  .  │  🌐 ORBITAL HIVE-MIND: PBFT + MoE 8-EXPERTS + PHEROMONE │  .  ╭───╮  *{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}     ╰─────╯    ╰────────────┬───────────────────────────────┬────────────╯    ╰─────╯{}",
            BRIGHT_CYAN, RESET
        );
    } else {
        println!(
            "\n{}{}┌── 1. HIERARCHICAL-MESH SWARM TOPOLOGY & FENCED WORKTREE LEASES ──────────────────────────────────────────────┐{}",
            BOLD, BLUE, RESET
        );
        println!(
            "  {}         ┌───────────────────────────────────────────────────────────────────────┐{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         │  👑 QUEEN COORDINATOR (MoE 8-Experts + Pheromone + PBFT + ContinueGate)│{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         └───────────────┬───────────────────────────┬───────────────────────────┘{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         ┌───────────────▼───────────┐ ┌─────────────▼─────────────┐ ┌───────────────────────────┐{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         │ 🤖 Codex (Astra / Soul)   │ │ 🌌 Antigravity (Argon/Neon)│ │ 🧠 Claude (Opus 5.5 / Son)│{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         │  Fenced Worktree Slot-A   │ │  Fenced Worktree Slot-B   │ │  Fenced Worktree Slot-C   │{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         └───────────────────────────┘ └───────────────────────────┘ └───────────────────────────┘{}",
            BRIGHT_CYAN, RESET
        );
    }

    println!(
        "\n{}{}┌── 2. AI PROVIDERS, QUOTA CIRCUIT-BREAKER, Q-SCORES & PHEROMONE TRAILS (ADR-330) ─────────────────────────────┐{}",
        BOLD, BLUE, RESET
    );
    for (id, p) in &state.providers {
        let reset_note = p
            .retry_at
            .as_ref()
            .map(|r| format!(" {}[Auto-Resume: {}]{}", YELLOW, r, RESET))
            .unwrap_or_default();
        println!(
            "  {}▪ {:<12}{} │ {:<14} │ Active: {}{:<16}{} │ Effort: {:<12} │ Quota: {}{}{}",
            BOLD,
            id.to_uppercase(),
            RESET,
            badge_status(&p.status),
            WHITE,
            p.active_model,
            RESET,
            badge_effort(&p.active_effort),
            render_bar(p.daily_quota_used, p.daily_quota_limit, 12),
            RESET,
            reset_note
        );
        for m in &p.models {
            println!(
                "     {}├─ Model:{} {:<18} {:<11} Eff:{:<10} Q:{}  Pheromone:{}  Trust:{:.2} ({}/{} ok)",
                DIM,
                RESET,
                m.name,
                badge_tier(&m.tier),
                badge_effort(&m.default_effort),
                render_float_bar(m.q_score, 1.0, 8, BRIGHT_CYAN),
                render_float_bar(m.pheromone, 2.0, 8, MAGENTA),
                m.trust_score,
                m.successes,
                m.calls_today
            );
        }
    }

    println!(
        "\n{}{}┌── 3. MIXTURE-OF-EXPERTS (8-EXPERT NEURAL GATING NETWORK & UTILIZATION) ──────────────────────────────────────┐{}",
        BOLD, BLUE, RESET
    );
    for chunk in state.moe_experts.chunks(2) {
        let left = &chunk[0];
        let right_str = if chunk.len() > 1 {
            let r = &chunk[1];
            format!(
                "{:<12} {} (EMA:{:.2}, runs:{:<2}, {})",
                r.name,
                render_float_bar(r.weight, 0.25, 8, GREEN),
                r.reward_ema,
                r.utilization,
                badge_effort(&r.preferred_effort)
            )
        } else {
            String::new()
        };
        println!(
            "  {}⚡ {:<12}{} {} (EMA:{:.2}, runs:{:<2}, {})   │   {}⚡{} {}",
            CYAN,
            left.name,
            RESET,
            render_float_bar(left.weight, 0.25, 8, GREEN),
            left.reward_ema,
            left.utilization,
            badge_effort(&left.preferred_effort),
            CYAN,
            RESET,
            right_str
        );
    }

    println!(
        "\n{}{}┌── 4. SWARM TASKS, FENCED LEASES & ADAPTIVE MOE/PHEROMONE ROUTING ────────────────────────────────────────────┐{}",
        BOLD, BLUE, RESET
    );
    let busy = BTreeSet::new();
    for t in state.tasks.values() {
        let is_rev = t.status == "review" || t.status == "reviewing";
        let route_str = match compute_optimal_route(state, t, &busy, is_rev) {
            Some(r) => {
                let exp_tags = r
                    .moe_experts
                    .iter()
                    .map(|(n, w)| format!("{}:{:.0}%", n, w * 100.0))
                    .collect::<Vec<_>>()
                    .join(", ");
                let fb = r
                    .fallback_chain
                    .first()
                    .map(|(fp, fm, fe)| format!(" {}-> fallback {}/{} ({}){}", DIM, fp, fm, fe, RESET))
                    .unwrap_or_default();
                format!(
                    "{}{}/{}{} ({}) │ MoE:[{}] │ Score:{:.3}{}",
                    GREEN,
                    r.selected_provider,
                    r.selected_model,
                    RESET,
                    badge_effort(&r.selected_effort),
                    exp_tags,
                    r.composite_score,
                    fb
                )
            }
            None => format!("{}Waiting for provider quota reset{}", YELLOW, RESET),
        };
        println!(
            "  {}◆ Task [{}{}]{} (Epoch #{}) — {}{}{}",
            BOLD, BRIGHT_CYAN, t.id, RESET, t.lease_epoch, WHITE, t.title, RESET
        );
        println!(
            "    ├─ Status: {:<18} │ Tier: {:<13} │ Owned: {}{:?}{}",
            badge_status(&t.status),
            badge_tier(&t.tier),
            CYAN,
            t.owned_files,
            RESET
        );
        println!("    └─ Route:  {}", route_str);
    }

    println!(
        "\n{}{}┌── 5. CASA AUTHORITY ENVELOPE (ADR-380), CONTINUE-GATE & REASONING BANK ({} patterns) ─────────────────────────┐{}",
        BOLD,
        BLUE,
        state.reasoning_bank.len(),
        RESET
    );
    println!(
        "  {}🛡 CASA Envelope:{} Allow={:?} │ Deny={:?} │ PaidAPI=${:.2}",
        GREEN, RESET, state.casa_envelope.allow, state.casa_envelope.deny, state.casa_envelope.budget_usd
    );
    println!("  {}🚦 ContinueGate:{}  {} — {}", CYAN, RESET, badge_status(gate_act), gate_msg);
    if let Some(last_c) = state.consensus_history.last() {
        println!(
            "  {}⚖ Last PBFT Consensus [{}]:{} '{}' -> {:.1}% weighted quorum ({})",
            MAGENTA,
            last_c.proposal_id,
            RESET,
            last_c.topic,
            last_c.weighted_approval * 100.0,
            last_c.phase
        );
    }
    println!(
        "{}{}└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘{}\n",
        BOLD, BLUE, RESET
    );
}

pub fn print_help() {
    println!(
        "\n{}{}AI-FLEET v2.0.0 (Rust){} — Enterprise Multi-AI Swarm Orchestrator (All Ruflo V3 Subsystems + Multi-AI)",
        BOLD, BRIGHT_CYAN, RESET
    );
    println!("Controls Claude Code (Opus 5.5), Codex (Astra/Soul), Antigravity (Argon/Neon), and any custom AI CLI.\n");
    println!("{}CORE & VISUAL TUI COMMANDS:{}", BOLD, RESET);
    println!("  {}ai-fleet status [--look bbs|orbital]{}              Live visual dashboard (quotas, MoE, pheromones, CASA)", GREEN, RESET);
    println!("  {}ai-fleet start{}                                    Start parallel orchestration across all ready AIs", GREEN, RESET);
    println!("  {}ai-fleet stop{}                                     Drain active writers safely and save checkpoints", GREEN, RESET);
    println!("  {}ai-fleet exec <task_id> <prompt>{}                  Execute prompt with AIDefence + CASA + auto model failover", GREEN, RESET);
    println!("  {}ai-fleet drill{}                                    Run live v2 failover + FencedLease + PBFT consensus drill", GREEN, RESET);
    println!("\n{}PROVIDER & MODEL MANAGEMENT (CONNECT ANY AI):{}", BOLD, RESET);
    println!("  {}ai-fleet provider list{}                            Show connected AIs, models, Q-scores, pheromones & trust", CYAN, RESET);
    println!("  {}ai-fleet provider add <id> <bin> <model> <eff>{}    Connect any new AI CLI tool (ollama, gemini, cursor, aider)", CYAN, RESET);
    println!("  {}ai-fleet provider switch <id> <model> <effort>{}    Switch active model & reasoning effort for an AI", CYAN, RESET);
    println!("  {}ai-fleet provider quota <id> <used> <limit> [ts]{}  Update or reset quota / 5-hour limit window", CYAN, RESET);
    println!("\n{}ADVANCED RUFLO V3 SUBSYSTEMS (NATIVE RUST):{}", BOLD, RESET);
    println!("  {}ai-fleet moe <text>{}                               Run 8-Expert Mixture-of-Experts neural gating on a task", MAGENTA, RESET);
    println!("  {}ai-fleet pheromone [evaporate|boost <model>]{}      Inspect or update ADR-330 pheromone-adaptive trails", MAGENTA, RESET);
    println!("  {}ai-fleet defense <prompt_or_text>{}                 Run AIDefence (ADR-118) prompt-injection & secret scan", MAGENTA, RESET);
    println!("  {}ai-fleet casa <action>{}                            Test CASA (ADR-380 §3) deny-by-default authority envelope", MAGENTA, RESET);
    println!("  {}ai-fleet consensus <topic>{}                        Run a 4-phase PBFT weighted consensus vote across models", MAGENTA, RESET);
    println!("  {}ai-fleet gate{}                                     Evaluate ContinueGate (coherence, uncertainty, rework)", MAGENTA, RESET);
    println!("  {}ai-fleet patterns{}                                 List ReasoningBank learned strategies & success rates", MAGENTA, RESET);
    println!("\n{}TASK, ROUTING, MEMORY & LEDGER:{}", BOLD, RESET);
    println!("  {}ai-fleet task add <id> <title> <file1,file2>{}      Create task with FencedLease file lock & auto-tiering", YELLOW, RESET);
    println!("  {}ai-fleet route <task_id>{}                          Preview MoE + Q-Learning + Pheromone route & fallbacks", YELLOW, RESET);
    println!("  {}ai-fleet swarm <topology> [max_agents]{}            Set swarm topology (hierarchical-mesh, mesh, ring, star)", YELLOW, RESET);
    println!("  {}ai-fleet autopilot <enable|disable|status>{}        Control continuous background completion", YELLOW, RESET);
    println!("  {}ai-fleet budget <show|pause|resume>{}               Global quota circuit-breaker control", YELLOW, RESET);
    println!("  {}ai-fleet memory <set|get|list> [key] [val]{}        Shared cross-AI hive memory store", YELLOW, RESET);
    println!("  {}ai-fleet policy verify{}                            Verify SHA-256 hash-chained decision ledger\n", YELLOW, RESET);
}
