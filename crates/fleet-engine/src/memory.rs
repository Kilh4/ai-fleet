//! Persistent Hybrid Memory & Reflexion ReasoningBank (`SQLite FTS5 + 128-D
//! Signed Feature-Hashed Vector Index + Navigable Small-World Graph + Reciprocal
//! Rank Fusion`).
//!
//! Stores institutional knowledge, completed task episodes, and automatic
//! Reflexion lessons (synthesized when a failed attempt is later fixed) inside
//! `engine.db` so agents never repeat the same mistake twice across sessions.

use crate::error::Result;
use crate::skills::tokenize;
use crate::store::{now_ts, truncate};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub const EMBED_DIM: usize = 128;

const MEMORY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT 'repo',
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    files TEXT NOT NULL DEFAULT '[]',
    tags TEXT NOT NULL DEFAULT '[]',
    task_id TEXT,
    importance REAL NOT NULL DEFAULT 1.0,
    access_count INTEGER NOT NULL DEFAULT 0,
    embedding BLOB NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);

CREATE TABLE IF NOT EXISTS memory_edges (
    src INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    dst INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    similarity REAL NOT NULL,
    PRIMARY KEY (src, dst)
);
"#;

const FTS_SCHEMA: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    title,
    content,
    tags,
    files,
    content='memories',
    content_rowid='id'
);
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// Synthesized automatically when a failed attempt (tests, scope, audit, review) is overcome.
    Reflexion,
    /// Summary of a completed task and its touched files.
    Episode,
    /// Explicit architectural invariant or domain rule.
    Knowledge,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::Reflexion => "reflexion",
            MemoryKind::Episode => "episode",
            MemoryKind::Knowledge => "knowledge",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "reflexion" => MemoryKind::Reflexion,
            "episode" => MemoryKind::Episode,
            _ => MemoryKind::Knowledge,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewMemory {
    pub kind: MemoryKind,
    pub title: String,
    pub content: String,
    pub files: Vec<String>,
    pub tags: Vec<String>,
    pub task_id: Option<String>,
    pub importance: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: i64,
    pub kind: MemoryKind,
    pub title: String,
    pub content: String,
    pub files: Vec<String>,
    pub tags: Vec<String>,
    pub task_id: Option<String>,
    pub importance: f64,
    pub access_count: u64,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryHit {
    pub record: MemoryRecord,
    pub score: f64,
    pub vector_sim: f64,
    pub lexical_rank: Option<usize>,
    pub graph_hop: bool,
}

/// Deterministic 128-D L2-normalized feature-hashed embedding over word unigrams,
/// word bigrams, and character 4-grams (handles English, Arabic, and code identifiers).
pub fn embed_text(text: &str) -> [f32; EMBED_DIM] {
    let mut vec = [0.0_f32; EMBED_DIM];
    let toks = tokenize(text);

    // 1. Word unigrams (weight 1.0)
    for tok in &toks {
        accumulate_feature(&mut vec, tok.as_bytes(), 1.0);
    }
    // 2. Word bigrams (weight 1.35 — captures phrases like "double entry", "row level")
    for pair in toks.windows(2) {
        let bigram = format!("{}_{}", pair[0], pair[1]);
        accumulate_feature(&mut vec, bigram.as_bytes(), 1.35);
    }
    // 3. Character 4-grams on normalized text (weight 0.35 — morphological & typo resilience)
    let chars: Vec<char> = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || ('\u{0600}'..='\u{06FF}').contains(c))
        .take(2048)
        .collect();
    for win in chars.windows(4) {
        let s: String = win.iter().collect();
        accumulate_feature(&mut vec, s.as_bytes(), 0.35);
    }

    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-8 {
        for x in &mut vec {
            *x /= norm;
        }
    }
    vec
}

fn accumulate_feature(vec: &mut [f32; EMBED_DIM], bytes: &[u8], weight: f32) {
    let digest = Sha256::digest(bytes);
    // Project each feature onto 2 orthogonal signed coordinates (CountSketch / Weinberger hashing).
    let idx1 = (u16::from_le_bytes([digest[0], digest[1]]) as usize) % EMBED_DIM;
    let sign1 = if digest[2] & 1 == 0 { 1.0 } else { -1.0 };
    let idx2 = (u16::from_le_bytes([digest[3], digest[4]]) as usize) % EMBED_DIM;
    let sign2 = if digest[5] & 1 == 0 { 1.0 } else { -1.0 };
    vec[idx1] += sign1 * weight;
    vec[idx2] += sign2 * weight * 0.7;
}

pub fn cosine(a: &[f32; EMBED_DIM], b: &[f32; EMBED_DIM]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| (*x as f64) * (*y as f64)).sum()
}

fn encode_vec(v: &[f32; EMBED_DIM]) -> Vec<u8> {
    let mut out = Vec::with_capacity(EMBED_DIM * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn decode_vec(bytes: &[u8]) -> [f32; EMBED_DIM] {
    let mut out = [0.0_f32; EMBED_DIM];
    for (i, chunk) in bytes.chunks_exact(4).take(EMBED_DIM).enumerate() {
        out[i] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    out
}

pub struct MemoryStore {
    conn: Mutex<Connection>,
    fts_enabled: bool,
}

impl MemoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(MEMORY_SCHEMA)?;
        let fts_enabled = conn.execute_batch(FTS_SCHEMA).is_ok();
        Ok(MemoryStore { conn: Mutex::new(conn), fts_enabled })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn count(&self) -> Result<usize> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Store a memory, index it in FTS5, and wire bidirectional navigable
    /// small-world graph edges (`M = 8`) to its nearest semantic neighbors.
    pub fn store(&self, nm: NewMemory) -> Result<i64> {
        let full_text = format!("{}\n{}\n{}\n{}", nm.title, nm.content, nm.tags.join(" "), nm.files.join(" "));
        let emb = embed_text(&full_text);
        let emb_bytes = encode_vec(&emb);
        let files_json = serde_json::to_string(&nm.files).unwrap_or_else(|_| "[]".into());
        let tags_json = serde_json::to_string(&nm.tags).unwrap_or_else(|_| "[]".into());
        let now = now_ts();

        let conn = self.lock();
        conn.execute(
            "INSERT INTO memories (kind, title, content, files, tags, task_id, importance, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                nm.kind.as_str(),
                nm.title,
                nm.content,
                files_json,
                tags_json,
                nm.task_id,
                nm.importance.clamp(0.1, 5.0),
                emb_bytes,
                now,
            ],
        )?;
        let id = conn.last_insert_rowid();

        if self.fts_enabled {
            let _ = conn.execute(
                "INSERT INTO memories_fts(rowid, title, content, tags, files) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, nm.title, nm.content, nm.tags.join(" "), nm.files.join(" ")],
            );
        }

        // Link to top M=8 nearest semantic neighbors (Navigable Small-World graph).
        let mut stmt = conn.prepare("SELECT id, embedding FROM memories WHERE id != ?1 ORDER BY id DESC LIMIT 1024")?;
        let mut neighbors: Vec<(i64, f64)> = stmt
            .query_map([id], |r| {
                let other_id: i64 = r.get(0)?;
                let blob: Vec<u8> = r.get(1)?;
                Ok((other_id, cosine(&emb, &decode_vec(&blob))))
            })?
            .filter_map(|x| x.ok())
            .filter(|(_, sim)| *sim > 0.18)
            .collect();
        drop(stmt);

        neighbors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (nbr_id, sim) in neighbors.into_iter().take(8) {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO memory_edges (src, dst, similarity) VALUES (?1, ?2, ?3)",
                params![id, nbr_id, sim],
            );
            let _ = conn.execute(
                "INSERT OR REPLACE INTO memory_edges (src, dst, similarity) VALUES (?1, ?2, ?3)",
                params![nbr_id, id, sim],
            );
        }
        Ok(id)
    }

    /// Hybrid Retrieval: Reciprocal Rank Fusion (RRF) combining:
    /// 1. SQLite FTS5 BM25 lexical ranking
    /// 2. 128-D Cosine Vector search + 1-hop Small-World Graph expansion
    /// 3. File-path overlap & `Reflexion` importance boost
    pub fn search(&self, query: &str, touched_files: &[String], limit: usize) -> Result<Vec<MemoryHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let q_emb = embed_text(&format!("{query} {}", touched_files.join(" ")));
        let conn = self.lock();

        // 1. FTS5 BM25 lexical search
        let mut fts_ranks: HashMap<i64, usize> = HashMap::new();
        if self.fts_enabled {
            let q_toks = tokenize(query);
            if !q_toks.is_empty() {
                let fts_expr = q_toks
                    .iter()
                    .take(12)
                    .map(|t| format!("\"{}\"", t.replace('"', "")))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                if let Ok(mut stmt) = conn.prepare(
                    "SELECT rowid FROM memories_fts WHERE memories_fts MATCH ?1 ORDER BY bm25(memories_fts) LIMIT 40",
                ) {
                    if let Ok(rows) = stmt.query_map([fts_expr], |r| r.get::<_, i64>(0)) {
                        for (rank, id) in rows.flatten().enumerate() {
                            fts_ranks.insert(id, rank + 1);
                        }
                    }
                }
            }
        }

        // 2. Dense vector scan over recent/important memories + 1-hop NSW graph expansion
        let mut stmt = conn.prepare(
            "SELECT id, kind, title, content, files, tags, task_id, importance, access_count, embedding, created_at
             FROM memories ORDER BY importance DESC, id DESC LIMIT 2048",
        )?;
        let mut by_id: HashMap<i64, (MemoryRecord, f64)> = HashMap::new();
        let rows = stmt.query_map([], |r| {
            let kind_s: String = r.get(1)?;
            let files_s: String = r.get(4)?;
            let tags_s: String = r.get(5)?;
            let blob: Vec<u8> = r.get(9)?;
            let rec = MemoryRecord {
                id: r.get(0)?,
                kind: MemoryKind::parse(&kind_s),
                title: r.get(2)?,
                content: r.get(3)?,
                files: serde_json::from_str(&files_s).unwrap_or_default(),
                tags: serde_json::from_str(&tags_s).unwrap_or_default(),
                task_id: r.get(6)?,
                importance: r.get(7)?,
                access_count: r.get::<_, i64>(8)? as u64,
                created_at: r.get(10)?,
            };
            let sim = cosine(&q_emb, &decode_vec(&blob));
            Ok((rec, sim))
        })?;
        for row in rows {
            let (rec, sim) = row?;
            by_id.insert(rec.id, (rec, sim));
        }
        drop(stmt);

        if by_id.is_empty() {
            return Ok(Vec::new());
        }

        let mut vec_order: Vec<(i64, f64)> = by_id.iter().map(|(&id, (_, sim))| (id, *sim)).collect();
        vec_order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let vec_ranks: HashMap<i64, usize> = vec_order.iter().enumerate().map(|(i, (id, _))| (*id, i + 1)).collect();

        // Expand 1-hop NSW neighbors from top-4 seeds for associative recall.
        let mut graph_expanded: HashSet<i64> = HashSet::new();
        for &(seed_id, seed_sim) in vec_order.iter().take(4) {
            if seed_sim < 0.15 {
                continue;
            }
            let mut estmt = conn.prepare("SELECT dst FROM memory_edges WHERE src = ?1 ORDER BY similarity DESC LIMIT 4")?;
            for dst in estmt.query_map([seed_id], |r| r.get::<_, i64>(0))?.flatten() {
                graph_expanded.insert(dst);
            }
        }

        let file_set: HashSet<String> = touched_files.iter().map(|f| f.replace('\\', "/")).collect();
        let k_rrf = 30.0_f64;
        let mut hits: Vec<MemoryHit> = Vec::new();

        for (&id, (rec, sim)) in &by_id {
            let lex_rank = fts_ranks.get(&id).copied();
            let v_rank = vec_ranks.get(&id).copied().unwrap_or(9999);
            let is_hop = graph_expanded.contains(&id);

            let mut rrf = 1.0 / (k_rrf + v_rank as f64) + (*sim).max(0.0) * 0.45;
            if let Some(lr) = lex_rank {
                rrf += 1.35 / (k_rrf + lr as f64);
            }
            if is_hop {
                rrf += 0.015;
            }
            let file_overlap = rec.files.iter().any(|f| file_set.contains(&f.replace('\\', "/")));
            if file_overlap {
                rrf += 0.12;
            }
            if rec.kind == MemoryKind::Reflexion {
                rrf *= 1.18;
            }
            rrf *= (0.8 + 0.2 * rec.importance).clamp(0.8, 1.8);

            if *sim < 0.08 && lex_rank.is_none() && !file_overlap {
                continue;
            }

            hits.push(MemoryHit {
                record: rec.clone(),
                score: rrf,
                vector_sim: *sim,
                lexical_rank: lex_rank,
                graph_hop: is_hop,
            });
        }

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| b.record.id.cmp(&a.record.id)));
        hits.truncate(limit);

        for h in &hits {
            let _ = conn.execute("UPDATE memories SET access_count = access_count + 1 WHERE id = ?1", [h.record.id]);
        }
        Ok(hits)
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<MemoryRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, kind, title, content, files, tags, task_id, importance, access_count, created_at
             FROM memories ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            let kind_s: String = r.get(1)?;
            let files_s: String = r.get(4)?;
            let tags_s: String = r.get(5)?;
            Ok(MemoryRecord {
                id: r.get(0)?,
                kind: MemoryKind::parse(&kind_s),
                title: r.get(2)?,
                content: r.get(3)?,
                files: serde_json::from_str(&files_s).unwrap_or_default(),
                tags: serde_json::from_str(&tags_s).unwrap_or_default(),
                task_id: r.get(6)?,
                importance: r.get(7)?,
                access_count: r.get::<_, i64>(8)? as u64,
                created_at: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn render_prompt_section(&self, hits: &[MemoryHit], max_bytes: usize) -> String {
        if hits.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "\n## Institutional Memory & Past Reflexion Lessons\nHeed these verified lessons and architectural facts from earlier runs on this repository:\n",
        );
        for h in hits {
            let entry = format!(
                "- **[{}] {}** (score {:.2}): {}\n",
                h.record.kind.as_str(),
                h.record.title,
                h.score,
                truncate(h.record.content.trim(), 900).replace('\n', " ")
            );
            if out.len() + entry.len() > max_bytes {
                break;
            }
            out.push_str(&entry);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hybrid_fts5_vector_and_graph_recall() {
        let m = MemoryStore::open_in_memory().unwrap();
        m.store(NewMemory {
            kind: MemoryKind::Reflexion,
            title: "Never use float64 for invoice VAT amounts".into(),
            content: "Migration 0013 failed audit because invoice total_amount used DOUBLE PRECISION instead of NUMERIC(19,4).".into(),
            files: vec!["migrations/0013_invoices.sql".into()],
            tags: vec!["accounting".into(), "sql".into(), "decimal".into()],
            task_id: Some("t-1".into()),
            importance: 2.0,
        })
        .unwrap();
        m.store(NewMemory {
            kind: MemoryKind::Knowledge,
            title: "Tokio mutex guard across await".into(),
            content: "Keep rusqlite MutexGuard inside synchronous helper scopes; never hold across .await.".into(),
            files: vec!["src/store.rs".into()],
            tags: vec!["rust".into(), "tokio".into()],
            task_id: None,
            importance: 1.2,
        })
        .unwrap();

        // Query in Arabic about invoices & tax -> hits the SQL NUMERIC(19,4) reflexion lesson!
        let hits = m.search("بناء جدول محاسبة وضريبة القيمة المضافة", &[], 3).unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].record.title.contains("VAT"));

        // Query in English about async locks -> hits Tokio mutex memory!
        let hits2 = m.search("deadlock holding mutex guard across async await", &[], 3).unwrap();
        assert!(hits2[0].record.title.contains("Tokio"));
    }
}
