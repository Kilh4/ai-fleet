//! Queen Hive-Mind Epic Decomposer & Topological Wave Planner.
//!
//! Turns a single high-level epic into a cycle-free, file-scope-disjoint
//! Directed Acyclic Graph (DAG) of `NewTask`s organized into parallel execution
//! waves, using either a live AI planner or deterministic architectural slicing
//! backed by [`CodeGraph`] and [`SkillRegistry`].

use crate::error::{EngineError, Result};
use crate::intelligence::CodeGraph;
use crate::model::{infer_complexity, Complexity, NewTask};
use crate::router::Role;
use crate::skills::SkillRegistry;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedNode {
    pub key: String,
    pub title: String,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub owned_files: Vec<String>,
    #[serde(default)]
    pub complexity: Option<Complexity>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub wave: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpicPlan {
    pub epic: String,
    pub planner_source: String,
    pub detected_verify_cmd: Option<String>,
    pub waves: usize,
    pub auto_serialized_edges: Vec<(String, String, String)>,
    pub nodes: Vec<PlannedNode>,
}

/// Detect the repository's native build/test command when `--verify` is omitted.
pub fn detect_repo_verify_cmd(repo: &Path) -> Option<String> {
    if repo.join("Cargo.toml").is_file() {
        return Some("cargo check --quiet".into());
    }
    if let Ok(entries) = std::fs::read_dir(repo) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("sln") {
                return Some(format!("dotnet build {}", e.file_name().to_string_lossy()));
            }
        }
    }
    if repo.join("package.json").is_file() {
        return Some("npm test".into());
    }
    if repo.join("pyproject.toml").is_file() || repo.join("pytest.ini").is_file() {
        return Some("pytest -q".into());
    }
    if repo.join("go.mod").is_file() {
        return Some("go test ./...".into());
    }
    None
}

/// Build the structured planning prompt sent to a live AI Queen provider.
pub fn queen_planning_prompt(epic: &str, graph: &CodeGraph, skills: &SkillRegistry, max_tasks: usize) -> String {
    let graph_map = graph.render_prompt_map(&[], 4000);
    let top_skills = skills.select(epic, &[], Role::Author, &[], 6);
    let skill_list = top_skills
        .iter()
        .map(|s| format!("- `{}` ({})", s.id, s.domain))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "# Queen Architectural Decomposition\n\n\
Decompose the epic below into 2..={max_tasks} concrete, non-overlapping engineering tasks forming a valid DAG.\n\
Do NOT modify any files. Output ONLY a JSON object inside a ```json fenced block.\n\n\
## Epic\n{epic}\n\
{graph_map}\n\
## Candidate Domain Skills\n{skill_list}\n\n\
## Rules\n\
1. Parallel tasks in the same wave MUST own disjoint file globs (`owned_files`) so worktrees merge cleanly.\n\
2. Foundational schema / domain contracts go first (`depends_on: []`); parallel implementation slices depend on contracts; final integration/verification depends on slices.\n\
3. Output schema:\n\
```json\n\
{{\n\
  \"nodes\": [\n\
    {{\n\
      \"key\": \"contracts\",\n\
      \"title\": \"Define domain contracts and schema\",\n\
      \"prompt\": \"Detailed actionable instructions...\",\n\
      \"depends_on\": [],\n\
      \"owned_files\": [\"src/domain/**\"],\n\
      \"complexity\": \"deep\",\n\
      \"skills\": [\"hexagonal-architecture\"]\n\
    }}\n\
  ]\n\
}}\n\
```\n"
    )
}

#[derive(Deserialize)]
struct RawPlan {
    nodes: Vec<PlannedNode>,
}

/// Extract a JSON `EpicPlan` from an AI planner's response text.
pub fn parse_ai_plan_json(epic: &str, source: &str, text: &str, verify_cmd: Option<String>) -> Result<EpicPlan> {
    let json_slice = extract_json_object(text).ok_or_else(|| EngineError::Config("Queen response did not contain a JSON plan".into()))?;
    let raw: RawPlan = serde_json::from_str(json_slice).map_err(|e| EngineError::Config(format!("invalid Queen JSON: {e}")))?;
    if raw.nodes.is_empty() {
        return Err(EngineError::Config("Queen produced 0 tasks".into()));
    }
    validate_and_schedule_dag(epic, source, raw.nodes, verify_cmd)
}

fn extract_json_object(text: &str) -> Option<&str> {
    if let Some(fence) = text.find("```json") {
        let rest = &text[fence + 7..];
        if let Some(end) = rest.find("```") {
            return Some(rest[..end].trim());
        }
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then_some(&text[start..=end])
}

/// Deterministic architectural decomposition using the repository's real
/// `CodeGraph` layout and matched `SkillRegistry` skills. Used when `--offline`
/// is set or when no live planner is invoked.
pub fn decompose_architecturally(
    epic: &str,
    repo: &Path,
    graph: &CodeGraph,
    skills: &SkillRegistry,
    verify_cmd: Option<String>,
) -> Result<EpicPlan> {
    let verify = verify_cmd.or_else(|| detect_repo_verify_cmd(repo));
    let base_comp = infer_complexity(epic);
    let matched = skills.select(epic, &[], Role::Author, &[], 5);
    let skill_ids: Vec<String> = matched.iter().map(|m| m.id.clone()).collect();

    // Discover real top-level source directories in the repo so owned_files match real layout.
    let mut dirs: BTreeMap<String, usize> = BTreeMap::new();
    for path in graph.nodes.keys() {
        if let Some((top, _)) = path.split_once('/') {
            *dirs.entry(top.to_string()).or_insert(0) += 1;
        }
    }
    let has_sql = graph.languages.contains_key("sql")
        || epic.to_lowercase().contains("sql")
        || epic.to_lowercase().contains("migration")
        || epic.contains("محاسبة")
        || epic.contains("جدول");
    let has_ui = graph.languages.contains_key("typescript")
        || dirs.contains_key("web")
        || dirs.contains_key("frontend")
        || dirs.contains_key("ui")
        || epic.to_lowercase().contains("ui")
        || epic.contains("واجهة")
        || epic.contains("تصميم");

    let mut nodes = Vec::new();

    // Wave 0: Domain Contracts / Schema Foundation
    let contract_scope = if has_sql && repo.join("migrations").is_dir() {
        vec!["migrations/**".into(), "docs/specs/**".into()]
    } else {
        vec!["docs/specs/**".into()]
    };
    nodes.push(PlannedNode {
        key: "contracts-foundation".into(),
        title: format!("Contracts, Invariants & Architecture Spec — {}", short_title(epic)),
        prompt: format!(
            "Establish the foundational domain contracts, data invariants, edge-case matrix, and specification for:\n\n{epic}\n\n\
Document exact types, error boundaries, and verification criteria inside the owned specification/migration paths so downstream parallel agents implement against an unambiguous contract."
        ),
        depends_on: vec![],
        owned_files: contract_scope,
        complexity: Some(base_comp),
        skills: skill_ids.iter().take(2).cloned().collect(),
        wave: 0,
    });

    // Wave 1 (Parallel Slice A): Core Domain & Backend Logic
    let core_scope = if dirs.contains_key("crates") {
        vec!["crates/**".into()]
    } else if dirs.contains_key("src") {
        vec!["src/**".into()]
    } else {
        vec!["backend/**".into(), "src/**".into()]
    };
    let mut upstream_for_verify = vec!["core-implementation".to_string()];
    nodes.push(PlannedNode {
        key: "core-implementation".into(),
        title: format!("Core Engine & Domain Implementation — {}", short_title(epic)),
        prompt: format!(
            "Implement the core domain logic, algorithms, and error handling defined in `contracts-foundation` for:\n\n{epic}\n\n\
Enforce strict type safety, exact numeric precision (never binary floats for money), and clean module boundaries."
        ),
        depends_on: vec!["contracts-foundation".into()],
        owned_files: core_scope,
        complexity: Some(base_comp),
        skills: skill_ids.iter().take(3).cloned().collect(),
        wave: 1,
    });

    // Wave 1 (Parallel Slice B, when UI/API surface exists): Interface / Frontend Layer
    if has_ui {
        let ui_scope = if dirs.contains_key("web") {
            vec!["web/**".into()]
        } else if dirs.contains_key("frontend") {
            vec!["frontend/**".into()]
        } else {
            vec!["ui/**".into(), "public/**".into()]
        };
        upstream_for_verify.push("interface-surface".into());
        let ui_skills: Vec<String> = skills
            .select(&format!("{epic} ui ux design accessibility"), &["App.tsx".into()], Role::Author, &[], 2)
            .into_iter()
            .map(|m| m.id)
            .collect();
        nodes.push(PlannedNode {
            key: "interface-surface".into(),
            title: format!("Interface, UX & Client Integration — {}", short_title(epic)),
            prompt: format!(
                "Implement the user-facing interface / client integration layer matching `contracts-foundation` for:\n\n{epic}\n\n\
Ensure responsive layout, clear loading/error states, keyboard accessibility, and strict contract alignment."
            ),
            depends_on: vec!["contracts-foundation".into()],
            owned_files: ui_scope,
            complexity: Some(Complexity::Standard),
            skills: ui_skills,
            wave: 1,
        });
    }

    // Final Wave: End-to-End Tests & Security/Regression Hardening
    let audit_skills: Vec<String> = skills
        .select(&format!("{epic} test verification security audit"), &[], Role::Reviewer, &[], 2)
        .into_iter()
        .map(|m| m.id)
        .collect();
    nodes.push(PlannedNode {
        key: "verification-hardening".into(),
        title: format!("Integration Tests & Security Hardening — {}", short_title(epic)),
        prompt: format!(
            "Write comprehensive integration, edge-case, and regression tests covering all slices of:\n\n{epic}\n\n\
Verify boundary conditions, failure recovery, and security/audit invariants."
        ),
        depends_on: upstream_for_verify,
        owned_files: vec!["tests/**".into()],
        complexity: Some(Complexity::Deep),
        skills: audit_skills,
        wave: 2,
    });

    validate_and_schedule_dag(epic, "queen-architectural-dag", nodes, verify)
}

fn short_title(epic: &str) -> String {
    let line = epic.lines().next().unwrap_or("Epic").trim();
    if line.chars().count() <= 46 {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(45).collect::<String>())
    }
}

/// Validates the DAG with Kahn's algorithm (rejecting cycles), automatically
/// serializes any concurrent tasks whose `owned_files` overlap, and computes
/// exact parallel wave indices (`0..waves`).
pub fn validate_and_schedule_dag(
    epic: &str,
    source: &str,
    mut nodes: Vec<PlannedNode>,
    verify_cmd: Option<String>,
) -> Result<EpicPlan> {
    let mut key_to_idx: HashMap<String, usize> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.key.trim().is_empty() {
            return Err(EngineError::Config("planned node has empty key".into()));
        }
        if key_to_idx.insert(n.key.clone(), i).is_some() {
            return Err(EngineError::Config(format!("duplicate planned node key `{}`", n.key)));
        }
    }
    for n in &nodes {
        for dep in &n.depends_on {
            if !key_to_idx.contains_key(dep) {
                return Err(EngineError::Config(format!("node `{}` depends on unknown key `{dep}`", n.key)));
            }
            if dep == &n.key {
                return Err(EngineError::Config(format!("node `{}` depends on itself", n.key)));
            }
        }
    }

    // 1. First Kahn pass to verify acyclicity and compute initial topological order.
    let order = topological_order(&nodes, &key_to_idx)?;

    // 2. Check pairwise file-scope overlap; if neither node reaches the other,
    //    add a dependency edge from earlier -> later so parallel worktrees never conflict!
    let mut reach = compute_reachability(&nodes, &order, &key_to_idx);
    let mut auto_edges: Vec<(String, String, String)> = Vec::new();

    for pos_a in 0..order.len() {
        for pos_b in (pos_a + 1)..order.len() {
            let ia = order[pos_a];
            let ib = order[pos_b];
            if reach[ia].contains(&ib) || reach[ib].contains(&ia) {
                continue;
            }
            if let Some(reason) = scopes_overlap(&nodes[ia].owned_files, &nodes[ib].owned_files) {
                let ka = nodes[ia].key.clone();
                let kb = nodes[ib].key.clone();
                nodes[ib].depends_on.push(ka.clone());
                auto_edges.push((ka, kb, reason));
                reach = compute_reachability(&nodes, &order, &key_to_idx);
            }
        }
    }

    // 3. Final Kahn pass to assign exact wave numbers (`wave(u) = max_{d in deps(u)} (wave(d) + 1)`).
    let final_order = topological_order(&nodes, &key_to_idx)?;
    let mut max_wave = 0usize;
    for &idx in &final_order {
        let w = nodes[idx]
            .depends_on
            .iter()
            .filter_map(|d| key_to_idx.get(d))
            .map(|&di| nodes[di].wave + 1)
            .max()
            .unwrap_or(0);
        nodes[idx].wave = w;
        max_wave = max_wave.max(w);
    }

    // Sort output nodes by `(wave, key)` for clean presentation and submission order.
    nodes.sort_by(|a, b| a.wave.cmp(&b.wave).then_with(|| a.key.cmp(&b.key)));

    Ok(EpicPlan {
        epic: epic.to_string(),
        planner_source: source.to_string(),
        detected_verify_cmd: verify_cmd,
        waves: if nodes.is_empty() { 0 } else { max_wave + 1 },
        auto_serialized_edges: auto_edges,
        nodes,
    })
}

fn topological_order(nodes: &[PlannedNode], key_to_idx: &HashMap<String, usize>) -> Result<Vec<usize>> {
    let n = nodes.len();
    let mut in_deg = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, node) in nodes.iter().enumerate() {
        for dep in &node.depends_on {
            let di = key_to_idx[dep];
            dependents[di].push(i);
            in_deg[i] += 1;
        }
    }
    let mut q: VecDeque<usize> = (0..n).filter(|&i| in_deg[i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(u) = q.pop_front() {
        order.push(u);
        for &v in &dependents[u] {
            in_deg[v] -= 1;
            if in_deg[v] == 0 {
                q.push_back(v);
            }
        }
    }
    if order.len() != n {
        return Err(EngineError::Config("Queen DAG contains a dependency cycle".into()));
    }
    Ok(order)
}

fn compute_reachability(nodes: &[PlannedNode], topo: &[usize], key_to_idx: &HashMap<String, usize>) -> Vec<HashSet<usize>> {
    // `reach[a]` contains `b` iff `b` transitively depends on `a`.
    let n = nodes.len();
    let mut reach: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for &idx in topo.iter().rev() {
        for dep in &nodes[idx].depends_on {
            let di = key_to_idx[dep];
            let inherited: Vec<usize> = reach[idx].iter().copied().collect();
            reach[di].insert(idx);
            reach[di].extend(inherited);
        }
    }
    reach
}

fn scope_prefix(pat: &str) -> String {
    let norm = pat.trim().replace('\\', "/");
    let norm = norm.trim_start_matches("./");
    match norm.find(['*', '?', '[', '{']) {
        Some(i) => norm[..i].trim_end_matches('/').to_string(),
        None => norm.trim_end_matches('/').to_string(),
    }
}

/// Returns `Some(reason)` if two `owned_files` sets can touch the same file.
pub fn scopes_overlap(a: &[String], b: &[String]) -> Option<String> {
    if a.is_empty() || b.is_empty() {
        return Some("unrestricted file scope".into());
    }
    for pa in a {
        let pfa = scope_prefix(pa);
        for pb in b {
            let pfb = scope_prefix(pb);
            if pfa.is_empty() || pfb.is_empty() || pfa == pfb || pfa.starts_with(&format!("{pfb}/")) || pfb.starts_with(&format!("{pfa}/")) {
                return Some(format!("overlapping scope `{pa}` <-> `{pb}`"));
            }
        }
    }
    None
}

impl PlannedNode {
    pub fn to_new_task(&self, resolved_deps: Vec<String>, verify_cmd: Option<String>, review: bool) -> NewTask {
        let mut nt = NewTask::new(self.title.clone(), self.prompt.clone());
        nt.depends_on = resolved_deps;
        nt.owned_files = self.owned_files.clone();
        nt.complexity = self.complexity;
        nt.verify_cmd = verify_cmd;
        nt.skills = self.skills.clone();
        nt.review = review;
        // Earlier waves get higher scheduling priority so critical path unlocks first!
        nt.priority = 100_i64.saturating_sub((self.wave as i64) * 10);
        nt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cycles_and_auto_serializes_overlapping_scopes() {
        let cyclic = vec![
            PlannedNode { key: "a".into(), title: "A".into(), prompt: "p".into(), depends_on: vec!["b".into()], owned_files: vec!["src/a/**".into()], complexity: None, skills: vec![], wave: 0 },
            PlannedNode { key: "b".into(), title: "B".into(), prompt: "p".into(), depends_on: vec!["a".into()], owned_files: vec!["src/b/**".into()], complexity: None, skills: vec![], wave: 0 },
        ];
        assert!(validate_and_schedule_dag("e", "test", cyclic, None).is_err());

        // Two independent nodes touching overlapping `src/ledger/**` and `src/**` -> auto-serialized!
        let overlapping = vec![
            PlannedNode { key: "n1".into(), title: "N1".into(), prompt: "p".into(), depends_on: vec![], owned_files: vec!["src/**".into()], complexity: None, skills: vec![], wave: 0 },
            PlannedNode { key: "n2".into(), title: "N2".into(), prompt: "p".into(), depends_on: vec![], owned_files: vec!["src/ledger/**".into()], complexity: None, skills: vec![], wave: 0 },
            PlannedNode { key: "n3".into(), title: "N3".into(), prompt: "p".into(), depends_on: vec![], owned_files: vec!["web/**".into()], complexity: None, skills: vec![], wave: 0 },
        ];
        let plan = validate_and_schedule_dag("e", "test", overlapping, None).unwrap();
        assert_eq!(plan.auto_serialized_edges.len(), 1);
        assert_eq!(plan.waves, 2);
        let n2 = plan.nodes.iter().find(|n| n.key == "n2").unwrap();
        let n3 = plan.nodes.iter().find(|n| n.key == "n3").unwrap();
        assert_eq!(n2.wave, 1, "n2 overlapped n1 so it moved to wave 1");
        assert_eq!(n3.wave, 0, "n3 in web/** stays parallel in wave 0");
    }
}
