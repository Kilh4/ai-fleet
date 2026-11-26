//! Infrastructure layer for `fleet-security`: TruthAnchor builder and Microkernel
//! `AidDefenceInterceptor` implementing `fleet_kernel::PluginInterceptor`.

use crate::application::scan_aidefence_threats;
use fleet_core::{sha256_str, TruthAnchor};
use fleet_kernel::{HookPhase, InterceptContext, InterceptOutcome, PluginInterceptor};

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

/// Microkernel Interceptor blocking prompt-injection & secret exfiltration at priority 0.
pub struct AiDefenceKernelInterceptor;

impl PluginInterceptor for AiDefenceKernelInterceptor {
    fn plugin_id(&self) -> &str {
        "fleet-aidefence-shield"
    }
    fn priority(&self) -> u16 {
        0
    }
    fn phases(&self) -> &[HookPhase] {
        &[HookPhase::BeforePlan, HookPhase::BeforeExecute]
    }
    fn intercept(&self, ctx: &InterceptContext) -> InterceptOutcome {
        // Scan only the user task objective/input attribute (or raw prompt if attribute absent)
        let target = ctx.attributes.get("user_prompt").map(String::as_str).unwrap_or(&ctx.prompt);
        let findings = scan_aidefence_threats(target);
        let critical: Vec<String> = findings
            .into_iter()
            .filter(|f| f.severity == "CRITICAL")
            .map(|f| format!("[{}] {}", f.category, f.description))
            .collect();
        if !critical.is_empty() {
            InterceptOutcome::Reject {
                reason: format!("AIDefence Shield blocked task: {}", critical.join("; ")),
            }
        } else {
            InterceptOutcome::Continue
        }
    }
}
