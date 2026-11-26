//! Typed Domain Event Bus with correlation IDs, monotonic sequence numbers,
//! bounded replay history, and async broadcast fan-out.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEventPayload {
    TaskQueued { title: String, complexity: String },
    LeaseClaimed { worker_id: String, epoch: i64 },
    WorkStealingRebalanced { task_id: String, from_worker: String, to_worker: String, reason: String },
    PluginIntercepted { plugin_id: String, phase: String, verdict: String },
    CircuitStateChanged { resource: String, from_state: String, to_state: String },
    RunDispatched { provider: String, model: String, effort: String, role: String },
    CheckpointSealed { commit: String, files_changed: usize },
    PolicyGateEvaluated { gate: String, passed: bool, findings: usize },
    ConsensusReached { protocol: String, quorum_ok: bool, approval_ratio_bps: u32 },
    ReflexionSynthesized { failure_kind: String, memory_title: String },
    TaskCompleted { branch: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelEvent {
    pub seq: u64,
    pub timestamp: String,
    pub correlation_id: String,
    pub task_id: Option<String>,
    pub payload: DomainEventPayload,
    pub prev_digest: String,
    pub digest: String,
}

struct ReplayRing {
    capacity: usize,
    last_digest: String,
    items: VecDeque<KernelEvent>,
}

#[derive(Clone)]
pub struct DomainEventBus {
    seq: Arc<AtomicU64>,
    ring: Arc<Mutex<ReplayRing>>,
    tx: broadcast::Sender<KernelEvent>,
}

impl Default for DomainEventBus {
    fn default() -> Self {
        Self::new(2048)
    }
}

impl DomainEventBus {
    pub fn new(replay_capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(replay_capacity.max(64));
        DomainEventBus {
            seq: Arc::new(AtomicU64::new(0)),
            ring: Arc::new(Mutex::new(ReplayRing {
                capacity: replay_capacity.max(16),
                last_digest: "KERNEL_GENESIS".into(),
                items: VecDeque::with_capacity(replay_capacity.min(512)),
            })),
            tx,
        }
    }

    pub fn publish(&self, correlation_id: impl Into<String>, task_id: Option<String>, payload: DomainEventPayload) -> KernelEvent {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let timestamp = Utc::now().to_rfc3339();
        let correlation_id = correlation_id.into();
        let payload_json = serde_json::to_string(&payload).unwrap_or_default();

        let mut guard = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        let prev_digest = guard.last_digest.clone();
        let mut hasher = Sha256::new();
        hasher.update(seq.to_le_bytes());
        hasher.update(prev_digest.as_bytes());
        hasher.update(timestamp.as_bytes());
        hasher.update(correlation_id.as_bytes());
        hasher.update(payload_json.as_bytes());
        let digest = format!("{:x}", hasher.finalize());
        guard.last_digest = digest.clone();

        let ev = KernelEvent {
            seq,
            timestamp,
            correlation_id,
            task_id,
            payload,
            prev_digest,
            digest,
        };
        if guard.items.len() >= guard.capacity {
            guard.items.pop_front();
        }
        guard.items.push_back(ev.clone());
        drop(guard);

        let _ = self.tx.send(ev.clone());
        ev
    }

    pub fn subscribe(&self) -> broadcast::Receiver<KernelEvent> {
        self.tx.subscribe()
    }

    pub fn replay_since(&self, min_seq: u64) -> Vec<KernelEvent> {
        let guard = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        guard.items.iter().filter(|e| e.seq >= min_seq).cloned().collect()
    }

    pub fn verify_ring_integrity(&self) -> Result<usize, u64> {
        let guard = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        for win in guard.items.as_slices().0.windows(2) {
            if win[1].prev_digest != win[0].digest {
                return Err(win[1].seq);
            }
        }
        for ev in &guard.items {
            let payload_json = serde_json::to_string(&ev.payload).unwrap_or_default();
            let mut hasher = Sha256::new();
            hasher.update(ev.seq.to_le_bytes());
            hasher.update(ev.prev_digest.as_bytes());
            hasher.update(ev.timestamp.as_bytes());
            hasher.update(ev.correlation_id.as_bytes());
            hasher.update(payload_json.as_bytes());
            if format!("{:x}", hasher.finalize()) != ev.digest {
                return Err(ev.seq);
            }
        }
        Ok(guard.items.len())
    }
}
