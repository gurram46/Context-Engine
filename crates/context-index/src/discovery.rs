use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use context_core::ContextError;
use ignore::WalkBuilder;

use crate::classification::{classify_file, FileKind};
use crate::hash::hash_file;
use crate::project_root::ProjectRoot;

/// Engine-internal excludes — never indexed/searched, generic.
/// These are build/cache dirs, not user source.
const ENGINE_INTERNAL_EXCLUDES: &[&str] = &[
    ".git",
    ".context",
    ".opencode/index",
    "node_modules",
    "dist",
    "build",
    "target",
    "__pycache__",
    ".pytest_cache",
    ".next",
    ".nuxt",
    "coverage",
];

/// Returns true if this relative path should be excluded as engine-internal.
/// `crates/` is only excluded when the *target* is the engine repo itself (frozen eval).
fn is_engine_internal(rel: &str, engine_root: &Path) -> bool {
    let lower = rel.to_lowercase();
    for pat in ENGINE_INTERNAL_EXCLUDES {
        if lower == *pat
            || lower.starts_with(&format!("{}/", pat))
            || lower.contains(&format!("/{}/", pat))
        {
            return true;
        }
    }
    // R3: Keep crates/ indexed for Rust structural intelligence. Previously excluded for frozen eval,
    // but structural needs Rust symbols (retrieve_context, CandidateProvider, etc.).
    // Exact search still excludes crates via rg args, so frozen eval remains stable.
    let _ = engine_root;
    false
}

/// File metadata for R1.
#[derive(Debug, Clone)]
pub struct FileRecord {
    /// Relative to project root, posix style.
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub kind: FileKind,
    pub size_bytes: u64,
    pub modified_time: Option<SystemTime>,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    pub discovered: usize,
    pub source: usize,
    pub test: usize,
    pub doc: usize,
    pub config: usize,
    pub build: usize,
    pub generated: usize,
    pub unknown: usize,
    pub skipped_generated: usize,
    pub hash_errors: usize,
    pub total_bytes: u64,
}

/// Minimal project index for R1.
#[derive(Debug, Clone)]
pub struct ProjectIndex {
    pub root: PathBuf,
    pub files: Vec<FileRecord>,
    pub stats: ScanStats,
}

/// Result of refreshing a project index for a set of dirty paths.
#[derive(Debug, Clone)]
pub struct ProjectIndexDelta {
    pub project: ProjectIndex,
    pub changed_files: Vec<String>,
    pub deleted_files: Vec<String>,
}

impl ProjectIndex {
    /// Discover, classify, hash (streaming, 10 MB limit).
    pub fn discover(root: &ProjectRoot) -> Result<Self, ContextError> {
        Self::discover_with_options(root, true)
    }

    /// Discover with optional hashing (for tests that want to skip hashing).
    pub fn discover_with_options(root: &ProjectRoot, do_hash: bool) -> Result<Self, ContextError> {
        let root_path = root.path();
        let mut files = Vec::new();
        let mut stats = ScanStats::default();

        // Use `ignore` WalkBuilder which respects .gitignore, .ignore, and hidden.
        // Add overrides for ENGINE_INTERNAL_EXCLUDES and .opencodeignore.
        let mut builder = WalkBuilder::new(root_path);
        builder
            .hidden(false) // don't hide . files by default; let gitignore handle
            .git_ignore(true)
            .git_global(false)
            .git_exclude(false)
            .parents(true)
            .ignore(true) // .ignore
            .require_git(true) // only if git repo
            .follow_links(false);

        // Add custom ignore for .opencodeignore if present
        let opencode_ignore = root_path.join(".opencodeignore");
        if opencode_ignore.exists() {
            builder.add_ignore(opencode_ignore);
        }

        // Walk
        for entry in builder.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "walk entry error");
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            // Relative posix
            let rel = path
                .strip_prefix(root_path)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().to_string());

            if let Some(record) = make_record(root_path, &rel, do_hash, &mut stats) {
                files.push(record);
            }
        }

        stats.discovered = files.len();
        // Sort for determinism
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        Ok(Self {
            root: root_path.to_path_buf(),
            files,
            stats,
        })
    }

    /// Filename lookup (exact basename, <10ms warm).
    pub fn find_by_filename(&self, name: &str) -> Vec<&FileRecord> {
        let lower = name.to_lowercase();
        self.files
            .iter()
            .filter(|f| {
                let base = Path::new(&f.relative_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_lowercase())
                    .unwrap_or_default();
                base == lower || f.relative_path.to_lowercase() == lower
            })
            .collect()
    }

    /// Path lookup (exact relative path).
    pub fn find_by_path(&self, rel: &str) -> Option<&FileRecord> {
        let norm = rel
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_lowercase();
        self.files
            .iter()
            .find(|f| f.relative_path.to_lowercase() == norm)
    }

    /// Refresh the index for a set of dirty relative paths without a full walk.
    pub fn refresh_paths(
        &self,
        paths: &BTreeSet<String>,
    ) -> Result<ProjectIndexDelta, ContextError> {
        let mut files = self.files.clone();
        let mut changed = Vec::new();
        let mut deleted = Vec::new();

        for rel in paths {
            let norm = rel.replace('\\', "/").trim_start_matches("./").to_string();
            let old = files.iter().find(|f| f.relative_path == norm).cloned();
            if let Some(pos) = files.iter().position(|f| f.relative_path == norm) {
                files.remove(pos);
            }
            let mut scratch = ScanStats::default();
            if let Some(new) = make_record(&self.root, &norm, true, &mut scratch) {
                let is_changed = match &old {
                    Some(o) => {
                        o.kind != new.kind
                            || o.size_bytes != new.size_bytes
                            || o.modified_time != new.modified_time
                            || o.content_hash != new.content_hash
                    }
                    None => true,
                };
                if is_changed {
                    changed.push(norm.clone());
                }
                files.push(new);
            } else if old.is_some() {
                deleted.push(norm.clone());
            }
        }

        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        changed.sort();
        deleted.sort();

        let stats = recompute_stats(&files);

        Ok(ProjectIndexDelta {
            project: ProjectIndex {
                root: self.root.clone(),
                files,
                stats,
            },
            changed_files: changed,
            deleted_files: deleted,
        })
    }
}

/// Build a single `FileRecord` from a relative posix path.
/// Updates `stats` to reflect the record, mirroring the logic in full discovery.
fn make_record(
    root_path: &Path,
    rel: &str,
    do_hash: bool,
    stats: &mut ScanStats,
) -> Option<FileRecord> {
    let path = root_path.join(rel);

    // Engine-internal check
    if is_engine_internal(rel, root_path) {
        stats.skipped_generated += 1;
        return None;
    }

    let kind = classify_file(Path::new(rel));
    // Skip generated for discovery? Keep but mark.
    if kind == FileKind::Generated {
        stats.skipped_generated += 1;
        // Still record? For R1, we skip generated from filesVec to keep memory small.
        return None;
    }

    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return None,
    };
    let size = meta.len();
    stats.total_bytes += size;
    match kind {
        FileKind::Source => stats.source += 1,
        FileKind::Test => stats.test += 1,
        FileKind::Doc => stats.doc += 1,
        FileKind::Config => stats.config += 1,
        FileKind::Build => stats.build += 1,
        FileKind::Generated => stats.generated += 1,
        FileKind::Unknown => stats.unknown += 1,
    }

    // Binary safety: only hash supported kinds and size < 10 MB, and not binary
    let hash = if do_hash && is_text_searchable(rel, kind) && size <= 10 * 1024 * 1024 {
        match hash_file(&path) {
            Ok(h) => Some(h),
            Err(e) => {
                stats.hash_errors += 1;
                tracing::debug!(file = %rel, error = %e, "hash skipped");
                None
            }
        }
    } else {
        None
    };

    let modified = meta.modified().ok();

    Some(FileRecord {
        relative_path: rel.to_string(),
        absolute_path: path,
        kind,
        size_bytes: size,
        modified_time: modified,
        content_hash: hash,
    })
}

/// Recompute aggregate scan stats from the final file vector.
fn recompute_stats(files: &[FileRecord]) -> ScanStats {
    let mut stats = ScanStats::default();
    stats.discovered = files.len();
    for f in files {
        stats.total_bytes += f.size_bytes;
        match f.kind {
            FileKind::Source => stats.source += 1,
            FileKind::Test => stats.test += 1,
            FileKind::Doc => stats.doc += 1,
            FileKind::Config => stats.config += 1,
            FileKind::Build => stats.build += 1,
            FileKind::Generated => stats.generated += 1,
            FileKind::Unknown => stats.unknown += 1,
        }
        if f.content_hash.is_none()
            && is_text_searchable(&f.relative_path, f.kind)
            && f.size_bytes <= 10 * 1024 * 1024
        {
            stats.hash_errors += 1;
        }
    }
    stats
}

/// Whether file is text-searchable for exact search.
/// Based on extension and kind, not content sniffing.
fn is_text_searchable(rel: &str, kind: FileKind) -> bool {
    if matches!(kind, FileKind::Unknown) {
        // Only allow if extension is known text
        let ext = Path::new(rel)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        return matches!(
            ext.as_str(),
            "py" | "ts"
                | "js"
                | "tsx"
                | "jsx"
                | "go"
                | "rs"
                | "java"
                | "kt"
                | "c"
                | "cc"
                | "cpp"
                | "h"
                | "hpp"
                | "cs"
                | "rb"
                | "php"
                | "swift"
                | "sql"
                | "sh"
                | "ps1"
                | "json"
                | "yaml"
                | "yml"
                | "toml"
                | "ini"
                | "xml"
                | "proto"
                | "md"
                | "txt"
                | "rst"
        );
    }
    matches!(
        kind,
        FileKind::Source | FileKind::Test | FileKind::Doc | FileKind::Config | FileKind::Build
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_root::ProjectRoot;
    use std::collections::BTreeSet;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discover_temp() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        fs::write(tmp.path().join("b.md"), b"# doc").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        assert!(idx.files.iter().any(|f| f.relative_path == "a.py"));
        assert!(idx.files.iter().any(|f| f.relative_path == "b.md"));
    }

    #[test]
    fn ignore_target() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::create_dir_all(tmp.path().join("target/debug")).unwrap();
        fs::write(tmp.path().join("target/debug/foo"), b"bin").unwrap();
        fs::write(tmp.path().join("a.rs"), b"fn main(){}").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        assert!(idx.files.iter().any(|f| f.relative_path == "a.rs"));
        assert!(!idx
            .files
            .iter()
            .any(|f| f.relative_path.contains("target/")));
    }

    #[test]
    fn filename_lookup() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("go.mod"), b"module foo").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let found = idx.find_by_filename("go.mod");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].relative_path, "go.mod");
    }

    #[test]
    fn refresh_paths_create() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        fs::write(tmp.path().join("b.py"), b"world").unwrap();
        let paths: BTreeSet<String> = ["b.py".to_string()].into_iter().collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        assert!(delta
            .project
            .files
            .iter()
            .any(|f| f.relative_path == "b.py"));
        assert_eq!(delta.changed_files, vec!["b.py"]);
        assert!(delta.deleted_files.is_empty());
        assert!(delta
            .project
            .files
            .iter()
            .any(|f| f.relative_path == "a.py"));
    }

    #[test]
    fn refresh_paths_modify() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let old_hash = idx
            .files
            .iter()
            .find(|f| f.relative_path == "a.py")
            .unwrap()
            .content_hash
            .clone();
        fs::write(tmp.path().join("a.py"), b"hello world").unwrap();
        let paths: BTreeSet<String> = ["a.py".to_string()].into_iter().collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        let new_hash = delta
            .project
            .files
            .iter()
            .find(|f| f.relative_path == "a.py")
            .unwrap()
            .content_hash
            .clone();
        assert_ne!(old_hash, new_hash);
        assert_eq!(delta.changed_files, vec!["a.py"]);
        assert!(delta.deleted_files.is_empty());
    }

    #[test]
    fn refresh_paths_delete() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        fs::write(tmp.path().join("b.py"), b"world").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        fs::remove_file(tmp.path().join("a.py")).unwrap();
        let paths: BTreeSet<String> = ["a.py".to_string()].into_iter().collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        assert!(!delta
            .project
            .files
            .iter()
            .any(|f| f.relative_path == "a.py"));
        assert!(delta.changed_files.is_empty());
        assert_eq!(delta.deleted_files, vec!["a.py"]);
        assert!(delta
            .project
            .files
            .iter()
            .any(|f| f.relative_path == "b.py"));
    }

    #[test]
    fn refresh_paths_unchanged() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let paths: BTreeSet<String> = ["a.py".to_string()].into_iter().collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        assert!(delta.changed_files.is_empty());
        assert!(delta.deleted_files.is_empty());
    }

    #[test]
    fn refresh_paths_unrelated_stable() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        fs::write(tmp.path().join("b.py"), b"world").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let old_b = idx
            .files
            .iter()
            .find(|f| f.relative_path == "b.py")
            .unwrap()
            .clone();
        fs::write(tmp.path().join("a.py"), b"hello world").unwrap();
        let paths: BTreeSet<String> = ["a.py".to_string()].into_iter().collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        let new_b = delta
            .project
            .files
            .iter()
            .find(|f| f.relative_path == "b.py")
            .unwrap();
        assert_eq!(old_b.content_hash, new_b.content_hash);
        assert_eq!(old_b.size_bytes, new_b.size_bytes);
        assert_eq!(old_b.modified_time, new_b.modified_time);
        assert_eq!(old_b.kind, new_b.kind);
    }

    #[test]
    fn refresh_paths_sorted() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("z.py"), b"z").unwrap();
        fs::write(tmp.path().join("a.py"), b"a").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        fs::remove_file(tmp.path().join("z.py")).unwrap();
        fs::write(tmp.path().join("m.py"), b"m").unwrap();
        let paths: BTreeSet<String> = ["z.py".to_string(), "m.py".to_string()]
            .into_iter()
            .collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        assert_eq!(delta.changed_files, vec!["m.py"]);
        assert_eq!(delta.deleted_files, vec!["z.py"]);
    }

    #[test]
    fn refresh_paths_excludes_generated_internal() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        fs::create_dir_all(tmp.path().join("target/debug")).unwrap();
        fs::write(tmp.path().join("target/debug/foo"), b"bin").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        // New generated file must not appear.
        fs::write(tmp.path().join("target/debug/bar"), b"bin2").unwrap();
        let paths: BTreeSet<String> = ["target/debug/bar".to_string()].into_iter().collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        assert!(!delta
            .project
            .files
            .iter()
            .any(|f| f.relative_path == "target/debug/bar"));
        assert!(delta.changed_files.is_empty());
        assert!(delta.deleted_files.is_empty());
        // Unrelated existing excludes remain absent after a separate refresh.
        fs::write(tmp.path().join("a.py"), b"hello world").unwrap();
        let paths2: BTreeSet<String> = ["a.py".to_string()].into_iter().collect();
        let delta2 = idx.refresh_paths(&paths2).unwrap();
        assert!(!delta2
            .project
            .files
            .iter()
            .any(|f| f.relative_path.contains("target/")));
    }

    #[test]
    fn refresh_paths_recomputes_stats() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.py"), b"hello").unwrap();
        fs::write(tmp.path().join("b.md"), b"# doc").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        fs::remove_file(tmp.path().join("b.md")).unwrap();
        fs::write(tmp.path().join("c.rs"), b"fn main() {}").unwrap();
        let paths: BTreeSet<String> = ["b.md".to_string(), "c.rs".to_string()]
            .into_iter()
            .collect();
        let delta = idx.refresh_paths(&paths).unwrap();
        assert_eq!(delta.project.stats.discovered, 2);
        assert_eq!(delta.project.stats.source, 2);
        assert_eq!(delta.project.stats.doc, 0);
        assert_eq!(delta.project.stats.total_bytes, 5 + 12);
    }
}
