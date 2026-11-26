//! Production Async HTTP + Server-Sent Events (`SSE`) Web Server built on `axum`.
//!
//! Connected exclusively to the real SQLite `engine.db`, `memory.db`, `CodeGraph`,
//! `StaticAuditor`, and 120 Embedded Skills — zero mock or hardcoded demo state.

use crate::engine::{Engine, EngineConfig};
use crate::mcp::call_tool;
use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct WebState {
    pub engine: Engine,
    pub repo: PathBuf,
}

pub fn app_router(state: WebState) -> Router {
    Router::new()
        .route("/", get(index_html))
        .route("/api/overview", get(api_overview))
        .route("/api/events/stream", get(api_sse_events))
        .route("/api/tasks", post(api_submit_task))
        .route("/api/queen", post(api_queen))
        .route("/api/skills", get(api_skills_list))
        .route("/api/skills/match", post(api_skills_match))
        .route("/api/memory/store", post(api_memory_store))
        .route("/api/memory/search", post(api_memory_search))
        .route("/api/graph", get(api_graph))
        .route("/api/audit", post(api_audit))
        .route("/api/run-idle", post(api_run_idle))
        .with_state(Arc::new(state))
}

pub async fn serve_http_async(work_dir: &Path, bind_addr: &str) -> crate::Result<()> {
    let cfg = EngineConfig::load(work_dir)?;
    let engine = Engine::open(cfg)?;
    let repo = engine.config().repo.clone();
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    println!("AI-Fleet axum HTTP + SSE server listening on http://{bind_addr}");
    println!("SQLite DB: {}", engine.config().state_dir.join("engine.db").display());

    let router = app_router(WebState { engine, repo });
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn index_html() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

fn json_response(res: std::result::Result<Value, String>) -> impl IntoResponse {
    match res {
        Ok(val) => (StatusCode::OK, Json(val)),
        Err(err) => (StatusCode::BAD_REQUEST, Json(json!({ "error": err }))),
    }
}

async fn api_overview(State(st): State<Arc<WebState>>) -> impl IntoResponse {
    let st2 = st.clone();
    let res = tokio::task::spawn_blocking(move || -> std::result::Result<Value, String> {
        let status = call_tool(&st2.repo, "fleet_status", &json!({}))?;
        let tasks = st2.engine.store().list_tasks().map_err(|e| e.to_string())?;
        let runs = st2.engine.store().recent_runs(25).map_err(|e| e.to_string())?;
        let events = st2.engine.store().events(None, 40).map_err(|e| e.to_string())?;
        let memories = st2.engine.memory().recent(15).map_err(|e| e.to_string())?;
        Ok(json!({
            "status": status,
            "tasks": tasks,
            "runs": runs,
            "events": events,
            "memories": memories
        }))
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    json_response(res)
}

async fn api_sse_events(
    State(st): State<Arc<WebState>>,
) -> Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>>> {
    let rx = st.engine.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(ev) => {
            let payload = serde_json::to_string(&ev).unwrap_or_default();
            Some(Ok(Event::default().event("engine_event").data(payload)))
        }
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)).text("keepalive"))
}

async fn run_tool_blocking(st: Arc<WebState>, tool: &'static str, body: Value) -> impl IntoResponse {
    let res = tokio::task::spawn_blocking(move || call_tool(&st.repo, tool, &body))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    json_response(res)
}

async fn api_submit_task(State(st): State<Arc<WebState>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool_blocking(st, "fleet_task_submit", body).await
}

async fn api_queen(State(st): State<Arc<WebState>>, Json(body): Json<Value>) -> impl IntoResponse {
    let epic = body.get("epic").and_then(Value::as_str).unwrap_or("").to_string();
    if epic.trim().is_empty() {
        return json_response(Err("missing `epic`".into())).into_response();
    }
    let offline = body.get("offline").and_then(Value::as_bool).unwrap_or(false);
    let submit = body.get("submit").and_then(Value::as_bool).unwrap_or(true);
    let verify_cmd = body.get("verify_cmd").and_then(Value::as_str).map(str::to_string);
    match st.engine.plan_epic(&epic, offline, verify_cmd).await {
        Ok(plan) => {
            let ids = if submit {
                match st.engine.submit_plan(&plan, true) {
                    Ok(v) => v,
                    Err(e) => return json_response(Err(e.to_string())).into_response(),
                }
            } else {
                Vec::new()
            };
            json_response(Ok(json!({ "plan": plan, "submitted_task_ids": ids }))).into_response()
        }
        Err(e) => json_response(Err(e.to_string())).into_response(),
    }
}

async fn api_skills_list(State(st): State<Arc<WebState>>) -> impl IntoResponse {
    run_tool_blocking(st, "fleet_skills_list", json!({})).await
}

async fn api_skills_match(State(st): State<Arc<WebState>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool_blocking(st, "fleet_skills_match", body).await
}

async fn api_memory_store(State(st): State<Arc<WebState>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool_blocking(st, "fleet_memory_store", body).await
}

async fn api_memory_search(State(st): State<Arc<WebState>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool_blocking(st, "fleet_memory_search", body).await
}

async fn api_graph(State(st): State<Arc<WebState>>) -> impl IntoResponse {
    run_tool_blocking(st, "fleet_code_graph", json!({ "top_hubs": 20 })).await
}

async fn api_audit(State(st): State<Arc<WebState>>, Json(body): Json<Value>) -> impl IntoResponse {
    run_tool_blocking(st, "fleet_static_audit", body).await
}

async fn api_run_idle(State(st): State<Arc<WebState>>) -> impl IntoResponse {
    let eng = st.engine.clone();
    tokio::spawn(async move {
        if let Err(e) = eng.run_until_idle().await {
            let _ = eng.store().append_event(None, "web.run_error", &e.to_string());
        }
    });
    json_response(Ok(json!({ "started": true })))
}

const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AI-Fleet — axum + SSE SQLite Mission Control</title>
<style>
  :root { --bg:#090d16; --panel:#111827; --border:#1f2937; --txt:#e5e7eb; --muted:#9ca3af; --acc:#38bdf8; }
  * { box-sizing:border-box; font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif; }
  body { margin:0; background:var(--bg); color:var(--txt); padding:20px; }
  header { display:flex; justify-content:space-between; align-items:center; border-bottom:1px solid var(--border); padding-bottom:14px; margin-bottom:18px; }
  h1 { margin:0; font-size:19px; }
  .grid { display:grid; grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); gap:16px; margin-bottom:16px; }
  .card { background:var(--panel); border:1px solid var(--border); border-radius:10px; padding:16px; }
  .card h2 { margin:0 0 10px 0; font-size:14px; color:var(--acc); }
  input, textarea { width:100%; background:#0b1120; color:var(--txt); border:1px solid var(--border); border-radius:6px; padding:8px 10px; margin-bottom:8px; font-size:13px; }
  button { background:var(--acc); color:#06121d; border:none; border-radius:6px; padding:8px 14px; font-weight:700; cursor:pointer; font-size:13px; margin-right:6px; }
  button.sec { background:#1e293b; color:var(--txt); border:1px solid var(--border); }
  table { width:100%; border-collapse:collapse; font-size:12px; }
  th, td { text-align:left; padding:7px 6px; border-bottom:1px solid var(--border); }
  th { color:var(--muted); }
  pre { background:#0b1120; padding:10px; border-radius:6px; overflow:auto; max-height:220px; font-size:12px; margin:8px 0 0; }
</style>
</head>
<body>
<header>
  <div>
    <h1>AI-Fleet — Real SQLite + axum + SSE Mission Control</h1>
    <div id="sub" style="color:var(--muted);font-size:12px;margin-top:4px">Connecting to SQLite & SSE stream…</div>
  </div>
  <div>
    <button onclick="runIdle()">▶ Run Queue</button>
    <button class="sec" onclick="runAudit()">🛡 Static Gate</button>
    <button class="sec" onclick="loadGraph()">🕸 CodeGraph</button>
  </div>
</header>

<div class="grid">
  <div class="card">
    <h2>Task & Live AI Queen Submitter</h2>
    <textarea id="prompt" rows="3" placeholder="Task or Epic (Arabic or English)…"></textarea>
    <input id="owned" placeholder="Optional owned globs (comma-separated, e.g. src/**)">
    <button onclick="submitTask()">+ Submit Task</button>
    <button class="sec" onclick="submitQueen()">👑 Queen Plan & Enqueue</button>
    <pre id="planOut" style="display:none"></pre>
  </div>
  <div class="card">
    <h2>120 Skills Matcher (Okapi BM25)</h2>
    <input id="skillQuery" placeholder="Query skills…">
    <button onclick="matchSkills()">Match</button>
    <pre id="skillOut"></pre>
  </div>
  <div class="card">
    <h2>Live SSE Event Stream (`/api/events/stream`)</h2>
    <pre id="sseLog"></pre>
  </div>
</div>

<div class="card" style="margin-bottom:16px">
  <h2>SQLite Task Queue (`engine.db`)</h2>
  <table><thead><tr><th>ID</th><th>Status</th><th>Complexity</th><th>Att/Hand</th><th>Provider/Model</th><th>Session</th><th>Title</th></tr></thead>
  <tbody id="tasksBody"></tbody></table>
</div>

<div class="grid">
  <div class="card"><h2>Providers & Cooldowns</h2><pre id="provOut"></pre></div>
  <div class="card"><h2>Diagnostics / Memory</h2><pre id="diagOut"></pre></div>
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
  document.getElementById('sub').textContent = `Repo: ${st.repo} | Tasks: ${st.total_tasks} | Skills: ${st.skills} | Memories: ${st.memories}`;
  document.getElementById('tasksBody').innerHTML = (d.tasks || []).map(t =>
    `<tr><td><code>${t.id}</code></td><td><b>${t.status}</b></td><td>${t.complexity}</td><td>${t.attempts}/${t.handoffs}</td><td>${t.last_provider||'-'}/${t.last_model||'-'}</td><td><code>${(t.last_session_id||'-').slice(0,10)}</code></td><td>${t.title}</td></tr>`
  ).join('');
  document.getElementById('provOut').textContent = JSON.stringify(st.providers, null, 2);
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
async function runIdle() { await api('POST', '/api/run-idle', {}); }
async function runAudit() { document.getElementById('diagOut').textContent = JSON.stringify(await api('POST', '/api/audit', {}), null, 2); }
async function loadGraph() { document.getElementById('diagOut').textContent = JSON.stringify(await api('GET', '/api/graph'), null, 2); }

const es = new EventSource('/api/events/stream');
es.addEventListener('engine_event', (e) => {
  const ev = JSON.parse(e.data);
  const pre = document.getElementById('sseLog');
  pre.textContent = `${ev.ts.slice(11,19)} [${ev.kind}] ${ev.task_id||'-'}: ${ev.message}\n` + pre.textContent.slice(0, 6000);
  refresh();
});
refresh();
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ModelSpec;
    use crate::{Effort, NewTask, ProviderSpec};
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use std::process::Command;
    use tower::ServiceExt;

    #[tokio::test]
    async fn axum_router_serves_real_sqlite_overview_and_task_submission() {
        let dir = tempfile::tempdir().unwrap();
        let run_git = |args: &[&str]| {
            Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
        };
        run_git(&["init", "-q"]);
        std::fs::write(dir.path().join("README.md"), "hi\n").unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-q", "-m", "init"]);

        let spec = ProviderSpec::generic("alpha", "claude", &[], vec![ModelSpec { name: "opus".into(), tier: 3, max_effort: Effort::Max }]);
        let cfg = EngineConfig::new(dir.path().to_path_buf(), vec![spec]);
        let engine = Engine::open(cfg).unwrap();
        engine.submit(NewTask::new("real sqlite task", "implement x")).unwrap();

        let app = app_router(WebState { engine, repo: dir.path().to_path_buf() });
        let resp = app
            .oneshot(Request::builder().uri("/api/overview").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["status"]["total_tasks"], 1);
        assert_eq!(v["tasks"][0]["title"], "real sqlite task");
    }
}
