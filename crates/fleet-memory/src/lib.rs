//! `fleet-memory` — 3-Layer Hexagonal Multi-Layer HNSW Vector Index Crate:
//! - `domain`: `MultiLayerHnsw` skip-list points, `VectorMemoryNode`, `SemanticSearchHit`.
//! - `application`: Malkov & Yashunin (2018) hierarchical skip-layer HNSW search & insertion.
//! - `infrastructure`: `HnswPortAdapter` implementing `fleet_kernel::MemoryBackendPort`.

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{compute_embedding, cosine_similarity, insert_hnsw_node, search_hnsw_semantic};
pub use domain::{HnswPoint, MultiLayerHnsw, SemanticSearchHit, VectorMemoryNode, EMBED_DIM};
pub use infrastructure::{default_vector_memories, HnswPortAdapter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_layer_hnsw_finds_exact_target_in_sublinear_hops() {
        let mut hnsw = MultiLayerHnsw::new(6, 28);
        for i in 0..200 {
            let txt = format!("synthetic engineering module {i} handling cache invalidation #{i}");
            hnsw.insert(format!("mod-{i}"), txt.clone(), compute_embedding(&txt));
        }
        let target_txt = "PostgreSQL double-entry ledger reconciliation with ZATCA secp256k1 signature";
        let target_id = hnsw.insert("zatca-ledger", target_txt, compute_embedding(target_txt));

        let (hits, hops) = hnsw.search_knn(&compute_embedding("ZATCA secp256k1 double-entry ledger"), 3, 24);
        assert_eq!(hits[0].0, target_id);
        assert!(hits[0].1 > 0.65);
        assert!(hops < 160, "HNSW graph must visit fewer nodes than brute-force 201 scan, took {hops} hops");
    }
}
