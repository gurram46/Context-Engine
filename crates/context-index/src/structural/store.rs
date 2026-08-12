use crate::structural::language::Language;
use crate::structural::types::{
    CallConfidence, CallEdge, ParsedFile, Reference, ReferenceKind, Symbol, SymbolKind, Visibility,
};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i32 = 2;

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

        -- R4D native vector store (content-hash keyed for reuse)
        CREATE TABLE IF NOT EXISTS vectors (
            content_hash TEXT NOT NULL,
            model_id TEXT NOT NULL,
            version TEXT NOT NULL,
            dimension INTEGER NOT NULL,
            vector BLOB NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(content_hash, model_id, version)
        );
        CREATE INDEX IF NOT EXISTS idx_vectors_model ON vectors(model_id, version);
        CREATE INDEX IF NOT EXISTS idx_vectors_hash ON vectors(content_hash);
        "#,
    )?;

    // Check schema version
    let existing: Option<i32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()?;
    match existing {
        None => {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        }
        Some(v) if v == SCHEMA_VERSION => {}
        Some(v) if v < SCHEMA_VERSION => {
            // Simple migration: for R3, we only have v1, so no migration needed.
            // If older, update.
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
pub fn upsert_parsed_file(conn: &mut Connection, pf: &ParsedFile, size_bytes: u64) -> Result<()> {
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

    for s in &pf.symbols {
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
    for c in &pf.chunks {
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
        "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE name LIKE ?1 OR qualified_name LIKE ?1 ORDER BY name LIMIT 50",
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

pub fn find_callers(conn: &Connection, symbol_id_or_name: &str) -> Result<Vec<CallEdge>> {
    // Resolve symbol name to ids, then find edges where resolved_symbol_id matches or callee_name matches
    // First try to find symbol ids for name
    let syms = find_definitions(conn, symbol_id_or_name).unwrap_or_default();
    let ids: Vec<String> = syms.iter().map(|s| s.id.clone()).collect();
    let mut out = Vec::new();
    if !ids.is_empty() {
        for id in &ids {
            let mut stmt = conn.prepare(
                "SELECT caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line FROM call_edges WHERE resolved_symbol_id=?1 LIMIT 50",
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
            return Ok(out);
        }
    }
    // Fallback: search by callee_name
    let mut stmt = conn.prepare(
        "SELECT caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line FROM call_edges WHERE callee_name=?1 LIMIT 50",
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
    Ok(out)
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
            "SELECT caller_symbol_id, callee_name, resolved_symbol_id, confidence, file, line FROM call_edges WHERE caller_symbol_id=?1 LIMIT 50",
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
    Ok(out)
}

pub fn find_tests_related(conn: &Connection, query: &str) -> Result<Vec<Symbol>> {
    // Use FileKind::Test via filename heuristics + symbol relationships.
    // Find symbols with name matching query (prefix) and then filter to test files using LIKE on file.
    // For R3, we just find symbols where file contains test patterns.
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent FROM symbols WHERE (name LIKE ?1 OR qualified_name LIKE ?1) AND (file LIKE '%test%' OR file LIKE '%Test%' OR file LIKE '%_test.go' OR file LIKE '%spec%') LIMIT 20",
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
}
