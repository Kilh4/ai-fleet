# 🚀 AI-Fleet v7.0 — Hexagonal Microkernel Multi-AI Swarm Platform

**Production-Grade Multi-AI Orchestration Platform in Native Rust (`20 Crates + Hexagonal Ports & Adapters + 120 Embedded Elite GitHub Skills`)** — Run **Claude Code (`Opus` / `Sonnet`)**, **OpenAI Codex (`Astra` / `Soul`)**, **Google Antigravity (`Argon` / `Neon`)**, and any custom AI CLI concurrently on personal subscriptions (`$0.00` API billing protection), governed by a **pure `fleet-kernel` Microkernel (`Port` traits, `DomainEventBus`, `CircuitBreaker`, `Bulkhead`, `WorkStealingBroker`, `PluginInterceptor` pipeline)**, **3-Layer DDD Bounded Contexts (`domain / application / infrastructure`)**, **SQLite WAL persistence**, **Queen Topological Wave-DAG planning**, **Hybrid FTS5 + 128-D Vector Reflexion Memory**, **Sherman-Morrison LinUCB Contextual Bandit**, **$3f+1$ PBFT / Raft / Vector-Clock CRDT Consensus**, and **Automated Architecture Fitness Tests**.

> 📐 **Full C4 & Hexagonal Architecture Blueprint:** [`docs/architecture/ARCHITECTURE.md`](docs/architecture/ARCHITECTURE.md)

---

## 🔥 Core Capabilities (`crates/fleet-engine`)

1. **Real Multi-AI Async Execution & Zero-Loss Quota Failover (`engine.rs`, `providers.rs`, `proc.rs`, `git.rs`)**
   - Streams `claude -p --output-format stream-json`, `codex exec --json`, and custom CLIs concurrently under `tokio`.
   - Strips metered `*_API_KEY` environment variables automatically so tools strictly use personal subscriptions.
   - Detects subscription quota limits & parses exact reset timestamps (`try again in 2h 5m`, `Retry-After`, clock resets, epoch tags).
   - Commits partial work onto isolated per-task Git worktrees (`ai-fleet/<task-id>`) and seamlessly hands off to the next healthy AI provider without losing a single byte.
   - Contains every child/grandchild process inside **Windows Job Objects (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`)** / Unix process groups, enforcing wall-clock and idle-stall timeouts.
   - Uses monotonic **SQLite Lease Fencing (`lease_epoch`)** so crashed or zombie workers can never overwrite newer task state.

2. **120 Hand-Verified Elite GitHub Agent Skills (`skills/elite/` + `skills.rs`)**
   - Compiled directly into `ai-fleet.exe` (`build.rs`) from the highest-starred `MIT` / `Apache-2.0` repositories on GitHub (`obra/superpowers` ★327K, `affaan-m/ECC` ★311K, `DietrichGebert/ponytail` ★199K, `msitarzewski/agency-agents` ★170K, `nextlevelbuilder/ui-ux-pro-max-skill` ★154K, `Graphify-Labs/graphify` ★153K, `Leonxlnx/taste-skill` ★121K, `addyosmani/agent-skills` ★120K, `pbakaus/impeccable` ★117K, `coreyhaines31/marketingskills` ★62K, `emilkowalski/skills` ★59K, `K-Dense-AI/claude-scientific-skills` ★58K, `wshobson/agents` ★42K).
   - Balanced across **12 specialized domains** (`10` skills each = **120 skills**):
     - `software-engineering`, `architecture`, `thinking-wisdom`, `principal-experience`, `design-ui-ux`, `languages`, `accounting-erp`, `auditing-security`, `mathematics`, `physics-simulation`, `economics-quant`, `product-features`.
   - Matched automatically per task via **Okapi BM25 + Bilingual Arabic/English Query Expansion + File-Extension Affinity + Role Boosting**, and injected cleanly into both Author and Reviewer prompts.
   - Full catalog: [`skills/elite/README.md`](skills/elite/README.md).

3. **Autonomous Queen Wave-DAG Planner (`queen.rs`)**
   - Scans repository layout and `CodeGraph` hubs, decomposes complex epics into **cycle-free, file-scope-disjoint topological waves** (verified via Kahn's algorithm), automatically serializes overlapping `owned_files` scopes to prevent merge conflicts, and executes waves in parallel.

4. **Persistent Hybrid Memory & Automatic Reflexion Bank (`memory.rs`)**
   - Combines **SQLite FTS5 BM25** + **128-D Signed Feature-Hashed Cosine Vectors** + **Navigable Small-World (`NSW`) associative graph** fused via **Reciprocal Rank Fusion (`RRF`)**.
   - Automatically synthesizes and stores **`Reflexion` lessons** whenever a failed attempt (`verify_failed`, `static_audit_failed`, `scope_violation`, `review_rejected`) is overcome, injecting those lessons into future tasks so mistakes never repeat.

5. **Multi-Language CodeGraph (`PageRank` + `Tarjan SCC`) & Pre-Merge Static Gate (`intelligence.rs`)**
   - Extracts symbols and cross-file dependencies across **Rust, TypeScript/JS, C#, Python, Go, and SQL**, computing **iterative PageRank centrality**, **circular dependency SCCs**, and **transitive reverse-dependency blast radius**.
   - **Pre-Merge Deterministic Static Gate**: Automatically blocks commits introducing hardcoded secrets, SQL-injection string interpolation, **IEEE-754 binary floats (`f64`/`float`/`DOUBLE PRECISION`) on monetary fields/columns**, or multi-tenant SQL tables missing `ENABLE ROW LEVEL SECURITY`.

6. **Independent & Quorum Multi-Provider Code Review + Adaptive UCB1 Routing (`router.rs`)**
   - Enforces non-author review (`Claude` cannot approve its own diff; `Codex` or `Antigravity` reviews it) and supports **Multi-Provider Quorum Review (`--quorum 2`)** requiring multiple distinct AI providers to approve critical changes.
   - Escalates model tier and reasoning effort (`low -> medium -> high -> xhigh -> max`) on work failures while learning provider/model win-rates via **UCB1**.

7. **Standards-Compliant 15-Tool MCP Server & Live SQLite Web Console (`mcp.rs`, `web.rs`)**
   - `ai-fleet engine mcp`: JSON-RPC 2.0 MCP server (`stdio`, both newline and `Content-Length` framing) exposing 15 live tools (`fleet_task_submit`, `fleet_queen_decompose`, `fleet_skills_match`, `fleet_memory_search`, `fleet_code_graph`, `fleet_blast_radius`, `fleet_static_audit`, etc.).
   - `ai-fleet engine console`: Live browser Mission Control dashboard connected directly to `engine.db`.

---

## ⚡ Quick Start

```bash
# Run the full workspace unit & end-to-end test suite
cargo test --workspace
cargo build --release --bin ai-fleet

# 1. Execute a task end-to-end (auto-matches Elite Skills, runs in isolated worktree, static-audits, verifies & merges)
ai-fleet engine do "Implement double-entry journal posting with exact minor-unit balances" \
  --owned "src/ledger/**" --verify "cargo test" --quorum 1

# 2. Decompose a multi-layer Epic with Queen into parallel DAG waves & execute
ai-fleet engine queen "Build multi-tenant invoicing API, PostgreSQL RLS migration, and dashboard UI" --execute

# 3. Browse & match the 120 Bundled Elite GitHub Skills (supports English & Arabic!)
ai-fleet skills list
ai-fleet skills list --domain accounting-erp
ai-fleet skills match "بناء نظام محاسبة قيد مزدوج وتدقيق RLS"
ai-fleet skills show party-ledger-reconciliation

# 4. Inspect CodeGraph PageRank hubs, Blast Radius & Pre-Merge Static Gate
ai-fleet graph --blast crates/fleet-engine/src/store.rs
ai-fleet audit

# 5. Query Persistent Hybrid FTS5 + 128-D Vector + Reflexion Memory
ai-fleet engine memory remember "Money Scale Invariant" "Always use NUMERIC(19,4) or i64 minor units; never float64"
ai-fleet engine memory search "invoice tax currency rounding"

# 6. Launch Live SQLite Web Console or Real MCP Server
ai-fleet engine console --bind 127.0.0.1:7878
ai-fleet engine mcp
```
