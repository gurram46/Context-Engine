#![allow(dead_code)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    Bm25,
    Vectors,
}

struct Entry {
    bytes: usize,
    last_accessed: Instant,
    pin_count: AtomicUsize,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("bytes", &self.bytes)
            .field("last_accessed", &self.last_accessed)
            .field("pin_count", &self.pin_count.load(Ordering::Relaxed))
            .finish()
    }
}

pub struct ResourceManager {
    budget_bytes: usize,
    components: RwLock<HashMap<(PathBuf, ComponentKind), Entry>>,
}

impl ResourceManager {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            components: RwLock::new(HashMap::new()),
        }
    }

    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    pub async fn total_bytes(&self) -> usize {
        let g = self.components.read().await;
        g.values().map(|e| e.bytes).sum()
    }

    fn canon(p: &Path) -> PathBuf {
        let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let s = c.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            PathBuf::from(stripped)
        } else {
            c
        }
    }

    pub async fn register(&self, root: &Path, kind: ComponentKind, bytes: usize) {
        let mut g = self.components.write().await;
        g.insert(
            (Self::canon(root), kind),
            Entry {
                bytes,
                last_accessed: Instant::now(),
                pin_count: AtomicUsize::new(0),
            },
        );
    }

    pub async fn touch(&self, root: &Path, kind: ComponentKind) {
        let mut g = self.components.write().await;
        if let Some(e) = g.get_mut(&(Self::canon(root), kind)) {
            e.last_accessed = Instant::now();
        }
    }

    pub async fn pin(&self, root: &Path, kind: ComponentKind) {
        let g = self.components.read().await;
        if let Some(e) = g.get(&(Self::canon(root), kind)) {
            e.pin_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub async fn unpin(&self, root: &Path, kind: ComponentKind) {
        let g = self.components.read().await;
        if let Some(e) = g.get(&(Self::canon(root), kind)) {
            let prev = e.pin_count.load(Ordering::SeqCst);
            if prev > 0 {
                e.pin_count.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    // Sync variants for RAII Drop (no async lock needed beyond atomic)
    pub fn pin_sync(&self, root: &Path, kind: ComponentKind) {
        if let Ok(g) = self.components.try_read() {
            if let Some(e) = g.get(&(Self::canon(root), kind)) {
                e.pin_count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    pub fn unpin_sync(&self, root: &Path, kind: ComponentKind) {
        if let Ok(g) = self.components.try_read() {
            if let Some(e) = g.get(&(Self::canon(root), kind)) {
                let prev = e.pin_count.load(Ordering::SeqCst);
                if prev > 0 {
                    e.pin_count.fetch_sub(1, Ordering::SeqCst);
                }
            }
        }
    }

    pub async fn remove(&self, root: &Path, kind: ComponentKind) {
        let mut g = self.components.write().await;
        g.remove(&(Self::canon(root), kind));
    }

    pub async fn estimated_bytes_for(&self, root: &Path, kind: ComponentKind) -> Option<usize> {
        let g = self.components.read().await;
        g.get(&(Self::canon(root), kind)).map(|e| e.bytes)
    }

    pub async fn is_pinned(&self, root: &Path, kind: ComponentKind) -> bool {
        let g = self.components.read().await;
        g.get(&(Self::canon(root), kind))
            .map(|e| e.pin_count.load(Ordering::SeqCst) > 0)
            .unwrap_or(false)
    }

    /// Check if needed bytes can fit, evict LRU not-pinned until fits, return true if fits (caller may then load), false if still not fit (use cold path)
    /// `evict_cb` is called for each eviction: `evict_cb(root, kind)` should drop hot component
    pub async fn ensure_budget<F>(&self, needed: usize, mut evict_cb: F) -> bool
    where
        F: FnMut(&Path, ComponentKind),
    {
        if needed > self.budget_bytes {
            // single component larger than total budget must cold-fallback
            return false;
        }
        let total = self.total_bytes().await;
        if total + needed <= self.budget_bytes {
            return true;
        }
        // Need to evict LRU not-pinned
        let mut to_evict: Vec<((PathBuf, ComponentKind), Instant)> = Vec::new();
        {
            let g = self.components.read().await;
            for (k, v) in g.iter() {
                if v.pin_count.load(Ordering::SeqCst) == 0 {
                    to_evict.push((k.clone(), v.last_accessed));
                }
            }
        }
        to_evict.sort_by_key(|(_, t)| *t);
        for ((root, kind), _) in to_evict {
            let current_total = self.total_bytes().await;
            if current_total + needed <= self.budget_bytes {
                break;
            }
            evict_cb(&root, kind);
            {
                let mut g = self.components.write().await;
                g.remove(&(root.clone(), kind));
            }
        }
        let final_total = self.total_bytes().await;
        final_total + needed <= self.budget_bytes
    }
}

/// RAII guard that pins a hot component for the duration of a query.
/// On drop, automatically unpins. Uses atomic pin count so Drop is sync.
pub struct PinGuard {
    rm: Arc<ResourceManager>,
    root: PathBuf,
    kind: ComponentKind,
}

impl PinGuard {
    pub async fn new(rm: Arc<ResourceManager>, root: PathBuf, kind: ComponentKind) -> Self {
        rm.pin(&root, kind).await;
        Self { rm, root, kind }
    }

    pub fn new_sync(rm: Arc<ResourceManager>, root: PathBuf, kind: ComponentKind) -> Self {
        rm.pin_sync(&root, kind);
        Self { rm, root, kind }
    }
}

impl Drop for PinGuard {
    fn drop(&mut self) {
        // Use sync unpin via atomic, no await needed
        // Try to use try_read path first
        if let Ok(g) = self.rm.components.try_read() {
            if let Some(e) = g.get(&(self.root.clone(), self.kind)) {
                let prev = e.pin_count.load(Ordering::SeqCst);
                if prev > 0 {
                    e.pin_count.fetch_sub(1, Ordering::SeqCst);
                }
                return;
            }
        }
        // Fallback: if we couldn't get read lock, spawn async unpin
        let rm = self.rm.clone();
        let root = self.root.clone();
        let kind = self.kind;
        // If runtime exists, spawn; otherwise ignore (pin will leak but test will catch)
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                rm.unpin(&root, kind).await;
            });
        }
    }
}
