use std::sync::Arc;
use tokio::sync::RwLock;

use context_index::{ProjectIndex, ProjectRoot};
use tracing::info;

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
    pub async fn ensure(&self) -> Result<ProjectIndex, context_core::ContextError> {
        // Fast path: if index exists and root hasn't changed, return clone.
        // For R1, we clone the whole index (cheap, Vec<FileRecord> ~ few MB).
        let current_root = ProjectRoot::resolve(None)?;
        {
            let guard = self.root.read().await;
            if let Some(cached_root) = guard.as_ref() {
                if cached_root.path() == current_root.path() {
                    if let Some(idx) = self.index.read().await.clone() {
                        return Ok(idx);
                    }
                }
            }
        }
        // Need rebuild
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
