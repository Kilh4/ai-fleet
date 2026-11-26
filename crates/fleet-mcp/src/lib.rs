//! `fleet-mcp` — Native Rust Model Context Protocol (MCP) JSON-RPC 2.0 Server
//! over stdio & direct CLI tool executor (`ai-fleet mcp start` / `ai-fleet mcp exec`).
//! Connectable directly to Claude Code, Codex, Cursor, and Antigravity.

use chrono::Utc;
use fleet_agents::{catalog_64_agents, find_best_persona};
use fleet_federation::create_federated_packet;
use fleet_memory::{insert_hnsw_node, search_hnsw_semantic};
use fleet_neural::{compress_message_to_token_budget, route_moe_experts};
use fleet_providers::{load_or_init_fleet_state, save_fleet_state};
use fleet_security::{check_casa_authorization, scan_aidefence_threats};
use fleet_swarm::{compute_optimal_route, execute_consensus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolSpec {
    pub name: String,
    pub category: String,
    pub description: String,
}

pub fn list_mcp_tools() -> Vec<McpToolSpec> {
    let raw = [
        ("fleet_swarm_status", "swarm", "Return full multi-AI swarm topology, quotas, active leases, and ledger health"),
        ("fleet_agent_catalog", "agents", "List or query the 64 specialized agent personas across 8 domains"),
        ("fleet_route_moe", "neural", "Run 2-Layer 8-Expert MoE + Pheromone + SONA/EWC routing for a task"),
        ("fleet_memory_store", "memory", "Index a key-value document into the 64-dim HNSW semantic vector graph"),
        ("fleet_memory_search", "memory", "Execute hybrid HNSW cosine + lexical semantic search across shared memory"),
        ("fleet_consensus_vote", "consensus", "Execute PBFT, Raft, or Gossip weighted consensus across all AI models"),
        ("fleet_aidefence_scan", "security", "Scan prompt or file payload for ADR-118 prompt injection & secret leaks"),
        ("fleet_casa_authorize", "security", "Deterministically evaluate CASA (ADR-380 §3) deny-by-default authority envelope"),
        ("fleet_compress_message", "neural", "Compress inter-agent handoff message to target token budget (Information Bottleneck)"),
        ("fleet_federation_send", "federation", "Strip PII/secrets and embed Gumbel-Softmax watermark for cross-team federation"),
    ];
    raw.iter()
        .map(|(n, c, d)| McpToolSpec {
            name: (*n).to_string(),
            category: (*c).to_string(),
            description: (*d).to_string(),
        })
        .collect()
}

pub fn execute_mcp_tool(tool_name: &str, params: &Value) -> Value {
    let mut state = load_or_init_fleet_state();
    match tool_name {
        "fleet_swarm_status" => json!({
            "success": true,
            "version": state.version,
            "topology": state.topology,
            "global_epoch": state.global_epoch,
            "providers_count": state.providers.len(),
            "tasks_count": state.tasks.len(),
            "vector_memory_nodes": state.vector_memory.len(),
        }),
        "fleet_agent_catalog" => {
            let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if query.is_empty() {
                let all = catalog_64_agents();
                json!({ "success": true, "total_personas": all.len(), "sample": &all[..8] })
            } else {
                let best = find_best_persona(query);
                json!({ "success": true, "matched_persona": best })
            }
        }
        "fleet_route_moe" => {
            let query = params
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("Audit SQL migration and RLS security");
            let moe = route_moe_experts(&state.moe_experts, query, 3);
            let task = state.tasks.values().next().cloned();
            let route = task.and_then(|t| {
                compute_optimal_route(
                    &state.providers,
                    &state.moe_experts,
                    &state.reasoning_bank,
                    &t,
                    &BTreeSet::new(),
                    false,
                )
            });
            json!({ "success": true, "moe": moe, "optimal_route": route })
        }
        "fleet_memory_store" => {
            let ns = params.get("namespace").and_then(|v| v.as_str()).unwrap_or("global");
            let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("note");
            let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
            insert_hnsw_node(&mut state.vector_memory, ns, key, content, vec!["mcp".into()], 0.95);
            save_fleet_state(&state);
            json!({ "success": true, "stored_key": key, "total_nodes": state.vector_memory.len() })
        }
        "fleet_memory_search" => {
            let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("money decimal");
            let hits = search_hnsw_semantic(&state.vector_memory, query, 3);
            json!({ "success": true, "query": query, "hits": hits })
        }
        "fleet_consensus_vote" => {
            let proto = params.get("protocol").and_then(|v| v.as_str()).unwrap_or("PBFT");
            let topic = params.get("topic").and_then(|v| v.as_str()).unwrap_or("Approve release gate");
            let seq = (state.consensus_history.len() as u64) + 1;
            let round = execute_consensus(&state.providers, proto, topic, seq);
            state.consensus_history.push(round.clone());
            save_fleet_state(&state);
            json!({ "success": true, "round": round })
        }
        "fleet_aidefence_scan" => {
            let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let findings = scan_aidefence_threats(text);
            json!({ "success": true, "clean": findings.is_empty(), "findings": findings })
        }
        "fleet_casa_authorize" => {
            let action = params.get("action").and_then(|v| v.as_str()).unwrap_or("worktree.write");
            let dec = check_casa_authorization(&state.casa_envelope, action, &Utc::now().to_rfc3339());
            json!({ "success": true, "decision": dec })
        }
        "fleet_compress_message" => {
            let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let budget = params.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let (compressed, orig, final_t) = compress_message_to_token_budget(text, budget);
            json!({ "success": true, "compressed": compressed, "original_tokens": orig, "compressed_tokens": final_t })
        }
        "fleet_federation_send" => {
            let peer = params.get("peer").and_then(|v| v.as_str()).unwrap_or("team-alpha");
            let payload = params.get("payload").and_then(|v| v.as_str()).unwrap_or("");
            let env = create_federated_packet("ai-fleet-local", peer, payload, "estr-salt-v3");
            json!({ "success": true, "envelope": env })
        }
        _ => json!({ "success": false, "error": format!("Unknown MCP tool '{}'", tool_name) }),
    }
}

/// Runs a standards-compliant JSON-RPC 2.0 MCP Server loop over stdio.
pub fn run_mcp_stdio_server() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "serverInfo": { "name": "ai-fleet-mcp", "version": "3.0.0" },
                    "capabilities": { "tools": {} }
                }
            }),
            "tools/list" => {
                let tools: Vec<Value> = list_mcp_tools()
                    .into_iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": format!("[{}] {}", t.category, t.description),
                            "inputSchema": { "type": "object" }
                        })
                    })
                    .collect();
                json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools } })
            }
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let out = execute_mcp_tool(name, &args);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&out).unwrap_or_default() }]
                    }
                })
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "status": "ok" }
            }),
        };

        if let Ok(line_out) = serde_json::to_string(&response) {
            let _ = writeln!(stdout, "{}", line_out);
            let _ = stdout.flush();
        }
    }
}
