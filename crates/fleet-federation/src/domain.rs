//! Domain layer for `fleet-federation`: Federated packets, RFC-6962 Binary
//! Merkle Inclusion Proofs, and Agent-to-Agent (`A2A`) Bundle manifests.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedEnvelope {
    pub packet_id: String,
    pub sender_team: String,
    pub target_peer: String,
    pub sanitized_payload: String,
    pub pii_redactions_count: usize,
    pub watermark_digest: String,
    pub z_score: f64,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleStep {
    pub sibling_hex: String,
    /// `true` if sibling is on the left (`H(0x01 || sibling || cur)`), `false` if right.
    pub sibling_is_left: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleInclusionProof {
    pub leaf_index: usize,
    pub root_hex: String,
    pub path: Vec<MerkleStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aArtifactBundle {
    pub bundle_id: String,
    pub sender_agent: String,
    pub merkle_root: String,
    pub sanitized_artifacts: Vec<(String, String)>,
    pub total_redactions: usize,
    pub provenance_verified: bool,
}
