//! `fleet-analyzer` — Real Git Repository & Codebase Risk Analyzer (`ai-fleet analyze`)
//! and System Health Doctor (`ai-fleet doctor [--fix]`).

use fleet_security::scan_aidefence_threats;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoAnalysisReport {
    pub repo_root: String,
    pub branch: String,
    pub head_commit: String,
    pub modified_files: usize,
    pub untracked_files: usize,
    pub domain_breakdown: BTreeMap<String, usize>,
    pub risk_level: String, // "LOW", "MEDIUM", "HIGH", "CRITICAL"
    pub risk_reasons: Vec<String>,
    pub recommended_effort: String,
    pub secret_findings: usize,
}

pub fn analyze_git_repository(repo_root: &Path) -> RepoAnalysisReport {
    let branch = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "detached/main".into());

    let head = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "HEAD".into());

    let status_out = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(repo_root)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let mut modified = 0usize;
    let mut untracked = 0usize;
    let mut domains: BTreeMap<String, usize> = BTreeMap::new();
    let mut reasons = Vec::new();
    let mut critical_hit = false;
    let mut deep_hit = false;
    let mut secret_hits = 0usize;

    for line in status_out.lines() {
        if line.len() < 4 {
            continue;
        }
        let code = &line[..2];
        let path = line[3..].trim();
        if code.contains('?') {
            untracked += 1;
        } else {
            modified += 1;
        }

        let dom = if path.starts_with("database/") || path.ends_with(".sql") {
            critical_hit = true;
            reasons.push(format!("SQL/Migration file touched: {}", path));
            "database-sql"
        } else if path.contains("Money") || path.contains("Audit") || path.contains("Access") || path.contains("Scram") || path.contains("security") {
            critical_hit = true;
            reasons.push(format!("Critical security/financial path touched: {}", path));
            "security-finance"
        } else if path.ends_with(".rs") || path.ends_with(".cs") || path.ends_with(".ts") {
            deep_hit = true;
            "source-code"
        } else if path.starts_with("golden/") || path.contains("test") {
            deep_hit = true;
            "testing-golden"
        } else if path.starts_with("deploy/") || path.contains("docker") || path.contains("ci.yml") {
            critical_hit = true;
            "devops-deploy"
        } else {
            "docs-config"
        };

        *domains.entry(dom.to_string()).or_insert(0) += 1;
        secret_hits += scan_aidefence_threats(path).len();
    }

    let total_changed = modified + untracked;
    if total_changed > 50 {
        reasons.push(format!("Large uncommitted working tree ({} files); isolate slices in worktrees", total_changed));
    }

    let (risk_level, effort) = if critical_hit || secret_hits > 0 {
        ("CRITICAL".to_string(), "xhigh".to_string())
    } else if deep_hit || total_changed > 15 {
        ("HIGH".to_string(), "high".to_string())
    } else if total_changed > 0 {
        ("MEDIUM".to_string(), "medium".to_string())
    } else {
        ("LOW".to_string(), "medium".to_string())
    };

    if reasons.is_empty() {
        reasons.push("Working tree is clean or contains only standard changes".into());
    }
    reasons.truncate(5);

    RepoAnalysisReport {
        repo_root: repo_root.to_string_lossy().to_string(),
        branch,
        head_commit: head,
        modified_files: modified,
        untracked_files: untracked,
        domain_breakdown: domains,
        risk_level,
        risk_reasons: reasons,
        recommended_effort: effort,
        secret_findings: secret_hits,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub component: String,
    pub healthy: bool,
    pub detail: String,
}

pub fn run_system_doctor() -> Vec<DoctorCheck> {
    let probes = [
        ("Git VCS", "git", &["--version"][..]),
        ("Rust Compiler (rustc)", "rustc", &["--version"][..]),
        ("Cargo Package Manager", "cargo", &["--version"][..]),
        ("Claude Code CLI", "claude", &["--version"][..]),
        ("Node.js Runtime", "node", &["--version"][..]),
        ("Docker Engine CLI", "docker", &["--version"][..]),
        (".NET SDK", "dotnet", &["--version"][..]),
    ];

    let mut results = Vec::new();
    for (name, bin, args) in probes {
        let out = Command::new(bin).args(args).output();
        match out {
            Ok(o) if o.status.success() => {
                let ver = String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("installed").trim().to_string();
                results.push(DoctorCheck {
                    component: name.to_string(),
                    healthy: true,
                    detail: ver,
                });
            }
            _ => {
                results.push(DoctorCheck {
                    component: name.to_string(),
                    healthy: false,
                    detail: format!("'{}' not on PATH or requires alias", bin),
                });
            }
        }
    }
    results
}
