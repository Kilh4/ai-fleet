//! `fleet-workflows` — SPARC 5-Phase Methodology Pipeline (Specification,
//! Pseudocode, Architecture, Refinement, Completion), Pair Programming
//! Driver/Navigator engine, and 12 Background Daemon Workers.

use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const SPARC_PHASES: [(&str, &str, &str, &str); 5] = [
    ("Spec", "requirements-analyst", "high", "Define business rule IDs (BR-*), acceptance criteria, and golden test inputs/outputs."),
    ("Pseudocode", "ddd-domain-expert", "high", "Design algorithmic steps, invariants, and idempotency fingerprint boundaries."),
    ("Architecture", "system-architect", "xhigh", "Verify module Contracts boundaries, RLS policies, and ADR compliance."),
    ("Refinement", "coder", "xhigh", "Implement clean warning-free code in isolated FencedLease worktree with TDD."),
    ("Completion", "independent-reviewer", "max", "Run independent non-author verification, fitness functions, and PBFT sign-off."),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparcPipelineExecution {
    pub task_id: String,
    pub current_phase_index: usize,
    pub phases_completed: Vec<(String, String, String, String)>, // (phase, persona, model, timestamp)
    pub status: String,
}

pub fn advance_sparc_phase(current: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    let idx = SPARC_PHASES
        .iter()
        .position(|(name, _, _, _)| name.eq_ignore_ascii_case(current))
        .map(|i| (i + 1).min(SPARC_PHASES.len() - 1))
        .unwrap_or(0);
    SPARC_PHASES[idx]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairProgrammingSession {
    pub session_id: String,
    pub driver_provider: String,
    pub driver_model: String,
    pub driver_effort: String,
    pub navigator_provider: String,
    pub navigator_model: String,
    pub navigator_effort: String,
    pub objective: String,
    pub swaps: u32,
}

pub fn create_pair_session(objective: &str) -> PairProgrammingSession {
    PairProgrammingSession {
        session_id: format!("pair-{}", Utc::now().timestamp()),
        driver_provider: "codex".into(),
        driver_model: "gpt-6.2-astra".into(),
        driver_effort: "high".into(),
        navigator_provider: "antigravity".into(),
        navigator_model: "argon".into(),
        navigator_effort: "xhigh".into(),
        objective: objective.to_string(),
        swaps: 0,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonWorkerSpec {
    pub id: String,
    pub interval_sec: u64,
    pub enabled: bool,
    pub last_status: String,
    pub description: String,
}

pub fn built_in_12_daemon_workers() -> Vec<DaemonWorkerSpec> {
    let specs = [
        ("quota-watchdog", 15, "Monitors 5-hour/daily quota windows and auto-resumes expired holds"),
        ("pheromone-evaporator", 60, "Applies ADR-330 0.97x pheromone decay to prevent routing starvation"),
        ("sona-ewc-consolidator", 120, "Updates Fisher Information diagonal to preserve learned MoE weights"),
        ("lease-fencing-reaper", 30, "Reclaims expired worktree leases and increments monotonic fencing epochs"),
        ("ledger-hash-auditor", 45, "Verifies continuous SHA-256 decision ledger chain integrity"),
        ("aidefence-pattern-sync", 300, "Refreshes ADR-118 prompt-injection and secret redaction signatures"),
        ("casa-expiry-enforcer", 60, "Validates CASA authority envelope timestamps and $0 paid-API limits"),
        ("continue-gate-monitor", 20, "Checks agent coherence, uncertainty, and rework ratios per step"),
        ("hnsw-graph-optimizer", 180, "Rebalances HNSW semantic memory small-world neighbor edges"),
        ("reasoning-bank-distiller", 240, "Distills completed task trajectories into reusable ReasoningBank patterns"),
        ("gossip-anti-entropy", 30, "Synchronizes swarm health and trust scores across active worker slots"),
        ("autopilot-dispatcher", 10, "Continuously dispatches queued slices to ready AI providers"),
    ];
    specs
        .iter()
        .map(|(id, sec, desc)| DaemonWorkerSpec {
            id: (*id).to_string(),
            interval_sec: *sec,
            enabled: true,
            last_status: "healthy".to_string(),
            description: (*desc).to_string(),
        })
        .collect()
}
