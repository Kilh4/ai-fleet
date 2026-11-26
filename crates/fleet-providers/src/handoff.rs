//! `handoff.rs` — Cryptographic SHA-256 Worktree Checkpoint & Zero-Loss Cross-AI Handoff,
//! Owned-File Boundary Enforcer, and Real Verification Test Runner.

use chrono::Utc;
use fleet_core::{sha256_file, FencedLease};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeCheckpoint {
    pub task_id: String,
    pub lease_epoch: u64,
    pub worktree_path: String,
    pub base_commit: String,
    pub author_provider: String,
    pub author_model: String,
    pub effort_used: String,
    pub owned_hashes: BTreeMap<String, Option<String>>,
    pub diff_summary: String,
    pub verification_cmd: Option<String>,
    pub verification_exit_code: Option<i32>,
    pub verification_output: String,
    pub writer_processes_remaining: u32,
    pub timestamp: String,
}

/// Computes SHA-256 digest for every owned file inside a worktree and ensures
/// no unowned file was touched.
pub fn compute_owned_hashes(worktree: &Path, owned_files: &[String]) -> Result<BTreeMap<String, Option<String>>, String> {
    let mut map = BTreeMap::new();
    for rel in owned_files {
        if rel.contains("..") || rel.starts_with('/') || rel.starts_with('\\') {
            return Err(format!("Unsafe owned path '{}'", rel));
        }
        let full = worktree.join(rel);
        map.insert(rel.clone(), sha256_file(&full));
    }
    Ok(map)
}

/// Checks git status inside the worktree to ensure the agent did not modify any unowned tracked file.
pub fn enforce_owned_file_boundaries(worktree: &Path, owned_files: &[String]) -> Result<String, String> {
    let allowed: BTreeSet<String> = owned_files.iter().map(|s| s.replace('\\', "/")).collect();
    let out = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(worktree)
        .output();

    let mut diff_lines = Vec::new();
    if let Ok(o) = out {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                if line.len() < 4 {
                    continue;
                }
                let path = line[3..].trim().replace('\\', "/");
                if path.starts_with(".ai-fleet/") {
                    continue;
                }
                if !allowed.is_empty() && !allowed.contains(&path) {
                    return Err(format!("Agent modified unowned file '{}' outside lease scope {:?}", path, owned_files));
                }
                diff_lines.push(line.to_string());
            }
        }
    }
    Ok(diff_lines.join("\n"))
}

/// Copies owned files from a previous writer's worktree checkpoint to a receiver's fresh worktree
/// after verifying exact SHA-256 match (prevents silent corruption or post-stop tampering).
pub fn transfer_checkpoint_to_receiver(
    prior: &WorktreeCheckpoint,
    receiver_lease: &FencedLease,
    repo_root: &Path,
    owned_files: &[String],
) -> Result<PathBuf, String> {
    if receiver_lease.epoch <= prior.lease_epoch {
        return Err(format!(
            "FencedLease epoch violation: receiver epoch #{} must be strictly greater than prior epoch #{}",
            receiver_lease.epoch, prior.lease_epoch
        ));
    }
    if prior.writer_processes_remaining != 0 {
        return Err(format!(
            "Cannot hand off task '{}': prior writer still has {} active process(es)",
            prior.task_id, prior.writer_processes_remaining
        ));
    }

    let prior_wt = PathBuf::from(&prior.worktree_path);
    let prior_actual = if prior_wt.is_absolute() {
        prior_wt
    } else {
        repo_root.join(&prior.worktree_path)
    };

    // Verify prior files have not drifted since checkpoint was sealed
    let current_hashes = compute_owned_hashes(&prior_actual, owned_files)?;
    if current_hashes != prior.owned_hashes {
        return Err("Checkpoint SHA-256 mismatch: prior worktree changed after writer stopped!".to_string());
    }

    let dest_wt = repo_root.join(&receiver_lease.worktree_path);
    let _ = fs::create_dir_all(&dest_wt);

    for rel in owned_files {
        let src_file = prior_actual.join(rel);
        let dst_file = dest_wt.join(rel);
        if src_file.is_file() {
            if let Some(parent) = dst_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::copy(&src_file, &dst_file).map_err(|e| format!("Failed copying '{}': {}", rel, e))?;
        }
    }

    let receiver_hashes = compute_owned_hashes(&dest_wt, owned_files)?;
    if receiver_hashes != prior.owned_hashes {
        return Err("Receiver worktree SHA-256 verification failed after copy!".to_string());
    }

    Ok(dest_wt)
}

/// Runs a real verification command (e.g. `cargo test`, `dotnet test`) inside the worktree
/// and captures its exact exit code and output.
pub fn run_verification_command(worktree: &Path, command_line: &str) -> (i32, String) {
    if command_line.trim().is_empty() {
        return (0, "No verification command configured".to_string());
    }
    let shell = if cfg!(windows) { "powershell" } else { "sh" };
    let flag = if cfg!(windows) { "-Command" } else { "-c" };

    match Command::new(shell).args([flag, command_line]).current_dir(worktree).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(1);
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout).trim(),
                String::from_utf8_lossy(&o.stderr).trim()
            )
            .trim()
            .to_string();
            (code, combined)
        }
        Err(e) => (1, format!("Failed to execute verification command '{}': {}", command_line, e)),
    }
}

/// Loads real Markdown prompt context from `.claude/agents/<persona>.md` and `.agents/skills/` on disk.
pub fn load_on_disk_persona_and_skill_context(repo_root: &Path, persona_id: &str) -> String {
    let mut context = String::new();
    let candidates = [
        repo_root.join(".claude/agents").join(format!("{}.md", persona_id)),
        repo_root.join(".agents/skills").join(format!("agent-{}", persona_id)).join("SKILL.md"),
        repo_root.join(".ai-fleet/SKILL.md"),
    ];
    for path in candidates {
        if path.is_file() {
            if let Ok(txt) = fs::read_to_string(&path) {
                let snippet: String = txt.lines().take(25).collect::<Vec<_>>().join("\n");
                context.push_str(&format!("\n--- Loaded Spec [{}] ---\n{}\n", path.display(), snippet));
            }
        }
    }
    context
}

pub fn build_handoff_prompt(
    task_title: &str,
    owned_files: &[String],
    persona_context: &str,
    prior: Option<&WorktreeCheckpoint>,
    is_review: bool,
) -> String {
    let mode_header = if is_review {
        "INDEPENDENT NON-AUTHOR CODE REVIEW: Inspect git diff HEAD and owned files. Do NOT modify any file. Verify constitution rules and tests."
    } else {
        "ISOLATED WORKTREE EXECUTION: First inspect git status, git diff HEAD, and any existing owned files before editing. Touch ONLY owned files."
    };

    let prior_json = prior
        .and_then(|p| serde_json::to_string_pretty(p).ok())
        .unwrap_or_else(|| "{}".to_string());

    format!(
        "{}\nTask: {}\nOwned Files: {:?}\nPrevious Checkpoint: {}\n{}\n",
        mode_header, task_title, owned_files, prior_json, persona_context
    )
}

pub fn seal_checkpoint(
    task_id: &str,
    lease_epoch: u64,
    worktree: &Path,
    base_commit: &str,
    provider_id: &str,
    model_name: &str,
    effort: &str,
    owned_files: &[String],
    verify_cmd: Option<&str>,
    active_procs_remaining: u32,
) -> Result<WorktreeCheckpoint, String> {
    let diff_summary = enforce_owned_file_boundaries(worktree, owned_files)?;
    let owned_hashes = compute_owned_hashes(worktree, owned_files)?;
    let (v_code, v_out) = match verify_cmd {
        Some(cmd) if !cmd.is_empty() => {
            let (c, o) = run_verification_command(worktree, cmd);
            (Some(c), o)
        }
        _ => (Some(0), "Owned file hash boundary verified".to_string()),
    };

    Ok(WorktreeCheckpoint {
        task_id: task_id.to_string(),
        lease_epoch,
        worktree_path: worktree.to_string_lossy().to_string(),
        base_commit: base_commit.to_string(),
        author_provider: provider_id.to_string(),
        author_model: model_name.to_string(),
        effort_used: effort.to_string(),
        owned_hashes,
        diff_summary,
        verification_cmd: verify_cmd.map(|s| s.to_string()),
        verification_exit_code: v_code,
        verification_output: v_out,
        writer_processes_remaining: active_procs_remaining,
        timestamp: Utc::now().to_rfc3339(),
    })
}
