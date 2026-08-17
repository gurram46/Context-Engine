use crate::structural::language::Language;
use crate::structural::types::{
    CallConfidence, CallEdge, ParsedFile, Reference, ReferenceKind, Symbol, SymbolKind, Visibility,
};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i32 = 3;

/// Index location — worktree-safe.
/// Chooses `<repo>/.context/index/structural.db`
/// Each worktree gets its own directory, so they don't fight.
/// We also ensure .gitignore inside .context.
pub fn index_db_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".context")
        .join("index")
        .join("structural.db")
}

pub fn ensure_index_dir(project_root: &Path) -> Result<PathBuf> {
    let dir = project_root.join(".context").join("index");
    std::fs::create_dir_all(&dir)?;
    // Ensure .context/.gitignore exists to avoid committing
    let gitignore = project_root.join(".context").join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(&gitignore, "*\n");
    }
    Ok(dir)
}

/// Open or create DB, ensure schema.
/// Returns connection.
pub fn open_db(project_root: &Path) -> Result<Connection> {
    ensure_index_dir(project_root)?;
    let db_path = index_db_path(project_root);
    let conn = Connection::open(&db_path)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Open DB in temp path (for tests)
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Async wrapper for [`open_db`] that runs the filesystem/locking work on a
/// blocking thread. `contextd` uses this so SQLite open syscalls do not stall
/// the async executor.
pub async fn open_db_async(project_root: PathBuf) -> Result<Connection> {
    tokio::task::spawn_blocking(move || open_db(&project_root))
        .await
        .map_err(|e| anyhow::anyhow!("open_db panicked: {e}"))?
        .map_err(|e| anyhow::anyhow!("open_db failed: {e}"))
}

fn init_schema(conn: &Connection) -> Result<()> {
    // First create core structural tables (without vectors/semantic) to allow version check without column errors
    conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        PRAGMA foreign_keys=ON;

        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY,
            hash TEXT NOT NULL,
            language TEXT NOT NULL,
            size_bytes INTEGER,
            modified_time TEXT,
            parse_error TEXT
        );

        CREATE TABLE IF NOT EXISTS symbols (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            qualified_name TEXT NOT NULL,
            kind TEXT NOT NULL,
            file TEXT NOT NULL,
            language TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            visibility TEXT NOT NULL,
            parent TEXT,
            FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON symbols(qualified_name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file);

        CREATE TABLE IF NOT EXISTS imports (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file TEXT NOT NULL,
            import_path TEXT NOT NULL,
            alias TEXT,
            line INTEGER NOT NULL,
            is_relative INTEGER NOT NULL,
            FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file);

        CREATE TABLE IF NOT EXISTS refs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            file TEXT NOT NULL,
            line INTEGER NOT NULL,
            parent_symbol TEXT,
            kind TEXT NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_refs_name ON refs(name);
        CREATE INDEX IF NOT EXISTS idx_refs_file ON refs(file);
        CREATE INDEX IF NOT EXISTS idx_refs_parent ON refs(parent_symbol);

        CREATE TABLE IF NOT EXISTS chunks (
            id TEXT PRIMARY KEY,
            file TEXT NOT NULL,
            language TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            parent_symbol TEXT,
            content_hash TEXT NOT NULL,
            text_size_bytes INTEGER NOT NULL,
            FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file);
        CREATE INDEX IF NOT EXISTS idx_chunks_parent ON chunks(parent_symbol);

        CREATE TABLE IF NOT EXISTS call_edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            caller_symbol_id TEXT NOT NULL,
            callee_name TEXT NOT NULL,
            resolved_symbol_id TEXT,
            confidence TEXT NOT NULL,
            file TEXT NOT NULL,
            line INTEGER NOT NULL,
            FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_edges_caller ON call_edges(caller_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_edges_callee ON call_edges(callee_name);
        CREATE INDEX IF NOT EXISTS idx_edges_resolved ON call_edges(resolved_symbol_id);

        CREATE TABLE IF NOT EXISTS structural_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        -- R4 BM25 native lexical index
        CREATE TABLE IF NOT EXISTS bm25_documents (
            doc_id TEXT PRIMARY KEY,
            chunk_id TEXT NOT NULL,
            file TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            length INTEGER NOT NULL,
            symbol TEXT,
            start_line INTEGER,
            end_line INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_bm25_docs_file ON bm25_documents(file);
        CREATE INDEX IF NOT EXISTS idx_bm25_docs_hash ON bm25_documents(content_hash);

        CREATE TABLE IF NOT EXISTS bm25_postings (
            term TEXT NOT NULL,
            doc_id TEXT NOT NULL,
            tf INTEGER NOT NULL,
            PRIMARY KEY(term, doc_id),
            FOREIGN KEY(doc_id) REFERENCES bm25_documents(doc_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_bm25_post_term ON bm25_postings(term);
        CREATE INDEX IF NOT EXISTS idx_bm25_post_doc ON bm25_postings(doc_id);

        CREATE TABLE IF NOT EXISTS bm25_terms (
            term TEXT PRIMARY KEY,
            df INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS bm25_stats (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;

    // Check schema version before handling vectors/semantic
    let existing: Option<i32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()?;
    match existing {
        None => {
            // Fresh DB: create new v3 semantic tables
            conn.execute_batch(
                r#"
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
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        }
        Some(v) if v == SCHEMA_VERSION => {
            // ponytail: SCHEMA_VERSION 3 with representation_hash was introduced in f58e54c (R5.1-C2).
            // No released/merged history had v3 with old content_hash shape (v2 was 2, v3 is new), so
            // CREATE IF NOT EXISTS is safe for idempotent re-create. No shape validation/migration
            // for hypothetical intermediate dev DBs with v3+old shape — not a supported upgrade path.
            // Supported path is v2->v3 via the Some(2) branch below.
            conn.execute_batch(
                r#"
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
        }
        Some(2) => {
            // Migration v2 -> v3: discard unsafe content_hash vectors, create new representation-keyed store.
            conn.execute_batch(
                r#"
                DROP TABLE IF EXISTS vectors;
                CREATE TABLE vectors (
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
            conn.execute(
                "UPDATE schema_version SET version=?1",
                params![SCHEMA_VERSION],
            )?;
        }
        Some(v) if v < SCHEMA_VERSION => {
            conn.execute_batch(
                r#"
                DROP TABLE IF EXISTS vectors;
                CREATE TABLE vectors (
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
            conn.execute(
                "UPDATE schema_version SET version=?1",
                params![SCHEMA_VERSION],
            )?;
        }
        Some(v) => {
            anyhow::bail!(
                "unsupported schema version {} (current {}) — delete .context/index and reindex",
                v,
                SCHEMA_VERSION
            );
        }
    }
    Ok(())
}

/// Upsert parsed file atomically.
/// Deletes old symbols/refs/imports/chunks for that file, then inserts new.
/// If `parse_error` is present, preserves last-known-good symbols/refs/chunks.
pub fn upsert_parsed_file(conn: &mut Connection, pf: &ParsedFile, size_bytes: u64) -> Result<()> {
    if pf.parse_error.is_some() {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE path=?1)",
                params![pf.file],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if exists {
            conn.execute(
                "UPDATE files SET parse_error=?1 WHERE path=?2",
                params![pf.parse_error, pf.file],
            )?;
            tracing::warn!(file=%pf.file, error=?pf.parse_error, "parse error — preserving last-known-good");
            return Ok(());
        }
        // new file with parse error: insert with no symbols (fall through)
    }
    // Defensive dedupe: stable symbol IDs can collide for distinct symbols that share
    // the same qualified name within a file (e.g. trait impl methods). Keep the first.
    let mut seen = std::collections::HashSet::new();
    let symbols: Vec<&Symbol> = pf
        .symbols
        .iter()
        .filter(|s| seen.insert(s.id.clone()))
        .collect();
    if symbols.len() < pf.symbols.len() {
        tracing::warn!(
            file = %pf.file,
            total = %pf.symbols.len(),
            unique = %symbols.len(),
            "deduplicated symbol ids before upsert"
        );
    }
    let tx = conn.transaction()?;
    // Insert or replace file
    tx.execute(
        "INSERT OR REPLACE INTO files (path, hash, language, size_bytes, parse_error) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            pf.file,
            pf.content_hash,
            pf.language.as_str(),
            size_bytes as i64,
            pf.parse_error
        ],
    )?;
    // Delete old associated (cascade would delete on file delete, but we replace so need manual)
    tx.execute("DELETE FROM symbols WHERE file=?1", params![pf.file])?;
    tx.execute("DELETE FROM refs WHERE file=?1", params![pf.file])?;
    tx.execute("DELETE FROM imports WHERE file=?1", params![pf.file])?;
    tx.execute("DELETE FROM chunks WHERE file=?1", params![pf.file])?;
    tx.execute("DELETE FROM call_edges WHERE file=?1", params![pf.file])?;

    for s in symbols {
        tx.execute(
            "INSERT INTO symbols (id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                s.id,
                s.name,
                s.qualified_name,
                s.kind.as_str(),
                s.file,
                s.language.as_str(),
                s.start_line,
                s.end_line,
                s.start_byte as i64,
                s.end_byte as i64,
                s.visibility.as_str(),
                s.parent
            ],
        )?;
    }
    for imp in &pf.imports {
        tx.execute(
            "INSERT INTO imports (file, import_path, alias, line, is_relative) VALUES (?1,?2,?3,?4,?5)",
            params![
                imp.file,
                imp.import_path,
                imp.alias,
                imp.line,
                if imp.is_relative { 1 } else { 0 }
            ],
        )?;
    }
    for r in &pf.references {
        tx.execute(
            "INSERT INTO refs (name, file, line, parent_symbol, kind, start_byte, end_byte) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                r.name,
                r.file,
                r.line,
                r.parent_symbol,
                r.kind.as_str(),
                r.start_byte as i64,
                r.end_byte as i64
            ],
        )?;
    }
    // Defensive dedupe for chunks as well (large vendor JS can produce duplicate chunk ids)
    let mut seen_chunks = std::collections::HashSet::new();
    let chunks: Vec<&crate::structural::types::Chunk> = pf
        .chunks
        .iter()
        .filter(|c| seen_chunks.insert(c.id.clone()))
        .collect();
    if chunks.len() < pf.chunks.len() {
        tracing::warn!(
            file = %pf.file,
            total = %pf.chunks.len(),
            unique = %chunks.len(),
            "deduplicated chunk ids before upsert"
        );
    }
    for c in chunks {
        tx.execute(
            "INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                c.id,
                c.file,
                c.language.as_str(),
                c.start_line,
                c.end_line,
                c.start_byte as i64,
                c.end_byte as i64,
                c.parent_symbol,
                c.content_hash,
                c.text_size_bytes as i64
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn delete_file(conn: &Connection, path: &str) -> Result<()> {
    conn.execute("DELETE FROM files WHERE path=?1", params![path])?;
    // cascades will clean but we already deleted imports etc via foreign key? Actually we rely on cascade, but also explicit above for upsert.
    // Need to delete symbols etc if not cascaded due to REPLACE? For delete, cascade will work because we have foreign keys.
    // However our earlier upsert deletes manually, so for delete we just delete file row.
    Ok(())
}

pub fn list_files(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT path, hash FROM files")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_file_hash(conn: &Connection, path: &str) -> Result<Option<String>> {
    let opt: Option<String> = conn
        .query_row("SELECT hash FROM files WHERE path=?1", params![path], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(opt)
}

/// For tests: count
pub fn count_symbols(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?)
}
pub fn count_files(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?)
}

// --- Native lookup APIs ---

pub fn find_definitions(conn: &Connection, name: &str) -> Result<Vec<Symbol>> {
    // Exact name or qualified_name match, case-sensitive
    let mut stmt = conn.prepare(
        "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE name=?1 OR qualified_name=?1 ORDER BY file, start_line",
    )?;
    let rows = stmt.query_map(params![name], |row| {
        Ok(Symbol {
            id: row.get(0)?,
            name: row.get(1)?,
            qualified_name: row.get(2)?,
            kind: SymbolKind::from_str(&row.get::<_, String>(3)?),
            file: row.get(4)?,
            language: Language::from_str(&row.get::<_, String>(5)?),
            start_line: row.get(6)?,
            end_line: row.get(7)?,
            start_byte: row.get::<_, i64>(8)? as usize,
            end_byte: row.get::<_, i64>(9)? as usize,
            visibility: match row.get::<_, String>(10)?.as_str() {
                "public" => Visibility::Public,
                "private" => Visibility::Private,
                _ => Visibility::Unknown,
            },
            parent: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn find_symbol_exact(conn: &Connection, name: &str) -> Result<Vec<Symbol>> {
    find_definitions(conn, name)
}

pub fn find_symbol_prefix(conn: &Connection, prefix: &str) -> Result<Vec<Symbol>> {
    let pattern = format!("{}%", prefix);
    let mut stmt = conn.prepare(
        "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE name LIKE ?1 OR qualified_name LIKE ?1 ORDER BY name LIMIT 100",
    )?;
    let rows = stmt.query_map(params![pattern], |row| {
        Ok(Symbol {
            id: row.get(0)?,
            name: row.get(1)?,
            qualified_name: row.get(2)?,
            kind: SymbolKind::from_str(&row.get::<_, String>(3)?),
            file: row.get(4)?,
            language: Language::from_str(&row.get::<_, String>(5)?),
            start_line: row.get(6)?,
            end_line: row.get(7)?,
            start_byte: row.get::<_, i64>(8)? as usize,
            end_byte: row.get::<_, i64>(9)? as usize,
            visibility: match row.get::<_, String>(10)?.as_str() {
                "public" => Visibility::Public,
                "private" => Visibility::Private,
                _ => Visibility::Unknown,
            },
            parent: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn find_references(conn: &Connection, name: &str) -> Result<Vec<Reference>> {
    let mut stmt = conn.prepare(
        "SELECT name, file, line, parent_symbol, kind, start_byte, end_byte FROM refs WHERE name=?1 ORDER BY file, line LIMIT 100",
    )?;
    let rows = stmt.query_map(params![name], |row| {
        Ok(Reference {
            name: row.get(0)?,
            file: row.get(1)?,
            line: row.get(2)?,
            parent_symbol: row.get(3)?,
            kind: ReferenceKind::from_str(&row.get::<_, String>(4)?),
            start_byte: row.get::<_, i64>(5)? as usize,
            end_byte: row.get::<_, i64>(6)? as usize,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn dedup_call_edges(edges: Vec<CallEdge>) -> Vec<CallEdge> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in edges {
        let target = e
            .resolved_symbol_id
            .clone()
            .unwrap_or_else(|| e.callee_name.clone());
        let key = format!("{}:{}:{}:{}", e.file, e.line, e.caller_symbol_id, target);
        if seen.insert(key) {
            out.push(e);
        }
    }
    out
}

pub fn find_callers(conn: &Connection, symbol_id_or_name: &str) -> Result<Vec<CallEdge>> {
    // Resolve symbol name to ids, then find edges where resolved_symbol_id matches or callee_name matches
    // First try to find symbol ids for name
    let syms = find_definitions(conn, symbol_id_or_name).unwrap_or_default();
    let ids: Vec<String> = syms.iter().map(|s| s.id.clone()).collect();
    let mut out = Vec::new();
    if !ids.is_empty() {
        for id in &ids {
            let mut stmt = conn.prepare(
                "SELECT caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line FROM call_edges WHERE resolved_symbol_id=?1 ORDER BY file, line LIMIT 100",
            )?;
            let rows = stmt.query_map(params![id], |row| {
                Ok(CallEdge {
                    caller_symbol_id: row.get(0)?,
                    callee_name: row.get(1)?,
                    resolved_symbol_id: row.get(2)?,
                    confidence: match row.get::<_, String>(3)?.as_str() {
                        "resolved" => CallConfidence::Resolved,
                        "probable" => CallConfidence::Probable,
                        _ => CallConfidence::Unresolved,
                    },
                    file: row.get(4)?,
                    line: row.get(5)?,
                })
            })?;
            for r in rows {
                out.push(r?);
            }
        }
        if !out.is_empty() {
            return Ok(dedup_call_edges(out));
        }
    }
    // Fallback: search by exact qualified callee_name
    let mut stmt = conn.prepare(
        "SELECT caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line FROM call_edges WHERE callee_name=?1 ORDER BY file, line LIMIT 100",
    )?;
    let rows = stmt.query_map(params![symbol_id_or_name], |row| {
        Ok(CallEdge {
            caller_symbol_id: row.get(0)?,
            callee_name: row.get(1)?,
            resolved_symbol_id: row.get(2)?,
            confidence: match row.get::<_, String>(3)?.as_str() {
                "resolved" => CallConfidence::Resolved,
                "probable" => CallConfidence::Probable,
                _ => CallConfidence::Unresolved,
            },
            file: row.get(4)?,
            line: row.get(5)?,
        })
    })?;
    for r in rows {
        out.push(r?);
    }
    if !out.is_empty() {
        return Ok(dedup_call_edges(out));
    }
    Ok(dedup_call_edges(out))
}

pub fn find_callees(conn: &Connection, symbol_id_or_name: &str) -> Result<Vec<CallEdge>> {
    // Find edges where caller_symbol_id matches
    let syms = find_definitions(conn, symbol_id_or_name).unwrap_or_default();
    let ids: Vec<String> = if syms.is_empty() {
        // treat input as id directly
        vec![symbol_id_or_name.to_string()]
    } else {
        syms.iter().map(|s| s.id.clone()).collect()
    };
    let mut out = Vec::new();
    for id in ids {
        let mut stmt = conn.prepare(
            "SELECT caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line FROM call_edges WHERE caller_symbol_id=?1 ORDER BY file, line LIMIT 100",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(CallEdge {
                caller_symbol_id: row.get(0)?,
                callee_name: row.get(1)?,
                resolved_symbol_id: row.get(2)?,
                confidence: match row.get::<_, String>(3)?.as_str() {
                    "resolved" => CallConfidence::Resolved,
                    "probable" => CallConfidence::Probable,
                    _ => CallConfidence::Unresolved,
                },
                file: row.get(4)?,
                line: row.get(5)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
    }
    Ok(dedup_call_edges(out))
}

pub fn find_tests_related(conn: &Connection, query: &str) -> Result<Vec<Symbol>> {
    // Use FileKind::Test via filename heuristics + symbol relationships.
    // Find symbols with name matching query (prefix) and then filter to test files using LIKE on file.
    // For R3, we just find symbols where file contains test patterns.
    // For single-letter queries like "Q", use exact match to avoid matching many symbols containing "q" as substring (e.g., sequence_list)
    let (pattern, use_exact) = if query.len() == 1 {
        (query.to_string(), true)
    } else {
        (format!("%{}%", query), false)
    };
    let mut stmt = if use_exact {
        conn.prepare(
            "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE (name = ?1 OR qualified_name = ?1) AND (file LIKE '%test%' OR file LIKE '%Test%' OR file LIKE '%_test.go' OR file LIKE '%spec%') ORDER BY file, start_line LIMIT 20",
        )?
    } else {
        conn.prepare(
            "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE (name LIKE ?1 OR qualified_name LIKE ?1) AND (file LIKE '%test%' OR file LIKE '%Test%' OR file LIKE '%_test.go' OR file LIKE '%spec%') ORDER BY file, start_line LIMIT 20",
        )?
    };
    let rows = stmt.query_map(params![pattern], |row| {
        Ok(Symbol {
            id: row.get(0)?,
            name: row.get(1)?,
            qualified_name: row.get(2)?,
            kind: SymbolKind::from_str(&row.get::<_, String>(3)?),
            file: row.get(4)?,
            language: Language::from_str(&row.get::<_, String>(5)?),
            start_line: row.get(6)?,
            end_line: row.get(7)?,
            start_byte: row.get::<_, i64>(8)? as usize,
            end_byte: row.get::<_, i64>(9)? as usize,
            visibility: match row.get::<_, String>(10)?.as_str() {
                "public" => Visibility::Public,
                "private" => Visibility::Private,
                _ => Visibility::Unknown,
            },
            parent: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn find_symbol_by_id(conn: &Connection, id: &str) -> Result<Option<Symbol>> {
    let opt = conn
        .query_row(
            "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE id=?1",
            params![id],
            |row| {
                Ok(Symbol {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    qualified_name: row.get(2)?,
                    kind: SymbolKind::from_str(&row.get::<_, String>(3)?),
                    file: row.get(4)?,
                    language: Language::from_str(&row.get::<_, String>(5)?),
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    start_byte: row.get::<_, i64>(8)? as usize,
                    end_byte: row.get::<_, i64>(9)? as usize,
                    visibility: match row.get::<_, String>(10)?.as_str() {
                        "public" => Visibility::Public,
                        "private" => Visibility::Private,
                        _ => Visibility::Unknown,
                    },
                    parent: row.get(11)?,
                })
            },
        )
        .optional()?;
    Ok(opt)
}

pub fn upsert_call_edges(conn: &mut Connection, edges: &[CallEdge]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM call_edges", [])?; // For simplicity, rebuild all edges each indexing run (legacy)
    for e in edges {
        tx.execute(
            "INSERT INTO call_edges (caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                e.caller_symbol_id,
                e.callee_name,
                e.resolved_symbol_id,
                e.confidence.as_str(),
                e.file,
                e.line
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn insert_call_edges(conn: &mut Connection, edges: &[CallEdge]) -> Result<usize> {
    if edges.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let mut inserted = 0usize;
    for e in edges {
        tx.execute(
            "INSERT INTO call_edges (caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                e.caller_symbol_id,
                e.callee_name,
                e.resolved_symbol_id,
                e.confidence.as_str(),
                e.file,
                e.line
            ],
        )?;
        inserted += 1;
    }
    tx.commit()?;
    Ok(inserted)
}

pub fn delete_call_edges_for_files(conn: &Connection, files: &[String]) -> Result<usize> {
    if files.is_empty() {
        return Ok(0);
    }
    let placeholders = files.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM call_edges WHERE file IN ({})", placeholders);
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::ToSql> =
        files.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    Ok(stmt.execute(params.as_slice())?)
}

pub fn delete_call_edges_for_callee_names_excluding_files(
    conn: &Connection,
    names: &[String],
    exclude_files: &[String],
) -> Result<usize> {
    if names.is_empty() {
        return Ok(0);
    }
    let name_placeholders = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut sql = format!(
        "DELETE FROM call_edges WHERE callee_name IN ({})",
        name_placeholders
    );
    let mut all_params: Vec<String> = names.to_vec();
    if !exclude_files.is_empty() {
        let file_placeholders = exclude_files
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        sql.push_str(&format!(" AND file NOT IN ({})", file_placeholders));
        all_params.extend(exclude_files.iter().cloned());
    }
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = all_params
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    Ok(stmt.execute(params_refs.as_slice())?)
}

pub fn load_symbols_for_file(conn: &Connection, file: &str) -> Result<Vec<Symbol>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE file=?1",
    )?;
    let rows = stmt.query_map(params![file], |row| {
        Ok(Symbol {
            id: row.get(0)?,
            name: row.get(1)?,
            qualified_name: row.get(2)?,
            kind: SymbolKind::from_str(&row.get::<_, String>(3)?),
            file: row.get(4)?,
            language: Language::from_str(&row.get::<_, String>(5)?),
            start_line: row.get(6)?,
            end_line: row.get(7)?,
            start_byte: row.get::<_, i64>(8)? as usize,
            end_byte: row.get::<_, i64>(9)? as usize,
            visibility: match row.get::<_, String>(10)?.as_str() {
                "public" => Visibility::Public,
                "private" => Visibility::Private,
                _ => Visibility::Unknown,
            },
            parent: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn load_refs_for_file(conn: &Connection, file: &str) -> Result<Vec<Reference>> {
    let mut stmt = conn.prepare(
        "SELECT name, file, line, parent_symbol, kind, start_byte, end_byte FROM refs WHERE file=?1",
    )?;
    let rows = stmt.query_map(params![file], |row| {
        Ok(Reference {
            name: row.get(0)?,
            file: row.get(1)?,
            line: row.get(2)?,
            parent_symbol: row.get(3)?,
            kind: ReferenceKind::from_str(&row.get::<_, String>(4)?),
            start_byte: row.get::<_, i64>(5)? as usize,
            end_byte: row.get::<_, i64>(6)? as usize,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn load_refs_by_callee_names_excluding_files(
    conn: &Connection,
    names: &[String],
    exclude_files: &[String],
) -> Result<Vec<Reference>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let name_placeholders = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut sql = format!(
        "SELECT name, file, line, parent_symbol, kind, start_byte, end_byte FROM refs WHERE kind='call' AND name IN ({})",
        name_placeholders
    );
    let mut all_params: Vec<String> = names.to_vec();
    if !exclude_files.is_empty() {
        let file_placeholders = exclude_files
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        sql.push_str(&format!(" AND file NOT IN ({})", file_placeholders));
        all_params.extend(exclude_files.iter().cloned());
    }
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = all_params
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(Reference {
            name: row.get(0)?,
            file: row.get(1)?,
            line: row.get(2)?,
            parent_symbol: row.get(3)?,
            kind: ReferenceKind::from_str(&row.get::<_, String>(4)?),
            start_byte: row.get::<_, i64>(5)? as usize,
            end_byte: row.get::<_, i64>(6)? as usize,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn count_call_edges(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM call_edges", [], |r| r.get(0))?)
}

pub fn get_generation(conn: &Connection) -> Result<u64> {
    let opt: Option<String> = conn
        .query_row(
            "SELECT value FROM structural_meta WHERE key='generation'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(opt.and_then(|s| s.parse().ok()).unwrap_or(0))
}

pub fn set_generation(conn: &Connection, gen: u64) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO structural_meta (key, value) VALUES ('generation', ?1)",
        params![gen.to_string()],
    )?;
    Ok(())
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO structural_meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    let opt: Option<String> = conn
        .query_row(
            "SELECT value FROM structural_meta WHERE key=?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(opt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_creates() {
        let conn = open_in_memory().unwrap();
        let v: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }
    #[test]
    fn worktree_path_isolated() {
        let p1 = index_db_path(Path::new("/tmp/repo1"));
        let p2 = index_db_path(Path::new("/tmp/repo2"));
        assert_ne!(p1, p2);
        assert!(p1.to_string_lossy().contains("repo1"));
    }
    #[test]
    fn preserve_last_good_on_parse_error() {
        let mut conn = open_in_memory().unwrap();
        let pf_good = crate::structural::types::ParsedFile {
            file: "a.py".into(),
            language: crate::structural::language::Language::Python,
            content_hash: "hash_good".into(),
            symbols: vec![crate::structural::types::Symbol {
                id: "id_foo".into(),
                name: "foo".into(),
                qualified_name: "foo".into(),
                kind: crate::structural::types::SymbolKind::Function,
                file: "a.py".into(),
                language: crate::structural::language::Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 10,
                visibility: crate::structural::types::Visibility::Private,
                parent: None,
            }],
            references: vec![],
            imports: vec![],
            chunks: vec![],
            parse_error: None,
        };
        upsert_parsed_file(&mut conn, &pf_good, 100).unwrap();
        assert_eq!(count_symbols(&conn).unwrap(), 1);
        let pf_bad = crate::structural::types::ParsedFile {
            file: "a.py".into(),
            language: crate::structural::language::Language::Python,
            content_hash: "hash_bad".into(),
            symbols: vec![],
            references: vec![],
            imports: vec![],
            chunks: vec![],
            parse_error: Some("partial parse with errors".into()),
        };
        upsert_parsed_file(&mut conn, &pf_bad, 100).unwrap();
        assert_eq!(
            count_symbols(&conn).unwrap(),
            1,
            "symbols should be preserved on parse error"
        );
        let hash: String = conn
            .query_row("SELECT hash FROM files WHERE path='a.py'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(hash, "hash_good", "hash should remain last-good");
    }

    #[test]
    fn dedup_chunk_ids_before_upsert() {
        // Production chunk_id = blake3(file, start_byte, end_byte, content_hash)
        // So a real duplicate is an ACTUAL identical duplicate (same file/range/hash -> same ID)
        let mut conn = open_in_memory().unwrap();
        let lang = crate::structural::language::Language::Python;
        let content_hash_a = "hash_a".to_string();
        let dup_id = crate::structural::types::chunk_id("a.py", 0, 10, &content_hash_a);
        let chunk_a = crate::structural::types::Chunk {
            id: dup_id.clone(),
            file: "a.py".into(),
            language: lang,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 10,
            parent_symbol: None,
            content_hash: content_hash_a.clone(),
            text_size_bytes: 10,
        };
        // Actual identical duplicate
        let chunk_b = chunk_a.clone();
        let content_hash_c = "hash_c".to_string();
        let unique_id = crate::structural::types::chunk_id("a.py", 40, 50, &content_hash_c);
        let chunk_c = crate::structural::types::Chunk {
            id: unique_id.clone(),
            file: "a.py".into(),
            language: lang,
            start_line: 5,
            end_line: 6,
            start_byte: 40,
            end_byte: 50,
            parent_symbol: None,
            content_hash: content_hash_c.clone(),
            text_size_bytes: 10,
        };
        let pf = crate::structural::types::ParsedFile {
            file: "a.py".into(),
            language: lang,
            content_hash: "file_hash".into(),
            symbols: vec![],
            references: vec![],
            imports: vec![],
            chunks: vec![chunk_a.clone(), chunk_b, chunk_c.clone()],
            parse_error: None,
        };
        // Should succeed despite duplicate IDs
        upsert_parsed_file(&mut conn, &pf, 100).unwrap();
        // Exactly one dup_id and one unique_id should persist, total 2
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks WHERE file='a.py'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            count, 2,
            "duplicate chunk ID should be deduped to 1, plus unique"
        );
        let dup_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE id=?1",
                params![dup_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dup_count, 1, "exactly one dup_id should remain");
        let uniq_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chunks WHERE id=?1)",
                params![unique_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(uniq_exists, "unique chunk should remain");
        // Ensure no unrelated chunk lost: query all chunks
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2);
        // Persisted duplicate fields must match the original chunk
        let (persisted_start, persisted_hash): (i64, String) = conn
            .query_row(
                "SELECT start_byte, content_hash FROM chunks WHERE id=?1",
                params![dup_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted_start as usize, chunk_a.start_byte);
        assert_eq!(persisted_hash, chunk_a.content_hash);
    }

    #[test]
    fn migration_v2_to_v3_discards_old_vectors() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let db_path = crate::structural::store::index_db_path(tmp.path());
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        // Create a v2 DB manually
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                PRAGMA journal_mode=WAL;
                PRAGMA foreign_keys=ON;
                CREATE TABLE schema_version (version INTEGER PRIMARY KEY, created_at TEXT NOT NULL DEFAULT (datetime('now')));
                INSERT INTO schema_version (version) VALUES (2);
                CREATE TABLE files (path TEXT PRIMARY KEY, hash TEXT NOT NULL, language TEXT NOT NULL, size_bytes INTEGER, modified_time TEXT, parse_error TEXT);
                INSERT INTO files (path, hash, language) VALUES ('a.py', 'h1', 'python');
                CREATE TABLE symbols (id TEXT PRIMARY KEY, name TEXT NOT NULL, qualified_name TEXT NOT NULL, kind TEXT NOT NULL, file TEXT NOT NULL, language TEXT NOT NULL, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL, start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL, visibility TEXT NOT NULL, parent TEXT, FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE);
                CREATE TABLE imports (id INTEGER PRIMARY KEY AUTOINCREMENT, file TEXT NOT NULL, import_path TEXT NOT NULL, alias TEXT, line INTEGER NOT NULL, is_relative INTEGER NOT NULL, FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE);
                CREATE TABLE refs (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, file TEXT NOT NULL, line INTEGER NOT NULL, parent_symbol TEXT, kind TEXT NOT NULL, start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL, FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE);
                CREATE TABLE chunks (id TEXT PRIMARY KEY, file TEXT NOT NULL, language TEXT NOT NULL, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL, start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL, parent_symbol TEXT, content_hash TEXT NOT NULL, text_size_bytes INTEGER NOT NULL, FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE);
                INSERT INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES ('c1', 'a.py', 'python', 1, 2, 0, 10, NULL, 'chash1', 10);
                CREATE TABLE call_edges (id INTEGER PRIMARY KEY AUTOINCREMENT, caller_symbol_id TEXT NOT NULL, callee_name TEXT NOT NULL, resolved_symbol_id TEXT, confidence TEXT NOT NULL, file TEXT NOT NULL, line INTEGER NOT NULL, FOREIGN KEY(file) REFERENCES files(path) ON DELETE CASCADE);
                CREATE TABLE structural_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE bm25_documents (doc_id TEXT PRIMARY KEY, chunk_id TEXT NOT NULL, file TEXT NOT NULL, content_hash TEXT NOT NULL, length INTEGER NOT NULL, symbol TEXT, start_line INTEGER, end_line INTEGER);
                CREATE TABLE bm25_postings (term TEXT NOT NULL, doc_id TEXT NOT NULL, tf INTEGER NOT NULL, PRIMARY KEY(term, doc_id), FOREIGN KEY(doc_id) REFERENCES bm25_documents(doc_id) ON DELETE CASCADE);
                CREATE TABLE bm25_terms (term TEXT PRIMARY KEY, df INTEGER NOT NULL);
                CREATE TABLE bm25_stats (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO bm25_documents (doc_id, chunk_id, file, content_hash, length) VALUES ('d1', 'c1', 'a.py', 'chash1', 10);
                CREATE TABLE vectors (content_hash TEXT NOT NULL, model_id TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, vector BLOB NOT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now')), PRIMARY KEY(content_hash, model_id, version));
                INSERT INTO vectors (content_hash, model_id, version, dimension, vector) VALUES ('chash1', 'all-minilm', 'ollama-all-minilm-v1', 384, randomblob(384*4));
                "#,
            )
            .unwrap();
        }
        // Now open via our store which should migrate to v3
        let conn = open_db(tmp.path()).unwrap();
        let v: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 3, "should migrate to 3");
        // structural data survives
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 1);
        let cnt2: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt2, 1, "BM25 should survive");
        // old vectors should be discarded (new table empty)
        let cnt_vectors: i64 = conn
            .query_row("SELECT COUNT(*) FROM vectors", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt_vectors, 0, "old unsafe vectors should be discarded");
        // new semantic tables exist
        let cnt_refs: i64 = conn
            .query_row("SELECT COUNT(*) FROM semantic_chunk_refs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt_refs, 0);
        // ensure new vectors schema has representation_hash column
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='vectors'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("representation_hash"),
            "new vectors should have representation_hash: {}",
            sql
        );
    }
}
