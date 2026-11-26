use crate::domain::{ save_state, ConsensusRound, DecisionLedgerRow, FleetState };
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub fn sha256_file(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|b| sha256_bytes(&b))
}

pub fn append_ledger(state: &mut FleetState, actor: &str, action: &str, summary: &str) {
    let seq = (state.decision_ledger.len() as u64) + 1;
    let prev_hash = state
        .decision_ledger
        .last()
        .map(|r| r.row_hash.clone())
        .unwrap_or_else(|| "0".repeat(64));
    let ts = Utc::now().to_rfc3339();
    let material = format!("{}|{}|{}|{}|{}|{}", seq, ts, actor, action, summary, prev_hash);
    let row_hash = sha256_bytes(material.as_bytes());
    state.decision_ledger.push(DecisionLedgerRow {
        seq,
        timestamp: ts,
        actor: actor.to_string(),
        action: action.to_string(),
        payload_summary: summary.to_string(),
        prev_hash,
        row_hash,
    });
}

pub fn verify_ledger(state: &FleetState) -> Result<usize, String> {
    let mut expected_prev = "0".repeat(64);
    for (idx, row) in state.decision_ledger.iter().enumerate() {
        if row.seq != (idx as u64) + 1 {
            return Err(format!("Sequence mismatch at row {}", idx + 1));
        }
        if row.prev_hash != expected_prev {
            return Err(format!("Broken hash chain at row {}", row.seq));
        }
        let material = format!(
            "{}|{}|{}|{}|{}|{}",
            row.seq, row.timestamp, row.actor, row.action, row.payload_summary, row.prev_hash
        );
        let calc = sha256_bytes(material.as_bytes());
        if calc != row.row_hash {
            return Err(format!("Tampered ledger row {} (hash mismatch)", row.seq));
        }
        expected_prev = row.row_hash.clone();
    }
    Ok(state.decision_ledger.len())
}

pub fn run_pbft_consensus(state: &mut FleetState, topic: &str) -> ConsensusRound {
    let seq = (state.consensus_history.len() as u64) + 1;
    let proposal_id = format!("PBFT-{:04}", seq);
    let mut votes = BTreeMap::new();
    let mut total_weight = 0.0;
    let mut approved_weight = 0.0;

    for (pid, prov) in &state.providers {
        for m in &prov.models {
            let weight = (m.trust_score * 0.6) + (m.q_score * 0.4);
            let approve = m.trust_score >= 0.75;
            votes.insert(format!("{}/{}", pid, m.name), (approve, weight));
            total_weight += weight;
            if approve {
                approved_weight += weight;
            }
        }
    }

    let ratio = if total_weight > 0.0 {
        approved_weight / total_weight
    } else {
        0.0
    };
    let quorum = ratio >= 0.667;
    let ts = Utc::now().to_rfc3339();
    let digest = sha256_bytes(format!("{}|{}|{}|{:.4}|{}", proposal_id, topic, seq, ratio, ts).as_bytes());

    let round = ConsensusRound {
        proposal_id: proposal_id.clone(),
        topic: topic.to_string(),
        phase: if quorum {
            "commit -> reply (FINALIZED)".into()
        } else {
            "view-change (REJECTED)".into()
        },
        view_number: 1,
        sequence_number: seq,
        votes,
        weighted_approval: ratio,
        quorum_achieved: quorum,
        digest: digest[..24].to_string(),
        timestamp: ts,
    };

    append_ledger(
        state,
        "queen:pbft",
        "consensus.round",
        &format!("{} on '{}' -> approval={:.1}% quorum={}", proposal_id, topic, ratio * 100.0, quorum),
    );
    state.consensus_history.push(round.clone());
    save_state(state);
    round
}
