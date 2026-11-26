//! `fleet-core` — Enterprise domain types, cryptographic hashing, hash-chained
//! decision ledger, and workspace discovery for AI-Fleet v3.0.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelSpec {
    pub name: String,
    pub tier: String,
    pub default_effort: String,
    pub q_score: f64,
    pub pheromone: f64,
    pub trust_score: f64,
    pub ewc_fisher_importance: f64,
    pub calls_today: u64,
    pub successes: u64,
    pub failures: u64,
    pub avg_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: String,
    pub display_name: String,
    pub binary: String,
    pub args_template: Vec<String>,
    pub stdin_prompt: bool,
    pub status: String,
    pub active_model: String,
    pub active_effort: String,
    pub daily_quota_limit: u64,
    pub daily_quota_used: u64,
    pub retry_at: Option<String>,
    pub last_detail: String,
    pub models: Vec<ModelSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskSpec {
    pub id: String,
    pub title: String,
    pub assigned_persona: String,
    pub domain: String,
    pub status: String,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub id: String,
    pub title: String,
    pub base_commit: String,
    pub owned_files: Vec<String>,
    pub required_outputs: Vec<String>,
    pub sparc_phase: String, // "Spec", "Pseudocode", "Architecture", "Refinement", "Completion"
    pub subtasks: Vec<SubTaskSpec>,
    pub status: String,
    pub tier: String,
    pub target_effort: String,
    pub assigned_provider: Option<String>,
    pub assigned_model: Option<String>,
    pub assigned_persona: Option<String>,
    pub active_worktree: Option<String>,
    pub authors: Vec<String>,
    pub reviewer: Option<String>,
    pub retries: u32,
    pub lease_epoch: u64,
    pub next_step: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FencedLease {
    pub lease_id: String,
    pub task_id: String,
    pub provider_id: String,
    pub model_name: String,
    pub worktree_path: String,
    pub branch: String,
    pub scopes: Vec<String>,
    pub epoch: u64,
    pub issued_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionLedgerRow {
    pub seq: u64,
    pub timestamp: String,
    pub actor: String,
    pub action: String,
    pub payload_summary: String,
    pub prev_hash: String,
    pub row_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruthAnchor {
    pub anchor_id: String,
    pub invariant: String,
    pub source_authority: String,
    pub signature_sha256: String,
    pub immutable: bool,
}

pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub fn sha256_str(text: &str) -> String {
    sha256_bytes(text.as_bytes())
}

pub fn sha256_file(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|b| sha256_bytes(&b))
}

pub fn append_ledger_row(ledger: &mut Vec<DecisionLedgerRow>, actor: &str, action: &str, summary: &str) -> DecisionLedgerRow {
    let seq = (ledger.len() as u64) + 1;
    let prev_hash = ledger
        .last()
        .map(|r| r.row_hash.clone())
        .unwrap_or_else(|| "0".repeat(64));
    let ts = Utc::now().to_rfc3339();
    let material = format!("{}|{}|{}|{}|{}|{}", seq, ts, actor, action, summary, prev_hash);
    let row_hash = sha256_bytes(material.as_bytes());
    let row = DecisionLedgerRow {
        seq,
        timestamp: ts,
        actor: actor.to_string(),
        action: action.to_string(),
        payload_summary: summary.to_string(),
        prev_hash,
        row_hash,
    };
    ledger.push(row.clone());
    row
}

pub fn verify_ledger_chain(ledger: &[DecisionLedgerRow]) -> Result<usize, String> {
    let mut expected_prev = "0".repeat(64);
    for (idx, row) in ledger.iter().enumerate() {
        if row.seq != (idx as u64) + 1 {
            return Err(format!("Sequence mismatch at row {}", idx + 1));
        }
        if row.prev_hash != expected_prev {
            return Err(format!("Broken hash chain at row {}", row.seq));
        }
        let material = format!(
            "{}|{}|{}|{}|{}|{}",
            row.seq, row.timestamp, row.actor, row.action, row.payload_summary, row.prev_hash
        );
        let calc = sha256_bytes(material.as_bytes());
        if calc != row.row_hash {
            return Err(format!("Tampered ledger row {} (hash mismatch)", row.seq));
        }
        expected_prev = row.row_hash.clone();
    }
    Ok(ledger.len())
}

pub fn detect_workspace_root() -> PathBuf {
    if let Ok(custom) = env::var("AI_FLEET_WORKSPACE") {
        return PathBuf::from(custom);
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&cwd)
        .output()
    {
        if out.status.success() {
            let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !root.is_empty() {
                return PathBuf::from(root);
            }
        }
    }
    cwd
}

pub fn detect_head_commit(root: &Path) -> String {
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
    {
        if out.status.success() {
            let h = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !h.is_empty() {
                return h;
            }
        }
    }
    "HEAD".to_string()
}

pub fn default_providers_map() -> BTreeMap<String, AiProvider> {
    let mut map = BTreeMap::new();
    map.insert(
        "claude".to_string(),
        AiProvider {
            id: "claude".into(),
            display_name: "Claude Code (Anthropic)".into(),
            binary: "claude".into(),
            args_template: vec![
                "-p".into(), "--model".into(), "{model}".into(),
                "--output-format".into(), "json".into(),
                "--no-session-persistence".into(), "--strict-mcp-config".into(),
                "--permission-mode".into(), "acceptEdits".into(),
            ],
            stdin_prompt: true,
            status: "quota".into(),
            active_model: "claude-opus-5-5".into(),
            active_effort: "max".into(),
            daily_quota_limit: 45,
            daily_quota_used: 45,
            retry_at: Some("2026-11-26T08:20:00+03:00".into()),
            last_detail: "5-hour limit reached; auto-resumes at 08:20 Asia/Riyadh".into(),
            models: vec![
                ModelSpec {
                    name: "claude-opus-5-5".into(),
                    tier: "critical".into(),
                    default_effort: "max".into(),
                    q_score: 0.985,
                    pheromone: 1.48,
                    trust_score: 0.99,
                    ewc_fisher_importance: 0.96,
                    calls_today: 42,
                    successes: 41,
                    failures: 1,
                    avg_latency_ms: 14200,
                },
                ModelSpec {
                    name: "claude-sonnet-4-5".into(),
                    tier: "standard".into(),
                    default_effort: "high".into(),
                    q_score: 0.918,
                    pheromone: 1.08,
                    trust_score: 0.94,
                    ewc_fisher_importance: 0.84,
                    calls_today: 3,
                    successes: 3,
                    failures: 0,
                    avg_latency_ms: 6800,
                },
            ],
        },
    );
    map.insert(
        "codex".to_string(),
        AiProvider {
            id: "codex".into(),
            display_name: "OpenAI Codex (Astra / Soul)".into(),
            binary: "codex".into(),
            args_template: vec![
                "exec".into(), "--ignore-user-config".into(), "--ephemeral".into(),
                "-s".into(), "workspace-write".into(), "-m".into(), "{model}".into(),
                "-c".into(), "model_reasoning_effort=\"{effort}\"".into(),
                "-c".into(), "forced_login_method=\"chatgpt\"".into(),
                "--json".into(), "-C".into(), "{worktree}".into(), "-".into(),
            ],
            stdin_prompt: true,
            status: "ready".into(),
            active_model: "gpt-6.2-astra".into(),
            active_effort: "xhigh".into(),
            daily_quota_limit: 120,
            daily_quota_used: 18,
            retry_at: None,
            last_detail: "Logged in using ChatGPT subscription (API billing blocked)".into(),
            models: vec![
                ModelSpec {
                    name: "gpt-6.2-astra".into(),
                    tier: "critical".into(),
                    default_effort: "xhigh".into(),
                    q_score: 0.971,
                    pheromone: 1.41,
                    trust_score: 0.97,
                    ewc_fisher_importance: 0.94,
                    calls_today: 12,
                    successes: 12,
                    failures: 0,
                    avg_latency_ms: 11500,
                },
                ModelSpec {
                    name: "soul".into(),
                    tier: "deep".into(),
                    default_effort: "high".into(),
                    q_score: 0.945,
                    pheromone: 1.24,
                    trust_score: 0.95,
                    ewc_fisher_importance: 0.89,
                    calls_today: 6,
                    successes: 6,
                    failures: 0,
                    avg_latency_ms: 7900,
                },
            ],
        },
    );
    map.insert(
        "antigravity".to_string(),
        AiProvider {
            id: "antigravity".into(),
            display_name: "Google Antigravity (Argon / Neon)".into(),
            binary: "agy".into(),
            args_template: vec![
                "--sandbox".into(), "--mode".into(), "accept-edits".into(),
                "--disable-slash-commands".into(), "--model".into(), "{model}".into(),
                "-p".into(), "{prompt}".into(),
            ],
            stdin_prompt: false,
            status: "ready".into(),
            active_model: "argon".into(),
            active_effort: "xhigh".into(),
            daily_quota_limit: 150,
            daily_quota_used: 9,
            retry_at: None,
            last_detail: "Active OAuth session; Argon & Gemini 4 Neon routing ready".into(),
            models: vec![
                ModelSpec {
                    name: "argon".into(),
                    tier: "critical".into(),
                    default_effort: "xhigh".into(),
                    q_score: 0.969,
                    pheromone: 1.43,
                    trust_score: 0.97,
                    ewc_fisher_importance: 0.95,
                    calls_today: 5,
                    successes: 5,
                    failures: 0,
                    avg_latency_ms: 9400,
                },
                ModelSpec {
                    name: "gemini-4-neon".into(),
                    tier: "deep".into(),
                    default_effort: "high".into(),
                    q_score: 0.938,
                    pheromone: 1.20,
                    trust_score: 0.95,
                    ewc_fisher_importance: 0.88,
                    calls_today: 4,
                    successes: 4,
                    failures: 0,
                    avg_latency_ms: 5200,
                },
            ],
        },
    );
    map
}
