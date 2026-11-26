//! Core domain types for the execution engine.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Reasoning effort requested from an AI tool. Ordered from cheapest to strongest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = [Effort::Low, Effort::Medium, Effort::High, Effort::Xhigh, Effort::Max];

    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Effort::Low),
            "medium" | "med" => Some(Effort::Medium),
            "high" => Some(Effort::High),
            "xhigh" | "x-high" | "extra-high" => Some(Effort::Xhigh),
            "max" | "maximum" => Some(Effort::Max),
            _ => None,
        }
    }

    /// Raise the effort by `steps` levels, saturating at `Max`.
    pub fn escalate(self, steps: u32) -> Self {
        let idx = (self as usize).saturating_add(steps as usize).min(Self::ALL.len() - 1);
        Self::ALL[idx]
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How hard a task is. Drives the initial model tier and effort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Trivial,
    Standard,
    Deep,
    Critical,
}

impl Complexity {
    pub fn as_str(self) -> &'static str {
        match self {
            Complexity::Trivial => "trivial",
            Complexity::Standard => "standard",
            Complexity::Deep => "deep",
            Complexity::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trivial" | "easy" | "low" => Some(Complexity::Trivial),
            "standard" | "normal" | "medium" => Some(Complexity::Standard),
            "deep" | "hard" | "high" => Some(Complexity::Deep),
            "critical" | "max" => Some(Complexity::Critical),
            _ => None,
        }
    }

    pub fn base_effort(self) -> Effort {
        match self {
            Complexity::Trivial => Effort::Low,
            Complexity::Standard => Effort::Medium,
            Complexity::Deep => Effort::High,
            Complexity::Critical => Effort::Xhigh,
        }
    }

    /// Minimum model tier (1 = fast/cheap, 3 = strongest).
    pub fn tier(self) -> u8 {
        match self {
            Complexity::Trivial => 1,
            Complexity::Standard => 2,
            Complexity::Deep | Complexity::Critical => 3,
        }
    }
}

/// Keyword heuristic used when the user does not label a task.
pub fn infer_complexity(prompt: &str) -> Complexity {
    let p = prompt.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| p.contains(w));
    if any(&[
        "security", "vulnerab", "migration", "authenticat", "authoriz", "payment", "money", "billing",
        "rls", "row level", "production", "data loss", "concurren", "encrypt", "secret",
    ]) {
        Complexity::Critical
    } else if prompt.len() > 1500
        || any(&[
            "architect", "refactor", "redesign", "design", "performance", "race", "distributed",
            "algorithm", "optimi", "deadlock", "memory leak",
        ])
    {
        Complexity::Deep
    } else if prompt.len() < 160 && any(&["typo", "rename", "comment", "format", "readme", "spelling", "lint"]) {
        Complexity::Trivial
    } else {
        Complexity::Standard
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    AwaitingReview,
    Reviewing,
    Done,
    Failed,
    Blocked,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::AwaitingReview => "awaiting_review",
            TaskStatus::Reviewing => "reviewing",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => TaskStatus::Queued,
            "running" => TaskStatus::Running,
            "awaiting_review" => TaskStatus::AwaitingReview,
            "reviewing" => TaskStatus::Reviewing,
            "done" => TaskStatus::Done,
            "failed" => TaskStatus::Failed,
            "blocked" => TaskStatus::Blocked,
            "cancelled" => TaskStatus::Cancelled,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Blocked | TaskStatus::Cancelled)
    }

    pub fn is_pending(self) -> bool {
        matches!(self, TaskStatus::Queued | TaskStatus::AwaitingReview)
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a run did not succeed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// Subscription quota / rate limit exhausted. Hand off to another provider.
    Quota,
    /// Tool is not logged in. Hand off and keep the provider parked.
    Auth,
    /// Binary missing or could not be started.
    Unavailable,
    /// Exceeded the wall-clock limit.
    Timeout,
    /// Produced no output for longer than the idle limit.
    Stalled,
    /// Non-zero exit without a recognised cause.
    Crash,
    /// The tool itself reported an error result.
    AgentError,
    /// The verification command failed.
    VerifyFailed,
    /// Files outside the task's owned scope were modified.
    ScopeViolation,
    /// Deterministic pre-merge security / money / RLS static gate failed.
    StaticAuditFailed,
    /// Independent reviewer requested changes.
    ReviewRejected,
    /// Branch could not be merged into the integration branch.
    MergeConflict,
    /// The engine was asked to stop.
    Cancelled,
    /// Engine-side failure (git, filesystem, ...).
    Internal,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FailureKind::Quota => "quota",
            FailureKind::Auth => "auth",
            FailureKind::Unavailable => "unavailable",
            FailureKind::Timeout => "timeout",
            FailureKind::Stalled => "stalled",
            FailureKind::Crash => "crash",
            FailureKind::AgentError => "agent_error",
            FailureKind::VerifyFailed => "verify_failed",
            FailureKind::ScopeViolation => "scope_violation",
            FailureKind::StaticAuditFailed => "static_audit_failed",
            FailureKind::ReviewRejected => "review_rejected",
            FailureKind::MergeConflict => "merge_conflict",
            FailureKind::Cancelled => "cancelled",
            FailureKind::Internal => "internal",
        }
    }

    /// Failures caused by the provider rather than the work: hand off without
    /// consuming one of the task's attempts and without escalating effort.
    pub fn is_provider_fault(self) -> bool {
        matches!(self, FailureKind::Quota | FailureKind::Auth | FailureKind::Unavailable)
    }
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RunOutcome {
    Success,
    Failure {
        kind: FailureKind,
        detail: String,
        retry_at: Option<DateTime<Utc>>,
    },
}

impl RunOutcome {
    pub fn failure(kind: FailureKind, detail: impl Into<String>) -> Self {
        RunOutcome::Failure { kind, detail: detail.into(), retry_at: None }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, RunOutcome::Success)
    }

    pub fn label(&self) -> &'static str {
        match self {
            RunOutcome::Success => "success",
            RunOutcome::Failure { kind, .. } => kind.as_str(),
        }
    }

    pub fn kind(&self) -> Option<FailureKind> {
        match self {
            RunOutcome::Success => None,
            RunOutcome::Failure { kind, .. } => Some(*kind),
        }
    }
}

/// A task as submitted by the user.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewTask {
    pub title: String,
    pub prompt: String,
    pub priority: i64,
    pub depends_on: Vec<String>,
    /// Glob patterns (relative to repo root) the task may modify. Empty = unrestricted.
    pub owned_files: Vec<String>,
    pub complexity: Option<Complexity>,
    /// Shell command that must exit 0 for the work to be accepted.
    pub verify_cmd: Option<String>,
    pub max_attempts: u32,
    /// Require approval from a provider that did not author the change.
    pub review: bool,
    /// Number of distinct non-author providers that must approve (`1` default, `>=2` quorum).
    #[serde(default = "default_quorum")]
    pub review_quorum: u32,
    pub preferred_provider: Option<String>,
    /// Explicit skill IDs to inject (`--skill <id>`).
    #[serde(default)]
    pub skills: Vec<String>,
    /// Automatically match and inject relevant domain skills (`true` by default).
    #[serde(default = "default_auto_skills")]
    pub auto_skills: bool,
}

fn default_auto_skills() -> bool {
    true
}

fn default_quorum() -> u32 {
    1
}

impl NewTask {
    pub fn new(title: impl Into<String>, prompt: impl Into<String>) -> Self {
        NewTask {
            title: title.into(),
            prompt: prompt.into(),
            priority: 0,
            depends_on: Vec::new(),
            owned_files: Vec::new(),
            complexity: None,
            verify_cmd: None,
            max_attempts: 4,
            review: true,
            review_quorum: 1,
            preferred_provider: None,
            skills: Vec::new(),
            auto_skills: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub status: TaskStatus,
    pub priority: i64,
    pub depends_on: Vec<String>,
    pub owned_files: Vec<String>,
    pub complexity: Complexity,
    pub verify_cmd: Option<String>,
    pub max_attempts: u32,
    /// Attempts consumed by work-related failures (drive escalation).
    pub attempts: u32,
    /// Provider-fault handoffs (quota/auth/unavailable) and reviewer retries.
    pub handoffs: u32,
    pub review_required: bool,
    #[serde(default = "default_quorum")]
    pub review_quorum: u32,
    pub preferred_provider: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default = "default_auto_skills")]
    pub auto_skills: bool,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub base_commit: Option<String>,
    pub head_commit: Option<String>,
    pub authors: Vec<String>,
    pub reviewer: Option<String>,
    pub last_provider: Option<String>,
    pub last_model: Option<String>,
    pub last_effort: Option<String>,
    #[serde(default)]
    pub last_session_id: Option<String>,
    pub last_failure: Option<String>,
    pub last_error: Option<String>,
    pub result_summary: Option<String>,
    pub review_feedback: Option<String>,
    pub lease_epoch: i64,
    pub lease_expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: i64,
    pub task_id: String,
    pub attempt: u32,
    pub role: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub route_reason: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub exit_code: Option<i64>,
    pub outcome: Option<String>,
    pub failure_kind: Option<String>,
    pub detail: Option<String>,
    pub session_id: Option<String>,
    pub log_path: Option<String>,
    pub commit_before: Option<String>,
    pub commit_after: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: i64,
    pub ts: String,
    pub task_id: Option<String>,
    pub kind: String,
    pub message: String,
    pub hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_escalation_saturates() {
        assert_eq!(Effort::Medium.escalate(1), Effort::High);
        assert_eq!(Effort::High.escalate(10), Effort::Max);
        assert_eq!(Effort::parse("XHIGH"), Some(Effort::Xhigh));
    }

    #[test]
    fn complexity_inference() {
        assert_eq!(infer_complexity("Fix the RLS policy on invoices"), Complexity::Critical);
        assert_eq!(infer_complexity("Refactor the router module"), Complexity::Deep);
        assert_eq!(infer_complexity("fix typo in README"), Complexity::Trivial);
        assert_eq!(infer_complexity("Add a CSV export button to the reports page"), Complexity::Standard);
    }
}
