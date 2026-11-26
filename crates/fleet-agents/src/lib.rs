//! `fleet-agents` — 3-Layer Hexagonal Specialist Persona & Slash-Command Crate:
//! - `domain`: `AgentPersona` and `SlashCommandSpec`.
//! - `application`: Capability-vector persona matcher.
//! - `infrastructure`: 64 built-in personas + dynamic `.claude/agents` & `.claude/commands` loader.

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{find_best_persona, match_best_from_slice};
pub use domain::{AgentPersona, SlashCommandSpec};
pub use infrastructure::{catalog_64_agents, load_all_workspace_personas, load_workspace_slash_commands};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn loads_builtin_and_ondisk_ruflo_agents_and_commands() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
        let all_personas = load_all_workspace_personas(&root);
        assert!(all_personas.len() >= 150, "expected 64 built-in + 100+ on-disk .claude/agents, got {}", all_personas.len());

        let cmds = load_workspace_slash_commands(&root);
        assert!(cmds.len() >= 100, "expected >= 100 on-disk .claude/commands, got {}", cmds.len());
    }
}
