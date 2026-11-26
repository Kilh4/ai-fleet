//! Application layer for `fleet-agents`: Capability-vector persona matcher
//! across both built-in and on-disk `.claude/agents/*.md` personas.

use crate::domain::AgentPersona;
use crate::infrastructure::catalog_64_agents;

pub fn match_best_from_slice(catalog: &[AgentPersona], query: &str) -> AgentPersona {
    let low = query.to_lowercase();
    let mut best = catalog[0].clone();
    let mut best_score = 0usize;

    for agent in catalog {
        let mut score = 0usize;
        if low.contains(&agent.id) {
            score += 5;
        }
        for cap in &agent.capabilities {
            if low.contains(cap) {
                score += 3;
            }
        }
        for word in agent.role_title.to_lowercase().split_whitespace() {
            if word.len() > 3 && low.contains(word) {
                score += 2;
            }
        }
        if score > best_score {
            best_score = score;
            best = agent.clone();
        }
    }
    best
}

pub fn find_best_persona(query: &str) -> AgentPersona {
    let catalog = catalog_64_agents();
    match_best_from_slice(&catalog, query)
}
