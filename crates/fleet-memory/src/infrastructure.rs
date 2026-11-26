//! Infrastructure layer for `fleet-memory`: `HnswPortAdapter` implementing
//! `fleet_kernel::MemoryBackendPort` and default vector memories.

use crate::application::{compute_embedding, insert_hnsw_node};
use crate::domain::{MultiLayerHnsw, VectorMemoryNode};
use fleet_kernel::{MemoryBackendPort, PortMemoryItem};
use std::sync::Mutex;

pub struct HnswPortAdapter {
    index: Mutex<MultiLayerHnsw>,
}

impl HnswPortAdapter {
    pub fn new() -> Self {
        HnswPortAdapter { index: Mutex::new(MultiLayerHnsw::new(6, 24)) }
    }
}

impl Default for HnswPortAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackendPort for HnswPortAdapter {
    fn remember(&self, kind: &str, title: &str, content: &str, tags: &[String]) -> Result<String, String> {
        let text = format!("[{kind}] {title}: {content} {}", tags.join(" "));
        let emb = compute_embedding(&text);
        let mut g = self.index.lock().unwrap_or_else(|e| e.into_inner());
        let id = g.insert(title, content, emb);
        Ok(id.to_string())
    }

    fn recall_hybrid(&self, query: &str, limit: usize) -> Result<Vec<PortMemoryItem>, String> {
        let q = compute_embedding(query);
        let g = self.index.lock().unwrap_or_else(|e| e.into_inner());
        let (hits, _) = g.search_knn(&q, limit, 32);
        Ok(hits
            .into_iter()
            .map(|(idx, sim)| {
                let p = &g.points[idx];
                PortMemoryItem {
                    id: p.id.to_string(),
                    kind: "hnsw".into(),
                    title: p.key.clone(),
                    content: p.payload.clone(),
                    score: sim as f64,
                }
            })
            .collect())
    }
}

pub fn default_vector_memories() -> Vec<VectorMemoryNode> {
    let mut nodes = Vec::new();
    insert_hnsw_node(
        &mut nodes,
        "estr-core",
        "constitution.money",
        "All money, quantity, unit price, and rate values use decimal in C# and numeric(19,4) in PostgreSQL with Half-Up line rounding. Never double or float.",
        vec!["money".into(), "decimal".into(), "BR-MNY".into()],
        0.99,
    );
    insert_hnsw_node(
        &mut nodes,
        "estr-core",
        "owner.br-wf04",
        "BR-WF04 applies across all modules: document creator never approves their own request, even system_administrator.",
        vec!["workflow".into(), "sod".into(), "BR-WF04".into()],
        0.99,
    );
    insert_hnsw_node(
        &mut nodes,
        "swarm-core",
        "policy.fenced-worktree",
        "Every writing agent works inside an isolated git worktree protected by a monotonic FencedLease epoch and reviewed by a non-author AI provider.",
        vec!["worktree".into(), "fenced-lease".into(), "review".into()],
        0.98,
    );
    insert_hnsw_node(
        &mut nodes,
        "swarm-core",
        "routing.moe-pheromone",
        "2-Layer 8-Expert MoE gating combined with ADR-330 pheromone trails and SONA/EWC Fisher weights selects optimal provider, model, and effort level.",
        vec!["moe".into(), "pheromone".into(), "sona".into(), "ewc".into()],
        0.97,
    );
    nodes
}
