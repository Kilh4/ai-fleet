//! Provider adapters: how to invoke each AI CLI, how to read its output, and
//! how to tell "out of quota" apart from "the work failed".
//!
//! Every provider is plain data (`ProviderSpec`) loaded from
//! `.ai-fleet/engine/providers.json`, so new tools can be added without
//! recompiling.

use crate::error::{EngineError, Result};
use crate::model::{Effort, FailureKind, RunOutcome};
use crate::proc::ProcResult;
use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptVia {
    /// Prompt is written to the process's stdin (no length or quoting limits).
    Stdin,
    /// Prompt replaces the `{prompt}` placeholder in `args`.
    Arg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Text,
    /// `claude -p --output-format stream-json --verbose`
    ClaudeStreamJson,
    /// `codex exec --json`
    CodexJsonl,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelSpec {
    pub name: String,
    /// 1 = fast/cheap, 2 = balanced, 3 = strongest.
    pub tier: u8,
    #[serde(default = "default_max_effort")]
    pub max_effort: Effort,
}

fn default_max_effort() -> Effort {
    Effort::Max
}
fn default_concurrency() -> usize {
    2
}
fn default_cooldown() -> u64 {
    3600
}
fn default_true() -> bool {
    true
}

/// Environment variables that would switch a CLI from the user's personal
/// subscription to metered API billing. Always removed.
pub fn default_strip_env() -> Vec<String> {
    [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_GENAI_API_KEY",
        "GOOGLE_APPLICATION_CREDENTIALS",
        // Set inside a running Claude Code session; nested CLIs must not inherit it.
        "CLAUDECODE",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub id: String,
    pub display_name: String,
    /// Executable name (looked up on PATH, incl. `.cmd` shims on Windows) or absolute path.
    pub binary: String,
    /// Argument template. Placeholders: `{model}`, `{effort}`, `{prompt}`, `{worktree}`.
    pub args: Vec<String>,
    /// Extra arguments appended when resuming a previous session on the same provider (`{session_id}`).
    #[serde(default)]
    pub resume_args: Vec<String>,
    pub prompt_via: PromptVia,
    pub output: OutputFormat,
    pub models: Vec<ModelSpec>,
    /// Maps engine effort levels to the tool's own vocabulary (defaults to the level name).
    #[serde(default)]
    pub effort_values: BTreeMap<Effort, String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_strip_env")]
    pub strip_env: Vec<String>,
    /// Extra case-insensitive regexes that indicate quota exhaustion.
    #[serde(default)]
    pub quota_patterns: Vec<String>,
    /// Extra case-insensitive regexes that indicate missing authentication.
    #[serde(default)]
    pub auth_patterns: Vec<String>,
    #[serde(default = "default_concurrency")]
    pub max_concurrency: usize,
    /// Cooldown used when a quota error carries no reset time.
    #[serde(default = "default_cooldown")]
    pub default_cooldown_secs: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub notes: String,
}

impl ProviderSpec {
    /// A provider for any CLI that reads a prompt on stdin and prints text.
    pub fn generic(id: &str, binary: &str, args: &[&str], models: Vec<ModelSpec>) -> Self {
        ProviderSpec {
            id: id.into(),
            display_name: id.into(),
            binary: binary.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            resume_args: vec![],
            prompt_via: PromptVia::Stdin,
            output: OutputFormat::Text,
            models,
            effort_values: BTreeMap::new(),
            env: BTreeMap::new(),
            strip_env: default_strip_env(),
            quota_patterns: vec![],
            auth_patterns: vec![],
            max_concurrency: default_concurrency(),
            default_cooldown_secs: default_cooldown(),
            enabled: true,
            notes: String::new(),
        }
    }

    pub fn effort_value(&self, e: Effort) -> String {
        self.effort_values.get(&e).cloned().unwrap_or_else(|| e.as_str().to_string())
    }
}

fn model(name: &str, tier: u8, max_effort: Effort) -> ModelSpec {
    ModelSpec { name: name.into(), tier, max_effort }
}

/// Built-in definitions. Written to `providers.json` on first use; edit that
/// file to change models, flags, or add new tools.
pub fn default_providers() -> Vec<ProviderSpec> {
    let mut claude = ProviderSpec::generic(
        "claude",
        "claude",
        &[
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--model",
            "{model}",
            "--effort",
            "{effort}",
            "--permission-mode",
            "acceptEdits",
        ],
        vec![model("opus", 3, Effort::Max), model("sonnet", 2, Effort::Max), model("haiku", 1, Effort::High)],
    );
    claude.display_name = "Claude Code".into();
    claude.output = OutputFormat::ClaudeStreamJson;
    claude.resume_args = vec!["--resume".into(), "{session_id}".into()];
    claude.notes = "Flags verified against `claude --help` (2.x). `acceptEdits` lets Claude edit files but not run \
        arbitrary shell commands; switch to `bypassPermissions` only if you accept that risk inside the worktree."
        .into();

    let mut codex = ProviderSpec::generic(
        "codex",
        "codex",
        &[
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--sandbox",
            "workspace-write",
            "-m",
            "{model}",
            "-c",
            "model_reasoning_effort={effort}",
            "-",
        ],
        vec![model("gpt-6.2-astra", 3, Effort::Max), model("soul", 2, Effort::Max)],
    );
    codex.display_name = "OpenAI Codex".into();
    codex.output = OutputFormat::CodexJsonl;
    codex.effort_values.insert(Effort::Max, "xhigh".into());
    codex.notes = "Codex CLI was not found on this machine when this file was generated; verify flags with \
        `codex exec --help` and model names with your plan."
        .into();

    let mut agy = ProviderSpec::generic(
        "antigravity",
        "agy",
        &["-p", "{prompt}", "--model", "{model}"],
        vec![model("argon", 3, Effort::Max), model("gemini-4-neon", 2, Effort::Max)],
    );
    agy.display_name = "Google Antigravity CLI".into();
    agy.prompt_via = PromptVia::Arg;
    agy.notes = "UNVERIFIED flags: `agy` was not installed when this file was generated. Run `agy --help` and \
        fix `args` (non-interactive prompt flag, model flag) before relying on this provider."
        .into();

    vec![claude, codex, agy]
}

#[derive(Serialize, Deserialize)]
struct ProvidersFile {
    version: u32,
    #[serde(default)]
    comment: String,
    providers: Vec<ProviderSpec>,
}

pub fn load_or_init(path: &Path) -> Result<Vec<ProviderSpec>> {
    if path.is_file() {
        let txt = std::fs::read_to_string(path)?;
        let file: ProvidersFile =
            serde_json::from_str(&txt).map_err(|e| EngineError::Config(format!("{}: {e}", path.display())))?;
        validate(&file.providers)?;
        return Ok(file.providers);
    }
    let providers = default_providers();
    write_providers(path, &providers)?;
    Ok(providers)
}

pub fn write_providers(path: &Path, providers: &[ProviderSpec]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = ProvidersFile {
        version: 1,
        comment: "AI-Fleet provider definitions. Placeholders in args: {model} {effort} {prompt} {worktree}. \
            API-key variables in strip_env are always removed so tools use your subscriptions."
            .into(),
        providers: providers.to_vec(),
    };
    std::fs::write(path, serde_json::to_string_pretty(&file)?)?;
    Ok(())
}

pub fn validate(providers: &[ProviderSpec]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for p in providers {
        if !seen.insert(p.id.as_str()) {
            return Err(EngineError::Config(format!("duplicate provider id `{}`", p.id)));
        }
        if p.models.is_empty() {
            return Err(EngineError::Config(format!("provider `{}` has no models", p.id)));
        }
        if p.prompt_via == PromptVia::Arg && !p.args.iter().any(|a| a.contains("{prompt}")) {
            return Err(EngineError::Config(format!("provider `{}` uses prompt_via=arg but no {{prompt}} placeholder", p.id)));
        }
        for pat in p.quota_patterns.iter().chain(&p.auth_patterns) {
            Regex::new(&format!("(?i){pat}"))
                .map_err(|e| EngineError::Config(format!("provider `{}` pattern `{pat}`: {e}", p.id)))?;
        }
    }
    Ok(())
}

/// Locate an executable the way a shell would, including Windows `.cmd`/`.bat` shims.
pub fn resolve_binary(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().count() > 1 || p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut v: Vec<String> = pathext.split(';').filter(|e| !e.is_empty()).map(|e| e.to_ascii_lowercase()).collect();
        if p.extension().is_some() {
            v.insert(0, String::new());
        }
        v
    } else {
        vec![String::new()]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[derive(Debug, Clone)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub env: Vec<(String, String)>,
}

/// Environment for a child process: current env minus billing keys, plus overrides.
pub fn child_env(strip: &[String], extra: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let strip: Vec<String> = strip.iter().map(|s| s.to_ascii_uppercase()).collect();
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| !strip.contains(&k.to_ascii_uppercase()))
        .filter(|(k, _)| !extra.contains_key(k))
        .collect();
    env.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    env
}

pub fn build_invocation(
    spec: &ProviderSpec,
    program: &Path,
    model: &str,
    effort: Effort,
    prompt: &str,
    worktree: &Path,
) -> Invocation {
    build_invocation_with_session(spec, program, model, effort, prompt, worktree, None)
}

pub fn build_invocation_with_session(
    spec: &ProviderSpec,
    program: &Path,
    model: &str,
    effort: Effort,
    prompt: &str,
    worktree: &Path,
    resume_session: Option<&str>,
) -> Invocation {
    let effort_value = spec.effort_value(effort);
    let wt = worktree.to_string_lossy();
    let mut args: Vec<String> = spec
        .args
        .iter()
        .map(|a| {
            let a = a.replace("{model}", model).replace("{effort}", &effort_value).replace("{worktree}", &wt);
            if spec.prompt_via == PromptVia::Arg {
                a.replace("{prompt}", prompt)
            } else {
                a
            }
        })
        .collect();
    if let Some(sid) = resume_session.filter(|s| !s.trim().is_empty()) {
        if !spec.resume_args.is_empty() {
            for ra in &spec.resume_args {
                args.push(ra.replace("{session_id}", sid));
            }
        }
    }
    Invocation {
        program: program.to_path_buf(),
        args,
        stdin: (spec.prompt_via == PromptVia::Stdin).then(|| prompt.to_string()),
        env: child_env(&spec.strip_env, &spec.env),
    }
}

// ------------------------------------------------------------------ parsing

#[derive(Debug, Clone, Default)]
pub struct ParsedOutput {
    /// The agent's final answer / summary.
    pub final_text: String,
    pub session_id: Option<String>,
    /// Error message reported in-band by the tool.
    pub reported_error: Option<String>,
    /// The tool's structured output marked the run as failed.
    pub is_error: bool,
    pub json_events: u64,
}

pub struct StreamParser {
    format: OutputFormat,
    out: ParsedOutput,
    last_assistant_text: String,
}

impl StreamParser {
    pub fn new(format: OutputFormat) -> Self {
        StreamParser { format, out: ParsedOutput::default(), last_assistant_text: String::new() }
    }

    pub fn feed(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || self.format == OutputFormat::Text {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        self.out.json_events += 1;
        match self.format {
            OutputFormat::ClaudeStreamJson => self.feed_claude(&v),
            OutputFormat::CodexJsonl => self.feed_codex(&v),
            OutputFormat::Text => {}
        }
    }

    fn feed_claude(&mut self, v: &Value) {
        if let Some(sid) = v.get("session_id").and_then(Value::as_str) {
            self.out.session_id = Some(sid.to_string());
        }
        match v.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(content) = v.pointer("/message/content").and_then(Value::as_array) {
                    let text: Vec<&str> = content
                        .iter()
                        .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|c| c.get("text").and_then(Value::as_str))
                        .collect();
                    if !text.is_empty() {
                        self.last_assistant_text = text.join("\n");
                    }
                }
            }
            Some("result") => {
                if let Some(r) = v.get("result").and_then(Value::as_str) {
                    self.out.final_text = r.to_string();
                }
                let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false)
                    || v.get("subtype").and_then(Value::as_str).is_some_and(|s| s.starts_with("error"));
                if is_error {
                    self.out.is_error = true;
                    let msg = v
                        .get("result")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| v.get("subtype").and_then(Value::as_str).map(str::to_string))
                        .unwrap_or_else(|| "claude reported an error".into());
                    self.out.reported_error = Some(msg);
                }
            }
            _ => {}
        }
    }

    fn feed_codex(&mut self, v: &Value) {
        let ty = v.get("type").and_then(Value::as_str);
        match ty {
            Some("thread.started") => {
                if let Some(id) = v.get("thread_id").and_then(Value::as_str) {
                    self.out.session_id = Some(id.to_string());
                }
            }
            Some("item.completed") => {
                let item = v.get("item");
                let item_type = item.and_then(|i| i.get("type").or_else(|| i.get("item_type"))).and_then(Value::as_str);
                if matches!(item_type, Some("agent_message") | Some("assistant_message")) {
                    if let Some(t) = item.and_then(|i| i.get("text")).and_then(Value::as_str) {
                        self.out.final_text = t.to_string();
                    }
                }
            }
            Some("error") => {
                self.out.is_error = true;
                self.out.reported_error =
                    v.get("message").and_then(Value::as_str).map(str::to_string).or(Some(v.to_string()));
            }
            Some("turn.failed") => {
                self.out.is_error = true;
                self.out.reported_error = v
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(Some(v.to_string()));
            }
            _ => {
                // Older codex versions: {"id":..,"msg":{"type":"agent_message","message":".."}}
                if let Some(msg) = v.get("msg") {
                    match msg.get("type").and_then(Value::as_str) {
                        Some("agent_message") => {
                            if let Some(t) = msg.get("message").and_then(Value::as_str) {
                                self.out.final_text = t.to_string();
                            }
                        }
                        Some("error") | Some("stream_error") => {
                            self.out.is_error = true;
                            self.out.reported_error = msg.get("message").and_then(Value::as_str).map(str::to_string);
                        }
                        Some("session_configured") => {
                            if let Some(id) = msg.get("session_id").and_then(Value::as_str) {
                                self.out.session_id = Some(id.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn finish(mut self, proc: &ProcResult) -> ParsedOutput {
        if self.out.final_text.is_empty() {
            self.out.final_text = if !self.last_assistant_text.is_empty() {
                std::mem::take(&mut self.last_assistant_text)
            } else {
                crate::store::truncate(proc.stdout_tail.trim(), 16_000)
            };
        }
        self.out
    }
}

// ----------------------------------------------------------- classification

const QUOTA_PATTERNS: &[&str] = &[
    r"usage limit",
    r"rate[ _-]?limit",
    r"hit (your|the) (usage )?limit",
    r"limit (has been |was )?reached",
    r"quota",
    r"too many requests",
    r"\b429\b",
    r"resource[ _-]?exhausted",
    r"out of (credits|usage|messages)",
    r"insufficient[ _]credits",
    r"overloaded",
];

const AUTH_PATTERNS: &[&str] = &[
    r"not logged in",
    r"please (log ?in|login|sign in|run [^\n]*login)",
    r"unauthori[sz]ed",
    r"\b401\b",
    r"authentication (failed|required|error)",
    r"invalid (api key|credentials|token)",
    r"(token|session|credentials?) (has )?expired",
];

fn compile(patterns: &[&str], extra: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|p| p.to_string())
        .chain(extra.iter().cloned())
        .filter_map(|p| Regex::new(&format!("(?i){p}")).ok())
        .collect()
}

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("valid regex"))
}

/// Extract when a quota resets from a tool's error text.
pub fn parse_reset_time(text: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    static EPOCH: OnceLock<Regex> = OnceLock::new();
    static RELATIVE: OnceLock<Regex> = OnceLock::new();
    static RETRY_AFTER: OnceLock<Regex> = OnceLock::new();
    static CLOCK: OnceLock<Regex> = OnceLock::new();

    // Claude Code legacy format: "Claude AI usage limit reached|1764100800"
    if let Some(c) = re(&EPOCH, r"(?i)limit reached\|(\d{10})\b").captures(text) {
        if let Some(t) = c[1].parse::<i64>().ok().and_then(|s| Utc.timestamp_opt(s, 0).single()) {
            if t > now {
                return Some(t);
            }
        }
    }

    // "try again in 2h 5m", "resets in 45 minutes", "retry in 30 seconds"
    let rel = re(
        &RELATIVE,
        r"(?i)(?:try again|retry|resets?|available again|wait)\s+(?:in|after)\s+(?:(\d+)\s*(?:h|hr|hrs|hours?)\b)?\s*(?:(\d+)\s*(?:m|min|mins|minutes?)\b)?\s*(?:(\d+)\s*(?:s|sec|secs|seconds?)\b)?",
    );
    for c in rel.captures_iter(text) {
        let num = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok());
        let (h, m, s) = (num(1), num(2), num(3));
        if h.is_none() && m.is_none() && s.is_none() {
            continue;
        }
        let secs = h.unwrap_or(0) * 3600 + m.unwrap_or(0) * 60 + s.unwrap_or(0);
        return Some(now + Duration::seconds(secs.max(1)));
    }

    if let Some(c) = re(&RETRY_AFTER, r"(?i)retry[- ]after[\s:=]+(\d+)").captures(text) {
        if let Ok(secs) = c[1].parse::<i64>() {
            return Some(now + Duration::seconds(secs.max(1)));
        }
    }

    // "resets 3pm", "resets at 15:30", "limit will reset at 9:00 am"
    if let Some(c) = re(&CLOCK, r"(?i)resets?\s+(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)?").captures(text) {
        let has_minutes = c.get(2).is_some();
        let meridiem = c.get(3).map(|m| m.as_str().to_ascii_lowercase());
        if has_minutes || meridiem.is_some() {
            let mut hour: u32 = c[1].parse().ok()?;
            let minute: u32 = c.get(2).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
            match meridiem.as_deref() {
                Some("pm") if hour < 12 => hour += 12,
                Some("am") if hour == 12 => hour = 0,
                _ => {}
            }
            let local_now = now.with_timezone(&Local);
            let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
            let mut candidate = local_now.date_naive().and_time(time);
            if candidate <= local_now.naive_local() {
                candidate += Duration::days(1);
            }
            return Local.from_local_datetime(&candidate).earliest().map(|d| d.with_timezone(&Utc));
        }
    }
    None
}

/// Decide what a finished process means.
pub fn classify(spec: &ProviderSpec, proc: &ProcResult, parsed: &ParsedOutput, now: DateTime<Utc>) -> RunOutcome {
    if proc.cancelled {
        return RunOutcome::failure(FailureKind::Cancelled, "engine shutdown requested");
    }
    if proc.timed_out {
        return RunOutcome::failure(FailureKind::Timeout, "exceeded the run time limit; process tree terminated");
    }
    if proc.stalled {
        return RunOutcome::failure(FailureKind::Stalled, "no output within the idle limit; process tree terminated");
    }

    let succeeded = proc.exit_code == Some(0) && !parsed.is_error;
    if succeeded {
        return RunOutcome::Success;
    }

    let stdout_tail = crate::store::truncate(&proc.stdout_tail, 6000);
    let stderr_tail = crate::store::truncate(&proc.stderr_tail, 6000);
    let haystack = format!(
        "{}\n{}\n{}\n{}",
        parsed.reported_error.as_deref().unwrap_or(""),
        if parsed.is_error { parsed.final_text.as_str() } else { "" },
        stderr_tail,
        stdout_tail
    );
    let detail = {
        let d = parsed
            .reported_error
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| if stderr_tail.trim().is_empty() { stdout_tail.clone() } else { stderr_tail.clone() });
        crate::store::truncate(d.trim(), 4000)
    };

    if compile(QUOTA_PATTERNS, &spec.quota_patterns).iter().any(|r| r.is_match(&haystack)) {
        let retry_at = parse_reset_time(&haystack, now)
            .unwrap_or_else(|| now + Duration::seconds(spec.default_cooldown_secs as i64));
        return RunOutcome::Failure { kind: FailureKind::Quota, detail, retry_at: Some(retry_at) };
    }
    if compile(AUTH_PATTERNS, &spec.auth_patterns).iter().any(|r| r.is_match(&haystack)) {
        return RunOutcome::Failure { kind: FailureKind::Auth, detail, retry_at: Some(now + Duration::hours(6)) };
    }
    if parsed.is_error {
        return RunOutcome::Failure { kind: FailureKind::AgentError, detail, retry_at: None };
    }
    let code = proc.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
    RunOutcome::Failure { kind: FailureKind::Crash, detail: format!("exit code {code}: {detail}"), retry_at: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(code: i32, stdout: &str, stderr: &str) -> ProcResult {
        ProcResult {
            exit_code: Some(code),
            stdout_tail: stdout.into(),
            stderr_tail: stderr.into(),
            ..Default::default()
        }
    }

    #[test]
    fn relative_reset_times() {
        let now = Utc::now();
        let t = parse_reset_time("You've hit your limit · try again in 2h 5m", now).unwrap();
        assert_eq!((t - now).num_minutes(), 125);
        let t = parse_reset_time("Rate limit exceeded. Please try again in 30 seconds.", now).unwrap();
        assert_eq!((t - now).num_seconds(), 30);
        let t = parse_reset_time("HTTP 429\nRetry-After: 90", now).unwrap();
        assert_eq!((t - now).num_seconds(), 90);
    }

    #[test]
    fn epoch_and_clock_reset_times() {
        let now = Utc::now();
        let future = now.timestamp() + 3600;
        let t = parse_reset_time(&format!("Claude AI usage limit reached|{future}"), now).unwrap();
        assert_eq!(t.timestamp(), future);

        let t = parse_reset_time("5-hour limit reached ∙ resets 3pm", now).unwrap();
        assert!(t > now && t - now <= Duration::hours(24));
        assert_eq!(t.with_timezone(&Local).format("%H:%M").to_string(), "15:00");
        assert!(parse_reset_time("resets 5 things", now).is_none());
    }

    #[test]
    fn classifies_quota_auth_and_crash() {
        let spec = ProviderSpec::generic("x", "x", &[], vec![model("m", 2, Effort::Max)]);
        let now = Utc::now();
        let q = classify(&spec, &proc(1, "", "Claude AI usage limit reached. try again in 1h"), &ParsedOutput::default(), now);
        match q {
            RunOutcome::Failure { kind: FailureKind::Quota, retry_at: Some(t), .. } => {
                assert_eq!((t - now).num_minutes(), 60)
            }
            other => panic!("expected quota, got {other:?}"),
        }
        let a = classify(&spec, &proc(1, "", "Error: Not logged in. Please run /login"), &ParsedOutput::default(), now);
        assert_eq!(a.kind(), Some(FailureKind::Auth));
        let c = classify(&spec, &proc(2, "", "segfault"), &ParsedOutput::default(), now);
        assert_eq!(c.kind(), Some(FailureKind::Crash));
        assert!(classify(&spec, &proc(0, "done", ""), &ParsedOutput::default(), now).is_success());
    }

    #[test]
    fn parses_claude_stream_json() {
        let mut p = StreamParser::new(OutputFormat::ClaudeStreamJson);
        p.feed(r#"{"type":"system","subtype":"init","session_id":"abc-123"}"#);
        p.feed(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Working on it"}]}}"#);
        p.feed(r#"{"type":"result","subtype":"success","is_error":false,"result":"Added the endpoint and tests.","session_id":"abc-123"}"#);
        let out = p.finish(&ProcResult::default());
        assert_eq!(out.final_text, "Added the endpoint and tests.");
        assert_eq!(out.session_id.as_deref(), Some("abc-123"));
        assert!(!out.is_error);

        let mut p = StreamParser::new(OutputFormat::ClaudeStreamJson);
        p.feed(r#"{"type":"result","subtype":"success","is_error":true,"result":"Claude AI usage limit reached|1999999999"}"#);
        let out = p.finish(&ProcResult::default());
        assert!(out.is_error);
        let spec = default_providers().remove(0);
        let r = classify(&spec, &proc(1, "", ""), &out, Utc::now());
        assert_eq!(r.kind(), Some(FailureKind::Quota));
    }

    #[test]
    fn parses_codex_jsonl() {
        let mut p = StreamParser::new(OutputFormat::CodexJsonl);
        p.feed(r#"{"type":"thread.started","thread_id":"th_1"}"#);
        p.feed(r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"Refactored module."}}"#);
        p.feed(r#"{"type":"turn.completed","usage":{"input_tokens":10}}"#);
        let out = p.finish(&ProcResult::default());
        assert_eq!(out.final_text, "Refactored module.");
        assert_eq!(out.session_id.as_deref(), Some("th_1"));

        let mut p = StreamParser::new(OutputFormat::CodexJsonl);
        p.feed(r#"{"type":"turn.failed","error":{"message":"You've hit your usage limit. Try again in 3 hours."}}"#);
        let out = p.finish(&ProcResult::default());
        let spec = default_providers().remove(1);
        let r = classify(&spec, &proc(1, "", ""), &out, Utc::now());
        assert_eq!(r.kind(), Some(FailureKind::Quota));
    }

    #[test]
    fn invocation_strips_billing_keys_and_fills_template() {
        std::env::set_var("ANTHROPIC_API_KEY", "sk-should-not-leak");
        let spec = default_providers().remove(0);
        let inv = build_invocation(&spec, Path::new("claude"), "opus", Effort::Xhigh, "do it", Path::new("/w"));
        assert!(inv.env.iter().all(|(k, _)| k != "ANTHROPIC_API_KEY"));
        assert!(inv.args.windows(2).any(|w| w[0] == "--model" && w[1] == "opus"));
        assert!(inv.args.windows(2).any(|w| w[0] == "--effort" && w[1] == "xhigh"));
        assert_eq!(inv.stdin.as_deref(), Some("do it"));

        let inv_res = build_invocation_with_session(&spec, Path::new("claude"), "opus", Effort::High, "continue", Path::new("/w"), Some("sess-99"));
        assert!(inv_res.args.windows(2).any(|w| w[0] == "--resume" && w[1] == "sess-99"));

        let codex = default_providers().remove(1);
        let inv = build_invocation(&codex, Path::new("codex"), "soul", Effort::Max, "x", Path::new("/w"));
        assert!(inv.args.contains(&"model_reasoning_effort=xhigh".to_string()));
    }

    #[test]
    fn defaults_validate() {
        validate(&default_providers()).unwrap();
    }
}
