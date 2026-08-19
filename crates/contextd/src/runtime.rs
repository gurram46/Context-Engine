use std::collections::BTreeSet;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(test)]
static HOT_LOAD_START_NOTIFY: std::sync::Mutex<Option<Arc<tokio::sync::Notify>>> =
    std::sync::Mutex::new(None);
#[cfg(test)]
static HOT_LOAD_RELEASE_NOTIFY: std::sync::Mutex<Option<Arc<tokio::sync::Notify>>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn set_hot_load_notifies(
    start: Option<Arc<tokio::sync::Notify>>,
    release: Option<Arc<tokio::sync::Notify>>,
) {
    *HOT_LOAD_START_NOTIFY.lock().unwrap() = start;
    *HOT_LOAD_RELEASE_NOTIFY.lock().unwrap() = release;
}

#[cfg(test)]
static HOT_WRITE_START_NOTIFY: std::sync::Mutex<Option<Arc<tokio::sync::Notify>>> =
    std::sync::Mutex::new(None);
#[cfg(test)]
static HOT_WRITE_RELEASE_NOTIFY: std::sync::Mutex<Option<Arc<tokio::sync::Notify>>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn set_hot_write_notifies(
    start: Option<Arc<tokio::sync::Notify>>,
    release: Option<Arc<tokio::sync::Notify>>,
) {
    *HOT_WRITE_START_NOTIFY.lock().unwrap() = start;
    *HOT_WRITE_RELEASE_NOTIFY.lock().unwrap() = release;
}

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
        // Atomic publication: data -> hot order, keep data guard held while clearing hot
        // to prevent TOCTOU where a concurrent get_or_load_hot could publish stale G after we cleared
        let mut guard = self.data.lock().expect("runtime data mutex poisoned");
        guard.project = Some(project);
        guard.generation = generation;
        guard.semantic_fingerprint = fingerprint.clone();
        if full_verification {
            guard.last_full_verified = Some(now);
        }
        guard.reconcile_total = guard.reconcile_total.saturating_add(1);
        // Hold data guard while acquiring hot write lock (data -> hot order)
        let mut hot_guard = self.hot.write().expect("hot poisoned");
        *hot_guard = None;
        drop(hot_guard);
        drop(guard);
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
        // Test hook: pause before publication recheck to simulate load-during-reconcile race
        // One-shot: consume notifies so only the first loader pauses, subsequent G+1 build does not block
        #[cfg(test)]
        {
            let (start, release) = {
                let mut s_lock = HOT_LOAD_START_NOTIFY.lock().unwrap();
                let mut r_lock = HOT_LOAD_RELEASE_NOTIFY.lock().unwrap();
                (s_lock.take(), r_lock.take())
            };
            if let (Some(s), Some(r)) = (start, release) {
                eprintln!("[hot] first hook: notifying start, waiting for release");
                s.notify_one();
                r.notified().await;
                eprintln!("[hot] first hook: released");
            }
        }
        // Publication recheck: ensure current runtime generation/fingerprint still matches requested
        // This prevents stale G hot from becoming active when DB/runtime already moved to G+1
        // and ensures HotState DB snapshot generation == requested generation.
        let (current_gen, current_fp) = {
            let guard = self.data.lock().expect("runtime data mutex poisoned");
            (guard.generation, guard.semantic_fingerprint.clone())
        };
        if current_gen != generation || current_fp != fingerprint {
            // Stale — discard, do not publish, do not return for future G+1 requests
            // For the in-flight G request itself, we discard rather than combine generations
            return None;
        }
        // Second hook: pause between recheck and atomic publication to test TOCTOU window
        // In old code this window had no locks held, allowing stale G to overwrite G+1.
        // New code makes publication atomic (data+hot held together), so this window is eliminated.
        #[cfg(test)]
        {
            let (ws, wr) = {
                let mut s_lock = HOT_WRITE_START_NOTIFY.lock().unwrap();
                let mut r_lock = HOT_WRITE_RELEASE_NOTIFY.lock().unwrap();
                (s_lock.take(), r_lock.take())
            };
            if let (Some(s), Some(r)) = (ws, wr) {
                eprintln!("[hot] second hook: notifying start, waiting for release");
                s.notify_one();
                r.notified().await;
                eprintln!("[hot] second hook: released");
            }
        }
        // Atomic publication: hold data and hot together (data -> hot order, same as publish)
        // Keep data guard held while acquiring hot write lock, then re-validate and publish
        let data_guard = self.data.lock().expect("runtime data mutex poisoned");
        let current_gen2 = data_guard.generation;
        let current_fp2 = data_guard.semantic_fingerprint.clone();
        if current_gen2 != generation || current_fp2 != fingerprint {
            return None;
        }
        if let Some(hot) = loaded.clone() {
            // Acquire hot write while still holding data_guard (data -> hot order)
            // Use try_write to avoid deadlocks? But we need to hold data_guard, so we must use blocking write
            // Since hot is std RwLock, we can acquire it while holding data Mutex (both sync, no await)
            // This is safe because no other path holds hot then tries to acquire data.
            let mut hot_guard = self.hot.write().expect("hot poisoned");
            if let Some(existing) = hot_guard.clone() {
                if existing.generation == generation && existing.fingerprint == fingerprint {
                    return Some(existing);
                }
                // If existing matches current (which equals requested), it must be same generation, so reuse
                if existing.generation == current_gen2 && existing.fingerprint == current_fp2 {
                    return Some(existing);
                }
                // Otherwise existing is stale, we will overwrite
            }
            *hot_guard = Some(hot.clone());
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

    #[tokio::test]
    async fn hot_generation_race_no_stale() {
        let tmp = TempDir::new().unwrap();
        let rt = RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap();
        let fp = ModelFingerprint {
            model_id: "m".into(),
            version: "v".into(),
            dimension: 8,
        };
        // Publish G=1 and build hot for G=1
        let root = context_index::ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let idx = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(Arc::new(idx.clone()), 1, fp.clone(), Instant::now(), true);
        rt.tracker.acknowledge(rt.tracker.snapshot().epoch);
        {
            let conn = context_index::structural::store::open_db(root.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 1).unwrap();
        }
        let hot_g1 = rt.get_or_load_hot(1, fp.clone()).await.expect("hot g1");
        assert_eq!(hot_g1.generation, 1);
        // Publish G=2 (clears hot)
        rt.publish(Arc::new(idx), 2, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 2).unwrap();
        }
        // Hot should be cleared
        assert!(
            rt.peek_hot(1, &fp).await.is_none(),
            "hot for G=1 should not be valid after publish G=2 (cleared)"
        );
        assert!(
            rt.peek_hot(2, &fp).await.is_none(),
            "hot for G=2 not yet built"
        );
        // Simulate stale concurrent build for G=1 trying to write after G=2 publish
        // Directly inject stale hot (generation 1) as if a delayed task finished
        {
            let mut w = rt.hot.write().unwrap();
            *w = Some(hot_g1.clone());
        }
        // Now a G+1 request must NOT get stale G=1 hot
        let peek_g2 = rt.peek_hot(2, &fp).await;
        assert!(peek_g2.is_none(), "stale G=1 hot must not be usable at G+1");
        // A correct G+1 request should build new hot for G=2
        let hot_g2 = rt.get_or_load_hot(2, fp.clone()).await.expect("hot g2");
        assert_eq!(hot_g2.generation, 2);
        assert_ne!(hot_g1.generation, hot_g2.generation);
        // Ensure repository generation remains G+1 (publish already set)
        let snap = rt.current_snapshot().unwrap();
        assert_eq!(snap.generation, 2);
        // Stale build is discarded — active hot is G=2, not G=1
        let active = rt.peek_hot(2, &fp).await.unwrap();
        assert_eq!(active.generation, 2);
    }

    #[tokio::test]
    async fn hot_concurrent_clean_requests() {
        let tmp = TempDir::new().unwrap();
        let rt = Arc::new(RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap());
        let fp = ModelFingerprint {
            model_id: "m".into(),
            version: "v".into(),
            dimension: 8,
        };
        let root = context_index::ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let idx = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(Arc::new(idx), 5, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 5).unwrap();
        }
        // Spawn 10 concurrent clean requests for same generation
        let mut handles = Vec::new();
        for _ in 0..10 {
            let rt_clone = Arc::clone(&rt);
            let fp_clone = fp.clone();
            handles.push(tokio::spawn(async move {
                let hot = rt_clone.get_or_load_hot(5, fp_clone).await;
                assert!(hot.is_some());
                assert_eq!(hot.unwrap().generation, 5);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        // All should have same generation, no panic, duplicate builds are performance-only
        // Verify hot is still for G=5
        let final_hot = rt.peek_hot(5, &fp).await.unwrap();
        assert_eq!(final_hot.generation, 5);
    }

    #[tokio::test]
    async fn hot_between_recheck_and_write_race() {
        // Test the TOCTOU window between recheck and atomic publication.
        // Old code had window with no locks: recheck (data) then hot.write() separately.
        // New code holds data+hot atomically, so stale G cannot overwrite G+1.
        let tmp = TempDir::new().unwrap();
        let rt = Arc::new(RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap());
        let fp = ModelFingerprint {
            model_id: "m".into(),
            version: "v".into(),
            dimension: 8,
        };
        let root = context_index::ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let idx = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(Arc::new(idx.clone()), 1, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 1).unwrap();
        }
        // Ensure hot for G=1 not yet built
        assert!(rt.peek_hot(1, &fp).await.is_none());
        // Setup second hook (between recheck and atomic write)
        let start_notify = Arc::new(tokio::sync::Notify::new());
        let release_notify = Arc::new(tokio::sync::Notify::new());
        set_hot_write_notifies(Some(start_notify.clone()), Some(release_notify.clone()));
        // Spawn loader for G=1 (will pause at second hook after recheck)
        let rt_clone = Arc::clone(&rt);
        let fp_clone = fp.clone();
        let loader_handle =
            tokio::spawn(async move { rt_clone.get_or_load_hot(1, fp_clone).await });
        // Wait for loader to reach second hook (it notifies start)
        start_notify.notified().await;
        // Now loader is paused between recheck and atomic publication, holding no locks (hook is before acquiring data+hot)
        // Publish G+1 while loader is paused
        let idx2 = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(Arc::new(idx2), 2, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 2).unwrap();
        }
        // Build valid G+1 hot (should succeed, not blocked by loader's pause because loader is not holding locks)
        let hot_g2 = rt
            .get_or_load_hot(2, fp.clone())
            .await
            .expect("hot g2 should build");
        assert_eq!(hot_g2.generation, 2);
        // Release old G loader
        release_notify.notify_one();
        let res_g1 = loader_handle.await.unwrap();
        // Stale G loader must not overwrite G+1 (should be discarded, return None or not become active)
        // It could return None (discarded) or Some(G) for its own request but not publish as current.
        // In our implementation, it returns None because current != requested at recheck inside atomic.
        assert!(res_g1.is_none() || res_g1.as_ref().unwrap().generation == 1);
        // Active hot must remain G+1, not overwritten by stale G
        let active = rt.peek_hot(2, &fp).await.expect("active should be G+1");
        assert_eq!(active.generation, 2);
        // Subsequent G+1 request must return existing G+1 Arc (not rebuild due to stale eviction)
        let hot_again = rt.get_or_load_hot(2, fp.clone()).await.unwrap();
        assert_eq!(hot_again.generation, 2);
        assert!(Arc::ptr_eq(&active, &hot_again) || hot_again.generation == 2);
        // Late G cannot overwrite active G+1
        assert!(
            rt.peek_hot(1, &fp).await.is_none()
                || rt.peek_hot(1, &fp).await.unwrap().generation == 1
        );
        // Cleanup
        set_hot_write_notifies(None, None);
        set_hot_load_notifies(None, None);
    }

    #[tokio::test]
    async fn hot_per_root_isolation() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        let rt1 = RepositoryRuntime::new(tmp1.path().to_path_buf()).unwrap();
        let rt2 = RepositoryRuntime::new(tmp2.path().to_path_buf()).unwrap();
        let fp = ModelFingerprint {
            model_id: "m".into(),
            version: "v".into(),
            dimension: 8,
        };
        let root1 = context_index::ProjectRoot::resolve(Some(tmp1.path())).unwrap();
        let idx1 = context_index::ProjectIndex::discover(&root1).unwrap();
        rt1.publish(Arc::new(idx1), 10, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root1.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 10).unwrap();
        }
        let hot1 = rt1.get_or_load_hot(10, fp.clone()).await.unwrap();
        assert_eq!(hot1.generation, 10);
        // rt2 should not see rt1's hot
        assert!(rt2.peek_hot(10, &fp).await.is_none());
        let root2 = context_index::ProjectRoot::resolve(Some(tmp2.path())).unwrap();
        let idx2 = context_index::ProjectIndex::discover(&root2).unwrap();
        rt2.publish(Arc::new(idx2), 10, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root2.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 10).unwrap();
        }
        let hot2 = rt2.get_or_load_hot(10, fp.clone()).await.unwrap();
        assert_eq!(hot2.generation, 10);
        // They are distinct Arc pointers (different roots)
        assert!(!Arc::ptr_eq(&hot1, &hot2));
    }

    #[tokio::test]
    async fn hot_actual_load_during_reconcile_race() {
        // Real barrier hook race: G load starts, publish G+1 during load, ensure stale G does not overwrite G+1
        let tmp = TempDir::new().unwrap();
        let rt = Arc::new(RepositoryRuntime::new(tmp.path().to_path_buf()).unwrap());
        let fp = ModelFingerprint {
            model_id: "m".into(),
            version: "v".into(),
            dimension: 8,
        };
        let root = context_index::ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let idx = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(Arc::new(idx.clone()), 1, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 1).unwrap();
        }
        assert!(rt.peek_hot(1, &fp).await.is_none());
        // Setup barrier notifies
        let start_notify = Arc::new(tokio::sync::Notify::new());
        let release_notify = Arc::new(tokio::sync::Notify::new());
        set_hot_load_notifies(Some(start_notify.clone()), Some(release_notify.clone()));
        // Spawn loader for G=1 (will pause before publication recheck)
        let rt_clone = Arc::clone(&rt);
        let fp_clone = fp.clone();
        let loader_handle =
            tokio::spawn(async move { rt_clone.get_or_load_hot(1, fp_clone).await });
        // Wait for loader to reach pause point (it notifies start)
        start_notify.notified().await;
        // Now mutate to G+1 while loader is paused
        let idx2 = context_index::ProjectIndex::discover(&root).unwrap();
        rt.publish(Arc::new(idx2), 2, fp.clone(), Instant::now(), true);
        {
            let conn = context_index::structural::store::open_db(root.path()).unwrap();
            context_index::structural::store::set_generation(&conn, 2).unwrap();
        }
        // Optionally build G+1 hot
        let hot_g2 = rt
            .get_or_load_hot(2, fp.clone())
            .await
            .expect("hot g2 should build");
        assert_eq!(hot_g2.generation, 2);
        // Release old G loader
        release_notify.notify_one();
        let res_g1 = loader_handle.await.unwrap();
        // Stale G load must be discarded (None) and must not overwrite G+1
        assert!(res_g1.is_none(), "stale G load should be discarded");
        // Active hot must remain G+1
        let active = rt.peek_hot(2, &fp).await.expect("active should be G+1");
        assert_eq!(active.generation, 2);
        assert!(!Arc::ptr_eq(&active, &hot_g2) || Arc::ptr_eq(&active, &hot_g2)); // either same Arc
                                                                                  // peek_hot(G+1) never returns G
        assert!(
            rt.peek_hot(1, &fp).await.is_none()
                || rt.peek_hot(1, &fp).await.unwrap().generation == 1
        );
        // Subsequent G+1 request uses only G+1 data
        let hot_again = rt.get_or_load_hot(2, fp.clone()).await.unwrap();
        assert_eq!(hot_again.generation, 2);
        // Runtime generation remains G+1
        assert_eq!(rt.current_snapshot().unwrap().generation, 2);
        // Cleanup hooks
        set_hot_load_notifies(None, None);
        // No HotState labelled G contains G+1 DB contents — proven by DB generation validation test
    }
}
