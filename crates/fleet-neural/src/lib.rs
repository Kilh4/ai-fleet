//! `fleet-neural` — 3-Layer Hexagonal Neural & Contextual Bandit Crate:
//! - `domain`: Disjoint LinUCB arms, MoE Expert slots, ReasoningBank patterns.
//! - `application`: Sherman-Morrison $O(d^2)$ LinUCB Contextual Bandit, 2-Layer Softmax MoE + EWC Fisher consolidation, Information-Bottleneck compressor.
//! - `infrastructure`: Default expert priors and ReasoningBank seeds.

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{
    compress_message_to_token_budget, extract_context_features, route_moe_experts,
    update_sona_ewc_weights, LinUcbRouter,
};
pub use domain::{ExpertSlot, LinUcbArm, LinUcbSelection, MoERoutingResult, ReasoningPattern, EXPERT_NAMES, LINUCB_DIM};
pub use infrastructure::{default_moe_experts, default_reasoning_bank};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linucb_sherman_morrison_learns_contextual_specialization() {
        let mut router = LinUcbRouter::new(0.4);
        let arms = vec!["claude/opus".to_string(), "codex/astra".to_string()];
        for a in &arms {
            router.ensure_arm(a);
        }

        let sql_ctx = extract_context_features("Implement SQL ledger migration", 3, 0, 2, false);
        let ui_ctx = extract_context_features("Redesign tsx UI layout", 2, 0, 3, false);

        // Train: claude/opus excels on SQL context, codex/astra excels on UI context
        for _ in 0..18 {
            router.update("claude/opus", &sql_ctx, 1.0);
            router.update("codex/astra", &sql_ctx, 0.1);
            router.update("codex/astra", &ui_ctx, 1.0);
            router.update("claude/opus", &ui_ctx, 0.1);
        }

        let pick_sql = router.select(&sql_ctx, &arms).unwrap();
        let pick_ui = router.select(&ui_ctx, &arms).unwrap();
        assert_eq!(pick_sql.arm_id, "claude/opus");
        assert_eq!(pick_ui.arm_id, "codex/astra");
        assert!(pick_sql.expected_reward > 0.75);
    }
}
