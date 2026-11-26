//! Real Model Context Protocol (MCP) JSON-RPC 2.0 Server over `stdio`, wired
//! directly to `fleet-engine` (`SQLite`, `120 Elite Skills`, `Hybrid FTS5+Vector
//! Memory`, `CodeGraph PageRank/SCC`, `Static Security/Money/RLS Auditor`, and
//! `Queen Epic DAG Decomposer`).
//!
//! Supports both newline-delimited JSON-RPC (`MCP 2024-11-05` / `2025-03-26`)
//! and `Content-Length` framed JSON-RPC so Claude Code, Codex, Cursor, and
//! custom MCP clients connect seamlessly.

use crate::engine::{Engine, EngineConfig};
use crate::git;
use crate::intelligence::{audit_file_list, CodeGraph};
use crate::memory::{MemoryKind, MemoryStore, NewMemory};
use crate::model::{Complexity, NewTask};
use crate::queen;
use crate::router::Role;
use crate::skills::SkillRegistry;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::Path;

pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "fleet_status",
            "description": "Return real-time status of AI-Fleet engine: queued/running/done tasks, provider health & cooldowns, memory count, and bundled skills count.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "fleet_task_submit",
            "description": "Submit a real task into the persistent SQLite execution queue (`engine.db`) with optional file-scope boundaries, verification command, and pinned skills.",
            "inputSchema": {
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "title": { "type": "string" },
                    "prompt": { "type": "string" },
                    "owned_files": { "type": "array", "items": { "type": "string" } },
                    "verify_cmd": { "type": "string" },
                    "complexity": { "type": "string", "enum": ["trivial", "standard", "deep", "critical"] },
                    "skills": { "type": "array", "items": { "type": "string" } },
                    "review": { "type": "boolean" }
                }
            }
        },
        {
            "name": "fleet_task_list",
            "description": "List all tasks in the persistent SQLite queue with status, complexity, attempts, handoffs, authors, and reviewer.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "fleet_task_show",
            "description": "Inspect one task in detail along with every historical run, provider/model/effort routing reason, and log path.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" } }
            }
        },
        {
            "name": "fleet_queen_decompose",
            "description": "Decompose a complex epic into a validated, cycle-free, file-scope-disjoint multi-wave DAG of tasks and optionally queue them into SQLite.",
            "inputSchema": {
                "type": "object",
                "required": ["epic"],
                "properties": {
                    "epic": { "type": "string" },
                    "submit": { "type": "boolean", "description": "If true, enqueue all planned DAG waves into engine.db" },
                    "verify_cmd": { "type": "string" }
                }
            }
        },
        {
            "name": "fleet_skills_match",
            "description": "Match the top Elite GitHub Skills (120+ skills across software, architecture, accounting/ERP, auditing/RLS, UI/UX, math, physics, economics, languages) using Okapi BM25 + bilingual Arabic/English expansion.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "owned_files": { "type": "array", "items": { "type": "string" } },
                    "top": { "type": "integer", "default": 5 },
                    "reviewer": { "type": "boolean", "default": false }
                }
            }
        },
        {
            "name": "fleet_skills_get",
            "description": "Fetch full SKILL.md instructions and provenance metadata for a specific skill ID.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" } }
            }
        },
        {
            "name": "fleet_skills_list",
            "description": "List available Elite Skills, optionally filtered by domain (`accounting-erp`, `auditing-security`, `mathematics`, `physics-simulation`, `economics-quant`, `architecture`, `design-ui-ux`, `languages`, etc.).",
            "inputSchema": {
                "type": "object",
                "properties": { "domain": { "type": "string" } }
            }
        },
        {
            "name": "fleet_memory_store",
            "description": "Persist an architectural fact, episode, or reflexion lesson into Hybrid FTS5 + 128-D Vector + Navigable Small-World Graph memory.",
            "inputSchema": {
                "type": "object",
                "required": ["title", "content"],
                "properties": {
                    "kind": { "type": "string", "enum": ["knowledge", "reflexion", "episode"], "default": "knowledge" },
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "files": { "type": "array", "items": { "type": "string" } },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "importance": { "type": "number", "default": 1.5 }
                }
            }
        },
        {
            "name": "fleet_memory_search",
            "description": "Hybrid Reciprocal Rank Fusion (FTS5 BM25 + 128-D cosine vector + 1-hop NSW graph) search across persistent repository memory.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "files": { "type": "array", "items": { "type": "string" } },
                    "limit": { "type": "integer", "default": 6 }
                }
            }
        },
        {
            "name": "fleet_code_graph",
            "description": "Scan tracked repository source files across Rust/TS/C#/Python/Go/SQL and return PageRank hub modules, symbol exports, and circular dependency SCCs.",
            "inputSchema": {
                "type": "object",
                "properties": { "top_hubs": { "type": "integer", "default": 15 } }
            }
        },
        {
            "name": "fleet_blast_radius",
            "description": "Compute transitive reverse-dependency blast radius of modifying one or more repository files.",
            "inputSchema": {
                "type": "object",
                "required": ["files"],
                "properties": { "files": { "type": "array", "items": { "type": "string" } } }
            }
        },
        {
            "name": "fleet_static_audit",
            "description": "Run deterministic security, SQL-injection, IEEE-754 float-money, and multi-tenant PostgreSQL RLS audit across repository files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Optional subset of relative paths; defaults to all tracked source/SQL files." }
                }
            }
        },
        {
            "name": "fleet_providers_health",
            "description": "Inspect configured AI CLI providers (`claude`, `codex`, `antigravity`, custom), binary paths, active quota cooldowns, and historical UCB1 success rates.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "fleet_events_verify",
            "description": "Cryptographically verify the SHA-256 hash chain of all engine events in SQLite.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

pub fn call_tool(work_dir: &Path, name: &str, args: &Value) -> std::result::Result<Value, String> {
    let maybe_repo = git::repo_root(work_dir).ok();

    // Tools that work even outside a git repository:
    match name {
        "fleet_skills_match" => {
            let reg = SkillRegistry::load(maybe_repo.as_deref());
            let q = args.get("query").and_then(Value::as_str).ok_or("missing `query`")?;
            let owned: Vec<String> = str_vec(args.get("owned_files"));
            let top = args.get("top").and_then(Value::as_u64).unwrap_or(5) as usize;
            let role = if args.get("reviewer").and_then(Value::as_bool).unwrap_or(false) {
                Role::Reviewer
            } else {
                Role::Author
            };
            return Ok(serde_json::to_value(reg.select(q, &owned, role, &[], top)).unwrap());
        }
        "fleet_skills_get" => {
            let reg = SkillRegistry::load(maybe_repo.as_deref());
            let id = args.get("id").and_then(Value::as_str).ok_or("missing `id`")?;
            let spec = reg.get(id).ok_or_else(|| format!("skill `{id}` not found"))?;
            return Ok(serde_json::to_value(spec).unwrap());
        }
        "fleet_skills_list" => {
            let reg = SkillRegistry::load(maybe_repo.as_deref());
            let dom = args.get("domain").and_then(Value::as_str);
            let list: Vec<Value> = reg
                .list(dom)
                .into_iter()
                .map(|s| json!({ "id": s.id, "title": s.title, "domain": s.domain, "source_repo": s.source_repo, "stars": s.stars }))
                .collect();
            return Ok(json!({ "total": list.len(), "skills": list }));
        }
        _ => {}
    }

    let cfg = EngineConfig::load(work_dir).map_err(|e| e.to_string())?;
    let engine = Engine::open(cfg).map_err(|e| e.to_string())?;
    let repo = engine.config().repo.clone();

    match name {
        "fleet_status" => {
            let tasks = engine.store().list_tasks().map_err(|e| e.to_string())?;
            let mut counts = serde_json::Map::new();
            for t in &tasks {
                let k = t.status.as_str().to_string();
                let cur = counts.get(&k).and_then(Value::as_u64).unwrap_or(0);
                counts.insert(k, json!(cur + 1));
            }
            let mem_count = engine.memory().count().unwrap_or(0);
            Ok(json!({
                "repo": repo.display().to_string(),
                "total_tasks": tasks.len(),
                "tasks_by_status": counts,
                "memories": mem_count,
                "skills": engine.skills().len(),
                "providers": engine.provider_reports().map_err(|e| e.to_string())?,
            }))
        }
        "fleet_task_submit" => {
            let prompt = args.get("prompt").and_then(Value::as_str).ok_or("missing `prompt`")?;
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| prompt.lines().next().unwrap_or("task").chars().take(72).collect());
            let mut nt = NewTask::new(title, prompt);
            nt.owned_files = str_vec(args.get("owned_files"));
            nt.verify_cmd = args.get("verify_cmd").and_then(Value::as_str).map(str::to_string);
            nt.skills = str_vec(args.get("skills"));
            if let Some(r) = args.get("review").and_then(Value::as_bool) {
                nt.review = r;
            }
            if let Some(c) = args.get("complexity").and_then(Value::as_str) {
                nt.complexity = Complexity::parse(c);
            }
            let task = engine.submit(nt).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(task).unwrap())
        }
        "fleet_task_list" => Ok(serde_json::to_value(engine.store().list_tasks().map_err(|e| e.to_string())?).unwrap()),
        "fleet_task_show" => {
            let id = args.get("id").and_then(Value::as_str).ok_or("missing `id`")?;
            let task = engine.store().get_task(id).map_err(|e| e.to_string())?.ok_or_else(|| format!("task `{id}` not found"))?;
            let runs = engine.store().runs_for_task(id).map_err(|e| e.to_string())?;
            Ok(json!({ "task": task, "runs": runs }))
        }
        "fleet_queen_decompose" => {
            let epic = args.get("epic").and_then(Value::as_str).ok_or("missing `epic`")?;
            let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
            let verify_cmd = args.get("verify_cmd").and_then(Value::as_str).map(str::to_string);
            let graph = CodeGraph::scan(&repo, 600).map_err(|e| e.to_string())?;
            let plan = queen::decompose_architecturally(epic, &repo, &graph, engine.skills(), verify_cmd)
                .map_err(|e| e.to_string())?;
            let submitted = if submit {
                engine.submit_plan(&plan, true).map_err(|e| e.to_string())?
            } else {
                Vec::new()
            };
            Ok(json!({ "plan": plan, "submitted_task_ids": submitted }))
        }
        "fleet_memory_store" => {
            let title = args.get("title").and_then(Value::as_str).ok_or("missing `title`")?;
            let content = args.get("content").and_then(Value::as_str).ok_or("missing `content`")?;
            let kind = args.get("kind").and_then(Value::as_str).map(MemoryKind::parse).unwrap_or(MemoryKind::Knowledge);
            let id = engine
                .memory()
                .store(NewMemory {
                    kind,
                    title: title.into(),
                    content: content.into(),
                    files: str_vec(args.get("files")),
                    tags: str_vec(args.get("tags")),
                    task_id: None,
                    importance: args.get("importance").and_then(Value::as_f64).unwrap_or(1.5),
                })
                .map_err(|e| e.to_string())?;
            Ok(json!({ "stored_id": id }))
        }
        "fleet_memory_search" => {
            let query = args.get("query").and_then(Value::as_str).ok_or("missing `query`")?;
            let files = str_vec(args.get("files"));
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(6) as usize;
            let hits = engine.memory().search(query, &files, limit).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(hits).unwrap())
        }
        "fleet_code_graph" => {
            let top = args.get("top_hubs").and_then(Value::as_u64).unwrap_or(15) as usize;
            let g = CodeGraph::scan(&repo, 1200).map_err(|e| e.to_string())?;
            let hubs: Vec<Value> = g
                .top_hubs(top)
                .into_iter()
                .map(|n| json!({ "path": n.path, "language": n.language, "lines": n.lines, "pagerank": n.pagerank, "in_degree": n.in_degree, "out_degree": n.out_degree, "symbols": n.symbols }))
                .collect();
            Ok(json!({
                "total_files": g.total_files,
                "total_lines": g.total_lines,
                "languages": g.languages,
                "edges_count": g.edges.len(),
                "cycles": g.cycles,
                "top_hubs": hubs,
            }))
        }
        "fleet_blast_radius" => {
            let files = str_vec(args.get("files"));
            let g = CodeGraph::scan(&repo, 1200).map_err(|e| e.to_string())?;
            Ok(json!({ "changed": files, "impacted_dependents": g.blast_radius(&files) }))
        }
        "fleet_static_audit" => {
            let mut files = str_vec(args.get("files"));
            if files.is_empty() {
                let tracked = git::git(&repo, &["ls-files"]).unwrap_or_default();
                files = tracked
                    .lines()
                    .filter(|l| !l.starts_with("v3/") && !l.starts_with("plugins/") && !l.starts_with("skills/"))
                    .take(800)
                    .map(str::to_string)
                    .collect();
            }
            let report = audit_file_list(&repo, &files).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(report).unwrap())
        }
        "fleet_providers_health" => Ok(serde_json::to_value(engine.provider_reports().map_err(|e| e.to_string())?).unwrap()),
        "fleet_events_verify" => match engine.store().verify_events().map_err(|e| e.to_string())? {
            Ok(n) => Ok(json!({ "intact": true, "verified_events": n })),
            Err(bad_id) => Ok(json!({ "intact": false, "tampered_at_event_id": bad_id })),
        },
        other => Err(format!("unknown MCP tool `{other}`")),
    }
}

fn str_vec(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Handle a single JSON-RPC 2.0 request object (`None` for notifications).
pub fn handle_jsonrpc(work_dir: &Path, req: &Value) -> Option<Value> {
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let id = req.get("id").cloned();

    // Notifications carry no `id` and expect no response.
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let id = id.unwrap();

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "ai-fleet-engine", "version": "6.0.0" }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
            let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(work_dir, tool_name, &arguments) {
                Ok(payload) => Ok(json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap_or_default() }],
                    "isError": false
                })),
                Err(err) => Ok(json!({
                    "content": [{ "type": "text", "text": format!("error: {err}") }],
                    "isError": true
                })),
            }
        }
        other => Err((
            -32601,
            format!("method not found: {other}"),
        )),
    };

    Some(match result {
        Ok(res) => json!({ "jsonrpc": "2.0", "id": id, "result": res }),
        Err((code, msg)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }),
    })
}

/// Serve MCP JSON-RPC 2.0 over `stdin`/`stdout`, auto-detecting `Content-Length`
/// framing vs newline-delimited JSON.
pub fn serve_stdio(work_dir: &Path) -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (framed, payload) = if let Some(rest) = trimmed.strip_prefix("Content-Length:").or_else(|| trimmed.strip_prefix("content-length:")) {
            let len: usize = rest.trim().parse().unwrap_or(0);
            // Consume remaining header lines up to blank line.
            loop {
                line.clear();
                if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
                    break;
                }
            }
            let mut buf = vec![0u8; len];
            io::Read::read_exact(&mut reader, &mut buf)?;
            (true, String::from_utf8_lossy(&buf).to_string())
        } else {
            (false, trimmed.to_string())
        };

        let Ok(req) = serde_json::from_str::<Value>(&payload) else { continue };
        if let Some(resp) = handle_jsonrpc(work_dir, &req) {
            let body = serde_json::to_string(&resp).unwrap_or_default();
            if framed {
                write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
            } else {
                writeln!(writer, "{body}")?;
            }
            writer.flush()?;
        }
    }
    Ok(())
}

// Keep unused import clean when referenced by tests
#[allow(dead_code)]
fn _touch_mem(_: &MemoryStore) {}
