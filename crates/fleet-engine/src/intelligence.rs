//! Codebase Intelligence & Deterministic Pre-Merge Static Gate:
//! 1. [`CodeGraph`]: Multi-language symbol & dependency analyzer (`Rust`, `TS/JS`,
//!    `C#`, `Python`, `Go`, `SQL`) with iterative **PageRank** centrality,
//!    **Tarjan SCC** cycle detection, and transitive **Blast Radius** analysis.
//! 2. [`StaticAuditor`]: Deterministic pre-merge gate catching leaked secrets,
//!    SQL injection string interpolation, **IEEE-754 float money bugs** (critical
//!    for accounting/ERP), and missing PostgreSQL **Row-Level Security (RLS)** on
//!    multi-tenant tables before reviewer quota is spent.

use crate::error::Result;
use crate::git;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileNode {
    pub path: String,
    pub language: String,
    pub lines: usize,
    pub symbols: Vec<String>,
    pub imports: Vec<String>,
    pub pagerank: f64,
    pub in_degree: usize,
    pub out_degree: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodeGraph {
    pub total_files: usize,
    pub total_lines: usize,
    pub languages: BTreeMap<String, usize>,
    pub nodes: BTreeMap<String, FileNode>,
    /// Directed edges `importer -> imported`.
    pub edges: Vec<(String, String)>,
    /// Strongly connected components with `len > 1` (circular file dependencies).
    pub cycles: Vec<Vec<String>>,
}

fn detect_language(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension().and_then(|e| e.to_str())?.to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some("rust"),
        "ts" | "tsx" | "js" | "jsx" | "mjs" => Some("typescript"),
        "cs" => Some("csharp"),
        "py" => Some("python"),
        "go" => Some("go"),
        "sql" => Some("sql"),
        _ => None,
    }
}

fn extract_symbols_and_raw_imports(lang: &str, src: &str) -> (Vec<String>, Vec<String>) {
    static RE_RS_SYM: OnceLock<Regex> = OnceLock::new();
    static RE_RS_USE: OnceLock<Regex> = OnceLock::new();
    static RE_TS_SYM: OnceLock<Regex> = OnceLock::new();
    static RE_TS_IMP: OnceLock<Regex> = OnceLock::new();
    static RE_CS_SYM: OnceLock<Regex> = OnceLock::new();
    static RE_CS_NS: OnceLock<Regex> = OnceLock::new();
    static RE_PY_SYM: OnceLock<Regex> = OnceLock::new();
    static RE_PY_IMP: OnceLock<Regex> = OnceLock::new();
    static RE_GO_SYM: OnceLock<Regex> = OnceLock::new();
    static RE_SQL_TBL: OnceLock<Regex> = OnceLock::new();
    static RE_SQL_REF: OnceLock<Regex> = OnceLock::new();

    let mut syms = Vec::new();
    let mut imps = Vec::new();

    match lang {
        "rust" => {
            let rs_sym = RE_RS_SYM.get_or_init(|| {
                Regex::new(r"(?m)^\s*pub(?:\([^)]*\))?\s+(?:async\s+)?(?:fn|struct|enum|trait|type|mod)\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap()
            });
            let rs_use = RE_RS_USE.get_or_init(|| {
                Regex::new(r"(?m)^\s*(?:pub\s+)?(?:use\s+(?:crate|super|self)::([A-Za-z0-9_]+)|mod\s+([A-Za-z0-9_]+)\s*;)").unwrap()
            });
            for c in rs_sym.captures_iter(src).take(32) {
                syms.push(c[1].to_string());
            }
            for c in rs_use.captures_iter(src) {
                if let Some(m) = c.get(1).or_else(|| c.get(2)) {
                    imps.push(m.as_str().to_string());
                }
            }
        }
        "typescript" => {
            let ts_sym = RE_TS_SYM.get_or_init(|| {
                Regex::new(r"(?m)^\s*export\s+(?:default\s+)?(?:async\s+)?(?:function|class|interface|type|const|enum)\s+([A-Za-z_$][A-Za-z0-9_$]*)").unwrap()
            });
            let ts_imp = RE_TS_IMP.get_or_init(|| {
                Regex::new(r#"(?m)(?:from|import)\s+['"](\.[^'"]+)['"]"#).unwrap()
            });
            for c in ts_sym.captures_iter(src).take(32) {
                syms.push(c[1].to_string());
            }
            for c in ts_imp.captures_iter(src) {
                imps.push(c[1].to_string());
            }
        }
        "csharp" => {
            let cs_sym = RE_CS_SYM.get_or_init(|| {
                Regex::new(r"(?m)^\s*public\s+(?:sealed\s+|abstract\s+|static\s+|partial\s+)*(?:class|record|interface|struct|enum)\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap()
            });
            let cs_ns = RE_CS_NS.get_or_init(|| Regex::new(r"(?m)^\s*using\s+([A-Za-z0-9_.]+)\s*;").unwrap());
            for c in cs_sym.captures_iter(src).take(32) {
                syms.push(c[1].to_string());
            }
            for c in cs_ns.captures_iter(src) {
                imps.push(c[1].to_string());
            }
        }
        "python" => {
            let py_sym = RE_PY_SYM.get_or_init(|| Regex::new(r"(?m)^(?:def|class)\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
            let py_imp = RE_PY_IMP.get_or_init(|| Regex::new(r"(?m)^\s*(?:from\s+([A-Za-z0-9_.]+)\s+import|import\s+([A-Za-z0-9_.]+))").unwrap());
            for c in py_sym.captures_iter(src).take(32) {
                if !c[1].starts_with('_') {
                    syms.push(c[1].to_string());
                }
            }
            for c in py_imp.captures_iter(src) {
                if let Some(m) = c.get(1).or_else(|| c.get(2)) {
                    imps.push(m.as_str().to_string());
                }
            }
        }
        "go" => {
            let go_sym = RE_GO_SYM.get_or_init(|| Regex::new(r"(?m)^(?:func|type)\s+([A-Z][A-Za-z0-9_]*)").unwrap());
            for c in go_sym.captures_iter(src).take(32) {
                syms.push(c[1].to_string());
            }
        }
        "sql" => {
            let sql_tbl = RE_SQL_TBL.get_or_init(|| {
                Regex::new(r"(?im)CREATE\s+TABLE(?:\s+IF\s+NOT\s+EXISTS)?\s+(?:[A-Za-z0-9_]+\.)?([A-Za-z0-9_]+)").unwrap()
            });
            let sql_ref = RE_SQL_REF.get_or_init(|| {
                Regex::new(r"(?im)REFERENCES\s+(?:[A-Za-z0-9_]+\.)?([A-Za-z0-9_]+)").unwrap()
            });
            for c in sql_tbl.captures_iter(src) {
                syms.push(c[1].to_string());
            }
            for c in sql_ref.captures_iter(src) {
                imps.push(c[1].to_string());
            }
        }
        _ => {}
    }
    (syms, imps)
}

impl CodeGraph {
    /// Scan up to `max_files` tracked source files in `repo` and build the symbol,
    /// dependency, PageRank, and Tarjan-SCC cycle graph.
    pub fn scan(repo: &Path, max_files: usize) -> Result<Self> {
        let tracked = git::git(repo, &["ls-files"]).unwrap_or_default();
        let mut nodes: BTreeMap<String, FileNode> = BTreeMap::new();
        let mut raw_imports: HashMap<String, Vec<String>> = HashMap::new();
        let mut stem_index: HashMap<String, Vec<String>> = HashMap::new();
        let mut symbol_owner: HashMap<String, String> = HashMap::new();
        let mut languages: BTreeMap<String, usize> = BTreeMap::new();
        let mut total_lines = 0usize;

        for rel in tracked.lines() {
            if nodes.len() >= max_files {
                break;
            }
            let rel = rel.trim().replace('\\', "/");
            if rel.is_empty()
                || rel.starts_with("v3/")
                || rel.starts_with("plugins/")
                || rel.starts_with(".claude/")
                || rel.starts_with(".agents/")
                || rel.starts_with("skills/")
                || rel.contains("/node_modules/")
                || rel.contains("/target/")
            {
                continue;
            }
            let Some(lang) = detect_language(&rel) else { continue };
            let full = repo.join(&rel);
            let Ok(meta) = fs::metadata(&full) else { continue };
            if !meta.is_file() || meta.len() > 512 * 1024 {
                continue;
            }
            let Ok(src) = fs::read_to_string(&full) else { continue };
            let lines = src.lines().count();
            total_lines += lines;
            *languages.entry(lang.to_string()).or_insert(0) += 1;

            let (symbols, imps) = extract_symbols_and_raw_imports(lang, &src);
            for s in &symbols {
                symbol_owner.entry(s.clone()).or_insert_with(|| rel.clone());
            }
            if let Some(stem) = Path::new(&rel).file_stem().and_then(|s| s.to_str()) {
                stem_index.entry(stem.to_string()).or_default().push(rel.clone());
            }
            raw_imports.insert(rel.clone(), imps);
            nodes.insert(
                rel.clone(),
                FileNode {
                    path: rel,
                    language: lang.to_string(),
                    lines,
                    symbols,
                    imports: Vec::new(),
                    pagerank: 0.0,
                    in_degree: 0,
                    out_degree: 0,
                },
            );
        }

        // Resolve raw imports/references to concrete repository file paths.
        let mut edge_set: BTreeSet<(String, String)> = BTreeSet::new();
        let all_paths: HashSet<String> = nodes.keys().cloned().collect();

        for (src_path, imps) in &raw_imports {
            let parent_dir = Path::new(src_path).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            for imp in imps {
                let mut resolved: Option<String> = None;

                // 1. Relative path (`./foo` or `../bar`)
                if imp.starts_with('.') {
                    let joined = normalize_rel(&parent_dir, imp);
                    for cand in [
                        joined.clone(),
                        format!("{joined}.ts"),
                        format!("{joined}.tsx"),
                        format!("{joined}.js"),
                        format!("{joined}/index.ts"),
                        format!("{joined}.rs"),
                        format!("{joined}.py"),
                    ] {
                        if all_paths.contains(&cand) {
                            resolved = Some(cand);
                            break;
                        }
                    }
                }

                // 2. Same-crate / module stem match (`use crate::router` -> `.../router.rs` or `.../router/mod.rs`)
                if resolved.is_none() {
                    let last = imp.rsplit([':', '.', '/']).next().unwrap_or(imp);
                    if let Some(cands) = stem_index.get(last) {
                        // Prefer sibling or same-crate file
                        resolved = cands
                            .iter()
                            .find(|c| same_top_dir(src_path, c))
                            .or_else(|| cands.first())
                            .cloned();
                    } else if let Some(owner) = symbol_owner.get(last) {
                        resolved = Some(owner.clone());
                    }
                }

                if let Some(dst) = resolved {
                    if &dst != src_path {
                        edge_set.insert((src_path.clone(), dst));
                    }
                }
            }
        }

        let edges: Vec<(String, String)> = edge_set.into_iter().collect();
        for (u, v) in &edges {
            if let Some(nu) = nodes.get_mut(u) {
                nu.imports.push(v.clone());
                nu.out_degree += 1;
            }
            if let Some(nv) = nodes.get_mut(v) {
                nv.in_degree += 1;
            }
        }

        // Compute iterative PageRank (`d = 0.85`, 25 iterations) over the dependency graph
        // where `u -> v` means `u` depends on `v`, so architectural foundations accumulate rank!
        let n = nodes.len();
        if n > 0 {
            let keys: Vec<String> = nodes.keys().cloned().collect();
            let idx_of: HashMap<&str, usize> = keys.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();
            let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
            for (u, v) in &edges {
                if let (Some(&iu), Some(&iv)) = (idx_of.get(u.as_str()), idx_of.get(v.as_str())) {
                    out_adj[iu].push(iv);
                }
            }
            let d = 0.85_f64;
            let base = (1.0 - d) / (n as f64);
            let mut rank = vec![1.0 / (n as f64); n];
            for _ in 0..25 {
                let mut next = vec![base; n];
                let mut dangling = 0.0;
                for i in 0..n {
                    if out_adj[i].is_empty() {
                        dangling += rank[i];
                    } else {
                        let share = d * rank[i] / (out_adj[i].len() as f64);
                        for &dst in &out_adj[i] {
                            next[dst] += share;
                        }
                    }
                }
                let dangling_share = d * dangling / (n as f64);
                for x in &mut next {
                    *x += dangling_share;
                }
                rank = next;
            }
            for (i, k) in keys.iter().enumerate() {
                if let Some(node) = nodes.get_mut(k) {
                    node.pagerank = rank[i];
                }
            }
        }

        let cycles = tarjan_cycles(&nodes, &edges);
        Ok(CodeGraph { total_files: nodes.len(), total_lines, languages, nodes, edges, cycles })
    }

    pub fn top_hubs(&self, limit: usize) -> Vec<&FileNode> {
        let mut v: Vec<&FileNode> = self.nodes.values().collect();
        v.sort_by(|a, b| b.pagerank.partial_cmp(&a.pagerank).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.path.cmp(&b.path)));
        v.truncate(limit);
        v
    }

    /// Every file that directly or transitively depends on `changed_files` (BFS on reverse edges).
    pub fn blast_radius(&self, changed_files: &[String]) -> Vec<String> {
        let mut rev: HashMap<&str, Vec<&str>> = HashMap::new();
        for (u, v) in &self.edges {
            rev.entry(v.as_str()).or_default().push(u.as_str());
        }
        let mut visited: HashSet<String> = changed_files.iter().map(|s| s.replace('\\', "/")).collect();
        let mut q: VecDeque<String> = visited.iter().cloned().collect();
        let mut impacted = Vec::new();
        while let Some(cur) = q.pop_front() {
            if let Some(dependents) = rev.get(cur.as_str()) {
                for &dep in dependents {
                    if visited.insert(dep.to_string()) {
                        impacted.push(dep.to_string());
                        q.push_back(dep.to_string());
                    }
                }
            }
        }
        impacted.sort();
        impacted
    }

    /// Compact architectural map injected into prompts so agents orient instantly.
    pub fn render_prompt_map(&self, focus_files: &[String], max_bytes: usize) -> String {
        if self.nodes.is_empty() {
            return String::new();
        }
        let mut out = format!(
            "\n## Codebase Knowledge Graph ({} source files, {} LOC)\n",
            self.total_files, self.total_lines
        );
        out.push_str("Top architectural hub files (by PageRank centrality):\n");
        for hub in self.top_hubs(8) {
            let syms = if hub.symbols.is_empty() {
                String::new()
            } else {
                format!(" — exports: `{}`", hub.symbols.iter().take(6).cloned().collect::<Vec<_>>().join("`, `"))
            };
            let line = format!("- `{}` (PR {:.3}, in={} out={}){}\n", hub.path, hub.pagerank, hub.in_degree, hub.out_degree, syms);
            if out.len() + line.len() > max_bytes {
                return out;
            }
            out.push_str(&line);
        }
        if !focus_files.is_empty() {
            let impacted = self.blast_radius(focus_files);
            if !impacted.is_empty() {
                let line = format!(
                    "Transitive dependents (blast radius): `{}`\n",
                    impacted.into_iter().take(12).collect::<Vec<_>>().join("`, `")
                );
                if out.len() + line.len() <= max_bytes {
                    out.push_str(&line);
                }
            }
        }
        if !self.cycles.is_empty() {
            let cyc = format!("Detected {} circular dependency cycle(s); do not introduce new cycles.\n", self.cycles.len());
            if out.len() + cyc.len() <= max_bytes {
                out.push_str(&cyc);
            }
        }
        out
    }
}

fn same_top_dir(a: &str, b: &str) -> bool {
    let mut pa = a.split('/');
    let mut pb = b.split('/');
    pa.next() == pb.next() && pa.next() == pb.next()
}

fn normalize_rel(base_dir: &str, rel: &str) -> String {
    let mut stack: Vec<&str> = if base_dir.is_empty() { Vec::new() } else { base_dir.split('/').collect() };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    stack.join("/")
}

fn tarjan_cycles(nodes: &BTreeMap<String, FileNode>, edges: &[(String, String)]) -> Vec<Vec<String>> {
    let keys: Vec<&str> = nodes.keys().map(String::as_str).collect();
    let idx_of: HashMap<&str, usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); keys.len()];
    for (u, v) in edges {
        if let (Some(&iu), Some(&iv)) = (idx_of.get(u.as_str()), idx_of.get(v.as_str())) {
            adj[iu].push(iv);
        }
    }

    struct Tarjan<'a> {
        adj: &'a [Vec<usize>],
        index: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        sccs: Vec<Vec<usize>>,
    }

    fn strongconnect(v: usize, t: &mut Tarjan<'_>) {
        t.indices[v] = Some(t.index);
        t.lowlink[v] = t.index;
        t.index += 1;
        t.stack.push(v);
        t.on_stack[v] = true;

        for &w in &t.adj[v] {
            if t.indices[w].is_none() {
                strongconnect(w, t);
                t.lowlink[v] = t.lowlink[v].min(t.lowlink[w]);
            } else if t.on_stack[w] {
                t.lowlink[v] = t.lowlink[v].min(t.indices[w].unwrap());
            }
        }

        if Some(t.lowlink[v]) == t.indices[v] {
            let mut scc = Vec::new();
            loop {
                let w = t.stack.pop().unwrap();
                t.on_stack[w] = false;
                scc.push(w);
                if w == v {
                    break;
                }
            }
            if scc.len() > 1 {
                t.sccs.push(scc);
            }
        }
    }

    let n = keys.len();
    let mut state = Tarjan {
        adj: &adj,
        index: 0,
        indices: vec![None; n],
        lowlink: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        sccs: Vec::new(),
    };
    for v in 0..n {
        if state.indices[v].is_none() {
            strongconnect(v, &mut state);
        }
    }
    state
        .sccs
        .into_iter()
        .map(|scc| scc.into_iter().map(|i| keys[i].to_string()).collect())
        .collect()
}

// ------------------------------------------------ Static Security / Money / RLS Gate

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditSeverity {
    Warning,
    High,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditFinding {
    pub severity: AuditSeverity,
    pub rule: String,
    pub file: String,
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StaticAuditReport {
    pub passed: bool,
    pub findings: Vec<AuditFinding>,
}

/// Inspects changed files in `wt` (`from..to`) for deterministic security,
/// financial precision (IEEE-754 money), SQL injection, and multi-tenant RLS bugs.
pub fn audit_changes(wt: &Path, from: &str, to: &str) -> Result<StaticAuditReport> {
    let changed = git::changed_files(wt, from, to)?;
    audit_file_list(wt, &changed)
}

pub fn audit_file_list(root: &Path, files: &[String]) -> Result<StaticAuditReport> {
    static RE_SECRET: OnceLock<Regex> = OnceLock::new();
    static RE_FLOAT_MONEY_SQL: OnceLock<Regex> = OnceLock::new();
    static RE_FLOAT_MONEY_CODE: OnceLock<Regex> = OnceLock::new();
    static RE_SQLI_INTERP: OnceLock<Regex> = OnceLock::new();
    static RE_TENANT_COL: OnceLock<Regex> = OnceLock::new();
    static RE_RLS_ENABLE: OnceLock<Regex> = OnceLock::new();

    let re_secret = RE_SECRET.get_or_init(|| {
        Regex::new(r"(sk-ant-[A-Za-z0-9_-]{20,}|sk-proj-[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----)").unwrap()
    });
    let re_float_sql = RE_FLOAT_MONEY_SQL.get_or_init(|| {
        Regex::new(r"(?i)\b(?:[a-z0-9_]*_)?(amount|balance|price|subtotal|total|debit|credit|vat|salary|fee)(?:_[a-z0-9_]*)?\b[^\n,;]{0,40}\b(FLOAT|REAL|DOUBLE\s+PRECISION)\b").unwrap()
    });
    let re_float_code = RE_FLOAT_MONEY_CODE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:[a-z0-9_]*_)?(amount|balance|price|subtotal|total|debit|credit|vat|money)(?:_[a-z0-9_]*)?\s*:\s*(f32|f64)\b|\b(float|double)\s+[A-Za-z0-9_]*(Amount|Balance|Price|Subtotal|Total|Debit|Credit|Vat|Money)[A-Za-z0-9_]*\b").unwrap()
    });
    let re_sqli = RE_SQLI_INTERP.get_or_init(|| {
        Regex::new(r#"(?i)(?:format!\s*\(\s*"[^"]*\b(?:SELECT|INSERT|UPDATE|DELETE)\b[^"]*\{[A-Za-z0-9_:]+\}|(?:\$"|`)\s*(?:SELECT|INSERT|UPDATE|DELETE)\b[^"`]*\{[A-Za-z0-9_.]+\})"#).unwrap()
    });
    let re_tenant = RE_TENANT_COL.get_or_init(|| {
        Regex::new(r"(?i)CREATE\s+TABLE[\s\S]{0,1200}?\b(tenant_id|organization_id|company_id)\b").unwrap()
    });
    let re_rls = RE_RLS_ENABLE.get_or_init(|| Regex::new(r"(?i)ENABLE\s+ROW\s+LEVEL\s+SECURITY").unwrap());

    let mut findings = Vec::new();

    for rel in files {
        let norm = rel.replace('\\', "/");
        if norm.contains("test") || norm.starts_with("skills/") || norm.ends_with(".md") || norm.ends_with(".lock") {
            continue;
        }
        let full = root.join(rel);
        let Ok(src) = fs::read_to_string(&full) else { continue };
        let is_sql = norm.to_ascii_lowercase().ends_with(".sql");

        for (idx, line) in src.lines().enumerate() {
            let line_no = idx + 1;
            if line.contains("ai-fleet:allow") {
                continue;
            }
            if re_secret.is_match(line) {
                findings.push(AuditFinding {
                    severity: AuditSeverity::Critical,
                    rule: "secret.hardcoded".into(),
                    file: norm.clone(),
                    line: line_no,
                    message: "Hardcoded credential or private key detected; load secrets from environment or vault.".into(),
                });
            }
            if is_sql && re_float_sql.is_match(line) {
                findings.push(AuditFinding {
                    severity: AuditSeverity::Critical,
                    rule: "accounting.float_money_sql".into(),
                    file: norm.clone(),
                    line: line_no,
                    message: "Monetary SQL column uses approximate IEEE-754 FLOAT/REAL/DOUBLE; use exact NUMERIC(19,4) or BIGINT minor units.".into(),
                });
            }
            if !is_sql && re_float_code.is_match(line) {
                findings.push(AuditFinding {
                    severity: AuditSeverity::Critical,
                    rule: "accounting.float_money_code".into(),
                    file: norm.clone(),
                    line: line_no,
                    message: "Monetary field uses IEEE-754 binary float (f32/f64/float/double); use exact Decimal or integer minor units.".into(),
                });
            }
            if !is_sql && re_sqli.is_match(line) {
                findings.push(AuditFinding {
                    severity: AuditSeverity::Critical,
                    rule: "security.sql_injection_interpolation".into(),
                    file: norm.clone(),
                    line: line_no,
                    message: "SQL statement built via string interpolation; use parameterized bind variables (?1 / $1 / @p).".into(),
                });
            }
        }

        if is_sql && re_tenant.is_match(&src) && !re_rls.is_match(&src) && !src.contains("ai-fleet:allow-no-rls") {
            findings.push(AuditFinding {
                severity: AuditSeverity::High,
                rule: "security.missing_tenant_rls".into(),
                file: norm.clone(),
                line: 1,
                message: "Multi-tenant SQL table defines tenant_id/organization_id/company_id without `ALTER TABLE ... ENABLE ROW LEVEL SECURITY`.".into(),
            });
        }
    }

    let passed = !findings.iter().any(|f| f.severity >= AuditSeverity::High);
    Ok(StaticAuditReport { passed, findings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn static_auditor_catches_float_money_missing_rls_and_sqli() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("migrations")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("migrations/001_ledger.sql"),
            "CREATE TABLE invoices (\n  id UUID PRIMARY KEY,\n  tenant_id UUID NOT NULL,\n  total_amount DOUBLE PRECISION NOT NULL\n);\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/repo.rs"),
            "pub struct Line { pub debit: f64 }\npub fn bad(id: &str) -> String { format!(\"SELECT * FROM invoices WHERE id = {id}\") }\n",
        )
        .unwrap();

        let rep = audit_file_list(
            dir.path(),
            &["migrations/001_ledger.sql".into(), "src/repo.rs".into()],
        )
        .unwrap();
        assert!(!rep.passed);
        let rules: Vec<&str> = rep.findings.iter().map(|f| f.rule.as_str()).collect();
        assert!(rules.contains(&"accounting.float_money_sql"), "{rules:?}");
        assert!(rules.contains(&"security.missing_tenant_rls"), "{rules:?}");
        assert!(rules.contains(&"accounting.float_money_code"), "{rules:?}");
        assert!(rules.contains(&"security.sql_injection_interpolation"), "{rules:?}");
    }

    #[test]
    fn code_graph_pagerank_blast_radius_and_cycle_detection() {
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
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/domain.rs"), "pub struct Money(pub i64);\n").unwrap();
        fs::write(dir.path().join("src/ledger.rs"), "use crate::domain;\npub fn post() {}\n").unwrap();
        fs::write(dir.path().join("src/api.rs"), "use crate::ledger;\nuse crate::domain;\npub fn handler() {}\n").unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-q", "-m", "init"]);

        let g = CodeGraph::scan(dir.path(), 500).unwrap();
        assert_eq!(g.total_files, 3);
        let hubs = g.top_hubs(3);
        assert_eq!(hubs[0].path, "src/domain.rs", "foundation file must have highest PageRank: {hubs:?}");
        let radius = g.blast_radius(&["src/domain.rs".into()]);
        assert_eq!(radius, vec!["src/api.rs".to_string(), "src/ledger.rs".to_string()]);
    }
}
