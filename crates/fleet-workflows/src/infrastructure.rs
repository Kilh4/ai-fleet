//! Infrastructure layer for `fleet-workflows`: Built-in 12 background daemon
//! worker specifications and interval tick scheduler.

use crate::domain::DaemonWorkerSpec;

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

/// Return IDs of enabled daemon workers due to fire at `elapsed_secs`.
pub fn due_daemon_workers(workers: &[DaemonWorkerSpec], elapsed_secs: u64) -> Vec<String> {
    if elapsed_secs == 0 {
        return Vec::new();
    }
    workers
        .iter()
        .filter(|w| w.enabled && w.interval_sec > 0 && elapsed_secs % w.interval_sec == 0)
        .map(|w| w.id.clone())
        .collect()
}
