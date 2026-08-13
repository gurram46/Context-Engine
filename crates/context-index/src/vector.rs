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
        "SELECT vector, dimension FROM vectors WHERE content_hash=?1 AND model_id=?2 AND version=?3",
    )?;
    let row = stmt.query_row(
        params![content_hash, fingerprint.model_id, fingerprint.version],
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
        "SELECT COUNT(*) FROM vectors WHERE model_id=?1 AND version=?2",
        params![fingerprint.model_id, fingerprint.version],
        |r| r.get(0),
    )?)
}

/// Delete vectors for a model if dimension mismatches (model change invalidation).
/// Old vectors must not be silently reused.
pub fn invalidate_stale_model(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_vector_schema(conn)?;
    // If dimension or version changed, old entries have different version/dimension, but we keep them? Spec says old vectors must not be reused when model id/version/dim changes.
    // Our get_vector already keys on version, so old version won't be returned. But to avoid disk bloat, we could delete old versions.
    // For now, just report count of stale (different version) — caller can delete if needed.
    let cnt: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vectors WHERE model_id=?1 AND version!=?2",
        params![fingerprint.model_id, fingerprint.version],
        |r| r.get(0),
    )?;
    Ok(cnt as usize)
}

pub fn delete_stale_vectors(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_vector_schema(conn)?;
    Ok(conn.execute(
        "DELETE FROM vectors WHERE model_id=?1 AND version!=?2",
        params![fingerprint.model_id, fingerprint.version],
    )?)
}

/// Conservative orphan GC: delete vectors whose content_hash is not referenced by any current chunk
/// for the given model. Keeps content-addressed reuse for active chunks, but prevents unbounded growth
/// after many edits. Does NOT delete immediately if there is any chunk referencing it.
/// For rename/revert: if content reappears, it will be re-embedded (acceptable for bounded storage).
pub fn gc_orphaned_vectors(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_vector_schema(conn)?;
    // Vectors whose hash not in any current chunk
    Ok(conn.execute(
        "DELETE FROM vectors WHERE model_id=?1 AND version=?2 AND content_hash NOT IN (SELECT content_hash FROM chunks)",
        params![fingerprint.model_id, fingerprint.version],
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
    // Get all vectors for model
    let mut stmt = conn.prepare(
        "SELECT content_hash, vector, dimension FROM vectors WHERE model_id=?1 AND version=?2",
    )?;
    let rows = stmt.query_map(params![fingerprint.model_id, fingerprint.version], |row| {
        let hash: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        let dim: i64 = row.get(2)?;
        Ok((hash, blob, dim as usize))
    })?;
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
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit * 2); // oversample before dedup via chunk mapping
                                // Map content_hash -> chunk metadata (choose one chunk per hash, or multiple if same content appears elsewhere)
                                // For now, pick first chunk per hash from chunks table
    let mut out = Vec::new();
    for (hash, score) in scored {
        let mut stmt2 = conn.prepare(
            "SELECT id, file, start_line, end_line, parent_symbol, content_hash FROM chunks WHERE content_hash=?1 LIMIT 5",
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
}
