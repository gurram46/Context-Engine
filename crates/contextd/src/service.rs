#![allow(dead_code)]
use std::path::PathBuf;
use std::sync::Arc;

use context_core::ContextError;
use context_index::embed::Embedder;
use context_index::structural::store as structural_store;
use context_index::{ProjectIndex, ProjectRoot};

use crate::pipeline::{retrieve_context, ContextResult, Providers};
use crate::project::ProjectCache;

/// Options for search-like calls.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub budget_tokens: usize,
    pub max_results: usize,
    pub debug: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            budget_tokens: 10000,
            max_results: 10,
            debug: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Callers,
    Callees,
    Both,
}

impl Direction {
    #[allow(clippy::should_implement_trait)]
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "callees" => Self::Callees,
            "both" => Self::Both,
            _ => Self::Callers,
        }
    }
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Callers => "callers",
            Self::Callees => "callees",
            Self::Both => "both",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ReconcileStats {
    pub discovered: usize,
    pub changed_files: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub version: String,
    #[serde(rename = "contextdVersion")]
    pub contextd_version: String,
    pub rust_version: String,
    pub pid: u32,
    pub project_root: String,
    pub git_branch: Option<String>,
    pub index_generation: Option<u64>,
    pub files_indexed: usize,
    pub symbols: usize,
    pub bm25_documents: usize,
    pub vector_count: usize,
    pub embedding_model: String,
    pub embedding_runtime: String,
    pub semantic_available: bool,
    pub watcher_state: String,
    pub store_schema_version: Option<u32>,
}

/// Native service — single core for CLI and MCP.
pub struct ContextService {
    cache: Arc<ProjectCache>,
    root: PathBuf,
    #[allow(dead_code)]
    explicit_root: Option<PathBuf>,
    build_lock: Arc<tokio::sync::Mutex<()>>,
}

impl ContextService {
    /// Create service for given root (or auto-resolve via ProjectRoot::resolve).
    pub async fn new(root: Option<PathBuf>) -> Result<Self, ContextError> {
        let root_path = if let Some(p) = root.clone() {
            p.canonicalize().unwrap_or(p)
        } else {
            ProjectRoot::resolve(None)
                .map(|r| r.path().to_path_buf())
                .map_err(|e| ContextError::InvalidRoot(e.to_string()))?
        };
        Ok(Self {
            cache: Arc::new(ProjectCache::new()),
            root: root_path,
            explicit_root: root,
            build_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// For tests: create with explicit cache.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn with_cache(cache: Arc<ProjectCache>, root: PathBuf) -> Self {
        Self {
            cache,
            root: root.clone(),
            explicit_root: Some(root),
            build_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Cheap reconcile — discovery + incremental structural/BM25.
    /// Target <100ms for no-change. Serialized to avoid concurrent SQLite writes.
    pub async fn reconcile(&self) -> Result<ReconcileStats, ContextError> {
        let _guard = self.build_lock.lock().await;
        let t0 = std::time::Instant::now();
        let root = ProjectRoot::resolve(Some(&self.root))
            .map_err(|e| ContextError::InvalidRoot(e.to_string()))?;
        let idx =
            ProjectIndex::discover(&root).map_err(|e| ContextError::Internal(e.to_string()))?;
        // Incremental structural build (hash skip) — spawn_blocking
        let root_path = root.path().to_path_buf();
        let idx_clone = idx.clone();
        tokio::task::spawn_blocking(move || {
            let si = context_index::structural::StructuralIndex::for_path(root_path);
            si.build(&idx_clone)
        })
        .await
        .map_err(|e| ContextError::Internal(format!("structural build panicked: {e}")))?
        .map_err(|e| ContextError::Internal(format!("structural build failed: {e}")))?;
        let elapsed = t0.elapsed().as_millis();
        Ok(ReconcileStats {
            discovered: idx.stats.discovered,
            changed_files: 0,
            elapsed_ms: elapsed,
        })
    }

    pub async fn search(
        &self,
        query: &str,
        opts: SearchOptions,
    ) -> Result<ContextResult, ContextError> {
        self.reconcile()
            .await
            .map_err(|e| ContextError::Internal(format!("reconcile failed: {e}")))?;
        let root = ProjectRoot::resolve(Some(&self.root))
            .map_err(|e| ContextError::InvalidRoot(e.to_string()))?;
        let project =
            ProjectIndex::discover(&root).map_err(|e| ContextError::Internal(e.to_string()))?;
        let providers = Providers {};
        retrieve_context(
            query,
            &project,
            &providers,
            opts.budget_tokens,
            opts.max_results,
        )
        .await
        .map_err(|e| ContextError::Internal(e.to_string()))
    }

    pub async fn symbol(
        &self,
        symbol: &str,
        opts: SearchOptions,
    ) -> Result<ContextResult, ContextError> {
        self.search(symbol, opts).await
    }

    pub async fn dependency(
        &self,
        symbol: &str,
        direction: Direction,
        opts: SearchOptions,
    ) -> Result<ContextResult, ContextError> {
        let q = match direction {
            Direction::Callers => format!("What calls {}?", symbol),
            Direction::Callees => format!("What does {} call?", symbol),
            Direction::Both => format!("dependency of {}", symbol),
        };
        self.search(&q, opts).await
    }

    pub async fn tests(
        &self,
        query: &str,
        opts: SearchOptions,
    ) -> Result<ContextResult, ContextError> {
        let q = if query.to_lowercase().contains("test") {
            query.to_string()
        } else {
            format!("What tests cover {}?", query)
        };
        self.search(&q, opts).await
    }

    pub async fn status(&self) -> Result<StatusReport, ContextError> {
        let version = env!("CARGO_PKG_VERSION").to_string();
        let project_root = self.root.display().to_string();
        let mut files_indexed = 0usize;
        let mut generation = None;
        let mut symbols = 0usize;
        let mut bm25_docs = 0usize;
        let mut vector_count = 0usize;
        let mut store_version = None;

        if let Ok(root) = ProjectRoot::resolve(Some(&self.root)) {
            if let Ok(conn) = structural_store::open_db(root.path()) {
                if let Ok(gen) = structural_store::get_generation(&conn) {
                    generation = Some(gen);
                }
                if let Ok(cnt) = structural_store::count_symbols(&conn) {
                    symbols = cnt as usize;
                }
                if let Ok(cnt) = context_index::bm25::count_bm25_docs(&conn) {
                    bm25_docs = cnt as usize;
                }
                if let Ok(v) = context_index::vector::count_vectors(
                    &conn,
                    &context_index::embed::OllamaEmbedder::with_model("all-minilm", 384)
                        .fingerprint(),
                ) {
                    vector_count = v as usize;
                }
                // schema version if table exists
                if let Ok(mut stmt) = conn.prepare("SELECT version FROM schema_version LIMIT 1") {
                    if let Ok(v) = stmt.query_row([], |r| r.get::<_, u32>(0)) {
                        store_version = Some(v);
                    }
                }
            }
            if let Ok(idx) = ProjectIndex::discover(&root) {
                files_indexed = idx.stats.discovered;
            }
        }

        // git branch cheap — moved off async executor thread
        let root = self.root.clone();
        let git_branch = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .arg("rev-parse")
                .arg("--abbrev-ref")
                .arg("HEAD")
                .current_dir(&root)
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        String::from_utf8(o.stdout).ok()
                    } else {
                        None
                    }
                })
                .map(|s| s.trim().to_string())
        })
        .await
        .map_err(|e| ContextError::Internal(format!("git branch capture panicked: {e}")))?;

        let model =
            std::env::var("CONTEXTD_EMBED_MODEL").unwrap_or_else(|_| "all-minilm".to_string());
        let semantic_available = {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(500))
                .build();
            if let Ok(c) = client {
                let url = std::env::var("OLLAMA_HOST")
                    .unwrap_or_else(|_| "http://localhost:11434".to_string())
                    + "/api/tags";
                match tokio::time::timeout(
                    std::time::Duration::from_millis(800),
                    c.get(&url).send(),
                )
                .await
                {
                    Ok(Ok(resp)) => resp.status().is_success(),
                    _ => false,
                }
            } else {
                false
            }
        };
        // watcher state — simple
        let watcher_state = "rust-notify".to_string();

        let rust_version = env!("CARGO_PKG_RUST_VERSION").to_string();
        let rust_version = if rust_version.is_empty() {
            "1.80".to_string()
        } else {
            rust_version
        };
        Ok(StatusReport {
            version: version.clone(),
            contextd_version: version,
            rust_version,
            pid: std::process::id(),
            project_root,
            git_branch,
            index_generation: generation,
            files_indexed,
            symbols,
            bm25_documents: bm25_docs,
            vector_count,
            embedding_model: model,
            embedding_runtime: if semantic_available {
                "ollama".into()
            } else {
                "none".into()
            },
            semantic_available,
            watcher_state,
            store_schema_version: store_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn concurrent_searches_serialized() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo(): pass").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        svc.reconcile().await.unwrap();
        let opts = SearchOptions::default();
        let (r1, r2) = tokio::join!(svc.search("foo", opts.clone()), svc.search("foo", opts));
        assert!(
            r1.is_ok(),
            "first concurrent search should succeed: {:?}",
            r1.err()
        );
        assert!(
            r2.is_ok(),
            "second concurrent search should succeed: {:?}",
            r2.err()
        );
    }

    #[tokio::test]
    async fn reconcile_failure_is_propagated() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".context")).unwrap();
        fs::write(root.join(".context/index"), b"not a directory").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let res = svc.search("foo", SearchOptions::default()).await;
        assert!(
            res.is_err(),
            "search should propagate reconcile failure, got {:?}",
            res
        );
        let err = res.unwrap_err().to_string().to_lowercase();
        assert!(
            err.contains("reconcile"),
            "error should mention reconcile, got: {}",
            err
        );
    }
}
