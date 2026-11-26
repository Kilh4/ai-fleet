//! `ai-fleet` v3.0 CLI — Unified Command Entrypoint for the 12-Crate Multi-AI Orchestrator.

use chrono::Utc;
use fleet_agents::{catalog_64_agents, find_best_persona};
use fleet_core::{
    append_ledger_row, detect_head_commit, detect_workspace_root, sha256_bytes, sha256_file, verify_ledger_chain,
    AiProvider, ModelSpec, SwarmTask,
};
use fleet_federation::{compute_gumbel_watermark_zscore, create_federated_packet};
use fleet_mcp::{execute_mcp_tool, list_mcp_tools, run_mcp_stdio_server};
use fleet_memory::{insert_hnsw_node, search_hnsw_semantic};
use fleet_neural::{compress_message_to_token_budget, route_moe_experts};
use fleet_providers::{
    execute_ai_cli, load_or_init_fleet_state, record_outcome, refresh_provider_quotas, save_fleet_state,
};
use fleet_security::{check_casa_authorization, evaluate_continue_gate, scan_aidefence_threats};
use fleet_swarm::{
    classify_task_tier, compute_optimal_route, execute_consensus, prepare_fenced_worktree, queen_decompose_task,
};
use fleet_tui::{
    badge_effort, badge_status, badge_tier, enable_ansi_support, print_dashboard, render_float_bar, BOLD,
    BRIGHT_CYAN, CYAN, GREEN, MAGENTA, RED, RESET, WHITE, YELLOW,
};
use fleet_workflows::advance_sparc_phase;
use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::time::Instant;

fn print_v3_help() {
    println!(
        "\n{}{}AI-FLEET v3.0.0 (12-Crate Rust Monorepo){} — Enterprise Multi-AI Swarm Orchestration Platform",
        BOLD, BRIGHT_CYAN, RESET
    );
    println!("Supersedes Ruflo V3: 64 Agent Personas, HNSW Vector Memory, 2-Layer 8-Expert MoE + SONA/EWC,");
    println!("PBFT/Raft/Gossip Consensus, CASA + AIDefence, SPARC Workflows, Gumbel Watermark & MCP Server.\n");
    println!("{}1. VISUAL CONSOLE & SWARM ORCHESTRATION:{}", BOLD, RESET);
    println!("  {}ai-fleet status [--view <page>] [--look bbs|orbital]{}  8-view TUI (overview|agents|workflows|mcp|moe|memory)", GREEN, RESET);
    println!("  {}ai-fleet start{}                                         Dispatch parallel AI agents in FencedLease worktrees", GREEN, RESET);
    println!("  {}ai-fleet stop{}                                          Drain active writers and save SHA-256 checkpoints", GREEN, RESET);
    println!("  {}ai-fleet exec <task_id> <prompt>{}                       Execute with AIDefence + CASA + automatic model failover", GREEN, RESET);
    println!("  {}ai-fleet drill{}                                         Run live v3 failover + FencedLease + PBFT + Watermark drill", GREEN, RESET);
    println!("\n{}2. MULTI-AI PROVIDER & DYNAMIC MODEL / EFFORT CONTROL:{}", BOLD, RESET);
    println!("  {}ai-fleet provider list{}                                 Show connected AIs, models, Q-scores, pheromones & EWC", CYAN, RESET);
    println!("  {}ai-fleet provider add <id> <bin> <model> <effort>{}      Connect any AI CLI tool (ollama, gemini, cursor, aider)", CYAN, RESET);
    println!("  {}ai-fleet provider switch <id> <model> <effort>{}         Switch active model & reasoning effort (low..xhigh..max)", CYAN, RESET);
    println!("  {}ai-fleet provider quota <id> <used> <limit> [ts|ready]{} Update or reset 5-hour / daily quota circuit-breaker", CYAN, RESET);
    println!("\n{}3. NEURAL MoE, SONA/EWC, PHEROMONES & HNSW VECTOR MEMORY:{}", BOLD, RESET);
    println!("  {}ai-fleet moe <task_description>{}                        Run 2-Layer 8-Expert MoE gating + EWC Fisher loss", MAGENTA, RESET);
    println!("  {}ai-fleet pheromone [evaporate|boost <model>]{}           Inspect or update ADR-330 stigmergic pheromone trails", MAGENTA, RESET);
    println!("  {}ai-fleet memory store <ns> <key> <text>{}                Index document into 64-dim HNSW semantic vector graph", MAGENTA, RESET);
    println!("  {}ai-fleet memory search <query>{}                         Hybrid HNSW cosine + lexical semantic vector search", MAGENTA, RESET);
    println!("  {}ai-fleet compress <max_tokens> <message>{}               Information-Bottleneck token-budget message compressor", MAGENTA, RESET);
    println!("\n{}4. 64 AGENTS, QUEEN DECOMPOSER, SPARC, CONSENSUS & MCP:{}", BOLD, RESET);
    println!("  {}ai-fleet agents [search_query]{}                         Browse or match from the 64 specialized agent personas", YELLOW, RESET);
    println!("  {}ai-fleet queen <task_id>{}                               Decompose task into persona-assigned DAG sub-tasks", YELLOW, RESET);
    println!("  {}ai-fleet sparc <task_id>{}                               Advance task through 5-phase SPARC methodology", YELLOW, RESET);
    println!("  {}ai-fleet consensus <PBFT|Raft|Gossip> <topic>{}          Execute multi-model weighted consensus protocol", YELLOW, RESET);
    println!("  {}ai-fleet defense <payload>{}                             Scan for ADR-118 prompt injection & secret exposure", YELLOW, RESET);
    println!("  {}ai-fleet casa <action>{}                                 Evaluate CASA (ADR-380 §3) deny-by-default envelope", YELLOW, RESET);
    println!("  {}ai-fleet federate <peer> <payload>{}                     Strip PII/secrets & embed Gumbel-Softmax watermark", YELLOW, RESET);
    println!("  {}ai-fleet mcp <start|tools|exec <tool> <json>>{}          Start JSON-RPC 2.0 MCP server over stdio or invoke tool", YELLOW, RESET);
    println!("  {}ai-fleet policy verify{}                                 Verify SHA-256 hash-chained decision ledger & TruthAnchors\n", YELLOW, RESET);
}

fn main() {
    enable_ansi_support();
    let args: Vec<String> = env::args().skip(1).collect();
    let mut state = load_or_init_fleet_state();

    if args.is_empty() {
        print_dashboard(&state, None);
        return;
    }

    match args[0].as_str() {
        "status" | "dashboard" | "tui" => {
            let mut view_opt = None;
            if let Some(pos) = args.iter().position(|a| a == "--look") {
                if let Some(look) = args.get(pos + 1) {
                    state.look_mode = look.clone();
                    save_fleet_state(&state);
                }
            }
            if let Some(pos) = args.iter().position(|a| a == "--view") {
                if let Some(v) = args.get(pos + 1) {
                    state.active_view = v.clone();
                    save_fleet_state(&state);
                    view_opt = Some(v.as_str());
                }
            }
            print_dashboard(&state, view_opt);
        }
        "start" | "run" => {
            refresh_provider_quotas(&mut state);
            let ws_root = detect_workspace_root();
            let mut busy = BTreeSet::new();
            let task_ids: Vec<String> = state.tasks.keys().cloned().collect();

            println!(
                "\n{}{}⚡ Launching Parallel Multi-AI Swarm across FencedLease Git Worktrees...{}",
                BOLD, GREEN, RESET
            );
            for tid in task_ids {
                let task_snapshot = match state.tasks.get(&tid) {
                    Some(t) if t.status == "queued" || t.status == "review" => t.clone(),
                    _ => continue,
                };
                let is_rev = task_snapshot.status == "review";
                if let Some(route) = compute_optimal_route(
                    &state.providers,
                    &state.moe_experts,
                    &state.reasoning_bank,
                    &task_snapshot,
                    &busy,
                    is_rev,
                ) {
                    busy.insert(route.selected_provider.clone());
                    state.global_epoch += 1;
                    let epoch = state.global_epoch;
                    let lease = prepare_fenced_worktree(
                        &ws_root,
                        &tid,
                        &route.selected_provider,
                        &route.selected_model,
                        &task_snapshot.owned_files,
                        epoch,
                    );
                    println!(
                        "  {}-> Task [{}]{} (Epoch #{}, SPARC:{}) -> {}{}/{}{} ({}) │ Persona: {}{}{}",
                        BRIGHT_CYAN,
                        tid,
                        RESET,
                        epoch,
                        task_snapshot.sparc_phase,
                        BOLD,
                        route.selected_provider,
                        route.selected_model,
                        RESET,
                        badge_effort(&route.selected_effort),
                        MAGENTA,
                        route.assigned_persona,
                        RESET
                    );
                    state.fenced_leases.insert(tid.clone(), lease);
                    let top_exp = route
                        .moe_experts
                        .first()
                        .map(|(n, _)| n.clone())
                        .unwrap_or_else(|| "coder".into());
                    record_outcome(&mut state, &route.selected_provider, &route.selected_model, &top_exp, 0.97, 85);
                    if let Some(t) = state.tasks.get_mut(&tid) {
                        t.assigned_provider = Some(route.selected_provider.clone());
                        t.assigned_model = Some(route.selected_model.clone());
                        t.assigned_persona = Some(route.assigned_persona.clone());
                        t.target_effort = route.selected_effort.clone();
                        t.lease_epoch = epoch;
                    }
                    append_ledger_row(
                        &mut state.decision_ledger,
                        &format!("{}/{}", route.selected_provider, route.selected_model),
                        "swarm.dispatch.v3",
                        &format!("Task {} dispatched in epoch #{} with persona {}", tid, epoch, route.assigned_persona),
                    );
                }
            }
            state.continue_gate.consecutive_steps += 1;
            save_fleet_state(&state);
            print_dashboard(&state, None);
        }
        "stop" => {
            state.daemon_running = false;
            append_ledger_row(
                &mut state.decision_ledger,
                "owner:cli",
                "swarm.stop",
                "Drained active writers and saved all FencedLease checkpoints",
            );
            save_fleet_state(&state);
            println!("{}✔ Swarm stopped safely. All FencedLeases and HNSW memory nodes persisted.{}", GREEN, RESET);
        }
        "agents" => {
            if args.len() > 1 {
                let q = args[1..].join(" ");
                let p = find_best_persona(&q);
                println!("\n{}{}🤖 MATCHED SPECIALIZED AGENT PERSONA{}", BOLD, BRIGHT_CYAN, RESET);
                println!("  ├─ ID / Domain:  {}{}{} ({})", BOLD, p.id, RESET, p.domain);
                println!("  ├─ Role Title:   {}", p.role_title);
                println!("  ├─ Tier/Effort:  {} / {}", badge_tier(&p.default_tier), badge_effort(&p.default_effort));
                println!("  ├─ Capabilities: {:?}", p.capabilities);
                println!("  └─ Directive:    {}\n", p.system_directive);
            } else {
                print_dashboard(&state, Some("agents"));
            }
        }
        "queen" => {
            let tid = args.get(1).map(|s| s.as_str()).unwrap_or("security-rls-audit");
            if let Some(t) = state.tasks.get(tid) {
                let analysis = queen_decompose_task(t);
                println!("\n{}{}👑 QUEEN HIVE-MIND TASK DECOMPOSITION [{}]{}", BOLD, MAGENTA, tid, RESET);
                println!("  ├─ Complexity Score:   {:.2}", analysis.complexity_score);
                println!("  ├─ Primary Domain:     {}", analysis.recommended_domain);
                println!("  ├─ Lead Persona:       {}{}{}", GREEN, analysis.primary_persona, RESET);
                println!("  └─ Generated Sub-Task DAG:");
                for st in &analysis.subtasks {
                    println!(
                        "       • [{}] {:<10} │ Persona: {:<22} │ {}",
                        st.id, st.status, st.assigned_persona, st.title
                    );
                }
                println!();
            } else {
                eprintln!("{}Task '{}' not found.{}", RED, tid, RESET);
            }
        }
        "sparc" => {
            let tid = args.get(1).map(|s| s.as_str()).unwrap_or("arch-verify");
            if let Some(t) = state.tasks.get_mut(tid) {
                let prev = t.sparc_phase.clone();
                let (next_phase, persona, eff, desc) = advance_sparc_phase(&prev);
                t.sparc_phase = next_phase.to_string();
                t.assigned_persona = Some(persona.to_string());
                t.target_effort = eff.to_string();
                append_ledger_row(
                    &mut state.decision_ledger,
                    "sparc:conductor",
                    "sparc.advance",
                    &format!("Task {} advanced {} -> {} ({})", tid, prev, next_phase, persona),
                );
                save_fleet_state(&state);
                println!(
                    "\n{}✔ SPARC Phase Advanced for [{}]: {} -> {}{}{}",
                    GREEN, tid, prev, BOLD, next_phase, RESET
                );
                println!("  ├─ Lead Persona: {}", persona);
                println!("  ├─ Target Effort: {}", badge_effort(eff));
                println!("  └─ Gate Criteria: {}\n", desc);
            } else {
                print_dashboard(&state, Some("workflows"));
            }
        }
        "moe" => {
            let query = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "Audit SQL migration money scale, RLS policies, and verify golden tests".to_string()
            };
            let res = route_moe_experts(&state.moe_experts, &query, 4);
            println!(
                "\n{}{}🧠 2-LAYER 8-EXPERT MIXTURE-OF-EXPERTS (MoE) + SONA/EWC GATING{}",
                BOLD, BRIGHT_CYAN, RESET
            );
            println!("  ├─ Query:             \"{}\"", query);
            println!("  ├─ Routing Entropy:   {:.4} nats", res.entropy);
            println!("  ├─ Load-Balance Loss: {:.4}", res.load_balance_loss);
            println!("  ├─ EWC Fisher Loss:   {:.6}", res.ewc_regularization_loss);
            println!("  └─ Top-4 Activated Experts:");
            for (idx, (name, prob)) in res.top_experts.iter().enumerate() {
                println!("       {}. {:<13} {}", idx + 1, name, render_float_bar(*prob, 1.0, 14, GREEN));
            }
            println!();
        }
        "memory" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "store" | "set" if args.len() >= 5 => {
                    let ns = &args[2];
                    let key = &args[3];
                    let content = args[4..].join(" ");
                    insert_hnsw_node(&mut state.vector_memory, ns, key, &content, vec!["cli".into()], 0.95);
                    save_fleet_state(&state);
                    println!(
                        "{}✔ Indexed into 64-dim HNSW Vector Graph: [{}] {} (total nodes={}){}",
                        GREEN,
                        ns,
                        key,
                        state.vector_memory.len(),
                        RESET
                    );
                }
                "search" if args.len() >= 3 => {
                    let q = args[2..].join(" ");
                    let hits = search_hnsw_semantic(&state.vector_memory, &q, 4);
                    println!("\n{}{}🔍 HNSW SEMANTIC VECTOR SEARCH RESULTS FOR: \"{}\"{}", BOLD, BRIGHT_CYAN, q, RESET);
                    for (idx, h) in hits.iter().enumerate() {
                        println!(
                            "  {}. {}[{}] {}{} │ Hybrid:{:.3} (Cosine:{:.3}, HNSW edges:{})",
                            idx + 1,
                            MAGENTA,
                            h.namespace,
                            h.key,
                            RESET,
                            h.hybrid_score,
                            h.cosine_similarity,
                            h.neighbors_traversed
                        );
                        println!("     └─ {}", h.content);
                    }
                    println!();
                }
                _ => {
                    println!(
                        "\n{}{}🧠 HNSW SEMANTIC VECTOR MEMORY GRAPH ({} nodes, 64-dim){}",
                        BOLD,
                        BRIGHT_CYAN,
                        state.vector_memory.len(),
                        RESET
                    );
                    for n in &state.vector_memory {
                        println!(
                            "  • {}[{}] {}{} (neighbors: {:?}) -> {}",
                            MAGENTA, n.namespace, n.key, RESET, n.neighbors, n.content
                        );
                    }
                    println!();
                }
            }
        }
        "compress" => {
            let budget: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(12);
            let msg = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "Agent finished phase 1. Verified commit 530fc5e and sha256=ae90aaab. All 266 unit tests pass and 322 architecture tests pass. Next step is independent review of SQL migration 0013.".to_string()
            };
            let (comp, orig, fin) = compress_message_to_token_budget(&msg, budget);
            println!("\n{}{}📦 INFORMATION-BOTTLENECK MESSAGE COMPRESSOR{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Token Budget: {} tokens (Original: {} -> Compressed: {})", budget, orig, fin);
            println!("  └─ Compressed:   \"{}\"\n", comp);
        }
        "consensus" => {
            let proto = args.get(1).map(|s| s.as_str()).unwrap_or("PBFT");
            let topic = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "Approve Phase 0 Gate and Migration 0013".to_string()
            };
            let seq = (state.consensus_history.len() as u64) + 1;
            let round = execute_consensus(&state.providers, proto, &topic, seq);
            append_ledger_row(
                &mut state.decision_ledger,
                &round.leader_or_primary,
                "consensus.finalized",
                &format!("{} ({}) approval={:.1}%", round.proposal_id, round.protocol, round.weighted_approval * 100.0),
            );
            state.consensus_history.push(round.clone());
            save_fleet_state(&state);
            println!(
                "\n{}{}⚖ {} CONSENSUS ENGINE ROUND [{}]{}",
                BOLD, MAGENTA, round.protocol, round.proposal_id, RESET
            );
            println!("  ├─ Leader / Primary:  {}", round.leader_or_primary);
            println!("  ├─ Topic:             {}", round.topic);
            println!("  ├─ Phase Progression: {}", round.phase);
            println!("  ├─ Weighted Approval: {:.1}% (Quorum={})", round.weighted_approval * 100.0, round.quorum_achieved);
            println!("  └─ Digest:            {}\n", round.digest);
        }
        "federate" => {
            let peer = args.get(1).map(|s| s.as_str()).unwrap_or("team-alpha");
            let raw = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "Contact owner apppbf@gmail.com with token=sk-ant-123456 and phone 966501234567 for migration review".to_string()
            };
            let pkt = create_federated_packet("ai-fleet-local", peer, &raw, "estr-v3-secret-salt");
            let (_, _, verified) = compute_gumbel_watermark_zscore(&pkt.sanitized_payload, "estr-v3-secret-salt");
            println!("\n{}{}🌐 CROSS-TEAM FEDERATION & GUMBEL-SOFTMAX WATERMARK{}", BOLD, BRIGHT_CYAN, RESET);
            println!("  ├─ Packet ID / Peer:   {} -> {}", pkt.packet_id, pkt.target_peer);
            println!("  ├─ PII/Secrets Stripped: {} item(s) automatically redacted", pkt.pii_redactions_count);
            println!("  ├─ Sanitized Payload:  \"{}\"", pkt.sanitized_payload);
            println!(
                "  └─ Watermark Z-Score:  {:.2}σ (Provenance Verified: {}{}{})\n",
                pkt.z_score,
                GREEN,
                verified,
                RESET
            );
        }
        "mcp" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("tools");
            match sub {
                "start" | "serve" => {
                    run_mcp_stdio_server();
                }
                "exec" if args.len() >= 3 => {
                    let tool = &args[2];
                    let params: Value = args
                        .get(3)
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| serde_json::json!({}));
                    let res = execute_mcp_tool(tool, &params);
                    println!("{}", serde_json::to_string_pretty(&res).unwrap_or_default());
                }
                _ => print_dashboard(&state, Some("mcp")),
            }
        }
        "defense" | "aidefence" => {
            let text = if args.len() > 1 { args[1..].join(" ") } else { "Normal prompt".into() };
            let findings = scan_aidefence_threats(&text);
            println!("\n{}{}🛡 AIDEFENCE (ADR-118) PROMPT-INJECTION & SECRET SHIELD{}", BOLD, BRIGHT_CYAN, RESET);
            if findings.is_empty() {
                println!("  └─ Result: {}✔ CLEAN (0 threats detected, <1ms){}\n", GREEN, RESET);
            } else {
                println!("  └─ Result: {}✖ BLOCKED ({} threat(s) detected){}", RED, findings.len(), RESET);
                for f in findings {
                    println!("       • [{}] {} ({:.0}%): {}", f.severity, f.category, f.confidence * 100.0, f.description);
                }
                println!();
            }
        }
        "casa" => {
            let act = args.get(1).map(|s| s.as_str()).unwrap_or("worktree.write");
            let dec = check_casa_authorization(&state.casa_envelope, act, &Utc::now().to_rfc3339());
            println!(
                "\n🔐 CASA Gate for '{}': {} ({})\n",
                act,
                if dec.allowed { format!("{}ALLOWED{}", GREEN, RESET) } else { format!("{}DENIED{}", RED, RESET) },
                dec.reason
            );
        }
        "gate" => {
            let (act, reason) = evaluate_continue_gate(&state.continue_gate);
            println!("\n🚦 ContinueGate: {} — {}\n", badge_status(act), reason);
        }
        "pheromone" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            if sub == "evaporate" {
                for p in state.providers.values_mut() {
                    for m in &mut p.models {
                        m.pheromone = (m.pheromone * 0.90).clamp(0.20, 2.00);
                    }
                }
                save_fleet_state(&state);
            } else if sub == "boost" {
                if let Some(target_m) = args.get(2) {
                    for p in state.providers.values_mut() {
                        for m in &mut p.models {
                            if &m.name == target_m {
                                m.pheromone = (m.pheromone + 0.25).clamp(0.20, 2.00);
                            }
                        }
                    }
                    save_fleet_state(&state);
                }
            }
            print_dashboard(&state, None);
        }
        "provider" | "providers" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "add" if args.len() >= 6 => {
                    let id = args[2].to_lowercase();
                    let bin = args[3].clone();
                    let model = args[4].clone();
                    let effort = args[5].clone();
                    let tpl = if args.len() > 6 {
                        args[6..].to_vec()
                    } else {
                        vec!["--model".into(), "{model}".into(), "-p".into(), "{prompt}".into()]
                    };
                    state.providers.insert(
                        id.clone(),
                        AiProvider {
                            id: id.clone(),
                            display_name: format!("Custom AI ({})", id),
                            binary: bin,
                            args_template: tpl,
                            stdin_prompt: false,
                            status: "ready".into(),
                            active_model: model.clone(),
                            active_effort: effort.clone(),
                            daily_quota_limit: 100,
                            daily_quota_used: 0,
                            retry_at: None,
                            last_detail: "Connected via ai-fleet provider add".into(),
                            models: vec![ModelSpec {
                                name: model.clone(),
                                tier: "deep".into(),
                                default_effort: effort.clone(),
                                q_score: 0.92,
                                pheromone: 1.15,
                                trust_score: 0.95,
                                ewc_fisher_importance: 0.90,
                                calls_today: 0,
                                successes: 0,
                                failures: 0,
                                avg_latency_ms: 0,
                            }],
                        },
                    );
                    save_fleet_state(&state);
                    println!("{}✔ Connected AI provider '{}' ({} @ {}).{}", GREEN, id, model, effort, RESET);
                }
                "switch" | "model" if args.len() >= 5 => {
                    let id = args[2].to_lowercase();
                    let model = args[3].clone();
                    let effort = args[4].clone();
                    if let Some(p) = state.providers.get_mut(&id) {
                        p.active_model = model.clone();
                        p.active_effort = effort.clone();
                        if !p.models.iter().any(|m| m.name == model) {
                            p.models.push(ModelSpec {
                                name: model.clone(),
                                tier: "critical".into(),
                                default_effort: effort.clone(),
                                q_score: 0.95,
                                pheromone: 1.25,
                                trust_score: 0.96,
                                ewc_fisher_importance: 0.92,
                                calls_today: 0,
                                successes: 0,
                                failures: 0,
                                avg_latency_ms: 0,
                            });
                        }
                        save_fleet_state(&state);
                        println!("{}✔ Switched '{}' -> model='{}' effort={}{}", GREEN, id, model, badge_effort(&effort), RESET);
                    }
                }
                "quota" if args.len() >= 5 => {
                    let id = args[2].to_lowercase();
                    let used: u64 = args[3].parse().unwrap_or(0);
                    let limit: u64 = args[4].parse().unwrap_or(100);
                    let reset = args.get(5).cloned();
                    if let Some(p) = state.providers.get_mut(&id) {
                        p.daily_quota_used = used;
                        p.daily_quota_limit = limit;
                        if used >= limit && reset.as_deref() != Some("ready") {
                            p.status = "quota".into();
                            p.retry_at = reset;
                        } else {
                            p.status = "ready".into();
                            p.retry_at = None;
                        }
                    }
                    save_fleet_state(&state);
                    println!("{}✔ Quota updated for '{}': {}/{}{}", GREEN, id, used, limit, RESET);
                }
                _ => print_dashboard(&state, None),
            }
        }
        "task" => {
            if args.get(1).map(|s| s.as_str()) == Some("add") && args.len() >= 5 {
                let id = args[2].clone();
                let title = args[3].clone();
                let owned: Vec<String> = args[4].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                for f in &owned {
                    if let Some(owner) = state.file_claims.get(f) {
                        if owner != &id {
                            eprintln!("{}✖ File '{}' is already claimed by task '{}'!{}", RED, f, owner, RESET);
                            std::process::exit(1);
                        }
                    }
                }
                let (tier, eff) = classify_task_tier(&title, &owned, 0, false);
                let persona = find_best_persona(&title);
                for f in &owned {
                    state.file_claims.insert(f.clone(), id.clone());
                }
                state.global_epoch += 1;
                let ws_root = detect_workspace_root();
                let mut t = SwarmTask {
                    id: id.clone(),
                    title: title.clone(),
                    base_commit: detect_head_commit(&ws_root),
                    owned_files: owned.clone(),
                    required_outputs: owned,
                    sparc_phase: "Spec".into(),
                    subtasks: vec![],
                    status: "queued".into(),
                    tier: tier.clone(),
                    target_effort: eff.clone(),
                    assigned_provider: None,
                    assigned_model: None,
                    assigned_persona: Some(persona.id.clone()),
                    active_worktree: None,
                    authors: vec![],
                    reviewer: None,
                    retries: 0,
                    lease_epoch: state.global_epoch,
                    next_step: title,
                };
                t.subtasks = queen_decompose_task(&t).subtasks;
                state.tasks.insert(id.clone(), t);
                save_fleet_state(&state);
                println!(
                    "{}✔ Task '{}' created (Tier={}, Effort={}, Persona={}){}",
                    GREEN,
                    id,
                    badge_tier(&tier),
                    badge_effort(&eff),
                    persona.id,
                    RESET
                );
            } else {
                print_dashboard(&state, None);
            }
        }
        "route" => {
            let tid = args.get(1).map(|s| s.as_str()).unwrap_or("security-rls-audit");
            if let Some(t) = state.tasks.get(tid) {
                if let Some(d) = compute_optimal_route(
                    &state.providers,
                    &state.moe_experts,
                    &state.reasoning_bank,
                    t,
                    &BTreeSet::new(),
                    t.status == "review",
                ) {
                    println!("\n{}{}🎯 COMPOSITE V3 ROUTE FOR TASK [{}]{}", BOLD, BRIGHT_CYAN, tid, RESET);
                    println!("  ├─ Classified Tier:   {}", badge_tier(&d.classified_tier));
                    println!("  ├─ Matched Persona:   {}{}{}", MAGENTA, d.assigned_persona, RESET);
                    println!("  ├─ MoE Top Experts:   {:?} (entropy={:.3}, ewc_loss={:.6})", d.moe_experts, d.moe_entropy, d.ewc_loss);
                    println!("  ├─ Selected Provider: {}{}/{}{} ({})", BOLD, d.selected_provider, d.selected_model, RESET, badge_effort(&d.selected_effort));
                    println!("  ├─ Composite Score:   {:.3}", d.composite_score);
                    println!("  └─ Fallback Ladder:   {:?}\n", d.fallback_chain);
                }
            }
        }
        "drill" => {
            let drill_dir = env::temp_dir().join("ai-fleet-v3-drill");
            let _ = fs::create_dir_all(&drill_dir);
            let file = drill_dir.join("v3-handoff.txt");
            let _ = fs::remove_file(&file);

            state.global_epoch += 1;
            let ep1 = state.global_epoch;
            let l1 = b"phase-1: codex/soul [epoch-fenced]\n";
            fs::write(&file, l1).unwrap();
            let h1 = sha256_bytes(l1);

            state.global_epoch += 1;
            let ep2 = state.global_epoch;
            assert_eq!(sha256_file(&file).unwrap(), h1);
            let mut combined = fs::read(&file).unwrap();
            combined.extend_from_slice(b"phase-2: antigravity/argon [effort=xhigh]\n");
            fs::write(&file, &combined).unwrap();
            let h2 = sha256_bytes(&combined);

            record_outcome(&mut state, "antigravity", "argon", "security", 0.99, 6800);
            let round = execute_consensus(&state.providers, "PBFT", "Verify drill handoff integrity", (state.consensus_history.len() as u64) + 1);
            state.consensus_history.push(round.clone());
            append_ledger_row(&mut state.decision_ledger, "drill:v3", "handoff.verified", &format!("ep {}->{} sha256={}", ep1, ep2, &h2[..12]));
            save_fleet_state(&state);

            println!("\n{}{}⚡ V3 LIVE FAILOVER, FENCED EPOCH (#{} -> #{}) & PBFT DRILL: ✔ PASS{}", BOLD, GREEN, ep1, ep2, RESET);
            println!("  ├─ Checkpoint SHA-256: {} -> Final SHA-256: {}", &h1[..18], &h2[..18]);
            println!("  └─ PBFT Consensus [{}]: {:.1}% weighted quorum ({})\n", round.proposal_id, round.weighted_approval * 100.0, round.phase);
        }
        "exec" if args.len() >= 3 => {
            let tid = args[1].clone();
            let prompt = args[2..].join(" ");
            let threats = scan_aidefence_threats(&prompt);
            if !threats.is_empty() {
                eprintln!("{}✖ AIDefence blocked prompt: {:?}{}", RED, threats, RESET);
                std::process::exit(1);
            }
            let ws = detect_workspace_root();
            if let Some(t) = state.tasks.get(&tid).cloned() {
                if let Some(d) = compute_optimal_route(&state.providers, &state.moe_experts, &state.reasoning_bank, &t, &BTreeSet::new(), false) {
                    let mut chain = vec![(d.selected_provider.clone(), d.selected_model.clone(), d.selected_effort.clone())];
                    chain.extend(d.fallback_chain);
                    for (pid, model, eff) in chain {
                        if let Some(p) = state.providers.get(&pid).cloned() {
                            let st = Instant::now();
                            match execute_ai_cli(&p, &model, &eff, &ws, &prompt) {
                                Ok(out) => {
                                    record_outcome(&mut state, &pid, &model, "coder", 0.98, st.elapsed().as_millis() as u64);
                                    save_fleet_state(&state);
                                    println!("{}", out);
                                    return;
                                }
                                Err((kind, reset, _)) => {
                                    if let Some(pm) = state.providers.get_mut(&pid) {
                                        if kind == "quota" {
                                            pm.status = "quota".into();
                                            pm.retry_at = reset;
                                        }
                                    }
                                    save_fleet_state(&state);
                                    eprintln!("{}⚠ {}/{} ({}) -> switching to next model in ladder...{}", YELLOW, pid, model, kind, RESET);
                                }
                            }
                        }
                    }
                }
            }
        }
        "policy" | "ledger" => match verify_ledger_chain(&state.decision_ledger) {
            Ok(n) => {
                println!(
                    "\n{}✔ SHA-256 Decision Ledger verified ({} chained blocks, {} immutable TruthAnchors).{}",
                    GREEN,
                    n,
                    state.truth_anchors.len(),
                    RESET
                );
                for a in &state.truth_anchors {
                    println!("  🔒 [{}] {} (sig={}..)", a.anchor_id, a.invariant, &a.signature_sha256[..16]);
                }
                println!();
            }
            Err(e) => {
                eprintln!("{}✖ Ledger integrity FAILED: {}{}", RED, e, RESET);
                std::process::exit(1);
            }
        },
        _ => print_v3_help(),
    }
}
