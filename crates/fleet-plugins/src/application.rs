//! Application layer for `fleet-plugins`: Capability-enforced Microkernel
//! Pipeline Runner (`PluginHost`) that executes registered `PluginInterceptor`s
//! in deterministic `(priority, id)` order and strips unauthorized actions.

use crate::domain::{PluginCapability, PluginManifest};
use fleet_kernel::{HookPhase, InterceptContext, InterceptOutcome, PluginInterceptor};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PipelineStepTrace {
    pub plugin_id: String,
    pub outcome: InterceptOutcome,
    pub capability_downgraded: bool,
}

#[derive(Clone, Debug)]
pub struct PipelineExecutionResult {
    pub blocked_by: Option<(String, String)>,
    pub enriched_guidance: Vec<(String, String)>,
    pub traces: Vec<PipelineStepTrace>,
}

struct RegisteredPlugin {
    manifest: PluginManifest,
    interceptor: Arc<dyn PluginInterceptor>,
}

#[derive(Default)]
pub struct PluginHost {
    plugins: BTreeMap<(u16, String), RegisteredPlugin>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, manifest: PluginManifest, interceptor: Arc<dyn PluginInterceptor>) {
        let key = (manifest.priority, manifest.id.clone());
        self.plugins.insert(key, RegisteredPlugin { manifest, interceptor });
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    pub fn manifests(&self) -> Vec<&PluginManifest> {
        self.plugins.values().map(|p| &p.manifest).collect()
    }

    /// Execute all enabled plugins subscribed to `ctx.phase`, enforcing each
    /// plugin's `granted_capabilities` sandbox!
    pub fn run_phase(&self, ctx: &InterceptContext) -> PipelineExecutionResult {
        let mut traces = Vec::new();
        let mut enriched_guidance = Vec::new();

        for reg in self.plugins.values() {
            if !reg.manifest.enabled || !reg.interceptor.phases().contains(&ctx.phase) {
                continue;
            }
            let raw = reg.interceptor.intercept(ctx);
            let mut downgraded = false;

            let enforced = match raw {
                InterceptOutcome::Continue => InterceptOutcome::Continue,
                InterceptOutcome::Enrich { appended_guidance } => {
                    if reg.manifest.granted_capabilities.contains(&PluginCapability::EnrichPrompt) {
                        enriched_guidance.push((reg.manifest.id.clone(), appended_guidance.clone()));
                        InterceptOutcome::Enrich { appended_guidance }
                    } else {
                        downgraded = true;
                        InterceptOutcome::Continue
                    }
                }
                InterceptOutcome::Reject { reason } => {
                    let needed = if ctx.phase == HookPhase::BeforeMerge || ctx.phase == HookPhase::AfterDiff {
                        PluginCapability::BlockMerge
                    } else {
                        PluginCapability::BlockExecution
                    };
                    if reg.manifest.granted_capabilities.contains(&needed) {
                        traces.push(PipelineStepTrace {
                            plugin_id: reg.manifest.id.clone(),
                            outcome: InterceptOutcome::Reject { reason: reason.clone() },
                            capability_downgraded: false,
                        });
                        return PipelineExecutionResult {
                            blocked_by: Some((reg.manifest.id.clone(), reason)),
                            enriched_guidance,
                            traces,
                        };
                    } else {
                        downgraded = true;
                        InterceptOutcome::Continue
                    }
                }
            };

            traces.push(PipelineStepTrace {
                plugin_id: reg.manifest.id.clone(),
                outcome: enforced,
                capability_downgraded: downgraded,
            });
        }

        PipelineExecutionResult { blocked_by: None, enriched_guidance, traces }
    }
}
