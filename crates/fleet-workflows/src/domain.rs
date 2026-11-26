//! Domain layer for `fleet-workflows`: SPARC guarded FSM states, TDD cycle
//! states, Saga compensation steps, Pair Programming, and Daemon Worker specs.

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
    pub phases_completed: Vec<(String, String, String, String)>,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SparcStage {
    Spec,
    Pseudocode,
    Architecture,
    Refinement,
    Completion,
    Done,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SparcGateEvidence {
    pub has_acceptance_criteria: bool,
    pub has_domain_invariants: bool,
    pub architecture_contracts_clean: bool,
    pub verification_tests_green: bool,
    pub non_author_review_approved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TddPhase {
    WriteFailingTest,
    MakeTestPass,
    RefactorClean,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SagaOutcome {
    Committed { completed_steps: Vec<String> },
    Compensated { failed_step: String, error: String, rolled_back_lifo: Vec<String> },
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonWorkerSpec {
    pub id: String,
    pub interval_sec: u64,
    pub enabled: bool,
    pub last_status: String,
    pub description: String,
}
