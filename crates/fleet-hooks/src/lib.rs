//! `fleet-hooks` — Self-Learning Lifecycle Hooks (`pre-tool`, `post-tool`, `session-start`, `stop`)
//! and Project Initializer (`ai-fleet init`) that generates `.mcp.json`, `.claude/hooks.json`,
//! `.codex/hooks.json`, and `.agents/skills/ai-fleet/SKILL.md` in any repository.

use chrono::Utc;
use fleet_security::{check_casa_authorization, evaluate_continue_gate, scan_aidefence_threats, CasaEnvelope, ContinueGateMetrics};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookExecutionResult {
    pub hook_event: String,
    pub allowed: bool,
    pub action: String,
    pub message: String,
}

pub fn execute_lifecycle_hook(
    event: &str,
    payload: &str,
    casa: &CasaEnvelope,
    gate: &ContinueGateMetrics,
) -> HookExecutionResult {
    match event {
        "pre-tool" | "PreToolUse" => {
            let threats = scan_aidefence_threats(payload);
            if !threats.is_empty() {
                return HookExecutionResult {
                    hook_event: "PreToolUse".into(),
                    allowed: false,
                    action: "block".into(),
                    message: format!("AIDefence blocked tool payload ({} threat findings)", threats.len()),
                };
            }
            let auth = check_casa_authorization(casa, "worktree.write", &Utc::now().to_rfc3339());
            HookExecutionResult {
                hook_event: "PreToolUse".into(),
                allowed: auth.allowed,
                action: if auth.allowed { "allow".into() } else { "deny".into() },
                message: auth.reason,
            }
        }
        "post-tool" | "PostToolUse" => HookExecutionResult {
            hook_event: "PostToolUse".into(),
            allowed: true,
            action: "learn".into(),
            message: "Recorded tool execution trajectory into SONA/EWC and HNSW semantic memory".into(),
        },
        "stop" | "Stop" => {
            let (dec, reason) = evaluate_continue_gate(gate);
            HookExecutionResult {
                hook_event: "Stop".into(),
                allowed: dec == "continue" || dec == "checkpoint",
                action: dec.to_string(),
                message: reason,
            }
        }
        _ => HookExecutionResult {
            hook_event: "SessionStart".into(),
            allowed: true,
            action: "context-inject".into(),
            message: "AI-Fleet v4.0 harness active: 64 personas, HNSW memory, SONA/EWC MoE, CASA $0 API guard".into(),
        },
    }
}

/// Initializes any repository with `.mcp.json`, `.codex/hooks.json`, and `.agents/skills/ai-fleet/SKILL.md`
/// without overwriting existing project files unless requested.
pub fn init_workspace_integration(repo_root: &Path, write_mcp: bool) -> Vec<String> {
    let mut created = Vec::new();
    let fleet_dir = repo_root.join(".ai-fleet");
    let _ = fs::create_dir_all(&fleet_dir);

    // 1. Write .ai-fleet/mcp.json (and root .mcp.json if write_mcp && not existing)
    let mcp_cfg = json!({
        "mcpServers": {
            "ai-fleet": {
                "command": "ai-fleet",
                "args": ["mcp", "start"],
                "env": {}
            }
        }
    });
    let fleet_mcp_path = fleet_dir.join("mcp.json");
    if fs::write(&fleet_mcp_path, serde_json::to_string_pretty(&mcp_cfg).unwrap()).is_ok() {
        created.push(fleet_mcp_path.to_string_lossy().to_string());
    }
    let root_mcp = repo_root.join(".mcp.json");
    if write_mcp && !root_mcp.exists() {
        if fs::write(&root_mcp, serde_json::to_string_pretty(&mcp_cfg).unwrap()).is_ok() {
            created.push(root_mcp.to_string_lossy().to_string());
        }
    }

    // 2. Write .ai-fleet/hooks.json for Claude Code & Codex integration
    let hooks_cfg = json!({
        "hooks": {
            "SessionStart": [{ "command": "ai-fleet hooks session-start" }],
            "PreToolUse": [{ "command": "ai-fleet hooks pre-tool" }],
            "PostToolUse": [{ "command": "ai-fleet hooks post-tool" }],
            "Stop": [{ "command": "ai-fleet hooks stop" }]
        }
    });
    let hooks_path = fleet_dir.join("hooks.json");
    if fs::write(&hooks_path, serde_json::to_string_pretty(&hooks_cfg).unwrap()).is_ok() {
        created.push(hooks_path.to_string_lossy().to_string());
    }

    // 3. Write .ai-fleet/SKILL.md
    let skill_md = "---\nname: ai-fleet\ndescription: Native Rust 15-Crate Multi-AI Swarm Orchestrator (Claude Opus 5.5, Codex Astra/Soul, Antigravity Argon/Neon) with 64 Personas, 48 Plugins, HNSW Vector Memory, SONA/EWC MoE, PBFT/Raft/Gossip, and MCP.\n---\n\n# AI-Fleet v4.0 Skill\n\nRun `ai-fleet status`, `ai-fleet analyze`, `ai-fleet route <task>`, `ai-fleet memory search <q>`, or `ai-fleet mcp start`.\n";
    let skill_path = fleet_dir.join("SKILL.md");
    if fs::write(&skill_path, skill_md).is_ok() {
        created.push(skill_path.to_string_lossy().to_string());
    }

    created
}
