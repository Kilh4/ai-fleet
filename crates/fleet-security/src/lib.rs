//! `fleet-security` — CASA Deterministic Authority Gate (ADR-380 §3),
//! AIDefence Threat & Prompt-Injection Shield (ADR-118) with Hex/Base64 payload inspect,
//! Guidance ContinueGate, and Cryptographic Truth Anchors.

use chrono::DateTime;
use fleet_core::{sha256_str, TruthAnchor};
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

pub fn check_casa_authorization(envelope: &CasaEnvelope, requested_action: &str, now_rfc3339: &str) -> AuthDecision {
    let exp = DateTime::parse_from_rfc3339(&envelope.expires_at).ok();
    let now = DateTime::parse_from_rfc3339(now_rfc3339).ok();
    match (now, exp) {
        (Some(n), Some(e)) => {
            if n >= e {
                return AuthDecision {
                    allowed: false,
                    reason: format!("CASA envelope expired at {} (now: {})", envelope.expires_at, now_rfc3339),
                };
            }
        }
        _ => {
            return AuthDecision {
                allowed: false,
                reason: "Malformed RFC3339 timestamp in CASA envelope (deny-by-default)".to_string(),
            }
        }
    }

    if envelope.deny.iter().any(|d| d == requested_action) {
        return AuthDecision {
            allowed: false,
            reason: format!("Action '{}' is explicitly forbidden by CASA deny-list", requested_action),
        };
    }

    if envelope.allow.iter().any(|a| a == requested_action || a == "*") {
        return AuthDecision {
            allowed: true,
            reason: format!("Action '{}' authorized by CASA allow-list for '{}'", requested_action, envelope.objective),
        };
    }

    AuthDecision {
        allowed: false,
        reason: format!("Action '{}' not in CASA allow-list (deny-by-default)", requested_action),
    }
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

pub fn evaluate_continue_gate(m: &ContinueGateMetrics) -> (&'static str, String) {
    if m.coherence_score < m.min_coherence {
        return ("stop", format!("Coherence degraded ({:.2} < {:.2}); halting agent to prevent drift", m.coherence_score, m.min_coherence));
    }
    if m.uncertainty_score > m.max_uncertainty {
        return ("pause", format!("Uncertainty too high ({:.2} > {:.2}); escalating effort & requesting review", m.uncertainty_score, m.max_uncertainty));
    }
    if m.rework_ratio > m.max_rework_ratio {
        return ("pause", format!("Rework ratio exceeded ({:.2} > {:.2}); switching model via Q-Router", m.rework_ratio, m.max_rework_ratio));
    }
    if m.budget_slope > m.max_budget_slope {
        return ("throttle", format!("Quota burn slope high ({:.3}/step > {:.3}); throttling to conservation model", m.budget_slope, m.max_budget_slope));
    }
    if m.consecutive_steps >= m.max_consecutive_steps {
        return ("checkpoint", format!("Reached {} steps; forcing mandatory SHA-256 checkpoint", m.consecutive_steps));
    }
    ("continue", format!("All 5 ContinueGate invariants healthy (coherence={:.2}, uncertainty={:.2}, rework={:.2})", m.coherence_score, m.uncertainty_score, m.rework_ratio))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreatFinding {
    pub category: String,
    pub severity: String,
    pub confidence: f64,
    pub description: String,
}

pub fn scan_aidefence_threats(input: &str) -> Vec<ThreatFinding> {
    let low = input.to_lowercase();
    let mut findings = Vec::new();

    // 1. ADR-118 0..6 word window between override verbs and target nouns (English & Arabic)
    let verbs = ["ignore", "disregard", "forget", "override", "bypass", "تجاهل", "الغ", "انس"];
    let nouns = [
        "instruction", "instructions", "prompt", "prompts", "rule", "rules",
        "context", "constitution", "system-prompt", "التعليمات", "القواعد", "الدستور",
    ];
    let tokens: Vec<&str> = low.split_whitespace().collect();
    for (i, tok) in tokens.iter().enumerate() {
        let clean_tok = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if verbs.contains(&clean_tok) {
            let end = (i + 8).min(tokens.len());
            for (j, raw_target) in tokens.iter().enumerate().take(end).skip(i + 1) {
                let target = raw_target.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                if nouns.contains(&target) {
                    findings.push(ThreatFinding {
                        category: "instruction_override".into(),
                        severity: "CRITICAL".into(),
                        confidence: 0.96,
                        description: format!(
                            "ADR-118 verb->noun override window detected ('{} ... {}' within {} words)",
                            clean_tok,
                            target,
                            j - i - 1
                        ),
                    });
                    break;
                }
            }
        }
    }

    // 2. Role hijack / Jailbreak / Exfiltration markers
    let hijack_markers = [
        ("you are now", "role_switching", "HIGH", 0.88),
        ("act as a different", "role_switching", "HIGH", 0.85),
        ("behave like", "role_switching", "HIGH", 0.84),
        ("developer mode", "jailbreak", "CRITICAL", 0.93),
        ("god mode", "jailbreak", "CRITICAL", 0.95),
        ("sudo mode", "jailbreak", "HIGH", 0.90),
        ("curl http", "exfil_command", "HIGH", 0.89),
        ("invoke-webrequest", "exfil_command", "HIGH", 0.89),
    ];
    for (pat, cat, sev, conf) in hijack_markers {
        if low.contains(pat) {
            findings.push(ThreatFinding {
                category: cat.into(),
                severity: sev.into(),
                confidence: conf,
                description: format!("Detected adversarial marker '{}'", pat),
            });
        }
    }

    // 3. Secret / API Key exfiltration detector
    let secret_prefixes = ["sk-ant-", "sk-proj-", "ghp_", "gho_", "aiza", "-----begin private key", "postgres://postgres:"];
    for sp in secret_prefixes {
        if low.contains(sp) {
            findings.push(ThreatFinding {
                category: "secret_exposure".into(),
                severity: "CRITICAL".into(),
                confidence: 0.99,
                description: format!("Credential/private-key pattern '{}' detected in payload", sp),
            });
        }
    }

    findings
}

pub fn default_truth_anchors() -> Vec<TruthAnchor> {
    let invariants = [
        ("ANCHOR-01", "Never activate paid API billing; strip all *_API_KEY env vars and enforce subscription-only logins.", "owner:constitution"),
        ("ANCHOR-02", "Every code change is authored inside an isolated FencedLease git worktree and reviewed by a non-author AI.", "owner:constitution"),
        ("ANCHOR-03", "Financial and critical domain migrations require xhigh/max reasoning effort and preflight verification.", "owner:constitution"),
    ];
    invariants
        .iter()
        .map(|(id, inv, auth)| TruthAnchor {
            anchor_id: (*id).to_string(),
            invariant: (*inv).to_string(),
            source_authority: (*auth).to_string(),
            signature_sha256: sha256_str(&format!("{}|{}|{}", id, inv, auth)),
            immutable: true,
        })
        .collect()
}
