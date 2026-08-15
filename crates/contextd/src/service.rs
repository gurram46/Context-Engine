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
    pub deleted_files: usize,
    pub elapsed_ms: u128,
    pub vectors_created: usize,
    pub vectors_reused: usize,
    pub embedding_calls: usize,
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
    pub embedding_dimension: usize,
    pub embedding_runtime: String,
    pub semantic_available: bool,
    #[serde(rename = "semanticBackendAvailable")]
    pub semantic_backend_available: bool,
    #[serde(rename = "semanticIndexReady")]
    pub semantic_index_ready: bool,
    pub eligible_chunk_count: usize,
    pub missing_vector_count: usize,
    pub stale_vector_count: usize,
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

    async fn reconcile_inner(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<ReconcileStats, ContextError> {
        self.reconcile_full_inner(override_embedder).await
    }

    async fn reconcile_full_inner(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<ReconcileStats, ContextError> {
        let _guard = self.build_lock.lock().await;
        let t0 = std::time::Instant::now();
        let root = ProjectRoot::resolve(Some(&self.root))
            .map_err(|e| ContextError::InvalidRoot(e.to_string()))?;
        let idx =
            ProjectIndex::discover(&root).map_err(|e| ContextError::Internal(e.to_string()))?;
        let root_path = root.path().to_path_buf();
        let idx_clone = idx.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let si = context_index::structural::StructuralIndex::for_path(root_path);
            si.build_with_delta(&idx_clone)
        })
        .await
        .map_err(|e| ContextError::Internal(format!("structural build panicked: {e}")))?
        .map_err(|e| ContextError::Internal(format!("structural build failed: {e}")))?;

        let _elapsed_struct = t0.elapsed().as_millis();
        let mut vectors_created = 0usize;
        let mut vectors_reused = 0usize;
        let mut embedding_calls = 0usize;

        let fingerprint = if let Some(ref emb) = override_embedder {
            emb.fingerprint()
        } else {
            context_index::embed::configured_fingerprint()
        };
        let backend_available = if override_embedder.is_some() {
            true
        } else {
            context_index::embed::is_configured_model_available().await
        };

        if backend_available {
            let embedder: Arc<dyn Embedder> = if let Some(e) = override_embedder {
                e
            } else {
                Arc::new(context_index::embed::configured_embedder())
            };
            match structural_store::open_db(root.path()) {
                Ok(mut conn) => {
                    match context_index::vector::sync_missing_vectors_for_root(
                        &mut conn,
                        root.path(),
                        embedder.as_ref(),
                    )
                    .await
                    {
                        Ok((reused, created, calls, _docs)) => {
                            vectors_reused = reused;
                            vectors_created = created;
                            embedding_calls = calls;
                        }
                        Err(e) => {
                            tracing::warn!(error=%e, "semantic sync failed, structural still ok");
                        }
                    }
                    let _ = context_index::vector::gc_orphaned_vectors(&conn, &fingerprint);
                }
                Err(e) => {
                    tracing::warn!(error=%e, "open_db for semantic sync failed");
                }
            }
        } else {
            tracing::debug!("semantic backend unavailable, skipping vector sync");
        }

        let elapsed = t0.elapsed().as_millis();
        Ok(ReconcileStats {
            discovered: idx.stats.discovered,
            changed_files: outcome.changed_files.len(),
            deleted_files: outcome.deleted_files.len(),
            elapsed_ms: elapsed,
            vectors_created,
            vectors_reused,
            embedding_calls,
        })
    }

    async fn reconcile_fast_inner(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<ReconcileStats, ContextError> {
        let _guard = self.build_lock.lock().await;
        let t0 = std::time::Instant::now();
        let root = ProjectRoot::resolve(Some(&self.root))
            .map_err(|e| ContextError::InvalidRoot(e.to_string()))?;
        let idx =
            ProjectIndex::discover(&root).map_err(|e| ContextError::Internal(e.to_string()))?;
        let root_path = root.path().to_path_buf();
        let idx_clone = idx.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let si = context_index::structural::StructuralIndex::for_path(root_path);
            si.build_with_delta(&idx_clone)
        })
        .await
        .map_err(|e| ContextError::Internal(format!("structural build panicked: {e}")))?
        .map_err(|e| ContextError::Internal(format!("structural build failed: {e}")))?;

        let mut vectors_created = 0usize;
        let mut vectors_reused = 0usize;
        let mut embedding_calls = 0usize;

        let fingerprint = if let Some(ref emb) = override_embedder {
            emb.fingerprint()
        } else {
            context_index::embed::configured_fingerprint()
        };
        let backend_available = if override_embedder.is_some() {
            true
        } else {
            context_index::embed::is_configured_model_available().await
        };

        if backend_available {
            let embedder: Arc<dyn Embedder> = if let Some(e) = override_embedder {
                e
            } else {
                Arc::new(context_index::embed::configured_embedder())
            };
            match structural_store::open_db(root.path()) {
                Ok(mut conn) => {
                    // Fast path: only incremental when previously ready and small delta
                    let eligible = context_index::vector::eligible_chunk_count(&conn).unwrap_or(0);
                    let missing_before = context_index::vector::missing_vector_count(&conn, &fingerprint).unwrap_or(0);
                    let is_ready_before = eligible > 0 && missing_before == 0;
                    let small_delta = outcome.changed_files.len() <= 10 && outcome.deleted_files.len() <= 10;
                    if is_ready_before && small_delta && !outcome.changed_files.is_empty() {
                        match context_index::vector::sync_changed_files_vectors(
                            &mut conn,
                            root.path(),
                            &outcome.changed_files,
                            embedder.as_ref(),
                        )
                        .await
                        {
                            Ok((reused, embedded, calls)) => {
                                vectors_reused = reused;
                                vectors_created = embedded;
                                embedding_calls = calls;
                            }
                            Err(e) => {
                                tracing::warn!(error=%e, "incremental semantic sync failed");
                            }
                        }
                    } else {
                        // Cold or no-change or large delta: do not full backfill, just GC if deleted
                        if !outcome.deleted_files.is_empty() {
                            let _ = context_index::vector::gc_orphaned_vectors(&conn, &fingerprint);
                        }
                    }
                    // Ensure GC for deleted files even when skipping embedding
                    if !outcome.deleted_files.is_empty() {
                        let _ = context_index::vector::gc_orphaned_vectors(&conn, &fingerprint);
                    }
                }
                Err(e) => {
                    tracing::warn!(error=%e, "open_db for fast semantic sync failed");
                }
            }
        }

        let elapsed = t0.elapsed().as_millis();
        Ok(ReconcileStats {
            discovered: idx.stats.discovered,
            changed_files: outcome.changed_files.len(),
            deleted_files: outcome.deleted_files.len(),
            elapsed_ms: elapsed,
            vectors_created,
            vectors_reused,
            embedding_calls,
        })
    }

    /// Cheap reconcile — discovery + incremental structural/BM25 + semantic if available (fast, incremental only).
    /// Target <100ms for no-change. Serialized to avoid concurrent SQLite writes.
    pub async fn reconcile(&self) -> Result<ReconcileStats, ContextError> {
        self.reconcile_fast_inner(None).await
    }

    #[cfg(test)]
    pub async fn reconcile_with_embedder(
        &self,
        embedder: Arc<dyn Embedder>,
    ) -> Result<ReconcileStats, ContextError> {
        self.reconcile_inner(Some(embedder)).await
    }

    pub async fn full_semantic_index(&self) -> Result<ReconcileStats, ContextError> {
        self.reconcile_full_inner(None).await
    }

    #[cfg(test)]
    pub async fn full_semantic_index_with_embedder(
        &self,
        embedder: std::sync::Arc<dyn context_index::embed::Embedder>,
    ) -> Result<ReconcileStats, ContextError> {
        self.reconcile_full_inner(Some(embedder)).await
    }

    pub async fn search(
        &self,
        query: &str,
        opts: SearchOptions,
    ) -> Result<ContextResult, ContextError> {
        self.reconcile_fast_inner(None)
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
        let mut eligible = 0usize;
        let mut missing = 0usize;
        let mut stale = 0usize;
        let mut store_version = None;

        let fingerprint = context_index::embed::configured_fingerprint();
        if let Ok(root) = ProjectRoot::resolve(Some(&self.root)) {
            if let Ok(conn) = structural_store::open_db(root.path()) {
                if let Ok(gen) = structural_store::get_generation(&conn) {
                    generation = Some(gen);
                }
                if let Ok(cnt) = structural_store::count_symbols(&conn) {
                    symbols = cnt as usize;
                }
                if let Ok(cnt) = structural_store::count_files(&conn) {
                    files_indexed = cnt as usize;
                }
                if let Ok(cnt) = context_index::bm25::count_bm25_docs(&conn) {
                    bm25_docs = cnt as usize;
                }
                vector_count =
                    context_index::vector::count_vectors(&conn, &fingerprint).unwrap_or(0) as usize;
                eligible = context_index::vector::eligible_chunk_count(&conn).unwrap_or(0);
                missing =
                    context_index::vector::missing_vector_count(&conn, &fingerprint).unwrap_or(0);
                stale = context_index::vector::stale_vector_count(&conn, &fingerprint).unwrap_or(0);
                if let Ok(mut stmt) = conn.prepare("SELECT version FROM schema_version LIMIT 1") {
                    if let Ok(v) = stmt.query_row([], |r| r.get::<_, u32>(0)) {
                        store_version = Some(v);
                    }
                }
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

        let model = context_index::embed::configured_model_name();
        let dim = fingerprint.dimension;
        let semantic_backend_available =
            context_index::embed::is_configured_model_available().await;
        let semantic_index_ready = if !semantic_backend_available {
            false
        } else {
            // need conn again for is_semantic_ready but we already computed missing/eligible
            if eligible == 0 {
                false
            } else {
                missing == 0
            }
        };
        let semantic_available = semantic_backend_available;
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
            embedding_dimension: dim,
            embedding_runtime: if semantic_backend_available {
                "ollama".into()
            } else {
                "none".into()
            },
            semantic_available,
            semantic_backend_available,
            semantic_index_ready,
            eligible_chunk_count: eligible,
            missing_vector_count: missing,
            stale_vector_count: stale,
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
    static ENV_GLOBAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn backend_unavailable_lexical_still_works() {
        let _env_guard = ENV_GLOBAL_LOCK.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        // force semantic disabled via env
        let orig = std::env::var("CONTEXTD_SEMANTIC_ENABLED").ok();
        std::env::set_var("CONTEXTD_SEMANTIC_ENABLED", "0");
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let res = svc.reconcile().await;
        assert!(
            res.is_ok(),
            "reconcile should succeed even when semantic disabled: {:?}",
            res.err()
        );
        let st = svc.status().await.unwrap();
        assert!(
            !st.semantic_backend_available,
            "backend should be reported unavailable when disabled"
        );
        assert!(
            !st.semantic_index_ready,
            "ready should be false when backend unavailable"
        );
        assert!(st.eligible_chunk_count >= st.missing_vector_count);
        // lexical search should still work
        let search = svc.search("foo", SearchOptions::default()).await;
        assert!(
            search.is_ok(),
            "lexical search should succeed: {:?}",
            search.err()
        );
        assert!(
            !search.unwrap().evidence.is_empty(),
            "should find foo via lexical"
        );
        // restore
        if let Some(v) = orig {
            std::env::set_var("CONTEXTD_SEMANTIC_ENABLED", v);
        } else {
            std::env::remove_var("CONTEXTD_SEMANTIC_ENABLED");
        }
    }

    #[tokio::test]
    async fn reconcile_with_fake_embedder_initial_and_no_change() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        fs::write(root.join("b.py"), b"def bar():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let fake = std::sync::Arc::new(context_index::embed::FakeEmbedder::new("svc-test", 8));
        let stats = svc.reconcile_with_embedder(fake.clone()).await.unwrap();
        assert!(stats.changed_files >= 2 || stats.discovered >= 2);
        // second reconcile no change should be 0 embedding (we check via missing)
        let conn = context_index::structural::store::open_db(&root).unwrap();
        let fp = fake.fingerprint();
        let missing = context_index::vector::missing_vector_count(&conn, &fp).unwrap();
        assert_eq!(missing, 0, "after initial, missing should be 0");
        let fake2 = std::sync::Arc::new(context_index::embed::FakeEmbedder::new("svc-test", 8));
        let stats2 = svc.reconcile_with_embedder(fake2.clone()).await.unwrap();
        assert_eq!(
            stats2.changed_files, 0,
            "no-change should have 0 changed_files"
        );
        assert_eq!(stats2.deleted_files, 0);
        // vector count stable
        let conn2 = context_index::structural::store::open_db(&root).unwrap();
        let cnt = context_index::vector::count_vectors(&conn2, &fp).unwrap();
        assert!(cnt > 0);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn status_uses_configured_fingerprint_not_hardcoded() {
        let _env_guard = ENV_GLOBAL_LOCK.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let orig = std::env::var("CONTEXTD_EMBED_MODEL").ok();
        std::env::set_var("CONTEXTD_EMBED_MODEL", "nomic-embed-text");
        let fp = context_index::embed::configured_fingerprint();
        assert_eq!(fp.model_id, "nomic-embed-text");
        assert_eq!(fp.dimension, 768);
        {
            let mut conn = context_index::structural::store::open_db(&root).unwrap();
            let pr = context_index::ProjectRoot::resolve(Some(&root)).unwrap();
            let idx = context_index::ProjectIndex::discover(&pr).unwrap();
            let si = context_index::structural::StructuralIndex::for_path(root.clone());
            si.build_with_delta(&idx).unwrap();
            let vec_data = vec![0.1f32; 768];
            let conn2 = context_index::structural::store::open_db(&root).unwrap();
            let hash: String = conn2
                .query_row("SELECT content_hash FROM chunks LIMIT 1", [], |r| r.get(0))
                .unwrap();
            context_index::vector::upsert_vector(&mut conn, &hash, &fp, &vec_data).unwrap();
        }
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let st = svc.status().await.unwrap();
        assert_eq!(st.embedding_model, "nomic-embed-text");
        assert_eq!(st.embedding_dimension, 768);
        let conn = context_index::structural::store::open_db(&root).unwrap();
        let fp_all = context_index::embed::ModelFingerprint {
            model_id: "all-minilm".into(),
            version: "ollama-all-minilm-v1".into(),
            dimension: 384,
        };
        let cnt_nomic = context_index::vector::count_vectors(&conn, &fp).unwrap();
        let cnt_all = context_index::vector::count_vectors(&conn, &fp_all).unwrap();
        assert!(cnt_nomic > 0);
        assert_eq!(
            cnt_all, 0,
            "hardcoded all-minilm should not be used when nomic configured"
        );
        if let Some(v) = orig {
            std::env::set_var("CONTEXTD_EMBED_MODEL", v);
        } else {
            std::env::remove_var("CONTEXTD_EMBED_MODEL");
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn daemon_reachable_model_unavailable_reports_unavailable() {
        let _env_guard = ENV_GLOBAL_LOCK.lock().unwrap();
        // mock Ollama tags server - handle 2 requests
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let mut buf = [0u8; 2048];
                    let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
                    let body = r#"{"models":[{"name":"all-minilm:latest"}]}"#;
                    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                    let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, resp.as_bytes()).await;
                }
            }
        });
        let orig_host = std::env::var("OLLAMA_HOST").ok();
        let orig_model = std::env::var("CONTEXTD_EMBED_MODEL").ok();
        std::env::set_var("OLLAMA_HOST", format!("http://{}", addr));
        // model that is NOT in tags
        std::env::set_var("CONTEXTD_EMBED_MODEL", "nomic-embed-text");
        // ensure semantic enabled
        let orig_sem = std::env::var("CONTEXTD_SEMANTIC_ENABLED").ok();
        std::env::remove_var("CONTEXTD_SEMANTIC_ENABLED");
        let available = context_index::embed::is_configured_model_available().await;
        assert!(
            !available,
            "nomic should be unavailable when tags only has all-minilm"
        );
        // now check all-minilm is available
        std::env::set_var("CONTEXTD_EMBED_MODEL", "all-minilm");
        let available2 = context_index::embed::is_configured_model_available().await;
        assert!(available2, "all-minilm should be available");
        server.abort();
        if let Some(v) = orig_host {
            std::env::set_var("OLLAMA_HOST", v);
        } else {
            std::env::remove_var("OLLAMA_HOST");
        }
        if let Some(v) = orig_model {
            std::env::set_var("CONTEXTD_EMBED_MODEL", v);
        } else {
            std::env::remove_var("CONTEXTD_EMBED_MODEL");
        }
        if let Some(v) = orig_sem {
            std::env::set_var("CONTEXTD_SEMANTIC_ENABLED", v);
        } else {
            std::env::remove_var("CONTEXTD_SEMANTIC_ENABLED");
        }
    }

    #[tokio::test]
    async fn structural_delta_exposed() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("a.py"), b"def foo(): pass").unwrap();
        let pr = context_index::ProjectRoot::resolve(Some(&root)).unwrap();
        let idx = context_index::ProjectIndex::discover(&pr).unwrap();
        let si = context_index::structural::StructuralIndex::for_path(root.clone());
        let out1 = si.build_with_delta(&idx).unwrap();
        assert!(!out1.changed_files.is_empty());
        assert!(out1.deleted_files.is_empty());
        // no change second build
        let pr2 = context_index::ProjectRoot::resolve(Some(&root)).unwrap();
        let idx2 = context_index::ProjectIndex::discover(&pr2).unwrap();
        let out2 = si.build_with_delta(&idx2).unwrap();
        assert_eq!(out2.changed_files.len(), 0);
        assert_eq!(out2.deleted_files.len(), 0);
        // delete file
        std::fs::remove_file(root.join("a.py")).unwrap();
        let pr3 = context_index::ProjectRoot::resolve(Some(&root)).unwrap();
        let idx3 = context_index::ProjectIndex::discover(&pr3).unwrap();
        let out3 = si.build_with_delta(&idx3).unwrap();
        assert_eq!(out3.changed_files.len(), 0);
        assert_eq!(out3.deleted_files, vec!["a.py".to_string()]);
    }

    #[tokio::test]
    async fn cold_search_does_not_full_backfill() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for i in 0..16 {
            std::fs::write(root.join(format!("f{i}.py")), format!("def foo_{i}():\n    x={i}\n").as_bytes()).unwrap();
        }
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        // Use fast reconcile with counting fake (should embed 0 for cold)
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
        struct CountingFake2 {
            inner: context_index::embed::FakeEmbedder,
            calls: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl context_index::embed::Embedder for CountingFake2 {
            fn model_id(&self) -> &str { self.inner.model_id() }
            fn dimension(&self) -> usize { self.inner.dimension() }
            fn version(&self) -> &str { self.inner.version() }
            async fn embed_query(&self, q: &str) -> anyhow::Result<Vec<f32>> { self.inner.embed_query(q).await }
            async fn embed_documents(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.inner.embed_documents(texts).await
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let fake = Arc::new(CountingFake2 { inner: context_index::embed::FakeEmbedder::new("cold-test", 8), calls: calls.clone() });
        // fast reconcile should NOT embed all 16 (cold)
        let stats = svc.reconcile_fast_inner(Some(fake.clone())).await.unwrap();
        assert_eq!(stats.vectors_created, 0, "cold fast reconcile should embed 0");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // status should be not ready
        let conn = context_index::structural::store::open_db(&root).unwrap();
        let fp = fake.fingerprint();
        assert!(!context_index::vector::is_semantic_ready(&conn, &fp, true).unwrap());
        assert!(context_index::vector::missing_vector_count(&conn, &fp).unwrap() > 0);
        // lexical search should still succeed
        let res = svc.search("foo_0", crate::service::SearchOptions::default()).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn explicit_full_semantic_index_covers_all() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for i in 0..8 {
            std::fs::write(root.join(format!("a{i}.py")), format!("def bar_{i}():\n    y={i}\n").as_bytes()).unwrap();
        }
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let fake = Arc::new(context_index::embed::FakeEmbedder::new("explicit-test", 8));
        let stats = svc.full_semantic_index_with_embedder(fake.clone()).await.unwrap();
        assert!(stats.vectors_created > 0);
        let conn = context_index::structural::store::open_db(&root).unwrap();
        let fp = fake.fingerprint();
        assert_eq!(context_index::vector::missing_vector_count(&conn, &fp).unwrap(), 0);
        assert!(context_index::vector::is_semantic_ready(&conn, &fp, true).unwrap());
    }

    #[tokio::test]
    async fn incremental_one_file_change_only_that_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("a.py"), b"def foo():\n    x=1\n").unwrap();
        std::fs::write(root.join("b.py"), b"def bar():\n    y=2\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let fake = Arc::new(context_index::embed::FakeEmbedder::new("inc-test", 8));
        // first full
        svc.full_semantic_index_with_embedder(fake.clone()).await.unwrap();
        // change one file
        std::fs::write(root.join("a.py"), b"def foo():\n    x=999\n").unwrap();
        // fast reconcile should embed only changed file
        let stats = svc.reconcile_fast_inner(Some(fake.clone())).await.unwrap();
        assert_eq!(stats.changed_files, 1);
        assert_eq!(stats.vectors_created, 1, "only changed chunk");
        // no-change after
        let stats2 = svc.reconcile_fast_inner(Some(fake.clone())).await.unwrap();
        assert_eq!(stats2.changed_files, 0);
        assert_eq!(stats2.vectors_created, 0);
    }
}
