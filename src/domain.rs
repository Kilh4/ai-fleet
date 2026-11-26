use crate::consensus::append_ledger;
use crate::router::{default_moe_experts, default_reasoning_bank, refresh_quota_resets_and_pheromones, ExpertSlot, ReasoningPattern};
use crate::security::{CasaEnvelope, ContinueGateMetrics};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
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
pub struct TestEvidence {
    pub command: String,
    pub output: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub worktree: String,
    pub head_commit: String,
    pub file_hashes: BTreeMap<String, Option<String>>,
    pub summary: String,
    pub tests: Vec<TestEvidence>,
    pub timestamp: String,
    pub author_provider: String,
    pub author_model: String,
    pub effort_used: String,
    pub lease_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryEvent {
    pub timestamp: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub tier: String,
    pub action: String,
    pub status: String,
    pub detail: String,
    pub file_hashes: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub id: String,
    pub title: String,
    pub base_commit: String,
    pub owned_files: Vec<String>,
    pub required_outputs: Vec<String>,
    pub steps: Vec<String>,
    pub current_step: usize,
    pub status: String,
    pub tier: String,
    pub target_effort: String,
    pub assigned_provider: Option<String>,
    pub assigned_model: Option<String>,
    pub active_worktree: Option<String>,
    pub authors: Vec<String>,
    pub reviewer: Option<String>,
    pub retries: u32,
    pub lease_epoch: u64,
    pub checkpoint: Option<Checkpoint>,
    pub next_step: String,
    pub history: Vec<TaskHistoryEvent>,
    pub ruflo_id: Option<String>,
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
pub struct MemoryEntry {
    pub key: String,
    pub namespace: String,
    pub value: String,
    pub tags: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusRound {
    pub proposal_id: String,
    pub topic: String,
    pub phase: String,
    pub view_number: u64,
    pub sequence_number: u64,
    pub votes: BTreeMap<String, (bool, f64)>,
    pub weighted_approval: f64,
    pub quorum_achieved: bool,
    pub digest: String,
    pub timestamp: String,
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
pub struct FleetState {
    pub version: String,
    pub topology: String,
    pub look_mode: String,
    pub max_agents: usize,
    pub autopilot_enabled: bool,
    pub budget_paused: bool,
    pub daemon_running: bool,
    pub daemon_pid: Option<u32>,
    pub global_epoch: u64,
    pub updated_at: String,
    pub repo_root: String,
    pub providers: BTreeMap<String, AiProvider>,
    pub tasks: BTreeMap<String, SwarmTask>,
    pub file_claims: BTreeMap<String, String>,
    pub fenced_leases: BTreeMap<String, FencedLease>,
    pub moe_experts: Vec<ExpertSlot>,
    pub reasoning_bank: Vec<ReasoningPattern>,
    pub casa_envelope: CasaEnvelope,
    pub continue_gate: ContinueGateMetrics,
    pub consensus_history: Vec<ConsensusRound>,
    pub memory: BTreeMap<String, MemoryEntry>,
    pub decision_ledger: Vec<DecisionLedgerRow>,
}

pub fn default_providers() -> BTreeMap<String, AiProvider> {
    let mut map = BTreeMap::new();

    map.insert(
        "claude".to_string(),
        AiProvider {
            id: "claude".to_string(),
            display_name: "Claude Code (Anthropic)".to_string(),
            binary: "claude".to_string(),
            args_template: vec![
                "-p".into(),
                "--model".into(),
                "{model}".into(),
                "--output-format".into(),
                "json".into(),
                "--no-session-persistence".into(),
                "--strict-mcp-config".into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
            ],
            stdin_prompt: true,
            status: "quota".to_string(),
            active_model: "claude-opus-5-5".to_string(),
            active_effort: "max".to_string(),
            daily_quota_limit: 45,
            daily_quota_used: 45,
            retry_at: Some("2026-11-26T08:20:00+03:00".to_string()),
            last_detail: "5-hour limit reached; auto-resumes at 08:20 Asia/Riyadh".to_string(),
            models: vec![
                ModelSpec {
                    name: "claude-opus-5-5".to_string(),
                    tier: "critical".to_string(),
                    default_effort: "max".to_string(),
                    q_score: 0.985,
                    pheromone: 1.45,
                    trust_score: 0.99,
                    calls_today: 42,
                    successes: 41,
                    failures: 1,
                    avg_latency_ms: 14200,
                },
                ModelSpec {
                    name: "claude-sonnet-4-5".to_string(),
                    tier: "standard".to_string(),
                    default_effort: "high".to_string(),
                    q_score: 0.915,
                    pheromone: 1.05,
                    trust_score: 0.94,
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
            id: "codex".to_string(),
            display_name: "OpenAI Codex (Astra / Soul)".to_string(),
            binary: "codex".to_string(),
            args_template: vec![
                "exec".into(),
                "--ignore-user-config".into(),
                "--ephemeral".into(),
                "-s".into(),
                "workspace-write".into(),
                "-m".into(),
                "{model}".into(),
                "-c".into(),
                "model_reasoning_effort=\"{effort}\"".into(),
                "-c".into(),
                "forced_login_method=\"chatgpt\"".into(),
                "--json".into(),
                "-C".into(),
                "{worktree}".into(),
                "-".into(),
            ],
            stdin_prompt: true,
            status: "ready".to_string(),
            active_model: "gpt-6.2-astra".to_string(),
            active_effort: "xhigh".to_string(),
            daily_quota_limit: 120,
            daily_quota_used: 18,
            retry_at: None,
            last_detail: "Logged in using ChatGPT subscription (API billing blocked)".to_string(),
            models: vec![
                ModelSpec {
                    name: "gpt-6.2-astra".to_string(),
                    tier: "critical".to_string(),
                    default_effort: "xhigh".to_string(),
                    q_score: 0.968,
                    pheromone: 1.38,
                    trust_score: 0.97,
                    calls_today: 12,
                    successes: 12,
                    failures: 0,
                    avg_latency_ms: 11500,
                },
                ModelSpec {
                    name: "soul".to_string(),
                    tier: "deep".to_string(),
                    default_effort: "high".to_string(),
                    q_score: 0.942,
                    pheromone: 1.22,
                    trust_score: 0.95,
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
            id: "antigravity".to_string(),
            display_name: "Google Antigravity (Argon / Neon)".to_string(),
            binary: "agy".to_string(),
            args_template: vec![
                "--sandbox".into(),
                "--mode".into(),
                "accept-edits".into(),
                "--disable-slash-commands".into(),
                "--model".into(),
                "{model}".into(),
                "-p".into(),
                "{prompt}".into(),
            ],
            stdin_prompt: false,
            status: "ready".to_string(),
            active_model: "argon".to_string(),
            active_effort: "xhigh".to_string(),
            daily_quota_limit: 150,
            daily_quota_used: 9,
            retry_at: None,
            last_detail: "Active OAuth session; Argon & Gemini 4 Neon routing ready".to_string(),
            models: vec![
                ModelSpec {
                    name: "argon".to_string(),
                    tier: "critical".to_string(),
                    default_effort: "xhigh".to_string(),
                    q_score: 0.966,
                    pheromone: 1.40,
                    trust_score: 0.97,
                    calls_today: 5,
                    successes: 5,
                    failures: 0,
                    avg_latency_ms: 9400,
                },
                ModelSpec {
                    name: "gemini-4-neon".to_string(),
                    tier: "deep".to_string(),
                    default_effort: "high".to_string(),
                    q_score: 0.935,
                    pheromone: 1.18,
                    trust_score: 0.95,
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
    "HEAD (uncommitted/new-project)".to_string()
}

pub fn state_file_path() -> PathBuf {
    if let Ok(custom) = env::var("AI_FLEET_STATE") {
        return PathBuf::from(custom);
    }
    let root = detect_workspace_root();
    root.join(".ai-fleet").join("state.json")
}

pub fn load_or_init_state() -> FleetState {
    let path = state_file_path();
    if path.is_file() {
        if let Ok(txt) = fs::read_to_string(&path) {
            if let Ok(mut st) = serde_json::from_str::<FleetState>(&txt) {
                refresh_quota_resets_and_pheromones(&mut st);
                return st;
            }
        }
    }

    let ws_root = detect_workspace_root();
    let head_commit = detect_head_commit(&ws_root);
    let repo_root = ws_root.to_string_lossy().to_string();
    let mut tasks = BTreeMap::new();
    tasks.insert(
        "arch-review".to_string(),
        SwarmTask {
            id: "arch-review".to_string(),
            title: "Architecture, Security & Code Quality Verification".to_string(),
            base_commit: head_commit.clone(),
            owned_files: vec![".ai-fleet/reports/arch-review.md".to_string()],
            required_outputs: vec![".ai-fleet/reports/arch-review.md".to_string()],
            steps: vec![
                "Inspect project structure, verify build/tests, and write architecture report".to_string(),
            ],
            current_step: 0,
            status: "queued".to_string(),
            tier: "deep".to_string(),
            target_effort: "high".to_string(),
            assigned_provider: Some("codex".to_string()),
            assigned_model: Some("gpt-6.2-astra".to_string()),
            active_worktree: None,
            authors: vec![],
            reviewer: None,
            retries: 0,
            lease_epoch: 1,
            checkpoint: None,
            next_step: "Run initial parallel inspection in isolated git worktree.".to_string(),
            history: vec![],
            ruflo_id: None,
        },
    );
    tasks.insert(
        "critical-guard".to_string(),
        SwarmTask {
            id: "critical-guard".to_string(),
            title: "Critical Logic, Database Migration & RLS Security Audit".to_string(),
            base_commit: head_commit,
            owned_files: vec![".ai-fleet/reports/security-audit.md".to_string()],
            required_outputs: vec![],
            steps: vec![
                "Audit critical domain rules, migrations, and security constraints with xhigh reasoning effort".to_string(),
            ],
            current_step: 0,
            status: "queued".to_string(),
            tier: "critical".to_string(),
            target_effort: "xhigh".to_string(),
            assigned_provider: Some("antigravity".to_string()),
            assigned_model: Some("argon".to_string()),
            active_worktree: None,
            authors: vec!["claude".to_string()],
            reviewer: None,
            retries: 0,
            lease_epoch: 1,
            checkpoint: None,
            next_step: "Independent review by non-author agent with xhigh effort.".to_string(),
            history: vec![],
            ruflo_id: None,
        },
    );

    let mut file_claims = BTreeMap::new();
    file_claims.insert(".ai-fleet/reports/arch-review.md".to_string(), "arch-review".to_string());
    file_claims.insert(
        ".ai-fleet/reports/security-audit.md".to_string(),
        "critical-guard".to_string(),
    );

    let mut memory = BTreeMap::new();
    memory.insert(
        "policy.review".to_string(),
        MemoryEntry {
            key: "policy.review".to_string(),
            namespace: "global".to_string(),
            value: "Every change runs in an isolated FencedLease worktree and is reviewed by a non-author AI provider.".to_string(),
            tags: vec!["policy".into(), "review".into()],
            updated_at: Utc::now().to_rfc3339(),
        },
    );
    memory.insert(
        "policy.routing".to_string(),
        MemoryEntry {
            key: "policy.routing".to_string(),
            namespace: "global".to_string(),
            value: "8-Expert MoE + Pheromone Trails + Q-Learning dynamically selects model and escalates effort (medium -> high -> xhigh -> max).".to_string(),
            tags: vec!["routing".into(), "moe".into(), "pheromone".into()],
            updated_at: Utc::now().to_rfc3339(),
        },
    );

    let casa = CasaEnvelope {
        objective: "Autonomous multi-AI development, testing, and independent code review".to_string(),
        allow: vec![
            "repository.read".into(),
            "worktree.write".into(),
            "tests.execute".into(),
            "build.verify".into(),
            "consensus.vote".into(),
        ],
        deny: vec![
            "git.force_push".into(),
            "secret.export".into(),
            "api.paid_billing".into(),
            "production.drop_database".into(),
        ],
        budget_usd: 0.0,
        expires_at: (Utc::now() + Duration::days(365)).to_rfc3339(),
    };

    let mut st = FleetState {
        version: "2.0.0-rust".to_string(),
        topology: "hierarchical-mesh".to_string(),
        look_mode: "bbs".to_string(),
        max_agents: 15,
        autopilot_enabled: true,
        budget_paused: false,
        daemon_running: false,
        daemon_pid: None,
        global_epoch: 1,
        updated_at: Utc::now().to_rfc3339(),
        repo_root,
        providers: default_providers(),
        tasks,
        file_claims,
        fenced_leases: BTreeMap::new(),
        moe_experts: default_moe_experts(),
        reasoning_bank: default_reasoning_bank(),
        casa_envelope: casa,
        continue_gate: ContinueGateMetrics::default(),
        consensus_history: Vec::new(),
        memory,
        decision_ledger: Vec::new(),
    };
    append_ledger(
        &mut st,
        "system:init",
        "fleet.v2.init",
        "Initialized AI-Fleet v2.0 (MoE 8-Experts, Pheromones, CASA, ContinueGate, AIDefence, PBFT Consensus)",
    );
    save_state(&st);
    st
}

pub fn save_state(state: &FleetState) {
    let path = state_file_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}
