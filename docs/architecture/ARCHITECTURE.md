# 🏛 AI-Fleet v7.0 — Hexagonal DDD & Microkernel Architecture Reference

AI-Fleet v7.0 is engineered as a **20-Crate Native Rust Monorepo** governed by **Hexagonal Ports-and-Adapters (`SOLID` Dependency Inversion)**, **Bounded-Context 3-Layer Domain-Driven Design (`domain / application / infrastructure`)**, an **Async Event-Sourced Microkernel**, and **Compile/Test-Time Architectural Fitness Functions** (`bins/ai-fleet/tests/architecture_tests.rs`).

---

## 1. Layered Workspace Topology (C4 Container & Dependency Flow)

All dependencies point **strictly inward** toward `fleet-kernel` and `fleet-core`. Automated tests verify zero crate cycles and forbid inner layers from importing outer adapters.

```mermaid
flowchart TD
    subgraph UI_Transport["Presentation & Protocol Adapters (Outer Ring)"]
        CLI["bins/ai-fleet\nUnified CLI"]
        TUI["crates/fleet-tui\n8-View Terminal Dashboard"]
        WEB["crates/fleet-engine::web & fleet-web\nLive Mission Control HTTP/REST"]
        MCP["crates/fleet-engine::mcp & fleet-mcp\nJSON-RPC 2.0 MCP Server (stdio / Content-Length)"]
    end

    subgraph Orchestration["Application Orchestration Ring"]
        ENG["crates/fleet-engine\nAsync Tokio Scheduler, Queen Wave-DAG,\nSQLite WAL Adapters, Worktrees, CodeGraph & 120 Skills"]
    end

    subgraph BoundedContexts["3-Layer DDD Bounded Contexts (domain / application / infrastructure)"]
        PLG["crates/fleet-plugins\nCapability-Sandboxed Plugin SDK & Host"]
        SEC["crates/fleet-security\nCASA Envelopes, RFC-2104 HMAC Tokens & Deobfuscating AIDefence"]
        SWM["crates/fleet-swarm\n3f+1 PBFT + View-Change, Raft Log Safety & Vector-Clock CRDT"]
        NRL["crates/fleet-neural\nSherman-Morrison LinUCB Bandit, 2-Layer MoE & EWC Fisher"]
        WFL["crates/fleet-workflows\nLIFO Saga Coordinator, Guarded SPARC & TDD FSMs"]
        FED["crates/fleet-federation\nRFC-6962 Binary Merkle Proofs, PII Scrubber & A2A"]
        WMK["crates/ruflo-watermark\nSynthID Tournament, Gumbel & Bayesian Detector"]
    end

    subgraph Kernel["Pure Hexagonal Microkernel (Innermost Ring — Zero Internal Dependencies)"]
        KRN["crates/fleet-kernel\nPort Traits, Hash-Chained DomainEventBus,\nCircuitBreaker, Bulkhead, WorkStealingBroker & PluginInterceptor"]
        COR["crates/fleet-core\nShared Value Objects & SHA-256 Decision Ledger"]
    end

    UI_Transport --> ENG
    ENG --> BoundedContexts
    ENG --> KRN
    BoundedContexts --> KRN
    BoundedContexts --> COR
```

---

## 2. Hexagonal Ports & Microkernel Execution Sequence

Every task dispatched through `fleet-engine` traverses capability-sandboxed `PluginInterceptor` hooks, `Bulkhead` concurrency permits, `CircuitBreaker` state machines, isolated Git worktrees, deterministic static gates, and independent/quorum non-author review:

```mermaid
sequenceDiagram
    participant U as User / MCP / Queen DAG
    participant E as Engine (Scheduler)
    participant P as PluginHost (Priority 0..90)
    participant K as Kernel (Bulkhead + CircuitBreaker + EventBus)
    participant A as AI CLI (Job Object + Worktree)
    participant G as Static Security/Money/RLS Gate
    participant M as Hybrid FTS5 + 128D Vector Memory
    participant R as Independent / Quorum Reviewers

    U->>E: Submit Task or Queen Multi-Wave Epic
    E->>M: Hybrid RRF Recall (FTS5 BM25 + 128D Cosine + 1-hop NSW)
    E->>E: CodeGraph Scan (PageRank Hubs + Blast Radius) + Okapi BM25 Skill Match (120 Elite Skills)
    E->>P: run_phase(BeforeExecute) [AIDefence Shield + SQL/Financial Precision Enricher]
    alt Blocked by Security / Policy Plugin
        P-->>E: InterceptOutcome::Reject
        E->>K: Publish PluginIntercepted(reject)
    else Allowed / Enriched
        P-->>E: InterceptOutcome::Enrich
        E->>K: Acquire Bulkhead Permit + Check CircuitBreaker
        E->>A: Spawn CLI (env stripped of *_API_KEY) inside Windows Job Object
        A-->>E: Stream JSON / Checkpoint Commit (SHA-256)
        alt Subscription Quota Exhausted
            E->>K: Park Provider Until Parsed Reset Time
            E->>E: Hand off Branch & Prior Commits to Next Provider (0 Lost Bytes)
        else Exit 0
            E->>G: Audit Diff (Secrets, SQLi, IEEE-754 Float Money, Missing Tenant RLS)
            alt Gate or Verify Cmd Failed
                E->>E: Escalate Effort (low->medium->high->xhigh->max) & Retry
            else Passed Verification & Static Gate
                E->>M: Synthesize Reflexion Lesson (if overcoming prior failure)
                E->>R: Route to k Distinct Non-Author AI Providers (Quorum Review)
                R-->>E: VERDICT: APPROVE (Quorum Reached)
                E->>E: Serialized Merge into ai-fleet/integration + Post-Merge Re-Verify
            end
        end
    end
```

---

## 3. Core Mathematical & Distributed-Systems Foundations

| Subsystem | Crate & Module | Algorithm / Guarantee |
| :--- | :--- | :--- |
| **Microkernel & Resilience** | `fleet-kernel::{resilience, claims, events}` | 3-state `CircuitBreaker` (`Closed/Open/HalfOpen`), `Semaphore` `Bulkhead`, SplitMix64 full-jitter backoff, FIFO-steal `WorkStealingBroker`, SHA-256 `DomainEventBus` |
| **Contextual Bandit Routing** | `fleet-neural::application::LinUcbRouter` | Disjoint LinUCB ($\hat{\theta}_a^\top x + \alpha\sqrt{x^\top A_a^{-1}x}$) with exact $O(d^2)$ Sherman-Morrison rank-1 inverse updates |
| **Byzantine & Crash Consensus** | `fleet-swarm::application` | $3f+1$ PBFT with equivocation detection & deterministic view-change; Raft §5.4.1 up-to-date log election & §5.4.2 majority commit; Causal Vector-Clock LWW CRDT |
| **Transactional Workflows** | `fleet-workflows::application` | LIFO Saga Compensation state machine, Guarded 5-stage SPARC FSM, strict Red-Green-Refactor TDD FSM |
| **Cryptographic Federation** | `fleet-federation::application` | RFC-6962 domain-separated (`0x00`/`0x01`) SHA-256 Binary Merkle Tree inclusion proofs + SynthID Gumbel/Tournament watermarking |
| **Hybrid Memory & Skills** | `fleet-engine::{memory, skills, intelligence}` | SQLite FTS5 + 128-D CountSketch cosine vectors + 1-hop NSW graph via RRF; Okapi BM25 over 120 embedded GitHub skills; iterative PageRank ($d=0.85$) + Tarjan SCC |
