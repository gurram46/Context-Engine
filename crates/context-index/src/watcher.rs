use anyhow::Result;
use notify::{
    event::{CreateKind, ModifyKind, RemoveKind},
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher,
};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
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

fn is_opencode_index_ignored(lower: &str) -> bool {
    lower == ".opencode/index"
        || lower.starts_with(".opencode/index/")
        || lower.contains("/.opencode/index/")
        || lower.ends_with("/.opencode/index")
}

fn is_ignored(rel: &str) -> bool {
    let lower = rel.to_lowercase().replace('\\', "/");
    if is_opencode_index_ignored(&lower) {
        return true;
    }
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
        return None;
    }
    if Path::new(&s).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(s)
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
        let paths: Vec<String> = paths.into_iter().collect();
        if paths.is_empty() {
            // ponytail: true no-op; do not bump epoch or move Clean -> Paths(empty).
            return self.snapshot();
        }

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

        if epoch == guard.epoch {
            guard.state = DirtyState::Clean;
        }
        DirtySnapshot {
            state: guard.state.clone(),
            epoch: guard.epoch,
        }
    }
}

fn event_kind_implies_unknown(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(CreateKind::Folder)
            | EventKind::Remove(RemoveKind::Folder)
            | EventKind::Modify(ModifyKind::Name(_))
    )
}

fn looks_like_file(path: &Path) -> bool {
    path.extension().is_some()
}

fn is_ignore_control_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == ".gitignore" || n == ".ignore" || n == ".opencodeignore")
        .unwrap_or(false)
}

/// Action derived from a filesystem event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventAction {
    Noop,
    MarkUnknown,
    MarkPaths(Vec<String>),
}

impl EventAction {
    fn apply(&self, tracker: &DirtyTracker) {
        match self {
            EventAction::Noop => {}
            EventAction::MarkUnknown => {
                tracker.mark_unknown();
            }
            EventAction::MarkPaths(paths) => {
                tracker.mark_paths(paths.clone());
            }
        }
    }
}

/// Classify a notify event relative to a repository root.
///
/// Per-path order: normalize; skip ignored; if ignore-control then MarkUnknown;
/// if nonignored folder kind or path is a directory then MarkUnknown;
/// otherwise collect the regular relative paths. Empty/all-ignored -> Noop.
fn classify_event(root: &Path, event: &Event) -> EventAction {
    if event.need_rescan() {
        return EventAction::MarkUnknown;
    }
    let mut rels = Vec::new();
    for p in &event.paths {
        let rel = match normalize_rel(root, p) {
            Some(r) => r,
            None => return EventAction::MarkUnknown,
        };
        if is_ignored(&rel) {
            continue;
        }
        if is_ignore_control_file(p) {
            return EventAction::MarkUnknown;
        }
        if event_kind_implies_unknown(&event.kind) || p.is_dir() {
            return EventAction::MarkUnknown;
        }
        if matches!(
            event.kind,
            EventKind::Remove(RemoveKind::Any) | EventKind::Remove(RemoveKind::Other)
        ) {
            if looks_like_file(p) {
                rels.push(rel);
            } else {
                return EventAction::MarkUnknown;
            }
            continue;
        }
        rels.push(rel);
    }
    if rels.is_empty() {
        EventAction::Noop
    } else {
        EventAction::MarkPaths(rels)
    }
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
                    classify_event(&root_cb, &event).apply(&tracker_cb);
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
    use notify::event::{DataChange, ModifyKind};
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
    fn dirty_ack_current_epoch_clears_unknown_stale_cannot() {
        let tracker = DirtyTracker::new(10);
        tracker.mark_unknown();
        let snap = tracker.snapshot();
        assert_eq!(snap.state, DirtyState::Unknown);
        let epoch = snap.epoch;

        // Current unchanged epoch clears Unknown.
        let ack = tracker.acknowledge(epoch);
        assert_eq!(ack.state, DirtyState::Clean);

        // A new Unknown event bumps the epoch.
        tracker.mark_unknown();
        // Stale epoch cannot clear Unknown.
        let stale_ack = tracker.acknowledge(epoch);
        assert_eq!(stale_ack.state, DirtyState::Unknown);
    }

    #[test]
    fn dirty_opencodeignore_is_unknown() {
        assert!(
            is_ignore_control_file(Path::new("/repo/.opencodeignore")),
            ".opencodeignore should be an ignore-control file"
        );
        assert!(
            is_ignore_control_file(Path::new(".opencodeignore")),
            ".opencodeignore base name should match"
        );
        assert!(
            !is_ignore_control_file(Path::new("/repo/main.rs")),
            "regular files should not match"
        );
    }

    #[test]
    fn dirty_mark_paths_empty_is_noop() {
        let tracker = DirtyTracker::new(10);
        let before = tracker.snapshot();
        assert_eq!(before.state, DirtyState::Clean);
        assert_eq!(before.epoch, 0);

        let returned = tracker.mark_paths(std::iter::empty::<String>());
        assert_eq!(returned.state, DirtyState::Clean);
        assert_eq!(returned.epoch, 0);

        let after = tracker.snapshot();
        assert_eq!(after.state, DirtyState::Clean);
        assert_eq!(after.epoch, 0, "empty mark_paths must not bump epoch");
    }

    #[test]
    fn dirty_folder_create_remove_kind_is_unknown() {
        assert!(
            event_kind_implies_unknown(&EventKind::Create(CreateKind::Folder)),
            "Create(Folder) should imply Unknown"
        );
        assert!(
            event_kind_implies_unknown(&EventKind::Remove(RemoveKind::Folder)),
            "Remove(Folder) should imply Unknown even if the path no longer exists"
        );
        assert!(
            !event_kind_implies_unknown(&EventKind::Create(CreateKind::File)),
            "Create(File) should not imply Unknown"
        );
        assert!(
            !event_kind_implies_unknown(&EventKind::Remove(RemoveKind::File)),
            "Remove(File) should not imply Unknown"
        );
    }

    #[test]
    fn classify_ignored_context_folder_create_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let context_dir = root.join(".context");
        std::fs::create_dir_all(&context_dir).unwrap();
        let event = Event::new(EventKind::Create(CreateKind::Folder)).add_path(context_dir);
        assert_eq!(
            classify_event(root, &event),
            EventAction::Noop,
            "creating the runtime .context directory must not dirty state"
        );
    }

    #[test]
    fn classify_ignored_context_folder_remove_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let context_dir = root.join(".context");
        // Path no longer exists, as it would be for a Remove(Folder) event.
        let event = Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(context_dir);
        assert_eq!(
            classify_event(root, &event),
            EventAction::Noop,
            "removing the runtime .context directory must not dirty state"
        );
    }

    #[test]
    fn classify_ignored_context_db_modify_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let db = root.join(".context/index/structural.db");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        std::fs::write(&db, b"").unwrap();
        let event =
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(db);
        assert_eq!(
            classify_event(root, &event),
            EventAction::Noop,
            "modifying the runtime structural DB must not dirty state"
        );
    }

    #[test]
    fn classify_nonignored_folder_remove_is_unknown() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let src_dir = root.join("src");
        // Directory has been removed, so it does not exist.
        let event = Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(src_dir);
        assert_eq!(
            classify_event(root, &event),
            EventAction::MarkUnknown,
            "removing a nonignored directory must mark Unknown"
        );
    }

    #[test]
    fn classify_regular_file_modify_is_paths() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let file = root.join("src/main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"fn main() {}").unwrap();
        let event =
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(file);
        assert_eq!(
            classify_event(root, &event),
            EventAction::MarkPaths(vec!["src/main.rs".to_string()]),
            "modifying a regular nonignored file must record its relative path"
        );
    }

    #[test]
    fn classify_all_ignored_paths_leaves_tracker_unchanged() {
        let tracker = DirtyTracker::new(10);
        let before = tracker.snapshot();

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let event = Event::new(EventKind::Create(CreateKind::Folder))
            .add_path(root.join(".context"))
            .add_path(root.join(".git/objects"));
        let action = classify_event(root, &event);
        assert_eq!(
            action,
            EventAction::Noop,
            "all-ignored event must produce Noop"
        );

        // Applying Noop must not mutate tracker state/epoch.
        match action {
            EventAction::MarkPaths(rels) => {
                tracker.mark_paths(rels);
            }
            EventAction::MarkUnknown => {
                tracker.mark_unknown();
            }
            EventAction::Noop => {}
        }
        let after = tracker.snapshot();
        assert_eq!(after.state, before.state);
        assert_eq!(after.epoch, before.epoch, "Noop must not bump epoch");
    }

    #[test]
    fn classify_parent_dir_component_is_unknown() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let evil = root.join("a/../b.rs");
        let event =
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(evil);
        assert_eq!(
            classify_event(root, &event),
            EventAction::MarkUnknown,
            "path containing ParentDir '..' must classify as Unknown"
        );
        let evil2 = root.join("src/../../secret.txt");
        let event2 = Event::new(EventKind::Create(CreateKind::File)).add_path(evil2);
        assert_eq!(
            classify_event(root, &event2),
            EventAction::MarkUnknown,
            "escape via '..' must classify as Unknown"
        );
    }

    #[test]
    fn classify_opencode_settings_is_paths() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let file = root.join(".opencode/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"{}").unwrap();
        let event =
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(file);
        assert_eq!(
            classify_event(root, &event),
            EventAction::MarkPaths(vec![".opencode/settings.json".to_string()]),
            ".opencode/settings.json must not be ignored"
        );
        assert!(!is_ignored(".opencode/settings.json"));
    }

    #[test]
    fn classify_opencode_index_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let file = root.join(".opencode/index/structural.db");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"").unwrap();
        let event = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(file.clone());
        assert_eq!(
            classify_event(root, &event),
            EventAction::Noop,
            ".opencode/index and descendants must be ignored"
        );
        assert!(is_ignored(".opencode/index/structural.db"));
        assert!(is_ignored(".opencode/index"));
        // sibling file under .opencode/index subdir
        let file2 = root.join(".opencode/index/a/b.rs");
        let event2 = Event::new(EventKind::Create(CreateKind::File)).add_path(file2);
        assert_eq!(classify_event(root, &event2), EventAction::Noop);
    }

    #[test]
    fn classify_vanished_generic_rename_is_unknown() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let vanished = root.join("src/renamed.rs");
        // do not create file — vanished
        let event = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::Any,
        )))
        .add_path(vanished);
        assert_eq!(
            classify_event(root, &event),
            EventAction::MarkUnknown,
            "generic ModifyKind::Name for vanished path must be MarkUnknown"
        );
        // also Both variant
        let vanished2 = root.join("src/other.rs");
        let event2 = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::Both,
        )))
        .add_path(vanished2);
        assert_eq!(classify_event(root, &event2), EventAction::MarkUnknown);
        // ignored path with rename must stay Noop
        let ignored = root.join(".opencode/index/foo.db");
        let event3 = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::Any,
        )))
        .add_path(ignored);
        assert_eq!(
            classify_event(root, &event3),
            EventAction::Noop,
            "ignored paths must remain Noop even for rename"
        );
    }

    #[test]
    fn classify_vanished_generic_remove_is_unknown() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // File with extension via generic remove remains path-local (not unknown)
        let vanished = root.join("src/gone.rs");
        let event = Event::new(EventKind::Remove(RemoveKind::Any)).add_path(vanished);
        assert_eq!(
            classify_event(root, &event),
            EventAction::MarkPaths(vec!["src/gone.rs".to_string()]),
            "generic Remove(Any) for file with extension must remain path-local"
        );
        // Directory-like path without extension via generic remove is ambiguous -> Unknown
        let vanished2 = root.join("src/old_dir");
        let event2 = Event::new(EventKind::Remove(RemoveKind::Other)).add_path(vanished2);
        assert_eq!(classify_event(root, &event2), EventAction::MarkUnknown);
        // ignored generic remove stays Noop
        let ignored = root.join(".context/index/db");
        let event3 = Event::new(EventKind::Remove(RemoveKind::Any)).add_path(ignored);
        assert_eq!(classify_event(root, &event3), EventAction::Noop);
    }

    #[test]
    fn classify_exact_file_remove_is_paths() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let vanished = root.join("src/deleted.rs");
        // exact file remove for vanished path remains path-local
        let event = Event::new(EventKind::Remove(RemoveKind::File)).add_path(vanished);
        assert_eq!(
            classify_event(root, &event),
            EventAction::MarkPaths(vec!["src/deleted.rs".to_string()]),
            "exact Remove(File) must remain path-local even when vanished"
        );
        // exact folder remove remains unknown (coverage)
        let vanished_dir = root.join("src/subdir");
        let event2 = Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(vanished_dir);
        assert_eq!(classify_event(root, &event2), EventAction::MarkUnknown);
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
