#![allow(dead_code)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    Bm25,
    Vectors,
}

#[derive(Debug)]
struct Entry {
    bytes: usize,
    last_accessed: Instant,
    // Weak to hot state to check pin (strong_count >1 means in use)
    // For BM25 we track HotBm25 Arc, for Vectors HotVectors Arc
    // We store as Weak trait object via Box<dyn Any> not needed; just track via bytes and last_accessed and use registry to check pin
    pin_count: usize, // active queries holding Arc
}

pub struct ResourceManager {
    budget_bytes: usize,
    // (root, kind) -> Entry
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

    pub async fn register(&self, root: &Path, kind: ComponentKind, bytes: usize) {
        let mut g = self.components.write().await;
        g.insert(
            (root.to_path_buf(), kind),
            Entry {
                bytes,
                last_accessed: Instant::now(),
                pin_count: 0,
            },
        );
    }

    pub async fn touch(&self, root: &Path, kind: ComponentKind) {
        let mut g = self.components.write().await;
        if let Some(e) = g.get_mut(&(root.to_path_buf(), kind)) {
            e.last_accessed = Instant::now();
        }
    }

    pub async fn pin(&self, root: &Path, kind: ComponentKind) {
        let mut g = self.components.write().await;
        if let Some(e) = g.get_mut(&(root.to_path_buf(), kind)) {
            e.pin_count = e.pin_count.saturating_add(1);
        }
    }

    pub async fn unpin(&self, root: &Path, kind: ComponentKind) {
        let mut g = self.components.write().await;
        if let Some(e) = g.get_mut(&(root.to_path_buf(), kind)) {
            e.pin_count = e.pin_count.saturating_sub(1);
        }
    }

    pub async fn remove(&self, root: &Path, kind: ComponentKind) {
        let mut g = self.components.write().await;
        g.remove(&(root.to_path_buf(), kind));
    }

    pub async fn estimated_bytes_for(&self, root: &Path, kind: ComponentKind) -> Option<usize> {
        let g = self.components.read().await;
        g.get(&(root.to_path_buf(), kind)).map(|e| e.bytes)
    }

    /// Check if needed bytes can fit, evict LRU not-pinned until fits, return true if fits (caller may then load), false if still not fit (use cold path)
    /// `evict_cb` is called for each eviction: `evict_cb(root, kind)` should drop hot component
    pub async fn ensure_budget<F>(&self, needed: usize, mut evict_cb: F) -> bool
    where
        F: FnMut(&Path, ComponentKind),
    {
        let total = self.total_bytes().await;
        if total + needed <= self.budget_bytes {
            return true;
        }
        // Need to evict
        let mut to_evict: Vec<((PathBuf, ComponentKind), Instant)> = Vec::new();
        {
            let g = self.components.read().await;
            for (k, v) in g.iter() {
                if v.pin_count == 0 {
                    to_evict.push((k.clone(), v.last_accessed));
                }
            }
        }
        // LRU first
        to_evict.sort_by_key(|(_, t)| *t);
        for ((root, kind), _) in to_evict {
            // check if still needed
            let current_total = self.total_bytes().await;
            if current_total + needed <= self.budget_bytes {
                break;
            }
            evict_cb(&root, kind);
            // remove from map
            {
                let mut g = self.components.write().await;
                g.remove(&(root.clone(), kind));
            }
        }
        let final_total = self.total_bytes().await;
        final_total + needed <= self.budget_bytes
    }
}
