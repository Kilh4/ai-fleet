//! Distributed Work-Stealing & Fenced Lease-Heartbeat Broker (`@claude-flow/claims` parity & beyond).
//!
//! Tracks active worker claims, lease heartbeats, progress ticks, and allows
//! idle workers to steal starved or overloaded queued claims safely via
//! monotonic fencing epochs.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimSnapshot {
    pub task_id: String,
    pub worker_id: String,
    pub epoch: u64,
    pub progress_pct: u8,
    pub age_ms: u64,
    pub idle_since_heartbeat_ms: u64,
}

struct ActiveClaim {
    task_id: String,
    worker_id: String,
    epoch: u64,
    progress_pct: u8,
    claimed_at: Instant,
    last_heartbeat: Instant,
}

struct BrokerInner {
    epoch_seq: u64,
    lease_ttl: Duration,
    active: BTreeMap<String, ActiveClaim>,
    worker_queues: BTreeMap<String, VecDeque<String>>,
}

#[derive(Clone)]
pub struct WorkStealingBroker {
    inner: Arc<Mutex<BrokerInner>>,
}

impl WorkStealingBroker {
    pub fn new(lease_ttl: Duration) -> Self {
        WorkStealingBroker {
            inner: Arc::new(Mutex::new(BrokerInner {
                epoch_seq: 0,
                lease_ttl,
                active: BTreeMap::new(),
                worker_queues: BTreeMap::new(),
            })),
        }
    }

    /// Enqueue a task onto a worker's local deque.
    pub fn enqueue_local(&self, worker_id: &str, task_id: impl Into<String>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.worker_queues.entry(worker_id.to_string()).or_default().push_back(task_id.into());
    }

    /// Pop next task for `worker_id`: pops from its own deque first (LIFO for
    /// cache locality), or **steals from the tail (FIFO)** of the busiest peer!
    pub fn next_or_steal(&self, worker_id: &str) -> Option<(String, u64, Option<String>)> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut stolen_from: Option<String> = None;

        let task_id = if let Some(own) = g.worker_queues.get_mut(worker_id).and_then(|q| q.pop_front()) {
            Some(own)
        } else {
            // Find peer with largest backlog (`>= 1`) and steal oldest item from its front.
            let busiest = g
                .worker_queues
                .iter()
                .filter(|(k, q)| k.as_str() != worker_id && !q.is_empty())
                .max_by_key(|(_, q)| q.len())
                .map(|(k, _)| k.clone());
            if let Some(victim) = busiest {
                let item = g.worker_queues.get_mut(&victim).and_then(|q| q.pop_front());
                if item.is_some() {
                    stolen_from = Some(victim);
                }
                item
            } else {
                None
            }
        }?;

        g.epoch_seq += 1;
        let epoch = g.epoch_seq;
        let now = Instant::now();
        g.active.insert(
            task_id.clone(),
            ActiveClaim {
                task_id: task_id.clone(),
                worker_id: worker_id.to_string(),
                epoch,
                progress_pct: 0,
                claimed_at: now,
                last_heartbeat: now,
            },
        );
        Some((task_id, epoch, stolen_from))
    }

    /// Renew lease heartbeat; returns `false` if claim was already fenced/stolen.
    pub fn heartbeat(&self, task_id: &str, epoch: u64, progress_pct: u8) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(c) = g.active.get_mut(task_id) else { return false };
        if c.epoch != epoch {
            return false;
        }
        c.last_heartbeat = Instant::now();
        c.progress_pct = progress_pct.min(100);
        true
    }

    pub fn complete(&self, task_id: &str, epoch: u64) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.active.get(task_id).is_some_and(|c| c.epoch == epoch) {
            g.active.remove(task_id);
            true
        } else {
            false
        }
    }

    /// Reclaim active claims whose worker missed `lease_ttl` heartbeats,
    /// pushing them onto `target_worker` with a fresh epoch on next claim.
    pub fn reclaim_stalled(&self, target_worker: &str) -> Vec<(String, String)> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let ttl = g.lease_ttl;
        let stalled: Vec<(String, String)> = g
            .active
            .values()
            .filter(|c| c.last_heartbeat.elapsed() > ttl)
            .map(|c| (c.task_id.clone(), c.worker_id.clone()))
            .collect();
        for (tid, _) in &stalled {
            g.active.remove(tid);
            g.worker_queues.entry(target_worker.to_string()).or_default().push_front(tid.clone());
        }
        stalled
    }

    pub fn snapshots(&self) -> Vec<ClaimSnapshot> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.active
            .values()
            .map(|c| ClaimSnapshot {
                task_id: c.task_id.clone(),
                worker_id: c.worker_id.clone(),
                epoch: c.epoch,
                progress_pct: c.progress_pct,
                age_ms: c.claimed_at.elapsed().as_millis() as u64,
                idle_since_heartbeat_ms: c.last_heartbeat.elapsed().as_millis() as u64,
            })
            .collect()
    }
}
