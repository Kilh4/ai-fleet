//! Application layer for `fleet-swarm`:
//! - `PbftStateMachine`: $3f+1$ quorum math, `PrePrepare -> Prepare -> Commit`, equivocation detection & deterministic View-Change.
//! - `RaftNode`: Election safety (up-to-date log check) + majority `commit_index` advancement.
//! - `GossipCrdtMap`: Commutative, associative, idempotent lattice merge with causal Vector Clocks.
//! - Queen sub-task decomposition & composite MoE/Pheromone routing.

use crate::domain::*;
use chrono::Utc;
use fleet_agents::find_best_persona;
use fleet_core::{sha256_str, AiProvider, SubTaskSpec, SwarmTask};
use fleet_kernel::ConsensusPort;
use fleet_neural::{route_moe_experts, ExpertSlot, ReasoningPattern};
use std::collections::{BTreeMap, BTreeSet};

impl PbftStateMachine {
    pub fn new(replicas: Vec<String>) -> Self {
        PbftStateMachine {
            replicas,
            view: 0,
            seq: 1,
            phase: PbftPhase::Idle,
            active_digest: None,
            prepare_votes: BTreeMap::new(),
            commit_votes: BTreeSet::new(),
            equivocations: Vec::new(),
        }
    }

    /// Maximum tolerated Byzantine faulty replicas $f = \lfloor (n - 1) / 3 \rfloor$.
    pub fn max_faulty(&self) -> usize {
        self.replicas.len().saturating_sub(1) / 3
    }

    /// Quorum certificate size $2f + 1$.
    pub fn quorum_size(&self) -> usize {
        2 * self.max_faulty() + 1
    }

    pub fn primary(&self) -> Option<&str> {
        if self.replicas.is_empty() {
            None
        } else {
            Some(self.replicas[(self.view as usize) % self.replicas.len()].as_str())
        }
    }

    pub fn pre_prepare(&mut self, sender: &str, digest: impl Into<String>) -> Result<(), String> {
        if self.primary() != Some(sender) {
            return Err(format!("`{sender}` is not primary for view {}", self.view));
        }
        let d = digest.into();
        self.active_digest = Some(d.clone());
        self.phase = PbftPhase::PrePrepared;
        self.prepare_votes.clear();
        self.commit_votes.clear();
        self.prepare_votes.insert(sender.to_string(), d);
        Ok(())
    }

    /// Record a replica's `Prepare` vote; detects Byzantine equivocation if the
    /// same replica signs two distinct digests in `(view, seq)` and triggers view-change!
    pub fn on_prepare(&mut self, replica: &str, digest: &str) -> PbftPhase {
        if let Some(prev) = self.prepare_votes.get(replica) {
            if prev != digest {
                self.equivocations.push(EquivocationProof {
                    replica: replica.to_string(),
                    view: self.view,
                    seq: self.seq,
                    digest_a: prev.clone(),
                    digest_b: digest.to_string(),
                });
                self.trigger_view_change();
                return self.phase;
            }
        }
        self.prepare_votes.insert(replica.to_string(), digest.to_string());
        if let Some(target) = &self.active_digest {
            let matching = self.prepare_votes.values().filter(|d| *d == target).count();
            if matching >= self.quorum_size() && self.phase == PbftPhase::PrePrepared {
                self.phase = PbftPhase::Prepared;
            }
        }
        self.phase
    }

    pub fn on_commit(&mut self, replica: &str, digest: &str) -> PbftPhase {
        if self.active_digest.as_deref() == Some(digest) {
            self.commit_votes.insert(replica.to_string());
            if self.phase == PbftPhase::Prepared && self.commit_votes.len() >= self.quorum_size() {
                self.phase = PbftPhase::Committed;
            }
        }
        self.phase
    }

    pub fn trigger_view_change(&mut self) {
        self.view += 1;
        self.phase = PbftPhase::ViewChanging;
        self.active_digest = None;
        self.prepare_votes.clear();
        self.commit_votes.clear();
    }
}

// ─── Raft Election & Majority Commit Safety ─────────────────────────────────

#[derive(Debug, Clone)]
pub struct RaftNode {
    pub id: String,
    pub peers: Vec<String>,
    pub current_term: u64,
    pub voted_for: Option<String>,
    pub role: RaftRole,
    pub log: Vec<RaftLogEntry>,
    pub commit_index: u64,
}

impl RaftNode {
    pub fn new(id: impl Into<String>, peers: Vec<String>) -> Self {
        RaftNode {
            id: id.into(),
            peers,
            current_term: 0,
            voted_for: None,
            role: RaftRole::Follower,
            log: Vec::new(),
            commit_index: 0,
        }
    }

    pub fn last_log_term_and_index(&self) -> (u64, u64) {
        self.log.last().map(|e| (e.term, e.index)).unwrap_or((0, 0))
    }

    /// Raft §5.4.1 Election Restriction: grant vote iff candidate's term >= ours
    /// AND candidate's log is at least as up-to-date as ours.
    pub fn handle_request_vote(
        &mut self,
        candidate_id: &str,
        candidate_term: u64,
        candidate_last_term: u64,
        candidate_last_idx: u64,
    ) -> bool {
        if candidate_term < self.current_term {
            return false;
        }
        if candidate_term > self.current_term {
            self.current_term = candidate_term;
            self.voted_for = None;
            self.role = RaftRole::Follower;
        }
        let can_vote = self.voted_for.as_deref().map_or(true, |v| v == candidate_id);
        let (my_term, my_idx) = self.last_log_term_and_index();
        let up_to_date = (candidate_last_term, candidate_last_idx) >= (my_term, my_idx);
        if can_vote && up_to_date {
            self.voted_for = Some(candidate_id.to_string());
            true
        } else {
            false
        }
    }

    /// Advance `commit_index` to the highest index replicated on a strict majority
    /// whose entry term matches `current_term` (Raft §5.4.2 safety).
    pub fn advance_commit_index(&mut self, peer_match_indices: &[u64]) -> u64 {
        let cluster_size = self.peers.len() + 1;
        let majority = cluster_size / 2 + 1;
        let (_, my_last) = self.last_log_term_and_index();
        let mut all_indices = Vec::with_capacity(cluster_size);
        all_indices.push(my_last);
        all_indices.extend_from_slice(peer_match_indices);
        all_indices.sort_unstable_by(|a, b| b.cmp(a));
        let candidate_n = all_indices[majority - 1];
        if candidate_n > self.commit_index {
            if self.log.iter().any(|e| e.index == candidate_n && e.term == self.current_term) {
                self.commit_index = candidate_n;
            }
        }
        self.commit_index
    }
}

// ─── Vector-Clock LWW CRDT Gossip Merge ─────────────────────────────────────

impl VectorClock {
    pub fn tick(&mut self, node: &str) {
        *self.dots.entry(node.to_string()).or_insert(0) += 1;
    }

    pub fn merge_from(&mut self, other: &VectorClock) {
        for (k, &v) in &other.dots {
            let entry = self.dots.entry(k.clone()).or_insert(0);
            *entry = (*entry).max(v);
        }
    }

    /// True iff `self <= other` in every coordinate and `<` in at least one.
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut strictly_less = false;
        for (k, &v) in &self.dots {
            let ov = *other.dots.get(k).unwrap_or(&0);
            if v > ov {
                return false;
            }
            if v < ov {
                strictly_less = true;
            }
        }
        for (k, &ov) in &other.dots {
            if !self.dots.contains_key(k) && ov > 0 {
                strictly_less = true;
            }
        }
        strictly_less
    }
}

impl GossipCrdtMap {
    pub fn put(&mut self, node_id: &str, key: impl Into<String>, value: impl Into<String>) {
        let k = key.into();
        let mut clock = self.entries.get(&k).map(|r| r.clock.clone()).unwrap_or_default();
        clock.tick(node_id);
        self.entries.insert(k, CrdtRegister { value: value.into(), clock, writer: node_id.into() });
    }

    /// Commutative, associative, idempotent join semi-lattice merge.
    pub fn merge(&mut self, remote: &GossipCrdtMap) {
        for (k, rem) in &remote.entries {
            match self.entries.get(k) {
                None => {
                    self.entries.insert(k.clone(), rem.clone());
                }
                Some(loc) => {
                    if loc.clock.happens_before(&rem.clock) {
                        self.entries.insert(k.clone(), rem.clone());
                    } else if rem.clock.happens_before(&loc.clock) {
                        // keep local
                    } else if (&rem.writer, &rem.value) > (&loc.writer, &loc.value) {
                        // Concurrent write tie-breaker: deterministic lexical `(writer, value)` + merged clock!
                        let mut merged_clock = loc.clock.clone();
                        merged_clock.merge_from(&rem.clock);
                        self.entries.insert(
                            k.clone(),
                            CrdtRegister { value: rem.value.clone(), clock: merged_clock, writer: rem.writer.clone() },
                        );
                    } else {
                        let mut merged_clock = loc.clock.clone();
                        merged_clock.merge_from(&rem.clock);
                        self.entries.insert(
                            k.clone(),
                            CrdtRegister { value: loc.value.clone(), clock: merged_clock, writer: loc.writer.clone() },
                        );
                    }
                }
            }
        }
    }
}

pub struct PbftConsensusAdapter;

impl ConsensusPort for PbftConsensusAdapter {
    fn protocol_name(&self) -> &str {
        "PBFT-Byzantine-2f+1"
    }
    fn propose_and_tally(&self, _proposal_id: &str, votes: &[(String, bool, f64)]) -> Result<(bool, f64), String> {
        let total: f64 = votes.iter().map(|(_, _, w)| *w).sum();
        let yes: f64 = votes.iter().filter(|(_, ok, _)| *ok).map(|(_, _, w)| *w).sum();
        let ratio = if total > 0.0 { yes / total } else { 0.0 };
        Ok((ratio >= (2.0 / 3.0), ratio))
    }
}

// ─── Preserved Queen & Swarm Consensus/Routing Functions ────────────────────

pub fn queen_decompose_task(task: &SwarmTask) -> QueenTaskAnalysis {
    let persona = find_best_persona(&format!("{} {}", task.title, task.owned_files.join(" ")));
    let file_count = task.owned_files.len().max(1);
    let complexity = ((file_count as f64 * 0.18) + if task.tier == "critical" { 0.55 } else { 0.25 }).clamp(0.15, 0.99);

    let sub1_id = format!("{}-spec", task.id);
    let sub2_id = format!("{}-impl", task.id);
    let sub3_id = format!("{}-verify", task.id);

    let subtasks = vec![
        SubTaskSpec {
            id: sub1_id.clone(),
            title: format!("Inspect git diff HEAD, checkpoints & contracts for '{}'", task.title),
            assigned_persona: "system-architect".into(),
            domain: "architecture".into(),
            status: "completed".into(),
            dependencies: vec![],
        },
        SubTaskSpec {
            id: sub2_id.clone(),
            title: format!("Execute isolated worktree implementation across {} owned file(s)", file_count),
            assigned_persona: persona.id.clone(),
            domain: persona.domain.clone(),
            status: "ready".into(),
            dependencies: vec![sub1_id],
        },
        SubTaskSpec {
            id: sub3_id,
            title: "Run independent non-author verification & test evidence capture".into(),
            assigned_persona: "independent-reviewer".into(),
            domain: "testing-qa".into(),
            status: "pending".into(),
            dependencies: vec![sub2_id],
        },
    ];

    QueenTaskAnalysis {
        task_id: task.id.clone(),
        complexity_score: complexity,
        recommended_domain: persona.domain,
        primary_persona: persona.id,
        subtasks,
        bottleneck_warning: (file_count > 10).then(|| format!("Task touches {} files; consider splitting into smaller slices", file_count)),
    }
}

pub fn execute_consensus(
    providers: &BTreeMap<String, AiProvider>,
    protocol: &str,
    topic: &str,
    seq: u64,
) -> ConsensusRound {
    let proto_upper = protocol.to_uppercase();
    let proposal_id = format!("{}-{:04}", proto_upper, seq);
    let mut votes = BTreeMap::new();
    let mut total_weight = 0.0;
    let mut approved_weight = 0.0;
    let mut leader = "queen:primary".to_string();
    let mut best_leader_score = -1.0_f64;

    for (pid, prov) in providers {
        for m in &prov.models {
            let weight = (m.trust_score * 0.55) + (m.q_score * 0.45);
            if prov.status == "ready" && weight > best_leader_score {
                best_leader_score = weight;
                leader = format!("{}/{}", pid, m.name);
            }
            let approve = m.trust_score >= 0.70;
            votes.insert(format!("{}/{}", pid, m.name), (approve, weight));
            total_weight += weight;
            if approve {
                approved_weight += weight;
            }
        }
    }

    let ratio = if total_weight > 0.0 { approved_weight / total_weight } else { 0.0 };
    let (threshold, phase_str) = match proto_upper.as_str() {
        "RAFT" => (0.501, "leader-election -> log-append -> majority-commit (COMMITTED)"),
        "GOSSIP" => (0.500, "epidemic-push -> anti-entropy-pull -> converged (CONVERGED)"),
        _ => (0.667, "pre-prepare -> prepare -> commit -> reply (PBFT FINALIZED)"),
    };
    let quorum = ratio >= threshold;
    let ts = Utc::now().to_rfc3339();
    let digest = sha256_str(&format!("{}|{}|{}|{:.4}|{}", proposal_id, topic, leader, ratio, ts));

    ConsensusRound {
        proposal_id,
        protocol: proto_upper,
        topic: topic.to_string(),
        phase: if quorum { phase_str.to_string() } else { "quorum-failed (ABORTED)".to_string() },
        leader_or_primary: leader,
        term_or_view: seq,
        votes,
        weighted_approval: ratio,
        quorum_achieved: quorum,
        digest: digest[..24].to_string(),
        timestamp: ts,
    }
}

pub fn classify_task_tier(title: &str, owned_files: &[String], retries: u32, is_review: bool) -> (String, String) {
    let joined = format!("{} {}", title.to_lowercase(), owned_files.join(" ").to_lowercase());
    let critical_keywords = [
        "migration", ".sql", "money", "audit", "access", "scram", "security",
        "syncspike", "adr-", "kernel", "workflow", "review", "ledger", "rls", "critical",
    ];
    let deep_keywords = [
        ".cs", ".ts", ".rs", "golden/", ".case.yaml", "test", "screen", "extract", "api", "worker",
    ];

    if is_review || retries > 0 || critical_keywords.iter().any(|k| joined.contains(k)) {
        let effort = if is_review || retries > 0 { "max" } else { "xhigh" };
        ("critical".to_string(), effort.to_string())
    } else if deep_keywords.iter().any(|k| joined.contains(k)) {
        ("deep".to_string(), "high".to_string())
    } else {
        ("standard".to_string(), "medium".to_string())
    }
}

pub fn compute_optimal_route(
    providers: &BTreeMap<String, AiProvider>,
    moe_experts: &[ExpertSlot],
    reasoning_bank: &[ReasoningPattern],
    task: &SwarmTask,
    busy_providers: &BTreeSet<String>,
    is_review: bool,
) -> Option<RouteDecision> {
    let (tier, base_effort) = classify_task_tier(&task.title, &task.owned_files, task.retries, is_review);
    let query_str = format!("{} {}", task.title, task.owned_files.join(" "));
    let moe = route_moe_experts(moe_experts, &query_str, 2);
    let persona = find_best_persona(&query_str);

    let low = task.title.to_lowercase();
    let matched_pattern = reasoning_bank
        .iter()
        .find(|p| {
            (low.contains("migration") || low.contains("money")) && p.domain == "financial-migration"
                || (low.contains("security") || low.contains("audit")) && p.domain == "rls-and-audit"
                || p.domain == "quota-handoff"
        })
        .map(|p| format!("{} ({} - {:.0}% success)", p.pattern_id, p.domain, p.success_rate * 100.0));

    let mut ranked: Vec<(f64, String, String, String)> = Vec::new();
    let ready_count = providers.values().filter(|p| p.status == "ready").count();

    for (pid, prov) in providers {
        if prov.status != "ready" || busy_providers.contains(pid) {
            continue;
        }
        if is_review && task.authors.contains(pid) && ready_count > 1 {
            continue;
        }
        let quota_ratio = if prov.daily_quota_limit > 0 {
            (prov.daily_quota_used as f64) / (prov.daily_quota_limit as f64)
        } else {
            0.0
        };
        if quota_ratio >= 1.0 {
            continue;
        }

        for m in &prov.models {
            let tier_bonus = if m.tier == tier { 0.18 } else if tier == "critical" && m.tier == "deep" { 0.05 } else { 0.0 };
            let conservation_bonus = if tier == "standard" && m.tier != "critical" { 0.14 } else { 0.0 };
            let headroom_bonus = (1.0 - quota_ratio) * 0.12;
            let pheromone_bonus = (m.pheromone - 1.0).clamp(-0.2, 0.40) * 0.25;
            let trust_bonus = m.trust_score * 0.15;
            let fisher_bonus = m.ewc_fisher_importance * 0.08;
            let total_score = m.q_score + tier_bonus + conservation_bonus + headroom_bonus + pheromone_bonus + trust_bonus + fisher_bonus;

            let effort = if task.retries > 0 || is_review {
                if pid == "claude" { "max".to_string() } else { "xhigh".to_string() }
            } else if tier == "critical" {
                m.default_effort.clone()
            } else {
                base_effort.clone()
            };
            ranked.push((total_score, pid.clone(), m.name.clone(), effort));
        }
    }

    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let (top_score, best_prov, best_model, best_effort) = ranked.first()?.clone();
    let fallback_chain = ranked.into_iter().skip(1).map(|(_, p, m, e)| (p, m, e)).collect();
    let top_exp_str = moe.top_experts.iter().map(|(n, w)| format!("{}:{:.0}%", n, w * 100.0)).collect::<Vec<_>>().join("+");

    Some(RouteDecision {
        task_id: task.id.clone(),
        classified_tier: tier,
        assigned_persona: persona.id.clone(),
        moe_experts: moe.top_experts,
        moe_entropy: moe.entropy,
        ewc_loss: moe.ewc_regularization_loss,
        matched_pattern,
        selected_provider: best_prov.clone(),
        selected_model: best_model.clone(),
        selected_effort: best_effort.clone(),
        composite_score: top_score,
        fallback_chain,
        reason: format!("Persona[{}] + MoE[{}] -> {}/{} (effort={}, score={:.3})", persona.id, top_exp_str, best_prov, best_model, best_effort, top_score),
    })
}
