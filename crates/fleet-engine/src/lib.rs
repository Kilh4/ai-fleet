//! `fleet-engine` — the real execution core of AI-Fleet.
//!
//! * [`store`]: durable SQLite state (tasks, runs, cooldowns, hash-chained events) with lease fencing.
//! * [`providers`]: data-driven adapters for Claude Code, Codex, Antigravity and any CLI; output
//!   parsing; quota/auth classification with reset-time extraction; billing-key stripping.
//! * [`proc`]: async supervision with streaming logs, wall/idle timeouts, cancellation and
//!   whole-process-tree termination (Windows Job Objects / Unix process groups).
//! * [`git`]: per-task worktrees, checkpoint commits (the unit of lossless handoff), scope checks,
//!   integration merges.
//! * [`router`]: complexity/attempt-driven tier and effort escalation + UCB1 learning from history.
//! * [`engine`]: the scheduler tying it together, with independent non-author review.

pub mod adapters;
pub mod cli;
pub mod engine;
pub mod error;
pub mod git;
pub mod intelligence;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod proc;
pub mod prompts;
pub mod providers;
pub mod queen;
pub mod router;
pub mod skills;
pub mod store;
pub mod tui;
pub mod web;

pub use engine::{Engine, EngineConfig, EngineEvent, ProviderReport, RunSummary};
pub use error::{EngineError, Result};
pub use intelligence::{AuditFinding, AuditSeverity, CodeGraph, StaticAuditReport};
pub use memory::{MemoryHit, MemoryKind, MemoryRecord, MemoryStore, NewMemory};
pub use model::*;
pub use providers::{ModelSpec, OutputFormat, PromptVia, ProviderSpec};
pub use queen::{EpicPlan, PlannedNode};
pub use skills::{SkillMatch, SkillRegistry, SkillSpec};
