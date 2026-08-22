#![allow(dead_code)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, Semaphore};

use crate::resource::{ComponentKind, ResourceManager};
use crate::service::ContextService;

pub struct RepoEntry {
    pub service: Arc<ContextService>,
    pub last_accessed: Instant,
    pub client_count: usize,
    pub active_queries: usize,
}

pub struct RepositoryRegistry {
    // canonical root -> entry
    repos: RwLock<HashMap<PathBuf, RepoEntry>>,
    pub resource_manager: Arc<ResourceManager>,
    pub global_semaphore: Arc<Semaphore>, // 1 permit for heavy semantic indexing
    idle_timeout: Duration,
}

impl RepositoryRegistry {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            repos: RwLock::new(HashMap::new()),
            resource_manager: Arc::new(ResourceManager::new(budget_bytes)),
            global_semaphore: Arc::new(Semaphore::new(1)),
            idle_timeout: Duration::from_secs(300),
        }
    }

    pub async fn get_or_create(&self, root: PathBuf) -> Arc<ContextService> {
        // fast read
        {
            let g = self.repos.read().await;
            if let Some(e) = g.get(&root) {
                return e.service.clone();
            }
        }
        // write lock for creation (singleflight per root via global lock - simple)
        let mut g = self.repos.write().await;
        if let Some(e) = g.get(&root) {
            return e.service.clone();
        }
        // create new service (heavy)
        let svc = ContextService::new(Some(root.clone()))
            .await
            .expect("create service");
        let arc = Arc::new(svc);
        g.insert(
            root.clone(),
            RepoEntry {
                service: arc.clone(),
                last_accessed: Instant::now(),
                client_count: 0,
                active_queries: 0,
            },
        );
        arc
    }

    pub async fn touch(&self, root: &Path) {
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(root) {
            e.last_accessed = Instant::now();
        }
    }

    pub async fn inc_client(&self, root: &Path) {
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(root) {
            e.client_count += 1;
            e.last_accessed = Instant::now();
        }
    }

    pub async fn inc_client_with_svc(&self, root: PathBuf, svc: Arc<ContextService>) {
        let mut g = self.repos.write().await;
        let e = g.entry(root.clone()).or_insert_with(|| RepoEntry {
            service: svc.clone(),
            last_accessed: Instant::now(),
            client_count: 0,
            active_queries: 0,
        });
        e.client_count += 1;
        e.last_accessed = Instant::now();
        e.service = svc;
    }

    pub async fn dec_client(&self, root: &Path) {
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(root) {
            e.client_count = e.client_count.saturating_sub(1);
            e.last_accessed = Instant::now();
        }
    }

    pub async fn runtime_count(&self) -> usize {
        let g = self.repos.read().await;
        g.len()
    }

    pub async fn client_counts(&self) -> HashMap<PathBuf, usize> {
        let g = self.repos.read().await;
        g.iter().map(|(k, v)| (k.clone(), v.client_count)).collect()
    }

    pub async fn global_client_count(&self) -> usize {
        let g = self.repos.read().await;
        g.values().map(|v| v.client_count).sum()
    }

    pub async fn evict_idle(&self) {
        let now = Instant::now();
        let mut g = self.repos.write().await;
        let to_remove: Vec<PathBuf> = g
            .iter()
            .filter(|(_, e)| {
                e.client_count == 0
                    && e.active_queries == 0
                    && now.duration_since(e.last_accessed) > self.idle_timeout
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in to_remove {
            g.remove(&k);
            // also remove resource manager components for this root
            // we leave ResourceManager entries to be evicted naturally, but we can clean
            // For now just remove
        }
    }

    pub async fn evict_hot_component(&self, root: &Path, kind: ComponentKind) {
        let g = self.repos.read().await;
        if let Some(e) = g.get(root) {
            // evict from runtime's hot
            // For BM25 vs Vectors, we need to clear hot
            // We will call runtime evict
            // Access via service.runtime
            // service.runtime is private, so we need a public method on ContextService to evict
            // For now, we can just remove from resource manager and let next load be cold; the hot remains in memory but not counted?
            // Better to actually clear hot via service
            let _ = e.service.evict_hot(kind).await;
        }
        self.resource_manager.remove(root, kind).await;
    }

    pub async fn total_hot_bytes(&self) -> usize {
        self.resource_manager.total_bytes().await
    }
}
