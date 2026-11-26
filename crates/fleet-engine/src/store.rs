//! Durable state: tasks, runs, provider cooldowns and a hash-chained event log,
//! stored in SQLite (WAL mode) so the engine survives crashes and restarts.

use crate::error::{EngineError, Result};
use crate::model::*;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    depends_on TEXT NOT NULL DEFAULT '[]',
    owned_files TEXT NOT NULL DEFAULT '[]',
    complexity TEXT NOT NULL,
    verify_cmd TEXT,
    max_attempts INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    handoffs INTEGER NOT NULL DEFAULT 0,
    review_required INTEGER NOT NULL,
    review_quorum INTEGER NOT NULL DEFAULT 1,
    preferred_provider TEXT,
    skills TEXT NOT NULL DEFAULT '[]',
    auto_skills INTEGER NOT NULL DEFAULT 1,
    branch TEXT,
    worktree TEXT,
    base_commit TEXT,
    head_commit TEXT,
    authors TEXT NOT NULL DEFAULT '[]',
    reviewer TEXT,
    last_provider TEXT,
    last_model TEXT,
    last_effort TEXT,
    last_failure TEXT,
    last_error TEXT,
    result_summary TEXT,
    review_feedback TEXT,
    lease_epoch INTEGER NOT NULL DEFAULT 0,
    lease_expires_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    attempt INTEGER NOT NULL,
    role TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL,
    route_reason TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    duration_ms INTEGER,
    exit_code INTEGER,
    outcome TEXT,
    failure_kind TEXT,
    detail TEXT,
    session_id TEXT,
    log_path TEXT,
    commit_before TEXT,
    commit_after TEXT
);
CREATE INDEX IF NOT EXISTS idx_runs_task ON runs(task_id);

CREATE TABLE IF NOT EXISTS provider_state (
    id TEXT PRIMARY KEY,
    cooldown_until TEXT,
    status TEXT NOT NULL,
    last_error TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    task_id TEXT,
    kind TEXT NOT NULL,
    message TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    hash TEXT NOT NULL
);
"#;

const GENESIS: &str = "GENESIS";

/// Fixed-width UTC timestamp, so lexical order == chronological order in SQL.
pub fn ts(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn now_ts() -> String {
    ts(Utc::now())
}

pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

/// Aggregated historical performance of one (provider, model) pair.
#[derive(Clone, Debug, Default)]
pub struct Stat {
    pub successes: u32,
    pub failures: u32,
    pub avg_ms: f64,
}

#[derive(Clone, Debug)]
pub struct Cooldown {
    pub until: DateTime<Utc>,
    pub status: String,
    pub last_error: Option<String>,
}

pub struct Store {
    conn: Mutex<Connection>,
}

fn to_json(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn from_json(s: String) -> Vec<String> {
    serde_json::from_str(&s).unwrap_or_default()
}

fn row_to_task(r: &Row<'_>) -> rusqlite::Result<Task> {
    let status: String = r.get("status")?;
    let complexity: String = r.get("complexity")?;
    Ok(Task {
        id: r.get("id")?,
        title: r.get("title")?,
        prompt: r.get("prompt")?,
        status: TaskStatus::parse(&status).unwrap_or(TaskStatus::Failed),
        priority: r.get("priority")?,
        depends_on: from_json(r.get("depends_on")?),
        owned_files: from_json(r.get("owned_files")?),
        complexity: Complexity::parse(&complexity).unwrap_or(Complexity::Standard),
        verify_cmd: r.get("verify_cmd")?,
        max_attempts: r.get("max_attempts")?,
        attempts: r.get("attempts")?,
        handoffs: r.get("handoffs")?,
        review_required: r.get("review_required")?,
        review_quorum: r.get::<_, u32>("review_quorum").unwrap_or(1).max(1),
        preferred_provider: r.get("preferred_provider")?,
        skills: from_json(r.get("skills")?),
        auto_skills: r.get("auto_skills")?,
        branch: r.get("branch")?,
        worktree: r.get("worktree")?,
        base_commit: r.get("base_commit")?,
        head_commit: r.get("head_commit")?,
        authors: from_json(r.get("authors")?),
        reviewer: r.get("reviewer")?,
        last_provider: r.get("last_provider")?,
        last_model: r.get("last_model")?,
        last_effort: r.get("last_effort")?,
        last_failure: r.get("last_failure")?,
        last_error: r.get("last_error")?,
        result_summary: r.get("result_summary")?,
        review_feedback: r.get("review_feedback")?,
        lease_epoch: r.get("lease_epoch")?,
        lease_expires_at: r.get("lease_expires_at")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn row_to_run(r: &Row<'_>) -> rusqlite::Result<RunRecord> {
    Ok(RunRecord {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        attempt: r.get("attempt")?,
        role: r.get("role")?,
        provider: r.get("provider")?,
        model: r.get("model")?,
        effort: r.get("effort")?,
        route_reason: r.get("route_reason")?,
        started_at: r.get("started_at")?,
        ended_at: r.get("ended_at")?,
        duration_ms: r.get("duration_ms")?,
        exit_code: r.get("exit_code")?,
        outcome: r.get("outcome")?,
        failure_kind: r.get("failure_kind")?,
        detail: r.get("detail")?,
        session_id: r.get("session_id")?,
        log_path: r.get("log_path")?,
        commit_before: r.get("commit_before")?,
        commit_after: r.get("commit_after")?,
    })
}

fn event_hash(prev: &str, ts: &str, task: Option<&str>, kind: &str, msg: &str) -> String {
    let mut h = Sha256::new();
    for part in [prev, ts, task.unwrap_or(""), kind, msg] {
        h.update(part.as_bytes());
        h.update([0x1f]);
    }
    format!("{:x}", h.finalize())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN skills TEXT NOT NULL DEFAULT '[]'", []);
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN auto_skills INTEGER NOT NULL DEFAULT 1", []);
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN review_quorum INTEGER NOT NULL DEFAULT 1", []);
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave SQLite inconsistent
        // (statements are atomic), so recover from poisoning.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---------------------------------------------------------------- tasks

    pub fn insert_task(&self, id: &str, nt: &NewTask, complexity: Complexity, base_commit: &str) -> Result<Task> {
        let now = now_ts();
        {
            let conn = self.lock();
            conn.execute(
                "INSERT INTO tasks (id, title, prompt, status, priority, depends_on, owned_files, complexity,
                    verify_cmd, max_attempts, review_required, review_quorum, preferred_provider, skills, auto_skills, base_commit, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
                params![
                    id,
                    nt.title,
                    nt.prompt,
                    nt.priority,
                    to_json(&nt.depends_on),
                    to_json(&nt.owned_files),
                    complexity.as_str(),
                    nt.verify_cmd,
                    nt.max_attempts.max(1),
                    nt.review,
                    nt.review_quorum.max(1),
                    nt.preferred_provider,
                    to_json(&nt.skills),
                    nt.auto_skills,
                    base_commit,
                    now,
                ],
            )?;
        }
        self.get_task(id)?.ok_or_else(|| EngineError::NotFound(id.into()))
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let conn = self.lock();
        Ok(conn.query_row("SELECT * FROM tasks WHERE id = ?1", [id], row_to_task).optional()?)
    }

    /// All tasks, highest priority first, then oldest first.
    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM tasks ORDER BY priority DESC, created_at ASC, id ASC")?;
        let rows = stmt.query_map([], row_to_task)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn write_task(conn: &Connection, t: &Task, fence: Option<i64>) -> Result<bool> {
        let n = conn.execute(
            "UPDATE tasks SET title=?2, prompt=?3, status=?4, priority=?5, depends_on=?6, owned_files=?7,
                complexity=?8, verify_cmd=?9, max_attempts=?10, attempts=?11, handoffs=?12, review_required=?13,
                review_quorum=?14, preferred_provider=?15, skills=?16, auto_skills=?17, branch=?18, worktree=?19,
                base_commit=?20, head_commit=?21, authors=?22, reviewer=?23, last_provider=?24, last_model=?25,
                last_effort=?26, last_failure=?27, last_error=?28, result_summary=?29, review_feedback=?30,
                lease_epoch=?31, lease_expires_at=?32, updated_at=?33
             WHERE id=?1 AND (?34 IS NULL OR lease_epoch=?34)",
            params![
                t.id,
                t.title,
                t.prompt,
                t.status.as_str(),
                t.priority,
                to_json(&t.depends_on),
                to_json(&t.owned_files),
                t.complexity.as_str(),
                t.verify_cmd,
                t.max_attempts,
                t.attempts,
                t.handoffs,
                t.review_required,
                t.review_quorum.max(1),
                t.preferred_provider,
                to_json(&t.skills),
                t.auto_skills,
                t.branch,
                t.worktree,
                t.base_commit,
                t.head_commit,
                to_json(&t.authors),
                t.reviewer,
                t.last_provider,
                t.last_model,
                t.last_effort,
                t.last_failure,
                t.last_error,
                t.result_summary,
                t.review_feedback,
                t.lease_epoch,
                t.lease_expires_at,
                now_ts(),
                fence,
            ],
        )?;
        Ok(n == 1)
    }

    /// Persist a task only if nobody re-leased it since `epoch` (fencing token).
    /// Returns `false` when the write was rejected because the lease is stale.
    pub fn save_task_fenced(&self, t: &Task, epoch: i64) -> Result<bool> {
        let conn = self.lock();
        Self::write_task(&conn, t, Some(epoch))
    }

    /// Unconditional write for administrative operations (retry/cancel).
    pub fn save_task(&self, t: &Task) -> Result<()> {
        let conn = self.lock();
        Self::write_task(&conn, t, None)?;
        Ok(())
    }

    /// Atomically move a task from `from` to `to`, bumping its fencing epoch.
    pub fn claim(&self, id: &str, from: TaskStatus, to: TaskStatus, lease: Duration) -> Result<Option<i64>> {
        let conn = self.lock();
        let now = Utc::now();
        let n = conn.execute(
            "UPDATE tasks SET status=?1, lease_epoch=lease_epoch+1, lease_expires_at=?2, updated_at=?3
             WHERE id=?4 AND status=?5",
            params![to.as_str(), ts(now + lease), ts(now), id, from.as_str()],
        )?;
        if n == 0 {
            return Ok(None);
        }
        let epoch: i64 = conn.query_row("SELECT lease_epoch FROM tasks WHERE id=?1", [id], |r| r.get(0))?;
        Ok(Some(epoch))
    }

    /// Return tasks whose worker died (lease expired) to the queue. Their
    /// committed work stays on the task branch, so nothing is lost.
    pub fn recover_expired_leases(&self, now: DateTime<Utc>) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id FROM tasks WHERE status IN ('running','reviewing')
               AND (lease_expires_at IS NULL OR lease_expires_at < ?1)",
        )?;
        let ids: Vec<String> = stmt.query_map([ts(now)], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        for id in &ids {
            conn.execute(
                "UPDATE tasks SET status = CASE status WHEN 'running' THEN 'queued' ELSE 'awaiting_review' END,
                    lease_epoch = lease_epoch + 1, lease_expires_at = NULL, updated_at = ?2
                 WHERE id = ?1",
                params![id, ts(now)],
            )?;
        }
        Ok(ids)
    }

    // ----------------------------------------------------------------- runs

    #[allow(clippy::too_many_arguments)]
    pub fn insert_run(
        &self,
        task_id: &str,
        attempt: u32,
        role: &str,
        provider: &str,
        model: &str,
        effort: &str,
        route_reason: &str,
        log_path: &str,
        commit_before: &str,
    ) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO runs (task_id, attempt, role, provider, model, effort, route_reason, started_at, log_path, commit_before)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![task_id, attempt, role, provider, model, effort, route_reason, now_ts(), log_path, commit_before],
        )?;
        Ok(conn.last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_run(
        &self,
        run_id: i64,
        outcome: &RunOutcome,
        exit_code: Option<i32>,
        duration_ms: u64,
        session_id: Option<&str>,
        commit_after: &str,
    ) -> Result<()> {
        let (label, kind, detail) = match outcome {
            RunOutcome::Success => ("success", None, None),
            RunOutcome::Failure { kind, detail, .. } => ("failure", Some(kind.as_str()), Some(truncate(detail, 8000))),
        };
        let conn = self.lock();
        conn.execute(
            "UPDATE runs SET ended_at=?2, duration_ms=?3, exit_code=?4, outcome=?5, failure_kind=?6, detail=?7,
                session_id=?8, commit_after=?9 WHERE id=?1",
            params![run_id, now_ts(), duration_ms as i64, exit_code, label, kind, detail, session_id, commit_after],
        )?;
        Ok(())
    }

    pub fn runs_for_task(&self, task_id: &str) -> Result<Vec<RunRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM runs WHERE task_id=?1 ORDER BY id")?;
        let rows = stmt.query_map([task_id], row_to_run)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn recent_runs(&self, limit: usize) -> Result<Vec<RunRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT * FROM runs ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit as i64], row_to_run)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Success/failure counts per (provider, model), excluding outcomes that
    /// say nothing about quality (quota, auth, missing binary, cancellation).
    pub fn stats(&self) -> Result<HashMap<(String, String), Stat>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT provider, model,
                    SUM(CASE WHEN outcome='success' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN outcome='failure' THEN 1 ELSE 0 END),
                    COALESCE(AVG(duration_ms), 0)
             FROM runs
             WHERE ended_at IS NOT NULL
               AND (failure_kind IS NULL OR failure_kind NOT IN ('quota','auth','unavailable','cancelled','internal'))
             GROUP BY provider, model",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                Stat { successes: r.get::<_, i64>(2)? as u32, failures: r.get::<_, i64>(3)? as u32, avg_ms: r.get(4)? },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    // ------------------------------------------------------------ providers

    pub fn set_cooldown(&self, provider: &str, until: DateTime<Utc>, status: &str, err: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO provider_state (id, cooldown_until, status, last_error, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET cooldown_until=excluded.cooldown_until, status=excluded.status,
                last_error=excluded.last_error, updated_at=excluded.updated_at",
            params![provider, ts(until), status, truncate(err, 2000), now_ts()],
        )?;
        Ok(())
    }

    pub fn clear_cooldown(&self, provider: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE provider_state SET cooldown_until=NULL, status='ready', updated_at=?2 WHERE id=?1",
            params![provider, now_ts()],
        )?;
        Ok(())
    }

    pub fn active_cooldowns(&self, now: DateTime<Utc>) -> Result<HashMap<String, Cooldown>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, cooldown_until, status, last_error FROM provider_state
             WHERE cooldown_until IS NOT NULL AND cooldown_until > ?1",
        )?;
        let rows = stmt.query_map([ts(now)], |r| {
            let until: String = r.get(1)?;
            Ok((r.get::<_, String>(0)?, until, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, until, status, last_error) = row?;
            if let Some(until) = parse_ts(&until) {
                out.insert(id, Cooldown { until, status, last_error });
            }
        }
        Ok(out)
    }

    // --------------------------------------------------------------- events

    pub fn append_event(&self, task_id: Option<&str>, kind: &str, message: &str) -> Result<EventRecord> {
        let conn = self.lock();
        let prev: String = conn
            .query_row("SELECT hash FROM events ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .optional()?
            .unwrap_or_else(|| GENESIS.to_string());
        let now = now_ts();
        let hash = event_hash(&prev, &now, task_id, kind, message);
        conn.execute(
            "INSERT INTO events (ts, task_id, kind, message, prev_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now, task_id, kind, message, prev, hash],
        )?;
        Ok(EventRecord {
            id: conn.last_insert_rowid(),
            ts: now,
            task_id: task_id.map(str::to_string),
            kind: kind.to_string(),
            message: message.to_string(),
            hash,
        })
    }

    pub fn events(&self, task_id: Option<&str>, limit: usize) -> Result<Vec<EventRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts, task_id, kind, message, hash FROM events
             WHERE (?1 IS NULL OR task_id = ?1) ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![task_id, limit as i64], |r| {
            Ok(EventRecord {
                id: r.get(0)?,
                ts: r.get(1)?,
                task_id: r.get(2)?,
                kind: r.get(3)?,
                message: r.get(4)?,
                hash: r.get(5)?,
            })
        })?;
        let mut v = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        v.reverse();
        Ok(v)
    }

    /// Recompute the hash chain. `Ok(n)` = n intact events, `Err(id)` = first tampered row.
    pub fn verify_events(&self) -> Result<std::result::Result<usize, i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id, ts, task_id, kind, message, prev_hash, hash FROM events ORDER BY id")?;
        let mut rows = stmt.query([])?;
        let mut prev = GENESIS.to_string();
        let mut n = 0usize;
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            let ts: String = r.get(1)?;
            let task: Option<String> = r.get(2)?;
            let kind: String = r.get(3)?;
            let msg: String = r.get(4)?;
            let prev_hash: String = r.get(5)?;
            let hash: String = r.get(6)?;
            if prev_hash != prev || event_hash(&prev, &ts, task.as_deref(), &kind, &msg) != hash {
                return Ok(Err(id));
            }
            prev = hash;
            n += 1;
        }
        Ok(Ok(n))
    }

    #[doc(hidden)]
    pub fn raw_execute(&self, sql: &str) -> Result<usize> {
        Ok(self.lock().execute(sql, [])?)
    }
}

/// Truncate to at most `max` bytes on a char boundary, keeping the *end*
/// (the most recent output is usually the most informative).
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(store: &Store, id: &str) -> Task {
        store.insert_task(id, &NewTask::new("t", "p"), Complexity::Standard, "abc").unwrap()
    }

    #[test]
    fn claim_is_exclusive_and_fenced() {
        let s = Store::open_in_memory().unwrap();
        let t = sample(&s, "t1");
        let e1 = s.claim("t1", TaskStatus::Queued, TaskStatus::Running, Duration::minutes(5)).unwrap();
        assert_eq!(e1, Some(1));
        // Second claim fails: status is no longer queued.
        assert_eq!(s.claim("t1", TaskStatus::Queued, TaskStatus::Running, Duration::minutes(5)).unwrap(), None);

        // Lease expires -> recovery re-queues and bumps the epoch.
        let recovered = s.recover_expired_leases(Utc::now() + Duration::hours(1)).unwrap();
        assert_eq!(recovered, vec!["t1".to_string()]);
        let fresh = s.get_task("t1").unwrap().unwrap();
        assert_eq!(fresh.status, TaskStatus::Queued);
        assert_eq!(fresh.lease_epoch, 2);

        // The zombie worker holding epoch 1 can no longer write.
        let mut stale = t.clone();
        stale.status = TaskStatus::Done;
        stale.lease_epoch = 1;
        assert!(!s.save_task_fenced(&stale, 1).unwrap());
        assert_eq!(s.get_task("t1").unwrap().unwrap().status, TaskStatus::Queued);
    }

    #[test]
    fn event_chain_detects_tampering() {
        let s = Store::open_in_memory().unwrap();
        for i in 0..5 {
            s.append_event(Some("t"), "k", &format!("m{i}")).unwrap();
        }
        assert_eq!(s.verify_events().unwrap(), Ok(5));
        s.raw_execute("UPDATE events SET message='forged' WHERE id=3").unwrap();
        assert_eq!(s.verify_events().unwrap(), Err(3));
    }

    #[test]
    fn cooldowns_expire() {
        let s = Store::open_in_memory().unwrap();
        let now = Utc::now();
        s.set_cooldown("claude", now + Duration::minutes(10), "quota", "limit").unwrap();
        assert!(s.active_cooldowns(now).unwrap().contains_key("claude"));
        assert!(s.active_cooldowns(now + Duration::minutes(11)).unwrap().is_empty());
    }

    #[test]
    fn truncate_keeps_tail_on_char_boundary() {
        let s = "أهلاً بالعالم hello";
        let t = truncate(s, 7);
        assert!(t.ends_with("hello"));
    }
}
