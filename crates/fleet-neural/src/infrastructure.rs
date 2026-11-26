//! Infrastructure layer for `fleet-neural`: Default expert configurations and
//! curated ReasoningBank seed patterns.

use crate::domain::{ExpertSlot, ReasoningPattern};

pub fn default_moe_experts() -> Vec<ExpertSlot> {
    vec![
        ExpertSlot { name: "coder".into(), weight: 0.16, fisher_diagonal: 0.92, optimal_weight: 0.16, utilization: 18, reward_ema: 0.95, preferred_tier: "deep".into(), preferred_effort: "high".into() },
        ExpertSlot { name: "tester".into(), weight: 0.14, fisher_diagonal: 0.89, optimal_weight: 0.14, utilization: 12, reward_ema: 0.94, preferred_tier: "deep".into(), preferred_effort: "high".into() },
        ExpertSlot { name: "reviewer".into(), weight: 0.15, fisher_diagonal: 0.98, optimal_weight: 0.15, utilization: 14, reward_ema: 0.98, preferred_tier: "critical".into(), preferred_effort: "max".into() },
        ExpertSlot { name: "architect".into(), weight: 0.13, fisher_diagonal: 0.96, optimal_weight: 0.13, utilization: 9, reward_ema: 0.97, preferred_tier: "critical".into(), preferred_effort: "xhigh".into() },
        ExpertSlot { name: "security".into(), weight: 0.14, fisher_diagonal: 0.99, optimal_weight: 0.14, utilization: 11, reward_ema: 0.98, preferred_tier: "critical".into(), preferred_effort: "xhigh".into() },
        ExpertSlot { name: "performance".into(), weight: 0.10, fisher_diagonal: 0.85, optimal_weight: 0.10, utilization: 6, reward_ema: 0.92, preferred_tier: "deep".into(), preferred_effort: "high".into() },
        ExpertSlot { name: "researcher".into(), weight: 0.09, fisher_diagonal: 0.82, optimal_weight: 0.09, utilization: 8, reward_ema: 0.91, preferred_tier: "standard".into(), preferred_effort: "medium".into() },
        ExpertSlot { name: "coordinator".into(), weight: 0.09, fisher_diagonal: 0.91, optimal_weight: 0.09, utilization: 10, reward_ema: 0.95, preferred_tier: "deep".into(), preferred_effort: "high".into() },
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
