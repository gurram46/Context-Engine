use std::collections::BTreeSet;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use context_index::embed::ModelFingerprint;
use context_index::watcher::{DirtyTracker, RepositoryWatcher};
use context_index::ProjectIndex;

pub(crate) const FULL_VERIFY_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeState {
    Unknown,
    Clean,
    Dirty,
}

pub(crate) struct RuntimeData {
    project: Option<Arc<ProjectIndex>>,
    generation: u64,
    semantic_fingerprint: ModelFingerprint,
    last_full_verified: Option<Instant>,
    discovery_total: u64,
    reconcile_total: u64,
}

pub(crate) struct RepositoryRuntime {
    #[allow(dead_code)]
    root: PathBuf,
    pub(crate) tracker: DirtyTracker,
    _watcher: RepositoryWatcher,
    data: std::sync::Mutex<RuntimeData>,
    pub(crate) reconcile_lock: tokio::sync::Mutex<()>,
    hot: std::sync::RwLock<Option<Arc<crate::hot::HotState>>>,
    #[cfg(test)]
    test_hook: std::sync::Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pre_reconcile_hook: std::sync::Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSnapshot {
    pub project: Arc<ProjectIndex>,
    pub generation: u64,
    #[allow(dead_code)]
    pub last_full_verified: Option<Instant>,
}

/// Single atomic view of dirty state, captured paths, and epoch. Derived from one
/// `DirtyTracker::snapshot()` so `state`, `paths`, and `epoch` can never disagree.
#[derive(Debug, Clone)]
pub(crate) struct DirtyAccess {
    pub state: RuntimeState,
    pub paths: Option<BTreeSet<String>>,
    pub epoch: u64,
}

impl RepositoryRuntime {
    pub(crate) fn new(root: PathBuf) -> Result<Self, context_core::ContextError> {
        let watcher = RepositoryWatcher::new(root.clone())
            .map_err(|e| context_core::ContextError::Internal(format!("watcher failed: {e}")))?;
        let tracker = watcher.tracker().clone();
        tracker.mark_unknown();
        Ok(Self {
            root,
            tracker,
            _watcher: watcher,
            data: std::sync::Mutex::new(RuntimeData {
                project: None,
                generation: 0,
                semantic_fingerprint: context_index::embed::configured_fingerprint(),
                last_full_verified: None,
                discovery_total: 0,
                reconcile_total: 0,
            }),
            reconcile_lock: tokio::sync::Mutex::new(()),
            hot: std::sync::RwLock::new(None),
            #[cfg(test)]
            test_hook: std::sync::Mutex::new(None),
            #[cfg(test)]
            pre_reconcile_hook: std::sync::Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> RuntimeState {
        match self.tracker.snapshot().state {
            context_index::watcher::DirtyState::Clean => RuntimeState::Clean,
            context_index::watcher::DirtyState::Paths(_) => RuntimeState::Dirty,
            context_index::watcher::DirtyState::Unknown => RuntimeState::Unknown,
        }
    }

    /// Capture dirty state, paths, and epoch in a single tracker snapshot so a
    /// concurrent watcher event cannot split them (the source of the F1/F2 race).
    pub(crate) fn dirty_access(&self) -> DirtyAccess {
        let snap = self.tracker.snapshot();
        let (state, paths) = match snap.state {
            context_index::watcher::DirtyState::Clean => (RuntimeState::Clean, None),
            context_index::watcher::DirtyState::Paths(paths) => (RuntimeState::Dirty, Some(paths)),
            context_index::watcher::DirtyState::Unknown => (RuntimeState::Unknown, None),
        };
        DirtyAccess {
            state,
            paths,
            epoch: snap.epoch,
        }
    }

    pub(crate) fn semantic_fingerprint(&self) -> ModelFingerprint {
        let guard = match self.data.lock() {
            Ok(g) => g,
            Err(_) => return context_index::embed::configured_fingerprint(),
        };
        guard.semantic_fingerprint.clone()
    }

    pub(crate) fn current_snapshot(&self) -> Option<RuntimeSnapshot> {
        let guard = self.data.lock().ok()?;
        let project = guard.project.clone()?;
        Some(RuntimeSnapshot {
            project,
            generation: guard.generation,
            last_full_verified: guard.last_full_verified,
        })
    }

    pub(crate) fn is_verification_expired(&self, now: Instant) -> bool {
        let guard = match self.data.lock() {
            Ok(g) => g,
            Err(_) => return true,
        };
        match guard.last_full_verified {
            Some(t) => now
                .checked_duration_since(t)
                .map(|elapsed| elapsed >= FULL_VERIFY_INTERVAL)
                .unwrap_or(false),
            None => true,
        }
    }

    /// Publish a validated project snapshot. Must be called while holding
    /// `reconcile_lock` (or during single-threaded initialization) so that
    /// the counters stay consistent with the work that produced the snapshot.
    ///
    /// `full_verification` is true only when the snapshot verified the whole
    /// repository (initial/full discovery). Incremental dirty-path publishes
    /// must preserve the existing `last_full_verified` deadline, otherwise a
    /// stream of single-file edits would postpone periodic full verification
    /// indefinitely.
    pub(crate) fn publish(
        &self,
        project: Arc<ProjectIndex>,
        generation: u64,
        fingerprint: ModelFingerprint,
        now: Instant,
        full_verification: bool,
    ) {
        let mut guard = self.data.lock().expect("runtime data mutex poisoned");
        guard.project = Some(project);
        guard.generation = generation;
        guard.semantic_fingerprint = fingerprint.clone();
        if full_verification {
            guard.last_full_verified = Some(now);
        }
        guard.reconcile_total = guard.reconcile_total.saturating_add(1);
        drop(guard);
        // Invalidate hot retrieval state on generation/fingerprint change (E3-D)
        // Hot is generation+fingerprint bound; stale hot must not be reused.
        // Invalidate hot retrieval state on any publish to avoid stale BM25/vectors
        // after incremental or full reconciliation. Next clean query will lazily rebuild.
        // This ensures generation-bound isolation and no stale results after mutation.
        let _ = self.hot.write().map(|mut w| *w = None);
    }

    pub(crate) fn increment_discovery(&self) {
        let mut guard = self.data.lock().expect("runtime data mutex poisoned");
        guard.discovery_total = guard.discovery_total.saturating_add(1);
    }

    #[allow(dead_code)]
    pub(crate) fn invalidate_hot(&self) {
        if let Ok(mut guard) = self.hot.write() {
            *guard = None;
        }
    }

    pub(crate) async fn get_or_load_hot(
        &self,
        generation: u64,
        fingerprint: ModelFingerprint,
    ) -> Option<Arc<crate::hot::HotState>> {
        // Fast read path
        if let Ok(guard) = self.hot.read() {
            if let Some(hot) = guard.clone() {
                if hot.generation == generation && hot.fingerprint == fingerprint {
                    return Some(hot);
                }
            }
        }
        // Need to load — do blocking load outside lock
        let root = self.root.clone();
        let fp_clone = fingerprint.clone();
        let loaded = tokio::task::spawn_blocking(move || {
            crate::hot::HotState::load_blocking(&root, generation, fp_clone)
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(Arc::new);
        if let Some(hot) = loaded.clone() {
            // Write back, but check again for race
            if let Ok(mut w) = self.hot.write() {
                if let Some(existing) = w.clone() {
                    if existing.generation == generation && existing.fingerprint == fingerprint {
                        return Some(existing);
                    }
                }
                *w = Some(hot.clone());
            }
            return Some(hot);
        }
        None
    }

    #[allow(dead_code)]
    pub(crate) async fn peek_hot(
        &self,
        generation: u64,
        fingerprint: &ModelFingerprint,
    ) -> Option<Arc<crate::hot::HotState>> {
        if let Ok(guard) = self.hot.read() {
            if let Some(hot) = guard.clone() {
                if hot.generation == generation && hot.fingerprint == *fingerprint {
                    return Some(hot);
                }
            }
        }
        None
    }

    #[cfg(test)]
    pub fn counters(&self) -> (u64, u64) {
        let guard = self.data.lock().expect("runtime data mutex poisoned");
        (guard.discovery_total, guard.reconcile_total)
    }

    #[cfg(test)]
    pub fn set_last_full_verified(&self, instant: Instant) {
        let mut guard = self.data.lock().expect("runtime data mutex poisoned");
        guard.last_full_verified = Some(instant);
    }

    #[cfg(test)]
    pub(crate) fn set_test_hook(&self, hook: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut guard) = self.test_hook.lock() {
            *guard = Some(hook);
        }
    }

    #[cfg(test)]
    pub(crate) fn run_test_hook(&self) {
        let hook = match self.test_hook.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Test hook fired between the dirty-path snapshot capture and the start of
    /// the path-local reconcile. Lets a test inject a watcher event in the exact
    /// F1 window (after `dirty_access()` captures paths+epoch, before reconcile).
    #[cfg(test)]
    pub(crate) fn set_pre_reconcile_hook(&self, hook: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut guard) = self.pre_reconcile_hook.lock() {
            *guard = Some(hook);
        }
    }

    #[cfg(test)]
    pub(crate) fn run_pre_reconcile_hook(&self) {
        let hook = match self.pre_reconcile_hook.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some(hook) = hook {
            hook();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context_index::watcher::DirtyState;
    use std::time::Instant;
    use tempfile::TempDir;

    #[test]
    fn initial_state_is_unknown() {
        let tmp = TempDir::new().unwrap();
        let rt = RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap();
        assert_eq!(rt.state(), RuntimeState::Unknown);
        assert!(rt.current_snapshot().is_none());
        assert_eq!(rt.counters(), (0, 0));
    }

    #[test]
    fn publish_and_ack_cleans_state() {
        let tmp = TempDir::new().unwrap();
        let rt = RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap();
        let epoch = rt.tracker.snapshot().epoch;
        let root = context_index::ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let idx = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(
            Arc::new(idx),
            1,
            ModelFingerprint {
                model_id: "m".into(),
                version: "v".into(),
                dimension: 1,
            },
            Instant::now(),
            true,
        );
        rt.tracker.acknowledge(epoch);
        assert_eq!(rt.state(), RuntimeState::Clean);
        assert_eq!(rt.counters(), (0, 1));
        assert!(rt.current_snapshot().is_some());
    }

    #[test]
    fn epoch_mismatch_retains_dirty() {
        let tmp = TempDir::new().unwrap();
        let rt = RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap();
        let initial_epoch = rt.tracker.snapshot().epoch;
        let root = context_index::ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let idx = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(
            Arc::new(idx),
            1,
            ModelFingerprint {
                model_id: "m".into(),
                version: "v".into(),
                dimension: 1,
            },
            Instant::now(),
            true,
        );
        rt.tracker.acknowledge(initial_epoch);
        rt.tracker.mark_paths(["a.py".to_string()]);
        let snapshot = rt.tracker.snapshot();
        rt.tracker.mark_paths(["b.py".to_string()]);
        rt.tracker.acknowledge(snapshot.epoch);
        assert_eq!(rt.state(), RuntimeState::Dirty);
        assert!(matches!(rt.tracker.snapshot().state, DirtyState::Paths(_)));
    }

    #[test]
    fn root_identity() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let rt = RepositoryRuntime::new(root.clone()).unwrap();
        assert_eq!(rt.root(), root);
    }

    #[test]
    fn verification_expiry() {
        let tmp = TempDir::new().unwrap();
        let rt = RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap();
        let now = Instant::now();
        rt.set_last_full_verified(now - FULL_VERIFY_INTERVAL - Duration::from_secs(1));
        assert!(rt.is_verification_expired(now));
        rt.set_last_full_verified(now);
        assert!(!rt.is_verification_expired(now));
    }
}
