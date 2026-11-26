//! Domain layer for `fleet-neural`: Expert slots, ReasoningBank patterns,
//! Disjoint LinUCB Contextual Bandit arms (`d = 8` ridge-regression inverse
//! covariance matrices), and Trainable 2-Layer MLP weights.

use serde::{Deserialize, Serialize};

pub const EXPERT_NAMES: [&str; 8] = [
    "coder",
    "tester",
    "reviewer",
    "architect",
    "security",
    "performance",
    "researcher",
    "coordinator",
];

pub const LINUCB_DIM: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertSlot {
    pub name: String,
    pub weight: f64,
    pub fisher_diagonal: f64,
    pub optimal_weight: f64,
    pub utilization: u64,
    pub reward_ema: f64,
    pub preferred_tier: String,
    pub preferred_effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoERoutingResult {
    pub top_experts: Vec<(String, f64)>,
    pub all_probabilities: Vec<(String, f64)>,
    pub entropy: f64,
    pub load_balance_loss: f64,
    pub ewc_regularization_loss: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPattern {
    pub pattern_id: String,
    pub domain: String,
    pub strategy: String,
    pub success_rate: f64,
    pub uses: u64,
    pub recommended_effort: String,
    pub key_learnings: Vec<String>,
}

/// Exact Disjoint LinUCB arm maintaining $A_a^{-1} \in \mathbb{R}^{8\times 8}$ via
/// Sherman-Morrison rank-1 updates and reward-weighted feature accumulator $b_a \in \mathbb{R}^8$.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinUcbArm {
    pub arm_id: String,
    pub a_inv: [[f64; LINUCB_DIM]; LINUCB_DIM],
    pub b: [f64; LINUCB_DIM],
    pub pulls: u64,
}

impl LinUcbArm {
    pub fn new(arm_id: impl Into<String>, ridge_lambda: f64) -> Self {
        let inv_diag = 1.0 / ridge_lambda.max(1e-6);
        let mut a_inv = [[0.0; LINUCB_DIM]; LINUCB_DIM];
        for (i, row) in a_inv.iter_mut().enumerate() {
            row[i] = inv_diag;
        }
        LinUcbArm { arm_id: arm_id.into(), a_inv, b: [0.0; LINUCB_DIM], pulls: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinUcbSelection {
    pub arm_id: String,
    pub expected_reward: f64,
    pub uncertainty_bonus: f64,
    pub ucb_score: f64,
}
