# 🚀 AI-Fleet v3.0 (`12-Crate Rust Enterprise Monorepo`)

**Enterprise Multi-AI Swarm Orchestration Platform in Native Rust** — Orchestrate **Claude Code (`Opus 5.5` / `Sonnet 4.5`)**, **OpenAI Codex (`gpt-6.2-astra` / `soul`)**, **Google Antigravity (`Argon` / `Gemini 4 Neon`)**, and any custom AI CLI simultaneously across isolated `FencedLease` Git worktrees.

Built as a **12-Crate Cargo Workspace** that ports and supersedes **Ruflo V3**:
- **64 Specialized Agent Personas** across 8 enterprise domains (`fleet-agents`)
- **64-Dim HNSW Semantic Vector Memory Graph** with sub-millisecond hybrid cosine + lexical retrieval (`fleet-memory`)
- **2-Layer 8-Expert Neural Mixture-of-Experts (MoE)** + **SONA / EWC Fisher Information Consolidation** + **Information-Bottleneck Token Compressor** (`fleet-neural`)
- **Queen Hive-Mind Task Decomposer** + **3 Consensus Engines (`PBFT Byzantine`, `Raft`, `Gossip`)** + **ADR-330 Pheromone Trails** + **Real Git Worktree Coordinator** (`fleet-swarm`)
- **CASA Deterministic Authority Gate (`ADR-380 §3`)** + **AIDefence Prompt-Injection & Secret Shield (`ADR-118`)** + **ContinueGate** + **Cryptographic Truth Anchors** (`fleet-security`)
- **SPARC 5-Phase Methodology Pipeline**, **Pair Programming Driver/Navigator**, and **12 Autonomous Daemon Workers** (`fleet-workflows`)
- **Cross-Team Federation Hub** with **Automatic PII/Secret Redaction** and **Gumbel-Softmax Statistical Cryptographic Watermarking** (`fleet-federation`)
- **Standards-Compliant Model Context Protocol (`MCP JSON-RPC 2.0 over stdio`) Server** (`fleet-mcp`)
- **8-View Multi-Page ANSI/Unicode Terminal UI (`BBS` & `Orbital` modes)** (`fleet-tui`)

---

## 🏗 12-Crate Workspace Architecture

```text
ai-fleet/
├── Cargo.toml                      # Workspace root (resolver = "2", 12 members)
├── crates/
│   ├── fleet-core/                 # Domain entities, SHA-256 hash-chained decision ledger, workspace discovery
│   ├── fleet-agents/               # 64 specialized agent personas across 8 domains + capability matcher
│   ├── fleet-memory/               # 64-dim HNSW semantic vector graph, n-gram embeddings & hybrid search
│   ├── fleet-neural/               # 2-Layer 8-Expert MoE, SONA/EWC Fisher consolidation & token compressor
│   ├── fleet-security/             # CASA (ADR-380), AIDefence (ADR-118), ContinueGate & TruthAnchors
│   ├── fleet-swarm/                # Queen task decomposer, PBFT/Raft/Gossip consensus, Pheromones & Git worktrees
│   ├── fleet-workflows/            # SPARC 5-phase methodology, Pair programming & 12 daemon workers
│   ├── fleet-federation/           # Cross-team PII redactor & Gumbel-Softmax statistical watermark verifier
│   ├── fleet-providers/            # Multi-AI CLI runner, subscription-only sanitizer & quota circuit-breaker
│   ├── fleet-mcp/                  # Native JSON-RPC 2.0 MCP server (10 tools over stdio & CLI exec)
│   └── fleet-tui/                  # 8-view terminal UI console (overview, agents, workflows, mcp, orbital)
└── bins/
    └── ai-fleet/                   # Unified CLI binary + workspace integration test suite
```

---

## ⚡ Quick Start & Commands

```bash
# Build & run all workspace tests
cargo test --workspace
cargo build --release

# 1. Multi-View Visual Console
ai-fleet status
ai-fleet status --look orbital
ai-fleet status --view agents
ai-fleet status --view workflows
ai-fleet status --view mcp

# 2. Parallel Swarm Execution & Live Failover Drill
ai-fleet start
ai-fleet drill
ai-fleet stop

# 3. Connect Any AI & Switch Models / Reasoning Effort (low -> medium -> high -> xhigh -> max)
ai-fleet provider add ollama ollama.exe qwen3-coder:30b high run {model} "{prompt}"
ai-fleet provider switch codex soul xhigh
ai-fleet provider switch antigravity argon xhigh
ai-fleet provider quota claude 0 45 ready

# 4. 64 Agent Personas, Queen Decomposition & SPARC Methodology
ai-fleet agents "PostgreSQL RLS policy audit"
ai-fleet queen security-rls-audit
ai-fleet sparc arch-verify

# 5. Neural MoE + SONA/EWC, HNSW Vector Memory & Context Compression
ai-fleet moe "Audit SQL migration money scale and RLS security policies"
ai-fleet memory store estr-core zatca.secp256k1 "ZATCA Phase-2 uses ECDSA secp256k1 and PIH chaining"
ai-fleet memory search "secp256k1 invoice signature"
ai-fleet compress 12 "Long inter-agent handoff message with sha256=ae90aaab and commit 530fc5e..."

# 6. PBFT / Raft / Gossip Consensus, AIDefence, CASA & Federation Watermarking
ai-fleet consensus PBFT "Approve Phase 0 Gate"
ai-fleet consensus Raft "Replicate swarm state"
ai-fleet defense " task prompt "
ai-fleet casa worktree.write
ai-fleet federate team-alpha "Payload with email apppbf@gmail.com and token=sk-ant-123"
ai-fleet policy verify

# 7. Native MCP Server (for Claude Code, Codex, Cursor, Antigravity)
ai-fleet mcp start
ai-fleet mcp exec fleet_swarm_status "{}"
```
