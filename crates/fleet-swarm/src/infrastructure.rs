//! Infrastructure layer for `fleet-swarm`: Git worktree adapter implementing
//! `fleet_kernel::VcsWorktreePort` and fenced worktree provisioning.

use chrono::{Duration, Utc};
use fleet_core::FencedLease;
use fleet_kernel::VcsWorktreePort;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn prepare_fenced_worktree(
    repo_root: &Path,
    task_id: &str,
    provider_id: &str,
    model_name: &str,
    scopes: &[String],
    epoch: u64,
) -> FencedLease {
    let wt_rel = format!(".ai-fleet/worktrees/{}-e{}", task_id, epoch);
    let wt_abs = repo_root.join(&wt_rel);
    let branch = format!("fleet/{}/{}-e{}", provider_id, task_id, epoch);

    let _ = fs::create_dir_all(&wt_abs);
    let _ = Command::new("git")
        .args(["worktree", "add", "--detach", &wt_abs.to_string_lossy(), "HEAD"])
        .current_dir(repo_root)
        .output();

    FencedLease {
        lease_id: format!("lease-{}-e{}", task_id, epoch),
        task_id: task_id.to_string(),
        provider_id: provider_id.to_string(),
        model_name: model_name.to_string(),
        worktree_path: wt_rel,
        branch,
        scopes: scopes.to_vec(),
        epoch,
        issued_at: Utc::now().to_rfc3339(),
        expires_at: (Utc::now() + Duration::minutes(45)).to_rfc3339(),
    }
}

pub struct GitWorktreeAdapter {
    pub repo_root: PathBuf,
}

impl VcsWorktreePort for GitWorktreeAdapter {
    fn prepare_workspace(&self, task_id: &str, base_ref: &str) -> Result<String, String> {
        let wt = self.repo_root.join(".ai-fleet").join("worktrees").join(task_id);
        fs::create_dir_all(&wt).map_err(|e| e.to_string())?;
        let _ = Command::new("git")
            .args(["worktree", "add", "--quiet", "--detach", &wt.to_string_lossy(), base_ref])
            .current_dir(&self.repo_root)
            .output();
        Ok(wt.to_string_lossy().to_string())
    }

    fn seal_checkpoint(&self, workspace_path: &str, message: &str) -> Result<Option<String>, String> {
        let dir = Path::new(workspace_path);
        let _ = Command::new("git").args(["add", "-A"]).current_dir(dir).output();
        let diff = Command::new("git").args(["diff", "--cached", "--quiet"]).current_dir(dir).status().map_err(|e| e.to_string())?;
        if diff.success() {
            return Ok(None);
        }
        let c = Command::new("git")
            .args(["-c", "user.name=ai-fleet", "-c", "user.email=ai-fleet@localhost", "commit", "-q", "--no-verify", "-m", message])
            .current_dir(dir)
            .status()
            .map_err(|e| e.to_string())?;
        if !c.success() {
            return Err("git commit failed".into());
        }
        let head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(dir).output().map_err(|e| e.to_string())?;
        Ok(Some(String::from_utf8_lossy(&head.stdout).trim().to_string()))
    }

    fn changed_paths(&self, workspace_path: &str, from_ref: &str, to_ref: &str) -> Result<Vec<String>, String> {
        let out = Command::new("git")
            .args(["diff", "--name-only", from_ref, to_ref])
            .current_dir(workspace_path)
            .output()
            .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect())
    }
}
