//! # AI-Fleet Core Library (`ai_fleet`)
//!
//! Modular enterprise multi-AI orchestration engine implementing:
//! - **`domain`**: Core state, providers, models, tasks, fenced leases, and shared memory.
//! - **`router`**: 8-Expert Mixture-of-Experts (MoE) gating, Pheromone trails (ADR-330), Q-Learning, and ReasoningBank.
//! - **`security`**: CASA deterministic authority gate (ADR-380 §3), AIDefence prompt-injection shield (ADR-118), and ContinueGate.
//! - **`consensus`**: Byzantine Fault Tolerant (PBFT) weighted quorum and SHA-256 hash-chained decision ledger.
//! - **`executor`**: Parallel multi-AI process runner, subscription-only environment sanitizer, and zero-loss failover drill.
//! - **`tui`**: Rich ANSI/Unicode terminal dashboard (`bbs` and `orbital` views).

pub mod consensus;
pub mod domain;
pub mod executor;
pub mod router;
pub mod security;
pub mod tui;
