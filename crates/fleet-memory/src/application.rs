//! Application layer for `fleet-memory`:
//! - Real **Multi-Layer Hierarchical Navigable Small World (`MultiLayerHnsw`,
//!   Malkov & Yashunin 2018)**: exponential level sampling, top-down greedy
//!   entry descent, beam search (`ef`), and bounded bidirectional link pruning.
//! - Deterministic 64-dim unit-norm word + trigram embeddings.

use crate::domain::{HnswPoint, MultiLayerHnsw, SemanticSearchHit, VectorMemoryNode, EMBED_DIM};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

#[derive(Clone, Copy, Debug)]
struct ScoredIdx {
    sim: f32,
    idx: usize,
}

impl PartialEq for ScoredIdx {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.sim.to_bits() == other.sim.to_bits()
    }
}
impl Eq for ScoredIdx {}
impl PartialOrd for ScoredIdx {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ScoredIdx {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sim.partial_cmp(&other.sim).unwrap_or(Ordering::Equal).then_with(|| other.idx.cmp(&self.idx))
    }
}

impl MultiLayerHnsw {
    pub fn new(m: usize, ef_construction: usize) -> Self {
        let m = m.max(2);
        MultiLayerHnsw {
            m,
            ef_construction: ef_construction.max(m * 2),
            entry_point: None,
            max_layer: 0,
            points: Vec::new(),
        }
    }

    fn sample_level(&self, key: &str, idx: usize) -> usize {
        let mut h = Sha256::new();
        h.update(key.as_bytes());
        h.update(idx.to_le_bytes());
        let d = h.finalize();
        let u_raw = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        let u = ((u_raw as f64) + 1.0) / ((u32::MAX as f64) + 2.0);
        let ml = 1.0 / (self.m as f64).ln();
        ((-u.ln() * ml).floor() as usize).min(8)
    }

    /// Greedy 1-NN walk within a single upper layer `layer`.
    fn greedy_step(&self, mut cur: usize, query: &[f32], layer: usize, hops: &mut usize) -> usize {
        let mut cur_sim = cosine_similarity(query, &self.points[cur].vector);
        loop {
            let mut improved = false;
            if let Some(nbrs) = self.points[cur].layers.get(layer) {
                for &nb in nbrs {
                    *hops += 1;
                    let s = cosine_similarity(query, &self.points[nb].vector);
                    if s > cur_sim + 1e-6 {
                        cur = nb;
                        cur_sim = s;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
        cur
    }

    /// Beam search (`ef`) at `layer` starting from `entry`, returning best candidates sorted descending by similarity.
    fn search_layer(&self, entry: usize, query: &[f32], ef: usize, layer: usize, hops: &mut usize) -> Vec<ScoredIdx> {
        let mut visited: HashSet<usize> = HashSet::new();
        let entry_sim = cosine_similarity(query, &self.points[entry].vector);
        visited.insert(entry);

        let mut candidates: BinaryHeap<ScoredIdx> = BinaryHeap::new();
        candidates.push(ScoredIdx { sim: entry_sim, idx: entry });
        let mut results: Vec<ScoredIdx> = vec![ScoredIdx { sim: entry_sim, idx: entry }];

        while let Some(curr) = candidates.pop() {
            let worst_best = results.iter().map(|r| r.sim).fold(f32::INFINITY, f32::min);
            if results.len() >= ef && curr.sim < worst_best {
                break;
            }
            if let Some(nbrs) = self.points[curr.idx].layers.get(layer) {
                for &nb in nbrs {
                    if !visited.insert(nb) {
                        continue;
                    }
                    *hops += 1;
                    let sim = cosine_similarity(query, &self.points[nb].vector);
                    if results.len() < ef || sim > worst_best {
                        candidates.push(ScoredIdx { sim, idx: nb });
                        results.push(ScoredIdx { sim, idx: nb });
                        if results.len() > ef {
                            results.sort_by(|a, b| b.cmp(a));
                            results.truncate(ef);
                        }
                    }
                }
            }
        }
        results.sort_by(|a, b| b.cmp(a));
        results
    }

    pub fn insert(&mut self, key: impl Into<String>, payload: impl Into<String>, vector: Vec<f32>) -> usize {
        let key = key.into();
        let payload = payload.into();
        let new_idx = self.points.len();
        let level = self.sample_level(&key, new_idx);
        let point = HnswPoint {
            id: new_idx,
            key,
            payload,
            vector: vector.clone(),
            max_layer: level,
            layers: vec![Vec::new(); level + 1],
        };
        self.points.push(point);

        let Some(mut ep) = self.entry_point else {
            self.entry_point = Some(new_idx);
            self.max_layer = level;
            return new_idx;
        };

        let mut hops = 0usize;
        // 1. Greedy descent from top layer down to `level + 1`
        if self.max_layer > level {
            for l in ((level + 1)..=self.max_layer).rev() {
                ep = self.greedy_step(ep, &vector, l, &mut hops);
            }
        }

        // 2. Beam search and link wiring at layers `min(level, max_layer) ..= 0`
        let top_connect = level.min(self.max_layer);
        for l in (0..=top_connect).rev() {
            let m_max = if l == 0 { self.m * 2 } else { self.m };
            let cands = self.search_layer(ep, &vector, self.ef_construction, l, &mut hops);
            if let Some(first) = cands.first() {
                ep = first.idx;
            }
            let chosen: Vec<usize> = cands
                .into_iter()
                .filter(|c| c.idx != new_idx)
                .take(self.m)
                .map(|c| c.idx)
                .collect();
            self.points[new_idx].layers[l] = chosen.clone();

            for nb in chosen {
                if self.points[nb].layers.len() <= l {
                    self.points[nb].layers.resize(l + 1, Vec::new());
                }
                if !self.points[nb].layers[l].contains(&new_idx) {
                    self.points[nb].layers[l].push(new_idx);
                    if self.points[nb].layers[l].len() > m_max {
                        let nb_vec = self.points[nb].vector.clone();
                        let mut scored: Vec<(f32, usize)> = self.points[nb].layers[l]
                            .iter()
                            .map(|&other| (cosine_similarity(&nb_vec, &self.points[other].vector), other))
                            .collect();
                        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
                        self.points[nb].layers[l] = scored.into_iter().take(m_max).map(|(_, idx)| idx).collect();
                    }
                }
            }
        }

        if level > self.max_layer {
            self.entry_point = Some(new_idx);
            self.max_layer = level;
        }
        new_idx
    }

    /// Approximate $k$-NN search returning `(point_idx, cosine_similarity, graph_hops)`.
    pub fn search_knn(&self, query_vec: &[f32], k: usize, ef_search: usize) -> (Vec<(usize, f32)>, usize) {
        let Some(mut ep) = self.entry_point else {
            return (Vec::new(), 0);
        };
        let mut hops = 1usize;
        for l in (1..=self.max_layer).rev() {
            ep = self.greedy_step(ep, query_vec, l, &mut hops);
        }
        let cands = self.search_layer(ep, query_vec, ef_search.max(k), 0, &mut hops);
        let top = cands.into_iter().take(k).map(|c| (c.idx, c.sim)).collect();
        (top, hops)
    }
}

pub fn compute_embedding(text: &str) -> Vec<f32> {
    let mut vec = vec![0.0_f32; EMBED_DIM];
    let normalized = text.to_lowercase();
    let words: Vec<&str> = normalized.split_whitespace().collect();

    for (w_idx, word) in words.iter().enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(word.as_bytes());
        let digest = hasher.finalize();
        for i in 0..EMBED_DIM {
            let byte_val = digest[i % 32] as f32;
            let sign = if (digest[(i + 7) % 32] & 1) == 0 { 1.0 } else { -1.0 };
            let positional_weight = 1.0 / (1.0 + (w_idx as f32) * 0.05);
            vec[i] += sign * (byte_val / 255.0) * positional_weight;
        }

        let chars: Vec<char> = word.chars().collect();
        for win in chars.windows(3) {
            let tri: String = win.iter().collect();
            let mut th = Sha256::new();
            th.update(tri.as_bytes());
            let td = th.finalize();
            let slot = (td[0] as usize) % EMBED_DIM;
            let sign = if (td[1] & 1) == 0 { 1.0 } else { -1.0 };
            vec[slot] += sign * 0.35;
        }
    }

    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    vec.into_iter().map(|x| x / norm).collect()
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>().clamp(-1.0, 1.0)
}

pub fn insert_hnsw_node(nodes: &mut Vec<VectorMemoryNode>, namespace: &str, key: &str, content: &str, tags: Vec<String>, importance: f32) {
    let combined_text = format!("{} {} {} {}", namespace, key, content, tags.join(" "));
    let emb = compute_embedding(&combined_text);

    let mut scored: Vec<(f32, String)> = nodes
        .iter()
        .filter(|n| n.key != key)
        .map(|n| (cosine_similarity(&emb, &n.embedding), n.key.clone()))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
    let neighbors: Vec<String> = scored.into_iter().take(3).map(|(_, k)| k).collect();

    for nb_key in &neighbors {
        if let Some(target) = nodes.iter_mut().find(|n| &n.key == nb_key) {
            if !target.neighbors.contains(&key.to_string()) && target.neighbors.len() < 6 {
                target.neighbors.push(key.to_string());
            }
        }
    }

    if let Some(existing) = nodes.iter_mut().find(|n| n.key == key) {
        existing.namespace = namespace.to_string();
        existing.content = content.to_string();
        existing.tags = tags;
        existing.embedding = emb;
        existing.neighbors = neighbors;
        existing.importance = importance;
        existing.updated_at = Utc::now().to_rfc3339();
    } else {
        nodes.push(VectorMemoryNode {
            id: format!("mem-{}", nodes.len() + 1),
            namespace: namespace.to_string(),
            key: key.to_string(),
            content: content.to_string(),
            tags,
            embedding: emb,
            neighbors,
            access_count: 0,
            importance,
            updated_at: Utc::now().to_rfc3339(),
        });
    }
}

pub fn search_hnsw_semantic(nodes: &[VectorMemoryNode], query: &str, top_k: usize) -> Vec<SemanticSearchHit> {
    if nodes.is_empty() {
        return Vec::new();
    }
    let q_emb = compute_embedding(query);
    let q_low = query.to_lowercase();
    let q_words: Vec<&str> = q_low.split_whitespace().collect();

    let mut hits: Vec<SemanticSearchHit> = nodes
        .iter()
        .map(|n| {
            let cos = cosine_similarity(&q_emb, &n.embedding);
            let hay = format!("{} {} {} {}", n.namespace, n.key, n.content, n.tags.join(" ")).to_lowercase();
            let lexical_hits = q_words.iter().filter(|&&w| hay.contains(w)).count() as f32;
            let lexical_bonus = (lexical_hits / (q_words.len().max(1) as f32)) * 0.35;
            let hybrid = (cos * 0.65) + lexical_bonus + (n.importance * 0.05);
            SemanticSearchHit {
                key: n.key.clone(),
                namespace: n.namespace.clone(),
                content: n.content.clone(),
                cosine_similarity: cos,
                hybrid_score: hybrid,
                neighbors_traversed: n.neighbors.len() + 1,
            }
        })
        .collect();

    hits.sort_by(|a, b| b.hybrid_score.partial_cmp(&a.hybrid_score).unwrap_or(Ordering::Equal));
    hits.truncate(top_k.max(1));
    hits
}
