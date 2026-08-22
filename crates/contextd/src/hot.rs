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
/// ponytail: compact contiguous matrix + O(K) heap, avoids per-vector heap + O(N) String sort.
pub struct HotVectors {
    fingerprint: ModelFingerprint,
    dimension: usize,
    // contiguous row-major: matrix[i*dimension .. (i+1)*dimension] == vector i
    matrix: Box<[f32]>,
    // hash per row, aligned with matrix, for tie-break and chunk_map lookup
    hashes: Vec<String>,
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
        let mut hashes: Vec<String> = Vec::new();
        let mut flat: Vec<f32> = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT representation_hash, vector, dimension FROM vectors WHERE representation_version=?1 AND model_id=?2 AND version=?3 AND dimension=?4 ORDER BY representation_hash ASC",
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
                if dim != fingerprint.dimension {
                    continue;
                }
                // decode directly into flat
                for chunk in blob.chunks_exact(4) {
                    let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
                    flat.push(f32::from_le_bytes(arr));
                }
                hashes.push(hash);
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
        let count = hashes.len();
        let expected_flat = count * fingerprint.dimension;
        // flat should be exactly count*dim, if not, truncate/pad defensively but we built it correctly
        debug_assert_eq!(flat.len(), expected_flat);
        let matrix: Box<[f32]> = flat.into_boxed_slice();
        Ok(Arc::new(Self {
            fingerprint: fingerprint.clone(),
            dimension: fingerprint.dimension,
            matrix,
            hashes,
            chunk_map,
        }))
    }

    pub fn count(&self) -> usize {
        self.hashes.len()
    }

    pub fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    pub fn estimated_bytes(&self) -> usize {
        self.matrix.len() * 4
            + self.hashes.iter().map(|h| h.len()).sum::<usize>()
            + self.chunk_map.len() * 64
    }

    /// In-memory brute search — same ranking as vector::search_brute, O(K) heap, no per-vector String clone.
    pub fn search_brute(
        &self,
        query_vec: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<VectorCandidate>> {
        if query_vec.len() != self.dimension {
            anyhow::bail!("dimension mismatch");
        }
        if self.hashes.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let cap = (limit * 2).min(self.hashes.len());
        // heap item: better = higher score, smaller hash; min-heap via Reverse keeps worst at top
        #[derive(Clone, Debug)]
        struct HeapItem {
            score: f32,
            hash: String,
            idx: usize,
        }
        impl PartialEq for HeapItem {
            fn eq(&self, other: &Self) -> bool {
                self.score.to_bits() == other.score.to_bits() && self.hash == other.hash
            }
        }
        impl Eq for HeapItem {}
        impl PartialOrd for HeapItem {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for HeapItem {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.score
                    .total_cmp(&other.score)
                    .then_with(|| other.hash.cmp(&self.hash))
            }
        }
        let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<HeapItem>> =
            std::collections::BinaryHeap::with_capacity(cap + 1);
        let dim = self.dimension;
        for (idx, hash) in self.hashes.iter().enumerate() {
            let base = idx * dim;
            let slice = &self.matrix[base..base + dim];
            let mut dot: f32 = 0.0;
            for (a, b) in slice.iter().zip(query_vec.iter()) {
                dot += a * b;
            }
            let item = HeapItem {
                score: dot,
                hash: hash.clone(),
                idx,
            };
            if heap.len() < cap {
                heap.push(std::cmp::Reverse(item));
            } else if let Some(std::cmp::Reverse(worst)) = heap.peek() {
                if item > *worst {
                    heap.pop();
                    heap.push(std::cmp::Reverse(item));
                }
            }
        }
        // drain heap into vec sorted best-first (score desc, hash asc)
        let mut scored: Vec<HeapItem> = heap.into_iter().map(|r| r.0).collect();
        scored.sort_by(|a, b| b.cmp(a));
        let mut out = Vec::new();
        for item in scored {
            if let Some(chunks) = self.chunk_map.get(&item.hash) {
                for ci in chunks.iter().take(5) {
                    out.push(VectorCandidate {
                        file: ci.file.clone(),
                        chunk_id: ci.chunk_id.clone(),
                        start_line: ci.start_line,
                        end_line: ci.end_line,
                        symbol: ci.parent_symbol.clone(),
                        content_hash: ci.content_hash.clone(),
                        score: item.score as f64,
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
        requested_generation: u64,
        fingerprint: ModelFingerprint,
    ) -> anyhow::Result<Self> {
        Self::load_blocking_with_options(root, requested_generation, fingerprint, true)
    }

    pub fn load_blocking_with_options(
        root: &Path,
        requested_generation: u64,
        fingerprint: ModelFingerprint,
        need_vectors: bool,
    ) -> anyhow::Result<Self> {
        let conn = structural_store::open_db(root)?;
        // Use a read transaction for a consistent snapshot across all loads.
        conn.execute("BEGIN", [])?;
        let db_generation = structural_store::get_generation(&conn).unwrap_or(0);
        if db_generation != requested_generation {
            let _ = conn.execute("ROLLBACK", []);
            anyhow::bail!(
                "generation mismatch: requested {} db {}",
                requested_generation,
                db_generation
            );
        }
        let bm25 = HotBm25::load(&conn)?;
        let vectors = if !need_vectors {
            None
        } else {
            // budget check before allocating contiguous matrix
            let budget = crate::config::memory_budget_bytes();
            let est_bytes = context_index::vector::count_vectors(&conn, &fingerprint)
                .map(|c| (c as usize) * fingerprint.dimension * 4)
                .unwrap_or(0);
            if est_bytes > budget {
                tracing::info!(
                    estimated_bytes = est_bytes,
                    budget_bytes = budget,
                    "hot vectors exceed memory budget, using cold fallback"
                );
                None
            } else {
                match HotVectors::load(&conn, &fingerprint) {
                    Ok(v) if v.count() > 0 => Some(v),
                    _ => None,
                }
            }
        };
        let db_generation2 = structural_store::get_generation(&conn).unwrap_or(0);
        if db_generation2 != requested_generation {
            let _ = conn.execute("ROLLBACK", []);
            anyhow::bail!(
                "generation changed during load: requested {} db now {}",
                requested_generation,
                db_generation2
            );
        }
        let _ = conn.execute("COMMIT", []);
        Ok(Self {
            generation: db_generation,
            fingerprint,
            bm25,
            vectors,
            created_at: std::time::Instant::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context_index::bm25::{self};
    use context_index::embed::{Embedder, FakeEmbedder, ModelFingerprint};
    use context_index::structural::store::open_in_memory;
    use context_index::structural::types::Chunk;
    use context_index::vector;

    fn make_test_bm25_conn() -> rusqlite::Connection {
        let mut conn = open_in_memory().expect("mem db");
        // Create 5 files with chunks of varying lengths/content for BM25 parity
        let long_content = "long ".repeat(50);
        let files = vec![
            (
                "a.py",
                "PaymentRetryHandler payment_retry Server.Start backend/payment/retry.go",
                "PaymentRetryHandler",
            ),
            ("b.py", "hello world hello", "hello"),
            ("c.py", "hello world hello", "hello"), // tie with b.py
            ("d.py", long_content.as_str(), "long"),
            (
                "e.py",
                "snake_case and CamelCase and path-ish tokens",
                "snake_case",
            ),
        ];
        for (file, content, symbol) in files {
            let chunk = Chunk {
                id: format!("{}::0", file),
                file: file.to_string(),
                language: context_index::structural::language::Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: content.len(),
                parent_symbol: Some(symbol.to_string()),
                content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
                text_size_bytes: content.len(),
            };
            bm25::upsert_bm25_for_file(&mut conn, file, &[chunk], content).expect("upsert");
        }
        conn
    }

    fn assert_bm25_parity(query: &str, limit: usize) {
        let conn = make_test_bm25_conn();
        let cold = bm25::search_bm25(&conn, query, limit).expect("cold");
        let hot = HotBm25::load(&conn)
            .expect("hot")
            .search(query, limit)
            .expect("hot search");
        // candidate count before top-K is not directly exposed, but we compare returned len and ordering
        assert_eq!(
            cold.len(),
            hot.len(),
            "count mismatch for query {:?}: cold {} hot {}",
            query,
            cold.len(),
            hot.len()
        );
        for (i, (c, h)) in cold.iter().zip(hot.iter()).enumerate() {
            assert_eq!(c.file, h.file, "file mismatch at {} for {:?}", i, query);
            assert_eq!(
                c.chunk_id, h.chunk_id,
                "chunk_id mismatch at {} for {:?}",
                i, query
            );
            assert_eq!(c.start_line, h.start_line, "start_line mismatch");
            assert_eq!(c.end_line, h.end_line, "end_line mismatch");
            let score_diff = (c.score - h.score).abs();
            assert!(
                score_diff < 1e-6,
                "score mismatch at {} for {:?}: cold {} hot {} diff {}",
                i,
                query,
                c.score,
                h.score,
                score_diff
            );
        }
        // deterministic tie ordering: if scores equal, order must be same
        // we already checked file order, so OK
    }

    #[test]
    fn hot_bm25_parity_single_term() {
        assert_bm25_parity("hello", 10);
    }
    #[test]
    fn hot_bm25_parity_multiple_terms() {
        assert_bm25_parity("hello world", 10);
    }
    #[test]
    fn hot_bm25_parity_repeated_term() {
        assert_bm25_parity("hello hello hello", 10);
    }
    #[test]
    fn hot_bm25_parity_absent_term() {
        assert_bm25_parity("nonexistenttermxyz", 10);
    }
    #[test]
    fn hot_bm25_parity_mixed_case() {
        assert_bm25_parity("HeLLo WoRLd", 10);
    }
    #[test]
    fn hot_bm25_parity_punctuation() {
        assert_bm25_parity("Server.Start!", 10);
    }
    #[test]
    fn hot_bm25_parity_snake_case() {
        assert_bm25_parity("payment_retry", 10);
    }
    #[test]
    fn hot_bm25_parity_camel_case() {
        assert_bm25_parity("PaymentRetryHandler", 10);
    }
    #[test]
    fn hot_bm25_parity_path_tokens() {
        assert_bm25_parity("backend/payment/retry.go", 10);
    }
    #[test]
    fn hot_bm25_parity_different_lengths() {
        // long doc should score differently but hot/cold must match
        assert_bm25_parity("long", 10);
    }
    #[test]
    fn hot_bm25_parity_tied_scores() {
        // b.py and c.py have identical content, same score, tie should be deterministic by doc_id
        let conn = make_test_bm25_conn();
        let cold = bm25::search_bm25(&conn, "hello", 10).unwrap();
        let hot = HotBm25::load(&conn).unwrap().search("hello", 10).unwrap();
        // Both should return b.py and c.py in same order (doc_id asc)
        assert!(cold.len() >= 2 && hot.len() >= 2);
        assert_eq!(cold[0].score, cold[1].score, "expected tie for hello");
        assert_eq!(hot[0].score, hot[1].score);
        assert_eq!(cold[0].file, hot[0].file);
        assert_eq!(cold[1].file, hot[1].file);
    }
    #[test]
    fn hot_bm25_parity_topk_truncation() {
        // limit 2 should give same top 2 as limit 10 truncated
        let conn = make_test_bm25_conn();
        let hot_full = HotBm25::load(&conn).unwrap();
        let cold2 = bm25::search_bm25(&conn, "hello", 2).unwrap();
        let hot2 = hot_full.search("hello", 2).unwrap();
        assert_eq!(cold2.len(), 2);
        assert_eq!(hot2.len(), 2);
        for (c, h) in cold2.iter().zip(hot2.iter()) {
            assert_eq!(c.file, h.file);
            assert!((c.score - h.score).abs() < 1e-6);
        }
    }
    #[test]
    fn hot_bm25_parity_deterministic_tie() {
        // Run same query twice, hot must be deterministic
        let conn = make_test_bm25_conn();
        let hot = HotBm25::load(&conn).unwrap();
        let a = hot.search("hello", 10).unwrap();
        let b = hot.search("hello", 10).unwrap();
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.file, y.file);
            assert_eq!(x.chunk_id, y.chunk_id);
        }
    }

    #[tokio::test]
    async fn hot_vectors_parity() {
        let mut conn = open_in_memory().unwrap();
        let embedder = FakeEmbedder::new("test-model", 8);
        let fp = embedder.fingerprint();
        // Create 3 chunks with distinct content
        let chunks = vec![
            Chunk {
                id: "c1".to_string(),
                file: "a.py".to_string(),
                language: context_index::structural::language::Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 5,
                parent_symbol: Some("foo".to_string()),
                content_hash: "hash1".to_string(),
                text_size_bytes: 5,
            },
            Chunk {
                id: "c2".to_string(),
                file: "b.py".to_string(),
                language: context_index::structural::language::Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 5,
                parent_symbol: Some("bar".to_string()),
                content_hash: "hash2".to_string(),
                text_size_bytes: 5,
            },
            Chunk {
                id: "c3".to_string(),
                file: "c.py".to_string(),
                language: context_index::structural::language::Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 4,
                parent_symbol: Some("baz".to_string()),
                content_hash: "hash3".to_string(),
                text_size_bytes: 4,
            },
        ];
        // Insert files/chunks
        for ch in &chunks {
            conn.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                rusqlite::params![ch.file, "filehash", ch.language.as_str()],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![ch.id, ch.file, ch.language.as_str(), ch.start_line as i64, ch.end_line as i64, ch.start_byte as i64, ch.end_byte as i64, ch.parent_symbol, ch.content_hash, ch.text_size_bytes as i64],
            )
            .unwrap();
        }
        // Create vectors via sync_vectors_for_file
        let content_a = "hello";
        let content_b = "world";
        let content_c = "test";
        // Use fake embedder to create vectors for distinct representations
        // We need to create representation hashes via HotVectors load after sync
        // For simplicity, we will directly use vector::sync_vectors_for_file to create vectors
        vector::sync_vectors_for_file(&mut conn, "a.py", &chunks[0..1], content_a, &embedder)
            .await
            .unwrap();
        vector::sync_vectors_for_file(&mut conn, "b.py", &chunks[1..2], content_b, &embedder)
            .await
            .unwrap();
        vector::sync_vectors_for_file(&mut conn, "c.py", &chunks[2..3], content_c, &embedder)
            .await
            .unwrap();

        let qvec = embedder.embed_query("hello").await.unwrap();
        let cold = vector::search_brute(&conn, &qvec, &fp, 5).unwrap();
        let hot = HotVectors::load(&conn, &fp).unwrap();
        let hot_res = hot.search_brute(&qvec, 5).unwrap();

        assert_eq!(cold.len(), hot_res.len(), "vector count mismatch");
        for (c, h) in cold.iter().zip(hot_res.iter()) {
            assert_eq!(c.file, h.file, "file mismatch");
            assert_eq!(c.chunk_id, h.chunk_id, "chunk_id mismatch");
            assert_eq!(c.start_line, h.start_line);
            assert!(
                (c.score - h.score).abs() < 1e-6,
                "score mismatch {} vs {}",
                c.score,
                h.score
            );
        }
        // deterministic tie: if scores equal, order file asc
        // Run twice
        let hot_res2 = hot.search_brute(&qvec, 5).unwrap();
        assert_eq!(hot_res.len(), hot_res2.len());
        for (a, b) in hot_res.iter().zip(hot_res2.iter()) {
            assert_eq!(a.file, b.file);
        }
        // deletion: remove one chunk file, gc, then hot reload should not contain it
        conn.execute("DELETE FROM chunks WHERE file='c.py'", [])
            .unwrap();
        let _ = vector::gc_orphaned_vectors(&conn, &fp);
        let hot2 = HotVectors::load(&conn, &fp).unwrap();
        let hot_res_after = hot2.search_brute(&qvec, 5).unwrap();
        assert!(
            !hot_res_after.iter().any(|c| c.file == "c.py"),
            "deleted file c.py should not appear"
        );
        // generation/fingerprint invalidation: different fp should not hit same vectors
        let fp2 = ModelFingerprint {
            model_id: "other".to_string(),
            version: "v1".to_string(),
            dimension: 8,
        };
        let hot_other = HotVectors::load(&conn, &fp2).unwrap();
        assert_eq!(
            hot_other.count(),
            0,
            "different fingerprint should have 0 vectors"
        );
    }

    #[test]
    fn packer_parity_static_init() {
        use context_rank::packer::{count_tokens, pack_evidence, PackOptions};
        use context_rank::types::{Evidence, EvidenceRelation, QueryType, RetrievalSource};

        let ev = Evidence {
            source: RetrievalSource::Exact,
            file: "a.py".into(),
            start_line: Some(1),
            end_line: Some(1),
            symbol: Some("foo".into()),
            symbol_kind: Some("function".into()),
            text: Some("def foo(): pass".into()),
            score: Some(1.0),
            relation: Some(EvidenceRelation::Definition),
            authority_score: Some(10),
            final_score: Some(30.0),
            provenance: Some("test".into()),
            metadata: None,
        };
        let ranked = vec![ev.clone(); 5];
        // First call initializes static BPE, second uses cached
        let c1 = count_tokens("hello world test pack");
        let c2 = count_tokens("hello world test pack");
        assert_eq!(c1, c2, "token count must be deterministic with static BPE");
        let p1 = pack_evidence(
            &ranked,
            "test",
            QueryType::Symbol,
            PackOptions {
                budget: 1000,
                max_files: 5,
            },
        );
        let p2 = pack_evidence(
            &ranked,
            "test",
            QueryType::Symbol,
            PackOptions {
                budget: 1000,
                max_files: 5,
            },
        );
        assert_eq!(
            p1.token_estimate, p2.token_estimate,
            "pack token estimate must be identical"
        );
        assert_eq!(p1.files, p2.files, "pack files must be identical");
        assert_eq!(p1.markdown, p2.markdown, "pack markdown must be identical");
    }

    #[test]
    fn hot_state_db_generation_validation() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let conn = structural_store::open_db(root).unwrap();
        structural_store::set_generation(&conn, 5).unwrap();
        let fp = ModelFingerprint {
            model_id: "m".into(),
            version: "v".into(),
            dimension: 8,
        };
        let res = HotState::load_blocking(root, 3, fp.clone());
        assert!(
            res.is_err(),
            "should reject generation mismatch: requested 3 db 5"
        );
        let Err(e) = res else { panic!("should err") };
        assert!(e.to_string().contains("generation mismatch"));
        let res2 = HotState::load_blocking(root, 5, fp.clone())
            .expect("should succeed for matching generation");
        assert_eq!(res2.generation, 5);
        assert_eq!(res2.fingerprint, fp);
        structural_store::set_generation(&conn, 6).unwrap();
        let res3 = HotState::load_blocking(root, 5, fp);
        assert!(res3.is_err(), "old G hot must not be built from G+1 DB");
    }
}
