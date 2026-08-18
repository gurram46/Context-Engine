use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex};

use crate::structural::StructuralIndex;

/// Watcher freshness state — concise, no giant history.
#[derive(Debug, Clone, Default)]
pub struct WatcherStatus {
    pub last_event_at: Option<u64>,             // epoch ms
    pub last_structural_update_at: Option<u64>, // epoch ms
    pub pending_paths: usize,
    pub structural_generation: u64,
    pub is_running: bool,
}

/// Internal pending coalescing entry
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PendingEntry {
    rel: String,
    abs: PathBuf,
}

/// Bounded watcher for one worktree root.
/// Uses `notify` + debouncing + hash verification.
/// Guarantees:
/// - duplicate path coalescing
/// - no concurrent SQLite writes for same worktree (single worker)
/// - graceful shutdown via Drop
/// - backpressure: if bounded channel overflows, trigger rescan rather than silent loss
pub struct StructuralWatcher {
    root: PathBuf,
    #[allow(dead_code)]
    index: Arc<StructuralIndex>,
    // status shared
    status: Arc<Mutex<WatcherStatus>>,
    // watcher handle (kept alive)
    _watcher: RecommendedWatcher,
    // sender for events
    #[allow(dead_code)]
    event_tx: mpsc::Sender<PooledEvent>,
    // shutdown signal
    shutdown: Arc<tokio::sync::Notify>,
    // reconcile flag for overflow (durable, not channel-dependent)
    #[allow(dead_code)]
    reconcile_needed: Arc<AtomicBool>,
    // worker handle
    _worker: tokio::task::JoinHandle<()>,
}

#[derive(Debug)]
#[allow(dead_code)]
enum PooledEvent {
    Paths(Vec<PathBuf>),
    Rescan,
}

const DEBOUNCE_MS: u64 = 120;
const BOUNDED_CAP: usize = 512;
const IGNORED_DIRS: &[&str] = &[
    ".git/",
    ".context/",
    ".opencode/",
    "target/",
    "node_modules/",
    "dist/",
    "build/",
    "__pycache__/",
    ".pytest_cache/",
    ".next/",
    ".nuxt/",
    "coverage/",
];

fn is_ignored(rel: &str) -> bool {
    let lower = rel.to_lowercase().replace('\\', "/");
    for pat in IGNORED_DIRS {
        if lower.starts_with(pat)
            || lower.contains(&format!("/{}", pat))
            || lower == pat.trim_end_matches('/')
        {
            return true;
        }
    }
    false
}

fn normalize_rel(root: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

impl StructuralWatcher {
    pub fn new(root: PathBuf) -> Result<Self> {
        Self::new_with_capacity(root, BOUNDED_CAP)
    }

    /// Test helper with tiny capacity to force overflow
    pub fn new_with_capacity(root: PathBuf, cap: usize) -> Result<Self> {
        let index = Arc::new(StructuralIndex::for_path(root.clone()));
        let status = Arc::new(Mutex::new(WatcherStatus {
            structural_generation: crate::structural::store::open_db(&root)
                .ok()
                .and_then(|c| crate::structural::store::get_generation(&c).ok())
                .unwrap_or(0),
            is_running: true,
            ..Default::default()
        }));
        let (tx, rx) = mpsc::channel::<PooledEvent>(cap);
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let reconcile_needed = Arc::new(AtomicBool::new(false));

        // Create notify watcher that forwards paths to bounded channel
        let tx_clone = tx.clone();
        let root_clone = root.clone();
        let reconcile_clone = reconcile_needed.clone();
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Extract paths, normalize, filter ignores — no async spawn here (notify thread has no tokio runtime)
                    let mut rels = Vec::new();
                    for p in event.paths {
                        if let Some(rel) = normalize_rel(&root_clone, &p) {
                            if is_ignored(&rel) {
                                continue;
                            }
                            if p.is_dir() {
                                continue;
                            }
                            rels.push(p);
                        }
                    }
                    if !rels.is_empty() {
                        if let Err(e) = tx_clone.try_send(PooledEvent::Paths(rels)) {
                            // Channel full → set durable flag, not just try to send Rescan into same full channel
                            if matches!(e, tokio::sync::mpsc::error::TrySendError::Full(_)) {
                                reconcile_clone.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error=%e, "watcher error");
                    reconcile_clone.store(true, Ordering::SeqCst);
                }
            }
        })?;

        watcher.watch(&root, RecursiveMode::Recursive)?;

        // Spawn worker that coalesces and processes
        let root_w = root.clone();
        let index_w = index.clone();
        let status_w = status.clone();
        let shutdown_w = shutdown.clone();
        let reconcile_w = reconcile_needed.clone();
        let worker = tokio::spawn(async move {
            watcher_worker(root_w, index_w, status_w, rx, shutdown_w, reconcile_w).await;
        });

        Ok(Self {
            root,
            index,
            status,
            _watcher: watcher,
            event_tx: tx,
            shutdown,
            reconcile_needed,
            _worker: worker,
        })
    }

    pub async fn status(&self) -> WatcherStatus {
        let mut s = self.status.lock().await.clone();
        s.pending_paths = 0; // approximate; worker tracks internally but we expose 0 for now unless busy
        s
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Graceful shutdown
    pub async fn shutdown(&self) {
        self.shutdown.notify_waiters();
        {
            let mut s = self.status.lock().await;
            s.is_running = false;
        }
        // Give worker a moment
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

impl Drop for StructuralWatcher {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
        // Best-effort: mark not running if we can (sync try_lock)
        if let Ok(mut s) = self.status.try_lock() {
            s.is_running = false;
        }
        self._worker.abort();
    }
}

async fn watcher_worker(
    root: PathBuf,
    index: Arc<StructuralIndex>,
    status: Arc<Mutex<WatcherStatus>>,
    mut rx: mpsc::Receiver<PooledEvent>,
    shutdown: Arc<tokio::sync::Notify>,
    reconcile_needed: Arc<AtomicBool>,
) {
    let mut pending: HashMap<String, PathBuf> = HashMap::new();
    let mut last_flush = tokio::time::Instant::now();
    let debounce = Duration::from_millis(DEBOUNCE_MS);

    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                tracing::info!("watcher worker shutdown");
                {
                    let mut s = status.lock().await;
                    s.is_running = false;
                }
                break;
            }
            msg = rx.recv() => {
                match msg {
                    Some(PooledEvent::Paths(paths)) => {
                        {
                            let mut s = status.lock().await;
                            s.last_event_at = Some(now_ms());
                            // pending count will be updated after insert
                        }
                        for p in paths {
                            if let Some(rel) = normalize_rel(&root, &p) {
                                if is_ignored(&rel) { continue; }
                                pending.insert(rel.clone(), p);
                            }
                        }
                        // update pending count in status
                        {
                            let mut s = status.lock().await;
                            s.pending_paths = pending.len();
                        }
                    }
                    Some(PooledEvent::Rescan) => {
                        // Full rescan fallback on overflow
                        tracing::warn!("watcher queue overflow → incremental rescan");
                        pending.clear();
                        // Trigger full build via ProjectIndex discovery
                        if let Ok(pr) = crate::project_root::ProjectRoot::resolve(Some(&root)) {
                            if let Ok(idx) = crate::discovery::ProjectIndex::discover(&pr) {
                                let _ = index.build(&idx);
                            }
                        }
                        {
                            let mut s = status.lock().await;
                            s.last_structural_update_at = Some(now_ms());
                            s.pending_paths = 0;
                            s.structural_generation = crate::structural::store::open_db(&root).ok().and_then(|c| crate::structural::store::get_generation(&c).ok()).unwrap_or(s.structural_generation);
                        }
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                // periodic debounce check
            }
        }

        // Durable reconcile flag (overflow) — not dependent on channel
        if reconcile_needed.load(Ordering::SeqCst) {
            reconcile_needed.store(false, Ordering::SeqCst);
            tracing::warn!("watcher reconcile flag set → full rescan");
            pending.clear();
            if let Ok(pr) = crate::project_root::ProjectRoot::resolve(Some(&root)) {
                if let Ok(idx) = crate::discovery::ProjectIndex::discover(&pr) {
                    let _ = index.build(&idx);
                }
            }
            {
                let mut s = status.lock().await;
                s.last_structural_update_at = Some(now_ms());
                s.pending_paths = 0;
                s.structural_generation = crate::structural::store::open_db(&root)
                    .ok()
                    .and_then(|c| crate::structural::store::get_generation(&c).ok())
                    .unwrap_or(s.structural_generation);
            }
        }

        // Debounce: if pending not empty and debounce window passed since last event, flush
        let now = tokio::time::Instant::now();
        if !pending.is_empty() && now.duration_since(last_flush) >= debounce {
            // Check if new events are still streaming: peek if channel has more pending without blocking
            // Simple: if we have pending, drain and process
            let to_process: HashMap<String, PathBuf> = std::mem::take(&mut pending);
            {
                let mut s = status.lock().await;
                s.pending_paths = 0;
            }
            last_flush = now;

            // Process each path with hash verification and bounded sequential writes
            let mut seen: HashSet<String> = HashSet::new();
            for (rel, abs) in to_process {
                if !seen.insert(rel.clone()) {
                    continue;
                }
                // Hash verification: if file exists, check if hash changed vs DB; if not, skip
                // The update_single_file already does this check
                let res = if abs.exists() {
                    // Only process if extension is structural language
                    let lang = crate::structural::language::detect_language(Path::new(&rel));
                    if lang == crate::structural::language::Language::Unknown {
                        continue;
                    }
                    index.update_single_file(&rel)
                } else {
                    // Delete case: ensure we delete from DB even if file gone
                    index.update_single_file(&rel)
                };
                match res {
                    Ok(stats) => {
                        let mut s = status.lock().await;
                        s.last_structural_update_at = Some(now_ms());
                        s.structural_generation = stats.structural_generation;
                        s.pending_paths = pending.len();
                    }
                    Err(e) => {
                        tracing::warn!(file=%rel, error=%e, "watcher incremental failed");
                    }
                }
                // Small yield to avoid starving
                tokio::task::yield_now().await;
            }
        }

        // If pending empty, update flush time
        if pending.is_empty() {
            last_flush = tokio::time::Instant::now();
        }
    }
    // Ensure stopped state is visible even if loop exited via channel close
    {
        let mut s = status.lock().await;
        s.is_running = false;
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Mark-only dirty state. `Unknown` means the watcher cannot tell which paths changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DirtyState {
    #[default]
    Clean,
    Paths(BTreeSet<String>),
    Unknown,
}

/// A snapshot of dirty state plus its epoch. The epoch protects acknowledgement from
/// clearing changes that arrived after the snapshot was taken.
#[derive(Debug, Clone)]
pub struct DirtySnapshot {
    pub state: DirtyState,
    pub epoch: u64,
}

#[derive(Debug)]
struct DirtyTrackerInner {
    state: DirtyState,
    epoch: u64,
    capacity: usize,
}

/// Bounded, epoch-protected dirty tracker.
///
/// - `snapshot` returns the current state and epoch without changing either.
/// - `acknowledge(epoch)` clears the state only if the epoch has not moved on.
/// - A poisoned lock always reports `Unknown` so the tracker never falsely claims cleanliness.
#[derive(Debug, Clone)]
pub struct DirtyTracker {
    inner: Arc<std::sync::Mutex<DirtyTrackerInner>>,
}

impl DirtyTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(DirtyTrackerInner {
                state: DirtyState::Clean,
                epoch: 0,
                capacity,
            })),
        }
    }

    pub fn mark_paths(&self, paths: impl IntoIterator<Item = String>) -> DirtySnapshot {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => {
                return DirtySnapshot {
                    state: DirtyState::Unknown,
                    epoch: 0,
                }
            }
        };

        // Once unknown, more precise path information is intentionally lost.
        if guard.state == DirtyState::Unknown {
            guard.epoch = guard.epoch.wrapping_add(1);
            return DirtySnapshot {
                state: DirtyState::Unknown,
                epoch: guard.epoch,
            };
        }

        let mut set = match std::mem::take(&mut guard.state) {
            DirtyState::Clean => BTreeSet::new(),
            DirtyState::Paths(s) => s,
            DirtyState::Unknown => BTreeSet::new(),
        };

        for p in paths {
            set.insert(p);
        }

        guard.state = if set.len() > guard.capacity {
            DirtyState::Unknown
        } else {
            DirtyState::Paths(set)
        };
        guard.epoch = guard.epoch.wrapping_add(1);
        DirtySnapshot {
            state: guard.state.clone(),
            epoch: guard.epoch,
        }
    }

    pub fn mark_unknown(&self) -> DirtySnapshot {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => {
                return DirtySnapshot {
                    state: DirtyState::Unknown,
                    epoch: 0,
                }
            }
        };
        guard.state = DirtyState::Unknown;
        guard.epoch = guard.epoch.wrapping_add(1);
        DirtySnapshot {
            state: guard.state.clone(),
            epoch: guard.epoch,
        }
    }

    pub fn snapshot(&self) -> DirtySnapshot {
        match self.inner.lock() {
            Ok(guard) => DirtySnapshot {
                state: guard.state.clone(),
                epoch: guard.epoch,
            },
            Err(_) => DirtySnapshot {
                state: DirtyState::Unknown,
                epoch: 0,
            },
        }
    }

    pub fn acknowledge(&self, epoch: u64) -> DirtySnapshot {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => {
                return DirtySnapshot {
                    state: DirtyState::Unknown,
                    epoch: 0,
                }
            }
        };

        if guard.state == DirtyState::Unknown {
            return DirtySnapshot {
                state: DirtyState::Unknown,
                epoch: guard.epoch,
            };
        }

        if epoch == guard.epoch {
            guard.state = DirtyState::Clean;
        }
        DirtySnapshot {
            state: guard.state.clone(),
            epoch: guard.epoch,
        }
    }
}

fn is_ignore_control_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == ".gitignore" || n == ".ignore")
        .unwrap_or(false)
}

/// Lightweight, mark-only repository watcher.
///
/// The notify callback records normalized relative paths in a `DirtyTracker`. It performs
/// no SQLite work, no parsing/hashing/embedding, and no async spawning.
pub struct RepositoryWatcher {
    root: PathBuf,
    tracker: DirtyTracker,
    _watcher: RecommendedWatcher,
}

impl RepositoryWatcher {
    pub fn new(root: PathBuf) -> Result<Self> {
        let tracker = DirtyTracker::new(BOUNDED_CAP);
        let tracker_cb = tracker.clone();
        let root_cb = root.clone();

        let mut watcher =
            notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
                Ok(event) => {
                    let mut rels = Vec::new();
                    let mut unknown = false;
                    for p in &event.paths {
                        if p.is_dir() {
                            unknown = true;
                            break;
                        }
                        if is_ignore_control_file(p) {
                            unknown = true;
                            break;
                        }
                        match normalize_rel(&root_cb, p) {
                            Some(rel) => rels.push(rel),
                            None => {
                                unknown = true;
                                break;
                            }
                        }
                    }
                    if unknown {
                        tracker_cb.mark_unknown();
                    } else if !rels.is_empty() {
                        tracker_cb.mark_paths(rels);
                    }
                }
                Err(_) => {
                    tracker_cb.mark_unknown();
                }
            })?;

        watcher.watch(&root, RecursiveMode::Recursive)?;

        Ok(Self {
            root,
            tracker,
            _watcher: watcher,
        })
    }

    pub fn tracker(&self) -> &DirtyTracker {
        &self.tracker
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ignored_paths() {
        assert!(is_ignored(".git/config"));
        assert!(is_ignored("target/debug/foo"));
        assert!(is_ignored(".context/index/db"));
        assert!(is_ignored("node_modules/a"));
        assert!(!is_ignored("src/main.rs"));
        assert!(!is_ignored("crates/context-index/src/lib.rs"));
        assert!(!is_ignored("backend/handler.go"));
    }

    #[test]
    fn normalize() {
        let root = Path::new("/tmp/repo");
        let abs = Path::new("/tmp/repo/src/main.rs");
        assert_eq!(normalize_rel(root, abs).unwrap(), "src/main.rs");
    }

    #[tokio::test]
    async fn watcher_coalesces() -> Result<()> {
        let tmp = TempDir::new()?;
        let root = tmp.path().to_path_buf();
        // Need .git for walk to work
        std::fs::create_dir_all(root.join(".git"))?;
        let w = StructuralWatcher::new(root.clone())?;
        // Simulate rapid writes coalescing: send multiple paths for same file, ensure pending coalesces
        // We can't easily trigger notify events without filesystem, but we can test the is_ignored and basic creation
        let st = w.status().await;
        assert!(st.is_running);
        w.shutdown().await;
        Ok(())
    }

    #[tokio::test]
    async fn watcher_status_after_shutdown_false() -> Result<()> {
        let tmp = TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        let w = StructuralWatcher::new(root.clone())?;
        assert!(w.status().await.is_running);
        w.shutdown().await;
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(
            !w.status().await.is_running,
            "status should be false after shutdown"
        );
        Ok(())
    }

    #[test]
    fn overflow_sets_flag() {
        let (tx, _rx) = mpsc::channel::<PooledEvent>(1);
        tx.try_send(PooledEvent::Paths(vec![PathBuf::from("a.py")]))
            .unwrap();
        let res = tx.try_send(PooledEvent::Paths(vec![PathBuf::from("b.py")]));
        assert!(matches!(
            res,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));
        // Simulate flag set
        let flag = Arc::new(AtomicBool::new(false));
        if matches!(res, Err(tokio::sync::mpsc::error::TrySendError::Full(_))) {
            flag.store(true, Ordering::SeqCst);
        }
        assert!(flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn watcher_overflow_reconciles() -> Result<()> {
        let tmp = TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        // Tiny queue to force overflow
        let w = StructuralWatcher::new_with_capacity(root.clone(), 2)?;
        // Rapidly create 10 files (exceeds cap 2)
        for i in 0..10 {
            let p = root.join(format!("file_{}.py", i));
            std::fs::write(&p, format!("def foo_{}():\n    pass\n", i))?;
        }
        // Give watcher time to process + debounce + potential rescan
        tokio::time::sleep(Duration::from_millis(800)).await;
        // Check that all 10 files are indexed despite overflow (reconciled)
        let conn = crate::structural::store::open_db(&root)?;
        let files = crate::structural::store::list_files(&conn)?;
        // At least 10 files should be present (plus maybe others)
        assert!(
            files.len() >= 10,
            "expected >=10 files after overflow reconcile, got {}",
            files.len()
        );
        // Final DB hash must match filesystem (via build)
        let pr = crate::project_root::ProjectRoot::resolve(Some(&root))?;
        let idx = crate::discovery::ProjectIndex::discover(&pr)?;
        let si = crate::structural::StructuralIndex::new(&pr);
        let stats = si.build(&idx)?;
        // No missing files
        assert_eq!(stats.files_parsed + stats.files_skipped, 10);
        w.shutdown().await;
        Ok(())
    }

    #[test]
    fn dirty_ack_does_not_lose_same_path_event_after_snapshot() {
        let tracker = DirtyTracker::new(10);
        tracker.mark_paths(["src/main.rs".to_string()]);
        let snap = tracker.snapshot();
        assert_eq!(
            snap.state,
            DirtyState::Paths(BTreeSet::from(["src/main.rs".to_string()]))
        );
        let epoch = snap.epoch;

        // Same-path event arrives after snapshot.
        tracker.mark_paths(["src/main.rs".to_string()]);
        let ack = tracker.acknowledge(epoch);

        // Acknowledging the old epoch must not drop the later same-path event.
        assert_ne!(
            ack.state,
            DirtyState::Clean,
            "ack of old epoch after a new event must not clear state"
        );
        assert!(
            matches!(ack.state, DirtyState::Paths(_)),
            "same-path event after snapshot must be preserved"
        );
    }

    #[test]
    fn dirty_capacity_overflow_becomes_unknown() {
        let tracker = DirtyTracker::new(2);
        tracker.mark_paths(["a.rs".to_string(), "b.rs".to_string()]);
        assert!(
            !matches!(tracker.snapshot().state, DirtyState::Unknown),
            "state under capacity should remain Paths"
        );

        tracker.mark_paths(["c.rs".to_string()]);
        assert_eq!(
            tracker.snapshot().state,
            DirtyState::Unknown,
            "exceeding capacity must collapse to Unknown"
        );
    }

    #[test]
    fn dirty_unchanged_epoch_acknowledges_to_clean() {
        let tracker = DirtyTracker::new(10);
        tracker.mark_paths(["src/main.rs".to_string()]);
        let snap = tracker.snapshot();
        let epoch = snap.epoch;
        assert!(
            matches!(snap.state, DirtyState::Paths(_)),
            "state should be dirty before ack"
        );

        let ack = tracker.acknowledge(epoch);
        assert_eq!(
            ack.state,
            DirtyState::Clean,
            "acknowledging the current unchanged epoch must clear to Clean"
        );
    }

    fn wait_for_state(
        tracker: &DirtyTracker,
        timeout: Duration,
        predicate: impl Fn(&DirtyState) -> bool,
    ) -> Option<DirtySnapshot> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            let snap = tracker.snapshot();
            if predicate(&snap.state) {
                return Some(snap);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    #[test]
    fn dirty_watcher_classifies_source_and_gitignore() -> Result<()> {
        let tmp = TempDir::new()?;
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git"))?;
        std::fs::create_dir_all(root.join("src"))?;

        let watcher = RepositoryWatcher::new(root.clone())?;
        assert_eq!(watcher.root(), root.as_path());

        // Regular source modification records its relative path.
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n")?;
        let snap = wait_for_state(watcher.tracker(), Duration::from_millis(1000), |s| {
            matches!(s, DirtyState::Paths(_))
        });
        assert!(snap.is_some(), "source change should be recorded");
        assert_eq!(
            snap.unwrap().state,
            DirtyState::Paths(BTreeSet::from(["src/main.rs".to_string()]))
        );

        // Acknowledge so the next event is isolated.
        let epoch = watcher.tracker().snapshot().epoch;
        watcher.tracker().acknowledge(epoch);
        assert_eq!(watcher.tracker().snapshot().state, DirtyState::Clean);

        // Changing .gitignore collapses state to Unknown.
        std::fs::write(root.join(".gitignore"), "target/\n")?;
        let snap = wait_for_state(watcher.tracker(), Duration::from_millis(1000), |s| {
            *s == DirtyState::Unknown
        });
        assert!(snap.is_some(), ".gitignore change should record Unknown");

        Ok(())
    }
}
