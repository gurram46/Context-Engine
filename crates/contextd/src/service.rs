#![allow(dead_code)]
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use context_core::ContextError;
use context_index::embed::Embedder;
use context_index::structural::store as structural_store;
use context_index::structural::{detect_language, Language};
use context_index::{ProjectIndex, ProjectRoot};

use crate::pipeline::{retrieve_context, ContextResult, Providers};
use crate::runtime::{RepositoryRuntime, RuntimeState};

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

/// Snapshot access result for search/dependency, carrying truthful E2 telemetry.
pub(crate) struct RuntimeAccess {
    pub project: Arc<ProjectIndex>,
    pub generation: u64,
    pub reconcile_skipped: bool,
    pub discovery_calls: u32,
    pub reconcile_calls: u32,
    pub runtime_state: &'static str,
    pub dirty_file_count: Option<usize>,
    pub discovery_ms: u128,
    pub reconcile_ms: u128,
}

/// Result of one full discovery + structural build, before any publish/ack.
struct FullReconcile {
    stats: ReconcileStats,
    project: Arc<ProjectIndex>,
    discovery_ms: u128,
    fingerprint: context_index::embed::ModelFingerprint,
    generation: u64,
    epoch: u64,
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
    pub semantic_ref_count: usize,
    pub representation_version: String,
    pub missing_vector_count: usize,
    pub stale_vector_count: usize,
    pub watcher_state: String,
    pub store_schema_version: Option<u32>,
}

/// Native service — single core for CLI and MCP.
pub struct ContextService {
    root: PathBuf,
    explicit_root: Option<PathBuf>,
    runtime: Arc<RepositoryRuntime>,
}

impl ContextService {
    /// Create service for given root (or auto-resolve via ProjectRoot::resolve).
    /// Starts the mark-only watcher and completes one full discovery/reconcile
    /// before returning so that the runtime snapshot is immediately usable.
    pub async fn new(root: Option<PathBuf>) -> Result<Self, ContextError> {
        let root_path = if let Some(p) = root.clone() {
            p.canonicalize().unwrap_or(p)
        } else {
            ProjectRoot::resolve(None)
                .map(|r| r.path().to_path_buf())
                .map_err(|e| ContextError::InvalidRoot(e.to_string()))?
        };
        let runtime = Arc::new(RepositoryRuntime::new(root_path.clone()).map_err(|e| {
            ContextError::Internal(format!("failed to start repository runtime: {e}"))
        })?);
        let service = Self {
            root: root_path,
            explicit_root: root,
            runtime,
        };
        service.initialize().await?;
        Ok(service)
    }

    async fn initialize(&self) -> Result<(), ContextError> {
        self.reconcile_and_publish(None).await?;
        Ok(())
    }

    async fn reconcile_and_publish(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<(ReconcileStats, Arc<ProjectIndex>, u128), ContextError> {
        let _guard = self.runtime.reconcile_lock.lock().await;
        self.reconcile_and_publish_locked(override_embedder).await
    }

    /// Caller holds `runtime.reconcile_lock`; runtime data is locked only while publishing.
    async fn reconcile_full_discovery_locked(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<FullReconcile, ContextError> {
        let epoch = self.runtime.tracker.snapshot().epoch;
        let t0 = Instant::now();
        let root = ProjectRoot::resolve(Some(&self.root))
            .map_err(|e| ContextError::InvalidRoot(e.to_string()))?;

        let fingerprint = override_embedder
            .as_ref()
            .map(|embedder| embedder.fingerprint())
            .unwrap_or_else(context_index::embed::configured_fingerprint);
        // Structural reconciliation mutates semantic references, so readiness must be sampled first.
        let was_semantic_ready = structural_store::open_db(root.path())
            .ok()
            .and_then(|conn| {
                context_index::vector::is_semantic_ready(&conn, &fingerprint, true).ok()
            })
            .unwrap_or(false);

        let t_discovery = Instant::now();
        self.runtime.increment_discovery();
        let idx =
            ProjectIndex::discover(&root).map_err(|e| ContextError::Internal(e.to_string()))?;
        let discovery_ms = t_discovery.elapsed().as_millis();
        let root_path = root.path().to_path_buf();
        let idx_clone = idx.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let si = context_index::structural::StructuralIndex::for_path(root_path);
            si.build_with_delta(&idx_clone)
        })
        .await
        .map_err(|e| ContextError::Internal(format!("structural build panicked: {e}")))?
        .map_err(|e| ContextError::Internal(format!("structural build failed: {e}")))?;

        let backend_available = if override_embedder.is_some() {
            true
        } else {
            context_index::embed::is_configured_model_available().await
        };

        let mut vectors_created = 0usize;
        let mut vectors_reused = 0usize;
        let mut embedding_calls = 0usize;

        if backend_available {
            let embedder: Arc<dyn Embedder> = if let Some(e) = override_embedder.clone() {
                e
            } else {
                Arc::new(context_index::embed::configured_embedder())
            };
            match structural_store::open_db(root.path()) {
                Ok(mut conn) => {
                    let small_delta =
                        outcome.changed_files.len() <= 10 && outcome.deleted_files.len() <= 10;
                    if was_semantic_ready && small_delta && !outcome.changed_files.is_empty() {
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
                    }
                }
                Err(e) => {
                    tracing::warn!(error=%e, "open_db for semantic sync failed");
                }
            }
        } else {
            tracing::debug!("semantic backend unavailable, skipping vector sync");
        }

        // Orphan GC after deletion is a local SQLite DELETE, independent of the
        // embedding backend (F5).
        if !outcome.deleted_files.is_empty() {
            if let Ok(conn) = structural_store::open_db(root.path()) {
                let _ = context_index::vector::gc_orphaned_vectors(&conn, &fingerprint);
            }
        }

        let generation = structural_store::open_db(root.path())
            .ok()
            .and_then(|c| structural_store::get_generation(&c).ok())
            .unwrap_or(0);
        let elapsed = t0.elapsed().as_millis();
        let stats = ReconcileStats {
            discovered: idx.stats.discovered,
            changed_files: outcome.changed_files.len(),
            deleted_files: outcome.deleted_files.len(),
            elapsed_ms: elapsed,
            vectors_created,
            vectors_reused,
            embedding_calls,
        };
        Ok(FullReconcile {
            stats,
            project: Arc::new(idx),
            discovery_ms,
            fingerprint,
            generation,
            epoch,
        })
    }

    async fn reconcile_and_publish_locked(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<(ReconcileStats, Arc<ProjectIndex>, u128), ContextError> {
        let full = self
            .reconcile_full_discovery_locked(override_embedder)
            .await?;
        self.runtime.publish(
            full.project.clone(),
            full.generation,
            full.fingerprint.clone(),
            Instant::now(),
            true,
        );
        #[cfg(test)]
        self.runtime.run_test_hook();
        self.runtime.tracker.acknowledge(full.epoch);
        Ok((full.stats, full.project, full.discovery_ms))
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
        let _guard = self.runtime.reconcile_lock.lock().await;
        let mut full = self
            .reconcile_full_discovery_locked(override_embedder.clone())
            .await?;
        let backend_available = if override_embedder.is_some() {
            true
        } else {
            context_index::embed::is_configured_model_available().await
        };
        if backend_available {
            let embedder: Arc<dyn Embedder> = override_embedder
                .unwrap_or_else(|| Arc::new(context_index::embed::configured_embedder()));
            match structural_store::open_db(&full.project.root) {
                Ok(mut conn) => {
                    match context_index::vector::sync_missing_vectors_for_root(
                        &mut conn,
                        &full.project.root,
                        embedder.as_ref(),
                    )
                    .await
                    {
                        Ok((reused, created, calls, _)) => {
                            full.stats.vectors_reused += reused;
                            full.stats.vectors_created += created;
                            full.stats.embedding_calls += calls;
                        }
                        Err(e) => {
                            tracing::warn!(error=%e, "full semantic sync failed, structural still ok");
                        }
                    }
                }
                Err(e) => tracing::warn!(error=%e, "open_db for semantic sync failed"),
            }
        }
        // Orphan GC is a local SQLite DELETE and must not be gated on the
        // embedding backend (NF2). Use the full discovery fingerprint.
        if let Ok(conn) = structural_store::open_db(&full.project.root) {
            let _ = context_index::vector::gc_orphaned_vectors(&conn, &full.fingerprint);
        }
        // Publish the clean replacement snapshot only after the full semantic
        // backfill has completed (F6).
        self.runtime.publish(
            full.project.clone(),
            full.generation,
            full.fingerprint.clone(),
            Instant::now(),
            true,
        );
        #[cfg(test)]
        self.runtime.run_test_hook();
        self.runtime.tracker.acknowledge(full.epoch);
        Ok(full.stats)
    }

    async fn reconcile_fast_inner(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<ReconcileStats, ContextError> {
        let (stats, _, _) = self.reconcile_and_publish(override_embedder).await?;
        Ok(stats)
    }

    /// Shared runtime access for search and dependency. Caller acquires the
    /// reconcile lock and releases it before retrieval runs.
    async fn access_runtime(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<RuntimeAccess, ContextError> {
        let _guard = self.runtime.reconcile_lock.lock().await;
        let now = Instant::now();
        // One snapshot for state + paths + epoch so a concurrent watcher event
        // cannot split them (F1/F2).
        let access = self.runtime.dirty_access();
        let has_snapshot = self.runtime.current_snapshot().is_some();
        let expired = self.runtime.is_verification_expired(now);

        if access.state == RuntimeState::Clean && !expired {
            let snapshot = self.runtime.current_snapshot().ok_or_else(|| {
                ContextError::Internal("clean runtime has no project snapshot".into())
            })?;
            return Ok(RuntimeAccess {
                project: snapshot.project,
                generation: snapshot.generation,
                reconcile_skipped: true,
                discovery_calls: 0,
                reconcile_calls: 0,
                runtime_state: "clean",
                dirty_file_count: Some(0),
                discovery_ms: 0,
                reconcile_ms: 0,
            });
        }

        // Expired verification dominates DirtyState::Paths (NF1): every
        // expired request performs exactly one full discovery/reconcile with
        // runtime_state unknown, discovery_calls 1, reconcile_calls 1,
        // dirty_file_count None.
        if !expired {
            if let Some(paths) = access.paths {
                if has_snapshot {
                    let dirty_count = paths.len();
                    #[cfg(test)]
                    self.runtime.run_pre_reconcile_hook();
                    let (stats, project) = self
                        .reconcile_dirty_paths_locked(override_embedder, &paths, access.epoch)
                        .await?;
                    let generation = self
                        .runtime
                        .current_snapshot()
                        .map(|s| s.generation)
                        .unwrap_or(0);
                    return Ok(RuntimeAccess {
                        project,
                        generation,
                        reconcile_skipped: false,
                        discovery_calls: 0,
                        reconcile_calls: 1,
                        runtime_state: "dirty",
                        dirty_file_count: Some(dirty_count),
                        discovery_ms: 0,
                        reconcile_ms: stats.elapsed_ms,
                    });
                }
            }
        }

        // Unknown, expired (including Dirty+expired), or dirty-without-snapshot:
        // one full discovery/reconcile.
        let (stats, project, discovery_ms) =
            self.reconcile_and_publish_locked(override_embedder).await?;
        let generation = self
            .runtime
            .current_snapshot()
            .map(|s| s.generation)
            .unwrap_or(0);
        Ok(RuntimeAccess {
            project,
            generation,
            reconcile_skipped: false,
            discovery_calls: 1,
            reconcile_calls: 1,
            runtime_state: "unknown",
            dirty_file_count: None,
            discovery_ms,
            reconcile_ms: stats.elapsed_ms,
        })
    }

    /// Path-local dirty reconciliation. Caller holds `reconcile_lock`.
    /// Only `refresh_paths` re-scans captured paths; structural updates run
    /// through `update_single_file` inside `spawn_blocking`.
    async fn reconcile_dirty_paths_locked(
        &self,
        override_embedder: Option<Arc<dyn Embedder>>,
        paths: &BTreeSet<String>,
        epoch: u64,
    ) -> Result<(ReconcileStats, Arc<ProjectIndex>), ContextError> {
        let t0 = Instant::now();
        let root = ProjectRoot::resolve(Some(&self.root))
            .map_err(|e| ContextError::InvalidRoot(e.to_string()))?;

        // Fingerprint of the model actually in use: the override when supplied,
        // else the runtime's published semantic fingerprint (set by the last
        // full/initial reconcile). This keeps orphan GC targeted at the vectors
        // that were really created, not always the configured model (F5).
        let fingerprint = override_embedder
            .as_ref()
            .map(|e| e.fingerprint())
            .unwrap_or_else(|| self.runtime.semantic_fingerprint());
        // Structural mutation may change semantic refs; sample readiness first.
        let was_semantic_ready = structural_store::open_db(root.path())
            .ok()
            .and_then(|conn| {
                context_index::vector::is_semantic_ready(&conn, &fingerprint, true).ok()
            })
            .unwrap_or(false);

        let current = self
            .runtime
            .current_snapshot()
            .ok_or_else(|| ContextError::Internal("no project snapshot to refresh".into()))?
            .project;

        // Path-local snapshot refresh — never a full walk.
        let delta = current
            .refresh_paths(paths)
            .map_err(|e| ContextError::Internal(e.to_string()))?;

        let root_path = root.path().to_path_buf();
        let changed = delta.changed_files.clone();
        let deleted = delta.deleted_files.clone();
        let (parsed, removed) = tokio::task::spawn_blocking(move || {
            let si = context_index::structural::StructuralIndex::for_path(root_path);
            let mut parsed = Vec::new();
            let mut removed = Vec::new();
            for f in &changed {
                if detect_language(std::path::Path::new(f)) == Language::Unknown {
                    continue;
                }
                let stats = si
                    .update_single_file(f)
                    .map_err(|e| ContextError::Internal(e.to_string()))?;
                if stats.files_parsed == 1 {
                    parsed.push(f.clone());
                }
            }
            for f in &deleted {
                if detect_language(std::path::Path::new(f)) == Language::Unknown {
                    continue;
                }
                let stats = si
                    .update_single_file(f)
                    .map_err(|e| ContextError::Internal(e.to_string()))?;
                if stats.files_deleted == 1 {
                    removed.push(f.clone());
                }
            }
            Ok::<_, ContextError>((parsed, removed))
        })
        .await
        .map_err(|e| ContextError::Internal(format!("structural update panicked: {e}")))??;

        let backend_available = if override_embedder.is_some() {
            true
        } else {
            context_index::embed::is_configured_model_available().await
        };

        let mut vectors_created = 0usize;
        let mut vectors_reused = 0usize;
        let mut embedding_calls = 0usize;

        if backend_available {
            let embedder: Arc<dyn Embedder> = override_embedder
                .clone()
                .unwrap_or_else(|| Arc::new(context_index::embed::configured_embedder()));
            match structural_store::open_db(root.path()) {
                Ok(mut conn) => {
                    if was_semantic_ready && !parsed.is_empty() {
                        match context_index::vector::sync_changed_files_vectors(
                            &mut conn,
                            root.path(),
                            &parsed,
                            embedder.as_ref(),
                        )
                        .await
                        {
                            Ok((reused, embedded, calls)) => {
                                vectors_reused = reused;
                                vectors_created = embedded;
                                embedding_calls = calls;
                            }
                            Err(e) => tracing::warn!(error=%e, "incremental semantic sync failed"),
                        }
                    }
                }
                Err(e) => tracing::warn!(error=%e, "open_db for semantic sync failed"),
            }
        }

        // Orphan GC is a local SQLite DELETE independent of the embedding backend;
        // it must run after any deletion with the fingerprint of the model actually
        // in use (F5).
        if !removed.is_empty() {
            if let Ok(conn) = structural_store::open_db(root.path()) {
                let _ = context_index::vector::gc_orphaned_vectors(&conn, &fingerprint);
            }
        }

        // Truthful repository generation: bump once when the project snapshot
        // content changed but no structural file was actually parsed/deleted.
        let structural_changed = !parsed.is_empty() || !removed.is_empty();
        let content_changed = {
            let mut any = !delta.deleted_files.is_empty();
            if !any {
                for f in &delta.changed_files {
                    let old_hash = current.find_by_path(f).map(|r| r.content_hash.clone());
                    let new_hash = delta
                        .project
                        .find_by_path(f)
                        .map(|r| r.content_hash.clone());
                    if old_hash != new_hash {
                        any = true;
                        break;
                    }
                }
            }
            any
        };
        let mut generation = structural_store::open_db(root.path())
            .ok()
            .and_then(|c| structural_store::get_generation(&c).ok())
            .unwrap_or(0);
        if content_changed && !structural_changed {
            if let Ok(conn) = structural_store::open_db(root.path()) {
                let next = generation.saturating_add(1);
                if structural_store::set_generation(&conn, next).is_ok() {
                    generation = next;
                }
            }
        }

        let elapsed = t0.elapsed().as_millis();
        let stats = ReconcileStats {
            discovered: delta.project.stats.discovered,
            changed_files: parsed.len(),
            deleted_files: removed.len(),
            elapsed_ms: elapsed,
            vectors_created,
            vectors_reused,
            embedding_calls,
        };
        let project_arc = Arc::new(delta.project);
        self.runtime.publish(
            project_arc.clone(),
            generation,
            fingerprint,
            Instant::now(),
            false,
        );
        #[cfg(test)]
        self.runtime.run_test_hook();
        self.runtime.tracker.acknowledge(epoch);
        Ok((stats, project_arc))
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
        self.search_with_override(query, opts, None).await
    }

    async fn search_with_override(
        &self,
        query: &str,
        opts: SearchOptions,
        override_embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<ContextResult, ContextError> {
        let t_search = std::time::Instant::now();
        let access = self
            .access_runtime(override_embedder)
            .await
            .map_err(|e| ContextError::Internal(format!("reconcile failed: {e}")))?;
        let providers = Providers {};
        let mut res = retrieve_context(
            query,
            &access.project,
            &providers,
            opts.budget_tokens,
            opts.max_results,
        )
        .await
        .map_err(|e| ContextError::Internal(e.to_string()))?;
        // Fill stage telemetry at service layer
        let total_ms = t_search.elapsed().as_millis();
        res.stats.total_ms = Some(total_ms);
        res.stats.discovery_ms = Some(access.discovery_ms);
        res.stats.reconcile_ms = Some(access.reconcile_ms);
        res.stats.generation = Some(access.generation);
        res.stats.dirty_file_count = access.dirty_file_count;
        res.stats.reconcile_skipped = Some(access.reconcile_skipped);
        res.stats.discovery_calls = Some(access.discovery_calls);
        res.stats.reconcile_calls = Some(access.reconcile_calls);
        res.stats.runtime_state = Some(access.runtime_state.to_string());
        res.stats.cache_hit = None;
        if res.stats.authority_ms.is_none() {
            res.stats.authority_ms = Some(res.stats.rank_ms);
        }
        Ok(res)
    }

    #[cfg(test)]
    pub(crate) async fn search_with_embedder(
        &self,
        query: &str,
        opts: SearchOptions,
        embedder: Arc<dyn Embedder>,
    ) -> Result<ContextResult, ContextError> {
        self.search_with_override(query, opts, Some(embedder)).await
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
        // Graph-native dependency retrieval: directly query call graph, not via generic search
        let access = self
            .access_runtime(None)
            .await
            .map_err(|e| ContextError::Internal(format!("reconcile failed: {e}")))?;
        let root = ProjectRoot::resolve(Some(&self.root))
            .map_err(|e| ContextError::InvalidRoot(e.to_string()))?;
        let t0 = std::time::Instant::now();
        let mut candidates: Vec<context_rank::types::Evidence> = Vec::new();
        let mut retrievers_used = Vec::new();
        let conn = structural_store::open_db(root.path())
            .map_err(|e| ContextError::Internal(e.to_string()))?;
        let t_graph = std::time::Instant::now();
        // Helper to build Evidence from CallEdge
        let build_evidence_for_edge = |edge: &context_index::structural::types::CallEdge,
                                       rel: context_rank::types::EvidenceRelation,
                                       provenance: String|
         -> anyhow::Result<context_rank::types::Evidence> {
            // Try to load caller/callee symbol for richer snippet
            let symbol_info: Option<context_index::structural::types::Symbol> =
                if rel == context_rank::types::EvidenceRelation::Caller {
                    structural_store::find_symbol_by_id(&conn, &edge.caller_symbol_id)
                        .ok()
                        .flatten()
                } else if rel == context_rank::types::EvidenceRelation::Callee {
                    edge.resolved_symbol_id
                        .as_deref()
                        .and_then(|id| {
                            structural_store::find_symbol_by_id(&conn, id)
                                .ok()
                                .flatten()
                        })
                        .or_else(|| {
                            structural_store::find_definitions(&conn, &edge.callee_name)
                                .ok()
                                .and_then(|v| v.into_iter().next())
                        })
                } else {
                    None
                };
            let (file, start_line, end_line, sym_name, sym_kind, text) = if let Some(sym) =
                symbol_info
            {
                let txt = {
                    let p = root.path().join(&sym.file);
                    std::fs::read_to_string(&p)
                        .ok()
                        .and_then(|c| {
                            let bytes = c.as_bytes();
                            let start = sym.start_byte.min(bytes.len());
                            let end = sym.end_byte.min(bytes.len());
                            if end > start {
                                let slice = &c[start..end];
                                Some(
                                    slice
                                        .chars()
                                        .take(400)
                                        .collect::<String>()
                                        .trim()
                                        .to_string(),
                                )
                            } else {
                                c.lines()
                                    .nth((sym.start_line as usize).saturating_sub(1))
                                    .map(|l| l.chars().take(400).collect())
                            }
                        })
                        .unwrap_or_else(|| format!("{} {}", sym.kind.as_str(), sym.qualified_name))
                };
                (
                    sym.file.clone(),
                    Some(sym.start_line),
                    Some(sym.end_line),
                    Some(sym.name.clone()),
                    Some(sym.kind.as_str().to_string()),
                    Some(txt),
                )
            } else {
                // Fallback: use edge file/line
                let txt = {
                    let p = root.path().join(&edge.file);
                    std::fs::read_to_string(&p).ok().and_then(|c| {
                        c.lines()
                            .nth((edge.line as usize).saturating_sub(1))
                            .map(|l| l.chars().take(400).collect::<String>())
                    })
                };
                (
                    edge.file.clone(),
                    Some(edge.line),
                    Some(edge.line),
                    Some(edge.callee_name.clone()),
                    None,
                    txt,
                )
            };
            let score = match edge.confidence {
                context_index::structural::types::CallConfidence::Resolved => 1.0,
                context_index::structural::types::CallConfidence::Probable => 0.8,
                context_index::structural::types::CallConfidence::Unresolved => 0.6,
            };
            Ok(context_rank::types::Evidence {
                source: context_rank::types::RetrievalSource::Graph,
                file,
                start_line,
                end_line,
                symbol: sym_name,
                symbol_kind: sym_kind,
                text,
                score: Some(score),
                relation: Some(rel),
                authority_score: None,
                final_score: None,
                provenance: Some(provenance),
                metadata: None,
            })
        };
        if direction == Direction::Callers || direction == Direction::Both {
            if let Ok(callers) = structural_store::find_callers(&conn, symbol) {
                for edge in callers.iter().take(50) {
                    if let Ok(ev) = build_evidence_for_edge(
                        edge,
                        context_rank::types::EvidenceRelation::Caller,
                        format!("rust:graph:callers:{}", edge.confidence.as_str()),
                    ) {
                        candidates.push(ev);
                    }
                }
                retrievers_used.push(format!("rust-graph:callers:{}:{}", symbol, callers.len()));
            } else {
                retrievers_used.push(format!("rust-graph:callers:{}:0", symbol));
            }
        }
        if direction == Direction::Callees || direction == Direction::Both {
            if let Ok(callees) = structural_store::find_callees(&conn, symbol) {
                for edge in callees.iter().take(20) {
                    if let Ok(ev) = build_evidence_for_edge(
                        edge,
                        context_rank::types::EvidenceRelation::Callee,
                        format!("rust:graph:callees:{}", edge.confidence.as_str()),
                    ) {
                        candidates.push(ev);
                    }
                }
                retrievers_used.push(format!("rust-graph:callees:{}:{}", symbol, callees.len()));
            } else {
                retrievers_used.push(format!("rust-graph:callees:{}:0", symbol));
            }
        }
        drop(conn);
        let graph_ms_total = t_graph.elapsed().as_millis();
        // If graph returned nothing, still try to provide at least symbol definition as fallback (truthful)
        if candidates.is_empty() {
            // Fallback: try symbol definition for the queried symbol itself (not its caller)
            let conn_fallback = structural_store::open_db(root.path())
                .map_err(|e| ContextError::Internal(format!("open_db for fallback failed: {e}")))?;
            if let Ok(defs) = structural_store::find_definitions(&conn_fallback, symbol) {
                for def in defs.iter().take(2) {
                    let txt = {
                        let p = root.path().join(&def.file);
                        std::fs::read_to_string(&p)
                            .ok()
                            .and_then(|c| {
                                let bytes = c.as_bytes();
                                let start = def.start_byte.min(bytes.len());
                                let end = def.end_byte.min(bytes.len());
                                if end > start {
                                    let slice = &c[start..end];
                                    Some(
                                        slice
                                            .chars()
                                            .take(400)
                                            .collect::<String>()
                                            .trim()
                                            .to_string(),
                                    )
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| {
                                format!("{} {}", def.kind.as_str(), def.qualified_name)
                            })
                    };
                    candidates.push(context_rank::types::Evidence {
                        source: context_rank::types::RetrievalSource::Symbol,
                        file: def.file.clone(),
                        start_line: Some(def.start_line),
                        end_line: Some(def.end_line),
                        symbol: Some(def.name.clone()),
                        symbol_kind: Some(def.kind.as_str().to_string()),
                        text: Some(txt),
                        score: Some(1.0),
                        relation: Some(context_rank::types::EvidenceRelation::Definition),
                        authority_score: None,
                        final_score: None,
                        provenance: Some("rust:symbol".into()),
                        metadata: None,
                    });
                }
                if !candidates.is_empty() {
                    retrievers_used.push(format!("rust-symbol:{}:{}", symbol, candidates.len()));
                }
            }
        }
        // Authority, fuse, pack (reuse pipeline helpers)
        let t_auth = std::time::Instant::now();
        // For dependency, use Symbol query type for authority (generic)
        let scored = context_rank::apply_authority(
            candidates,
            context_rank::types::QueryType::Dependency,
            symbol,
        );
        let rank_ms = t_auth.elapsed().as_millis();
        let t_fuse = std::time::Instant::now();
        let fused = context_rank::fuse_evidence(
            scored,
            context_rank::FuseOptions {
                top_n: opts.max_results,
                query_type: context_rank::types::QueryType::Dependency,
                raw_query: symbol.to_string(),
            },
        );
        let fuse_ms = t_fuse.elapsed().as_millis();
        let t_pack = std::time::Instant::now();
        let packed = context_rank::pack_evidence(
            &fused.ranked,
            symbol,
            context_rank::types::QueryType::Dependency,
            context_rank::PackOptions {
                budget: opts.budget_tokens,
                max_files: opts.max_results,
            },
        );
        let pack_ms = t_pack.elapsed().as_millis();
        let elapsed_ms = t0.elapsed().as_millis();
        let candidate_count = fused.ranked.len() + fused.deduped + fused.collapsed;
        let stats = crate::pipeline::PipelineStats {
            candidate_count,
            evidence_count: fused.ranked.len(),
            files_returned: packed.files.len(),
            packed_tokens: packed.token_estimate,
            retrievers_used: retrievers_used
                .into_iter()
                .chain(vec![
                    format!("authority:{}", rank_ms),
                    format!("fuse:{}", fuse_ms),
                    format!("pack:{}", pack_ms),
                    format!("graph_ms:{}", graph_ms_total),
                    format!("structural_ms:{}", graph_ms_total),
                ])
                .collect(),
            elapsed_ms,
            exact_ms: 0,
            structural_ms: graph_ms_total,
            bm25_ms: 0,
            semantic_ms: 0,
            rank_ms,
            pack_ms,
            total_ms: Some(elapsed_ms),
            discovery_ms: Some(access.discovery_ms),
            reconcile_ms: Some(access.reconcile_ms),
            semantic_embed_ms: None,
            semantic_search_ms: None,
            fusion_ms: Some(fuse_ms),
            authority_ms: Some(rank_ms),
            generation: Some(access.generation),
            dirty_file_count: access.dirty_file_count,
            vector_count_scanned: None,
            cache_hit: None,
            reconcile_skipped: Some(access.reconcile_skipped),
            discovery_calls: Some(access.discovery_calls),
            reconcile_calls: Some(access.reconcile_calls),
            runtime_state: Some(access.runtime_state.to_string()),
        };
        Ok(crate::pipeline::ContextResult {
            query: symbol.to_string(),
            query_type: context_rank::types::QueryType::Dependency,
            evidence: fused.ranked,
            packed,
            stats,
        })
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
        let mut semantic_ref_count = 0usize;
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
                semantic_ref_count = context_index::vector::semantic_ref_count(&conn).unwrap_or(0);
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
        let semantic_index_ready = semantic_backend_available
            && eligible != 0
            && semantic_ref_count == eligible
            && missing == 0;
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
            semantic_ref_count,
            representation_version: context_index::vector::SEMANTIC_REPRESENTATION_VERSION
                .to_string(),
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
        let res = ContextService::new(Some(root.clone())).await;
        assert!(
            res.is_err(),
            "initialization should propagate reconcile failure"
        );
        let err = res
            .err()
            .expect("initialization error")
            .to_string()
            .to_lowercase();
        assert!(
            err.contains("structural build"),
            "error should mention the failed build, got: {}",
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
            std::fs::write(
                root.join(format!("f{i}.py")),
                format!("def foo_{i}():\n    x={i}\n").as_bytes(),
            )
            .unwrap();
        }
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        // Use fast reconcile with counting fake (should embed 0 for cold)
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        struct CountingFake2 {
            inner: context_index::embed::FakeEmbedder,
            calls: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl context_index::embed::Embedder for CountingFake2 {
            fn model_id(&self) -> &str {
                self.inner.model_id()
            }
            fn dimension(&self) -> usize {
                self.inner.dimension()
            }
            fn version(&self) -> &str {
                self.inner.version()
            }
            async fn embed_query(&self, q: &str) -> anyhow::Result<Vec<f32>> {
                self.inner.embed_query(q).await
            }
            async fn embed_documents(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.inner.embed_documents(texts).await
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let fake = Arc::new(CountingFake2 {
            inner: context_index::embed::FakeEmbedder::new("cold-test", 8),
            calls: calls.clone(),
        });
        // fast reconcile should NOT embed all 16 (cold)
        let stats = svc.reconcile_fast_inner(Some(fake.clone())).await.unwrap();
        assert_eq!(
            stats.vectors_created, 0,
            "cold fast reconcile should embed 0"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // status should be not ready (no refs yet)
        let conn = context_index::structural::store::open_db(&root).unwrap();
        let fp = fake.fingerprint();
        assert!(!context_index::vector::is_semantic_ready(&conn, &fp, true).unwrap());
        let eligible = context_index::vector::eligible_chunk_count(&conn).unwrap();
        let refs = context_index::vector::semantic_ref_count(&conn).unwrap();
        assert!(
            refs != eligible
                || context_index::vector::missing_vector_count(&conn, &fp).unwrap() > 0
        );
        // lexical search should still succeed
        let res = svc
            .search("foo_0", crate::service::SearchOptions::default())
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn explicit_full_semantic_index_covers_all() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for i in 0..8 {
            std::fs::write(
                root.join(format!("a{i}.py")),
                format!("def bar_{i}():\n    y={i}\n").as_bytes(),
            )
            .unwrap();
        }
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let fake = Arc::new(context_index::embed::FakeEmbedder::new("explicit-test", 8));
        let stats = svc
            .full_semantic_index_with_embedder(fake.clone())
            .await
            .unwrap();
        assert!(stats.vectors_created > 0);
        let conn = context_index::structural::store::open_db(&root).unwrap();
        let fp = fake.fingerprint();
        assert_eq!(
            context_index::vector::missing_vector_count(&conn, &fp).unwrap(),
            0
        );
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
        svc.full_semantic_index_with_embedder(fake.clone())
            .await
            .unwrap();
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

    #[tokio::test]
    async fn dependency_db_failure_returns_error_not_panic() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join(".context")).unwrap();
        // Make .context/index a file to cause DB open failure
        std::fs::write(root.join(".context/index"), b"not a directory").unwrap();
        let res = ContextService::new(Some(root.clone())).await;
        assert!(
            res.is_err(),
            "service initialization should return DB failure, not panic"
        );
        let err = res
            .err()
            .expect("initialization error")
            .to_string()
            .to_lowercase();
        assert!(
            err.contains("structural build"),
            "error should mention the failed build, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn dependency_dedup_short_qualified_same_callsite() {
        // Verify that storing both short and qualified for same call site does not duplicate final result
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        // Create a TS file with qualified call NestFactory.create()
        std::fs::write(
            root.join("a.ts"),
            b"import { NestFactory } from '@nestjs/core';\nasync function bootstrap(){ await NestFactory.create(null); }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("b.ts"),
            b"export class NestFactory { static create(x:any){} }\n",
        )
        .unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        // Build index
        svc.reconcile().await.unwrap();
        // Query qualified
        let res_q = svc
            .dependency(
                "NestFactory.create",
                Direction::Callers,
                SearchOptions::default(),
            )
            .await
            .unwrap();
        // Query short
        let res_s = svc
            .dependency("create", Direction::Callers, SearchOptions::default())
            .await
            .unwrap();
        // Qualified should find at least a.ts
        assert!(
            res_q.evidence.iter().any(|e| e.file == "a.ts"),
            "qualified should find a.ts"
        );
        // Short should also find a.ts (via short edge)
        assert!(
            res_s.evidence.iter().any(|e| e.file == "a.ts"),
            "short should find a.ts"
        );
        // Check no duplicate same file/line in qualified result
        let mut seen = std::collections::HashSet::new();
        for e in &res_q.evidence {
            let key = format!("{}:{}:{:?}", e.file, e.start_line.unwrap_or(0), e.symbol);
            assert!(
                seen.insert(key.clone()),
                "duplicate evidence for same callsite in qualified result: {}",
                key
            );
        }
        // Also check short result has no duplicate same file/line
        let mut seen2 = std::collections::HashSet::new();
        for e in &res_s.evidence {
            let key = format!("{}:{}:{:?}", e.file, e.start_line.unwrap_or(0), e.symbol);
            assert!(
                seen2.insert(key.clone()),
                "duplicate evidence for same callsite in short result: {}",
                key
            );
        }
    }

    #[tokio::test]
    async fn runtime_initial() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let (d, r) = svc.runtime.counters();
        assert_eq!(d, 1, "initialization should perform one discovery");
        assert_eq!(r, 1, "initialization should perform one reconcile");
        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert!(!res.evidence.is_empty(), "search should find foo");
        let (d2, r2) = svc.runtime.counters();
        assert_eq!(d2, 1, "clean search should not discover again");
        assert_eq!(r2, 1, "clean search should not reconcile again");
    }

    #[tokio::test]
    async fn dependency_api_truthfulness() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("a.py"), b"def foo():\n    bar()\n    baz()\n").unwrap();
        std::fs::write(
            root.join("b.py"),
            b"def bar():\n    pass\ndef baz():\n    pass\n",
        )
        .unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        svc.reconcile().await.unwrap();
        // Callees of foo should include bar and baz
        let res_callees = svc
            .dependency("foo", Direction::Callees, SearchOptions::default())
            .await
            .unwrap();
        let _files_callees: Vec<_> = res_callees
            .evidence
            .iter()
            .map(|e| e.file.clone())
            .collect();
        // Check that at least bar or baz callee is found (via callee_name)
        // Callees relation should be Callee
        assert!(
            res_callees
                .evidence
                .iter()
                .any(|e| e.relation == Some(context_rank::types::EvidenceRelation::Callee)),
            "callees should have Callee relation"
        );
        // Callers of bar should include a.py (foo)
        let res_callers = svc
            .dependency("bar", Direction::Callers, SearchOptions::default())
            .await
            .unwrap();
        assert!(
            res_callers.evidence.iter().any(|e| e.file == "a.py"
                && e.relation == Some(context_rank::types::EvidenceRelation::Caller)),
            "bar callers should include a.py"
        );
        // Both should have both relations marked correctly
        let res_both = svc
            .dependency("foo", Direction::Both, SearchOptions::default())
            .await
            .unwrap();
        let _has_caller = res_both
            .evidence
            .iter()
            .any(|e| e.relation == Some(context_rank::types::EvidenceRelation::Caller));
        let has_callee = res_both
            .evidence
            .iter()
            .any(|e| e.relation == Some(context_rank::types::EvidenceRelation::Callee));
        // foo is a caller of bar/baz, and has no callers itself, so Both for foo should have at least Callees
        assert!(has_callee, "Both should include Callees for foo");
        // Fallback: query non-existent symbol should return Definition fallback, not fabricated caller
        let res_fallback = svc
            .dependency(
                "nonexistent_xyz_123",
                Direction::Callers,
                SearchOptions::default(),
            )
            .await
            .unwrap();
        if !res_fallback.evidence.is_empty() {
            for e in &res_fallback.evidence {
                assert!(
                    e.relation == Some(context_rank::types::EvidenceRelation::Definition)
                        || e.relation == Some(context_rank::types::EvidenceRelation::Unknown),
                    "fallback should be Definition, not Caller/Callee, got {:?}",
                    e.relation
                );
            }
        }
    }

    #[tokio::test]
    async fn clean_searches_keep_counters_unchanged() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let (d0, r0) = svc.runtime.counters();
        let mut first_gen = None;
        for _ in 0..5 {
            let res = svc.search("foo", SearchOptions::default()).await.unwrap();
            assert_eq!(res.stats.discovery_calls, Some(0));
            assert_eq!(res.stats.reconcile_calls, Some(0));
            assert_eq!(res.stats.reconcile_skipped, Some(true));
            assert_eq!(res.stats.runtime_state.as_deref(), Some("clean"));
            assert_eq!(res.stats.dirty_file_count, Some(0));
            first_gen = Some(res.stats.generation.unwrap());
        }
        let (d1, r1) = svc.runtime.counters();
        assert_eq!(
            (d1, r1),
            (d0, r0),
            "repeated clean searches must not increment totals"
        );

        let opts = SearchOptions::default();
        let (ra, rb) = tokio::join!(svc.search("foo", opts.clone()), svc.search("foo", opts));
        let a = ra.unwrap();
        let b = rb.unwrap();
        assert_eq!(a.stats.generation, b.stats.generation);
        assert_eq!(a.stats.generation, Some(first_gen.unwrap()));
        let (d2, r2) = svc.runtime.counters();
        assert_eq!(
            (d2, r2),
            (d0, r0),
            "concurrent clean searches must not increment totals"
        );
    }

    #[tokio::test]
    async fn dirty_modify_reconciles_path_local_once() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        fs::write(root.join("b.py"), b"def bar():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let gen0 = svc.runtime.current_snapshot().unwrap().generation;
        let (d0, r0) = svc.runtime.counters();

        fs::write(root.join("a.py"), b"def foo():\n    return 42\n").unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);

        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res.stats.discovery_calls, Some(0));
        assert_eq!(res.stats.reconcile_calls, Some(1));
        assert_eq!(res.stats.reconcile_skipped, Some(false));
        assert_eq!(res.stats.runtime_state.as_deref(), Some("dirty"));
        assert_eq!(res.stats.dirty_file_count, Some(1));
        let gen1 = res.stats.generation.unwrap();
        assert_eq!(gen1, gen0 + 1, "one-file mutation advances generation once");
        let (d1, r1) = svc.runtime.counters();
        assert_eq!(d1, d0, "dirty path must not run discovery");
        assert_eq!(r1, r0 + 1);

        // A second search (possibly processing a duplicate watcher event for the
        // same path) must not advance generation further.
        let res2 = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res2.stats.generation, Some(gen1));
    }

    #[tokio::test]
    async fn dirty_no_content_change_does_not_advance_generation() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let gen0 = svc.runtime.current_snapshot().unwrap().generation;

        // Rewrite identical content (touch) and mark dirty.
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);

        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res.stats.reconcile_calls, Some(1));
        assert_eq!(res.stats.runtime_state.as_deref(), Some("dirty"));
        assert_eq!(
            res.stats.generation,
            Some(gen0),
            "no-content-change event must not advance generation"
        );
    }

    #[tokio::test]
    async fn dirty_create_is_visible() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let gen0 = svc.runtime.current_snapshot().unwrap().generation;

        fs::write(root.join("c.py"), b"def baz():\n    pass\n").unwrap();
        svc.runtime.tracker.mark_paths(["c.py".to_string()]);

        let res = svc.search("baz", SearchOptions::default()).await.unwrap();
        assert!(
            res.evidence.iter().any(|e| e.file == "c.py"),
            "created file should be visible"
        );
        assert_eq!(res.stats.discovery_calls, Some(0));
        assert_eq!(res.stats.reconcile_calls, Some(1));
        assert_eq!(res.stats.generation, Some(gen0 + 1));
    }

    #[tokio::test]
    async fn dirty_delete_cleans_symbols_bm25_graph() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    bar()\n").unwrap();
        fs::write(root.join("b.py"), b"def bar():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();

        let conn = structural_store::open_db(&root).unwrap();
        assert!(structural_store::find_callers(&conn, "bar")
            .unwrap()
            .iter()
            .any(|e| e.file == "a.py"));
        let bm25_before = context_index::bm25::count_bm25_docs(&conn).unwrap();

        fs::remove_file(root.join("a.py")).unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);
        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res.stats.reconcile_calls, Some(1));

        let conn2 = structural_store::open_db(&root).unwrap();
        assert!(structural_store::find_definitions(&conn2, "foo")
            .unwrap()
            .is_empty());
        assert!(!structural_store::find_callers(&conn2, "bar")
            .unwrap()
            .iter()
            .any(|e| e.file == "a.py"));
        assert!(!structural_store::list_files(&conn2)
            .unwrap()
            .iter()
            .any(|(f, _)| f == "a.py"));
        let bm25_after = context_index::bm25::count_bm25_docs(&conn2).unwrap();
        assert!(
            bm25_after < bm25_before,
            "BM25 rows for deleted file should disappear"
        );
    }

    #[tokio::test]
    async fn dirty_semantic_ready_modify_embeds_only_changed() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    return 1\n").unwrap();
        fs::write(root.join("b.py"), b"def bar():\n    return 2\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();

        let fake_full = Arc::new(context_index::embed::FakeEmbedder::new("sem-ready", 8));
        svc.full_semantic_index_with_embedder(fake_full.clone())
            .await
            .unwrap();
        let fp = fake_full.fingerprint();
        let conn = structural_store::open_db(&root).unwrap();
        assert!(context_index::vector::is_semantic_ready(&conn, &fp, true).unwrap());
        let vectors_before = context_index::vector::count_vectors(&conn, &fp).unwrap();

        struct CountingEmbedder {
            inner: context_index::embed::FakeEmbedder,
            calls: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl context_index::embed::Embedder for CountingEmbedder {
            fn model_id(&self) -> &str {
                self.inner.model_id()
            }
            fn dimension(&self) -> usize {
                self.inner.dimension()
            }
            fn version(&self) -> &str {
                self.inner.version()
            }
            async fn embed_query(&self, q: &str) -> anyhow::Result<Vec<f32>> {
                self.inner.embed_query(q).await
            }
            async fn embed_documents(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.inner.embed_documents(texts).await
            }
        }

        fs::write(root.join("a.py"), b"def foo():\n    return 999\n").unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);
        let calls = Arc::new(AtomicUsize::new(0));
        let counting = Arc::new(CountingEmbedder {
            inner: context_index::embed::FakeEmbedder::new("sem-ready", 8),
            calls: calls.clone(),
        });
        let res = svc
            .search_with_embedder("foo", SearchOptions::default(), counting)
            .await
            .unwrap();
        assert_eq!(res.stats.reconcile_calls, Some(1));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "only the changed file's representations should embed"
        );

        let conn2 = structural_store::open_db(&root).unwrap();
        assert!(context_index::vector::is_semantic_ready(&conn2, &fp, true).unwrap());
        assert_eq!(
            context_index::vector::missing_vector_count(&conn2, &fp).unwrap(),
            0
        );
        let vectors_after = context_index::vector::count_vectors(&conn2, &fp).unwrap();
        assert!(
            vectors_after >= vectors_before,
            "unrelated vectors must remain"
        );

        // Delete a.py: semantic refs disappear, orphan vectors GC'd (F5), remaining files stay ready.
        fs::remove_file(root.join("a.py")).unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);
        let _ = svc.search("foo", SearchOptions::default()).await.unwrap();
        let conn3 = structural_store::open_db(&root).unwrap();
        assert!(context_index::vector::is_semantic_ready(&conn3, &fp, true).unwrap());
        let vectors_after_delete = context_index::vector::count_vectors(&conn3, &fp).unwrap();
        assert!(
            vectors_after_delete < vectors_after,
            "orphan vectors for the deleted file must be GC'd: {} -> {}",
            vectors_after,
            vectors_after_delete
        );
    }

    #[tokio::test]
    async fn unknown_fallback_full_reconcile() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let (d0, r0) = svc.runtime.counters();

        svc.runtime.tracker.mark_unknown();
        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res.stats.discovery_calls, Some(1));
        assert_eq!(res.stats.reconcile_calls, Some(1));
        assert_eq!(res.stats.runtime_state.as_deref(), Some("unknown"));
        assert_eq!(
            res.stats.dirty_file_count, None,
            "Unknown must report null dirty_file_count, not a fake zero"
        );
        let (d1, r1) = svc.runtime.counters();
        assert_eq!(d1, d0 + 1);
        assert_eq!(r1, r0 + 1);
    }

    #[tokio::test]
    async fn public_reconcile_forces_full_verification() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let (d0, r0) = svc.runtime.counters();

        let stats = svc.reconcile().await.unwrap();
        assert!(stats.discovered > 0);
        let (d1, r1) = svc.runtime.counters();
        assert_eq!(d1, d0 + 1);
        assert_eq!(r1, r0 + 1);
    }

    #[tokio::test]
    async fn verification_expiry_triggers_unknown_path_once() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();
        let now = Instant::now();
        svc.runtime.set_last_full_verified(
            now - crate::runtime::FULL_VERIFY_INTERVAL - std::time::Duration::from_secs(1),
        );
        assert!(svc.runtime.is_verification_expired(now));

        let (d0, r0) = svc.runtime.counters();
        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res.stats.discovery_calls, Some(1));
        assert_eq!(res.stats.runtime_state.as_deref(), Some("unknown"));
        let (d1, r1) = svc.runtime.counters();
        assert_eq!(d1, d0 + 1);
        assert_eq!(r1, r0 + 1);

        let res2 = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res2.stats.reconcile_skipped, Some(true));
        let (d2, r2) = svc.runtime.counters();
        assert_eq!(
            (d2, r2),
            (d1, r1),
            "post-expiry clean search must not re-discover"
        );
    }

    #[tokio::test]
    async fn event_during_reconcile_remains_dirty() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        fs::write(root.join("b.py"), b"def bar():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();

        fs::write(root.join("a.py"), b"def foo():\n    return 1\n").unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);

        let tracker = svc.runtime.tracker.clone();
        svc.runtime.set_test_hook(Box::new(move || {
            tracker.mark_paths(["b.py".to_string()]);
        }));

        let _ = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(
            svc.runtime.state(),
            RuntimeState::Dirty,
            "event arriving during reconcile must leave runtime dirty"
        );

        let _ = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(svc.runtime.state(), RuntimeState::Clean);
    }

    #[tokio::test]
    async fn event_between_snapshot_and_reconcile_remains_dirty() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        fs::write(root.join("b.py"), b"def bar():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();

        fs::write(root.join("a.py"), b"def foo():\n    return 1\n").unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);

        // Inject a second event in the F1 window: after `dirty_access()` captured
        // {a.py}+epoch, but before the path-local reconcile acknowledges.
        let tracker = svc.runtime.tracker.clone();
        svc.runtime.set_pre_reconcile_hook(Box::new(move || {
            tracker.mark_paths(["b.py".to_string()]);
        }));

        let _ = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(
            svc.runtime.state(),
            RuntimeState::Dirty,
            "event between snapshot capture and reconcile must not be acknowledged away"
        );

        // The second path is reconciled on the next request.
        let _ = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(svc.runtime.state(), RuntimeState::Clean);
    }

    #[tokio::test]
    async fn dirty_path_local_preserves_verification_deadline() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();

        // Non-expired deadline: 10s remaining.
        let base = Instant::now() - crate::runtime::FULL_VERIFY_INTERVAL
            + std::time::Duration::from_secs(10);
        svc.runtime.set_last_full_verified(base);
        let now = Instant::now();
        assert!(
            !svc.runtime.is_verification_expired(now),
            "deadline should not be expired yet"
        );

        fs::write(root.join("a.py"), b"def foo():\n    return 2\n").unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);

        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(res.stats.discovery_calls, Some(0));
        assert_eq!(res.stats.reconcile_calls, Some(1));
        assert_eq!(res.stats.runtime_state.as_deref(), Some("dirty"));
        assert_eq!(res.stats.dirty_file_count, Some(1));

        // Path-local publish must not refresh the full-verification deadline.
        // Synthetic instant just beyond the original deadline distinguishes
        // preservation (expired) from a reset to now (not expired).
        let synthetic =
            base + crate::runtime::FULL_VERIFY_INTERVAL + std::time::Duration::from_secs(1);
        assert!(
            svc.runtime.is_verification_expired(synthetic),
            "path-local publish must not reset the full-verification deadline"
        );
        assert!(
            !svc.runtime.is_verification_expired(now),
            "still not expired at the original now"
        );
    }

    #[tokio::test]
    async fn dirty_expired_triggers_full_reconcile() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.py"), b"def foo():\n    pass\n").unwrap();
        let svc = ContextService::new(Some(root.clone())).await.unwrap();

        let now = Instant::now();
        svc.runtime.set_last_full_verified(
            now - crate::runtime::FULL_VERIFY_INTERVAL - std::time::Duration::from_secs(1),
        );
        assert!(svc.runtime.is_verification_expired(now));

        fs::write(root.join("a.py"), b"def foo():\n    return 2\n").unwrap();
        svc.runtime.tracker.mark_paths(["a.py".to_string()]);

        let res = svc.search("foo", SearchOptions::default()).await.unwrap();
        assert_eq!(
            res.stats.discovery_calls,
            Some(1),
            "dirty+expired must perform exactly one full discovery"
        );
        assert_eq!(res.stats.reconcile_calls, Some(1));
        assert_eq!(res.stats.runtime_state.as_deref(), Some("unknown"));
        assert_eq!(
            res.stats.dirty_file_count, None,
            "expired fallback must report null dirty_file_count"
        );

        assert!(
            !svc.runtime.is_verification_expired(Instant::now()),
            "full discovery must clear the expired deadline"
        );
    }
}
