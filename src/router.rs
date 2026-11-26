use crate::domain::{FleetState, SwarmTask};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertSlot {
    pub name: String,
    pub weight: f64,
    pub utilization: u64,
    pub reward_ema: f64,
    pub preferred_tier: String,
    pub preferred_effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoERoutingResult {
    pub top_experts: Vec<(String, f64)>,
    pub entropy: f64,
    pub load_balance_loss: f64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    pub task_id: String,
    pub classified_tier: String,
    pub moe_experts: Vec<(String, f64)>,
    pub moe_entropy: f64,
    pub matched_pattern: Option<String>,
    pub selected_provider: String,
    pub selected_model: String,
    pub selected_effort: String,
    pub composite_score: f64,
    pub fallback_chain: Vec<(String, String, String)>,
    pub reason: String,
}

pub fn default_moe_experts() -> Vec<ExpertSlot> {
    vec![
        ExpertSlot { name: "coder".into(), weight: 0.16, utilization: 18, reward_ema: 0.95, preferred_tier: "deep".into(), preferred_effort: "high".into() },
        ExpertSlot { name: "tester".into(), weight: 0.14, utilization: 12, reward_ema: 0.94, preferred_tier: "deep".into(), preferred_effort: "high".into() },
        ExpertSlot { name: "reviewer".into(), weight: 0.15, utilization: 14, reward_ema: 0.98, preferred_tier: "critical".into(), preferred_effort: "max".into() },
        ExpertSlot { name: "architect".into(), weight: 0.13, utilization: 9, reward_ema: 0.97, preferred_tier: "critical".into(), preferred_effort: "xhigh".into() },
        ExpertSlot { name: "security".into(), weight: 0.14, utilization: 11, reward_ema: 0.98, preferred_tier: "critical".into(), preferred_effort: "xhigh".into() },
        ExpertSlot { name: "performance".into(), weight: 0.10, utilization: 6, reward_ema: 0.92, preferred_tier: "deep".into(), preferred_effort: "high".into() },
        ExpertSlot { name: "researcher".into(), weight: 0.09, utilization: 8, reward_ema: 0.91, preferred_tier: "standard".into(), preferred_effort: "medium".into() },
        ExpertSlot { name: "coordinator".into(), weight: 0.09, utilization: 10, reward_ema: 0.95, preferred_tier: "deep".into(), preferred_effort: "high".into() },
    ]
}

pub fn default_reasoning_bank() -> Vec<ReasoningPattern> {
    vec![
        ReasoningPattern {
            pattern_id: "PAT-FIN-001".into(),
            domain: "financial-migration".into(),
            strategy: "Preflight read-only scan -> isolated PostgreSQL 18 container rehearsal -> SCRAM verifier -> independent non-author review".into(),
            success_rate: 0.99,
            uses: 14,
            recommended_effort: "max".into(),
            key_learnings: vec![
                "Never use double/float; enforce numeric(19,4) + Currency char(3)".into(),
                "Check >= 10^15 overflow rows before ALTER COLUMN TYPE".into(),
            ],
        },
        ReasoningPattern {
            pattern_id: "PAT-SWARM-002".into(),
            domain: "quota-handoff".into(),
            strategy: "Verify Windows Job Object active_processes==0 -> verify SHA-256 checkpoint -> create fenced lease epoch+1 -> escalate effort to xhigh".into(),
            success_rate: 1.00,
            uses: 9,
            recommended_effort: "xhigh".into(),
            key_learnings: vec![
                "Receiver must inspect git diff HEAD and checkpoint hashes before writing".into(),
                "Fenced lease epoch prevents zombie writer overwrite".into(),
            ],
        },
        ReasoningPattern {
            pattern_id: "PAT-SEC-003".into(),
            domain: "rls-and-audit".into(),
            strategy: "Row-Level Security unit_visible policy + hash-chained audit log + external signed anchor verification".into(),
            success_rate: 0.97,
            uses: 11,
            recommended_effort: "xhigh".into(),
            key_learnings: vec![
                "Every tenant table must pass architecture RLS invariant test".into(),
            ],
        },
    ]
}

pub fn route_moe_experts(experts: &[ExpertSlot], text: &str, top_k: usize) -> MoERoutingResult {
    let low = text.to_lowercase();
    let mut raw_logits = Vec::with_capacity(experts.len());
    for ex in experts {
        let keyword_boost = match ex.name.as_str() {
            "security" if low.contains("security") || low.contains("scram") || low.contains("audit") || low.contains("rls") || low.contains("secret") => 1.4,
            "reviewer" if low.contains("review") || low.contains("verify") || low.contains("inspect") => 1.35,
            "architect" if low.contains("adr") || low.contains("migration") || low.contains("architecture") || low.contains("schema") => 1.35,
            "tester" if low.contains("test") || low.contains("golden") || low.contains("smoke") || low.contains("drill") => 1.3,
            "coder" if low.contains(".cs") || low.contains(".rs") || low.contains(".ts") || low.contains("implement") || low.contains("fix") => 1.25,
            "performance" if low.contains("perf") || low.contains("load") || low.contains("benchmark") || low.contains("latency") => 1.25,
            "researcher" if low.contains("doc") || low.contains("extract") || low.contains("catalog") => 1.2,
            "coordinator" if low.contains("swarm") || low.contains("sync") || low.contains("orchestrat") => 1.25,
            _ => 0.2,
        };
        let logit = (ex.weight * 2.5) + (ex.reward_ema * 1.2) + keyword_boost;
        raw_logits.push(logit);
    }

    let temp = 0.65_f64;
    let max_l = raw_logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = raw_logits.iter().map(|l| ((l - max_l) / temp).exp()).collect();
    let sum_exp: f64 = exps.iter().sum::<f64>().max(1e-9);
    let probs: Vec<f64> = exps.iter().map(|e| e / sum_exp).collect();

    let entropy: f64 = -probs
        .iter()
        .filter(|&&p| p > 1e-12)
        .map(|&p| p * p.ln())
        .sum::<f64>();

    let total_u: f64 = experts.iter().map(|e| e.utilization as f64).sum::<f64>().max(1.0);
    let n = experts.len() as f64;
    let lb_loss: f64 = n
        * probs
            .iter()
            .zip(experts.iter())
            .map(|(&p, e)| p * ((e.utilization as f64) / total_u))
            .sum::<f64>();

    let mut indexed: Vec<(String, f64)> = experts
        .iter()
        .zip(probs.iter())
        .map(|(e, &p)| (e.name.clone(), p))
        .collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(top_k.max(1));

    MoERoutingResult {
        top_experts: indexed,
        entropy,
        load_balance_loss: lb_loss,
    }
}

pub fn classify_task_tier(title: &str, owned_files: &[String], retries: u32, is_review: bool) -> (String, String) {
    let joined = format!("{} {}", title.to_lowercase(), owned_files.join(" ").to_lowercase());
    let critical_keywords = [
        "migration", ".sql", "money", "audit", "access", "scram", "security",
        "syncspike", "adr-", "kernel", "workflow", "review", "ledger", "rls", "critical",
    ];
    let deep_keywords = [
        ".cs", ".ts", ".rs", "golden/", ".case.yaml", "test", "screen", "extract", "api", "worker",
    ];

    if is_review || retries > 0 || critical_keywords.iter().any(|k| joined.contains(k)) {
        let effort = if is_review || retries > 0 { "max" } else { "xhigh" };
        ("critical".to_string(), effort.to_string())
    } else if deep_keywords.iter().any(|k| joined.contains(k)) {
        ("deep".to_string(), "high".to_string())
    } else {
        ("standard".to_string(), "medium".to_string())
    }
}

pub fn refresh_quota_resets_and_pheromones(state: &mut FleetState) {
    let now = Utc::now();
    for prov in state.providers.values_mut() {
        if prov.status == "quota" || prov.status == "network" {
            if let Some(ref ts) = prov.retry_at {
                if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
                    if dt.with_timezone(&Utc) <= now {
                        prov.status = "ready".to_string();
                        prov.retry_at = None;
                        prov.daily_quota_used = 0;
                        prov.last_detail = "Quota window reset automatically; back online".to_string();
                    }
                }
            }
        }
    }
}

pub fn compute_optimal_route(
    state: &FleetState,
    task: &SwarmTask,
    busy_providers: &BTreeSet<String>,
    is_review: bool,
) -> Option<RouteDecision> {
    let (tier, base_effort) = classify_task_tier(&task.title, &task.owned_files, task.retries, is_review);
    let moe = route_moe_experts(
        &state.moe_experts,
        &format!("{} {}", task.title, task.owned_files.join(" ")),
        2,
    );

    let low = task.title.to_lowercase();
    let matched_pattern = state
        .reasoning_bank
        .iter()
        .find(|p| {
            (low.contains("migration") || low.contains("money")) && p.domain == "financial-migration"
                || (low.contains("security") || low.contains("audit")) && p.domain == "rls-and-audit"
                || p.domain == "quota-handoff"
        })
        .map(|p| format!("{} ({} - {:.0}% success)", p.pattern_id, p.domain, p.success_rate * 100.0));

    let mut ranked: Vec<(f64, String, String, String)> = Vec::new();
    let ready_count = state.providers.values().filter(|p| p.status == "ready").count();

    for (pid, prov) in &state.providers {
        if prov.status != "ready" || busy_providers.contains(pid) {
            continue;
        }
        if is_review && task.authors.contains(pid) && ready_count > 1 {
            continue;
        }
        let quota_ratio = if prov.daily_quota_limit > 0 {
            (prov.daily_quota_used as f64) / (prov.daily_quota_limit as f64)
        } else {
            0.0
        };
        if quota_ratio >= 1.0 {
            continue;
        }

        for m in &prov.models {
            let tier_bonus = if m.tier == tier {
                0.18
            } else if tier == "critical" && m.tier == "deep" {
                0.05
            } else {
                0.0
            };
            let conservation_bonus = if tier == "standard" && m.tier != "critical" {
                0.14
            } else {
                0.0
            };
            let headroom_bonus = (1.0 - quota_ratio) * 0.12;
            let pheromone_bonus = (m.pheromone - 1.0).clamp(-0.2, 0.35) * 0.25;
            let trust_bonus = m.trust_score * 0.15;

            let total_score = m.q_score + tier_bonus + conservation_bonus + headroom_bonus + pheromone_bonus + trust_bonus;

            let effort = if task.retries > 0 || is_review {
                if pid == "claude" { "max".to_string() } else { "xhigh".to_string() }
            } else if tier == "critical" {
                m.default_effort.clone()
            } else {
                base_effort.clone()
            };

            ranked.push((total_score, pid.clone(), m.name.clone(), effort));
        }
    }

    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if ranked.is_empty() {
        return None;
    }

    let (top_score, best_prov, best_model, best_effort) = ranked[0].clone();
    let fallback_chain = ranked
        .into_iter()
        .skip(1)
        .map(|(_, p, m, e)| (p, m, e))
        .collect();

    let top_exp_str = moe
        .top_experts
        .iter()
        .map(|(n, w)| format!("{}:{:.0}%", n, w * 100.0))
        .collect::<Vec<_>>()
        .join("+");

    Some(RouteDecision {
        task_id: task.id.clone(),
        classified_tier: tier.clone(),
        moe_experts: moe.top_experts,
        moe_entropy: moe.entropy,
        matched_pattern,
        selected_provider: best_prov.clone(),
        selected_model: best_model.clone(),
        selected_effort: best_effort.clone(),
        composite_score: top_score,
        fallback_chain,
        reason: format!(
            "MoE[{}] + Q-Pheromone selected {}/{} (effort={}, tier={}, score={:.3})",
            top_exp_str, best_prov, best_model, best_effort, tier, top_score
        ),
    })
}

pub fn update_learning_and_pheromones(
    state: &mut FleetState,
    provider_id: &str,
    model_name: &str,
    reward: f64,
    latency_ms: u64,
) {
    let alpha = 0.18;
    let evaporation = 0.97;
    for p in state.providers.values_mut() {
        for m in &mut p.models {
            m.pheromone = (m.pheromone * evaporation).clamp(0.20, 2.00);
        }
    }
    if let Some(prov) = state.providers.get_mut(provider_id) {
        prov.daily_quota_used = prov.daily_quota_used.saturating_add(1);
        for m in &mut prov.models {
            if m.name == model_name {
                m.calls_today += 1;
                if reward >= 0.8 {
                    m.successes += 1;
                    m.pheromone = (m.pheromone + 0.14 * reward).clamp(0.20, 2.00);
                    m.trust_score = (m.trust_score * 0.95 + 0.05 * 1.0).clamp(0.10, 0.999);
                } else {
                    m.failures += 1;
                    m.pheromone = (m.pheromone * 0.75).clamp(0.20, 2.00);
                    m.trust_score = (m.trust_score * 0.90).clamp(0.10, 0.999);
                }
                m.q_score = ((1.0 - alpha) * m.q_score + alpha * reward).clamp(0.10, 0.999);
                m.avg_latency_ms = if m.avg_latency_ms == 0 {
                    latency_ms
                } else {
                    (m.avg_latency_ms * 3 + latency_ms) / 4
                };
            }
        }
    }
}
