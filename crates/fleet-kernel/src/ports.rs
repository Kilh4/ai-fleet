//! Hexagonal Ports (Dependency Inversion Traits).
//!
//! Application and orchestration layers depend on these traits rather than
//! concrete SQLite, Git, or CLI adapters, allowing storage backends, VCS
//! engines, policy gates, and AI runtimes to be swapped or mocked cleanly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortTaskSnapshot {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: i64,
    pub attempts: u32,
    pub handoffs: u32,
    pub lease_epoch: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortMemoryItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub score: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateCheckResult {
    pub passed: bool,
    pub rule_violations: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

/// Port for durable task persistence and fenced lease acquisition.
pub trait TaskStorePort: Send + Sync {
    fn snapshot_tasks(&self) -> Result<Vec<PortTaskSnapshot>, String>;
    fn try_acquire_fenced_lease(&self, task_id: &str, expected_status: &str, ttl_secs: u64) -> Result<Option<i64>, String>;
}

/// Port for hybrid lexical + dense-vector institutional memory.
pub trait MemoryBackendPort: Send + Sync {
    fn remember(&self, kind: &str, title: &str, content: &str, tags: &[String]) -> Result<String, String>;
    fn recall_hybrid(&self, query: &str, limit: usize) -> Result<Vec<PortMemoryItem>, String>;
}

/// Port for isolated VCS workspaces (Git worktrees, Jujutsu, or container snapshots).
pub trait VcsWorktreePort: Send + Sync {
    fn prepare_workspace(&self, task_id: &str, base_ref: &str) -> Result<String, String>;
    fn seal_checkpoint(&self, workspace_path: &str, message: &str) -> Result<Option<String>, String>;
    fn changed_paths(&self, workspace_path: &str, from_ref: &str, to_ref: &str) -> Result<Vec<String>, String>;
}

/// Port for deterministic pre-merge security, financial precision, and compliance gates.
pub trait PolicyGatePort: Send + Sync {
    fn gate_name(&self) -> &str;
    fn evaluate_diff(&self, workspace_path: &str, changed_files: &[String]) -> Result<GateCheckResult, String>;
}

/// Port for distributed multi-agent consensus engines (PBFT, Raft, Quorum, CRDT).
pub trait ConsensusPort: Send + Sync {
    fn protocol_name(&self) -> &str;
    fn propose_and_tally(&self, proposal_id: &str, votes: &[(String, bool, f64)]) -> Result<(bool, f64), String>;
}
