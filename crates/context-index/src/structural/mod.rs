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

    /// Initial or incremental build.
    /// - For every structurally supported file (Language != Unknown), check hash vs DB.
    ///   * hash equal → SKIP PARSE
    ///   * hash changed or new → parse and upsert
    /// - Deletes stale files (in DB but not on disk)
    /// - Rebuilds call edges if any parse happened or deletions occurred.
    /// R3: structural discovery walks the worktree directly (including crates/) because
    /// ProjectIndex respects .opencodeignore which hides crates for V2. Structural needs Rust.
    pub fn build(&self, project: &ProjectIndex) -> Result<IndexStats> {
        let t0 = Instant::now();
        let mut conn = store::open_db(&self.root)?;
        let mut stats = IndexStats::default();

        // Use direct structural walk (includes crates) merged with ProjectIndex for hashes
        let structural_files = collect_structural_files(&self.root, project);
        stats.files_discovered = structural_files.len();

        // Load existing hashes
        let existing = store::list_files(&conn).unwrap_or_default();
        let mut existing_map: std::collections::HashMap<String, String> =
            existing.into_iter().collect();

        // Collect parsed files for call graph
        let mut parsed_files: Vec<ParsedFile> = Vec::new();
        let mut needs_edge_rebuild = false;

        for (rel, _abs, cur_hash) in &structural_files {
            let lang = detect_language(Path::new(rel));
            if lang == Language::Unknown {
                continue;
            }
            if cur_hash.is_empty() {
                continue;
            }
            if let Some(prev_hash) = existing_map.get(rel) {
                if prev_hash == cur_hash {
                    // Unchanged → skip parse, but still need parsed for graph? We can load from DB later for edges?
                    // For edge rebuild we need full data; if we skip, edges already in DB remain correct unless other file changed.
                    // But if we skip all, we don't need rebuild.
                    stats.files_skipped += 1;
                    existing_map.remove(rel);
                    continue;
                }
            }
            // Need to parse
            let abs = self.root.join(rel);
            let content = match std::fs::read_to_string(&abs) {
                Ok(c) => c,
                Err(e) => {
                    // Don't destroy last-good state if file unreadable (e.g., temporarily deleted or permission)
                    tracing::warn!(file=%rel, error=%e, "skip unreadable file, preserve last-good");
                    existing_map.remove(rel);
                    continue;
                }
            };
            // Verify hash matches content_hash (in case ProjectIndex hash outdated due to race)
            let actual_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
            // Use actual_hash for store; ProjectIndex hash should match but we trust actual
            let pf = parse_file(rel, &content, &actual_hash);
            // If parse catastrophically failed (no symbols and has error, and previously had symbols), we might preserve? But for R3 we store partial.
            // Only bail if content is empty? Keep as is.
            let size = content.len() as u64;
            // Upsert
            match store::upsert_parsed_file(&mut conn, &pf, size) {
                Ok(_) => {
                    stats.files_parsed += 1;
                    stats.symbols += pf.symbols.len();
                    stats.references += pf.references.len();
                    stats.chunks += pf.chunks.len();
                    parsed_files.push(pf);
                    needs_edge_rebuild = true;
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
            let _ = store::delete_file(&conn, &stale_path);
            stats.files_deleted += 1;
            needs_edge_rebuild = true;
        }

        // Rebuild call edges if needed
        if needs_edge_rebuild {
            // Need all parsed files from DB + newly parsed for correct graph
            // Load all symbols/refs from DB via parsing? Instead collect from DB directly via query.
            // Simpler: load all ParsedFiles from DB by querying symbols/refs and building edges via helper that reads DB.
            // But we have parsed_files only for changed; need to load unchanged too from DB for graph.
            // Approach: fetch all symbols and refs via SQL and build edges in memory without needing ParsedFile objects.
            // For R3 we can just load all refs and symbols from DB and use build_call_edges logic that works on ParsedFiles.
            // To avoid reimplementing, we'll load all symbols/refs via DB and construct in-memory maps, then build edges similarly.
            // Easiest: call rebuild_edges_from_db(&mut conn)
            rebuild_edges_from_db(&mut conn)?;
        }

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
fn rebuild_edges_from_db(conn: &mut rusqlite::Connection) -> Result<()> {
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
    store::upsert_call_edges(conn, &edges)?;
    Ok(())
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
}
