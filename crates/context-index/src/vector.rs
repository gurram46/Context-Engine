//! R5.1-C2 — Native vector retrieval V2.
//! Vectors keyed by representation_hash (BLAKE3 of canonical semantic representation)
//! + representation_version + model fingerprint for correct dedup.
//!
//! Brute-force cosine baseline is reference truth.

use crate::embed::{Embedder, ModelFingerprint, QUERY_CACHE};
use crate::structural::types::Chunk;
use anyhow::Result;
use rusqlite::{params, Connection};

// --- Constants ---

pub const SEMANTIC_REPRESENTATION_VERSION: &str = "v2";
pub const SEMANTIC_CANDIDATE_K: usize = 25;

// --- Similarity ---

/// Cosine similarity (or dot if normalized).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    dot
}

#[allow(dead_code)]
fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    for x in v.iter_mut() {
        *x /= norm;
    }
}

// --- Canonical representation ---

/// Single canonical deterministic renderer for semantic representation V2.
/// Format: "<language>\n<normalized_path>\n<qualified_symbol_or_empty>\n<exact_slice>"
pub fn render_semantic_representation(
    language: &str,
    file: &str,
    qualified_symbol: &str,
    source_slice: &str,
) -> String {
    let normalized = file.replace('\\', "/");
    format!(
        "{}\n{}\n{}\n{}",
        language, normalized, qualified_symbol, source_slice
    )
}

pub fn representation_hash(rendered: &str) -> String {
    blake3::hash(rendered.as_bytes()).to_hex().to_string()
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

fn ensure_semantic_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS vectors (
            representation_hash TEXT NOT NULL,
            representation_version TEXT NOT NULL,
            model_id TEXT NOT NULL,
            version TEXT NOT NULL,
            dimension INTEGER NOT NULL,
            vector BLOB NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(representation_hash, representation_version, model_id, version, dimension)
        );
        CREATE INDEX IF NOT EXISTS idx_vectors_model ON vectors(model_id, version, dimension);
        CREATE INDEX IF NOT EXISTS idx_vectors_hash ON vectors(representation_hash, representation_version);
        CREATE TABLE IF NOT EXISTS semantic_chunk_refs (
            chunk_id TEXT NOT NULL,
            representation_hash TEXT NOT NULL,
            representation_version TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            PRIMARY KEY(chunk_id, representation_version),
            FOREIGN KEY(chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_semantic_refs_rep ON semantic_chunk_refs(representation_hash, representation_version);
        CREATE INDEX IF NOT EXISTS idx_semantic_refs_chunk ON semantic_chunk_refs(chunk_id);
        "#,
    )?;
    Ok(())
}

#[allow(dead_code)]
fn ensure_vector_schema(conn: &Connection) -> Result<()> {
    ensure_semantic_schema(conn)
}

pub fn upsert_vector(
    conn: &mut Connection,
    representation_hash: &str,
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
    ensure_semantic_schema(conn)?;
    let blob = vec_to_blob(vector);
    conn.execute(
        "INSERT OR REPLACE INTO vectors (representation_hash, representation_version, model_id, version, dimension, vector) VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            representation_hash,
            SEMANTIC_REPRESENTATION_VERSION,
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
    representation_hash: &str,
    fingerprint: &ModelFingerprint,
) -> Result<Option<Vec<f32>>> {
    ensure_semantic_schema(conn)?;
    let mut stmt = conn.prepare(
        "SELECT vector, dimension FROM vectors WHERE representation_hash=?1 AND representation_version=?2 AND model_id=?3 AND version=?4 AND dimension=?5",
    )?;
    let row = stmt.query_row(
        params![
            representation_hash,
            SEMANTIC_REPRESENTATION_VERSION,
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
    ensure_semantic_schema(conn)?;
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM vectors WHERE representation_version=?1 AND model_id=?2 AND version=?3 AND dimension=?4",
        params![
            SEMANTIC_REPRESENTATION_VERSION,
            fingerprint.model_id,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
        |r| r.get(0),
    )?)
}

/// Delete vectors for a model if dimension mismatches (model change invalidation).
pub fn invalidate_stale_model(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_semantic_schema(conn)?;
    let cnt: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vectors WHERE model_id=?1 AND representation_version=?2 AND (version!=?3 OR dimension!=?4)",
        params![
            fingerprint.model_id,
            SEMANTIC_REPRESENTATION_VERSION,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
        |r| r.get(0),
    )?;
    Ok(cnt as usize)
}

pub fn delete_stale_vectors(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_semantic_schema(conn)?;
    Ok(conn.execute(
        "DELETE FROM vectors WHERE model_id=?1 AND representation_version=?2 AND (version!=?3 OR dimension!=?4)",
        params![
            fingerprint.model_id,
            SEMANTIC_REPRESENTATION_VERSION,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
    )?)
}

/// Eligible chunks: total chunks
pub fn eligible_chunk_count(conn: &Connection) -> Result<usize> {
    ensure_semantic_schema(conn)?;
    let cnt: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    Ok(cnt as usize)
}

pub fn semantic_ref_count(conn: &Connection) -> Result<usize> {
    ensure_semantic_schema(conn)?;
    let cnt: i64 = conn.query_row(
        "SELECT COUNT(*) FROM semantic_chunk_refs WHERE representation_version=?1",
        params![SEMANTIC_REPRESENTATION_VERSION],
        |r| r.get(0),
    )?;
    Ok(cnt as usize)
}

pub fn vector_count_for(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    Ok(count_vectors(conn, fingerprint)? as usize)
}

/// Missing distinct representation_hash that have no compatible vector
pub fn missing_vector_count(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_semantic_schema(conn)?;
    let cnt: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT r.representation_hash) FROM semantic_chunk_refs r LEFT JOIN vectors v ON v.representation_hash=r.representation_hash AND v.representation_version=r.representation_version AND v.model_id=?1 AND v.version=?2 AND v.dimension=?3 WHERE r.representation_version=?4 AND v.representation_hash IS NULL",
        params![fingerprint.model_id, fingerprint.version, fingerprint.dimension as i64, SEMANTIC_REPRESENTATION_VERSION],
        |r| r.get(0),
    )?;
    Ok(cnt as usize)
}

#[allow(dead_code)]
fn missing_chunks_without_ref(conn: &Connection) -> Result<usize> {
    let cnt: i64 = conn.query_row(
        "SELECT COUNT(*) FROM chunks c LEFT JOIN semantic_chunk_refs r ON r.chunk_id=c.id AND r.representation_version=?1 WHERE r.chunk_id IS NULL",
        params![SEMANTIC_REPRESENTATION_VERSION],
        |r| r.get(0),
    )?;
    Ok(cnt as usize)
}

pub fn stale_vector_count(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    invalidate_stale_model(conn, fingerprint)
}

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
    let refs = semantic_ref_count(conn)?;
    if refs != eligible {
        return Ok(false);
    }
    let missing = missing_vector_count(conn, fingerprint)?;
    Ok(missing == 0)
}

// --- helpers for materializing semantic refs ---

#[allow(dead_code)]
fn resolve_qualified(conn: &Connection, parent_symbol: Option<&str>) -> String {
    if let Some(id) = parent_symbol {
        let res: Result<(String, String), rusqlite::Error> = conn.query_row(
            "SELECT qualified_name, name FROM symbols WHERE id=?1",
            params![id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        );
        if let Ok((qname, name)) = res {
            if !qname.is_empty() {
                return qname;
            }
            if !name.is_empty() {
                return name;
            }
        }
        return String::new();
    }
    String::new()
}

#[allow(clippy::type_complexity)]
/// Materialize semantic refs for given files (None = all chunks). Returns map representation_hash -> representation_text.
fn materialize_semantic_refs(
    conn: &mut Connection,
    root: &std::path::Path,
    files_filter: Option<&[String]>,
) -> Result<std::collections::HashMap<String, String>> {
    ensure_semantic_schema(conn)?;
    // Fetch chunks with resolved symbol via LEFT JOIN
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        String,
        String,
        usize,
        usize,
        String,
        Option<String>,
        Option<String>,
    )> = {
        let sql = if files_filter.is_some() {
            "SELECT c.id, c.file, c.language, c.start_byte, c.end_byte, c.content_hash, s.qualified_name, s.name FROM chunks c LEFT JOIN symbols s ON c.parent_symbol = s.id WHERE c.file IN (SELECT value FROM json_each(?1)) ORDER BY c.file, c.start_byte"
        } else {
            "SELECT c.id, c.file, c.language, c.start_byte, c.end_byte, c.content_hash, s.qualified_name, s.name FROM chunks c LEFT JOIN symbols s ON c.parent_symbol = s.id ORDER BY c.file, c.start_byte"
        };
        // Use json_each for variable list? Simpler: build placeholders if filter present.
        if let Some(filter) = files_filter {
            if filter.is_empty() {
                Vec::new()
            } else {
                let placeholders = filter.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let sql2 = format!("SELECT c.id, c.file, c.language, c.start_byte, c.end_byte, c.content_hash, s.qualified_name, s.name FROM chunks c LEFT JOIN symbols s ON c.parent_symbol = s.id WHERE c.file IN ({}) ORDER BY c.file, c.start_byte", placeholders);
                let mut stmt = conn.prepare(&sql2)?;
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    filter.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                let mapped = stmt.query_map(params_refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)? as usize,
                        row.get::<_, i64>(4)? as usize,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                    ))
                })?;
                let mut out = Vec::new();
                for r in mapped {
                    out.push(r?);
                }
                out
            }
        } else {
            let mut stmt = conn.prepare(sql)?;
            let mapped = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? as usize,
                    row.get::<_, i64>(4)? as usize,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })?;
            let mut out = Vec::new();
            for r in mapped {
                out.push(r?);
            }
            out
        }
    };

    if rows.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Grouped per-file processing for bounded memory and file-read-once (no per-chunk clone)
    use std::collections::HashMap as StdHashMap;
    let mut grouped: StdHashMap<
        String,
        Vec<(
            String,
            String,
            usize,
            usize,
            String,
            Option<String>,
            Option<String>,
        )>,
    > = StdHashMap::new();
    for (chunk_id, file, language, start_byte, end_byte, content_hash, qname, name) in rows {
        grouped.entry(file.clone()).or_default().push((
            chunk_id,
            language,
            start_byte,
            end_byte,
            content_hash,
            qname,
            name,
        ));
    }
    let mut map: StdHashMap<String, String> = StdHashMap::new();
    // Instrumentation counters
    let mut files_processed = 0usize;
    let mut chunks_processed = 0usize;
    for (file, file_rows) in grouped {
        files_processed += 1;
        let abs = root.join(&file);
        let content = std::fs::read_to_string(&abs)?;
        let bytes = content.as_bytes();
        // Per-file transaction for safe partial materialization
        let tx = conn.transaction()?;
        for (chunk_id, language, start_byte, end_byte, content_hash, qname, name) in file_rows {
            chunks_processed += 1;
            if start_byte > end_byte || end_byte > bytes.len() {
                anyhow::bail!(
                    "invalid byte range for {}: start {} end {} len {}",
                    file,
                    start_byte,
                    end_byte,
                    bytes.len()
                );
            }
            let slice = std::str::from_utf8(&bytes[start_byte..end_byte])
                .map_err(|e| anyhow::anyhow!("invalid utf8 slice for {}: {}", file, e))?;
            let qualified = if let Some(q) = qname {
                if !q.is_empty() {
                    q
                } else {
                    name.unwrap_or_default()
                }
            } else {
                name.unwrap_or_default()
            };
            let rendered = render_semantic_representation(&language, &file, &qualified, slice);
            let rep_hash = representation_hash(&rendered);
            map.insert(rep_hash.clone(), rendered);
            tx.execute(
                "INSERT OR REPLACE INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES (?1,?2,?3,?4)",
                params![chunk_id, rep_hash, SEMANTIC_REPRESENTATION_VERSION, content_hash],
            )?;
        }
        tx.commit()?;
    }
    eprintln!(
        "materialize: files={} chunks={} refs={} unique_reps={}",
        files_processed,
        chunks_processed,
        map.len(),
        map.len()
    );
    Ok(map)
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
#[allow(clippy::type_complexity)]
pub async fn sync_missing_vectors_for_root_with_batch_size(
    conn: &mut Connection,
    root: &std::path::Path,
    embedder: &dyn Embedder,
    batch_size: usize,
) -> Result<(usize, usize, usize, usize)> {
    ensure_semantic_schema(conn)?;
    let fp = embedder.fingerprint();
    let _ = delete_stale_vectors(conn, &fp)?;
    let t0 = std::time::Instant::now();
    let t_struct = std::time::Instant::now();
    // Per-file materialization with bounded memory and file-read-once
    // Get distinct files
    let files: Vec<String> = {
        let mut stmt = conn.prepare("SELECT DISTINCT file FROM chunks ORDER BY file")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        v
    };
    let eligible = eligible_chunk_count(conn)?;
    if eligible == 0 {
        return Ok((0, 0, 0, 0));
    }
    let t_row_load = t_struct.elapsed().as_millis();
    let mut total_files = 0usize;
    let mut total_chunks = 0usize;
    let mut total_refs = 0usize;
    let mut pending: Vec<(String, String)> = Vec::new();
    let mut total_embedded = 0usize;
    let mut total_calls = 0usize;
    let mut t_source_read_ms: u128 = 0;
    let mut t_render_ms: u128 = 0;
    let mut t_ref_write_ms: u128 = 0;
    // For reuse calculation, track unique before
    // We will also need to know existing vectors before for reuse telemetry
    // Instead, we will compute unique and missing after materialization per file
    for file in files {
        total_files += 1;
        // Load chunks for this file with resolved symbols
        let rows: Vec<(
            String,
            String,
            usize,
            usize,
            String,
            Option<String>,
            Option<String>,
        )> = {
            let mut stmt = conn.prepare("SELECT c.id, c.language, c.start_byte, c.end_byte, c.content_hash, s.qualified_name, s.name FROM chunks c LEFT JOIN symbols s ON c.parent_symbol = s.id WHERE c.file=?1 ORDER BY c.start_byte")?;
            let mapped = stmt.query_map(params![file], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? as usize,
                    r.get::<_, i64>(3)? as usize,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })?;
            let mut v = Vec::new();
            for r in mapped {
                v.push(r?);
            }
            v
        };
        if rows.is_empty() {
            continue;
        }
        total_chunks += rows.len();
        let abs = root.join(&file);
        let t_read = std::time::Instant::now();
        let content = std::fs::read_to_string(&abs)?;
        t_source_read_ms += t_read.elapsed().as_millis();
        let bytes = content.as_bytes();
        // Per-file transaction
        let t_ref = std::time::Instant::now();
        let tx = conn.transaction()?;
        let mut file_hashes: Vec<(String, String)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (chunk_id, language, start_byte, end_byte, content_hash, qname, name) in rows {
            if start_byte > end_byte || end_byte > bytes.len() {
                anyhow::bail!(
                    "invalid byte range for {}: start {} end {} len {}",
                    file,
                    start_byte,
                    end_byte,
                    bytes.len()
                );
            }
            let t_r = std::time::Instant::now();
            let slice = std::str::from_utf8(&bytes[start_byte..end_byte])
                .map_err(|e| anyhow::anyhow!("invalid utf8 slice for {}: {}", file, e))?;
            let qualified = if let Some(q) = qname {
                if !q.is_empty() {
                    q
                } else {
                    name.unwrap_or_default()
                }
            } else {
                name.unwrap_or_default()
            };
            let rendered = render_semantic_representation(&language, &file, &qualified, slice);
            t_render_ms += t_r.elapsed().as_millis();
            let rep_hash = representation_hash(&rendered);
            if seen.insert(rep_hash.clone()) {
                file_hashes.push((rep_hash.clone(), rendered));
            }
            tx.execute(
                "INSERT OR REPLACE INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES (?1,?2,?3,?4)",
                params![chunk_id, rep_hash, SEMANTIC_REPRESENTATION_VERSION, content_hash],
            )?;
            total_refs += 1;
        }
        tx.commit()?;
        t_ref_write_ms += t_ref.elapsed().as_millis();
        // For this file's distinct hashes, check which are missing
        for (h, text) in file_hashes {
            if get_vector(conn, &h, &fp)?.is_none() {
                pending.push((h, text));
            }
            // When pending reaches batch_size, embed
            if pending.len() >= batch_size {
                let batch: Vec<(String, String)> = pending.drain(..batch_size).collect();
                let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
                let hashes: Vec<String> = batch.iter().map(|(h, _)| h.clone()).collect();
                let t_emb = std::time::Instant::now();
                let vectors = embedder.embed_documents(&texts).await?;
                let t_vec_write = std::time::Instant::now();
                for (hash, vec) in hashes.into_iter().zip(vectors) {
                    upsert_vector(conn, &hash, &fp, &vec)?;
                    total_embedded += 1;
                }
                total_calls += 1;
                // embedding_ms and vector_write_ms could be tracked, but we aggregate
                let _ = t_emb.elapsed().as_millis();
                let _ = t_vec_write.elapsed().as_millis();
            }
        }
    }
    // Embed remaining pending
    while !pending.is_empty() {
        let batch_size_actual = std::cmp::min(batch_size, pending.len());
        let batch: Vec<(String, String)> = pending.drain(..batch_size_actual).collect();
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let hashes: Vec<String> = batch.iter().map(|(h, _)| h.clone()).collect();
        let vectors = embedder.embed_documents(&texts).await?;
        for (hash, vec) in hashes.into_iter().zip(vectors) {
            upsert_vector(conn, &hash, &fp, &vec)?;
            total_embedded += 1;
        }
        total_calls += 1;
    }
    let _ = gc_orphaned_vectors(conn, &fp);
    // Reuse telemetry: unique = total distinct reps (need to query)
    let unique: i64 = conn.query_row("SELECT COUNT(DISTINCT representation_hash) FROM semantic_chunk_refs WHERE representation_version=?1", params![SEMANTIC_REPRESENTATION_VERSION], |r| r.get(0))?;
    let unique = unique as usize;
    let reused = unique.saturating_sub(total_embedded);
    let t_total = t0.elapsed().as_millis();
    eprintln!(
        "sync_missing_vectors: eligible={} files={} chunks={} refs={} unique={} reused={} created={} calls={} row_load_ms={} source_read_ms={} render_ms={} ref_write_ms={} total_ms={}",
        eligible, total_files, total_chunks, total_refs, unique, reused, total_embedded, total_calls, t_row_load, t_source_read_ms, t_render_ms, t_ref_write_ms, t_total
    );
    Ok((reused, total_embedded, total_calls, total_embedded))
}

/// Load chunks for a file (for incremental sync).
pub fn load_chunks_for_file(conn: &Connection, file: &str) -> Result<Vec<Chunk>> {
    ensure_semantic_schema(conn)?;
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
    let _ = delete_stale_vectors(conn, &fp)?;
    // Materialize only changed files (cascade will have removed old refs for those files via DELETE FROM chunks trigger)
    // But we need to ensure we recreate refs for new chunks
    let rep_map = materialize_semantic_refs(conn, root, Some(changed_files))?;
    let distinct_changed = rep_map.len();

    // Determine which of the materialized reps are missing vectors
    let mut missing: Vec<(String, String)> = Vec::new();
    for (hash, text) in rep_map {
        if get_vector(conn, &hash, &fp)?.is_none() {
            missing.push((hash, text));
        }
    }
    missing.sort_by(|a, b| a.0.cmp(&b.0));

    let mut total_embedded = 0usize;
    let mut total_calls = 0usize;
    let total_reused = distinct_changed.saturating_sub(missing.len());

    if missing.is_empty() {
        // No embedding needed, but GC orphan still
        let _ = gc_orphaned_vectors(conn, &fp);
        return Ok((total_reused, 0, 0));
    }

    // Batch embed missing (batch = whole missing if small, else chunked by 256? use 256)
    for chunk in missing.chunks(256) {
        let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
        let hashes: Vec<String> = chunk.iter().map(|(h, _)| h.clone()).collect();
        total_calls += 1;
        let vectors = embedder.embed_documents(&texts).await?;
        for (hash, vec) in hashes.into_iter().zip(vectors) {
            upsert_vector(conn, &hash, &fp, &vec)?;
            total_embedded += 1;
        }
    }
    let _ = gc_orphaned_vectors(conn, &fp);
    Ok((total_reused, total_embedded, total_calls))
}

/// Conservative orphan GC: delete vectors whose representation_hash is not referenced by any current semantic ref
/// for the given model. Keeps representation reuse for active refs.
pub fn gc_orphaned_vectors(conn: &Connection, fingerprint: &ModelFingerprint) -> Result<usize> {
    ensure_semantic_schema(conn)?;
    Ok(conn.execute(
        "DELETE FROM vectors WHERE representation_version=?1 AND model_id=?2 AND version=?3 AND dimension=?4 AND representation_hash NOT IN (SELECT representation_hash FROM semantic_chunk_refs WHERE representation_version=?1)",
        params![
            SEMANTIC_REPRESENTATION_VERSION,
            fingerprint.model_id,
            fingerprint.version,
            fingerprint.dimension as i64
        ],
    )?)
}

// --- Changed-chunk reuse ---

/// Ensure vectors for chunks of a file, reusing unchanged representation hashes.
/// Returns (reused_count, embedded_count)
pub async fn sync_vectors_for_file(
    conn: &mut Connection,
    _file: &str,
    chunks: &[Chunk],
    file_content: &str,
    embedder: &dyn Embedder,
) -> Result<(usize, usize)> {
    ensure_semantic_schema(conn)?;
    let fp = embedder.fingerprint();
    let bytes = file_content.as_bytes();
    let mut missing: Vec<(String, String)> = Vec::new();
    let mut reused = 0usize;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for chunk in chunks {
        // Structural ownership: chunk must exist in structural storage; otherwise error and no mutation
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM chunks WHERE id=?1)",
            params![chunk.id],
            |r| r.get(0),
        )?;
        if !exists {
            anyhow::bail!("chunk {} not found in structural storage", chunk.id);
        }
        // Resolve qualified via central LEFT JOIN path (never raw parent_symbol)
        let qualified: String = conn
            .query_row(
                "SELECT COALESCE(s.qualified_name, s.name, '') FROM chunks c LEFT JOIN symbols s ON c.parent_symbol = s.id WHERE c.id=?1",
                params![chunk.id],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "".to_string());
        if chunk.start_byte > chunk.end_byte || chunk.end_byte > bytes.len() {
            anyhow::bail!(
                "invalid byte range for {}: {}..{} len {}",
                chunk.file,
                chunk.start_byte,
                chunk.end_byte,
                bytes.len()
            );
        }
        let slice_bytes = &bytes[chunk.start_byte..chunk.end_byte];
        let slice = std::str::from_utf8(slice_bytes)
            .map_err(|e| anyhow::anyhow!("invalid utf8 slice for {}: {}", chunk.file, e))?;
        let rendered =
            render_semantic_representation(chunk.language.as_str(), &chunk.file, &qualified, slice);
        let rep_hash = representation_hash(&rendered);
        // ALWAYS upsert semantic_chunk_refs for every valid chunk (even if representation hash duplicates)
        conn.execute(
            "INSERT OR REPLACE INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES (?1,?2,?3,?4)",
            params![chunk.id, rep_hash, SEMANTIC_REPRESENTATION_VERSION, chunk.content_hash],
        )?;
        // Deduplicate embedding work by representation_hash
        if !seen.insert(rep_hash.clone()) {
            // Already queued or reused check for this hash in current batch; ref already created
            continue;
        }
        if get_vector(conn, &rep_hash, &fp)?.is_some() {
            reused += 1;
        } else {
            missing.push((rep_hash, rendered));
        }
    }
    if missing.is_empty() {
        return Ok((reused, 0));
    }
    missing.sort_by(|a, b| a.0.cmp(&b.0));
    missing.dedup_by(|a, b| a.0 == b.0);
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
pub fn search_brute(
    conn: &Connection,
    query_vec: &[f32],
    fingerprint: &ModelFingerprint,
    limit: usize,
) -> Result<Vec<VectorCandidate>> {
    ensure_semantic_schema(conn)?;
    // Get all vectors for model
    let mut stmt = conn.prepare(
        "SELECT representation_hash, vector, dimension FROM vectors WHERE representation_version=?1 AND model_id=?2 AND version=?3 AND dimension=?4",
    )?;
    let rows = stmt.query_map(
        params![
            SEMANTIC_REPRESENTATION_VERSION,
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
                tracing::warn!(error = %e, representation_hash = %hash, "skipping corrupted vector row");
                continue;
            }
        };
        if vec.len() != query_vec.len() {
            continue;
        }
        let s = cosine(query_vec, &vec);
        scored.push((hash, s));
    }
    // Deterministic ordering: score desc, hash asc tie-breaker
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(limit * 2); // oversample before dedup via chunk mapping
                                // Map representation_hash -> chunks via semantic_chunk_refs
    let mut out = Vec::new();
    for (hash, score) in scored {
        let mut stmt2 = conn.prepare(
            "SELECT c.id, c.file, c.start_line, c.end_line, c.parent_symbol, c.content_hash FROM semantic_chunk_refs r JOIN chunks c ON c.id = r.chunk_id WHERE r.representation_hash=?1 AND r.representation_version=?2 ORDER BY c.file ASC, c.start_line ASC, c.id ASC LIMIT 5",
        )?;
        let chunk_rows =
            stmt2.query_map(params![hash, SEMANTIC_REPRESENTATION_VERSION], |row| {
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
pub async fn search_vector(
    conn: &Connection,
    query: &str,
    fingerprint: &ModelFingerprint,
    embedder: &dyn Embedder,
    limit: usize,
) -> Result<Vec<VectorCandidate>> {
    let cached = QUERY_CACHE.get(fingerprint, query).await;
    let qvec = if let Some(v) = cached {
        v
    } else {
        let v = embedder.embed_query(query).await?;
        QUERY_CACHE.insert(fingerprint, query, v.clone()).await;
        v
    };
    search_brute(conn, &qvec, fingerprint, limit)
}

/// Update vectors for a changed file transactionally.
pub async fn update_vectors_for_parsed(
    conn: &mut Connection,
    file: &str,
    chunks: &[Chunk],
    file_content: &str,
    embedder: &dyn Embedder,
) -> Result<(usize, usize)> {
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
        // Insert structural fixtures before semantic sync (semantic must not create structural rows)
        for ch in &[chunk_a.clone(), chunk_b.clone()] {
            conn.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                rusqlite::params![ch.file, "testhash", ch.language.as_str()],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![ch.id, ch.file, ch.language.as_str(), ch.start_line as i64, ch.end_line as i64, ch.start_byte as i64, ch.end_byte as i64, ch.parent_symbol, ch.content_hash, ch.text_size_bytes as i64],
            )?;
        }
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
        // Change only chunk B representation (different slice should cause different rep hash)
        // Modify content to change slice for B
        let content2 = "helloXXXXworld";
        let chunk_b_new = Chunk {
            content_hash: "hashB2".to_string(),
            start_byte: 6,
            end_byte: 11,
            ..chunk_b.clone()
        };
        let (reused3, embedded3) = sync_vectors_for_file(
            &mut conn,
            "a.py",
            &[chunk_a.clone(), chunk_b_new.clone()],
            content2,
            &embedder,
        )
        .await?;
        assert_eq!(reused3, 1);
        assert_eq!(embedded3, 1);
        // Verify vector reuse across same representation (identical file, symbol, slice) should reuse
        // Insert chunks into DB for search_brute to find (it joins via semantic_chunk_refs)
        // Using IGNORE to avoid cascade delete of existing semantic refs via REPLACE
        for ch in &[chunk_a.clone(), chunk_b.clone()] {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                rusqlite::params![ch.file, "testhash", ch.language.as_str()],
            );
            let _ = conn.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![ch.id, ch.file, ch.language.as_str(), ch.start_line as i64, ch.end_line as i64, ch.start_byte as i64, ch.end_byte as i64, ch.parent_symbol, ch.content_hash, ch.text_size_bytes as i64],
            );
        }
        // Need to ensure semantic_chunk_refs already has entries from sync_vectors_for_file, search should find them
        let qvec = embedder.embed_query("hello").await?;
        let res = search_brute(&conn, &qvec, &fp, 5)?;
        assert!(!res.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn one_line_edit_not_reembed_all() -> Result<()> {
        let mut conn = open_in_memory()?;
        let embedder = FakeEmbedder::new("fake", 4);
        // Simulate file with 4 chunks A,B,C,D each distinct slice
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
        let content = "AAAAAAAAAABBBBBBBBBBCCCCCCCCCCDDDDDDDDDD".to_string() + &"E".repeat(10);
        for ch in &chunks {
            conn.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                rusqlite::params![ch.file, "testhash", ch.language.as_str()],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![ch.id, ch.file, ch.language.as_str(), ch.start_line as i64, ch.end_line as i64, ch.start_byte as i64, ch.end_byte as i64, ch.parent_symbol, ch.content_hash, ch.text_size_bytes as i64],
            )?;
        }
        let (r1, e1) =
            sync_vectors_for_file(&mut conn, "f.rs", &chunks, &content, &embedder).await?;
        assert_eq!(e1, 4);
        assert_eq!(r1, 0);
        // Edit only chunk C (index 2) slice change without shifting later chunks
        let mut chunks2 = chunks.clone();
        chunks2[2].content_hash = "hash2_new".to_string();
        let mut bytes2 = content.as_bytes().to_vec();
        bytes2[20..25].copy_from_slice(b"XXXXX");
        let content2 = String::from_utf8(bytes2).unwrap();
        let (r2, e2) =
            sync_vectors_for_file(&mut conn, "f.rs", &chunks2, &content2, &embedder).await?;
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
        conn.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
            rusqlite::params![chunk.file, "testhash", chunk.language.as_str()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![chunk.id, chunk.file, chunk.language.as_str(), chunk.start_line as i64, chunk.end_line as i64, chunk.start_byte as i64, chunk.end_byte as i64, chunk.parent_symbol, chunk.content_hash, chunk.text_size_bytes as i64],
        )?;
        sync_vectors_for_file(&mut conn, "a.py", &[chunk.clone()], "abc", &e1).await?;
        assert_eq!(count_vectors(&conn, &fp1)?, 1);
        let e2 = FakeEmbedder::new("modelB", 4);
        let fp2 = e2.fingerprint();
        // Old vectors for modelA should not be returned for modelB
        assert_eq!(count_vectors(&conn, &fp2)?, 0);
        // Stale detection
        let _stale = invalidate_stale_model(&conn, &fp2)?;
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
        // Simulate 100 distinct representations via distinct file paths
        for i in 0..100 {
            let file = format!("a{}.py", i);
            let content = format!("content{}", i);
            let chunk = Chunk {
                id: format!("c{}", i),
                file: file.clone(),
                language: Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: content.len(),
                parent_symbol: None,
                content_hash: format!("hash{}", i),
                text_size_bytes: content.len(),
            };
            // Ensure file entry exists for FK
            let _ = conn.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                rusqlite::params![file, format!("filehash{}", i), "python"],
            );
            // Insert chunk
            conn.execute(
                "INSERT OR REPLACE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![chunk.id, chunk.file, "python", 1, 2, 0, content.len() as i64, Option::<String>::None, chunk.content_hash, content.len() as i64],
            )?;
            sync_vectors_for_file(&mut conn, &file, &[chunk.clone()], &content, &embedder).await?;
            // Keep only last chunk in DB for GC test final? For intermediate, we keep replacing, but we simulate orphan growth via distinct chunk ids and GC
            // For this test, we want 100 distinct representation hashes each with vector, but only last chunk remains referenced
            // So after each iteration, delete previous chunks refs? Instead we directly manage vectors via sync, but semantic_chunk_refs will have only latest chunk id for a.py (since we replace chunks where file same id different? Actually ids are c0..c99 distinct, so they all remain if we don't delete)
            // To simulate orphan, we delete previous semantic refs manually after each?
        }
        // For this GC test, we have 100 vectors but many orphaned because chunks table has 100 rows with 100 distinct chunk ids (c0..c99) each with its own vector, none orphaned yet
        // To test GC, we need to delete chunks for first 99 and keep only 1
        conn.execute("DELETE FROM chunks WHERE id != 'c99'", [])?;
        conn.execute(
            "DELETE FROM semantic_chunk_refs WHERE chunk_id != 'c99'",
            [],
        )?;
        let before = count_vectors(&conn, &fp)?;
        assert_eq!(before, 100);
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
        let start = Instant::now();
        let handle = tokio::spawn(async move {
            let mut c = open_mem().unwrap();
            // Insert structural fixture for chunk (semantic must not create structural rows)
            c.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
                rusqlite::params!["a.py", "testhash", "python"],
            )
            .unwrap();
            c.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![fast_chunk.id, fast_chunk.file, "python", fast_chunk.start_line as i64, fast_chunk.end_line as i64, fast_chunk.start_byte as i64, fast_chunk.end_byte as i64, fast_chunk.parent_symbol, fast_chunk.content_hash, fast_chunk.text_size_bytes as i64],
            )
            .unwrap();
            let s = SlowTestEmbedder::new(2000);
            let _ = sync_vectors_for_file(&mut c, "a.py", &[fast_chunk], "hello", &s).await;
        });
        let elapsed_before = start.elapsed();
        assert!(
            elapsed_before.as_millis() < 500,
            "exact should be available immediately, got {}ms",
            elapsed_before.as_millis()
        );
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        let total = start.elapsed();
        assert!(
            total.as_millis() >= 1900,
            "slow embedder should have taken ~2s, got {}ms",
            total.as_millis()
        );
        Ok(())
    }

    #[tokio::test]
    async fn wrong_length_blob_returns_error() -> Result<()> {
        let conn = open_in_memory()?;
        ensure_semantic_schema(&conn)?;
        let fp = ModelFingerprint {
            model_id: "test".to_string(),
            version: "v1".to_string(),
            dimension: 4,
        };
        conn.execute(
            "INSERT OR REPLACE INTO vectors (representation_hash, representation_version, model_id, version, dimension, vector) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params!["hash1", SEMANTIC_REPRESENTATION_VERSION, fp.model_id, fp.version, fp.dimension as i64, vec![0u8, 1, 2]],
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
        let bad_vec = vec![1.0, 2.0, 3.0];
        let res = upsert_vector(&mut conn, "hash1", &fp, &bad_vec);
        assert!(res.is_err(), "upsert should reject dimension mismatch");
        assert!(
            res.unwrap_err().to_string().contains("dimension mismatch"),
            "error should mention dimension mismatch"
        );
        assert_eq!(count_vectors(&conn, &fp)?, 0);
        Ok(())
    }

    // ---- R5.1-C2 representation tests ----

    #[test]
    fn renderer_determinism() {
        let text =
            render_semantic_representation("python", "a/b.py", "Foo.bar", "def foo():\n    pass");
        let h1 = representation_hash(&text);
        let text2 =
            render_semantic_representation("python", "a/b.py", "Foo.bar", "def foo():\n    pass");
        let h2 = representation_hash(&text2);
        assert_eq!(text, text2);
        assert_eq!(h1, h2);
        for _ in 0..20 {
            let t = render_semantic_representation(
                "python",
                "a/b.py",
                "Foo.bar",
                "def foo():\n    pass",
            );
            assert_eq!(t, text);
            assert_eq!(representation_hash(&t), h1);
        }
    }

    #[test]
    fn opaque_symbol_removal() {
        let opaque = "65f1dfe6aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let qualified = "MyClass.my_method";
        let rendered = render_semantic_representation("python", "a.py", qualified, "pass");
        assert!(rendered.contains(qualified));
        assert!(!rendered.contains(opaque));
        // Ensure parent_symbol hash is not embedded directly
        let h = representation_hash(&rendered);
        assert!(!h.contains(opaque));
    }

    #[test]
    fn same_source_different_path() {
        let slice = "def foo():\n    pass";
        let r1 = render_semantic_representation("python", "a.py", "foo", slice);
        let r2 = render_semantic_representation("python", "b.py", "foo", slice);
        assert_ne!(representation_hash(&r1), representation_hash(&r2));
        assert_ne!(r1, r2);
    }

    #[test]
    fn same_source_different_symbol() {
        let slice = "def foo():\n    pass";
        let r1 = render_semantic_representation("python", "a.py", "foo", slice);
        let r2 = render_semantic_representation("python", "a.py", "bar", slice);
        assert_ne!(representation_hash(&r1), representation_hash(&r2));
    }

    #[test]
    fn identical_representation_same_hash() {
        let slice = "same";
        let r1 = render_semantic_representation("rust", "a.rs", "Foo", slice);
        let r2 = render_semantic_representation("rust", "a.rs", "Foo", slice);
        assert_eq!(representation_hash(&r1), representation_hash(&r2));
    }

    // ---- R5.1-C extended tests (updated for V2) ----
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
        assert!(out2.changed_files.contains(&"a.py".to_string()));
        assert!(!out2.changed_files.contains(&"b.py".to_string()));
        let calls = Arc::new(AtomicUsize::new(0));
        let docs = Arc::new(AtomicUsize::new(0));
        let cf2 = CountingFake::new("c-model", 8, calls.clone(), docs.clone());
        let (reused, embedded, calls_n, _docs) =
            crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf2).await?;
        // Only changed representation(s) should embed
        assert!(embedded >= 1, "only changed should embed, got {}", embedded);
        assert!(reused >= 1);
        assert!(calls_n >= 1);
        assert_eq!(docs.load(Ordering::SeqCst), embedded);
        assert_eq!(missing_vector_count(&conn, &cf2.fingerprint())?, 0);
        let eligible_after = eligible_chunk_count(&conn)?;
        assert_eq!(reused + embedded, eligible_after);
        assert_eq!(eligible_before, eligible_after);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_d_delete_orphan_and_e_shared_preserved() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        // For shared representation test, need identical chunk reps? With V2, same slice in different files gives different rep hash due to path, so not shared.
        // Instead test that deleting file GCs its orphan vector but shared within same file not applicable.
        // Use two chunks with identical representation via same file and symbol and slice but different chunk_id? That would need same file+symbol+slice produce same hash.
        // Simpler: test orphan deletion via file delete.
        let same_slice = b"def foo():\n    pass\n";
        std::fs::write(root.join("a.py"), same_slice)?;
        std::fs::write(root.join("b.py"), same_slice)?;
        // Note a.py and b.py have same slice but different path => different rep hash, so 2 vectors expected.
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        let cf = FakeEmbedder::new("d-model", 4);
        crate::vector::sync_missing_vectors_for_root(&mut conn, &root, &cf).await?;
        let cnt_before = count_vectors(&conn, &cf.fingerprint())?;
        // With V2, different path => 2 distinct vectors
        assert_eq!(cnt_before, 2);
        // delete a.py
        std::fs::remove_file(root.join("a.py"))?;
        let pr2 = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx2 = crate::discovery::ProjectIndex::discover(&pr2)?;
        let out = si.build_with_delta(&idx2)?;
        assert!(out.deleted_files.contains(&"a.py".to_string()));
        let conn2 = crate::structural::store::open_db(&root)?;
        // Need to GC after structural delete? sync will handle but we also materialize? For this test, manually GC
        let _ = gc_orphaned_vectors(&conn2, &cf.fingerprint())?;
        // Also need to clean semantic refs for deleted file (cascade) – already done via chunks delete
        // But we didn't re-materialize after delete, so semantic refs for a.py still exist? Actually chunks for a.py deleted, cascade removed refs, so only b.py remains.
        // But our conn2 still has semantic refs for both? Let's reload after delete, materialize would be needed to clean? Actually semantic refs for a.py should have been cascade deleted via chunks delete (FK). So GC should remove orphan vector for a.py.
        // Check remaining refs
        let remaining_refs = semantic_ref_count(&conn2)?;
        assert_eq!(remaining_refs, 1);
        assert_eq!(
            count_vectors(&conn2, &cf.fingerprint())?,
            1,
            "orphan for deleted file should be GC'd"
        );
        // now delete b.py also
        std::fs::remove_file(root.join("b.py"))?;
        let pr3 = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx3 = crate::discovery::ProjectIndex::discover(&pr3)?;
        let out3 = si.build_with_delta(&idx3)?;
        assert!(out3.deleted_files.contains(&"b.py".to_string()));
        let conn3 = crate::structural::store::open_db(&root)?;
        let deleted = gc_orphaned_vectors(&conn3, &cf.fingerprint())?;
        assert_eq!(deleted, 1, "last orphan should be deleted");
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
        conn.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
            rusqlite::params![chunk.file, "testhash", chunk.language.as_str()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![chunk.id, chunk.file, chunk.language.as_str(), chunk.start_line as i64, chunk.end_line as i64, chunk.start_byte as i64, chunk.end_byte as i64, chunk.parent_symbol, chunk.content_hash, chunk.text_size_bytes as i64],
        )?;
        sync_vectors_for_file(&mut conn, "a.py", &[chunk.clone()], "abc", &e1).await?;
        assert_eq!(count_vectors(&conn, &fp1)?, 1);
        let e2 = FakeEmbedder::new("modelB", 4);
        let fp2 = e2.fingerprint();
        assert_eq!(count_vectors(&conn, &fp2)?, 0);
        // Retrieve same representation hash for checks via rendering
        let rendered = render_semantic_representation("python", "a.py", "", "abc");
        let rep_hash = representation_hash(&rendered);
        assert!(
            get_vector(&conn, &rep_hash, &fp2)?.is_none(),
            "different model should not reuse"
        );
        let fp_v2 = ModelFingerprint {
            model_id: "modelA".to_string(),
            version: "v2".to_string(),
            dimension: 4,
        };
        assert!(get_vector(&conn, &rep_hash, &fp_v2)?.is_none());
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
        // missing should count as missing for fp8 if chunk exists with ref
        conn.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1, ?2, ?3)",
            rusqlite::params!["a.py", "h", "python"],
        )?;
        conn.execute(
            "INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params!["c1", "a.py", "python", 1, 2, 0, 3, Option::<String>::None, "hash1", 3],
        )?;
        conn.execute(
            "INSERT INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES (?1,?2,?3,?4)",
            rusqlite::params!["c1", "hash1", SEMANTIC_REPRESENTATION_VERSION, "hash1"],
        )?;
        assert_eq!(missing_vector_count(&conn, &fp8)?, 1);
        assert_eq!(missing_vector_count(&conn, &fp4)?, 0);
        assert_eq!(stale_vector_count(&conn, &fp8)?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_j_partial_failure_and_retry() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
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
        let _conn = open_in_memory()?;
        let fp = ModelFingerprint {
            model_id: "all-minilm".into(),
            version: "ollama-all-minilm-v1".into(),
            dimension: 384,
        };
        let fp2 = ModelFingerprint {
            model_id: "nomic-embed-text".into(),
            version: "ollama-nomic-embed-text-v1".into(),
            dimension: 768,
        };
        let vec_data = vec![0.1f32; 384];
        let rep_hash = representation_hash(&render_semantic_representation(
            "python", "a.py", "foo", "pass",
        ));
        let conn2 = open_in_memory()?;
        // Need to insert a chunk and ref first for completeness
        conn2.execute(
            "INSERT INTO files (path, hash, language) VALUES ('a.py','h','python')",
            [],
        )?;
        conn2.execute("INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c1','a.py','python',1,2,0,4,NULL,'ch','4')", [])?;
        conn2.execute("INSERT INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES ('c1',?1,'v2','ch')", params![rep_hash])?;
        let mut conn_mut = conn2;
        upsert_vector(&mut conn_mut, &rep_hash, &fp, &vec_data)?;
        assert_eq!(count_vectors(&conn_mut, &fp)?, 1);
        assert_eq!(count_vectors(&conn_mut, &fp2)?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn r5c_m_shared_representation_reuse() -> Result<()> {
        // Two chunks with identical representation should share one vector
        let mut conn = open_in_memory()?;
        conn.execute(
            "INSERT INTO files (path, hash, language) VALUES ('a.py','h','python')",
            [],
        )?;
        conn.execute("INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c1','a.py','python',1,2,0,4,NULL,'h1','4')", [])?;
        conn.execute("INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c2','a.py','python',1,2,0,4,NULL,'h2','4')", [])?;
        let rep = render_semantic_representation("python", "a.py", "", "same slice");
        let h = representation_hash(&rep);
        conn.execute("INSERT INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES ('c1',?1,'v2','h1')", params![h])?;
        conn.execute("INSERT INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES ('c2',?1,'v2','h2')", params![h])?;
        let fp = ModelFingerprint {
            model_id: "test".into(),
            version: "v1".into(),
            dimension: 4,
        };
        upsert_vector(&mut conn, &h, &fp, &[1.0, 2.0, 3.0, 4.0])?;
        assert_eq!(count_vectors(&conn, &fp)?, 1);
        assert_eq!(semantic_ref_count(&conn)?, 2);
        assert_eq!(missing_vector_count(&conn, &fp)?, 0);
        // GC after deleting one ref should preserve vector
        conn.execute("DELETE FROM semantic_chunk_refs WHERE chunk_id='c1'", [])?;
        assert_eq!(semantic_ref_count(&conn)?, 1);
        let del = gc_orphaned_vectors(&conn, &fp)?;
        assert_eq!(del, 0);
        assert_eq!(count_vectors(&conn, &fp)?, 1);
        // Delete last ref => vector GC'd
        conn.execute("DELETE FROM semantic_chunk_refs WHERE chunk_id='c2'", [])?;
        let del2 = gc_orphaned_vectors(&conn, &fp)?;
        assert_eq!(del2, 1);
        assert_eq!(count_vectors(&conn, &fp)?, 0);
        Ok(())
    }

    #[test]
    fn path_normalization() {
        let r = render_semantic_representation("python", "a\\b\\c.py", "foo", "pass");
        assert!(r.contains("a/b/c.py"));
        assert!(!r.contains("\\"));
    }

    #[tokio::test]
    async fn materialization_read_failure_does_not_create_ref() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::write(root.join("a.py"), b"def foo():\n    pass\n")?;
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        // Remove source file to cause read failure
        std::fs::remove_file(root.join("a.py"))?;
        let res = crate::vector::sync_missing_vectors_for_root(
            &mut conn,
            &root,
            &FakeEmbedder::new("test", 8),
        )
        .await;
        assert!(
            res.is_err(),
            "materialization should fail when source missing"
        );
        // No semantic refs should have been committed
        assert_eq!(semantic_ref_count(&conn)?, 0);
        assert_eq!(
            count_vectors(&conn, &FakeEmbedder::new("test", 8).fingerprint())? as usize,
            0
        );
        // Ready must be false
        let fp = FakeEmbedder::new("test", 8).fingerprint();
        assert!(!is_semantic_ready(&conn, &fp, true)?);
        // Also test invalid byte range
        let conn2 = open_in_memory()?;
        conn2.execute(
            "INSERT INTO files (path, hash, language) VALUES ('a.py','h','python')",
            [],
        )?;
        conn2.execute("INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c1','a.py','python',1,2,10,5,NULL,'ch','5')", [])?; // start > end invalid
        std::fs::create_dir_all(tmp.path().join("invalid_test"))?;
        let invalid_root = tmp.path().join("invalid_test");
        std::fs::create_dir_all(invalid_root.join(".git"))?;
        std::fs::write(invalid_root.join("a.py"), b"abc")?;
        // Directly call materialize via sync_missing_vectors_for_root_with_batch_size which will attempt to materialize invalid range
        // We need to set up DB for that root
        let db_path = crate::structural::store::index_db_path(&invalid_root);
        std::fs::create_dir_all(db_path.parent().unwrap())?;
        // Copy the invalid chunk DB into that root's DB
        {
            let conn_invalid = crate::structural::store::open_db(&invalid_root)?;
            conn_invalid.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES ('a.py','h','python')",
                [],
            )?;
            conn_invalid.execute("INSERT OR REPLACE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c1','a.py','python',1,2,10,5,NULL,'ch','5')", [])?;
        }
        let mut conn3 = crate::structural::store::open_db(&invalid_root)?;
        let res2 = crate::vector::sync_missing_vectors_for_root(
            &mut conn3,
            &invalid_root,
            &FakeEmbedder::new("test", 8),
        )
        .await;
        assert!(res2.is_err(), "invalid byte range should error");
        assert_eq!(semantic_ref_count(&conn3)?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn semantic_sync_does_not_create_structural_rows() -> Result<()> {
        let mut conn = open_in_memory()?;
        let chunk = Chunk {
            id: "unattached".to_string(),
            file: "ghost.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 3,
            parent_symbol: None,
            content_hash: "h1".to_string(),
            text_size_bytes: 3,
        };
        let res = sync_vectors_for_file(
            &mut conn,
            "ghost.py",
            &[chunk],
            "abc",
            &FakeEmbedder::new("test", 8),
        )
        .await;
        assert!(res.is_err(), "should error for unattached chunk");
        let files: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let chunks: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
        let refs: i64 =
            conn.query_row("SELECT COUNT(*) FROM semantic_chunk_refs", [], |r| r.get(0))?;
        assert_eq!(files, 0);
        assert_eq!(chunks, 0);
        assert_eq!(refs, 0);
        Ok(())
    }

    #[test]
    fn unresolved_parent_symbol_is_empty() {
        let rendered = render_semantic_representation("python", "a.py", "", "def foo():\n    pass");
        // Simulate unresolved parent_symbol "arbitrary-symbol-id-that-does-not-resolve" should not be embedded
        let arbitrary = "arbitrary-symbol-id-that-does-not-resolve";
        assert!(!rendered.contains(arbitrary));
        // Ensure empty qualified path yields double newline
        let rendered2 = render_semantic_representation("python", "a.py", "", "pass");
        // Should be language, path, empty, slice with correct newlines
        assert_eq!(rendered2, "python\na.py\n\npass");
        // Also via DB resolution: create chunk with arbitrary parent_symbol and ensure resolved is empty
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO files (path, hash, language) VALUES ('a.py','h','python')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c1','a.py','python',1,2,0,4,'arbitrary-symbol-id-that-does-not-resolve','ch','4')", []).unwrap();
        let qualified: String = conn
            .query_row(
                "SELECT COALESCE(s.qualified_name, s.name, '') FROM chunks c LEFT JOIN symbols s ON c.parent_symbol = s.id WHERE c.id='c1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(qualified, "");
        let rendered3 = render_semantic_representation("python", "a.py", &qualified, "pass");
        assert!(!rendered3.contains(arbitrary));
    }

    #[tokio::test]
    async fn failure_atomic_refs() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::write(root.join("a.py"), b"def foo():\n    pass\n")?;
        std::fs::write(root.join("b.py"), b"def bar():\n    pass\n")?;
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        // Make b.py unreadable to cause failure on second chunk materialization (remove file)
        std::fs::remove_file(root.join("b.py"))?;
        let res = crate::vector::sync_missing_vectors_for_root(
            &mut conn,
            &root,
            &FakeEmbedder::new("test", 8),
        )
        .await;
        assert!(res.is_err());
        // With per-file transactions, valid files' refs may persist, but not all; ready must be false
        let refs = semantic_ref_count(&conn)?;
        let eligible = eligible_chunk_count(&conn)?;
        assert!(refs < eligible, "partial refs may persist but not all");
        assert!(!is_semantic_ready(
            &conn,
            &FakeEmbedder::new("test", 8).fingerprint(),
            true
        )?);
        // Restore b.py and ensure next successful materialization works and does not corrupt
        std::fs::write(root.join("b.py"), b"def bar():\n    pass\n")?;
        // Need to rebuild structural index to re-discover b.py (it was deleted then restored)
        let pr2 = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx2 = crate::discovery::ProjectIndex::discover(&pr2)?;
        si.build_with_delta(&idx2)?;
        let mut conn2 = crate::structural::store::open_db(&root)?;
        let res2 = crate::vector::sync_missing_vectors_for_root(
            &mut conn2,
            &root,
            &FakeEmbedder::new("test", 8),
        )
        .await;
        assert!(res2.is_ok());
        assert_eq!(semantic_ref_count(&conn2)?, eligible_chunk_count(&conn2)?);
        Ok(())
    }

    #[tokio::test]
    async fn missing_representation_text_never_embeds_fallback() -> Result<()> {
        // Verify that sync does not embed empty representation when rep_map missing hash
        // Force inconsistency by manually inserting a fake semantic ref that will be missing
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::write(root.join("a.py"), b"def foo():\n    pass\n")?;
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::for_path(root.clone());
        si.build_with_delta(&idx)?;
        let mut conn = crate::structural::store::open_db(&root)?;
        // First successful index
        crate::vector::sync_missing_vectors_for_root(
            &mut conn,
            &root,
            &FakeEmbedder::new("test", 8),
        )
        .await?;
        let before_vectors = count_vectors(&conn, &FakeEmbedder::new("test", 8).fingerprint())?;
        // Corrupt DB: insert a fake semantic ref for existing chunk with fake hash
        let chunk_id: String = conn.query_row("SELECT id FROM chunks LIMIT 1", [], |r| r.get(0))?;
        conn.execute(
            "INSERT OR REPLACE INTO semantic_chunk_refs (chunk_id, representation_hash, representation_version, content_hash) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chunk_id, "fake_hash_not_in_rep_map_1234567890abcdef", "v2", "h1"],
        )?;
        // Now sync again - materialize will fix the fake hash to correct one, so missing should be 0 and no empty embed
        // Ensure that after sync, no vector was created for fake_hash and no empty representation was embedded
        let mut conn2 = crate::structural::store::open_db(&root)?;
        let res = crate::vector::sync_missing_vectors_for_root(
            &mut conn2,
            &root,
            &FakeEmbedder::new("test", 8),
        )
        .await;
        assert!(
            res.is_ok(),
            "sync should succeed after fixing fake hash via materialize"
        );
        let fake_exists = get_vector(
            &conn2,
            "fake_hash_not_in_rep_map_1234567890abcdef",
            &FakeEmbedder::new("test", 8).fingerprint(),
        )?;
        assert!(
            fake_exists.is_none(),
            "no vector should be created for fake hash"
        );
        assert_eq!(
            count_vectors(&conn2, &FakeEmbedder::new("test", 8).fingerprint())?,
            before_vectors
        );
        Ok(())
    }

    #[tokio::test]
    async fn reuse_telemetry_duplicate_reps() -> Result<()> {
        // A: 2 chunks -> same representation, cold index
        let embedder = FakeEmbedder::new("test-dedup", 8);
        let content = "same slice content";
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::write(root.join("a.py"), content.as_bytes())?;
        let db_path = crate::structural::store::index_db_path(&root);
        std::fs::create_dir_all(db_path.parent().unwrap())?;
        let mut conn_file = crate::structural::store::open_db(&root)?;
        conn_file.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES ('a.py','fh','python')",
            [],
        )?;
        conn_file.execute("INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c1','a.py','python',1,2,0,18,NULL,'h1',18)", [])?;
        conn_file.execute("INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c2','a.py','python',1,2,0,18,NULL,'h2',18)", [])?;
        let (reused, created, _calls, _) =
            crate::vector::sync_missing_vectors_for_root(&mut conn_file, &root, &embedder).await?;
        // Unique reps =1, so reused 0, created 1
        assert_eq!(created, 1, "2 chunks same rep should create 1 vector");
        assert_eq!(reused, 0, "cold should have 0 reused");
        // B: second sync should have reused 1, created 0
        let (reused2, created2, _, _) =
            crate::vector::sync_missing_vectors_for_root(&mut conn_file, &root, &embedder).await?;
        assert_eq!(reused2, 1, "second sync should have 1 reused distinct");
        assert_eq!(created2, 0);
        // C: 2 new chunks same rep in same file, no vector yet => created 1
        conn_file.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES ('c.py','fh','python')",
            [],
        )?;
        for (id, h) in [("c3", "h3"), ("c4", "h4")] {
            conn_file.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,'c.py','python',1,2,0,5,NULL,?2,5)",
                rusqlite::params![id, h],
            )?;
        }
        let chunk_c1 = Chunk {
            id: "c3".to_string(),
            file: "c.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 5,
            parent_symbol: None,
            content_hash: "h3".to_string(),
            text_size_bytes: 5,
        };
        let chunk_c2 = Chunk {
            id: "c4".to_string(),
            file: "c.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 5,
            parent_symbol: None,
            content_hash: "h4".to_string(),
            text_size_bytes: 5,
        };
        let (reused_c, created_c) = crate::vector::sync_vectors_for_file(
            &mut conn_file,
            "c.py",
            &[chunk_c1, chunk_c2],
            "hello",
            &embedder,
        )
        .await?;
        assert_eq!(reused_c, 0, "new duplicate reps should have 0 reused");
        assert_eq!(created_c, 1, "2 chunks same rep should create 1 vector");
        Ok(())
    }

    #[tokio::test]
    async fn duplicate_representation_ref_invariant() -> Result<()> {
        // Two chunks may legitimately share same representation hash (same slice, same file, same symbol)
        // Correct invariant: semantic_ref_count == eligible_chunk_count (2 refs) even though vectors ==1
        let mut conn = open_in_memory()?;
        let embedder = FakeEmbedder::new("dup-invariant", 8);
        let fp = embedder.fingerprint();
        // Create two chunks with identical representation: same file, same language, same parent_symbol, same slice
        let file_content = "hello";
        for (id, ch) in [("c1", "h1"), ("c2", "h2")] {
            conn.execute(
                "INSERT OR IGNORE INTO files (path, hash, language) VALUES ('a.py','fh','python')",
                [],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,'a.py','python',1,2,0,5,NULL,?2,5)",
                rusqlite::params![id, ch],
            )?;
        }
        let chunks = vec![
            Chunk {
                id: "c1".to_string(),
                file: "a.py".to_string(),
                language: Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 5,
                parent_symbol: None,
                content_hash: "h1".to_string(),
                text_size_bytes: 5,
            },
            Chunk {
                id: "c2".to_string(),
                file: "a.py".to_string(),
                language: Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 5,
                parent_symbol: None,
                content_hash: "h2".to_string(),
                text_size_bytes: 5,
            },
        ];
        let (reused, created) =
            sync_vectors_for_file(&mut conn, "a.py", &chunks, file_content, &embedder).await?;
        assert_eq!(reused, 0);
        assert_eq!(created, 1, "duplicate reps should create 1 vector");
        assert_eq!(eligible_chunk_count(&conn)?, 2, "eligible should be 2");
        assert_eq!(
            semantic_ref_count(&conn)?,
            2,
            "semantic refs must equal eligible even with duplicate hash"
        );
        assert_eq!(
            count_vectors(&conn, &fp)?,
            1,
            "vectors should be 1 for duplicate hash"
        );
        assert_eq!(missing_vector_count(&conn, &fp)?, 0);
        assert!(is_semantic_ready(&conn, &fp, true)?);
        // Search should fan out to both refs (both chunks share same vector)
        let qvec = embedder.embed_query(file_content).await?;
        let res = search_brute(&conn, &qvec, &fp, 5)?;
        // Both chunks should be returned (2 results) since they share same hash but are separate refs
        let ids: Vec<String> = res.iter().map(|c| c.chunk_id.clone()).collect();
        assert!(ids.contains(&"c1".to_string()), "c1 should be found");
        assert!(ids.contains(&"c2".to_string()), "c2 should be found");
        Ok(())
    }

    #[tokio::test]
    async fn incremental_changed_file_duplicate_handling() -> Result<()> {
        // Incremental path with changed file containing duplicate reps
        let mut conn = open_in_memory()?;
        let embedder = FakeEmbedder::new("inc-dup", 8);
        let file_content = "hello";
        // Setup initial file with one chunk
        conn.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES ('a.py','fh','python')",
            [],
        )?;
        conn.execute("INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c0','a.py','python',1,2,0,5,NULL,'h0',5)", [])?;
        let c0 = Chunk {
            id: "c0".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 5,
            parent_symbol: None,
            content_hash: "h0".to_string(),
            text_size_bytes: 5,
        };
        let _ = sync_vectors_for_file(&mut conn, "a.py", &[c0], file_content, &embedder).await?;
        // Now simulate changed file with 2 chunks sharing same rep (e.g., file edit creates duplicate)
        conn.execute("DELETE FROM chunks WHERE id='c0'", [])?;
        conn.execute("DELETE FROM semantic_chunk_refs WHERE chunk_id='c0'", [])?;
        let _ = gc_orphaned_vectors(&conn, &embedder.fingerprint())?;
        for (id, ch) in [("c1", "h1"), ("c2", "h2")] {
            conn.execute(
                "INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,'a.py','python',1,2,0,5,NULL,?2,5)",
                rusqlite::params![id, ch],
            )?;
        }
        let chunks = vec![
            Chunk {
                id: "c1".to_string(),
                file: "a.py".to_string(),
                language: Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 5,
                parent_symbol: None,
                content_hash: "h1".to_string(),
                text_size_bytes: 5,
            },
            Chunk {
                id: "c2".to_string(),
                file: "a.py".to_string(),
                language: Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 5,
                parent_symbol: None,
                content_hash: "h2".to_string(),
                text_size_bytes: 5,
            },
        ];
        let (_reused, created) =
            sync_vectors_for_file(&mut conn, "a.py", &chunks, file_content, &embedder).await?;
        assert_eq!(created, 1);
        assert_eq!(semantic_ref_count(&conn)?, 2);
        assert_eq!(eligible_chunk_count(&conn)?, 2);
        assert_eq!(count_vectors(&conn, &embedder.fingerprint())?, 1);
        Ok(())
    }
}
