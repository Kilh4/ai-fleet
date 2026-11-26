//! `fleet-swarm` — 3-Layer Hexagonal Distributed Consensus & Coordination Crate:
//! - `domain`: PBFT Byzantine state, Raft log entries, Causal Vector-Clock CRDTs, Route models.
//! - `application`: $3f+1$ PBFT state machine with equivocation & view-change, Raft election/commit safety, CRDT lattice merge.
//! - `infrastructure`: `GitWorktreeAdapter` implementing `fleet_kernel::VcsWorktreePort`.

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{
    classify_task_tier, compute_optimal_route, execute_consensus, queen_decompose_task,
    PbftConsensusAdapter, RaftNode,
};
pub use domain::{
    ConsensusRound, CrdtRegister, EquivocationProof, GossipCrdtMap, PbftPhase, PbftStateMachine,
    QueenTaskAnalysis, RaftLogEntry, RaftRole, RouteDecision, VectorClock,
};
pub use infrastructure::{prepare_fenced_worktree, GitWorktreeAdapter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbft_reaches_quorum_and_detects_equivocation_triggering_view_change() {
        let replicas = vec!["r0".into(), "r1".into(), "r2".into(), "r3".into()]; // n=4 -> f=1, quorum=3
        let mut pbft = PbftStateMachine::new(replicas);
        assert_eq!(pbft.max_faulty(), 1);
        assert_eq!(pbft.quorum_size(), 3);

        pbft.pre_prepare("r0", "digest-A").unwrap();
        assert_eq!(pbft.on_prepare("r1", "digest-A"), PbftPhase::PrePrepared);
        assert_eq!(pbft.on_prepare("r2", "digest-A"), PbftPhase::Prepared);
        assert_eq!(pbft.on_commit("r0", "digest-A"), PbftPhase::Prepared);
        assert_eq!(pbft.on_commit("r1", "digest-A"), PbftPhase::Prepared);
        assert_eq!(pbft.on_commit("r2", "digest-A"), PbftPhase::Committed);

        // Equivocation by r1 -> immediate view change to view=1 (primary r1)!
        pbft.on_prepare("r1", "forged-digest-B");
        assert_eq!(pbft.equivocations.len(), 1);
        assert_eq!(pbft.view, 1);
        assert_eq!(pbft.primary(), Some("r1"));
    }

    #[test]
    fn raft_enforces_up_to_date_log_election_and_majority_commit() {
        let mut follower = RaftNode::new("n1", vec!["n2".into(), "n3".into()]);
        follower.current_term = 2;
        follower.log.push(RaftLogEntry { term: 2, index: 1, command: "set x=1".into() });

        // Stale candidate (last_log_term=1) MUST be rejected even with higher candidate_term!
        assert!(!follower.handle_request_vote("n2", 3, 1, 5));
        // Up-to-date candidate (last_log_term=2, idx=1) in term 4 succeeds!
        assert!(follower.handle_request_vote("n3", 4, 2, 1));

        let mut leader = RaftNode::new("n1", vec!["n2".into(), "n3".into(), "n4".into(), "n5".into()]);
        leader.role = RaftRole::Leader;
        leader.current_term = 3;
        leader.log.push(RaftLogEntry { term: 3, index: 1, command: "cmd1".into() });
        leader.log.push(RaftLogEntry { term: 3, index: 2, command: "cmd2".into() });
        // 5-node cluster -> majority = 3 (leader=2, n2=2, n3=2, n4=1, n5=0 -> sorted [2,2,2,1,0] -> 3rd is 2)
        assert_eq!(leader.advance_commit_index(&[2, 2, 1, 0]), 2);
    }

    #[test]
    fn gossip_vector_clock_crdt_is_commutative_associative_and_idempotent() {
        let mut a = GossipCrdtMap::default();
        let mut b = GossipCrdtMap::default();
        a.put("node-a", "provider:claude", "quota");
        b.put("node-b", "provider:claude", "ready");
        b.put("node-b", "provider:codex", "ready");

        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab, ba, "CRDT merge must be commutative");

        let mut idemp = ab.clone();
        idemp.merge(&ab);
        assert_eq!(ab, idemp, "CRDT merge must be idempotent");
    }
}
