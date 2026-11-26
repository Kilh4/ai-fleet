//! Adaptive routing: pick provider, model and effort for each run from task
//! complexity, attempt history, provider health and measured success rates.

use crate::model::{Complexity, Effort, Task};
use crate::providers::{ModelSpec, ProviderSpec};
use crate::store::Stat;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Role {
    Author,
    Reviewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Author => "author",
            Role::Reviewer => "reviewer",
        }
    }
}

pub struct Candidate<'a> {
    pub spec: &'a ProviderSpec,
    pub binary_ok: bool,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub inflight: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Route {
    pub provider: String,
    pub model: String,
    pub effort: Effort,
    pub score: f64,
    pub reason: String,
}

/// Required model tier and effort for the next run of `task`.
///
/// Every work-related failure (bad tests, crash, rejected review, ...) raises
/// effort by one level and the tier by one step; quota handoffs do not.
pub fn desired(task: &Task, role: Role) -> (u8, Effort) {
    match role {
        Role::Author => {
            let effort = task.complexity.base_effort().escalate(task.attempts);
            let tier = (task.complexity.tier() as u32 + task.attempts.min(2)).min(3) as u8;
            (tier, effort)
        }
        Role::Reviewer => {
            let effort = match task.complexity {
                Complexity::Trivial => Effort::Medium,
                Complexity::Standard => Effort::High,
                Complexity::Deep => Effort::Xhigh,
                Complexity::Critical => Effort::Max,
            };
            (task.complexity.tier().max(2), effort)
        }
    }
}

/// Cheapest model that meets the tier, otherwise the strongest available.
pub fn pick_model(spec: &ProviderSpec, tier: u8) -> Option<&ModelSpec> {
    spec.models
        .iter()
        .filter(|m| m.tier >= tier)
        .min_by_key(|m| m.tier)
        .or_else(|| spec.models.iter().max_by_key(|m| m.tier))
}

pub fn route(
    task: &Task,
    role: Role,
    candidates: &[Candidate<'_>],
    stats: &HashMap<(String, String), Stat>,
    now: DateTime<Utc>,
) -> Result<Route, String> {
    let (tier, effort) = desired(task, role);
    let total_runs: u32 = stats.values().map(|s| s.successes + s.failures).sum();
    let mut excluded = Vec::new();
    let mut scored: Vec<Route> = Vec::new();

    for c in candidates {
        let id = c.spec.id.as_str();
        if !c.spec.enabled {
            excluded.push(format!("{id}: disabled"));
            continue;
        }
        if !c.binary_ok {
            excluded.push(format!("{id}: `{}` not found on PATH", c.spec.binary));
            continue;
        }
        if let Some(until) = c.cooldown_until.filter(|u| *u > now) {
            excluded.push(format!("{id}: cooling down until {}", until.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M")));
            continue;
        }
        if c.inflight >= c.spec.max_concurrency.max(1) {
            excluded.push(format!("{id}: at its concurrency limit ({})", c.spec.max_concurrency));
            continue;
        }
        if role == Role::Reviewer && task.authors.iter().any(|a| a == id) {
            excluded.push(format!("{id}: authored this change (independent review required)"));
            continue;
        }
        if role == Role::Reviewer
            && task
                .reviewer
                .as_deref()
                .is_some_and(|rv| rv.split(',').any(|x| x.trim() == id))
        {
            excluded.push(format!("{id}: already cast an approval vote on this commit"));
            continue;
        }
        let Some(m) = pick_model(c.spec, tier) else {
            excluded.push(format!("{id}: no models configured"));
            continue;
        };

        // UCB1 over (provider, model) with a Beta(1,1) prior: exploit what has
        // worked, keep exploring what has little history.
        let st = stats.get(&(id.to_string(), m.name.clone())).cloned().unwrap_or_default();
        let n = (st.successes + st.failures) as f64;
        let mean = (st.successes as f64 + 1.0) / (n + 2.0);
        let explore = 0.5 * ((2.0 * (total_runs as f64 + 1.0).ln()) / (n + 1.0)).sqrt();
        let mut score = mean + explore;
        let mut notes = vec![format!("success {:.0}% over {} runs", mean * 100.0, n as u32)];

        if role == Role::Author && task.preferred_provider.as_deref() == Some(id) {
            score += 0.25;
            notes.push("preferred".into());
        }
        // After repeated work failures on one provider, favour a fresh pair of eyes.
        if role == Role::Author
            && task.attempts >= 2
            && task.last_provider.as_deref() == Some(id)
            && task.last_failure.as_deref().is_some_and(|k| !matches!(k, "quota" | "auth" | "unavailable" | "cancelled"))
        {
            score -= 0.2;
            notes.push("failed last attempt".into());
        }
        if m.tier < tier {
            score -= 0.1 * f64::from(tier - m.tier);
            notes.push(format!("best model below tier {tier}"));
        }
        score -= 0.05 * c.inflight as f64;

        scored.push(Route {
            provider: id.to_string(),
            model: m.name.clone(),
            effort: effort.min(m.max_effort),
            score,
            reason: notes.join(", "),
        });
    }

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then(a.provider.cmp(&b.provider)));
    let Some(mut best) = scored.first().cloned() else {
        return Err(if excluded.is_empty() { "no providers configured".into() } else { excluded.join("; ") });
    };
    let others: Vec<String> = scored.iter().skip(1).map(|r| format!("{}={:.2}", r.provider, r.score)).collect();
    best.reason = format!(
        "{} complexity={} attempts={} -> tier>={} effort={}; picked {}/{} score={:.2} ({}){}{}",
        role.as_str(),
        task.complexity.as_str(),
        task.attempts,
        tier,
        best.effort,
        best.provider,
        best.model,
        best.score,
        best.reason,
        if others.is_empty() { String::new() } else { format!("; alternatives: {}", others.join(", ")) },
        if excluded.is_empty() { String::new() } else { format!("; excluded: {}", excluded.join("; ")) },
    );
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NewTask, TaskStatus};
    use crate::providers::ModelSpec;

    fn task(complexity: Complexity, attempts: u32) -> Task {
        let nt = NewTask::new("t", "p");
        Task {
            id: "t".into(),
            title: nt.title,
            prompt: nt.prompt,
            status: TaskStatus::Queued,
            priority: 0,
            depends_on: vec![],
            owned_files: vec![],
            complexity,
            verify_cmd: None,
            max_attempts: 5,
            attempts,
            handoffs: 0,
            review_required: true,
            review_quorum: 1,
            preferred_provider: None,
            skills: vec![],
            auto_skills: true,
            branch: None,
            worktree: None,
            base_commit: None,
            head_commit: None,
            authors: vec![],
            reviewer: None,
            last_provider: None,
            last_model: None,
            last_effort: None,
            last_failure: None,
            last_error: None,
            result_summary: None,
            review_feedback: None,
            lease_epoch: 0,
            lease_expires_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn spec(id: &str) -> ProviderSpec {
        ProviderSpec::generic(
            id,
            id,
            &[],
            vec![
                ModelSpec { name: format!("{id}-fast"), tier: 1, max_effort: Effort::High },
                ModelSpec { name: format!("{id}-strong"), tier: 3, max_effort: Effort::Max },
            ],
        )
    }

    fn cands(specs: &[ProviderSpec]) -> Vec<Candidate<'_>> {
        specs.iter().map(|s| Candidate { spec: s, binary_ok: true, cooldown_until: None, inflight: 0 }).collect()
    }

    #[test]
    fn escalates_with_failures() {
        let specs = vec![spec("a")];
        let now = Utc::now();
        let r0 = route(&task(Complexity::Trivial, 0), Role::Author, &cands(&specs), &HashMap::new(), now).unwrap();
        assert_eq!((r0.model.as_str(), r0.effort), ("a-fast", Effort::Low));
        let r2 = route(&task(Complexity::Trivial, 2), Role::Author, &cands(&specs), &HashMap::new(), now).unwrap();
        assert_eq!((r2.model.as_str(), r2.effort), ("a-strong", Effort::High));
        let r9 = route(&task(Complexity::Critical, 9), Role::Author, &cands(&specs), &HashMap::new(), now).unwrap();
        assert_eq!(r9.effort, Effort::Max);
    }

    #[test]
    fn skips_cooldown_and_authors() {
        let specs = vec![spec("a"), spec("b"), spec("c")];
        let now = Utc::now();
        let mut c = cands(&specs);
        c[1].cooldown_until = Some(now + chrono::Duration::hours(1));
        let mut t = task(Complexity::Standard, 0);
        t.authors = vec!["a".into()];
        let r = route(&t, Role::Reviewer, &c, &HashMap::new(), now).unwrap();
        assert_eq!(r.provider, "c");
        assert!(r.reason.contains("b: cooling down"));
        assert!(r.reason.contains("a: authored"));

        c[2].binary_ok = false;
        let err = route(&t, Role::Reviewer, &c, &HashMap::new(), now).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn learns_from_history() {
        let specs = vec![spec("a"), spec("b")];
        let mut stats = HashMap::new();
        stats.insert(("a".to_string(), "a-strong".to_string()), Stat { successes: 2, failures: 18, avg_ms: 0.0 });
        stats.insert(("b".to_string(), "b-strong".to_string()), Stat { successes: 19, failures: 1, avg_ms: 0.0 });
        let r = route(&task(Complexity::Deep, 0), Role::Author, &cands(&specs), &stats, Utc::now()).unwrap();
        assert_eq!(r.provider, "b");
    }
}
