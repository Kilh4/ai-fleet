//! Domain layer for `fleet-agents`: Agent personas and on-disk Ruflo slash commands.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPersona {
    pub id: String,
    pub domain: String,
    pub role_title: String,
    pub default_tier: String,
    pub default_effort: String,
    pub capabilities: Vec<String>,
    pub system_directive: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommandSpec {
    pub name: String,
    pub category: String,
    pub summary: String,
    pub template: String,
}
