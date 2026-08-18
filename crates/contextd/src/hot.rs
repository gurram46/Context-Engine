#![allow(dead_code)]
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use context_index::bm25::{bm25_term_score, idf, tokenize, Bm25Candidate};
use context_index::embed::ModelFingerprint;
use context_index::structural::store as structural_store;
use context_index::vector::{self, VectorCandidate};

/// In-memory BM25 index, generation-bound.
pub struct HotBm25 {
    n: usize,
    avgdl: f64,
    docs: HashMap<String, Bm25Doc>,
    // term -> list of (doc_id, tf)
    postings: HashMap<String, Vec<(String, usize)>>,
    // term -> df (distinct doc count) — can derive from postings len but store for speed
}

#[derive(Clone, Debug)]
struct Bm25Doc {
    doc_id: String,
    chunk_id: String,
    file: String,
    content_hash: String,
    length: usize,
    symbol: Option<String>,
    start_line: u32,
    end_line: u32,
}

impl HotBm25 {
    pub fn load(conn: &rusqlite::Connection) -> anyhow::Result<Arc<Self>> {
        // Load docs
        let mut docs: HashMap<String, Bm25Doc> = HashMap::new();
        let mut n: usize = 0;
        let mut total_len: usize = 0;
        {
            let mut stmt = conn.prepare(
                "SELECT doc_id, chunk_id, file, content_hash, length, symbol, start_line, end_line FROM bm25_documents",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Bm25Doc {
                    doc_id: row.get(0)?,
                    chunk_id: row.get(1)?,
                    file: row.get(2)?,
                    content_hash: row.get(3)?,
                    length: row.get::<_, i64>(4)? as usize,
                    symbol: row.get(5)?,
                    start_line: row.get::<_, i64>(6)? as u32,
                    end_line: row.get::<_, i64>(7)? as u32,
                })
            })?;
            for r in rows {
                let d = r?;
                total_len += d.length;
                n += 1;
                docs.insert(d.doc_id.clone(), d);
            }
        }
        let avgdl = if n == 0 {
            1.0
        } else {
            total_len as f64 / n as f64
        };
        // Load postings
        let mut postings: HashMap<String, Vec<(String, usize)>> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT term, doc_id, tf FROM bm25_postings")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? as usize,
                ))
            })?;
            for r in rows {
                let (term, doc_id, tf) = r?;
                postings.entry(term).or_default().push((doc_id, tf));
            }
        }
        Ok(Arc::new(Self {
            n,
            avgdl,
            docs,
            postings,
        }))
    }

    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<Bm25Candidate>> {
        let query_tokens_raw = tokenize(query);
        if query_tokens_raw.is_empty() {
            return Ok(Vec::new());
        }
        let mut query_terms_set: HashSet<String> = HashSet::new();
        for t in query_tokens_raw {
            if t.len() > 64 {
                continue;
            }
            query_terms_set.insert(t);
        }
        let mut query_terms: Vec<String> = query_terms_set.into_iter().collect();
        query_terms.sort();
        if query_terms.is_empty() || self.n == 0 {
            return Ok(Vec::new());
        }
        let avgdl = if self.avgdl == 0.0 { 1.0 } else { self.avgdl };
        let n_usize = self.n;

        // df per term
        let mut df_map: HashMap<String, usize> = HashMap::new();
        for term in &query_terms {
            if let Some(list) = self.postings.get(term) {
                // distinct doc count = list len (since term,doc_id is PK, no duplicates)
                df_map.insert(term.clone(), list.len());
            }
        }
        // Gather postings for query terms: doc_id -> term->tf
        let mut doc_term_tfs: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut doc_ids_set: HashSet<String> = HashSet::new();
        for term in &query_terms {
            if let Some(list) = self.postings.get(term) {
                for (doc_id, tf) in list {
                    doc_term_tfs
                        .entry(doc_id.clone())
                        .or_default()
                        .insert(term.clone(), *tf);
                    doc_ids_set.insert(doc_id.clone());
                }
            }
        }
        if doc_ids_set.is_empty() {
            return Ok(Vec::new());
        }
        // Score per doc
        let mut scored: Vec<(String, f64)> = Vec::new();
        for (doc_id, term_tfs) in doc_term_tfs {
            let meta = match self.docs.get(&doc_id) {
                Some(m) => m,
                None => continue,
            };
            let doc_len = meta.length;
            let mut score = 0.0;
            for term in &query_terms {
                if let Some(tf) = term_tfs.get(term) {
                    let df = df_map.get(term).cloned().unwrap_or(1);
                    let idf_val = idf(n_usize, df);
                    score += bm25_term_score(*tf, doc_len, avgdl, idf_val);
                }
            }
            if score > 0.0 {
                scored.push((doc_id, score));
            }
        }
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(limit);
        let mut out = Vec::new();
        for (doc_id, score) in scored {
            if let Some(doc) = self.docs.get(&doc_id) {
                out.push(Bm25Candidate {
                    file: doc.file.clone(),
                    chunk_id: doc.chunk_id.clone(),
                    start_line: doc.start_line,
                    end_line: doc.end_line,
                    symbol: doc.symbol.clone(),
                    score,
                    content_hash: doc.content_hash.clone(),
                });
            }
        }
        Ok(out)
    }

    pub fn doc_count(&self) -> usize {
        self.n
    }
}

/// Hot vector state — contiguous in-memory exact scan.
pub struct HotVectors {
    fingerprint: ModelFingerprint,
    // vectors as (representation_hash, Vec<f32>)
    vectors: Vec<(String, Vec<f32>)>,
    // hash -> chunks
    chunk_map: HashMap<String, Vec<VectorChunkInfo>>,
}

#[derive(Clone, Debug)]
struct VectorChunkInfo {
    chunk_id: String,
    file: String,
    start_line: u32,
    end_line: u32,
    parent_symbol: Option<String>,
    content_hash: String,
}

impl HotVectors {
    pub fn load(
        conn: &rusqlite::Connection,
        fingerprint: &ModelFingerprint,
    ) -> anyhow::Result<Arc<Self>> {
        use rusqlite::params;
        let mut vectors: Vec<(String, Vec<f32>)> = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT representation_hash, vector, dimension FROM vectors WHERE representation_version=?1 AND model_id=?2 AND version=?3 AND dimension=?4",
            )?;
            let rows = stmt.query_map(
                params![
                    vector::SEMANTIC_REPRESENTATION_VERSION,
                    fingerprint.model_id,
                    fingerprint.version,
                    fingerprint.dimension as i64
                ],
                |row| {
                    let hash: String = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let dim: i64 = row.get(2)?;
                    Ok((hash, blob, dim as usize))
                },
            )?;
            for r in rows {
                let (hash, blob, dim) = r?;
                let expected = dim * 4;
                if blob.len() != expected {
                    continue;
                }
                let mut vecf = Vec::with_capacity(dim);
                for chunk in blob.chunks_exact(4) {
                    let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
                    vecf.push(f32::from_le_bytes(arr));
                }
                if vecf.len() != fingerprint.dimension {
                    continue;
                }
                vectors.push((hash, vecf));
            }
        }
        // Load chunk mapping: hash -> chunks via semantic_chunk_refs JOIN chunks
        let mut chunk_map: HashMap<String, Vec<VectorChunkInfo>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT r.representation_hash, c.id, c.file, c.start_line, c.end_line, c.parent_symbol, c.content_hash FROM semantic_chunk_refs r JOIN chunks c ON c.id = r.chunk_id WHERE r.representation_version=?1",
            )?;
            let rows = stmt.query_map(params![vector::SEMANTIC_REPRESENTATION_VERSION], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? as u32,
                    row.get::<_, i64>(4)? as u32,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?;
            for r in rows {
                let (hash, cid, file, sl, el, sym, ch) = r?;
                let info = VectorChunkInfo {
                    chunk_id: cid,
                    file,
                    start_line: sl,
                    end_line: el,
                    parent_symbol: sym,
                    content_hash: ch,
                };
                chunk_map.entry(hash).or_default().push(info);
            }
        }
        // Sort each chunk list deterministically for stable results
        for v in chunk_map.values_mut() {
            v.sort_by(|a, b| {
                a.file
                    .cmp(&b.file)
                    .then_with(|| a.start_line.cmp(&b.start_line))
                    .then_with(|| a.chunk_id.cmp(&b.chunk_id))
            });
        }
        Ok(Arc::new(Self {
            fingerprint: fingerprint.clone(),
            vectors,
            chunk_map,
        }))
    }

    pub fn count(&self) -> usize {
        self.vectors.len()
    }

    pub fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    /// In-memory brute search — same logic as vector::search_brute but without SQLite.
    pub fn search_brute(
        &self,
        query_vec: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<VectorCandidate>> {
        if query_vec.len() != self.fingerprint.dimension {
            anyhow::bail!("dimension mismatch");
        }
        let mut scored: Vec<(String, f32)> = Vec::with_capacity(self.vectors.len());
        for (hash, vec) in &self.vectors {
            if vec.len() != query_vec.len() {
                continue;
            }
            let dot: f32 = vec.iter().zip(query_vec.iter()).map(|(a, b)| a * b).sum();
            scored.push((hash.clone(), dot));
        }
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(limit * 2);
        let mut out = Vec::new();
        for (hash, score) in scored {
            if let Some(chunks) = self.chunk_map.get(&hash) {
                for ci in chunks.iter().take(5) {
                    out.push(VectorCandidate {
                        file: ci.file.clone(),
                        chunk_id: ci.chunk_id.clone(),
                        start_line: ci.start_line,
                        end_line: ci.end_line,
                        symbol: ci.parent_symbol.clone(),
                        content_hash: ci.content_hash.clone(),
                        score: score as f64,
                    });
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.start_line.cmp(&b.start_line))
                .then_with(|| a.chunk_id.cmp(&b.chunk_id))
        });
        out.truncate(limit);
        Ok(out)
    }
}

/// Generation-bound hot retrieval state — read-only after creation.
pub struct HotState {
    pub generation: u64,
    pub fingerprint: ModelFingerprint,
    pub bm25: Arc<HotBm25>,
    pub vectors: Option<Arc<HotVectors>>,
    pub created_at: std::time::Instant,
}

impl HotState {
    pub fn load_blocking(
        root: &Path,
        generation: u64,
        fingerprint: ModelFingerprint,
    ) -> anyhow::Result<Self> {
        let conn = structural_store::open_db(root)?;
        let bm25 = HotBm25::load(&conn)?;
        // Vectors only if backend available and fingerprint matches? We still load if vectors exist.
        let vectors = match HotVectors::load(&conn, &fingerprint) {
            Ok(v) if v.count() > 0 => Some(v),
            _ => None,
        };
        Ok(Self {
            generation,
            fingerprint,
            bm25,
            vectors,
            created_at: std::time::Instant::now(),
        })
    }
}
