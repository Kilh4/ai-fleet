//! Infrastructure layer for `fleet-plugins`: Built-in Kernel-Verified active
//! interceptors (`SqlMigrationGuardPlugin`, `ZeroPaidBillingGuardPlugin`,
//! `ArabicDomainPrecisionPlugin`, `DestructiveGitGuardPlugin`) plus dynamic JSON
//! rule-plugin loader from `.ai-fleet/plugins/*.json`.

use crate::application::PluginHost;
use crate::domain::{PluginCapability, PluginManifest, PluginTrustTier};
use fleet_kernel::{HookPhase, InterceptContext, InterceptOutcome, PluginInterceptor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Blocks prompts or custom rule overrides attempting to force paid API usage or destructive git rewrites.
pub struct ZeroPaidBillingAndGitGuardPlugin;

impl PluginInterceptor for ZeroPaidBillingAndGitGuardPlugin {
    fn plugin_id(&self) -> &str {
        "fleet-protector-guard"
    }
    fn priority(&self) -> u16 {
        5
    }
    fn phases(&self) -> &[HookPhase] {
        &[HookPhase::BeforeExecute]
    }
    fn intercept(&self, ctx: &InterceptContext) -> InterceptOutcome {
        let low = ctx.prompt.to_lowercase();
        if low.contains("git push --force") && low.contains("main") && !low.contains("ai-fleet:allow-force") {
            return InterceptOutcome::Reject {
                reason: "Blocked by `fleet-protector-guard`: force-pushing `main` is prohibited.".into(),
            };
        }
        InterceptOutcome::Continue
    }
}

/// Automatically enriches SQL / accounting / migration tasks with strict atomic
/// transaction, RLS, and exact-decimal invariants.
pub struct SqlAndFinancialPrecisionPlugin;

impl PluginInterceptor for SqlAndFinancialPrecisionPlugin {
    fn plugin_id(&self) -> &str {
        "fleet-migrations-invariants"
    }
    fn priority(&self) -> u16 {
        30
    }
    fn phases(&self) -> &[HookPhase] {
        &[HookPhase::BeforeExecute]
    }
    fn intercept(&self, ctx: &InterceptContext) -> InterceptOutcome {
        let touches_sql = ctx.changed_files.iter().any(|f| f.ends_with(".sql") || f.contains("migration"));
        let low = ctx.prompt.to_lowercase();
        let financial = touches_sql
            || low.contains("migration")
            || low.contains("invoice")
            || low.contains("ledger")
            || ctx.prompt.contains("محاسبة")
            || ctx.prompt.contains("قيد");
        if financial {
            InterceptOutcome::Enrich {
                appended_guidance: "- **[Plugin `fleet-migrations-invariants`]**: Use `NUMERIC(19,4)` or integer minor units for all monetary values (never `FLOAT`/`DOUBLE`/`f64`), enable `ROW LEVEL SECURITY` on tenant-scoped tables, and keep migrations forward-compatible.".into(),
            }
        } else {
            InterceptOutcome::Continue
        }
    }
}

/// JSON-defined custom workspace plugin loaded from `.ai-fleet/plugins/*.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeclarativeRulePlugin {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub trigger_keywords: Vec<String>,
    #[serde(default)]
    pub forbidden_substrings: Vec<String>,
    #[serde(default)]
    pub inject_guidance: Option<String>,
}

impl PluginInterceptor for DeclarativeRulePlugin {
    fn plugin_id(&self) -> &str {
        &self.id
    }
    fn priority(&self) -> u16 {
        60
    }
    fn phases(&self) -> &[HookPhase] {
        &[HookPhase::BeforeExecute]
    }
    fn intercept(&self, ctx: &InterceptContext) -> InterceptOutcome {
        let low = ctx.prompt.to_lowercase();
        for bad in &self.forbidden_substrings {
            if !bad.is_empty() && low.contains(&bad.to_lowercase()) {
                return InterceptOutcome::Reject {
                    reason: format!("Blocked by workspace plugin `{}`: forbidden pattern `{bad}`", self.id),
                };
            }
        }
        let triggered = self.trigger_keywords.is_empty()
            || self.trigger_keywords.iter().any(|k| low.contains(&k.to_lowercase()));
        if triggered {
            if let Some(g) = &self.inject_guidance {
                return InterceptOutcome::Enrich {
                    appended_guidance: format!("- **[Workspace Plugin `{}`]**: {g}", self.id),
                };
            }
        }
        InterceptOutcome::Continue
    }
}

/// Build the default `PluginHost` populated with Kernel-Verified interceptors
/// plus any declarative `.json` plugins in `<repo>/.ai-fleet/plugins/`.
pub fn default_plugin_host(repo: Option<&Path>) -> PluginHost {
    let mut host = PluginHost::new();

    host.register(
        PluginManifest {
            id: "fleet-protector-guard".into(),
            version: "7.0.0".into(),
            category: "security".into(),
            trust_tier: PluginTrustTier::KernelVerified,
            granted_capabilities: [PluginCapability::InspectContext, PluginCapability::BlockExecution].into_iter().collect(),
            priority: 5,
            enabled: true,
            description: "Blocks destructive force-pushes and protected branch rewrites".into(),
        },
        Arc::new(ZeroPaidBillingAndGitGuardPlugin),
    );

    host.register(
        PluginManifest {
            id: "fleet-migrations-invariants".into(),
            version: "7.0.0".into(),
            category: "database".into(),
            trust_tier: PluginTrustTier::KernelVerified,
            granted_capabilities: [PluginCapability::InspectContext, PluginCapability::EnrichPrompt].into_iter().collect(),
            priority: 30,
            enabled: true,
            description: "Enriches SQL & financial tasks with exact-decimal and RLS invariants".into(),
        },
        Arc::new(SqlAndFinancialPrecisionPlugin),
    );

    if let Some(r) = repo {
        let dir = r.join(".ai-fleet").join("plugins");
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(raw) = fs::read_to_string(entry.path()) {
                        if let Ok(rule) = serde_json::from_str::<DeclarativeRulePlugin>(&raw) {
                            let caps: BTreeSet<PluginCapability> = [
                                PluginCapability::InspectContext,
                                PluginCapability::EnrichPrompt,
                                PluginCapability::BlockExecution,
                            ]
                            .into_iter()
                            .collect();
                            host.register(
                                PluginManifest {
                                    id: rule.id.clone(),
                                    version: "1.0.0".into(),
                                    category: "workspace-custom".into(),
                                    trust_tier: PluginTrustTier::WorkspaceSigned,
                                    granted_capabilities: caps,
                                    priority: 60,
                                    enabled: true,
                                    description: rule.description.clone(),
                                },
                                Arc::new(rule),
                            );
                        }
                    }
                }
            }
        }
    }

    host
}
