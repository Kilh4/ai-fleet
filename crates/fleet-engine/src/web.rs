//! Live Mission Control Web Server (`http://127.0.0.1:<port>`) wired directly
//! to the real `fleet-engine` SQLite queue, Queen DAG planner, 120 Elite Skills,
//! Hybrid FTS5+Vector Memory, CodeGraph PageRank analyzer, and Static Auditor.

use crate::engine::{Engine, EngineConfig};
use crate::mcp::call_tool;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

pub fn serve_http(work_dir: &Path, bind_addr: &str) -> crate::Result<()> {
    let cfg = EngineConfig::load(work_dir)?;
    let engine = Engine::open(cfg)?;
    let listener = TcpListener::bind(bind_addr)?;
    let repo_dir = engine.config().repo.clone();
    println!("AI-Fleet v6.0 Live Mission Control listening on http://{bind_addr}");
    println!("Connected to SQLite engine: {}", engine.config().state_dir.join("engine.db").display());

    let rt = Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build()?);

    for stream in listener.incoming().flatten() {
        let eng = engine.clone();
        let repo = repo_dir.clone();
        let rt2 = rt.clone();
        std::thread::spawn(move || {
            let _ = handle_conn(stream, &eng, &repo, &rt2);
        });
    }
    Ok(())
}

fn handle_conn(
    mut stream: std::net::TcpStream,
    engine: &Engine,
    repo: &Path,
    rt: &Arc<tokio::runtime::Runtime>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buf = vec![0u8; 65536];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(());
    }
    let raw = String::from_utf8_lossy(&buf[..n]);
    let mut lines = raw.lines();
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");
    let body_str = raw.split("\r\n\r\n").nth(1).unwrap_or("").trim();
    let body_json: Value = serde_json::from_str(body_str).unwrap_or_else(|_| json!({}));

    if method == "GET" && (path == "/" || path == "/index.html") {
        return write_resp(&mut stream, 200, "text/html; charset=utf-8", DASHBOARD_HTML.as_bytes());
    }

    let result: std::result::Result<Value, String> = match (method, path) {
        ("GET", "/api/overview") => build_overview(engine, repo),
        ("POST", "/api/tasks") => call_tool(repo, "fleet_task_submit", &body_json),
        ("POST", "/api/queen") => call_tool(repo, "fleet_queen_decompose", &body_json),
        ("POST", "/api/skills/match") => call_tool(repo, "fleet_skills_match", &body_json),
        ("GET", "/api/skills") => call_tool(repo, "fleet_skills_list", &json!({})),
        ("POST", "/api/memory/store") => call_tool(repo, "fleet_memory_store", &body_json),
        ("POST", "/api/memory/search") => call_tool(repo, "fleet_memory_search", &body_json),
        ("GET", "/api/graph") => call_tool(repo, "fleet_code_graph", &json!({ "top_hubs": 20 })),
        ("POST", "/api/audit") => call_tool(repo, "fleet_static_audit", &body_json),
        ("POST", "/api/run-idle") => {
            let eng = engine.clone();
            rt.spawn(async move {
                let _ = eng.run_until_idle().await;
            });
            Ok(json!({ "started": true }))
        }
        _ => Err(format!("not found: {method} {path}")),
    };

    match result {
        Ok(val) => write_resp(&mut stream, 200, "application/json; charset=utf-8", serde_json::to_string(&val).unwrap_or_default().as_bytes()),
        Err(err) => write_resp(&mut stream, 400, "application/json; charset=utf-8", serde_json::to_string(&json!({ "error": err })).unwrap_or_default().as_bytes()),
    }
}

fn build_overview(engine: &Engine, repo: &Path) -> std::result::Result<Value, String> {
    let status = call_tool(repo, "fleet_status", &json!({}))?;
    let tasks = engine.store().list_tasks().map_err(|e| e.to_string())?;
    let runs = engine.store().recent_runs(25).map_err(|e| e.to_string())?;
    let events = engine.store().events(None, 30).map_err(|e| e.to_string())?;
    let memories = engine.memory().recent(15).map_err(|e| e.to_string())?;
    Ok(json!({ "status": status, "tasks": tasks, "runs": runs, "events": events, "memories": memories }))
}

fn write_resp(stream: &mut std::net::TcpStream, code: u16, ct: &str, body: &[u8]) -> std::io::Result<()> {
    let status = if code == 200 { "200 OK" } else { "400 Bad Request" };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AI-Fleet v6.0 — Live SQLite Engine Mission Control</title>
<style>
  :root { --bg:#090d16; --panel:#111827; --border:#1f2937; --txt:#e5e7eb; --muted:#9ca3af; --acc:#38bdf8; --ok:#22c55e; --warn:#f59e0b; --err:#ef4444; }
  * { box-sizing:border-box; font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif; }
  body { margin:0; background:var(--bg); color:var(--txt); padding:20px; }
  header { display:flex; justify-content:space-between; align-items:center; border-bottom:1px solid var(--border); padding-bottom:14px; margin-bottom:18px; }
  h1 { margin:0; font-size:20px; letter-spacing:0.3px; }
  .badge { background:#0ea5e922; color:var(--acc); border:1px solid #38bdf855; padding:4px 10px; border-radius:999px; font-size:12px; }
  .grid { display:grid; grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); gap:16px; margin-bottom:16px; }
  .card { background:var(--panel); border:1px solid var(--border); border-radius:10px; padding:16px; }
  .card h2 { margin:0 0 12px 0; font-size:15px; color:var(--acc); }
  input, textarea { width:100%; background:#0b1120; color:var(--txt); border:1px solid var(--border); border-radius:6px; padding:8px 10px; margin-bottom:8px; font-size:13px; }
  button { background:var(--acc); color:#06121d; border:none; border-radius:6px; padding:8px 14px; font-weight:700; cursor:pointer; font-size:13px; margin-right:6px; }
  button.sec { background:#1e293b; color:var(--txt); border:1px solid var(--border); }
  table { width:100%; border-collapse:collapse; font-size:12px; }
  th, td { text-align:left; padding:7px 6px; border-bottom:1px solid var(--border); }
  th { color:var(--muted); font-weight:600; }
  pre { background:#0b1120; padding:10px; border-radius:6px; overflow:auto; max-height:240px; font-size:12px; margin:8px 0 0; }
</style>
</head>
<body>
<header>
  <div>
    <h1>AI-Fleet v6.0 — Real Multi-AI Engine Mission Control</h1>
    <div id="sub" style="color:var(--muted);font-size:12px;margin-top:4px">Loading SQLite engine.db…</div>
  </div>
  <div>
    <button onclick="runIdle()">▶ Run Queue Now</button>
    <button class="sec" onclick="runAudit()">🛡 Static Security/Money/RLS Audit</button>
    <button class="sec" onclick="loadGraph()">🕸 Scan CodeGraph PageRank</button>
  </div>
</header>

<div class="grid">
  <div class="card">
    <h2>👑 Queen Epic DAG Decomposer & Task Submitter</h2>
    <textarea id="prompt" rows="3" placeholder="Write a task or multi-wave epic (Arabic or English)…"></textarea>
    <input id="owned" placeholder="Optional owned file globs (comma-separated, e.g. src/**, migrations/**)">
    <button onclick="submitTask()">+ Submit Single Task</button>
    <button class="sec" onclick="submitQueen()">👑 Queen Decompose & Enqueue DAG</button>
    <pre id="planOut" style="display:none"></pre>
  </div>

  <div class="card">
    <h2>🧠 120 Elite GitHub Skills Matcher (BM25 + Arabic/English)</h2>
    <input id="skillQuery" placeholder="Test skill matching: e.g. محاسبة قيد مزدوج و RLS or quantum fluid simulation">
    <button onclick="matchSkills()">Match Top Skills</button>
    <pre id="skillOut">Type a query to rank the 120 bundled Elite Skills…</pre>
  </div>

  <div class="card">
    <h2>💾 Hybrid Memory (FTS5 + 128-D Vector + Reflexion)</h2>
    <input id="memQuery" placeholder="Search institutional memory & past reflexion lessons…">
    <button onclick="searchMem()">Search Memory</button>
    <pre id="memOut"></pre>
  </div>
</div>

<div class="card" style="margin-bottom:16px">
  <h2>📋 Persistent SQLite Task Queue (`engine.db`)</h2>
  <table><thead><tr><th>ID</th><th>Status</th><th>Complexity</th><th>Attempts/Handoffs</th><th>Provider/Model</th><th>Reviewer</th><th>Title</th></tr></thead>
  <tbody id="tasksBody"></tbody></table>
</div>

<div class="grid">
  <div class="card"><h2>⚡ Providers & Subscription Cooldowns</h2><pre id="provOut"></pre></div>
  <div class="card"><h2>🔍 Graph / Static Audit Output</h2><pre id="diagOut">Click "Static Audit" or "Scan CodeGraph" above…</pre></div>
</div>

<script>
async function api(method, url, body) {
  const r = await fetch(url, { method, headers:{'Content-Type':'application/json'}, body: body ? JSON.stringify(body) : undefined });
  return r.json();
}
async function refresh() {
  const d = await api('GET', '/api/overview');
  if (d.error) return;
  const st = d.status;
  document.getElementById('sub').textContent = `Repo: ${st.repo} | Tasks: ${st.total_tasks} | Bundled Elite Skills: ${st.skills} | Persistent Memories: ${st.memories}`;
  document.getElementById('tasksBody').innerHTML = (d.tasks || []).map(t =>
    `<tr><td><code>${t.id}</code></td><td><b>${t.status}</b></td><td>${t.complexity}</td><td>${t.attempts} / ${t.handoffs}</td><td>${t.last_provider||'-'}/${t.last_model||'-'}</td><td>${t.reviewer||'-'}</td><td>${t.title}</td></tr>`
  ).join('');
  document.getElementById('provOut').textContent = JSON.stringify(st.providers, null, 2);
  if (!document.getElementById('memOut').dataset.searched) {
    document.getElementById('memOut').textContent = JSON.stringify(d.memories, null, 2);
  }
}
async function submitTask() {
  const prompt = document.getElementById('prompt').value.trim();
  if (!prompt) return;
  const owned = document.getElementById('owned').value.split(',').map(s=>s.trim()).filter(Boolean);
  const out = await api('POST', '/api/tasks', { prompt, owned_files: owned });
  const el = document.getElementById('planOut'); el.style.display='block'; el.textContent = JSON.stringify(out, null, 2);
  refresh();
}
async function submitQueen() {
  const epic = document.getElementById('prompt').value.trim();
  if (!epic) return;
  const out = await api('POST', '/api/queen', { epic, submit: true });
  const el = document.getElementById('planOut'); el.style.display='block'; el.textContent = JSON.stringify(out, null, 2);
  refresh();
}
async function matchSkills() {
  const query = document.getElementById('skillQuery').value.trim();
  if (!query) return;
  document.getElementById('skillOut').textContent = JSON.stringify(await api('POST', '/api/skills/match', { query, top: 6 }), null, 2);
}
async function searchMem() {
  const query = document.getElementById('memQuery').value.trim();
  if (!query) return;
  const el = document.getElementById('memOut'); el.dataset.searched = '1';
  el.textContent = JSON.stringify(await api('POST', '/api/memory/search', { query, limit: 6 }), null, 2);
}
async function runIdle() { await api('POST', '/api/run-idle', {}); setTimeout(refresh, 600); }
async function runAudit() { document.getElementById('diagOut').textContent = JSON.stringify(await api('POST', '/api/audit', {}), null, 2); }
async function loadGraph() { document.getElementById('diagOut').textContent = JSON.stringify(await api('GET', '/api/graph'), null, 2); }
refresh(); setInterval(refresh, 3000);
</script>
</body>
</html>
"#;
