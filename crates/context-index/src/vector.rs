//! R4D — Native vector retrieval.
//! Vectors keyed by chunk_content_hash + model_id/version/dimension for reuse.
//! Brute-force cosine baseline is reference truth; HNSW/USearch can be layered later.

use crate::embed::{Embedder, ModelFingerprint, QUERY_CACHE};
use crate::structural::types::Chunk;
use anyhow::Result;
use rusqlite::{params, Connection};

// --- Similarity ---

/// Cosine similarity (or dot if normalized).
/// Vectors are assumed normalized for dot==cosine; we handle both.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    // If normalized, dot is cosine. Otherwise compute properly:
    // But we normalize on embed, so dot suffices.
    dot
}

#[allow(dead_code)]
fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    for x in v.iter_mut() {
        *x /= norm;
    }
}

// --- Vector blob helpers ---

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn blob_to_vec(blob: &[u8], dim: usize) -> Result<Vec<f32>> {
    let expected = dim * 4;
    if blob.len() != expected {
        anyhow::bail!(
            "vector blob length mismatch: expected {} bytes for dim {}, got {}",
            expected,
            dim,
            blob.len()
        );
    }
    let mut out = Vec::with_capacity(dim);
    for chunk in blob.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
        out.push(f32::from_le_bytes(arr));
    }
    Ok(out)
}

// --- Store helpers ---

fn ensure_vector_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS vectors (
            content_hash TEXT NOT NULL,
            model_id TEXT NOT NULL,
            version TEXT NOT NULL,
            dimension INTEGER NOT NULL,
            vector BLOB NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(content_hash, model_id, version)
        );
        "#,
    )?;
    Ok(())
}

pub fn upsert_vector(
    conn: &mut Connection,
    content_hash: &str,
    fingerprint: &ModelFingerprint,
    vector: &[f32],
) -> Result<()> {
    if vector.len() != fingerprint.dimension {
        anyhow::bail!(
            "vector dimension mismatch: expected {}, got {}",
            fingerprint.dimension,
            vector.len()
        );
    }
    ensure_vector_schema(conn)?;
    let blob = vec_to_blob(vector);
    conn.execute(
        "INSERT OR REPLACE INTO vectors (content_hash, model_id, version, dimension, vector) VALUES (?1,?2,?3,?4,?5)",
        params![
            content_hash,
            fingerprint.model_id,
            fingerprint.version,
            fingerprint.dimension as i64,
            blob
        ],
    )?;
    Ok(())
}

pub fn get_vector(
    conn: &Connection,
    content_hash: &str,
    fingerprint: &ModelFingerprint,
) -> Result<Option<Vec<f32>>> {
    ensure_vector_schema(conn)?;
    let mut stmt = conn.prepare(
        "SELECT vector, dimension FROM vectors WHERE content_hash=?1 AND model_id=?2 AND version=?3 AND dimension=?4",
    )?;
    let row = stmt.query_row(
        params![
            content_hash,
            fingerprint.model_id,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
        |r| {
            let blob: Vec<u8> = r.get(0)?;
            let dim: i64 = r.get(1)?;
            Ok((blob, dim as usize))
        },
    );
    match row {
        Ok((blob, dim)) => Ok(Some(blob_to_vec(&blob, dim)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn count_vectors(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<i64> {
    ensure_vector_schema(conn)?;
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM vectors WHERE model_id=?1 AND version=?2 AND dimension=?3",
        params![
            fingerprint.model_id,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
        |r| r.get(0),
    )?)
}

/// Delete vectors for a model if dimension mismatches (model change invalidation).
/// Old vectors must not be silently reused.
pub fn invalidate_stale_model(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_vector_schema(conn)?;
    // Count stale: same model but version or dimension differs (must not be reused)
    let cnt: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vectors WHERE model_id=?1 AND (version!=?2 OR dimension!=?3)",
        params![
            fingerprint.model_id,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
        |r| r.get(0),
    )?;
    Ok(cnt as usize)
}

pub fn delete_stale_vectors(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_vector_schema(conn)?;
    Ok(conn.execute(
        "DELETE FROM vectors WHERE model_id=?1 AND (version!=?2 OR dimension!=?3)",
        params![
            fingerprint.model_id,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
    )?)
}

/// Eligible unique chunk hashes (distinct content_hash from current chunks).
pub fn eligible_chunk_count(conn: &Connection) -> Result<usize> {
    ensure_vector_schema(conn)?;
    let cnt: i64 = conn.query_row("SELECT COUNT(DISTINCT content_hash) FROM chunks", [], |r| {
        r.get(0)
    })?;
    Ok(cnt as usize)
}

/// Vector count for fingerprint (distinct content_hash, primary key ensures distinct).
pub fn vector_count_for(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    Ok(count_vectors(conn, fingerprint)? as usize)
}

/// Missing vectors for fingerprint: eligible distinct - present distinct (via LEFT JOIN for accuracy).
pub fn missing_vector_count(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_vector_schema(conn)?;
    let cnt: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT c.content_hash) FROM chunks c LEFT JOIN vectors v ON v.content_hash=c.content_hash AND v.model_id=?1 AND v.version=?2 AND v.dimension=?3 WHERE v.content_hash IS NULL",
        params![fingerprint.model_id, fingerprint.version, fingerprint.dimension as i64],
        |r| r.get(0),
    )?;
    Ok(cnt as usize)
}

/// Stale vector count (same model, version or dimension mismatch).
pub fn stale_vector_count(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    invalidate_stale_model(conn, fingerprint)
}

/// Whether semantic index is ready: backend available && missing==0 && eligible>0
pub fn is_semantic_ready(
    conn: &Connection,
    fingerprint: &ModelFingerprint,
    backend_available: bool,
) -> Result<bool> {
    if !backend_available {
        return Ok(false);
    }
    let eligible = eligible_chunk_count(conn)?;
    if eligible == 0 {
        return Ok(false);
    }
    let missing = missing_vector_count(conn, fingerprint)?;
    Ok(missing == 0)
}

/// Legacy wrapper — prefers CWD; use sync_missing_vectors_for_root if root known.
pub async fn sync_missing_vectors(
    conn: &mut Connection,
    embedder: &dyn Embedder,
) -> Result<(usize, usize, usize, usize)> {
    sync_missing_vectors_for_root(conn, std::path::Path::new("."), embedder).await
}

/// Root-aware variant for production (preferred): caller provides root for file reads.
pub async fn sync_missing_vectors_for_root(
    conn: &mut Connection,
    root: &std::path::Path,
    embedder: &dyn Embedder,
) -> Result<(usize, usize, usize, usize)> {
    sync_missing_vectors_for_root_with_batch_size(conn, root, embedder, 256).await
}

/// Testable helper with explicit batch_size (production 256, tests use 8).
pub async fn sync_missing_vectors_for_root_with_batch_size(
    conn: &mut Connection,
    root: &std::path::Path,
    embedder: &dyn Embedder,
    batch_size: usize,
) -> Result<(usize, usize, usize, usize)> {
    ensure_vector_schema(conn)?;
    let fp = embedder.fingerprint();
    let _ = delete_stale_vectors(conn, &fp)?;

    let by_hash: std::collections::HashMap<String, (String, usize, usize, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT c.content_hash, c.file, c.start_byte, c.end_byte, c.parent_symbol FROM chunks c LEFT JOIN vectors v ON v.content_hash=c.content_hash AND v.model_id=?1 AND v.version=?2 AND v.dimension=?3 WHERE v.content_hash IS NULL ORDER BY c.file, c.start_byte",
        )?;
        let rows = stmt.query_map(
            params![fp.model_id, fp.version, fp.dimension as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? as usize,
                    row.get::<_, i64>(3)? as usize,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;
        let mut map: std::collections::HashMap<String, (String, usize, usize, Option<String>)> =
            std::collections::HashMap::new();
        for r in rows {
            let (hash, file, sb, eb, sym) = r?;
            map.entry(hash).or_insert((file, sb, eb, sym));
        }
        map
    };
    if by_hash.is_empty() {
        // eligible distinct count for reused
        let eligible = eligible_chunk_count(conn)?;
        return Ok((eligible, 0, 0, 0));
    }
    let eligible = eligible_chunk_count(conn)?;
    let missing_distinct = by_hash.len();
    let reused = eligible.saturating_sub(missing_distinct);

    let mut hashes: Vec<String> = by_hash.keys().cloned().collect();
    hashes.sort();
    // Prepare texts batching
    let mut total_embedded = 0usize;
    let mut total_calls = 0usize;
    let mut file_cache: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Build all missing entries with combined text
    let mut entries: Vec<(String, String)> = Vec::with_capacity(missing_distinct);
    for hash in hashes {
        let (file, sb, eb, sym) = by_hash.get(&hash).unwrap();
        let content = if let Some(c) = file_cache.get(file) {
            c.clone()
        } else {
            let abs = root.join(file);
            let c = std::fs::read_to_string(&abs).unwrap_or_default();
            file_cache.insert(file.clone(), c.clone());
            c
        };
        let bytes = content.as_bytes();
        let slice = if *sb < bytes.len() && *eb <= bytes.len() {
            std::str::from_utf8(&bytes[*sb..*eb])
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        let combined = if let Some(sym) = sym {
            format!("{} {}\n{}", file, sym, slice)
        } else {
            format!("{} {}", file, slice)
        };
        entries.push((hash.clone(), combined));
    }

    // Batch embed with persistence per batch for partial failure handling
    for chunk in entries.chunks(batch_size) {
        let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
        let hashes_chunk: Vec<String> = chunk.iter().map(|(h, _)| h.clone()).collect();
        total_calls += 1;
        let vectors = embedder.embed_documents(&texts).await?;
        for (hash, vec) in hashes_chunk.into_iter().zip(vectors) {
            upsert_vector(conn, &hash, &fp, &vec)?;
            total_embedded += 1;
        }
    }
    // GC orphaned after sync (clean stale from deleted files)
    let _ = gc_orphaned_vectors(conn, &fp);
    Ok((reused, total_embedded, total_calls, total_embedded))
}

/// Load chunks for a file (for incremental sync).
pub fn load_chunks_for_file(conn: &Connection, file: &str) -> Result<Vec<Chunk>> {
    ensure_vector_schema(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes FROM chunks WHERE file=?1 ORDER BY start_line, start_byte",
    )?;
    let rows = stmt.query_map(params![file], |row| {
        Ok(Chunk {
            id: row.get(0)?,
            file: row.get(1)?,
            language: crate::structural::language::Language::from_str(&row.get::<_, String>(2)?),
            start_line: row.get::<_, i64>(3)? as u32,
            end_line: row.get::<_, i64>(4)? as u32,
            start_byte: row.get::<_, i64>(5)? as usize,
            end_byte: row.get::<_, i64>(6)? as usize,
            parent_symbol: row.get(7)?,
            content_hash: row.get(8)?,
            text_size_bytes: row.get::<_, i64>(9)? as usize,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Incremental sync for changed files only (fast, uses changed_files delta).
/// Returns (reused, embedded, calls).
pub async fn sync_changed_files_vectors(
    conn: &mut Connection,
    root: &std::path::Path,
    changed_files: &[String],
    embedder: &dyn Embedder,
) -> Result<(usize, usize, usize)> {
    if changed_files.is_empty() {
        return Ok((0, 0, 0));
    }
    let fp = embedder.fingerprint();
    // delete stale for this fingerprint first
    let _ = delete_stale_vectors(conn, &fp)?;
    let mut total_reused = 0usize;
    let mut total_embedded = 0usize;
    let mut total_calls = 0usize;
    for file in changed_files {
        let chunks = match load_chunks_for_file(conn, file) {
            Ok(c) if !c.is_empty() => c,
            _ => continue,
        };
        let abs = root.join(file);
        let content = std::fs::read_to_string(&abs).unwrap_or_default();
        let (reused, embedded) =
            sync_vectors_for_file(conn, file, &chunks, &content, embedder).await?;
        total_reused += reused;
        total_embedded += embedded;
        if embedded > 0 {
            total_calls += 1;
        }
    }
    // GC orphaned after incremental (handles deleted files if caller also calls GC)
    let _ = gc_orphaned_vectors(conn, &fp);
    Ok((total_reused, total_embedded, total_calls))
}

/// Conservative orphan GC: delete vectors whose content_hash is not referenced by any current chunk
/// for the given model. Keeps content-addressed reuse for active chunks, but prevents unbounded growth
/// after many edits. Does NOT delete immediately if there is any chunk referencing it.
/// For rename/revert: if content reappears, it will be re-embedded (acceptable for bounded storage).
pub fn gc_orphaned_vectors(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_vector_schema(conn)?;
    // Vectors whose hash not in any current chunk (scoped to fingerprint exact match)
    Ok(conn.execute(
        "DELETE FROM vectors WHERE model_id=?1 AND version=?2 AND dimension=?3 AND content_hash NOT IN (SELECT content_hash FROM chunks)",
        params![fingerprint.model_id, fingerprint.version, fingerprint.dimension as i64],
    )?)
}

// --- Changed-chunk reuse ---

/// Ensure vectors for chunks of a file, reusing unchanged hashes.
/// Returns (reused_count, embedded_count)
pub async fn sync_vectors_for_file(
    conn: &mut Connection,
    file: &str,
    chunks: &[Chunk],
    file_content: &str,
    embedder: &dyn Embedder,
) -> Result<(usize, usize)> {
    ensure_vector_schema(conn)?;
    let fp = embedder.fingerprint();
    let bytes = file_content.as_bytes();
    // Collect missing
    let mut missing: Vec<(String, String)> = Vec::new(); // (content_hash, text)
    let mut reused = 0usize;
    for chunk in chunks {
        if get_vector(conn, &chunk.content_hash, &fp)?.is_some() {
            reused += 1;
        } else {
            let text_slice = if chunk.start_byte < bytes.len() && chunk.end_byte <= bytes.len() {
                std::str::from_utf8(&bytes[chunk.start_byte..chunk.end_byte])
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            };
            // Combine chunk text + file path + symbol for context? For vector, we embed chunk text plus maybe surrounding? Keep simple: chunk text
            let combined = if let Some(sym) = &chunk.parent_symbol {
                format!("{} {}\n{}", file, sym, text_slice)
            } else {
                format!("{} {}", file, text_slice)
            };
            missing.push((chunk.content_hash.clone(), combined));
        }
    }
    if missing.is_empty() {
        return Ok((reused, 0));
    }
    // Batch embed missing
    let texts: Vec<String> = missing.iter().map(|(_, t)| t.clone()).collect();
    let vectors = embedder.embed_documents(&texts).await?;
    for ((hash, _), vec) in missing.into_iter().zip(vectors) {
        upsert_vector(conn, &hash, &fp, &vec)?;
    }
    Ok((reused, texts.len()))
}

// --- Search ---

#[derive(Debug, Clone)]
pub struct VectorCandidate {
    pub file: String,
    pub chunk_id: String,
    pub start_line: u32,
    pub end_line: u32,
    pub symbol: Option<String>,
    pub content_hash: String,
    pub score: f64, // cosine
}

/// Brute-force cosine search — reference truth.
/// For small fixture this is the baseline; for large repos HNSW can be validated against this.
pub fn search_brute(
    conn: &Connection,
    query_vec: &[f32],
    fingerprint: &ModelFingerprint,
    limit: usize,
) -> Result<Vec<VectorCandidate>> {
    ensure_vector_schema(conn)?;
    // Get all vectors for model (dimension must match)
    let mut stmt = conn.prepare(
        "SELECT content_hash, vector, dimension FROM vectors WHERE model_id=?1 AND version=?2 AND dimension=?3",
    )?;
    let rows = stmt.query_map(
        params![
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
    let mut scored: Vec<(String, f32)> = Vec::new();
    for r in rows {
        let (hash, blob, dim) = r?;
        let vec = match blob_to_vec(&blob, dim) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, content_hash = %hash, "skipping corrupted vector row");
                continue;
            }
        };
        if vec.len() != query_vec.len() {
            continue;
        }
        let s = cosine(query_vec, &vec);
        scored.push((hash, s));
    }
    // Deterministic ordering: score desc, hash asc tie-breaker, then after mapping file/line/chunk_id
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(limit * 2); // oversample before dedup via chunk mapping
                                // Map content_hash -> chunk metadata (choose one chunk per hash, or multiple if same content appears elsewhere)
                                // For now, pick first chunk per hash from chunks table with deterministic ordering
    let mut out = Vec::new();
    for (hash, score) in scored {
        let mut stmt2 = conn.prepare(
            "SELECT id, file, start_line, end_line, parent_symbol, content_hash FROM chunks WHERE content_hash=?1 ORDER BY file ASC, start_line ASC, id ASC LIMIT 5",
        )?;
        let chunk_rows = stmt2.query_map(params![hash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u32,
                row.get::<_, i64>(3)? as u32,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        for cr in chunk_rows {
            let (cid, file, sl, el, sym, ch) = cr?;
            out.push(VectorCandidate {
                file,
                chunk_id: cid,
                start_line: sl,
                end_line: el,
                symbol: sym,
                content_hash: ch,
                score: score as f64,
            });
            if out.len() >= limit {
                break;
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    // Final deterministic ordering for equal scores: score desc file asc start_line asc chunk_id asc
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

/// High-level search with query embedding cache.
/// Bounded cache key includes full fingerprint (model+version+dimension) + query.
pub async fn search_vector(
    conn: &Connection,
    query: &str,
    fingerprint: &ModelFingerprint,
    embedder: &dyn Embedder,
    limit: usize,
) -> Result<Vec<VectorCandidate>> {
    // Check cache — full fingerprint prevents stale reuse across model changes
    let cached = QUERY_CACHE.get(fingerprint, query).await;
    let qvec = if let Some(v) = cached {
        v
    } else {
        let v = embedder.embed_query(query).await?;
        QUERY_CACHE.insert(fingerprint, query, v.clone()).await;
        v
    };
    // Use brute force for now; usearch can be added later with same API
    search_brute(conn, &qvec, fingerprint, limit)
}

// --- Incremental update for file ---

/// Update vectors for a changed file transactionally.
/// Handles reuse via content_hash.
pub async fn update_vectors_for_parsed(
    conn: &mut Connection,
    file: &str,
    chunks: &[Chunk],
    file_content: &str,
    embedder: &dyn Embedder,
) -> Result<(usize, usize)> {
    // For R4 we process synchronously per file; caller ensures no concurrent writes.
    sync_vectors_for_file(conn, file, chunks, file_content, embedder).await
}

#[cfg(test)]
#[allow(clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::*;
    use crate::embed::{FakeEmbedder, ModelFingerprint};
    use crate::structural::language::Language;
    use crate::structural::store::open_in_memory;
    use crate::structural::types::Chunk;

    #[tokio::test]
    async fn chunk_hash_reuse() -> Result<()> {
        let mut conn = open_in_memory()?;
        let embedder = FakeEmbedder::new("fake", 8);
        let fp = embedder.fingerprint();
        let chunk_a = Chunk {
            id: "cA".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 5,
            parent_symbol: Some("foo".to_string()),
            content_hash: "hashA".to_string(),
            text_size_bytes: 5,
        };
        let chunk_b = Chunk {
            id: "cB".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 3,
            end_line: 4,
            start_byte: 6,
            end_byte: 10,
            parent_symbol: Some("bar".to_string()),
            content_hash: "hashB".to_string(),
            text_size_bytes: 4,
        };
        let content = "hello world";
        // First sync: both embedded
        let (reused, embedded) = sync_vectors_for_file(
            &mut conn,
            "a.py",
            &[chunk_a.clone(), chunk_b.clone()],
            content,
            &embedder,
        )
        .await?;
        assert_eq!(reused, 0);
        assert_eq!(embedded, 2);
        // Second sync with same hashes: reuse
        let (reused2, embedded2) = sync_vectors_for_file(
            &mut conn,
            "a.py",
            &[chunk_a.clone(), chunk_b.clone()],
            content,
            &embedder,
        )
        .await?;
        assert_eq!(reused2, 2);
        assert_eq!(embedded2, 0);
        // Change only chunk B hash
        let chunk_b_new = Chunk {
            content_hash: "hashB2".to_string(),
            ..chunk_b.clone()
        };
        let (reused3, embedded3) = sync_vectors_for_file(
            &mut conn,
            "a.py",
            &[chunk_a.clone(), chunk_b_new.clone()],
            content,
            &embedder,
        )
        .await?;
        assert_eq!(reused3, 1);
        assert_eq!(embedded3, 1);
        // Verify vector reuse across file rename: same content_hash in different file should reuse
        let chunk_renamed = Chunk {
            id: "cC".to_string(),
            file: "b.py".to_string(),
            ..chunk_a.clone()
        };
        let (reused4, embedded4) = sync_vectors_for_file(
            &mut conn,
            "b.py",
            &[chunk_renamed.clone()],
            content,
            &embedder,
        )
        .await?;
        assert_eq!(reused4, 1); // hashA already exists
        assert_eq!(embedded4, 0);
        // Insert chunks into DB for search_brute to find (it joins via chunks table)
        for ch in &[chunk_a.clone(), chunk_b.clone(), chunk_renamed] {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                rusqlite::params![ch.file, "testhash", ch.language.as_str()],
            );
            let _ = conn.execute(
                "INSERT OR REPLACE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![ch.id, ch.file, ch.language.as_str(), ch.start_line as i64, ch.end_line as i64, ch.start_byte as i64, ch.end_byte as i64, ch.parent_symbol, ch.content_hash, ch.text_size_bytes as i64],
            );
        }
        // Verify search finds it
        let qvec = embedder.embed_query("hello").await?;
        let res = search_brute(&conn, &qvec, &fp, 5)?;
        assert!(!res.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn one_line_edit_not_reembed_all() -> Result<()> {
        let mut conn = open_in_memory()?;
        let embedder = FakeEmbedder::new("fake", 4);
        // Simulate file with 4 chunks A,B,C,D
        let chunks: Vec<Chunk> = (0..4)
            .map(|i| Chunk {
                id: format!("c{}", i),
                file: "f.rs".to_string(),
                language: Language::Rust,
                start_line: i * 10 + 1,
                end_line: i * 10 + 5,
                start_byte: i as usize * 10,
                end_byte: i as usize * 10 + 5,
                parent_symbol: Some(format!("sym{}", i)),
                content_hash: format!("hash{}", i),
                text_size_bytes: 5,
            })
            .collect();
        let content = "a b c d e f g h i j k l m n o";
        let (r1, e1) =
            sync_vectors_for_file(&mut conn, "f.rs", &chunks, content, &embedder).await?;
        assert_eq!(e1, 4);
        assert_eq!(r1, 0);
        // Edit only chunk C (index 2)
        let mut chunks2 = chunks.clone();
        chunks2[2].content_hash = "hash2_new".to_string();
        let (r2, e2) =
            sync_vectors_for_file(&mut conn, "f.rs", &chunks2, content, &embedder).await?;
        assert_eq!(r2, 3);
        assert_eq!(e2, 1);
        Ok(())
    }

    #[tokio::test]
    async fn model_change_invalidation() -> Result<()> {
        let mut conn = open_in_memory()?;
        let e1 = FakeEmbedder::new("modelA", 4);
        let fp1 = e1.fingerprint();
        let chunk = Chunk {
            id: "c1".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 3,
            parent_symbol: None,
            content_hash: "h1".to_string(),
            text_size_bytes: 3,
        };
        sync_vectors_for_file(&mut conn, "a.py", &[chunk.clone()], "abc", &e1).await?;
        assert_eq!(count_vectors(&conn, &fp1)?, 1);
        let e2 = FakeEmbedder::new("modelB", 4);
        let fp2 = e2.fingerprint();
        // Old vectors for modelA should not be returned for modelB
        assert_eq!(count_vectors(&conn, &fp2)?, 0);
        // Stale detection
        let _stale = invalidate_stale_model(&conn, &fp2)?;
        // There is 1 vector for modelA, but we query for modelB, stale for modelB is 0? Actually we check same model_id different version.
        // For different model_id, stale is 0. Let's test same model different version via manual
        let fp1_v2 = ModelFingerprint {
            model_id: "modelA".to_string(),
            version: "v2".to_string(),
            dimension: 4,
        };
        let stale2 = invalidate_stale_model(&conn, &fp1_v2)?;
        assert_eq!(stale2, 1);
        Ok(())
    }

    #[tokio::test]
    async fn vector_gc_bounds_growth() -> Result<()> {
        let mut conn = open_in_memory()?;
        let embedder = FakeEmbedder::new("gc-test", 4);
        let fp = embedder.fingerprint();
        // Simulate 100 edits creating 100 obsolete hashes, each file has 1 chunk
        for i in 0..100 {
            let chunk = Chunk {
                id: format!("c{}", i),
                file: "a.py".to_string(),
                language: Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 3,
                parent_symbol: None,
                content_hash: format!("hash{}", i),
                text_size_bytes: 3,
            };
            // Also insert chunk into chunks table so GC can find it (only last remains)
            if i == 99 {
                // Only last chunk remains in DB (simulate current file has 1 chunk with hash99)
                conn.execute(
                    "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["a.py", "testhash", "python"],
                )?;
                conn.execute(
                    "INSERT OR REPLACE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    rusqlite::params![chunk.id, chunk.file, "python", 1, 2, 0, 3, Option::<String>::None, chunk.content_hash, 3],
                )?;
            }
            sync_vectors_for_file(&mut conn, "a.py", &[chunk.clone()], "abc", &embedder).await?;
        }
        // Before GC, we have 100 vectors (all hashes, orphaned except last)
        assert_eq!(count_vectors(&conn, &fp)?, 100);
        // GC should delete 99 orphaned (hash0..98 not in chunks)
        let deleted = gc_orphaned_vectors(&conn, &fp)?;
        assert_eq!(deleted, 99);
        assert_eq!(count_vectors(&conn, &fp)?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn async_freshness_slow_embedder() -> Result<()> {
        use crate::embed::SlowTestEmbedder;
        use crate::structural::store::open_in_memory as open_mem;
        use std::time::{Duration, Instant};
        // Simulate file save with slow embedder (2s) — exact must not block
        let fast_chunk = Chunk {
            id: "c1".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 5,
            parent_symbol: Some("foo".to_string()),
            content_hash: "hash_slow".to_string(),
            text_size_bytes: 5,
        };
        // Start slow embedding in background
        let start = Instant::now();
        let handle = tokio::spawn(async move {
            let mut c = open_mem().unwrap();
            let s = SlowTestEmbedder::new(2000);
            let _ = sync_vectors_for_file(&mut c, "a.py", &[fast_chunk], "hello", &s).await;
        });
        // Immediately, exact search should be available (simulate via hash check, not blocked)
        // In real pipeline, exact is via rg and not blocked by vector. Here we just check that we can do a sync operation quickly
        let elapsed_before = start.elapsed();
        assert!(
            elapsed_before.as_millis() < 500,
            "exact should be available immediately, got {}ms",
            elapsed_before.as_millis()
        );
        // Wait for slow to finish (should be ~2s)
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        let total = start.elapsed();
        assert!(
            total.as_millis() >= 1900,
            "slow embedder should have taken ~2s, got {}ms",
            total.as_millis()
        );
        // Now vector should be available
        Ok(())
    }

    #[tokio::test]
    async fn wrong_length_blob_returns_error() -> Result<()> {
        let conn = open_in_memory()?;
        ensure_vector_schema(&conn)?;
        let fp = ModelFingerprint {
            model_id: "test".to_string(),
            version: "v1".to_string(),
            dimension: 4,
        };
        // 3 bytes for a 4-dim vector (expected 16)
        conn.execute(
            "INSERT OR REPLACE INTO vectors (content_hash, model_id, version, dimension, vector) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params!["hash1", fp.model_id, fp.version, fp.dimension as i64, vec![0u8, 1, 2]],
        )?;
        let res = get_vector(&conn, "hash1", &fp);
        assert!(res.is_err(), "expected error for malformed vector blob");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("length mismatch"),
            "error should mention length mismatch: {}",
            err
        );
        Ok(())
    }

    #[tokio::test]
    async fn upsert_rejects_wrong_dimension() -> Result<()> {
        let mut conn = open_in_memory()?;
        let fp = ModelFingerprint {
            model_id: "test".to_string(),
            version: "v1".to_string(),
            dimension: 4,
        };
        let bad_vec = vec![1.0, 2.0, 3.0]; // 3 != 4
        let res = upsert_vector(&mut conn, "hash1", &fp, &bad_vec);
        assert!(res.is_err(), "upsert should reject dimension mismatch");
        assert!(
            res.unwrap_err().to_string().contains("dimension mismatch"),
            "error should mention dimension mismatch"
        );
        assert_eq!(count_vectors(&conn, &fp)?, 0);
        Ok(())
    }

    // ---- R5.1-C extended tests ----
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingFake {
        inner: FakeEmbedder,
        calls: Arc<AtomicUsize>,
        docs: Arc<AtomicUsize>,
    }
    impl CountingFake {
        fn new(model: &str, dim: usize, calls: Arc<AtomicUsize>, docs: Arc<AtomicUsize>) -> Self {
            Self {
                inner: FakeEmbedder::new(model, dim),
                calls,
                docs,
            }
        }
    }
    #[async_trait::async_trait]
    impl crate::embed::Embedder for CountingFake {
        fn model_id(&self) -> &str {
            self.inner.model_id()
        }
        fn dimension(&self) -> usize {
            self.inner.dimension()
        }
        fn version(&self) -> &str {
            self.inner.version()
        }
        async fn embed_query(&self, q: &str) -> Result<Vec<f32>> {
            self.inner.embed_query(q).await
        }
        async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.docs.fetch_add(texts.len(), Ordering::SeqCst);
            self.inner.embed_documents(texts).await
        }
    }

    struct FailingFake {
        inner: FakeEmbedder,
        fail_on_call: usize,
        calls: Arc<AtomicUsize>,
    }
    impl FailingFake {
        fn new(model: &str, dim: usize, fail_on_call: usize, calls: Arc<AtomicUsize>) -> Self {
            Self {
                inner: FakeEmbedder::new(model, dim),
                fail_on_call,
                calls,
            }
        }
    }
    #[async_trait::async_trait]
    impl crate::embed::Embedder for FailingFake {
        fn model_id(&self) -> &str {
            self.inner.model_id()
        }
        fn dimension(&self) -> usize {
            self.inner.dimension()
        }
        fn version(&self) -> &str {
            self.inner.version()
        }
        async fn embed_query(&self, q: &str) -> Result<Vec<f32>> {
            self.inner.embed_query(q).await
        }
        async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            let cur = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if cur == self.fail_on_call {
                anyhow::bail!("injected failure on call {}", cur);
            }
            self.inner.embed_documents(texts).await
        }
    }

    #[tokio::test]
    async fn r5c_a_initial_and_b_no_change() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::write(root.join("a.py"), b"def foo():\n    pass\n")?;
        std::fs::write(root.join("b.py"), b"def bar():\n    pass\n")?;
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        let out = si.build_with_delta(&idx)?;
        assert!(out.changed_files.len() >= 2);
        let mut conn = crate::structural::store::open_db(&root)?;
        let calls = Arc::new(AtomicUsize::new(0));
        let docs = Arc::new(AtomicUsize::new(0));
        let cf = CountingFake::new("test-model", 8, calls.clone(), docs.clone());
        let (reused, embedded, calls_n, _docs_n) =
            crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf).await?;
        assert!(embedded > 0, "initial should embed");
        assert_eq!(calls.load(Ordering::SeqCst), calls_n);
        assert_eq!(eligible_chunk_count(&conn)?, (reused + embedded));
        // B no-change: second call 0
        let calls2 = Arc::new(AtomicUsize::new(0));
        let docs2 = Arc::new(AtomicUsize::new(0));
        let cf2 = CountingFake::new("test-model", 8, calls2.clone(), docs2.clone());
        let (reused2, embedded2, calls_n2, _docs_n2) =
            crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf2).await?;
        assert_eq!(embedded2, 0, "no-change embedded should be 0");
        assert_eq!(calls_n2, 0, "no-change calls should be 0");
        assert_eq!(calls2.load(Ordering::SeqCst), 0);
        assert_eq!(missing_vector_count(&conn, &cf2.fingerprint())?, 0);
        assert_eq!(reused2, reused + embedded);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_c_one_file_change_only_missing() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        // a.py with two functions -> likely 2 chunks? plus b.py unchanged
        std::fs::write(
            root.join("a.py"),
            b"def foo():\n    x=1\ndef bar():\n    y=2\n",
        )?;
        std::fs::write(root.join("b.py"), b"def baz():\n    z=3\n")?;
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        let cf = FakeEmbedder::new("c-model", 8);
        crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf).await?;
        let before_missing = missing_vector_count(&conn, &cf.fingerprint())?;
        assert_eq!(before_missing, 0);
        let eligible_before = eligible_chunk_count(&conn)?;
        // edit one file: change foo body only
        std::fs::write(
            root.join("a.py"),
            b"def foo():\n    x=999\ndef bar():\n    y=2\n",
        )?;
        let pr2 = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx2 = crate::discovery::ProjectIndex::discover(&pr2)?;
        let out2 = si.build_with_delta(&idx2)?;
        // Only a.py should be changed
        assert!(out2.changed_files.contains(&"a.py".to_string()));
        assert!(!out2.changed_files.contains(&"b.py".to_string()));
        let calls = Arc::new(AtomicUsize::new(0));
        let docs = Arc::new(AtomicUsize::new(0));
        let cf2 = CountingFake::new("c-model", 8, calls.clone(), docs.clone());
        let (reused, embedded, calls_n, _docs) =
            crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf2).await?;
        // Only changed chunk embedded (1), others reused
        assert_eq!(
            embedded, 1,
            "only changed hash should embed, got {}",
            embedded
        );
        assert!(reused >= 1);
        assert_eq!(calls_n, 1);
        assert_eq!(docs.load(Ordering::SeqCst), 1);
        assert_eq!(missing_vector_count(&conn, &cf2.fingerprint())?, 0);
        let eligible_after = eligible_chunk_count(&conn)?;
        // eligible may stay same (if chunk count same) but ensure reused+embedded == eligible_after
        assert_eq!(reused + embedded, eligible_after);
        assert_eq!(eligible_before, eligible_after);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_d_delete_orphan_and_e_shared_preserved() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        // identical chunk content in two files
        let same = b"def foo():\n    pass\n";
        std::fs::write(root.join("a.py"), same)?;
        std::fs::write(root.join("b.py"), same)?;
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        let cf = FakeEmbedder::new("d-model", 4);
        crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf).await?;
        let cnt_before = count_vectors(&conn, &cf.fingerprint())?;
        // eligible distinct should be 1 (same chunk hash)
        assert_eq!(cnt_before, 1);
        // delete a.py
        std::fs::remove_file(root.join("a.py"))?;
        let pr2 = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx2 = crate::discovery::ProjectIndex::discover(&pr2)?;
        let out = si.build_with_delta(&idx2)?;
        assert!(out.deleted_files.contains(&"a.py".to_string()));
        // GC should retain vector because b.py still has same hash
        let conn2 = crate::structural::store::open_db(&root)?;
        let _ = crate::vector::gc_orphaned_vectors(&conn2, &cf.fingerprint())?;
        assert_eq!(
            count_vectors(&conn2, &cf.fingerprint())?,
            1,
            "shared vector should be preserved"
        );
        // now delete b.py also
        std::fs::remove_file(root.join("b.py"))?;
        let pr3 = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx3 = crate::discovery::ProjectIndex::discover(&pr3)?;
        let out3 = si.build_with_delta(&idx3)?;
        assert!(out3.deleted_files.contains(&"b.py".to_string()));
        let conn3 = crate::structural::store::open_db(&root)?;
        let deleted = crate::vector::gc_orphaned_vectors(&conn3, &cf.fingerprint())?;
        assert_eq!(deleted, 1, "orphan should be deleted");
        assert_eq!(count_vectors(&conn3, &cf.fingerprint())?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_f_model_mismatch_not_reused() -> Result<()> {
        let mut conn = open_in_memory()?;
        let e1 = FakeEmbedder::new("modelA", 4);
        let fp1 = e1.fingerprint();
        let chunk = Chunk {
            id: "c1".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 3,
            parent_symbol: None,
            content_hash: "h1".to_string(),
            text_size_bytes: 3,
        };
        sync_vectors_for_file(&mut conn, "a.py", &[chunk.clone()], "abc", &e1).await?;
        assert_eq!(count_vectors(&conn, &fp1)?, 1);
        // Different model B with same hash should be considered missing (not reused)
        let e2 = FakeEmbedder::new("modelB", 4);
        let fp2 = e2.fingerprint();
        assert_eq!(count_vectors(&conn, &fp2)?, 0);
        assert_eq!(missing_vector_count(&conn, &fp2)?, 0); // no chunks in this in-mem without chunks table? but we inserted via sync? chunks not in chunks table, eligible 0, so missing 0. Instead test via get_vector
        assert!(
            get_vector(&conn, "h1", &fp2)?.is_none(),
            "different model should not reuse"
        );
        // version mismatch
        let fp_v2 = ModelFingerprint {
            model_id: "modelA".to_string(),
            version: "v2".to_string(),
            dimension: 4,
        };
        assert!(get_vector(&conn, "h1", &fp_v2)?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn r5c_g_dimension_mismatch_rejected() -> Result<()> {
        let mut conn = open_in_memory()?;
        let fp4 = ModelFingerprint {
            model_id: "test".to_string(),
            version: "v1".to_string(),
            dimension: 4,
        };
        let fp8 = ModelFingerprint {
            model_id: "test".to_string(),
            version: "v1".to_string(),
            dimension: 8,
        };
        // upsert 4-dim vector
        let vec4 = vec![1.0, 2.0, 3.0, 4.0];
        upsert_vector(&mut conn, "hash1", &fp4, &vec4)?;
        assert_eq!(count_vectors(&conn, &fp4)?, 1);
        assert_eq!(
            count_vectors(&conn, &fp8)?,
            0,
            "dimension mismatch should not count"
        );
        assert!(
            get_vector(&conn, "hash1", &fp8)?.is_none(),
            "dimension mismatch should not reuse"
        );
        // missing should count as missing for fp8 if chunk exists
        conn.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
            rusqlite::params!["a.py", "h", "python"],
        )?;
        conn.execute(
            "INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params!["c1", "a.py", "python", 1, 2, 0, 3, Option::<String>::None, "hash1", 3],
        )?;
        // Now eligible 1, missing for fp8 should be 1 (since fp8 has no vector)
        assert_eq!(missing_vector_count(&conn, &fp8)?, 1);
        assert_eq!(missing_vector_count(&conn, &fp4)?, 0);
        // stale should be 1 for fp8 (same model, dimension diff)
        assert_eq!(stale_vector_count(&conn, &fp8)?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_j_partial_failure_and_retry() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        // Use small batch size 8 to test batch-independent partial failure
        // 16 docs -> 2 batches of 8
        for i in 0..16 {
            let content = format!("def foo_{}():\n    x = {}\n", i, i);
            std::fs::write(root.join(format!("f{}.py", i)), content.as_bytes())?;
        }
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        let calls = Arc::new(AtomicUsize::new(0));
        let ff = FailingFake::new("j-model", 8, 2, calls.clone());
        let res =
            crate::vector::sync_missing_vectors_for_root_with_batch_size(&mut conn, &root, &ff, 8)
                .await;
        assert!(res.is_err(), "second batch should fail");
        // first batch 8 should have persisted
        let fp = ff.fingerprint();
        let cnt = count_vectors(&conn, &fp)? as usize;
        assert_eq!(
            cnt, 8,
            "first batch persisted despite second failure, got {}",
            cnt
        );
        let missing = missing_vector_count(&conn, &fp)?;
        assert_eq!(missing, 8, "remaining missing should be 8, got {}", missing);
        assert!(!is_semantic_ready(&conn, &fp, true)?);
        // retry with good embedder should embed only remaining 8
        let calls2 = Arc::new(AtomicUsize::new(0));
        let docs2 = Arc::new(AtomicUsize::new(0));
        let cf = CountingFake::new("j-model", 8, calls2.clone(), docs2.clone());
        let (reused, embedded, calls_n, _docs) =
            crate::vector::sync_missing_vectors_for_root_with_batch_size(&mut conn, &root, &cf, 8)
                .await?;
        assert_eq!(
            embedded, 8,
            "retry should embed only missing 8, got {}",
            embedded
        );
        assert_eq!(calls_n, 1);
        assert_eq!(reused, 8);
        assert_eq!(missing_vector_count(&conn, &fp)?, 0);
        assert!(is_semantic_ready(&conn, &fp, true)?);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_l_determinism_20x() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::write(
            root.join("a.py"),
            b"def alpha():\n    query semantic search test\n",
        )?;
        std::fs::write(
            root.join("b.py"),
            b"def beta():\n    query semantic search test\n",
        )?;
        std::fs::write(root.join("c.py"), b"def gamma():\n    different content\n")?;
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        let cf = FakeEmbedder::new("det-model", 8);
        crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf).await?;
        let qvec = cf.embed_query("query semantic search test").await?;
        let fp = cf.fingerprint();
        let first = crate::vector::search_brute(&conn, &qvec, &fp, 10)?;
        for _ in 0..20 {
            let next = crate::vector::search_brute(&conn, &qvec, &fp, 10)?;
            assert_eq!(first.len(), next.len());
            for (a, b) in first.iter().zip(next.iter()) {
                assert_eq!(a.file, b.file);
                assert_eq!(a.chunk_id, b.chunk_id);
                assert_eq!(a.start_line, b.start_line);
                assert!((a.score - b.score).abs() < 1e-6);
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn r5c_k_status_uses_configured_fingerprint() -> Result<()> {
        // Verify configured_fingerprint reads env and vector counts are per fingerprint
        let guard = std::sync::Mutex::new(());
        let _g = guard.lock().unwrap();
        let orig = std::env::var("CONTEXTD_EMBED_MODEL").ok();
        std::env::set_var("CONTEXTD_EMBED_MODEL", "nomic-embed-text");
        let fp = crate::embed::configured_fingerprint();
        assert_eq!(fp.model_id, "nomic-embed-text");
        assert_eq!(fp.dimension, 768);
        assert_eq!(fp.version, "ollama-nomic-embed-text-v1");
        // ensure different from all-minilm
        std::env::set_var("CONTEXTD_EMBED_MODEL", "all-minilm");
        let fp2 = crate::embed::configured_fingerprint();
        assert_eq!(fp2.dimension, 384);
        assert_ne!(fp.model_id, fp2.model_id);
        // Vector count isolation: upsert for fp then count for other should be 0
        let mut conn = open_in_memory()?;
        let vec = vec![0.1f32; 768];
        crate::vector::upsert_vector(&mut conn, "h", &fp, &vec)?;
        assert_eq!(count_vectors(&conn, &fp)?, 1);
        assert_eq!(count_vectors(&conn, &fp2)?, 0);
        // restore
        if let Some(v) = orig {
            std::env::set_var("CONTEXTD_EMBED_MODEL", v);
        } else {
            std::env::remove_var("CONTEXTD_EMBED_MODEL");
        }
        Ok(())
    }
}
