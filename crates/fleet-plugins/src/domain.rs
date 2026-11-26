//! Domain layer for `fleet-plugins`: Plugin manifests, capability grants,
//! trust tiers, and catalog entities.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    InspectContext,
    EnrichPrompt,
    BlockExecution,
    InspectDiff,
    BlockMerge,
    EmitTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginTrustTier {
    KernelVerified,
    WorkspaceSigned,
    SandboxedCommunity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub category: String,
    pub trust_tier: PluginTrustTier,
    pub granted_capabilities: BTreeSet<PluginCapability>,
    pub priority: u16,
    pub enabled: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetPlugin {
    pub id: String,
    pub category: String,
    pub version: String,
    pub enabled_by_default: bool,
    pub tools_count: usize,
    pub description: String,
}
