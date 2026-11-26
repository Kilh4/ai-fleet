//! Domain layer for `fleet-swarm`:
//! - PBFT Byzantine Consensus state (`PrePrepare -> Prepare -> Commit` + equivocation proof)
//! - Raft Log-Replication entries & node roles (`Follower / Candidate / Leader`)
//! - Causal Vector-Clock LWW-Element-Map CRDT for Gossip convergence
//! - Queen task decomposition & route models.

use fleet_core::SubTaskSpec;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueenTaskAnalysis {
    pub task_id: String,
    pub complexity_score: f64,
    pub recommended_domain: String,
    pub primary_persona: String,
    pub subtasks: Vec<SubTaskSpec>,
    pub bottleneck_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusRound {
    pub proposal_id: String,
    pub protocol: String,
    pub topic: String,
    pub phase: String,
    pub leader_or_primary: String,
    pub term_or_view: u64,
    pub votes: BTreeMap<String, (bool, f64)>,
    pub weighted_approval: f64,
    pub quorum_achieved: bool,
    pub digest: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    pub task_id: String,
    pub classified_tier: String,
    pub assigned_persona: String,
    pub moe_experts: Vec<(String, f64)>,
    pub moe_entropy: f64,
    pub ewc_loss: f64,
    pub matched_pattern: Option<String>,
    pub selected_provider: String,
    pub selected_model: String,
    pub selected_effort: String,
    pub composite_score: f64,
    pub fallback_chain: Vec<(String, String, String)>,
    pub reason: String,
}

// ─── Real PBFT Byzantine Protocol Entities ──────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PbftPhase {
    Idle,
    PrePrepared,
    Prepared,
    Committed,
    ViewChanging,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquivocationProof {
    pub replica: String,
    pub view: u64,
    pub seq: u64,
    pub digest_a: String,
    pub digest_b: String,
}

#[derive(Debug, Clone)]
pub struct PbftStateMachine {
    pub replicas: Vec<String>,
    pub view: u64,
    pub seq: u64,
    pub phase: PbftPhase,
    pub active_digest: Option<String>,
    pub prepare_votes: BTreeMap<String, String>,
    pub commit_votes: BTreeSet<String>,
    pub equivocations: Vec<EquivocationProof>,
}

// ─── Real Raft Log-Replication Entities ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaftLogEntry {
    pub term: u64,
    pub index: u64,
    pub command: String,
}

// ─── Real Causal Vector-Clock LWW CRDT for Gossip ───────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    pub dots: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrdtRegister {
    pub value: String,
    pub clock: VectorClock,
    pub writer: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GossipCrdtMap {
    pub entries: BTreeMap<String, CrdtRegister>,
}
