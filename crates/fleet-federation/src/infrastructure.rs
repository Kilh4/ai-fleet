//! Infrastructure layer for `fleet-federation`: A2A Merkle-sealed bundle builder.

use crate::application::{compute_gumbel_watermark_zscore, strip_pii_and_secrets, MerkleTree};
use crate::domain::A2aArtifactBundle;
use fleet_core::sha256_str;

pub fn build_a2a_bundle(
    sender_agent: &str,
    artifacts: &[(&str, &str)],
    salt: &str,
) -> A2aArtifactBundle {
    let mut sanitized = Vec::with_capacity(artifacts.len());
    let mut total_redactions = 0usize;
    for (path, content) in artifacts {
        let (clean, r) = strip_pii_and_secrets(content);
        let marked = format!("{} [wm:{}]", clean, &sha256_str(salt)[..8]);
        total_redactions += r;
        sanitized.push(((*path).to_string(), marked));
    }

    let leaf_strings: Vec<String> = sanitized.iter().map(|(p, c)| format!("{p}:{c}")).collect();
    let leaf_refs: Vec<&[u8]> = leaf_strings.iter().map(|s| s.as_bytes()).collect();
    let tree = MerkleTree::build(&leaf_refs);
    let root = tree.root_hex();
    let provenance_ok = sanitized.iter().all(|(_, c)| compute_gumbel_watermark_zscore(c, salt).2);

    A2aArtifactBundle {
        bundle_id: format!("a2a-{}", &root[..12]),
        sender_agent: sender_agent.to_string(),
        merkle_root: root,
        sanitized_artifacts: sanitized,
        total_redactions,
        provenance_verified: provenance_ok,
    }
}
