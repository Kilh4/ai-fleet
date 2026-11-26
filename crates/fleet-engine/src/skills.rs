//! Agent Skill Runtime (`SKILL.md` loader, compile-time Elite Skill pack,
//! bilingual Arabic/English query expansion, Okapi BM25 + file-affinity ranker,
//! and bounded prompt injection for authors and reviewers).

use crate::router::Role;
use crate::store::truncate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/embedded_skills.rs"));

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillSpec {
    pub id: String,
    pub title: String,
    pub domain: String,
    pub description: String,
    pub source_repo: String,
    pub stars: u32,
    pub license: String,
    pub tags: Vec<String>,
    pub file_extensions: Vec<String>,
    pub role_hint: String,
    pub body: String,
    pub origin: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillMatch {
    pub id: String,
    pub title: String,
    pub domain: String,
    pub source_repo: String,
    pub stars: u32,
    pub score: f64,
    pub reason: String,
}

struct IndexedSkill {
    spec: SkillSpec,
    id_tf: HashMap<String, u32>,
    tag_tf: HashMap<String, u32>,
    desc_tf: HashMap<String, u32>,
    body_tf: HashMap<String, u32>,
    body_len: usize,
}

pub struct SkillRegistry {
    skills: Vec<IndexedSkill>,
    by_id: HashMap<String, usize>,
    df: HashMap<String, u32>,
    avg_body_len: f64,
}

/// Bilingual Arabic -> English technical expansion so Arabic prompts match
/// English `SKILL.md` packs with full precision.
const AR_EN_EXPANSIONS: &[(&str, &str)] = &[
    ("برمجه", "programming software engineering clean code implementation tdd"),
    ("برمجة", "programming software engineering clean code implementation tdd"),
    ("هندسه", "engineering systems architecture reliability design patterns"),
    ("هندسة", "engineering systems architecture reliability design patterns"),
    ("تصميم", "design ui ux interface typography layout visual styling frontend"),
    ("واجهه", "ui ux frontend interface component responsive accessibility"),
    ("واجهة", "ui ux frontend interface component responsive accessibility"),
    ("ذكاء", "intelligence reasoning first principles strategy synthesis"),
    ("حنكه", "wisdom senior judgment tradeoffs pragmatism root cause"),
    ("حنكة", "wisdom senior judgment tradeoffs pragmatism root cause"),
    ("تفكير", "thinking reasoning analysis decomposition problem solving critique"),
    ("خبره", "principal staff senior engineering review production reliability"),
    ("خبرة", "principal staff senior engineering review production reliability"),
    ("ارتكتشر", "architecture system design distributed hexagonal ddd clean"),
    ("معماريه", "architecture system design distributed hexagonal ddd clean"),
    ("معمارية", "architecture system design distributed hexagonal ddd clean"),
    ("محاسبه", "accounting ledger double entry journal debit credit financial erp"),
    ("محاسبة", "accounting ledger double entry journal debit credit financial erp"),
    ("قيد", "journal entry ledger double entry accounting balance reconciliation"),
    ("ضريبه", "tax vat compliance invoice accounting audit"),
    ("ضريبة", "tax vat compliance invoice accounting audit"),
    ("ماليه", "finance financial modeling valuation cashflow accounting"),
    ("مالية", "finance financial modeling valuation cashflow accounting"),
    ("تدقيق", "audit auditing security compliance verification rls review"),
    ("امان", "security hardening vulnerability authentication authorization owasp"),
    ("أمان", "security hardening vulnerability authentication authorization owasp"),
    ("رياضيات", "mathematics algebra calculus optimization probability statistics sympy"),
    ("احصاء", "statistics bayesian regression hypothesis inference probability"),
    ("إحصاء", "statistics bayesian regression hypothesis inference probability"),
    ("فيزياء", "physics mechanics dynamics quantum thermodynamics simulation differential"),
    ("محاكاه", "simulation numerical modeling differential equations monte carlo"),
    ("محاكاة", "simulation numerical modeling differential equations monte carlo"),
    ("اقتصاد", "economics microeconomics macroeconomics game theory pricing market"),
    ("تسعير", "pricing unit economics elasticity monetization revenue margin"),
    ("ميزات", "features product specification requirements user stories acceptance"),
    ("خصائص", "features specification state machine invariants edge cases"),
    ("لغات", "programming languages rust typescript python csharp go sql"),
    ("رست", "rust cargo borrow ownership tokio async systems"),
    ("بايثون", "python pytest numpy scipy typing async"),
    ("سي شارب", "csharp dotnet aspnet efcore linq"),
    ("تست", "testing tdd unit integration verification"),
    ("اختبار", "testing tdd unit integration verification"),
];

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "your", "you", "are", "use",
    "when", "will", "can", "all", "any", "not", "have", "has", "should", "must", "make", "please",
    "implement", "add", "create", "build", "write", "task", "file", "files", "code", "using",
    "من", "في", "على", "الى", "إلى", "عن", "مع", "هذا", "هذه", "كل", "او", "أو", "ان", "أن",
];

fn stopword_set() -> HashSet<&'static str> {
    STOPWORDS.iter().copied().collect()
}

/// Split text into normalized alphanumeric + Arabic tokens, splitting camelCase and snake_case.
pub fn tokenize(text: &str) -> Vec<String> {
    let stop = stopword_set();
    let mut expanded = text.to_string();
    let lower_orig = text.to_lowercase();
    for (ar, en) in AR_EN_EXPANSIONS {
        if lower_orig.contains(ar) {
            expanded.push(' ');
            expanded.push_str(en);
        }
    }

    // Split camelCase transitions (`RowLevelSecurity` -> `Row Level Security`).
    let mut separated = String::with_capacity(expanded.len() + 32);
    let mut prev_lower = false;
    for ch in expanded.chars() {
        if ch.is_ascii_uppercase() && prev_lower {
            separated.push(' ');
        }
        prev_lower = ch.is_ascii_lowercase();
        if ch.is_alphanumeric() || ('\u{0600}'..='\u{06FF}').contains(&ch) {
            separated.push(ch);
        } else {
            separated.push(' ');
        }
    }

    separated
        .split_whitespace()
        .map(|t| stem(&t.to_lowercase()))
        .filter(|t| t.chars().count() >= 2 && !stop.contains(t.as_str()))
        .collect()
}

fn stem(tok: &str) -> String {
    if !tok.is_ascii() || tok.len() <= 4 {
        return tok.to_string();
    }
    for suffix in ["ings", "ing", "ations", "ation", "ments", "ment", "ities", "ity", "ies", "ed", "es", "s"] {
        if let Some(base) = tok.strip_suffix(suffix) {
            if base.len() >= 4 {
                return if suffix == "ies" { format!("{base}y") } else { base.to_string() };
            }
        }
    }
    tok.to_string()
}

fn term_freqs(tokens: &[String]) -> HashMap<String, u32> {
    let mut m = HashMap::new();
    for t in tokens {
        *m.entry(t.clone()).or_insert(0) += 1;
    }
    m
}

pub fn parse_skill_markdown(raw: &str, fallback_id: &str, fallback_domain: &str, origin: &str) -> Option<SkillSpec> {
    let trimmed = raw.trim_start_matches('\u{feff}').trim();
    if trimmed.len() < 80 {
        return None;
    }
    let mut meta: BTreeMap<String, String> = BTreeMap::new();
    let body = if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end_idx) = rest.find("\n---") {
            let fm = &rest[..end_idx];
            let after = &rest[end_idx + 4..];
            let body = after.strip_prefix('\r').unwrap_or(after);
            let body = body.strip_prefix('\n').unwrap_or(body).trim();
            for line in fm.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    let key = k.trim().to_ascii_lowercase();
                    if !key.is_empty() && !key.contains(' ') {
                        let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                        meta.insert(key, val);
                    }
                }
            }
            body.to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };

    if body.len() < 60 {
        return None;
    }

    let id = sanitize_id(meta.get("id").or_else(|| meta.get("name")).map(String::as_str).unwrap_or(fallback_id));
    let title = meta
        .get("title")
        .cloned()
        .or_else(|| {
            body.lines()
                .find_map(|l| l.trim().strip_prefix("# ").map(|h| h.trim().to_string()))
        })
        .unwrap_or_else(|| id.replace('-', " "));
    let domain = meta
        .get("domain")
        .or_else(|| meta.get("category"))
        .cloned()
        .unwrap_or_else(|| fallback_domain.to_string());
    let description = meta.get("description").cloned().unwrap_or_else(|| {
        body.lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#') && l.len() > 20)
            .map(|l| truncate(l, 260))
            .unwrap_or_else(|| title.clone())
    });
    let source_repo = meta.get("source_repo").or_else(|| meta.get("source")).cloned().unwrap_or_else(|| "local".into());
    let stars = meta.get("stars").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let license = meta.get("license").cloned().unwrap_or_else(|| "MIT".into());
    let tags = split_csv(meta.get("tags").or_else(|| meta.get("triggers")));
    let file_extensions = split_csv(meta.get("extensions").or_else(|| meta.get("files")));
    let role_hint = meta.get("role").cloned().unwrap_or_else(|| "any".into());

    Some(SkillSpec {
        id,
        title,
        domain,
        description,
        source_repo,
        stars,
        license,
        tags,
        file_extensions,
        role_hint,
        body,
        origin: origin.to_string(),
    })
}

fn split_csv(v: Option<&String>) -> Vec<String> {
    let Some(s) = v else { return Vec::new() };
    s.trim_matches(['[', ']'])
        .split([',', ';'])
        .map(|x| x.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase())
        .filter(|x| !x.is_empty())
        .collect()
}

fn sanitize_id(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() { "skill".into() } else { s }
}

fn walk_skill_dir(dir: &Path, default_domain: &str, out: &mut Vec<SkillSpec>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            let folder = p.file_name().and_then(|n| n.to_str()).unwrap_or(default_domain);
            let next_dom = if default_domain == "custom" { folder } else { default_domain };
            walk_skill_dir(&p, next_dom, out);
        } else if p.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
            if let Ok(raw) = fs::read_to_string(&p) {
                let fallback_id = p.parent().and_then(|d| d.file_name()).and_then(|n| n.to_str()).unwrap_or("skill");
                if let Some(spec) = parse_skill_markdown(&raw, fallback_id, default_domain, &p.to_string_lossy()) {
                    out.push(spec);
                }
            }
        }
    }
}

fn infer_extensions_from_paths(paths: &[String], prompt: &str) -> HashSet<String> {
    let mut exts = HashSet::new();
    for p in paths {
        let clean = p.trim_end_matches(['*', '?', '/']);
        if let Some(ext) = Path::new(clean).extension().and_then(|e| e.to_str()) {
            exts.insert(ext.to_ascii_lowercase());
        }
    }
    for word in prompt.split(|c: char| c.is_whitespace() || matches!(c, '`' | '"' | '\'' | ',' | '(' | ')' | ';')) {
        if let Some((_, ext)) = word.rsplit_once('.') {
            let e = ext.to_ascii_lowercase();
            if matches!(
                e.as_str(),
                "rs" | "ts" | "tsx" | "js" | "jsx" | "cs" | "csproj" | "py" | "go" | "sql"
                    | "cpp" | "cc" | "h" | "java" | "kt" | "css" | "scss" | "vue" | "svelte" | "zig"
            ) {
                exts.insert(e);
            }
        }
    }
    exts
}

impl SkillRegistry {
    /// Load embedded Elite skills plus any on-disk overrides in `<repo>/skills/elite`
    /// and `<repo>/.ai-fleet/skills`.
    pub fn load(repo: Option<&Path>) -> Self {
        let mut map: BTreeMap<String, SkillSpec> = BTreeMap::new();

        for (rel, raw) in EMBEDDED_SKILL_RAW {
            let parts: Vec<&str> = rel.split('/').collect();
            let domain = parts.first().copied().unwrap_or("general");
            let slug = parts.get(parts.len().saturating_sub(2)).copied().unwrap_or("skill");
            if let Some(spec) = parse_skill_markdown(raw, slug, domain, &format!("bundled:{rel}")) {
                map.insert(spec.id.clone(), spec);
            }
        }

        if let Some(r) = repo {
            let mut disk = Vec::new();
            let elite = r.join("skills").join("elite");
            if elite.is_dir() {
                walk_skill_dir(&elite, "custom", &mut disk);
            }
            let local = r.join(".ai-fleet").join("skills");
            if local.is_dir() {
                walk_skill_dir(&local, "custom", &mut disk);
            }
            for spec in disk {
                map.insert(spec.id.clone(), spec);
            }
        }

        if let Ok(extra) = std::env::var("AI_FLEET_SKILLS_DIR") {
            let p = PathBuf::from(extra);
            if p.is_dir() {
                let mut disk = Vec::new();
                walk_skill_dir(&p, "custom", &mut disk);
                for spec in disk {
                    map.insert(spec.id.clone(), spec);
                }
            }
        }

        Self::from_specs(map.into_values().collect())
    }

    pub fn from_specs(specs: Vec<SkillSpec>) -> Self {
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut indexed = Vec::with_capacity(specs.len());
        let mut by_id = HashMap::with_capacity(specs.len());
        let mut total_body_len = 0usize;

        for spec in specs {
            let id_toks = tokenize(&format!("{} {}", spec.id, spec.title));
            let tag_toks = tokenize(&format!("{} {}", spec.domain, spec.tags.join(" ")));
            let desc_toks = tokenize(&spec.description);
            // Index the first 12 KB of body for fast, sharp BM25 matching.
            let body_head = if spec.body.len() <= 12_000 {
                spec.body.as_str()
            } else {
                let mut cut = 12_000;
                while !spec.body.is_char_boundary(cut) {
                    cut -= 1;
                }
                &spec.body[..cut]
            };
            let body_toks = tokenize(body_head);
            let body_len = body_toks.len().max(1);
            total_body_len += body_len;

            let mut unique: HashSet<&str> = HashSet::new();
            for t in id_toks.iter().chain(&tag_toks).chain(&desc_toks).chain(&body_toks) {
                unique.insert(t.as_str());
            }
            for u in unique {
                *df.entry(u.to_string()).or_insert(0) += 1;
            }

            let idx = indexed.len();
            by_id.insert(spec.id.clone(), idx);
            indexed.push(IndexedSkill {
                spec,
                id_tf: term_freqs(&id_toks),
                tag_tf: term_freqs(&tag_toks),
                desc_tf: term_freqs(&desc_toks),
                body_tf: term_freqs(&body_toks),
                body_len,
            });
        }

        let avg_body_len = if indexed.is_empty() {
            100.0
        } else {
            (total_body_len as f64) / (indexed.len() as f64)
        };

        SkillRegistry { skills: indexed, by_id, df, avg_body_len }
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&SkillSpec> {
        self.by_id.get(&sanitize_id(id)).map(|&i| &self.skills[i].spec)
    }

    pub fn list(&self, domain_filter: Option<&str>) -> Vec<&SkillSpec> {
        let mut out: Vec<&SkillSpec> = self
            .skills
            .iter()
            .map(|s| &s.spec)
            .filter(|s| match domain_filter {
                Some(d) => s.domain.eq_ignore_ascii_case(d) || s.domain.contains(&d.to_ascii_lowercase()),
                None => true,
            })
            .collect();
        out.sort_by(|a, b| a.domain.cmp(&b.domain).then_with(|| b.stars.cmp(&a.stars)).then_with(|| a.id.cmp(&b.id)));
        out
    }

    /// Rank and select up to `limit` skills for a task using Okapi BM25 +
    /// file-extension affinity + role boost + explicit pins.
    pub fn select(
        &self,
        query: &str,
        owned_files: &[String],
        role: Role,
        pinned: &[String],
        limit: usize,
    ) -> Vec<SkillMatch> {
        if self.skills.is_empty() || limit == 0 {
            return Vec::new();
        }

        let mut results: Vec<SkillMatch> = Vec::new();
        let mut chosen_ids: HashSet<String> = HashSet::new();

        // 1. Honor explicit `--skill <id>` pins first.
        for pin in pinned {
            if let Some(spec) = self.get(pin) {
                if chosen_ids.insert(spec.id.clone()) {
                    results.push(SkillMatch {
                        id: spec.id.clone(),
                        title: spec.title.clone(),
                        domain: spec.domain.clone(),
                        source_repo: spec.source_repo.clone(),
                        stars: spec.stars,
                        score: 100.0,
                        reason: "explicitly pinned".into(),
                    });
                }
            }
        }
        if results.len() >= limit {
            results.truncate(limit);
            return results;
        }

        // 2. Compute Okapi BM25 + multi-signal affinity across remaining skills.
        let mut q_tokens = tokenize(query);
        if role == Role::Reviewer {
            q_tokens.extend(["review".to_string(), "audit".to_string(), "security".to_string(), "quality".to_string()]);
        }
        let unique_q: Vec<String> = {
            let mut seen = HashSet::new();
            q_tokens.into_iter().filter(|t| seen.insert(t.clone())).collect()
        };
        let exts = infer_extensions_from_paths(owned_files, query);
        let n_docs = self.skills.len() as f64;
        let k1 = 1.5_f64;
        let b = 0.75_f64;

        let mut candidates: Vec<SkillMatch> = Vec::new();
        for item in &self.skills {
            if chosen_ids.contains(&item.spec.id) {
                continue;
            }
            let mut score = 0.0_f64;
            let mut matched_terms: Vec<String> = Vec::new();

            for term in &unique_q {
                let df = *self.df.get(term).unwrap_or(&0) as f64;
                if df == 0.0 {
                    continue;
                }
                let idf = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();

                let mut term_gain = 0.0;
                if let Some(&tf) = item.id_tf.get(term) {
                    term_gain += 4.2 * idf * (tf as f64) / (tf as f64 + 1.0);
                }
                if let Some(&tf) = item.tag_tf.get(term) {
                    term_gain += 3.4 * idf * (tf as f64) / (tf as f64 + 1.0);
                }
                if let Some(&tf) = item.desc_tf.get(term) {
                    term_gain += 2.2 * idf * (tf as f64) / (tf as f64 + 1.2);
                }
                if let Some(&tf) = item.body_tf.get(term) {
                    let norm = 1.0 - b + b * (item.body_len as f64 / self.avg_body_len.max(1.0));
                    let bm25_tf = (tf as f64 * (k1 + 1.0)) / (tf as f64 + k1 * norm);
                    term_gain += 1.0 * idf * bm25_tf;
                }
                if term_gain > 0.0 {
                    score += term_gain;
                    if matched_terms.len() < 5 && (item.id_tf.contains_key(term) || item.tag_tf.contains_key(term) || item.desc_tf.contains_key(term)) {
                        matched_terms.push(term.clone());
                    }
                }
            }

            let mut reasons = Vec::new();
            if !matched_terms.is_empty() {
                reasons.push(format!("matched [{}]", matched_terms.join(", ")));
            }

            // File-extension affinity boost (only amplifies when at least mildly relevant or direct language skill).
            let ext_hit: Vec<&String> = item.spec.file_extensions.iter().filter(|e| exts.contains(e.trim_start_matches('.'))).collect();
            if !ext_hit.is_empty() {
                score += 4.5;
                reasons.push(format!("file-ext .{}", ext_hit[0].trim_start_matches('.')));
            }

            // Role affinity.
            if role == Role::Reviewer && (item.spec.role_hint == "reviewer" || item.spec.domain.contains("audit") || item.spec.id.contains("review")) {
                if score > 1.5 {
                    score *= 1.25;
                    reasons.push("reviewer-gate boost".into());
                }
            }

            if score < 2.2 {
                continue;
            }

            // Slight quality tie-breaker from GitHub stars (`<= ~0.22`).
            if item.spec.stars > 0 {
                score += 0.04 * (item.spec.stars as f64).log10();
            }

            candidates.push(SkillMatch {
                id: item.spec.id.clone(),
                title: item.spec.title.clone(),
                domain: item.spec.domain.clone(),
                source_repo: item.spec.source_repo.clone(),
                stars: item.spec.stars,
                score,
                reason: if reasons.is_empty() { "BM25 lexical relevance".into() } else { reasons.join(", ") },
            });
        }

        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id)));

        // Diversify slightly: avoid picking 3 nearly identical skills from the exact same sub-topic if strong peers exist.
        let mut domain_counts: HashMap<String, usize> = HashMap::new();
        for c in candidates {
            if results.len() >= limit {
                break;
            }
            let cnt = domain_counts.entry(c.domain.clone()).or_insert(0);
            if *cnt >= 2 && limit <= 4 && c.score < results.first().map(|r| r.score * 0.85).unwrap_or(0.0) {
                continue;
            }
            *cnt += 1;
            results.push(c);
        }
        results
    }

    /// Render the selected skills into a compact, high-signal Markdown section
    /// bounded to `max_total_bytes` (preserving line boundaries).
    pub fn render_prompt_section(&self, matches: &[SkillMatch], max_total_bytes: usize) -> String {
        if matches.is_empty() {
            return String::new();
        }
        let per_skill = (max_total_bytes / matches.len().max(1)).clamp(1800, 5500);
        let mut out = String::from("\n## Activated Expert Skills\nApply the engineering principles, checklists, and invariants from these domain skills:\n");
        let mut used = out.len();

        for m in matches {
            let Some(spec) = self.get(&m.id) else { continue };
            let excerpt = slice_clean_prefix(&spec.body, per_skill);
            let block = format!(
                "\n### Skill `{}` — {} (domain: `{}`, source: `{}`)\n{}\n",
                spec.id, spec.title, spec.domain, spec.source_repo, excerpt
            );
            if used + block.len() > max_total_bytes + 512 && used > 300 {
                break;
            }
            used += block.len();
            out.push_str(&block);
        }
        out
    }
}

fn slice_clean_prefix(s: &str, max_bytes: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max_bytes {
        return trimmed.to_string();
    }
    let mut cut = max_bytes;
    while !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    let prefix = &trimmed[..cut];
    if let Some(nl) = prefix.rfind('\n') {
        if nl > max_bytes / 2 {
            return format!("{}\n… [skill condensed]", prefix[..nl].trim_end());
        }
    }
    format!("{prefix}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_registry() -> SkillRegistry {
        let s1 = parse_skill_markdown(
            "---\nname: rust-concurrency\ndomain: languages\nsource_repo: test/rust\nstars: 50000\ntags: rust, tokio, async, lockfree, borrow\nextensions: rs\n---\n# Rust Async & Concurrency\nNever hold a std::sync::MutexGuard across an .await point. Prefer bounded channels and structured JoinSet cancellation.",
            "rust-concurrency",
            "languages",
            "mem",
        ).unwrap();
        let s2 = parse_skill_markdown(
            "---\nname: double-entry-ledger\ndomain: accounting-erp\nsource_repo: test/fin\nstars: 40000\ntags: accounting, ledger, double-entry, debit, credit, ifrs, decimal, vat\nextensions: sql, cs\n---\n# Double-Entry Accounting Invariants\nEvery journal posting MUST balance sum(debits) == sum(credits) in integer minor units or fixed-scale NUMERIC(19,4); never IEEE-754 float.",
            "double-entry-ledger",
            "accounting-erp",
            "mem",
        ).unwrap();
        let s3 = parse_skill_markdown(
            "---\nname: ui-ux-pro-max\ndomain: design-ui-ux\nsource_repo: nextlevelbuilder/ui-ux-pro-max-skill\nstars: 154000\ntags: ui, ux, typography, layout, color, accessibility, design-system\nextensions: tsx, css\n---\n# UI/UX Pro Max\nEstablish strict typographic hierarchy, intentional whitespace grid, and WCAG AA contrast ratios.",
            "ui-ux-pro-max",
            "design-ui-ux",
            "mem",
        ).unwrap();
        SkillRegistry::from_specs(vec![s1, s2, s3])
    }

    #[test]
    fn matches_english_arabic_and_file_extensions() {
        let reg = sample_registry();

        let r = reg.select("Fix deadlock in async tokio worker", &["src/worker.rs".into()], Role::Author, &[], 2);
        assert_eq!(r[0].id, "rust-concurrency");

        // Arabic accounting prompt!
        let ar = reg.select("بناء نظام محاسبة قيد مزدوج وضريبة القيمة المضافة", &[], Role::Author, &[], 2);
        assert_eq!(ar[0].id, "double-entry-ledger");

        // UI design prompt + .tsx file affinity!
        let ui = reg.select("Redesign dashboard layout and typography", &["web/App.tsx".into()], Role::Author, &[], 2);
        assert_eq!(ui[0].id, "ui-ux-pro-max");

        let rendered = reg.render_prompt_section(&ar, 8000);
        assert!(rendered.contains("double-entry-ledger"));
        assert!(rendered.contains("NUMERIC(19,4)"));
    }
}
