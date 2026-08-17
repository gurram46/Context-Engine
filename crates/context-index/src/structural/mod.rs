pub mod language;
pub mod parser;
pub mod store;
pub mod types;

pub use language::{detect_language, Language};
pub use parser::{build_call_edges, parse_file};
pub use store::{
    delete_file, find_callees, find_callers, find_definitions, find_references, find_symbol_exact,
    find_symbol_prefix, find_tests_related, index_db_path, open_db, open_in_memory,
    upsert_call_edges, upsert_parsed_file,
};
pub use types::{
    CallConfidence, CallEdge, Chunk, Import, ParsedFile, Reference, ReferenceKind, Symbol,
    SymbolKind, Visibility,
};

use crate::discovery::ProjectIndex;
use crate::project_root::ProjectRoot;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Structural extraction version — increments when parser/extraction semantics change
/// (e.g., storing both short+qualified for qualified calls). Separate from SQLite SCHEMA_VERSION.
/// Stored in structural_meta as EXTRACTION_VERSION_KEY. When mismatch, forces one-time reparse.
pub const STRUCTURAL_EXTRACTION_VERSION: u32 = 2;
pub const EXTRACTION_VERSION_KEY: &str = "structural_extraction_version";

/// Indexer stats.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub files_skipped: usize,
    pub files_deleted: usize,
    pub symbols: usize,
    pub references: usize,
    pub chunks: usize,
    pub elapsed_ms: u128,
    // R4 incremental counters
    pub edges_deleted: usize,
    pub edges_inserted: usize,
    pub references_reresolved: usize,
    pub structural_generation: u64,
}

/// Result of structural reconcile exposing generic delta without semantic knowledge.
#[derive(Debug, Clone, Default)]
pub struct StructuralBuildOutcome {
    pub stats: IndexStats,
    pub changed_files: Vec<String>,
    pub deleted_files: Vec<String>,
}

/// Structural indexer — owns incremental build logic.
/// Uses hash-based reuse: unchanged hash → skip parse.
/// Persistent store: SQLite at .context/index/structural.db, worktree-safe.
pub struct StructuralIndex {
    pub root: PathBuf,
    pub db_path: PathBuf,
}

impl StructuralIndex {
    pub fn new(project_root: &ProjectRoot) -> Self {
        let root = project_root.path().to_path_buf();
        let db_path = store::index_db_path(&root);
        Self { root, db_path }
    }

    pub fn for_path(root: PathBuf) -> Self {
        let db_path = store::index_db_path(&root);
        Self { root, db_path }
    }

    #[allow(clippy::doc_lazy_continuation)]
    /// Initial or incremental build.
    /// - For every structurally supported file (Language != Unknown), check hash vs DB.
    ///   * hash equal → SKIP PARSE
    ///   * hash changed or new → parse and upsert
    /// - Deletes stale files (in DB but not on disk)
    /// - Incremental call graph update: only outgoing edges of changed files + affected references.
    /// R3: structural discovery walks the worktree directly (including crates/) because
    /// ProjectIndex respects .opencodeignore which hides crates for V2. Structural needs Rust.
    /// R4: incremental graph — no global rebuild on normal one-file change.
    pub fn build(&self, project: &ProjectIndex) -> Result<IndexStats> {
        self.build_with_delta(project).map(|o| o.stats)
    }

    /// Build with generic delta (changed/deleted paths). Ollama-free, deterministic.
    pub fn build_with_delta(&self, project: &ProjectIndex) -> Result<StructuralBuildOutcome> {
        let t0 = Instant::now();
        let mut conn = store::open_db(&self.root)?;
        // Check structural extraction version — if mismatch, force reparse of all files
        let stored_version = store::get_meta(&conn, EXTRACTION_VERSION_KEY).unwrap_or(None);
        let current_version_str = STRUCTURAL_EXTRACTION_VERSION.to_string();
        let needs_extraction_upgrade = stored_version.as_deref() != Some(&current_version_str);
        if needs_extraction_upgrade {
            tracing::info!(
                stored = ?stored_version,
                current = STRUCTURAL_EXTRACTION_VERSION,
                "structural extraction version mismatch, forcing reparse of all structural files"
            );
        }
        let structural_files = collect_structural_files(&self.root, project);
        let mut stats = IndexStats {
            files_discovered: structural_files.len(),
            ..Default::default()
        };

        // Load existing hashes
        let existing = store::list_files(&conn).unwrap_or_default();
        let mut existing_map: std::collections::HashMap<String, String> =
            existing.into_iter().collect();

        // Track incremental state
        let mut changed_files: Vec<String> = Vec::new();
        let mut stale_files: Vec<String> = Vec::new();
        let mut affected_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // For affected detection we need old symbols per file
        let mut old_symbols_map: std::collections::HashMap<String, Vec<Symbol>> =
            std::collections::HashMap::new();

        for (rel, _abs, cur_hash) in &structural_files {
            let lang = detect_language(Path::new(rel));
            if lang == Language::Unknown {
                continue;
            }
            if cur_hash.is_empty() {
                continue;
            }
            if !needs_extraction_upgrade {
                if let Some(prev_hash) = existing_map.get(rel) {
                    if prev_hash == cur_hash {
                        stats.files_skipped += 1;
                        existing_map.remove(rel);
                        continue;
                    }
                }
            }
            // Capture old definitions before overwrite for affected detection
            let old_syms = store::load_symbols_for_file(&conn, rel).unwrap_or_default();
            if !old_syms.is_empty() {
                old_symbols_map.insert(rel.clone(), old_syms);
            }
            // Need to parse
            let abs = self.root.join(rel);
            let content = match std::fs::read_to_string(&abs) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(file=%rel, error=%e, "skip unreadable file, preserve last-good");
                    existing_map.remove(rel);
                    continue;
                }
            };
            let actual_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
            let pf = parse_file(rel, &content, &actual_hash);
            let size = content.len() as u64;
            // Determine affected from old vs new
            let old_names: std::collections::HashSet<String> = old_symbols_map
                .get(rel)
                .map(|v| {
                    v.iter()
                        .flat_map(|s| vec![s.name.clone(), s.qualified_name.clone()])
                        .collect()
                })
                .unwrap_or_default();
            let new_names: std::collections::HashSet<String> = pf
                .symbols
                .iter()
                .flat_map(|s| vec![s.name.clone(), s.qualified_name.clone()])
                .collect();
            for n in old_names.symmetric_difference(&new_names) {
                affected_names.insert(n.clone());
            }
            // Also if old symbols existed and new empty -> affected includes old
            // Upsert
            match store::upsert_parsed_file(&mut conn, &pf, size) {
                Ok(_) => {
                    stats.files_parsed += 1;
                    stats.symbols += pf.symbols.len();
                    stats.references += pf.references.len();
                    stats.chunks += pf.chunks.len();
                    changed_files.push(rel.clone());
                    // R4B: BM25 incremental — transactionally replace postings for changed chunk
                    if let Err(e) =
                        crate::bm25::upsert_bm25_for_file(&mut conn, rel, &pf.chunks, &content)
                    {
                        tracing::warn!(file=%rel, error=%e, "bm25 upsert failed");
                    }
                    existing_map.remove(rel);
                }
                Err(e) => {
                    tracing::warn!(file=%rel, error=%e, "upsert failed, preserve");
                    existing_map.remove(rel);
                }
            }
        }

        // Remaining in existing_map are stale (deleted files)
        for (stale_path, _) in existing_map {
            let old_syms = store::load_symbols_for_file(&conn, &stale_path).unwrap_or_default();
            for s in &old_syms {
                affected_names.insert(s.name.clone());
                affected_names.insert(s.qualified_name.clone());
            }
            let _ = store::delete_file(&conn, &stale_path);
            let _ = crate::bm25::delete_bm25_for_file(&conn, &stale_path);
            stats.files_deleted += 1;
            stale_files.push(stale_path);
        }

        // Incremental call edges
        if !changed_files.is_empty() || !stale_files.is_empty() {
            // If this is initial build (no existing before, or many files), we could have many changes.
            // For initial build with empty DB, changed_files may be many; incremental still works but we want to avoid N queries.
            // Heuristic: if changed+stale > 50% of discovered, do full rebuild for simplicity.
            let total_changed = changed_files.len() + stale_files.len();
            let threshold = (stats.files_discovered as f64 * 0.5) as usize;
            if total_changed > threshold && stats.files_skipped == 0 {
                // Initial large build — full rebuild is acceptable and simpler
                let (del, ins) = rebuild_edges_from_db_with_counts(&mut conn)?;
                stats.edges_deleted = del;
                stats.edges_inserted = ins;
            } else {
                let inc = rebuild_edges_incremental(
                    &mut conn,
                    &changed_files,
                    &stale_files,
                    &affected_names,
                )?;
                stats.edges_deleted = inc.0;
                stats.edges_inserted = inc.1;
                stats.references_reresolved = inc.2;
            }
        }

        // Generation tracking for watcher status
        if !changed_files.is_empty() || !stale_files.is_empty() {
            // Bump generation: read current, increment
            let gen = store::get_generation(&conn).unwrap_or(0) + 1;
            let _ = store::set_generation(&conn, gen);
            stats.structural_generation = gen;
        } else {
            stats.structural_generation = store::get_generation(&conn).unwrap_or(0);
        }

        // Store extraction version after successful build
        if needs_extraction_upgrade {
            let _ = store::set_meta(&conn, EXTRACTION_VERSION_KEY, &current_version_str);
        } else if stored_version.is_none() {
            // First build with no version stored
            let _ = store::set_meta(&conn, EXTRACTION_VERSION_KEY, &current_version_str);
        }

        stats.elapsed_ms = t0.elapsed().as_millis();
        // deterministic ordering for callers (semantic sync) and tests
        changed_files.sort();
        stale_files.sort();
        Ok(StructuralBuildOutcome {
            stats,
            changed_files,
            deleted_files: stale_files,
        })
    }

    /// Single-file incremental update for watcher path.
    /// Caller ensures file exists or is deleted; we handle hash verification.
    pub fn update_single_file(&self, relative_path: &str) -> Result<IndexStats> {
        let abs = self.root.join(relative_path);
        let exists = abs.exists() && abs.is_file();
        let mut conn = store::open_db(&self.root)?;
        let mut stats = IndexStats::default();
        let t0 = Instant::now();

        if !exists {
            // Delete
            let old_syms = store::load_symbols_for_file(&conn, relative_path).unwrap_or_default();
            let mut affected: std::collections::HashSet<String> = std::collections::HashSet::new();
            for s in &old_syms {
                affected.insert(s.name.clone());
                affected.insert(s.qualified_name.clone());
            }
            store::delete_file(&conn, relative_path)?;
            let _ = crate::bm25::delete_bm25_for_file(&conn, relative_path);
            stats.files_deleted = 1;
            if !affected.is_empty() {
                let (del, ins, reresolved) = rebuild_edges_incremental(
                    &mut conn,
                    &[],
                    &[relative_path.to_string()],
                    &affected,
                )?;
                stats.edges_deleted = del;
                stats.edges_inserted = ins;
                stats.references_reresolved = reresolved;
            } else {
                // Still need to delete edges for file (cascade already did)
                let del = store::delete_call_edges_for_files(&conn, &[relative_path.to_string()])
                    .unwrap_or(0);
                stats.edges_deleted = del;
            }
            let gen = store::get_generation(&conn).unwrap_or(0) + 1;
            let _ = store::set_generation(&conn, gen);
            stats.structural_generation = gen;
            stats.elapsed_ms = t0.elapsed().as_millis();
            return Ok(stats);
        }

        let content = std::fs::read_to_string(&abs)?;
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let prev_hash = store::get_file_hash(&conn, relative_path).unwrap_or(None);
        if prev_hash.as_deref() == Some(&hash) {
            stats.files_skipped = 1;
            stats.structural_generation = store::get_generation(&conn).unwrap_or(0);
            stats.elapsed_ms = t0.elapsed().as_millis();
            return Ok(stats);
        }
        let old_syms = store::load_symbols_for_file(&conn, relative_path).unwrap_or_default();
        let pf = parse_file(relative_path, &content, &hash);
        let new_names: std::collections::HashSet<String> = pf
            .symbols
            .iter()
            .flat_map(|s| vec![s.name.clone(), s.qualified_name.clone()])
            .collect();
        let old_names: std::collections::HashSet<String> = old_syms
            .iter()
            .flat_map(|s| vec![s.name.clone(), s.qualified_name.clone()])
            .collect();
        let mut affected: std::collections::HashSet<String> = std::collections::HashSet::new();
        for n in old_names.symmetric_difference(&new_names) {
            affected.insert(n.clone());
        }
        let size = content.len() as u64;
        store::upsert_parsed_file(&mut conn, &pf, size)?;
        let _ = crate::bm25::upsert_bm25_for_file(&mut conn, relative_path, &pf.chunks, &content);
        stats.files_parsed = 1;
        stats.symbols = pf.symbols.len();
        stats.references = pf.references.len();
        stats.chunks = pf.chunks.len();
        let (del, ins, reresolved) =
            rebuild_edges_incremental(&mut conn, &[relative_path.to_string()], &[], &affected)?;
        stats.edges_deleted = del;
        stats.edges_inserted = ins;
        stats.references_reresolved = reresolved;
        let gen = store::get_generation(&conn).unwrap_or(0) + 1;
        let _ = store::set_generation(&conn, gen);
        stats.structural_generation = gen;
        stats.elapsed_ms = t0.elapsed().as_millis();
        Ok(stats)
    }

    /// Query helpers — open DB per call (cheap, SQLite file cache)
    pub fn find_definitions(&self, name: &str) -> Result<Vec<Symbol>> {
        let conn = store::open_db(&self.root)?;
        store::find_definitions(&conn, name)
    }
    pub fn find_symbol_exact(&self, name: &str) -> Result<Vec<Symbol>> {
        let conn = store::open_db(&self.root)?;
        store::find_symbol_exact(&conn, name)
    }
    pub fn find_symbol_prefix(&self, prefix: &str) -> Result<Vec<Symbol>> {
        let conn = store::open_db(&self.root)?;
        store::find_symbol_prefix(&conn, prefix)
    }
    pub fn find_references(&self, name: &str) -> Result<Vec<Reference>> {
        let conn = store::open_db(&self.root)?;
        store::find_references(&conn, name)
    }
    pub fn find_callers(&self, symbol: &str) -> Result<Vec<CallEdge>> {
        let conn = store::open_db(&self.root)?;
        store::find_callers(&conn, symbol)
    }
    pub fn find_callees(&self, symbol: &str) -> Result<Vec<CallEdge>> {
        let conn = store::open_db(&self.root)?;
        store::find_callees(&conn, symbol)
    }
    pub fn find_tests_related(&self, query: &str) -> Result<Vec<Symbol>> {
        let conn = store::open_db(&self.root)?;
        store::find_tests_related(&conn, query)
    }
    pub fn count_symbols(&self) -> Result<i64> {
        let conn = store::open_db(&self.root)?;
        store::count_symbols(&conn)
    }
}

/// Collect structural candidate files — includes crates/ even though ProjectIndex hides them via .opencodeignore.
/// This ensures Rust symbols like retrieve_context and CandidateProvider are indexed.
fn collect_structural_files(root: &Path, project: &ProjectIndex) -> Vec<(String, PathBuf, String)> {
    use ignore::WalkBuilder;
    // Build map of known hashes from ProjectIndex for quick reuse
    let mut known: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for f in &project.files {
        if let Some(h) = &f.content_hash {
            known.insert(f.relative_path.clone(), h.clone());
        }
    }
    let mut out = Vec::new();
    // Walk directly, without .opencodeignore, but respecting .gitignore
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .parents(true)
        .ignore(true)
        .require_git(true)
        .follow_links(false);
    // Do NOT add .opencodeignore — we want crates for structural
    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let rel = match path.strip_prefix(root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        // Exclude engine-internal dirs (target, .git, .context, node_modules, etc) but keep crates
        let lower = rel.to_lowercase();
        let mut skip = false;
        for pat in &[
            "target/",
            ".git/",
            ".context/",
            "node_modules/",
            "dist/",
            "build/",
            "__pycache__/",
            ".pytest_cache/",
            ".next/",
            ".nuxt/",
            "coverage/",
        ] {
            if lower.starts_with(pat) || lower.contains(&format!("/{}", pat)) {
                skip = true;
                break;
            }
        }
        if skip {
            continue;
        }
        if lower == "target" || lower == "node_modules" || lower == ".git" || lower == ".context" {
            continue;
        }
        let lang = detect_language(Path::new(&rel));
        if lang == Language::Unknown {
            continue;
        }
        // Use known hash if available and file not changed outside ProjectIndex view, otherwise hash
        let hash = if let Some(h) = known.get(&rel) {
            // Verify file still exists and size matches? For crates files not in known, compute
            h.clone()
        } else {
            // Compute hash for crates or new files
            match crate::hash::hash_file(path) {
                Ok(h) => h,
                Err(_) => continue,
            }
        };
        // For files where hash from ProjectIndex may be stale due to size limit, recompute if needed
        // If hash is empty (large file), skip
        if hash.is_empty() {
            continue;
        }
        out.push((rel, path.to_path_buf(), hash));
    }
    // Also include any ProjectIndex files that were not found via walk (e.g., if walk missed due to ignore)
    // But walk should have covered them; dedup
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// Rebuild call edges from DB contents.
/// Reads all symbols and refs, builds edges via similar logic to parser::build_call_edges, then upserts.
#[allow(dead_code)]
fn rebuild_edges_from_db(conn: &mut rusqlite::Connection) -> Result<()> {
    let (_, _) = rebuild_edges_from_db_with_counts(conn)?;
    Ok(())
}

fn rebuild_edges_from_db_with_counts(conn: &mut rusqlite::Connection) -> Result<(usize, usize)> {
    use std::collections::HashMap;
    let (by_name, by_qualified) = {
        let mut stmt = conn.prepare("SELECT id, name, qualified_name, file FROM symbols")?;
        let sym_rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut by_name: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut by_qualified: HashMap<String, String> = HashMap::new();
        for r in sym_rows {
            let (id, name, qname, file) = r?;
            by_name
                .entry(name)
                .or_default()
                .push((id.clone(), file.clone()));
            by_qualified.insert(qname, id);
        }
        (by_name, by_qualified)
    };
    let ref_data: Vec<(String, String, i64, Option<String>)> = {
        let mut stmt2 =
            conn.prepare("SELECT name, file, line, parent_symbol FROM refs WHERE kind='call'")?;
        let ref_rows = stmt2.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut v = Vec::new();
        for r in ref_rows {
            v.push(r?);
        }
        v
    };
    let mut edges: Vec<CallEdge> = Vec::new();
    for (name, file, line, parent) in ref_data {
        let candidates = by_name.get(&name);
        let (resolved, confidence) = match candidates {
            Some(list) if list.len() == 1 => (Some(list[0].0.clone()), CallConfidence::Resolved),
            Some(list) if list.len() > 1 => {
                let same_file: Vec<_> = list.iter().filter(|(_, f)| f == &file).collect();
                if same_file.len() == 1 {
                    (Some(same_file[0].0.clone()), CallConfidence::Probable)
                } else {
                    (None, CallConfidence::Unresolved)
                }
            }
            _ => (None, CallConfidence::Unresolved),
        };
        let (resolved, confidence) = if resolved.is_none() {
            if let Some(id) = by_qualified.get(&name) {
                (Some(id.clone()), CallConfidence::Resolved)
            } else {
                (resolved, confidence)
            }
        } else {
            (resolved, confidence)
        };
        edges.push(CallEdge {
            caller_symbol_id: parent.unwrap_or_default(),
            callee_name: name,
            resolved_symbol_id: resolved,
            confidence,
            file,
            line: line as u32,
        });
    }
    let before = store::count_call_edges(conn).unwrap_or(0) as usize;
    store::upsert_call_edges(conn, &edges)?;
    let after = store::count_call_edges(conn).unwrap_or(0) as usize;
    // For full rebuild, deleted = before, inserted = after
    Ok((before, after))
}

/// Incremental edge update.
/// Deletes nothing for changed files (already deleted by upsert), but re-resolves affected references.
/// Returns (edges_deleted, edges_inserted, references_reresolved)
#[allow(clippy::type_complexity)]
fn rebuild_edges_incremental(
    conn: &mut rusqlite::Connection,
    changed_files: &[String],
    stale_files: &[String],
    affected_names: &std::collections::HashSet<String>,
) -> Result<(usize, usize, usize)> {
    use std::collections::HashMap;
    if changed_files.is_empty() && stale_files.is_empty() && affected_names.is_empty() {
        return Ok((0, 0, 0));
    }
    // Load symbol maps from current DB (post upsert/delete)
    let (by_name, by_qualified): (
        HashMap<String, Vec<(String, String)>>,
        HashMap<String, String>,
    ) = {
        let mut stmt = conn.prepare("SELECT id, name, qualified_name, file FROM symbols")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut by_name: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut by_qualified: HashMap<String, String> = HashMap::new();
        for r in rows {
            let (id, name, qname, file) = r?;
            by_name
                .entry(name)
                .or_default()
                .push((id.clone(), file.clone()));
            by_qualified.insert(qname, id);
        }
        (by_name, by_qualified)
    };

    let mut edges_deleted = 0usize;
    let mut edges_inserted = 0usize;
    let mut references_reresolved = 0usize;

    // Helper to resolve a reference
    let resolve = |name: &str, file: &str| -> (Option<String>, CallConfidence) {
        let candidates = by_name.get(name);
        let (resolved, confidence) = match candidates {
            Some(list) if list.len() == 1 => (Some(list[0].0.clone()), CallConfidence::Resolved),
            Some(list) if list.len() > 1 => {
                let same_file: Vec<_> = list.iter().filter(|(_, f)| f == file).collect();
                if same_file.len() == 1 {
                    (Some(same_file[0].0.clone()), CallConfidence::Probable)
                } else {
                    (None, CallConfidence::Unresolved)
                }
            }
            _ => (None, CallConfidence::Unresolved),
        };
        if resolved.is_none() {
            if let Some(id) = by_qualified.get(name) {
                return (Some(id.clone()), CallConfidence::Resolved);
            }
        }
        (resolved, confidence)
    };

    // For affected names, delete old edges (excluding changed files) and count
    if !affected_names.is_empty() {
        let affected_vec: Vec<String> = affected_names.iter().cloned().collect();
        let mut exclude = changed_files.to_vec();
        exclude.extend(stale_files.iter().cloned());
        let del = store::delete_call_edges_for_callee_names_excluding_files(
            conn,
            &affected_vec,
            &exclude,
        )
        .unwrap_or(0);
        edges_deleted += del;
        // Load refs for affected names (excluding changed files)
        let affected_refs =
            store::load_refs_by_callee_names_excluding_files(conn, &affected_vec, &exclude)
                .unwrap_or_default();
        references_reresolved = affected_refs.len();
        let mut edges: Vec<CallEdge> = Vec::new();
        for r in affected_refs {
            if r.kind != ReferenceKind::Call {
                continue;
            }
            let (resolved, confidence) = resolve(&r.name, &r.file);
            edges.push(CallEdge {
                caller_symbol_id: r.parent_symbol.clone().unwrap_or_default(),
                callee_name: r.name.clone(),
                resolved_symbol_id: resolved,
                confidence,
                file: r.file.clone(),
                line: r.line,
            });
        }
        edges_inserted += store::insert_call_edges(conn, &edges).unwrap_or(0);
    }

    // For changed files, insert edges for their refs (edges already deleted by upsert)
    let mut changed_edges: Vec<CallEdge> = Vec::new();
    for file in changed_files {
        let refs = store::load_refs_for_file(conn, file).unwrap_or_default();
        for r in refs {
            if r.kind != ReferenceKind::Call {
                continue;
            }
            let (resolved, confidence) = resolve(&r.name, &r.file);
            changed_edges.push(CallEdge {
                caller_symbol_id: r.parent_symbol.clone().unwrap_or_default(),
                callee_name: r.name.clone(),
                resolved_symbol_id: resolved,
                confidence,
                file: r.file.clone(),
                line: r.line,
            });
        }
    }
    edges_inserted += store::insert_call_edges(conn, &changed_edges).unwrap_or(0);

    // Stale files edges already deleted via cascade; no insert needed

    Ok((edges_deleted, edges_inserted, references_reresolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::ProjectIndex;
    use crate::project_root::ProjectRoot;
    use tempfile::TempDir;

    #[test]
    fn incremental_build() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        // create files
        std::fs::write(tmp.path().join("a.py"), b"def foo():\n    pass\n")?;
        std::fs::write(tmp.path().join("b.py"), b"def bar():\n    foo()\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        let stats1 = si.build(&idx).unwrap();
        assert!(stats1.files_parsed >= 2);
        // Second build no change → should skip
        let idx2 = ProjectIndex::discover(&root).unwrap();
        let stats2 = si.build(&idx2).unwrap();
        assert_eq!(stats2.files_skipped, 2);
        assert_eq!(stats2.files_parsed, 0);
        // Modify one file
        std::fs::write(tmp.path().join("a.py"), b"def foo():\n    x=1\n")?;
        let idx3 = ProjectIndex::discover(&root).unwrap();
        let stats3 = si.build(&idx3).unwrap();
        assert_eq!(stats3.files_parsed, 1);
        assert_eq!(stats3.files_skipped, 1);
        // Delete file
        std::fs::remove_file(tmp.path().join("b.py"))?;
        let idx4 = ProjectIndex::discover(&root).unwrap();
        let stats4 = si.build(&idx4).unwrap();
        assert_eq!(stats4.files_deleted, 1);
        // Verify symbols deleted
        let conn = store::open_db(tmp.path()).unwrap();
        let cnt = store::count_symbols(&conn).unwrap();
        // a.py only has foo
        assert!(cnt >= 1);
        Ok(())
    }

    #[test]
    fn worktree_isolation() -> Result<()> {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        let root1 = ProjectRoot::resolve(Some(tmp1.path())).unwrap();
        let root2 = ProjectRoot::resolve(Some(tmp2.path())).unwrap();
        std::fs::write(tmp1.path().join("a.py"), b"def foo(): pass")?;
        std::fs::write(tmp2.path().join("a.py"), b"def bar(): pass")?;
        let idx1 = ProjectIndex::discover(&root1).unwrap();
        let idx2 = ProjectIndex::discover(&root2).unwrap();
        let si1 = StructuralIndex::new(&root1);
        let si2 = StructuralIndex::new(&root2);
        si1.build(&idx1).unwrap();
        si2.build(&idx2).unwrap();
        let syms1 = si1.find_definitions("foo").unwrap();
        let syms2 = si2.find_definitions("bar").unwrap();
        assert!(!syms1.is_empty());
        assert!(!syms2.is_empty());
        // Ensure not cross-contaminated
        let syms1_bar = si1.find_definitions("bar").unwrap();
        assert!(syms1_bar.is_empty());
        Ok(())
    }

    // R4A incremental graph tests
    #[test]
    fn incremental_body_change_only_outgoing() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        // a.py defines foo and caller_a that calls foo
        std::fs::write(
            tmp.path().join("a.py"),
            b"def foo():\n    pass\ndef caller_a():\n    foo()\n",
        )?;
        // b.py calls foo (unrelated caller)
        std::fs::write(tmp.path().join("b.py"), b"def bar():\n    foo()\n")?;
        // c.py unrelated
        std::fs::write(tmp.path().join("c.py"), b"def unrelated():\n    pass\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        let stats1 = si.build(&idx).unwrap();
        assert!(stats1.files_parsed >= 3);
        let conn = store::open_db(tmp.path())?;
        let edges_before = store::count_call_edges(&conn)?;
        // Change body of foo only (no rename)
        std::fs::write(
            tmp.path().join("a.py"),
            b"def foo():\n    x=1\n    y=2\ndef caller_a():\n    foo()\n",
        )?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let idx2 = ProjectIndex::discover(&root).unwrap();
        let stats2 = si.build(&idx2).unwrap();
        assert_eq!(stats2.files_parsed, 1);
        assert_eq!(stats2.files_skipped, 2);
        // Only outgoing edges for a.py should have been re-inserted; unrelated callers untouched
        // Since foo name unchanged, affected should be empty, so references_reresolved == 0
        assert_eq!(stats2.references_reresolved, 0);
        // edges_deleted should be 0 for affected (since no name change)
        assert_eq!(stats2.edges_deleted, 0);
        // Verify b.py still resolves foo
        let conn2 = store::open_db(tmp.path())?;
        let edges_after = store::count_call_edges(&conn2)?;
        assert_eq!(edges_before, edges_after);
        let callers = si.find_callers("foo")?;
        assert!(callers.iter().any(|e| e.file == "b.py"));
        Ok(())
    }

    #[test]
    fn incremental_rename() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        std::fs::write(tmp.path().join("a.py"), b"def foo():\n    pass\n")?;
        std::fs::write(tmp.path().join("b.py"), b"def bar():\n    foo()\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        let callers_before = si.find_callers("foo")?;
        assert!(callers_before
            .iter()
            .any(|e| e.confidence == CallConfidence::Resolved));
        // Rename foo -> bar_new
        std::fs::write(tmp.path().join("a.py"), b"def bar_new():\n    pass\n")?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let idx2 = ProjectIndex::discover(&root).unwrap();
        let stats2 = si.build(&idx2).unwrap();
        assert_eq!(stats2.files_parsed, 1);
        // foo removed, bar_new added → affected includes foo
        assert!(stats2.references_reresolved >= 1);
        // stale resolved foo edges should be removed: foo callers should now be unresolved
        let callers_foo = si.find_callers("foo")?;
        // After rename, b.py calls foo but foo no longer exists → should be unresolved or empty resolved
        let any_resolved = callers_foo
            .iter()
            .any(|e| e.confidence == CallConfidence::Resolved);
        assert!(!any_resolved, "stale resolved foo edges should be removed");
        // bar_new has no callers
        let defs_foo = si.find_definitions("foo")?;
        assert!(defs_foo.is_empty());
        let defs_bar = si.find_definitions("bar_new")?;
        assert!(!defs_bar.is_empty());
        Ok(())
    }

    #[test]
    fn incremental_add_missing_definition() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        // b.py calls Foo but Foo not defined
        std::fs::write(tmp.path().join("b.py"), b"def caller():\n    Foo()\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        let callers_before = si.find_callers("Foo")?;
        let unresolved_before = callers_before
            .iter()
            .any(|e| e.confidence == CallConfidence::Unresolved);
        assert!(unresolved_before || callers_before.is_empty());
        // Add Foo definition
        std::fs::write(tmp.path().join("a.py"), b"def Foo():\n    pass\n")?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let idx2 = ProjectIndex::discover(&root).unwrap();
        let stats2 = si.build(&idx2).unwrap();
        assert_eq!(stats2.files_parsed, 1);
        assert!(stats2.references_reresolved >= 1);
        let callers_after = si.find_callers("Foo")?;
        let resolved_after = callers_after.iter().any(|e| {
            e.confidence == CallConfidence::Resolved || e.confidence == CallConfidence::Probable
        });
        assert!(resolved_after, "after Foo added, caller should resolve");
        Ok(())
    }

    #[test]
    fn incremental_delete_definition() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        std::fs::write(tmp.path().join("a.py"), b"def Foo():\n    pass\n")?;
        std::fs::write(tmp.path().join("b.py"), b"def caller():\n    Foo()\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        let callers_before = si.find_callers("Foo")?;
        assert!(callers_before
            .iter()
            .any(|e| e.confidence == CallConfidence::Resolved));
        // Delete Foo
        std::fs::remove_file(tmp.path().join("a.py"))?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let idx2 = ProjectIndex::discover(&root).unwrap();
        let stats2 = si.build(&idx2).unwrap();
        assert_eq!(stats2.files_deleted, 1);
        assert!(stats2.references_reresolved >= 1);
        let callers_after = si.find_callers("Foo")?;
        let any_resolved = callers_after
            .iter()
            .any(|e| e.confidence == CallConfidence::Resolved);
        assert!(!any_resolved, "after delete, should be unresolved");
        Ok(())
    }

    #[test]
    fn incremental_unrelated_change_no_rewrite() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        std::fs::write(tmp.path().join("a.py"), b"def foo():\n    pass\n")?;
        std::fs::write(tmp.path().join("b.py"), b"def bar():\n    foo()\n")?;
        std::fs::write(tmp.path().join("c.py"), b"def unrelated():\n    pass\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        let conn = store::open_db(tmp.path())?;
        let edges_before = store::count_call_edges(&conn)?;
        // Change unrelated file c.py body only
        std::fs::write(tmp.path().join("c.py"), b"def unrelated():\n    x=42\n")?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let idx2 = ProjectIndex::discover(&root).unwrap();
        let stats2 = si.build(&idx2).unwrap();
        assert_eq!(stats2.files_parsed, 1);
        assert_eq!(stats2.references_reresolved, 0);
        assert_eq!(stats2.edges_deleted, 0);
        // Edges for foo/bar should be unchanged
        let conn2 = store::open_db(tmp.path())?;
        let edges_after = store::count_call_edges(&conn2)?;
        assert_eq!(edges_before, edges_after);
        let callers = si.find_callers("foo")?;
        assert!(callers
            .iter()
            .any(|e| e.file == "b.py" && e.confidence == CallConfidence::Resolved));
        Ok(())
    }

    #[test]
    fn single_file_update_api() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        std::fs::write(tmp.path().join("a.py"), b"def foo():\n    pass\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        // Use single-file API to modify
        std::fs::write(tmp.path().join("a.py"), b"def foo():\n    x=1\n")?;
        let stats = si.update_single_file("a.py")?;
        assert_eq!(stats.files_parsed, 1);
        // No-change via same API
        let stats2 = si.update_single_file("a.py")?;
        assert_eq!(stats2.files_skipped, 1);
        // Delete via API
        std::fs::remove_file(tmp.path().join("a.py"))?;
        let stats3 = si.update_single_file("a.py")?;
        assert_eq!(stats3.files_deleted, 1);
        Ok(())
    }

    #[test]
    fn generic_graph_fixtures_direct_qualified_multiple_callees() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        // Direct: foo -> bar
        std::fs::write(
            tmp.path().join("direct.py"),
            b"def bar():\n    pass\ndef foo():\n    bar()\n",
        )?;
        // Qualified: Foo.bar() style (Python class)
        std::fs::write(
            tmp.path().join("qualified.py"),
            b"class MyClass:\n    def my_method(self):\n        pass\ndef caller_q():\n    obj = MyClass()\n    obj.my_method()\n",
        )?;
        // TS qualified NestFactory.create style
        std::fs::write(
            tmp.path().join("ts_qualified.ts"),
            b"class NestFactory { static create(x:any){} }\nfunction caller_ts(){ NestFactory.create(null); }\n",
        )?;
        // Multiple callers: a,b -> target, c unrelated
        std::fs::write(tmp.path().join("target.py"), b"def target():\n    pass\n")?;
        std::fs::write(
            tmp.path().join("caller_a.py"),
            b"def caller_a():\n    target()\n",
        )?;
        std::fs::write(
            tmp.path().join("caller_b.py"),
            b"def caller_b():\n    target()\n",
        )?;
        std::fs::write(
            tmp.path().join("unrelated.py"),
            b"def unrelated():\n    pass\n",
        )?;
        // Callees: foo -> bar,baz
        std::fs::write(
            tmp.path().join("callee_src.py"),
            b"def my_caller():\n    bar()\n    baz()\n",
        )?;
        std::fs::write(tmp.path().join("bar.py"), b"def bar():\n    pass\n")?;
        std::fs::write(tmp.path().join("baz.py"), b"def baz():\n    pass\n")?;
        // Go direct
        std::fs::write(
            tmp.path().join("go_direct.go"),
            b"package main\nfunc bar_go(){}\nfunc foo_go(){ bar_go() }\n",
        )?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();

        // Direct: foo callers should include direct.py's foo? Actually bar callers should include foo
        let callers_bar = si.find_callers("bar")?;
        assert!(
            callers_bar.iter().any(|e| e.file == "direct.py"),
            "direct: bar callers should include direct.py"
        );
        // Short query bar should find
        let callers_bar_short = si.find_callers("bar")?;
        assert!(!callers_bar_short.is_empty());

        // Qualified: my_method via short and qualified
        let callers_short = si.find_callers("my_method")?;
        assert!(
            callers_short.iter().any(|e| e.file == "qualified.py"),
            "qualified short should find caller"
        );
        // TS qualified both forms
        let q_qual = si.find_callers("NestFactory.create")?;
        let q_short = si.find_callers("create")?;
        assert!(
            q_qual.iter().any(|e| e.file == "ts_qualified.ts"),
            "qualified query should find"
        );
        assert!(
            q_short.iter().any(|e| e.file == "ts_qualified.ts"),
            "short query should find"
        );
        // Multiple callers: target should have 2 callers (a,b)
        let callers_target = si.find_callers("target")?;
        let files_target: Vec<_> = callers_target.iter().map(|e| e.file.clone()).collect();
        assert!(
            files_target.iter().any(|f| f == "caller_a.py"),
            "multiple callers a"
        );
        assert!(
            files_target.iter().any(|f| f == "caller_b.py"),
            "multiple callers b"
        );
        assert!(
            !files_target.iter().any(|f| f == "unrelated.py"),
            "unrelated should not be caller"
        );
        // Callees: my_caller -> bar,baz
        let callees = si.find_callees("my_caller")?;
        let callee_names: Vec<_> = callees.iter().map(|e| e.callee_name.clone()).collect();
        assert!(
            callee_names.contains(&"bar".to_string()),
            "callees should include bar"
        );
        assert!(
            callee_names.contains(&"baz".to_string()),
            "callees should include baz"
        );
        // Both: check relations via direct store
        let both_callers = si.find_callers("target")?;
        let both_callees = si.find_callees("my_caller")?;
        assert!(!both_callers.is_empty());
        assert!(!both_callees.is_empty());
        // Dedup: same callsite short+qualified should not duplicate final caller list for that site beyond one per query
        // For ts_qualified, qualified query should not have duplicate same file/line
        let mut seen = std::collections::HashSet::new();
        for e in &q_qual {
            let key = format!("{}:{}", e.file, e.line);
            assert!(seen.insert(key.clone()), "qualified dedup failed: {}", key);
        }
        let mut seen2 = std::collections::HashSet::new();
        for e in &q_short {
            // Short query for create will include many, but for ts_qualified site, ensure not duplicated
            let key = format!("{}:{}:{}", e.file, e.line, e.callee_name);
            assert!(seen2.insert(key.clone()), "short dedup failed: {}", key);
        }
        // Go
        let callers_go = si.find_callers("bar_go")?;
        assert!(
            callers_go.iter().any(|e| e.file == "go_direct.go"),
            "go caller"
        );
        Ok(())
    }

    #[test]
    fn determinism_20x() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        std::fs::write(tmp.path().join("a.py"), b"def foo():\n    pass\n")?;
        std::fs::write(tmp.path().join("b.py"), b"def bar():\n    foo()\n")?;
        std::fs::write(tmp.path().join("c.py"), b"def baz():\n    foo()\n")?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        let first = si.find_callers("foo")?;
        let first_str = format!("{:?}", first);
        for _ in 0..20 {
            let nxt = si.find_callers("foo")?;
            assert_eq!(format!("{:?}", nxt), first_str, "20x determinism");
        }
        Ok(())
    }

    #[test]
    fn extraction_version_upgrade_forces_reparse() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        // Create file with qualified call Factory.create()
        std::fs::write(
            tmp.path().join("a.py"),
            b"class Factory:\n    @staticmethod\n    def create():\n        pass\ndef caller():\n    Factory.create()\n",
        )?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        let out1 = si.build(&idx).unwrap();
        assert!(out1.files_parsed >= 1);
        // Verify current extraction version stored and qualified ref exists
        let conn = store::open_db(tmp.path())?;
        let ver: Option<String> = store::get_meta(&conn, EXTRACTION_VERSION_KEY)?;
        let cur = STRUCTURAL_EXTRACTION_VERSION.to_string();
        assert_eq!(ver.as_deref(), Some(cur.as_str()));
        let refs: Vec<String> = conn
            .prepare("SELECT name FROM refs WHERE file='a.py'")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert!(
            refs.contains(&"Factory.create".to_string()),
            "new parser should store qualified, got {:?}",
            refs
        );
        assert!(refs.contains(&"create".to_string()));
        // Simulate old index: delete qualified refs and downgrade version to 1
        conn.execute("DELETE FROM refs WHERE name='Factory.create'", [])?;
        store::set_meta(&conn, EXTRACTION_VERSION_KEY, "1")?;
        // Also delete call_edges for qualified to simulate old
        conn.execute(
            "DELETE FROM call_edges WHERE callee_name='Factory.create'",
            [],
        )?;
        // Verify qualified gone, short remains
        let remaining: Vec<String> = conn
            .prepare("SELECT name FROM refs WHERE file='a.py'")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert!(!remaining.contains(&"Factory.create".to_string()));
        assert!(remaining.contains(&"create".to_string()));
        // Reconcile with unchanged source hash — should force reparse due to version mismatch
        let idx2 = ProjectIndex::discover(&root).unwrap();
        let out2 = si.build(&idx2).unwrap();
        // Should have reparsed despite same hash
        assert_eq!(
            out2.files_skipped, 0,
            "upgrade should not skip, files_skipped=0"
        );
        assert!(out2.files_parsed >= 1, "upgrade should reparse");
        // Verify qualified restored
        let conn2 = store::open_db(tmp.path())?;
        let refs2: Vec<String> = conn2
            .prepare("SELECT name FROM refs WHERE file='a.py'")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert!(
            refs2.contains(&"Factory.create".to_string()),
            "after upgrade qualified should exist, got {:?}",
            refs2
        );
        let ver2: Option<String> = store::get_meta(&conn2, EXTRACTION_VERSION_KEY)?;
        let cur2 = STRUCTURAL_EXTRACTION_VERSION.to_string();
        assert_eq!(ver2.as_deref(), Some(cur2.as_str()));
        // Second reconcile with same hash and current version should skip
        let idx3 = ProjectIndex::discover(&root).unwrap();
        let out3 = si.build(&idx3).unwrap();
        assert_eq!(out3.files_skipped, 1);
        assert_eq!(out3.files_parsed, 0);
        Ok(())
    }

    #[test]
    fn qualified_name_collision_isolation() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        std::fs::write(
            tmp.path().join("foo_def.py"),
            b"class Foo:\n    @staticmethod\n    def create():\n        pass\n",
        )?;
        std::fs::write(
            tmp.path().join("bar_def.py"),
            b"class Bar:\n    @staticmethod\n    def create():\n        pass\n",
        )?;
        std::fs::write(
            tmp.path().join("caller_foo.py"),
            b"def callerFoo():\n    Foo.create()\n",
        )?;
        std::fs::write(
            tmp.path().join("caller_bar.py"),
            b"def callerBar():\n    Bar.create()\n",
        )?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        let callers_foo = si.find_callers("Foo.create")?;
        let callers_bar = si.find_callers("Bar.create")?;
        let callers_short = si.find_callers("create")?;
        assert!(
            callers_foo.iter().any(|e| e.file == "caller_foo.py"),
            "Foo.create should find callerFoo"
        );
        assert!(
            !callers_foo.iter().any(|e| e.file == "caller_bar.py"),
            "Foo.create must NOT find callerBar, got {:?}",
            callers_foo
        );
        assert!(
            callers_bar.iter().any(|e| e.file == "caller_bar.py"),
            "Bar.create should find callerBar"
        );
        assert!(
            !callers_bar.iter().any(|e| e.file == "caller_foo.py"),
            "Bar.create must NOT find callerFoo"
        );
        // Short query may return both (intentionally ambiguous)
        assert!(
            callers_short.iter().any(|e| e.file == "caller_foo.py"),
            "short create should find foo"
        );
        assert!(
            callers_short.iter().any(|e| e.file == "caller_bar.py"),
            "short create should find bar"
        );
        Ok(())
    }

    #[test]
    fn same_line_callee_both_survive() -> Result<()> {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        // Python: two calls on same line
        std::fs::write(
            tmp.path().join("callee_defs.py"),
            b"def foo():\n    pass\ndef bar():\n    pass\n",
        )?;
        std::fs::write(
            tmp.path().join("target.py"),
            b"def target():\n    foo(); bar()\n",
        )?;
        // Also Go: same line
        std::fs::write(
            tmp.path().join("go_target.go"),
            b"package main\nfunc foo_go(){}\nfunc bar_go(){}\nfunc target_go(){ foo_go(); bar_go() }\n",
        )?;
        let idx = ProjectIndex::discover(&root).unwrap();
        let si = StructuralIndex::new(&root);
        si.build(&idx).unwrap();
        let callees = si.find_callees("target")?;
        let callee_names: Vec<_> = callees.iter().map(|e| e.callee_name.clone()).collect();
        assert!(
            callee_names.contains(&"foo".to_string()),
            "same-line foo should survive, got {:?}",
            callee_names
        );
        assert!(
            callee_names.contains(&"bar".to_string()),
            "same-line bar should survive, got {:?}",
            callee_names
        );
        assert!(
            callees.len() >= 2,
            "distinct callees >=2, got {}",
            callees.len()
        );
        // Go same-line
        let callees_go = si.find_callees("target_go")?;
        let callee_go_names: Vec<_> = callees_go.iter().map(|e| e.callee_name.clone()).collect();
        assert!(callee_go_names.contains(&"foo_go".to_string()), "go foo_go");
        assert!(callee_go_names.contains(&"bar_go".to_string()), "go bar_go");
        Ok(())
    }
}
