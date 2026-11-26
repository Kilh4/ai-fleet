//! Domain layer for `fleet-security`: CASA authority envelopes, ABAC capability
//! tokens, ContinueGate drift metrics, and adversarial ThreatFinding models.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CasaEnvelope {
    pub objective: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub budget_usd: f64,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthDecision {
    pub allowed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueGateMetrics {
    pub consecutive_steps: u32,
    pub max_consecutive_steps: u32,
    pub coherence_score: f64,
    pub min_coherence: f64,
    pub uncertainty_score: f64,
    pub max_uncertainty: f64,
    pub rework_ratio: f64,
    pub max_rework_ratio: f64,
    pub budget_slope: f64,
    pub max_budget_slope: f64,
}

impl Default for ContinueGateMetrics {
    fn default() -> Self {
        Self {
            consecutive_steps: 4,
            max_consecutive_steps: 25,
            coherence_score: 0.94,
            min_coherence: 0.40,
            uncertainty_score: 0.14,
            max_uncertainty: 0.80,
            rework_ratio: 0.05,
            max_rework_ratio: 0.30,
            budget_slope: 0.004,
            max_budget_slope: 0.020,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreatFinding {
    pub category: String,
    pub severity: String,
    pub confidence: f64,
    pub description: String,
}

/// Cryptographically signed ABAC capability token binding subject, task scope,
/// allowed actions, and expiration timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedCapabilityToken {
    pub subject: String,
    pub task_id: String,
    pub scopes: Vec<String>,
    pub expires_epoch_secs: i64,
    pub mac_hex: String,
}
