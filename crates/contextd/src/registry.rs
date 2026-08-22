#![allow(dead_code)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, Semaphore};

use crate::resource::{ComponentKind, ResourceManager};
use crate::service::ContextService;

pub struct RepoEntry {
    pub service: Arc<ContextService>,
    pub last_accessed: Instant,
    pub client_ids: Vec<String>, // clients attached to this repo (telemetry)
    pub active_queries: usize,
}

struct ClientInfo {
    root: PathBuf,
    last_heartbeat: Instant,
}

pub struct RepositoryRegistry {
    repos: RwLock<HashMap<PathBuf, RepoEntry>>,
    pub resource_manager: Arc<ResourceManager>,
    pub global_semaphore: Arc<Semaphore>,
    idle_timeout: Duration,
    // real client tracking
    clients: RwLock<HashMap<String, ClientInfo>>,
    client_id_counter: AtomicU64,
    client_timeout: Duration,
}

impl RepositoryRegistry {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            repos: RwLock::new(HashMap::new()),
            resource_manager: Arc::new(ResourceManager::new(budget_bytes)),
            global_semaphore: Arc::new(Semaphore::new(1)),
            idle_timeout: Duration::from_secs(300),
            clients: RwLock::new(HashMap::new()),
            client_id_counter: AtomicU64::new(1),
            client_timeout: Duration::from_secs(60),
        }
    }

    pub async fn get_or_create(&self, root: PathBuf) -> Arc<ContextService> {
        let canon = Self::canon(&root);
        {
            let g = self.repos.read().await;
            if let Some(e) = g.get(&canon) {
                return e.service.clone();
            }
        }
        let mut g = self.repos.write().await;
        if let Some(e) = g.get(&canon) {
            return e.service.clone();
        }
        // create new service with resource manager and semaphore
        let svc = ContextService::new_with_resources(
            Some(canon.clone()),
            self.resource_manager.clone(),
            self.global_semaphore.clone(),
        )
        .await
        .expect("create service");
        let arc = Arc::new(svc);
        g.insert(
            canon.clone(),
            RepoEntry {
                service: arc.clone(),
                last_accessed: Instant::now(),
                client_ids: Vec::new(),
                active_queries: 0,
            },
        );
        arc
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

    pub async fn touch(&self, root: &Path) {
        let canon = Self::canon(root);
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(&canon) {
            e.last_accessed = Instant::now();
        }
    }

    pub async fn inc_client(&self, root: &Path) {
        let canon = Self::canon(root);
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(&canon) {
            e.last_accessed = Instant::now();
        }
    }

    pub async fn inc_client_with_svc(&self, root: PathBuf, svc: Arc<ContextService>) {
        let canon = Self::canon(&root);
        // For backward compat with R1.1 tests, also register a synthetic client
        let cid = format!(
            "test-{}-{}",
            self.client_id_counter.fetch_add(1, Ordering::SeqCst),
            Instant::now().elapsed().as_nanos() % 100000
        );
        {
            let mut clients = self.clients.write().await;
            clients.insert(
                cid.clone(),
                ClientInfo {
                    root: canon.clone(),
                    last_heartbeat: Instant::now(),
                },
            );
        }
        let mut g = self.repos.write().await;
        let e = g.entry(canon.clone()).or_insert_with(|| RepoEntry {
            service: svc.clone(),
            last_accessed: Instant::now(),
            client_ids: Vec::new(),
            active_queries: 0,
        });
        e.last_accessed = Instant::now();
        e.service = svc;
        e.client_ids.push(cid);
    }

    pub async fn dec_client(&self, root: &Path) {
        let canon = Self::canon(root);
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(&canon) {
            e.last_accessed = Instant::now();
        }
    }

    // Real client tracking
    pub async fn register_client(&self, root: PathBuf) -> String {
        let canon = Self::canon(&root);
        let id = format!(
            "{}-{}-{}",
            std::process::id(),
            self.client_id_counter.fetch_add(1, Ordering::SeqCst),
            Instant::now().elapsed().as_nanos() % 1000000
        );
        {
            let mut clients = self.clients.write().await;
            clients.insert(
                id.clone(),
                ClientInfo {
                    root: canon.clone(),
                    last_heartbeat: Instant::now(),
                },
            );
        }
        {
            let mut repos = self.repos.write().await;
            if let Some(e) = repos.get_mut(&canon) {
                e.client_ids.push(id.clone());
            }
        }
        id
    }

    pub async fn unregister_client(&self, client_id: &str) {
        let info = {
            let mut clients = self.clients.write().await;
            clients.remove(client_id)
        };
        if let Some(info) = info {
            let mut repos = self.repos.write().await;
            if let Some(e) = repos.get_mut(&info.root) {
                e.client_ids.retain(|id| id != client_id);
            }
        }
    }

    pub async fn heartbeat(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.last_heartbeat = Instant::now();
        }
    }

    pub async fn cleanup_stale_clients(&self) {
        let now = Instant::now();
        let stale: Vec<(String, PathBuf)> = {
            let clients = self.clients.read().await;
            clients
                .iter()
                .filter(|(_, info)| now.duration_since(info.last_heartbeat) > self.client_timeout)
                .map(|(k, v)| (k.clone(), v.root.clone()))
                .collect()
        };
        for (id, root) in stale {
            {
                let mut clients = self.clients.write().await;
                clients.remove(&id);
            }
            {
                let mut repos = self.repos.write().await;
                if let Some(e) = repos.get_mut(&root) {
                    e.client_ids.retain(|cid| cid != &id);
                }
            }
        }
    }

    pub async fn runtime_count(&self) -> usize {
        let g = self.repos.read().await;
        g.len()
    }

    pub async fn client_counts(&self) -> HashMap<PathBuf, usize> {
        let g = self.repos.read().await;
        g.iter()
            .map(|(k, v)| (k.clone(), v.client_ids.len()))
            .collect()
    }

    pub async fn global_client_count(&self) -> usize {
        let g = self.clients.read().await;
        g.len()
    }

    pub async fn repository_client_count(&self, root: &Path) -> usize {
        let canon = Self::canon(root);
        let g = self.repos.read().await;
        g.get(&canon).map(|e| e.client_ids.len()).unwrap_or(0)
    }

    pub async fn evict_idle(&self) {
        self.cleanup_stale_clients().await;
        let now = Instant::now();
        let mut g = self.repos.write().await;
        let to_remove: Vec<PathBuf> = g
            .iter()
            .filter(|(_, e)| {
                // client registration does NOT keep runtime alive; only active_queries and timeout
                e.active_queries == 0 && now.duration_since(e.last_accessed) > self.idle_timeout
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in to_remove {
            if let Some(entry) = g.remove(&k) {
                // clean resource manager entries for this root
                self.resource_manager.remove(&k, ComponentKind::Bm25).await;
                self.resource_manager
                    .remove(&k, ComponentKind::Vectors)
                    .await;
                // hot already dropped with service Arc
                let _ = entry;
            }
        }
    }

    pub async fn evict_hot_component(&self, root: &Path, kind: ComponentKind) {
        let canon = Self::canon(root);
        let g = self.repos.read().await;
        if let Some(e) = g.get(&canon) {
            let _ = e.service.evict_hot(kind).await;
        }
        self.resource_manager.remove(&canon, kind).await;
    }

    pub async fn total_hot_bytes(&self) -> usize {
        self.resource_manager.total_bytes().await
    }

    // Global budget enforcement: estimate, ensure, evict physically, register
    #[allow(unused_variables)]
    pub async fn ensure_hot_budget(
        &self,
        root: &Path,
        kind: ComponentKind,
        estimated_bytes: usize,
    ) -> bool {
        if estimated_bytes > self.resource_manager.budget_bytes() {
            return false;
        }
        let rm = self.resource_manager.clone();
        // Clone registry Arc for eviction callback? We need &self in closure, but closure is FnMut sync.
        // We do manual loop instead of calling rm.ensure_budget with callback that needs self.
        let total = rm.total_bytes().await;
        if total + estimated_bytes <= rm.budget_bytes() {
            return true;
        }
        // Use rm.ensure_budget with callback that also evicts hot
        let mut evicted_roots: Vec<(PathBuf, ComponentKind)> = Vec::new();
        let ok = rm
            .ensure_budget(estimated_bytes, |r, k| {
                evicted_roots.push((r.to_path_buf(), k));
            })
            .await;
        for (r, k) in evicted_roots {
            self.evict_hot_component(&r, k).await;
        }
        ok
    }

    pub async fn inc_active_query(&self, root: &Path) {
        let canon = Self::canon(root);
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(&canon) {
            e.active_queries += 1;
            e.last_accessed = Instant::now();
        }
    }

    pub async fn dec_active_query(&self, root: &Path) {
        let canon = Self::canon(root);
        let mut g = self.repos.write().await;
        if let Some(e) = g.get_mut(&canon) {
            e.active_queries = e.active_queries.saturating_sub(1);
            e.last_accessed = Instant::now();
        }
    }
}
