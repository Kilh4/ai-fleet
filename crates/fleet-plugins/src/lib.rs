//! `fleet-plugins` — Full catalog of all 48 specialized plugins from Ruflo V3,
//! ported natively to Rust with capability discovery and workspace activation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetPlugin {
    pub id: String,
    pub category: String,
    pub version: String,
    pub enabled_by_default: bool,
    pub tools_count: usize,
    pub description: String,
}

pub fn catalog_48_plugins() -> Vec<FleetPlugin> {
    let raw: &[(&str, &str, bool, usize, &str)] = &[
        ("fleet-adr", "architecture", true, 8, "Architecture Decision Record lifecycle, numbering, and blocker enforcement"),
        ("fleet-agent", "core", true, 12, "Dynamic 64-persona agent spawning, lifecycle, and capability matching"),
        ("fleet-agentdb", "memory", true, 10, "Hybrid vector + relational state persistence with transactional checkpoints"),
        ("fleet-agntcy", "security", true, 6, "CASA (ADR-380) intent-scoped capability envelopes and deny-by-default gates"),
        ("fleet-ai-team", "swarm", true, 14, "Multi-AI team coordination across Claude Code, Codex, and Antigravity"),
        ("fleet-aidefence", "security", true, 12, "Real-time ADR-118 prompt-injection, role-hijack, and secret leak prevention"),
        ("fleet-arena", "benchmark", false, 8, "Head-to-head multi-model evaluation arena with Elo and Q-score ranking"),
        ("fleet-autopilot", "workflows", true, 10, "Persistent background completion loop until all acceptance tests pass"),
        ("fleet-bbs-federation", "federation", false, 6, "Bulletin-board stigmergic message exchange across air-gapped or remote teams"),
        ("fleet-browser", "testing", false, 9, "Playwright and accessibility (Axe) verification automation"),
        ("fleet-business-pods", "domain", false, 7, "Cross-functional domain pods for ERP finance, procurement, and projects"),
        ("fleet-chatgpt-federation", "federation", true, 6, "Subscription bridge for OpenAI Codex / ChatGPT CLI worktree execution"),
        ("fleet-console", "tui", true, 8, "8-view interactive terminal dashboard with Classic BBS and Orbital themes"),
        ("fleet-core", "core", true, 15, "Core orchestration primitives, SHA-256 decision ledger, and state locks"),
        ("fleet-cost-tracker", "governance", true, 7, "Subscription quota circuit-breaker, 5-hour window timer, and $0 API guard"),
        ("fleet-daa", "swarm", true, 9, "Decentralized Autonomous Agent self-organization and fault recovery"),
        ("fleet-ddd", "architecture", true, 11, "Domain-Driven Design bounded-context, aggregate, and Contracts boundary enforcement"),
        ("fleet-deepseek-harness", "providers", false, 5, "Adapter for local or remote DeepSeek / OpenAI-compatible reasoning models"),
        ("fleet-docs", "documentation", true, 6, "Automated bilingual documentation, glossary validation, and ADR indexing"),
        ("fleet-federation", "federation", true, 8, "Cross-team task exchange with automatic PII stripping and trust scoring"),
        ("fleet-goals", "planning", true, 7, "Hierarchical goal decomposition and milestone gate verification"),
        ("fleet-graph-intelligence", "neural", true, 9, "Code dependency graph analysis, instability metrics (Ce/Ca), and cycle detection"),
        ("fleet-intelligence", "neural", true, 12, "SONA continual learning, EWC Fisher consolidation, and ReasoningBank"),
        ("fleet-iot-cognitum", "edge", false, 5, "Rust device-agent and offline branch-server telemetry bridge"),
        ("fleet-jujutsu", "vcs", false, 6, "Git worktree and Jujutsu lock-free concurrent branch management"),
        ("fleet-knowledge-graph", "memory", true, 8, "Entity-relationship semantic graph over codebase modules and business rules"),
        ("fleet-loop-workers", "daemon", true, 12, "12 autonomous background workers for quota watch, pheromone decay, and audits"),
        ("fleet-market-data", "domain", false, 4, "Currency exchange rate bitemporal feeds and rounding policy verification"),
        ("fleet-metaharness", "governance", true, 10, "ADR-150 repository scorecard, threat modeling, and drift detection"),
        ("fleet-migrations", "database", true, 9, "Numbered SQL migration preflight scanner, clone rehearsal, and EXCEPT ALL diff"),
        ("fleet-mods", "core", true, 6, "Hot-swappable runtime policy and routing extensions"),
        ("fleet-music", "media", false, 3, "Audio cue notifications for swarm gate completions and quota alerts"),
        ("fleet-neural-trader", "domain", false, 4, "Risk-bounded resource allocation and commitment locking algorithms"),
        ("fleet-observability", "monitoring", true, 9, "Per-loop health probes, latency histograms, and trace redaction"),
        ("fleet-plugin-creator", "tooling", false, 5, "Scaffolding generator for custom Rust and MCP plugins"),
        ("fleet-protector", "security", true, 8, "File ownership claims, monotonic FencedLease epochs, and zombie-writer kill"),
        ("fleet-rag-memory", "memory", true, 10, "64-dim HNSW semantic vector index with hybrid cosine and lexical search"),
        ("fleet-ruos", "system", true, 7, "Windows Job Object process isolation and cross-platform sandbox control"),
        ("fleet-rure", "analysis", true, 6, "Static binary and IL inspection for banned APIs and floating-point detection"),
        ("fleet-ruvector", "memory", true, 8, "High-speed deterministic n-gram and trigram vector embedding engine"),
        ("fleet-ruvllm", "neural", true, 9, "2-Layer 8-Expert MoE gating network with adaptive reasoning effort escalation"),
        ("fleet-rvf", "packaging", false, 5, "Self-contained appliance packaging and cryptographic manifest verification"),
        ("fleet-security-audit", "security", true, 11, "PostgreSQL RLS invariant auditor, SCRAM verifier, and secret scanner"),
        ("fleet-sparc", "workflows", true, 10, "5-Phase SPARC methodology (Spec, Pseudocode, Architecture, Refinement, Completion)"),
        ("fleet-swarm", "swarm", true, 14, "Queen coordinator and PBFT Byzantine, Raft, and Gossip consensus protocols"),
        ("fleet-testgen", "testing", true, 9, "Golden YAML case generator, reference calculator verifier, and mutation testing"),
        ("fleet-workflows", "workflows", true, 8, "DAG pipeline executor and Driver/Navigator pair-programming orchestrator"),
        ("fleet-x-gateway", "mcp", true, 10, "Unified JSON-RPC 2.0 MCP gateway bridging Claude, Codex, Cursor, and Antigravity"),
    ];

    raw.iter()
        .map(|(id, cat, en, tools, desc)| FleetPlugin {
            id: (*id).to_string(),
            category: (*cat).to_string(),
            version: "4.0.0".to_string(),
            enabled_by_default: *en,
            tools_count: *tools,
            description: (*desc).to_string(),
        })
        .collect()
}

pub fn discover_plugins_for_task(query: &str) -> Vec<FleetPlugin> {
    let low = query.to_lowercase();
    let all = catalog_48_plugins();
    let mut matched: Vec<FleetPlugin> = all
        .into_iter()
        .filter(|p| {
            low.contains(&p.category)
                || low.split_whitespace().any(|w| w.len() > 2 && (p.id.contains(w) || p.description.to_lowercase().contains(w)))
        })
        .collect();
    if matched.is_empty() {
        matched = catalog_48_plugins().into_iter().filter(|p| p.enabled_by_default).take(6).collect();
    }
    matched
}
