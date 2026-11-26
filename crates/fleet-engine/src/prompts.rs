//! Prompt construction for authors and reviewers, including the handoff
//! context that lets a fresh provider continue another provider's work.

use crate::model::Task;
use crate::store::truncate;

pub struct AuthorContext<'a> {
    pub branch: &'a str,
    /// `git log --oneline base..HEAD` of work already on the branch.
    pub prior_commits: &'a str,
    pub prior_diffstat: &'a str,
    pub graph_section: &'a str,
    pub memory_section: &'a str,
    pub skills_section: &'a str,
}

pub fn author_prompt(task: &Task, ctx: &AuthorContext<'_>) -> String {
    let mut p = String::new();
    p.push_str(&format!("# Task `{}`: {}\n\n{}\n\n", task.id, task.title, task.prompt.trim()));

    p.push_str("## Working rules\n");
    p.push_str(&format!(
        "- Your current directory is a dedicated git worktree on branch `{}`. Make every change here.\n",
        ctx.branch
    ));
    if task.owned_files.is_empty() {
        p.push_str("- You may modify any file in this repository that the task requires.\n");
    } else {
        p.push_str(&format!(
            "- You may ONLY create or modify files matching: {}. Changes elsewhere are rejected and rolled back.\n",
            task.owned_files.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    p.push_str("- Do not push, switch branches, or rewrite history. AI-Fleet commits your changes automatically.\n");
    if let Some(cmd) = &task.verify_cmd {
        p.push_str(&format!("- Your work is accepted only if this command exits 0: `{cmd}`. Run it yourself before finishing.\n"));
    }
    if task.review_required {
        p.push_str("- A different AI tool will independently review your diff before it is merged.\n");
    }
    p.push_str("- Finish with a short summary: what you changed, why, and anything left undone.\n");

    if !ctx.prior_commits.trim().is_empty() {
        p.push_str(&format!(
            "\n## Work already on this branch\nEarlier runs produced these commits (oldest last):\n```\n{}\n```\n```\n{}\n```\nContinue from the current state of the files. Do not redo work that is already complete.\n",
            truncate(ctx.prior_commits.trim(), 4000),
            truncate(ctx.prior_diffstat.trim(), 4000)
        ));
    }

    if let Some(kind) = task.last_failure.as_deref() {
        let who = match (&task.last_provider, &task.last_model) {
            (Some(p), Some(m)) => format!("{p}/{m}"),
            (Some(p), None) => p.clone(),
            _ => "the previous agent".into(),
        };
        let err = truncate(task.last_error.as_deref().unwrap_or("").trim(), 3500);
        let headline = match kind {
            "quota" => format!("The previous agent ({who}) ran out of subscription quota mid-task. Its partial work is committed on this branch; pick up exactly where it stopped."),
            "auth" => format!("The previous agent ({who}) could not authenticate. Its partial work (if any) is committed; continue it."),
            "unavailable" => format!("The previous agent ({who}) could not be started. Start or continue the task."),
            "timeout" | "stalled" => format!("The previous run ({who}) was stopped because it {}. Partial work is committed; finish it efficiently.", if kind == "timeout" { "exceeded the time limit" } else { "stopped producing output" }),
            "verify_failed" => format!("The previous attempt ({who}) did not pass verification. Fix the failures below."),
            "static_audit_failed" => format!("The previous attempt ({who}) failed the deterministic pre-merge security / money / RLS static gate. Fix every finding below."),
            "scope_violation" => format!("The previous attempt ({who}) modified files outside the allowed scope, so it was rolled back. Stay within scope."),
            "merge_conflict" => "This branch conflicts with the integration branch. Merge it in (command below), resolve every conflict preserving both intents, and re-verify.".to_string(),
            "review_rejected" => "An independent reviewer requested changes (see below).".to_string(),
            _ => format!("The previous attempt ({who}) failed ({kind})."),
        };
        p.push_str(&format!("\n## Why the previous attempt stopped\n{headline}\n"));
        if !err.is_empty() && kind != "review_rejected" {
            p.push_str(&format!("```\n{err}\n```\n"));
        }
    }

    if let Some(fb) = task.review_feedback.as_deref() {
        p.push_str(&format!("\n## Reviewer feedback you must address\n{}\n", truncate(fb.trim(), 6000)));
    }
    if !ctx.graph_section.trim().is_empty() {
        p.push_str(ctx.graph_section);
    }
    if !ctx.memory_section.trim().is_empty() {
        p.push_str(ctx.memory_section);
    }
    if !ctx.skills_section.trim().is_empty() {
        p.push_str(ctx.skills_section);
    }
    p
}

pub fn review_prompt(
    task: &Task,
    diffstat: &str,
    diff: &str,
    author_summary: &str,
    memory_section: &str,
    skills_section: &str,
) -> String {
    format!(
        "# Independent review of task `{id}`: {title}\n\n\
You are reviewing a change written by a different AI tool ({authors}). You are the last gate before it is merged.\n\
Do NOT modify any files; any modification is discarded.\n\n\
## Original task\n{prompt}\n\n\
## Author's summary\n{summary}\n\n\
## Scope\n{scope}\n\n\
## Verification\n{verify}\n\n\
## Diff stat\n```\n{stat}\n```\n\n\
## Diff\n```diff\n{diff}\n```\n{memory}{skills}\n\
## What to check\n\
1. Does the change fully and correctly accomplish the task?\n\
2. Bugs, edge cases, error handling, concurrency, security (injection, secrets, authz).\n\
3. Tests: are the important behaviours covered?\n\
4. Anything outside the task's scope or unnecessary.\n\n\
## Required answer format\n\
Explain your findings briefly. If changes are needed, list each one as a numbered, actionable item.\n\
The LAST line of your answer must be exactly one of:\n\
VERDICT: APPROVE\n\
VERDICT: CHANGES_REQUESTED\n",
        id = task.id,
        title = task.title,
        authors = if task.authors.is_empty() { "unknown".into() } else { task.authors.join(", ") },
        prompt = truncate(task.prompt.trim(), 8000),
        summary = truncate(author_summary.trim(), 4000),
        scope = if task.owned_files.is_empty() { "Unrestricted.".to_string() } else { task.owned_files.join(", ") },
        verify = task.verify_cmd.as_deref().map(|c| format!("`{c}` already passed.")).unwrap_or_else(|| "No verification command configured.".into()),
        stat = diffstat.trim(),
        diff = diff,
        memory = memory_section,
        skills = skills_section,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    ChangesRequested,
}

/// Find the reviewer's verdict. The last verdict line wins.
pub fn parse_verdict(text: &str) -> Option<Verdict> {
    let re = regex::Regex::new(r"(?im)^[\s>*_`#-]*verdict[\s*_`]*:[\s*_`]*(approved?|changes[_ ]requested|changes|request[_ ]changes|reject(?:ed)?)\b")
        .expect("valid regex");
    re.captures_iter(text).last().map(|c| {
        if c[1].to_ascii_lowercase().starts_with("approve") {
            Verdict::Approve
        } else {
            Verdict::ChangesRequested
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_parsing() {
        assert_eq!(parse_verdict("ok\nVERDICT: APPROVE"), Some(Verdict::Approve));
        assert_eq!(parse_verdict("**Verdict:** CHANGES_REQUESTED"), Some(Verdict::ChangesRequested));
        assert_eq!(parse_verdict("VERDICT: APPROVE\n...\nVERDICT: CHANGES_REQUESTED"), Some(Verdict::ChangesRequested));
        assert_eq!(parse_verdict("I approve of this"), None);
    }
}
