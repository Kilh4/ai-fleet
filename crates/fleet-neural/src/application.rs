//! Application layer for `fleet-neural`:
//! 1. Exact **Disjoint LinUCB Contextual Bandit** (`Sherman-Morrison` $O(d^2)$ inverse covariance update).
//! 2. 2-Layer Softmax Mixture-of-Experts Gating + EWC Fisher Consolidation.
//! 3. Information-Bottleneck Extractive Message Compressor.

use crate::domain::{ExpertSlot, LinUcbArm, LinUcbSelection, MoERoutingResult, LINUCB_DIM};
use fleet_memory::compute_embedding;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct LinUcbRouter {
    pub alpha: f64,
    pub arms: BTreeMap<String, LinUcbArm>,
}

impl LinUcbRouter {
    pub fn new(alpha: f64) -> Self {
        LinUcbRouter { alpha: alpha.max(0.01), arms: BTreeMap::new() }
    }

    pub fn ensure_arm(&mut self, arm_id: &str) {
        self.arms.entry(arm_id.to_string()).or_insert_with(|| LinUcbArm::new(arm_id, 1.0));
    }

    /// Select the arm maximizing $\hat{\theta}_a^\top x + \alpha \sqrt{x^\top A_a^{-1} x}$.
    pub fn select(&self, x: &[f64; LINUCB_DIM], eligible: &[String]) -> Option<LinUcbSelection> {
        let mut best: Option<LinUcbSelection> = None;
        for id in eligible {
            let Some(arm) = self.arms.get(id) else { continue };
            // theta = A^-1 * b
            let theta = mat_vec(&arm.a_inv, &arm.b);
            let mean = dot(&theta, x);
            // u = x^T A^-1 x
            let a_inv_x = mat_vec(&arm.a_inv, x);
            let quad = dot(x, &a_inv_x).max(0.0);
            let bonus = self.alpha * quad.sqrt();
            let ucb = mean + bonus;

            if best.as_ref().map_or(true, |b| ucb > b.ucb_score) {
                best = Some(LinUcbSelection {
                    arm_id: id.clone(),
                    expected_reward: mean,
                    uncertainty_bonus: bonus,
                    ucb_score: ucb,
                });
            }
        }
        best
    }

    /// Online Sherman-Morrison rank-1 update:
    /// $A^{-1}_{new} = A^{-1} - \frac{(A^{-1} x)(A^{-1} x)^\top}{1 + x^\top A^{-1} x}$
    pub fn update(&mut self, arm_id: &str, x: &[f64; LINUCB_DIM], reward: f64) {
        self.ensure_arm(arm_id);
        let Some(arm) = self.arms.get_mut(arm_id) else { return };
        let u = mat_vec(&arm.a_inv, x); // A^-1 x (since A is symmetric, x^T A^-1 == (A^-1 x)^T)
        let denom = 1.0 + dot(x, &u);
        if denom > 1e-12 {
            for i in 0..LINUCB_DIM {
                for j in 0..LINUCB_DIM {
                    arm.a_inv[i][j] -= (u[i] * u[j]) / denom;
                }
            }
        }
        for i in 0..LINUCB_DIM {
            arm.b[i] += reward * x[i];
        }
        arm.pulls += 1;
    }
}

fn mat_vec(m: &[[f64; LINUCB_DIM]; LINUCB_DIM], v: &[f64; LINUCB_DIM]) -> [f64; LINUCB_DIM] {
    let mut out = [0.0; LINUCB_DIM];
    for i in 0..LINUCB_DIM {
        out[i] = dot(&m[i], v);
    }
    out
}

fn dot(a: &[f64; LINUCB_DIM], b: &[f64; LINUCB_DIM]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Extract an 8-D normalized feature vector `(bias, complexity, retry, files, review, fin_sql, ui, systems)`.
pub fn extract_context_features(
    prompt: &str,
    complexity_tier: u8,
    attempts: u32,
    file_count: usize,
    is_review: bool,
) -> [f64; LINUCB_DIM] {
    let low = prompt.to_lowercase();
    let is_fin_sql = if low.contains("sql") || low.contains("ledger") || low.contains("invoice") || prompt.contains("محاسبة") { 1.0 } else { 0.0 };
    let is_ui = if low.contains("ui") || low.contains("css") || low.contains("tsx") || prompt.contains("تصميم") { 1.0 } else { 0.0 };
    let is_sys = if low.contains("rust") || low.contains("async") || low.contains("concurrency") || low.contains("kernel") { 1.0 } else { 0.0 };
    [
        1.0,
        (complexity_tier as f64 / 3.0).clamp(0.0, 1.0),
        (attempts as f64 / 4.0).clamp(0.0, 1.0),
        ((file_count as f64 + 1.0).ln() / 3.0).clamp(0.0, 1.0),
        if is_review { 1.0 } else { 0.0 },
        is_fin_sql,
        is_ui,
        is_sys,
    ]
}

pub fn route_moe_experts(experts: &[ExpertSlot], text: &str, top_k: usize) -> MoERoutingResult {
    let emb = compute_embedding(text);
    let low = text.to_lowercase();

    let mut hidden = [0.0_f64; 32];
    for h in 0..32 {
        let a = emb[h * 2] as f64;
        let b = emb[h * 2 + 1] as f64;
        hidden[h] = (a * 0.7 + b * 0.3).max(0.0);
    }

    let mut raw_logits = Vec::with_capacity(experts.len());
    for (idx, ex) in experts.iter().enumerate() {
        let mlp_activation: f64 = hidden
            .iter()
            .enumerate()
            .map(|(h_i, &val)| {
                let syn_w = (((idx + 1) * (h_i + 3)) % 11) as f64 / 11.0;
                val * syn_w
            })
            .sum::<f64>()
            * 0.15;

        let keyword_boost = match ex.name.as_str() {
            "security" if low.contains("security") || low.contains("scram") || low.contains("audit") || low.contains("rls") || low.contains("secret") => 1.45,
            "reviewer" if low.contains("review") || low.contains("verify") || low.contains("inspect") => 1.40,
            "architect" if low.contains("adr") || low.contains("migration") || low.contains("architecture") || low.contains("schema") => 1.40,
            "tester" if low.contains("test") || low.contains("golden") || low.contains("smoke") || low.contains("drill") => 1.35,
            "coder" if low.contains(".cs") || low.contains(".rs") || low.contains(".ts") || low.contains("implement") || low.contains("fix") => 1.30,
            "performance" if low.contains("perf") || low.contains("load") || low.contains("benchmark") || low.contains("latency") => 1.30,
            "researcher" if low.contains("doc") || low.contains("extract") || low.contains("catalog") => 1.25,
            "coordinator" if low.contains("swarm") || low.contains("sync") || low.contains("orchestrat") => 1.30,
            _ => 0.20,
        };
        let logit = (ex.weight * 2.5) + (ex.reward_ema * 1.2) + mlp_activation + keyword_boost;
        raw_logits.push(logit);
    }

    let temp = 0.65_f64;
    let max_l = raw_logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = raw_logits.iter().map(|l| ((l - max_l) / temp).exp()).collect();
    let sum_exp: f64 = exps.iter().sum::<f64>().max(1e-9);
    let probs: Vec<f64> = exps.iter().map(|e| e / sum_exp).collect();

    let entropy: f64 = -probs.iter().filter(|&&p| p > 1e-12).map(|&p| p * p.ln()).sum::<f64>();
    let total_u: f64 = experts.iter().map(|e| e.utilization as f64).sum::<f64>().max(1.0);
    let n = experts.len() as f64;
    let lb_loss: f64 = n * probs.iter().zip(experts.iter()).map(|(&p, e)| p * ((e.utilization as f64) / total_u)).sum::<f64>();
    let ewc_loss: f64 = experts.iter().map(|e| 0.5 * e.fisher_diagonal * (e.weight - e.optimal_weight).powi(2)).sum();

    let all_probabilities: Vec<(String, f64)> = experts.iter().zip(probs.iter()).map(|(e, &p)| (e.name.clone(), p)).collect();
    let mut indexed = all_probabilities.clone();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(top_k.max(1));

    MoERoutingResult {
        top_experts: indexed,
        all_probabilities,
        entropy,
        load_balance_loss: lb_loss,
        ewc_regularization_loss: ewc_loss,
    }
}

pub fn update_sona_ewc_weights(experts: &mut [ExpertSlot], activated_expert: &str, reward: f64) {
    let lr = 0.04;
    let ewc_lambda = 0.25;
    for ex in experts.iter_mut() {
        if ex.name == activated_expert {
            ex.utilization += 1;
            ex.reward_ema = (ex.reward_ema * 0.90 + 0.10 * reward).clamp(0.10, 0.999);
            let grad = reward - ex.reward_ema;
            let ewc_pull = ewc_lambda * ex.fisher_diagonal * (ex.weight - ex.optimal_weight);
            ex.weight = (ex.weight + lr * (grad - ewc_pull)).clamp(0.04, 0.45);
            ex.fisher_diagonal = (ex.fisher_diagonal * 0.95 + 0.05 * grad.abs().max(0.85)).clamp(0.50, 0.999);
        }
    }
    let sum_w: f64 = experts.iter().map(|e| e.weight).sum::<f64>().max(1e-6);
    for ex in experts.iter_mut() {
        ex.weight /= sum_w;
    }
}

pub fn compress_message_to_token_budget(message: &str, max_tokens: usize) -> (String, usize, usize) {
    let words: Vec<&str> = message.split_whitespace().collect();
    let orig_tokens = words.len();
    if orig_tokens <= max_tokens {
        return (message.to_string(), orig_tokens, orig_tokens);
    }

    let sentences: Vec<&str> = message.split(&['.', '\n', ';'][..]).map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let mut scored: Vec<(usize, &str)> = sentences
        .into_iter()
        .map(|s| {
            let low = s.to_lowercase();
            let mut score = 1usize;
            for kw in ["sha256", "error", "fail", "pass", "commit", "head", "sql", "migration", "diff", "test", "fix"] {
                if low.contains(kw) {
                    score += 4;
                }
            }
            if s.chars().any(|c| c.is_ascii_digit()) {
                score += 2;
            }
            (score, s)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));

    let mut out_words = Vec::new();
    for (_, sent) in scored {
        for w in sent.split_whitespace() {
            if out_words.len() < max_tokens {
                out_words.push(w);
            }
        }
    }
    let compressed = out_words.join(" ");
    let final_tokens = out_words.len();
    (compressed, orig_tokens, final_tokens)
}
