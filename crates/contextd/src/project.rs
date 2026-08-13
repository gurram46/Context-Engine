#![allow(dead_code)]
use std::sync::Arc;
use tokio::sync::RwLock;

use context_index::structural::StructuralIndex;
use context_index::{ProjectIndex, ProjectRoot};
use tracing::{info, warn};

/// Cached project index for R1.
/// Owns discovery, classification, hashing, and exact search.
/// Semantic/symbol/graph remain in V2.
pub struct ProjectCache {
    root: Arc<RwLock<Option<ProjectRoot>>>,
    index: Arc<RwLock<Option<ProjectIndex>>>,
}

impl ProjectCache {
    pub fn new() -> Self {
        Self {
            root: Arc::new(RwLock::new(None)),
            index: Arc::new(RwLock::new(None)),
        }
    }

    /// Resolve current project root (env or cwd) and ensure index is built.
    /// R3: always rediscover to detect new/deleted/changed files for structural index.
    /// Structural build uses hash-based skip, so second build is cheap.
    pub async fn ensure(&self) -> Result<ProjectIndex, context_core::ContextError> {
        let current_root = ProjectRoot::resolve(None)?;
        // Need rebuild — always rediscover for structural freshness (R3).
        let t0 = std::time::Instant::now();
        let idx = ProjectIndex::discover(&current_root)?;
        let elapsed = t0.elapsed();
        info!(
            root = %current_root,
            files = %idx.stats.discovered,
            source = %idx.stats.source,
            test = %idx.stats.test,
            elapsed_ms = %elapsed.as_millis(),
            "project index built"
        );
        // Build structural index incrementally (hash-based skip) — blocking, so spawn.
        let root_clone = current_root.path().to_path_buf();
        let idx_clone = idx.clone();
        let structural_res = tokio::task::spawn_blocking(move || {
            let si = StructuralIndex::for_path(root_clone);
            si.build(&idx_clone)
        })
        .await;
        match structural_res {
            Ok(Ok(stats)) => {
                info!(
                    parsed = stats.files_parsed,
                    skipped = stats.files_skipped,
                    deleted = stats.files_deleted,
                    symbols = stats.symbols,
                    elapsed_ms = stats.elapsed_ms,
                    "structural index built"
                );
            }
            Ok(Err(e)) => {
                warn!(error=%e, "structural index build failed, continuing with exact only");
            }
            Err(e) => {
                warn!(error=%e, "structural index join failed");
            }
        }
        {
            let mut r = self.root.write().await;
            *r = Some(current_root);
        }
        {
            let mut i = self.index.write().await;
            *i = Some(idx.clone());
        }
        Ok(idx)
    }

    /// For tests: get current index if any.
    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn get(&self) -> Option<ProjectIndex> {
        self.index.read().await.clone()
    }
}

impl Default for ProjectCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to check if a query is unambiguously EXACT (filename, quoted, path).
#[allow(dead_code)]
pub fn is_unambiguous_exact(query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return false;
    }
    // Quoted literal: "foo" or 'foo'
    if (q.starts_with('"') && q.ends_with('"') && q.len() >= 2)
        || (q.starts_with('\'') && q.ends_with('\'') && q.len() >= 2)
    {
        return true;
    }
    // Path-like: contains / and .
    if q.contains('/') && q.contains('.') {
        return true;
    }
    // Single token filename with extension
    if q.split_whitespace().count() == 1 && q.contains('.') && !q.contains(' ') {
        // e.g., go.mod, Cargo.toml, package.json, backend/cmd/api/main.go
        let lower = q.to_lowercase();
        if matches!(
            lower.as_str(),
            "go.mod"
                | "go.sum"
                | "cargo.toml"
                | "cargo.lock"
                | "package.json"
                | "dockerfile"
                | "makefile"
        ) {
            return true;
        }
        if lower.contains('.') && lower.len() < 60 {
            // If it looks like a path with extension, treat as EXACT
            if lower.contains('/') || lower.contains('.') {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_filename() {
        assert!(is_unambiguous_exact("go.mod"));
        assert!(is_unambiguous_exact("Cargo.toml"));
        assert!(is_unambiguous_exact("\"hello world\""));
        assert!(is_unambiguous_exact("backend/cmd/api/main.go"));
        assert!(!is_unambiguous_exact("count_tokens"));
        assert!(!is_unambiguous_exact("Where is secret redaction?"));
    }
}
