//! Microkernel Plugin Interceptor Pipeline.
//!
//! Defines the capability-checked lifecycle middleware contract executed by
//! `fleet-engine` around planning, routing, prompt synthesis, diff inspection,
//! and integration merges.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    BeforePlan,
    BeforeRoute,
    BeforeExecute,
    AfterDiff,
    BeforeMerge,
}

impl HookPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            HookPhase::BeforePlan => "before_plan",
            HookPhase::BeforeRoute => "before_route",
            HookPhase::BeforeExecute => "before_execute",
            HookPhase::AfterDiff => "after_diff",
            HookPhase::BeforeMerge => "before_merge",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterceptContext {
    pub task_id: String,
    pub phase: HookPhase,
    pub role: String,
    pub provider: String,
    pub model: String,
    pub prompt: String,
    pub changed_files: Vec<String>,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum InterceptOutcome {
    /// Allow execution to proceed unchanged.
    Continue,
    /// Append or transform prompt/attributes and continue.
    Enrich { appended_guidance: String },
    /// Deterministically block the operation with an actionable reason.
    Reject { reason: String },
}

/// Implemented by active plugins in `fleet-plugins`, `fleet-security`, etc.
pub trait PluginInterceptor: Send + Sync {
    fn plugin_id(&self) -> &str;
    /// Lower numbers run earlier (`0` = security guards, `50` = domain enrichers, `90` = telemetry).
    fn priority(&self) -> u16 {
        50
    }
    fn phases(&self) -> &[HookPhase];
    fn intercept(&self, ctx: &InterceptContext) -> InterceptOutcome;
}
