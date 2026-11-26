//! Git plumbing: per-task worktrees, checkpoint commits (the unit of
//! handoff between providers), scope checks and integration merges.

use crate::error::{EngineError, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use std::process::Command;

const IDENT: [&str; 8] = [
    "-c",
    "user.name=ai-fleet",
    "-c",
    "user.email=ai-fleet@localhost",
    "-c",
    "commit.gpgsign=false",
    "-c",
    "core.editor=true",
];

pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", "true")
        .output()
        .map_err(|e| EngineError::Git(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return Err(EngineError::Git(format!(
            "`git {}` in {} failed: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

fn git_with_ident(dir: &Path, args: &[&str]) -> Result<String> {
    let mut full: Vec<&str> = IDENT.to_vec();
    full.extend_from_slice(args);
    git(dir, &full)
}

pub fn repo_root(dir: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?))
}

pub fn head(dir: &Path) -> Result<String> {
    git(dir, &["rev-parse", "HEAD"])
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok()
}

/// Make sure `.ai-fleet/engine/` never shows up as untracked in the user's repo.
pub fn ensure_excluded(repo: &Path, pattern: &str) -> Result<()> {
    let common = git(repo, &["rev-parse", "--git-common-dir"])?;
    let common = if Path::new(&common).is_absolute() { PathBuf::from(common) } else { repo.join(common) };
    let exclude = common.join("info").join("exclude");
    let current = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !current.lines().any(|l| l.trim() == pattern) {
        std::fs::create_dir_all(exclude.parent().unwrap_or(&common))?;
        let mut next = current;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(pattern);
        next.push('\n');
        std::fs::write(&exclude, next)?;
    }
    Ok(())
}

/// Create (or reuse) a worktree at `path` on `branch`, starting from `base`.
pub fn ensure_worktree(repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
    if path.join(".git").exists() {
        return Ok(());
    }
    let _ = git(repo, &["worktree", "prune"]);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let p = path.to_string_lossy().to_string();
    if branch_exists(repo, branch) {
        git(repo, &["worktree", "add", "--quiet", &p, branch])?;
    } else {
        git(repo, &["worktree", "add", "--quiet", "-b", branch, &p, base])?;
    }
    Ok(())
}

pub fn status_porcelain(wt: &Path) -> Result<String> {
    git(wt, &["status", "--porcelain", "--untracked-files=all"])
}

/// Commit everything in the worktree. Returns the new HEAD, or `None` if
/// there was nothing to commit. This is what makes handoff lossless: the
/// next provider starts from exactly these bytes.
pub fn checkpoint(wt: &Path, message: &str) -> Result<Option<String>> {
    git(wt, &["add", "-A"])?;
    if git(wt, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(None);
    }
    git_with_ident(wt, &["commit", "--quiet", "--no-verify", "-m", message])?;
    Ok(Some(head(wt)?))
}

pub fn changed_files(wt: &Path, from: &str, to: &str) -> Result<Vec<String>> {
    if from == to {
        return Ok(vec![]);
    }
    let out = git(wt, &["diff", "--name-only", "--no-renames", from, to])?;
    Ok(out.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect())
}

pub fn diff_stat(wt: &Path, from: &str, to: &str) -> Result<String> {
    if from == to {
        return Ok(String::new());
    }
    git(wt, &["diff", "--stat", from, to])
}

pub fn diff(wt: &Path, from: &str, to: &str, max_bytes: usize) -> Result<String> {
    let d = git(wt, &["diff", from, to])?;
    if d.len() <= max_bytes {
        return Ok(d);
    }
    let mut cut = max_bytes;
    while !d.is_char_boundary(cut) {
        cut -= 1;
    }
    Ok(format!("{}\n… [diff truncated: {} more bytes; inspect the worktree directly]", &d[..cut], d.len() - cut))
}

pub fn log_oneline(wt: &Path, from: &str, to: &str) -> Result<String> {
    if from == to {
        return Ok(String::new());
    }
    git(wt, &["log", "--oneline", "--no-decorate", &format!("{from}..{to}")])
}

pub fn reset_hard(wt: &Path, commit: &str) -> Result<()> {
    git(wt, &["reset", "--quiet", "--hard", commit])?;
    git(wt, &["clean", "-fdq"])?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeResult {
    Merged(String),
    UpToDate,
    Conflict(Vec<String>),
}

/// Merge `branch` into the branch checked out in `wt` (the integration worktree).
pub fn merge_into(wt: &Path, branch: &str, message: &str) -> Result<MergeResult> {
    let before = head(wt)?;
    match git_with_ident(wt, &["merge", "--no-ff", "--no-edit", "-m", message, branch]) {
        Ok(_) => {
            let after = head(wt)?;
            Ok(if after == before { MergeResult::UpToDate } else { MergeResult::Merged(after) })
        }
        Err(e) => {
            let conflicts = git(wt, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
            let files: Vec<String> = conflicts.lines().map(str::to_string).collect();
            let _ = git(wt, &["merge", "--abort"]);
            if files.is_empty() {
                Err(e)
            } else {
                Ok(MergeResult::Conflict(files))
            }
        }
    }
}

/// Compile scope patterns. A bare directory (`src/` or `src`) means everything under it.
pub fn scope_matcher(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim().replace('\\', "/");
        let p = p.trim_start_matches("./");
        let variants: Vec<String> = if p.ends_with('/') {
            vec![format!("{p}**")]
        } else if !p.contains(['*', '?', '[', '{']) {
            vec![p.to_string(), format!("{p}/**")]
        } else {
            vec![p.to_string()]
        };
        for v in variants {
            let g = GlobBuilder::new(&v)
                .literal_separator(true)
                .build()
                .map_err(|e| EngineError::Config(format!("invalid scope pattern `{v}`: {e}")))?;
            b.add(g);
        }
    }
    b.build().map_err(|e| EngineError::Config(e.to_string()))
}

pub fn scope_violations(patterns: &[String], changed: &[String]) -> Result<Vec<String>> {
    if patterns.is_empty() {
        return Ok(vec![]);
    }
    let set = scope_matcher(patterns)?;
    Ok(changed.iter().filter(|f| !set.is_match(f.replace('\\', "/"))).cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_patterns() {
        let owned = vec!["src/**".to_string(), "docs/".to_string(), "Cargo.toml".to_string(), "tests/*.rs".into()];
        let changed = vec![
            "src/a/b.rs".to_string(),
            "docs/x.md".into(),
            "Cargo.toml".into(),
            "tests/it.rs".into(),
            "tests/deep/it.rs".into(),
            "README.md".into(),
        ];
        assert_eq!(scope_violations(&owned, &changed).unwrap(), vec!["tests/deep/it.rs", "README.md"]);
        assert!(scope_violations(&[], &changed).unwrap().is_empty());
    }
}
