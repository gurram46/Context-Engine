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
    #[cfg(test)]
    test_hook: std::sync::Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSnapshot {
    pub project: Arc<ProjectIndex>,
    pub generation: u64,
    #[allow(dead_code)]
    pub semantic_fingerprint: ModelFingerprint,
    #[allow(dead_code)]
    pub last_full_verified: Option<Instant>,
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
            #[cfg(test)]
            test_hook: std::sync::Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn state(&self) -> RuntimeState {
        match self.tracker.snapshot().state {
            context_index::watcher::DirtyState::Clean => RuntimeState::Clean,
            context_index::watcher::DirtyState::Paths(_) => RuntimeState::Dirty,
            context_index::watcher::DirtyState::Unknown => RuntimeState::Unknown,
        }
    }

    pub(crate) fn dirty_paths(&self) -> Option<std::collections::BTreeSet<String>> {
        match self.tracker.snapshot().state {
            context_index::watcher::DirtyState::Paths(paths) => Some(paths),
            _ => None,
        }
    }

    pub(crate) fn current_snapshot(&self) -> Option<RuntimeSnapshot> {
        let guard = self.data.lock().ok()?;
        let project = guard.project.clone()?;
        Some(RuntimeSnapshot {
            project,
            generation: guard.generation,
            semantic_fingerprint: guard.semantic_fingerprint.clone(),
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
    pub(crate) fn publish(
        &self,
        project: Arc<ProjectIndex>,
        generation: u64,
        fingerprint: ModelFingerprint,
        now: Instant,
    ) {
        let mut guard = self.data.lock().expect("runtime data mutex poisoned");
        guard.project = Some(project);
        guard.generation = generation;
        guard.semantic_fingerprint = fingerprint;
        guard.last_full_verified = Some(now);
        guard.reconcile_total = guard.reconcile_total.saturating_add(1);
    }

    pub(crate) fn increment_discovery(&self) {
        let mut guard = self.data.lock().expect("runtime data mutex poisoned");
        guard.discovery_total = guard.discovery_total.saturating_add(1);
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
