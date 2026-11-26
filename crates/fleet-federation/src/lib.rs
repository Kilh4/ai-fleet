//! `fleet-federation` — 3-Layer Hexagonal Cross-Team & A2A Federation Crate:
//! - `domain`: Federated packets, RFC-6962 Merkle proofs, A2A bundles.
//! - `application`: Domain-separated SHA-256 Merkle Tree + inclusion verifier, PII scrubber, Gumbel watermark scorer.
//! - `infrastructure`: A2A Merkle-sealed bundle builder.

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{
    compute_gumbel_watermark_zscore, create_federated_packet, strip_pii_and_secrets,
    verify_merkle_proof, MerkleTree,
};
pub use domain::{A2aArtifactBundle, FederatedEnvelope, MerkleInclusionProof, MerkleStep};
pub use infrastructure::build_a2a_bundle;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6962_merkle_tree_proves_inclusion_and_rejects_tampering() {
        let leaves: Vec<&[u8]> = vec![b"file1:ok", b"file2:ledger", b"file3:rls", b"file4:test", b"file5:odd"];
        let tree = MerkleTree::build(&leaves);
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = tree.prove(i).unwrap();
            assert!(verify_merkle_proof(leaf, &proof), "valid leaf {i} must verify");
            assert!(!verify_merkle_proof(b"tampered", &proof), "tampered leaf {i} must fail");
        }
    }

    #[test]
    fn a2a_bundle_redacts_secrets_and_seals_merkle_root() {
        let bundle = build_a2a_bundle(
            "agent:claude",
            &[("notes.md", "Reach admin@corp.io with sk-ant-secret12345"), ("spec.md", "Clean spec content")],
            "fleet-salt",
        );
        assert_eq!(bundle.total_redactions, 2);
        assert!(bundle.provenance_verified);
        assert_eq!(bundle.merkle_root.len(), 64);
    }
}
