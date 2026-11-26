//! `fleet-tui` — 8-View Multi-Page Visual Console in CMD/PowerShell:
//! Views: `overview`, `swarm`, `moe`, `memory`, `security`, `agents`, `workflows`, `mcp`
//! Looks: `bbs` (Classic Box-Drawing) & `orbital` (Hive-Mind Globe).

use fleet_agents::catalog_64_agents;
use fleet_core::verify_ledger_chain;
use fleet_mcp::list_mcp_tools;
use fleet_providers::WorkspaceFleetState;
use fleet_security::evaluate_continue_gate;
use fleet_swarm::compute_optimal_route;
use fleet_workflows::SPARC_PHASES;
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
        "ready" | "running" | "reviewed" | "pass" | "continue" | "healthy" => {
            format!("{}● {}{}", GREEN, status.to_uppercase(), RESET)
        }
        "quota" | "needs-fix" | "review" | "throttle" | "checkpoint" => {
            format!("{}▲ {}{}", YELLOW, status.to_uppercase(), RESET)
        }
        "quarantine" | "auth" | "error" | "stop" => {
            format!("{}✖ {}{}", RED, status.to_uppercase(), RESET)
        }
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

pub fn print_dashboard(state: &WorkspaceFleetState, view_override: Option<&str>) {
    let view = view_override.unwrap_or(&state.active_view);
    let ledger_status = match verify_ledger_chain(&state.decision_ledger) {
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
        "{}{}║{} {}🚀 AI-FLEET v{}{} │ {}12-Crate Workspace{} │ {}64 Agents{} │ {}HNSW + SONA/EWC + PBFT/Raft{} │ {}Epoch:{} #{:<3} {}{}║{}",
        BOLD,
        BRIGHT_CYAN,
        RESET,
        BOLD,
        state.version,
        RESET,
        MAGENTA,
        RESET,
        GREEN,
        RESET,
        YELLOW,
        RESET,
        DIM,
        RESET,
        state.global_epoch,
        BOLD,
        BRIGHT_CYAN,
        RESET
    );
    println!(
        "{}{}║{} {}Views:{} [1:overview] [2:swarm] [3:moe] [4:memory] [5:security] [6:agents] [7:workflows] [8:mcp] (Active:{}{}{}) {}{}║{}",
        BOLD,
        BRIGHT_CYAN,
        RESET,
        DIM,
        RESET,
        BRIGHT_CYAN,
        view,
        RESET,
        BOLD,
        BRIGHT_CYAN,
        RESET
    );
    println!(
        "{}{}║{} {}Repo:{} {:<39} │ {}Ledger:{} {:<22} │ {}Gate:{} {} {}{}║{}",
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

    if view == "agents" {
        let agents = catalog_64_agents();
        println!(
            "\n{}{}┌── 64 SPECIALIZED AGENT PERSONAS ACROSS 8 ENTERPRISE DOMAINS ─────────────────────────────────────────────────┐{}",
            BOLD, BLUE, RESET
        );
        for a in agents.iter() {
            println!(
                "  {}▪ {:<24}{} │ Domain: {:<13} │ {:<11} │ Eff:{:<9} │ {}{}{}",
                BOLD,
                a.id,
                RESET,
                a.domain,
                badge_tier(&a.default_tier),
                badge_effort(&a.default_effort),
                WHITE,
                a.role_title,
                RESET
            );
        }
        println!(
            "{}{}└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘{}\n",
            BOLD, BLUE, RESET
        );
        return;
    }

    if view == "workflows" {
        println!(
            "\n{}{}┌── SPARC 5-PHASE METHODOLOGY, PAIR PROGRAMMING & 12 BACKGROUND DAEMON WORKERS ────────────────────────────────┐{}",
            BOLD, BLUE, RESET
        );
        for (idx, (phase, persona, eff, desc)) in SPARC_PHASES.iter().enumerate() {
            println!(
                "  {}[Phase {}/5: {:<12}]{} Persona: {:<22} │ Effort: {:<10} │ {}",
                BRIGHT_CYAN,
                idx + 1,
                phase,
                RESET,
                persona,
                badge_effort(eff),
                desc
            );
        }
        println!(
            "\n  {}🤝 Active Pair Session [{}]:{} Driver={}/{} ({}) <--> Navigator={}/{} ({})",
            MAGENTA,
            state.pair_session.session_id,
            RESET,
            state.pair_session.driver_provider,
            state.pair_session.driver_model,
            state.pair_session.driver_effort,
            state.pair_session.navigator_provider,
            state.pair_session.navigator_model,
            state.pair_session.navigator_effort
        );
        println!("\n  {}⚙ 12 Built-In Autonomous Background Daemon Workers:{}", BOLD, RESET);
        for w in &state.daemon_workers {
            println!(
                "    ├─ {:<25} │ Every {:>3}s │ {:<12} │ {}",
                w.id,
                w.interval_sec,
                badge_status(&w.last_status),
                w.description
            );
        }
        println!(
            "{}{}└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘{}\n",
            BOLD, BLUE, RESET
        );
        return;
    }

    if view == "mcp" {
        println!(
            "\n{}{}┌── MODEL CONTEXT PROTOCOL (MCP JSON-RPC 2.0) SERVER TOOLS (`ai-fleet mcp start`) ─────────────────────────────┐{}",
            BOLD, BLUE, RESET
        );
        for t in list_mcp_tools() {
            println!(
                "  {}🔌 {:<24}{} │ [{:<10}] │ {}",
                GREEN, t.name, RESET, t.category, t.description
            );
        }
        println!(
            "{}{}└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘{}\n",
            BOLD, BLUE, RESET
        );
        return;
    }

    // Default comprehensive multi-panel overview
    if state.look_mode == "orbital" {
        println!(
            "  {}      .  *  .   ╭──────────────────────────────────────────────────────────────────╮   .  *  .{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}   *  ╭───╮  .  │  🌐 ORBITAL HIVE-MIND v3: 64 AGENTS + HNSW + SONA/EWC + PBFT/RAFT │  .  ╭───╮  *{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}     ╰─────╯    ╰───────────────┬──────────────────────────────────┬───────────────╯    ╰─────╯{}",
            BRIGHT_CYAN, RESET
        );
    } else {
        println!(
            "\n{}{}┌── 1. HIERARCHICAL-MESH SWARM TOPOLOGY & FENCED WORKTREE LEASES ──────────────────────────────────────────────┐{}",
            BOLD, BLUE, RESET
        );
        println!(
            "  {}         ┌────────────────────────────────────────────────────────────────────────────┐{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         │ 👑 QUEEN COORDINATOR (64 Personas + HNSW Memory + SONA/EWC + PBFT/Raft)    │{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         └───────────────┬─────────────────────────────┬──────────────────────────────┘{}",
            BRIGHT_CYAN, RESET
        );
        println!(
            "  {}         ┌───────────────▼───────────┐ ┌───────────────▼───────────┐ ┌───────────────────────────┐{}",
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
        "\n{}{}┌── 2. AI PROVIDERS, QUOTA CIRCUIT-BREAKER, Q-SCORES, PHEROMONES & EWC FISHER WEIGHTS ─────────────────────────┐{}",
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
                "     {}├─ Model:{} {:<18} {:<11} Eff:{:<9} Q:{} Phero:{} EWC:{:.2} Trust:{:.2}",
                DIM,
                RESET,
                m.name,
                badge_tier(&m.tier),
                badge_effort(&m.default_effort),
                render_float_bar(m.q_score, 1.0, 7, BRIGHT_CYAN),
                render_float_bar(m.pheromone, 2.0, 7, MAGENTA),
                m.ewc_fisher_importance,
                m.trust_score
            );
        }
    }

    println!(
        "\n{}{}┌── 3. 2-LAYER NEURAL MIXTURE-OF-EXPERTS (8-EXPERT GATING + SONA/EWC FISHER CONSOLIDATION) ────────────────────┐{}",
        BOLD, BLUE, RESET
    );
    for chunk in state.moe_experts.chunks(2) {
        let left = &chunk[0];
        let right_str = if chunk.len() > 1 {
            let r = &chunk[1];
            format!(
                "{:<12} {} (EMA:{:.2}, Fisher:{:.2}, {})",
                r.name,
                render_float_bar(r.weight, 0.25, 7, GREEN),
                r.reward_ema,
                r.fisher_diagonal,
                badge_effort(&r.preferred_effort)
            )
        } else {
            String::new()
        };
        println!(
            "  {}⚡ {:<12}{} {} (EMA:{:.2}, Fisher:{:.2}, {})  │  {}⚡{} {}",
            CYAN,
            left.name,
            RESET,
            render_float_bar(left.weight, 0.25, 7, GREEN),
            left.reward_ema,
            left.fisher_diagonal,
            badge_effort(&left.preferred_effort),
            CYAN,
            RESET,
            right_str
        );
    }

    println!(
        "\n{}{}┌── 4. SWARM TASKS, QUEEN SUB-TASK DECOMPOSITION, SPARC PHASE & ADAPTIVE ROUTING ──────────────────────────────┐{}",
        BOLD, BLUE, RESET
    );
    let busy = BTreeSet::new();
    for t in state.tasks.values() {
        let is_rev = t.status == "review" || t.status == "reviewing";
        let route_str = match compute_optimal_route(&state.providers, &state.moe_experts, &state.reasoning_bank, t, &busy, is_rev) {
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
                    "{}{}/{}{} ({}) │ Persona:{}{}{} │ MoE:[{}] │ Score:{:.3}{}",
                    GREEN,
                    r.selected_provider,
                    r.selected_model,
                    RESET,
                    badge_effort(&r.selected_effort),
                    MAGENTA,
                    r.assigned_persona,
                    RESET,
                    exp_tags,
                    r.composite_score,
                    fb
                )
            }
            None => format!("{}Waiting for provider quota reset{}", YELLOW, RESET),
        };
        println!(
            "  {}◆ Task [{}{}]{} (Epoch #{}, SPARC:{}{}{}) — {}{}{}",
            BOLD, BRIGHT_CYAN, t.id, RESET, t.lease_epoch, YELLOW, t.sparc_phase, RESET, WHITE, t.title, RESET
        );
        println!(
            "    ├─ Status: {:<18} │ Tier: {:<13} │ Sub-tasks: {} │ Owned: {}{:?}{}",
            badge_status(&t.status),
            badge_tier(&t.tier),
            t.subtasks.len(),
            CYAN,
            t.owned_files,
            RESET
        );
        println!("    └─ Route:  {}", route_str);
    }

    println!(
        "\n{}{}┌── 5. HNSW VECTOR MEMORY ({} nodes), CASA (ADR-380), CONTINUE-GATE & 12 DAEMON WORKERS ────────────────────────┐{}",
        BOLD,
        BLUE,
        state.vector_memory.len(),
        RESET
    );
    println!(
        "  {}🛡 CASA Envelope:{} Allow={:?} │ Deny={:?} │ PaidAPI=${:.2}",
        GREEN, RESET, state.casa_envelope.allow, state.casa_envelope.deny, state.casa_envelope.budget_usd
    );
    println!("  {}🚦 ContinueGate:{}  {} — {}", CYAN, RESET, badge_status(gate_act), gate_msg);
    println!(
        "  {}🧠 HNSW Vector Graph:{} {} indexed nodes (64-dim embeddings) │ {} Truth Anchors │ {} Daemon Workers active",
        MAGENTA,
        RESET,
        state.vector_memory.len(),
        state.truth_anchors.len(),
        state.daemon_workers.len()
    );
    println!(
        "{}{}└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘{}\n",
        BOLD, BLUE, RESET
    );
}
