//! Hexagonal Adapters implementing `fleet_kernel` Port Traits (`TaskStorePort`,
//! `MemoryBackendPort`, `PolicyGatePort`) for SQLite `Store`, `MemoryStore`, and
//! `StaticAuditor`.

use crate::intelligence::audit_file_list;
use crate::memory::{MemoryKind, MemoryStore, NewMemory};
use crate::model::TaskStatus;
use crate::store::Store;
use chrono::Duration;
use fleet_kernel::{
    GateCheckResult, MemoryBackendPort, PolicyGatePort, PortMemoryItem, PortTaskSnapshot,
    TaskStorePort,
};
use std::collections::BTreeMap;
use std::path::Path;

impl TaskStorePort for Store {
    fn snapshot_tasks(&self) -> std::result::Result<Vec<PortTaskSnapshot>, String> {
        let tasks = self.list_tasks().map_err(|e| e.to_string())?;
        Ok(tasks
            .into_iter()
            .map(|t| PortTaskSnapshot {
                id: t.id,
                title: t.title,
                status: t.status.as_str().to_string(),
                priority: t.priority,
                attempts: t.attempts,
                handoffs: t.handoffs,
                lease_epoch: t.lease_epoch,
            })
            .collect())
    }

    fn try_acquire_fenced_lease(
        &self,
        task_id: &str,
        expected_status: &str,
        ttl_secs: u64,
    ) -> std::result::Result<Option<i64>, String> {
        let from = TaskStatus::parse(expected_status).ok_or_else(|| format!("invalid status `{expected_status}`"))?;
        let to = if from == TaskStatus::AwaitingReview { TaskStatus::Reviewing } else { TaskStatus::Running };
        self.claim(task_id, from, to, Duration::seconds(ttl_secs as i64)).map_err(|e| e.to_string())
    }
}

impl MemoryBackendPort for MemoryStore {
    fn remember(&self, kind: &str, title: &str, content: &str, tags: &[String]) -> std::result::Result<String, String> {
        let id = self
            .store(NewMemory {
                kind: MemoryKind::parse(kind),
                title: title.to_string(),
                content: content.to_string(),
                files: Vec::new(),
                tags: tags.to_vec(),
                task_id: None,
                importance: 1.5,
            })
            .map_err(|e| e.to_string())?;
        Ok(id.to_string())
    }

    fn recall_hybrid(&self, query: &str, limit: usize) -> std::result::Result<Vec<PortMemoryItem>, String> {
        let hits = self.search(query, &[], limit).map_err(|e| e.to_string())?;
        Ok(hits
            .into_iter()
            .map(|h| PortMemoryItem {
                id: h.record.id.to_string(),
                kind: h.record.kind.as_str().to_string(),
                title: h.record.title,
                content: h.record.content,
                score: h.score,
            })
            .collect())
    }
}

pub struct StaticSecurityMoneyGateAdapter;

impl PolicyGatePort for StaticSecurityMoneyGateAdapter {
    fn gate_name(&self) -> &str {
        "static-security-money-rls-gate"
    }

    fn evaluate_diff(&self, workspace_path: &str, changed_files: &[String]) -> std::result::Result<GateCheckResult, String> {
        let rep = audit_file_list(Path::new(workspace_path), changed_files).map_err(|e| e.to_string())?;
        let violations: Vec<String> = rep
            .findings
            .iter()
            .map(|f| format!("{}:{} [{}] {}", f.file, f.line, f.rule, f.message))
            .collect();
        let mut meta = BTreeMap::new();
        meta.insert("checked_files".into(), changed_files.len().to_string());
        Ok(GateCheckResult { passed: rep.passed, rule_violations: violations, metadata: meta })
    }
}
