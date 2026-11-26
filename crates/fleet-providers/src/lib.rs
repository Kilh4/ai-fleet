//! `fleet-providers` — Master Workspace State aggregator, Multi-AI CLI Runner,
//! Subscription-Only Environment Sanitizer, and Quota Circuit-Breaker.

use chrono::{DateTime, Duration, Utc};
use fleet_core::{
    append_ledger_row, default_providers_map, detect_head_commit, detect_workspace_root, AiProvider,
    DecisionLedgerRow, FencedLease, SwarmTask, TruthAnchor,
};
use fleet_memory::{default_vector_memories, VectorMemoryNode};
use fleet_neural::{default_moe_experts, default_reasoning_bank, update_sona_ewc_weights, ExpertSlot, ReasoningPattern};
use fleet_security::{default_truth_anchors, CasaEnvelope, ContinueGateMetrics};
use fleet_swarm::{queen_decompose_task, ConsensusRound};
use fleet_workflows::{built_in_12_daemon_workers, create_pair_session, DaemonWorkerSpec, PairProgrammingSession};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFleetState {
    pub version: String,
    pub topology: String,
    pub look_mode: String,
    pub active_view: String,
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
    pub vector_memory: Vec<VectorMemoryNode>,
    pub casa_envelope: CasaEnvelope,
    pub continue_gate: ContinueGateMetrics,
    pub truth_anchors: Vec<TruthAnchor>,
    pub consensus_history: Vec<ConsensusRound>,
    pub daemon_workers: Vec<DaemonWorkerSpec>,
    pub pair_session: PairProgrammingSession,
    pub decision_ledger: Vec<DecisionLedgerRow>,
}

pub fn state_file_path() -> PathBuf {
    if let Ok(custom) = env::var("AI_FLEET_STATE") {
        return PathBuf::from(custom);
    }
    let root = detect_workspace_root();
    root.join(".ai-fleet").join("state-v3.json")
}

pub fn save_fleet_state(state: &WorkspaceFleetState) {
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

pub fn refresh_provider_quotas(state: &mut WorkspaceFleetState) {
    let now = Utc::now();
    for prov in state.providers.values_mut() {
        if prov.status == "quota" || prov.status == "network" {
            if let Some(ref ts) = prov.retry_at {
                if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
                    if dt.with_timezone(&Utc) <= now {
                        prov.status = "ready".to_string();
                        prov.retry_at = None;
                        prov.daily_quota_used = 0;
                        prov.last_detail = "Quota window reset automatically; back online".to_string();
                    }
                }
            }
        }
    }
}

pub fn load_or_init_fleet_state() -> WorkspaceFleetState {
    let path = state_file_path();
    if path.is_file() {
        if let Ok(txt) = fs::read_to_string(&path) {
            if let Ok(mut st) = serde_json::from_str::<WorkspaceFleetState>(&txt) {
                refresh_provider_quotas(&mut st);
                return st;
            }
        }
    }

    let ws_root = detect_workspace_root();
    let head = detect_head_commit(&ws_root);
    let mut tasks = BTreeMap::new();

    let mut t1 = SwarmTask {
        id: "arch-verify".into(),
        title: "Modular Architecture, Contracts & Build Verification".into(),
        base_commit: head.clone(),
        owned_files: vec![".ai-fleet/reports/arch-verify.md".into()],
        required_outputs: vec![".ai-fleet/reports/arch-verify.md".into()],
        sparc_phase: "Architecture".into(),
        subtasks: vec![],
        status: "queued".into(),
        tier: "deep".into(),
        target_effort: "high".into(),
        assigned_provider: Some("codex".into()),
        assigned_model: Some("gpt-6.2-astra".into()),
        assigned_persona: Some("system-architect".into()),
        active_worktree: None,
        authors: vec![],
        reviewer: None,
        retries: 0,
        lease_epoch: 1,
        next_step: "Run Queen sub-task decomposition and verify module boundaries".into(),
    };
    t1.subtasks = queen_decompose_task(&t1).subtasks;

    let mut t2 = SwarmTask {
        id: "security-rls-audit".into(),
        title: "Critical Database Migration, Money Scale & RLS Security Audit".into(),
        base_commit: head,
        owned_files: vec![".ai-fleet/reports/security-rls-audit.md".into()],
        required_outputs: vec![".ai-fleet/reports/security-rls-audit.md".into()],
        sparc_phase: "Refinement".into(),
        subtasks: vec![],
        status: "queued".into(),
        tier: "critical".into(),
        target_effort: "xhigh".into(),
        assigned_provider: Some("antigravity".into()),
        assigned_model: Some("argon".into()),
        assigned_persona: Some("rls-policy-auditor".into()),
        active_worktree: None,
        authors: vec!["claude".into()],
        reviewer: None,
        retries: 0,
        lease_epoch: 1,
        next_step: "Execute independent non-author security audit at xhigh reasoning effort".into(),
    };
    t2.subtasks = queen_decompose_task(&t2).subtasks;

    tasks.insert(t1.id.clone(), t1);
    tasks.insert(t2.id.clone(), t2);

    let mut claims = BTreeMap::new();
    claims.insert(".ai-fleet/reports/arch-verify.md".into(), "arch-verify".into());
    claims.insert(".ai-fleet/reports/security-rls-audit.md".into(), "security-rls-audit".into());

    let casa = CasaEnvelope {
        objective: "Enterprise multi-AI swarm development, HNSW memory, and PBFT/Raft verification".into(),
        allow: vec![
            "repository.read".into(),
            "worktree.write".into(),
            "tests.execute".into(),
            "build.verify".into(),
            "consensus.vote".into(),
            "mcp.serve".into(),
            "federation.exchange".into(),
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

    let mut ledger = Vec::new();
    append_ledger_row(
        &mut ledger,
        "system:v3.0",
        "workspace.init",
        "Initialized 12-Crate AI-Fleet v3.0 (64 Agents, HNSW Memory, SONA/EWC MoE, PBFT/Raft/Gossip, MCP Server, SPARC)",
    );

    let st = WorkspaceFleetState {
        version: "3.0.0-rust".into(),
        topology: "hierarchical-mesh".into(),
        look_mode: "bbs".into(),
        active_view: "overview".into(),
        max_agents: 15,
        autopilot_enabled: true,
        budget_paused: false,
        daemon_running: false,
        daemon_pid: None,
        global_epoch: 1,
        updated_at: Utc::now().to_rfc3339(),
        repo_root: ws_root.to_string_lossy().to_string(),
        providers: default_providers_map(),
        tasks,
        file_claims: claims,
        fenced_leases: BTreeMap::new(),
        moe_experts: default_moe_experts(),
        reasoning_bank: default_reasoning_bank(),
        vector_memory: default_vector_memories(),
        casa_envelope: casa,
        continue_gate: ContinueGateMetrics::default(),
        truth_anchors: default_truth_anchors(),
        consensus_history: Vec::new(),
        daemon_workers: built_in_12_daemon_workers(),
        pair_session: create_pair_session("Continuous Driver/Navigator TDD verification"),
        decision_ledger: ledger,
    };
    save_fleet_state(&st);
    st
}

pub fn record_outcome(
    state: &mut WorkspaceFleetState,
    provider_id: &str,
    model_name: &str,
    activated_expert: &str,
    reward: f64,
    latency_ms: u64,
) {
    let alpha = 0.18;
    let evaporation = 0.97;
    for p in state.providers.values_mut() {
        for m in &mut p.models {
            m.pheromone = (m.pheromone * evaporation).clamp(0.20, 2.00);
        }
    }
    if let Some(prov) = state.providers.get_mut(provider_id) {
        prov.daily_quota_used = prov.daily_quota_used.saturating_add(1);
        for m in &mut prov.models {
            if m.name == model_name {
                m.calls_today += 1;
                if reward >= 0.8 {
                    m.successes += 1;
                    m.pheromone = (m.pheromone + 0.14 * reward).clamp(0.20, 2.00);
                    m.trust_score = (m.trust_score * 0.95 + 0.05).clamp(0.10, 0.999);
                    m.ewc_fisher_importance = (m.ewc_fisher_importance * 0.96 + 0.04 * 0.98).clamp(0.50, 0.999);
                } else {
                    m.failures += 1;
                    m.pheromone = (m.pheromone * 0.75).clamp(0.20, 2.00);
                    m.trust_score = (m.trust_score * 0.90).clamp(0.10, 0.999);
                }
                m.q_score = ((1.0 - alpha) * m.q_score + alpha * reward).clamp(0.10, 0.999);
                m.avg_latency_ms = if m.avg_latency_ms == 0 {
                    latency_ms
                } else {
                    (m.avg_latency_ms * 3 + latency_ms) / 4
                };
            }
        }
    }
    update_sona_ewc_weights(&mut state.moe_experts, activated_expert, reward);
}

pub fn sanitize_subscription_env() -> Vec<(String, String)> {
    let forbidden: BTreeSet<&str> = [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ]
    .into_iter()
    .collect();

    env::vars()
        .filter(|(k, _)| !forbidden.contains(k.to_uppercase().as_str()))
        .collect()
}

pub fn execute_ai_cli(
    provider: &AiProvider,
    model: &str,
    effort: &str,
    worktree: &Path,
    prompt: &str,
) -> Result<String, (String, Option<String>, String)> {
    let mut cmd = Command::new(&provider.binary);
    cmd.current_dir(worktree);
    cmd.env_clear();
    for (k, v) in sanitize_subscription_env() {
        cmd.env(k, v);
    }
    cmd.env("CLAUDE_CODE_EFFORT_LEVEL", effort);

    for arg in &provider.args_template {
        let expanded = arg
            .replace("{model}", model)
            .replace("{effort}", effort)
            .replace("{worktree}", &worktree.to_string_lossy())
            .replace("{prompt}", prompt);
        cmd.arg(expanded);
    }

    if provider.stdin_prompt {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err((
                "unavailable".into(),
                None,
                format!("Binary '{}' could not be launched: {}", provider.binary, e),
            ))
        }
    };

    if provider.stdin_prompt {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return Err(("error".into(), None, e.to_string())),
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() && !stdout.is_empty() {
        Ok(stdout)
    } else {
        let combined = format!("{}\n{}", stdout, stderr);
        let low = combined.to_lowercase();
        let (kind, reset) = if low.contains("hit your limit") || low.contains("quota") || low.contains("resets") {
            ("quota".to_string(), Some((Utc::now() + Duration::hours(2)).to_rfc3339()))
        } else if low.contains("unauthenticated") || low.contains("login") || low.contains("401") {
            ("auth".to_string(), None)
        } else {
            ("error".to_string(), None)
        };
        Err((kind, reset, combined))
    }
}
