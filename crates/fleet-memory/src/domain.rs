//! Domain layer for `fleet-memory`: Multi-layer HNSW graph points, vector memory
//! nodes, and semantic search results.

use serde::{Deserialize, Serialize};

pub const EMBED_DIM: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMemoryNode {
    pub id: String,
    pub namespace: String,
    pub key: String,
    pub content: String,
    pub tags: Vec<String>,
    pub embedding: Vec<f32>,
    pub neighbors: Vec<String>,
    pub access_count: u64,
    pub importance: f32,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchHit {
    pub key: String,
    pub namespace: String,
    pub content: String,
    pub cosine_similarity: f32,
    pub hybrid_score: f32,
    pub neighbors_traversed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswPoint {
    pub id: usize,
    pub key: String,
    pub payload: String,
    pub vector: Vec<f32>,
    pub max_layer: usize,
    /// `layers[l]` holds neighbor point indices at skip-layer `l`.
    pub layers: Vec<Vec<usize>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiLayerHnsw {
    pub m: usize,
    pub ef_construction: usize,
    pub entry_point: Option<usize>,
    pub max_layer: usize,
    pub points: Vec<HnswPoint>,
}
