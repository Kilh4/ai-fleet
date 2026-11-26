# 🚀 AI-Fleet (`v2.0.0-rust`)

**Enterprise Multi-AI Swarm Orchestrator in Native Rust** — Coordinate **Claude Code (`Opus 5.5` / `Sonnet 4.5`)**, **OpenAI Codex (`gpt-6.2-astra` / `soul`)**, **Google Antigravity (`Argon` / `Gemini 4 Neon`)**, and any custom AI CLI simultaneously across isolated Git worktrees with dynamic **8-Expert Mixture-of-Experts (MoE)** routing, **Pheromone-Adaptive Scheduling**, **Q-Learning**, and **Zero-Loss Quota Failover**.

---

## ✨ Key Architectural Advantages (Supersedes Ruflo V3)

| Capability | Legacy Node/TS Orchestrators (`Ruflo`) | `AI-Fleet v2.0` (Native Rust) |
| :--- | :--- | :--- |
| **Runtime & Dependencies** | Node.js + npm cache (`zod` / `EPERM` issues on Windows) | **Single Zero-Overhead Native Rust Binary (`ai-fleet.exe`)** |
| **Multi-AI Execution** | Tied primarily to Claude or paid API keys | **Runs Claude + Codex + Antigravity + Custom AIs in parallel** |
| **Billing Protection** | Risk of accidental API key spend / 3rd-party proxy | **Strict Subscription-Only Sanitizer + CASA `$0.00` Paid-API Deny Gate** |
| **Adaptive Model & Effort Routing** | Static model selection | **8-Expert MoE + Pheromone Trails + Q-Learning (`low` -> `medium` -> `high` -> `xhigh` -> `max`)** |
| **Quota Failover & Recovery** | Halts when 5-hour limit hits | **Circuit-Breaker + Auto-Resume Timer + Epoch-Fenced Worktree Handoff** |
| **Security & Governance** | External plugins | **Built-in CASA (ADR-380), AIDefence (ADR-118), ContinueGate, PBFT Consensus & SHA-256 Ledger** |

---

## 🏗 Clean Modular Architecture

```text
ai-fleet/
├── Cargo.toml                  # Crate manifest (library + CLI binary)
├── src/
│   ├── lib.rs                  # Public SDK surface
│   ├── main.rs                 # CLI command router
│   ├── domain.rs               # Core entities (FleetState, AiProvider, SwarmTask, FencedLease)
│   ├── router.rs               # 8-Expert MoE gating, ADR-330 Pheromones, Q-Learning, ReasoningBank
│   ├── security.rs             # CASA Authority Envelope, AIDefence Shield, ContinueGate
│   ├── consensus.rs            # 4-Phase PBFT Weighted Consensus & SHA-256 Decision Ledger
│   ├── executor.rs             # Parallel worktree runner, env sanitizer & live failover drill
│   └── tui.rs                  # Rich ANSI/Unicode Terminal UI (BBS & Orbital views)
├── tests/
│   └── integration_tests.rs    # Full automated test suite (MoE, CASA, AIDefence, PBFT, Ledger)
└── .github/workflows/ci.yml    # Cross-platform GitHub Actions CI (Linux, Windows, macOS)
```

---

## ⚡ Quick Start

### 1. Build & Install
```bash
cargo build --release
cargo test
```

### 2. Core Commands
```bash
# Launch live visual dashboard (BBS or Orbital look)
ai-fleet status
ai-fleet status --look orbital

# Start parallel orchestration across all ready AI providers
ai-fleet start

# Drain active writers safely and preserve checkpoints
ai-fleet stop

# Run live failover, FencedLease epoch escalation & PBFT consensus drill
ai-fleet drill
```

### 3. Connect Any AI & Switch Models / Reasoning Effort
```bash
# Connect any new AI CLI tool (e.g., Ollama, Gemini, Aider, Cursor)
ai-fleet provider add ollama ollama.exe qwen3-coder:30b high run {model} "{prompt}"

# Dynamically switch active model and reasoning effort
ai-fleet provider switch codex soul xhigh
ai-fleet provider switch antigravity argon xhigh

# Update or reset 5-hour / daily quota window
ai-fleet provider quota claude 0 45 ready
```

### 4. Advanced Subsystems
```bash
# 8-Expert Mixture-of-Experts neural gating
ai-fleet moe "Audit SQL migration money scale and RLS security policies"

# Inspect or evaporate/boost ADR-330 pheromone trails
ai-fleet pheromone
ai-fleet pheromone boost argon

# Scan prompts for ADR-118 prompt injection & secret exposure
ai-fleet defense " task prompt "

# Verify CASA (ADR-380 §3) deterministic authority gate
ai-fleet casa worktree.write

# Run 4-phase PBFT weighted consensus across all models
ai-fleet consensus "Approve release gate"

# Verify SHA-256 hash-chained decision ledger
ai-fleet policy verify
```

---

## 📄 License
Dual-licensed under **MIT** or **Apache-2.0**.
