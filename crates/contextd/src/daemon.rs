#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

use crate::service::{ContextService, Direction, SearchOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonMetadata {
    pub pid: u32,
    pub port: u16,
    pub root: String,
    pub started_at: u64,
}

fn daemon_dir(root: &Path) -> PathBuf {
    root.join(".context")
}

fn daemon_file(root: &Path) -> PathBuf {
    daemon_dir(root).join("daemon.json")
}

fn lock_file(root: &Path) -> PathBuf {
    daemon_dir(root).join("daemon.lock")
}

fn canonical_root(root: Option<PathBuf>) -> Result<PathBuf, anyhow::Error> {
    let pr = context_index::ProjectRoot::resolve(root.as_deref())?;
    Ok(pr.path().to_path_buf())
}

// Public for mcp.rs to resolve
pub fn resolve_canonical_root(root: Option<PathBuf>) -> Result<PathBuf, anyhow::Error> {
    canonical_root(root)
}

pub async fn try_attach(root: &Path) -> Option<RemoteClient> {
    let df = daemon_file(root);
    let content = tokio::fs::read_to_string(&df).await.ok()?;
    let meta: DaemonMetadata = serde_json::from_str(&content).ok()?;
    let addr = format!("127.0.0.1:{}", meta.port);
    // quick connect with timeout 800ms
    let conn = tokio::time::timeout(Duration::from_millis(800), TcpStream::connect(&addr)).await;
    match conn {
        Ok(Ok(_)) => Some(RemoteClient {
            addr,
            root: root.to_path_buf(),
            pid: meta.pid,
        }),
        _ => None,
    }
}

pub async fn is_stale(root: &Path) -> bool {
    // if daemon file exists but not connectable, stale
    if try_attach(root).await.is_some() {
        return false;
    }
    // if daemon file exists but not attachable, stale
    tokio::fs::try_exists(daemon_file(root))
        .await
        .unwrap_or(false)
}

pub async fn cleanup_stale(root: &Path) {
    let _ = tokio::fs::remove_file(daemon_file(root)).await;
    let _ = tokio::fs::remove_file(lock_file(root)).await;
}

pub struct DaemonServer {
    pub listener: TcpListener,
    pub service: Arc<ContextService>,
    pub root: PathBuf,
    pub client_count: Arc<AtomicUsize>,
}

impl DaemonServer {
    pub async fn run(self) {
        info!(port = %self.listener.local_addr().map(|a|a.port()).unwrap_or(0), root=%self.root.display(), "daemon listening");
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let svc = self.service.clone();
                    let cc = self.client_count.clone();
                    cc.fetch_add(1, Ordering::Relaxed);
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, svc).await {
                            warn!(error=%e, "daemon client error");
                        }
                        cc.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(e) => {
                    warn!(error=%e, "daemon accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

async fn handle_client(mut stream: TcpStream, service: Arc<ContextService>) -> Result<()> {
    let (r, mut w) = stream.split();
    let mut reader = tokio::io::BufReader::new(r);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = serde_json::json!({"id": null, "error": format!("bad json: {}", e)});
                w.write_all(serde_json::to_string(&resp)?.as_bytes())
                    .await?;
                w.write_all(b"\n").await?;
                continue;
            }
        };
        let id = req.get("id").cloned().unwrap_or(serde_json::json!(null));
        let tool = req.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(serde_json::json!({}));
        let resp = match tool {
            "context_search" => {
                let q = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let maxr = params
                    .get("maxResults")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: maxr,
                    debug: false,
                };
                match service.search(q, opts).await {
                    Ok(r) => {
                        let stats = serde_json::to_value(&r.stats).unwrap_or(serde_json::json!({}));
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": stats}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "symbol_lookup" => {
                let sym = params.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: 10,
                    debug: false,
                };
                match service.symbol(sym, opts).await {
                    Ok(r) => {
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": r.stats}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "dependency_trace" => {
                let sym = params.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                let dir = params
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("callers");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: 10,
                    debug: false,
                };
                let d = Direction::from_str(dir);
                match service.dependency(sym, d, opts).await {
                    Ok(r) => {
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": r.stats}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "test_lookup" => {
                let q = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: 10,
                    debug: false,
                };
                match service.tests(q, opts).await {
                    Ok(r) => {
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": r.stats}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "context_status" => {
                match service.status().await {
                    Ok(st) => {
                        let v = serde_json::to_value(&st).unwrap_or(serde_json::json!({}));
                        // augment with daemon fields is done in service.status() already; just return
                        serde_json::json!({"id": id, "result": v})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            _ => serde_json::json!({"id": id, "error": format!("unknown tool {}", tool)}),
        };
        w.write_all(serde_json::to_string(&resp)?.as_bytes())
            .await?;
        w.write_all(b"\n").await?;
        w.flush().await?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct RemoteClient {
    pub addr: String,
    pub root: PathBuf,
    pub pid: u32,
}

impl RemoteClient {
    async fn call(
        &self,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let mut stream =
            tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&self.addr)).await??;
        let id = 1;
        let req = serde_json::json!({"id": id, "tool": tool, "params": params, "root": self.root.display().to_string()});
        stream
            .write_all(serde_json::to_string(&req)?.as_bytes())
            .await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;
        let mut reader = tokio::io::BufReader::new(stream);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(30), reader.read_line(&mut line)).await??;
        let resp: serde_json::Value = serde_json::from_str(&line)?;
        if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
            anyhow::bail!("{}", err);
        }
        Ok(resp.get("result").cloned().unwrap_or(serde_json::json!({})))
    }

    pub async fn search(
        &self,
        query: &str,
        opts: SearchOptions,
    ) -> Result<crate::pipeline::ContextResult, anyhow::Error> {
        let params = serde_json::json!({"query": query, "budgetTokens": opts.budget_tokens, "maxResults": opts.max_results});
        let v = self.call("context_search", params).await?;
        // Reconstruct ContextResult from daemon's JSON: it returns {query,type,context,evidence,stats}
        // We need to parse evidence as Vec<Evidence> and packed.markdown
        // For thin client, we can just return a ContextResult that mcp will serialize as JSON again.
        // Instead we return serde_json::Value and let mcp forward? Simpler: mcp thin will just forward JSON directly, not via ContextResult.
        // But this method is used by mcp thin to get JSON to return. So we can just return the Value.
        // To keep trait, we return ContextResult by deserializing.
        let query_out = v
            .get("query")
            .and_then(|x| x.as_str())
            .unwrap_or(query)
            .to_string();
        let typ_str = v
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("conceptual");
        let qt = match typ_str {
            "symbol" => context_rank::types::QueryType::Symbol,
            "dependency" => context_rank::types::QueryType::Dependency,
            "test" => context_rank::types::QueryType::Test,
            "exact" => context_rank::types::QueryType::Exact,
            "mixed" => context_rank::types::QueryType::Mixed,
            _ => context_rank::types::QueryType::Conceptual,
        };
        let context_md = v
            .get("context")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let evidence: Vec<context_rank::types::Evidence> =
            serde_json::from_value(v.get("evidence").cloned().unwrap_or(serde_json::json!([])))
                .unwrap_or_default();
        let stats = crate::pipeline::PipelineStats {
            candidate_count: 0,
            evidence_count: evidence.len(),
            files_returned: 0,
            packed_tokens: 0,
            retrievers_used: vec![],
            elapsed_ms: 0,
            exact_ms: 0,
            structural_ms: 0,
            bm25_ms: 0,
            semantic_ms: 0,
            rank_ms: 0,
            pack_ms: 0,
            total_ms: None,
            discovery_ms: None,
            reconcile_ms: None,
            semantic_embed_ms: None,
            semantic_search_ms: None,
            fusion_ms: None,
            authority_ms: None,
            generation: None,
            dirty_file_count: None,
            vector_count_scanned: None,
            cache_hit: None,
            reconcile_skipped: None,
            discovery_calls: None,
            reconcile_calls: None,
            runtime_state: None,
            runtime_access_ms: None,
            graph_ms: None,
            test_ms: None,
            sqlite_open_ms: None,
            sqlite_open_calls: None,
            sqlite_query_ms: None,
            vector_load_ms: None,
            vector_scan_ms: None,
            filesystem_ms: None,
            files_read: None,
            query_embedding_cache_hit: None,
            result_cache_hit: None,
            total_internal_ms: None,
            wall_ms: None,
        };
        let packed = context_rank::packer::PackedResult {
            markdown: context_md,
            token_estimate: stats.packed_tokens,
            files: vec![],
        };
        Ok(crate::pipeline::ContextResult {
            query: query_out,
            query_type: qt,
            evidence,
            packed,
            stats,
        })
    }

    pub async fn symbol(
        &self,
        symbol: &str,
        opts: SearchOptions,
    ) -> Result<crate::pipeline::ContextResult, anyhow::Error> {
        let params = serde_json::json!({"symbol": symbol, "budgetTokens": opts.budget_tokens});
        let v = self.call("symbol_lookup", params).await?;
        self.search(
            v.get("query").and_then(|x| x.as_str()).unwrap_or(symbol),
            opts,
        )
        .await // fallback
        .or_else(|_| {
            let evidence: Vec<context_rank::types::Evidence> =
                serde_json::from_value(v.get("evidence").cloned().unwrap_or(serde_json::json!([])))
                    .unwrap_or_default();
            Ok(crate::pipeline::ContextResult {
                query: symbol.to_string(),
                query_type: context_rank::types::QueryType::Symbol,
                evidence,
                packed: context_rank::packer::PackedResult {
                    markdown: v
                        .get("context")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    token_estimate: 0,
                    files: vec![],
                },
                stats: crate::pipeline::PipelineStats {
                    candidate_count: 0,
                    evidence_count: 0,
                    files_returned: 0,
                    packed_tokens: 0,
                    retrievers_used: vec![],
                    elapsed_ms: 0,
                    exact_ms: 0,
                    structural_ms: 0,
                    bm25_ms: 0,
                    semantic_ms: 0,
                    rank_ms: 0,
                    pack_ms: 0,
                    total_ms: None,
                    discovery_ms: None,
                    reconcile_ms: None,
                    semantic_embed_ms: None,
                    semantic_search_ms: None,
                    fusion_ms: None,
                    authority_ms: None,
                    generation: None,
                    dirty_file_count: None,
                    vector_count_scanned: None,
                    cache_hit: None,
                    reconcile_skipped: None,
                    discovery_calls: None,
                    reconcile_calls: None,
                    runtime_state: None,
                    runtime_access_ms: None,
                    graph_ms: None,
                    test_ms: None,
                    sqlite_open_ms: None,
                    sqlite_open_calls: None,
                    sqlite_query_ms: None,
                    vector_load_ms: None,
                    vector_scan_ms: None,
                    filesystem_ms: None,
                    files_read: None,
                    query_embedding_cache_hit: None,
                    result_cache_hit: None,
                    total_internal_ms: None,
                    wall_ms: None,
                },
            })
        })
    }

    pub async fn dependency(
        &self,
        symbol: &str,
        dir: Direction,
        opts: SearchOptions,
    ) -> Result<crate::pipeline::ContextResult, anyhow::Error> {
        let params = serde_json::json!({"symbol": symbol, "direction": dir.as_str(), "budgetTokens": opts.budget_tokens});
        let v = self.call("dependency_trace", params).await?;
        let evidence: Vec<context_rank::types::Evidence> =
            serde_json::from_value(v.get("evidence").cloned().unwrap_or(serde_json::json!([])))
                .unwrap_or_default();
        let stats = crate::pipeline::PipelineStats {
            candidate_count: 0,
            evidence_count: evidence.len(),
            files_returned: 0,
            packed_tokens: 0,
            retrievers_used: vec![],
            elapsed_ms: 0,
            exact_ms: 0,
            structural_ms: 0,
            bm25_ms: 0,
            semantic_ms: 0,
            rank_ms: 0,
            pack_ms: 0,
            total_ms: None,
            discovery_ms: None,
            reconcile_ms: None,
            semantic_embed_ms: None,
            semantic_search_ms: None,
            fusion_ms: None,
            authority_ms: None,
            generation: None,
            dirty_file_count: None,
            vector_count_scanned: None,
            cache_hit: None,
            reconcile_skipped: None,
            discovery_calls: None,
            reconcile_calls: None,
            runtime_state: None,
            runtime_access_ms: None,
            graph_ms: None,
            test_ms: None,
            sqlite_open_ms: None,
            sqlite_open_calls: None,
            sqlite_query_ms: None,
            vector_load_ms: None,
            vector_scan_ms: None,
            filesystem_ms: None,
            files_read: None,
            query_embedding_cache_hit: None,
            result_cache_hit: None,
            total_internal_ms: None,
            wall_ms: None,
        };
        Ok(crate::pipeline::ContextResult {
            query: symbol.to_string(),
            query_type: context_rank::types::QueryType::Dependency,
            evidence,
            packed: context_rank::packer::PackedResult {
                markdown: v
                    .get("context")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                token_estimate: 0,
                files: vec![],
            },
            stats,
        })
    }

    pub async fn tests(
        &self,
        query: &str,
        opts: SearchOptions,
    ) -> Result<crate::pipeline::ContextResult, anyhow::Error> {
        let params = serde_json::json!({"query": query, "budgetTokens": opts.budget_tokens});
        let v = self.call("test_lookup", params).await?;
        let evidence: Vec<context_rank::types::Evidence> =
            serde_json::from_value(v.get("evidence").cloned().unwrap_or(serde_json::json!([])))
                .unwrap_or_default();
        let stats = crate::pipeline::PipelineStats {
            candidate_count: 0,
            evidence_count: evidence.len(),
            files_returned: 0,
            packed_tokens: 0,
            retrievers_used: vec![],
            elapsed_ms: 0,
            exact_ms: 0,
            structural_ms: 0,
            bm25_ms: 0,
            semantic_ms: 0,
            rank_ms: 0,
            pack_ms: 0,
            total_ms: None,
            discovery_ms: None,
            reconcile_ms: None,
            semantic_embed_ms: None,
            semantic_search_ms: None,
            fusion_ms: None,
            authority_ms: None,
            generation: None,
            dirty_file_count: None,
            vector_count_scanned: None,
            cache_hit: None,
            reconcile_skipped: None,
            discovery_calls: None,
            reconcile_calls: None,
            runtime_state: None,
            runtime_access_ms: None,
            graph_ms: None,
            test_ms: None,
            sqlite_open_ms: None,
            sqlite_open_calls: None,
            sqlite_query_ms: None,
            vector_load_ms: None,
            vector_scan_ms: None,
            filesystem_ms: None,
            files_read: None,
            query_embedding_cache_hit: None,
            result_cache_hit: None,
            total_internal_ms: None,
            wall_ms: None,
        };
        Ok(crate::pipeline::ContextResult {
            query: query.to_string(),
            query_type: context_rank::types::QueryType::Test,
            evidence,
            packed: context_rank::packer::PackedResult {
                markdown: v
                    .get("context")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                token_estimate: 0,
                files: vec![],
            },
            stats,
        })
    }

    pub async fn status(&self) -> Result<serde_json::Value, anyhow::Error> {
        self.call("context_status", serde_json::json!({})).await
    }
}

pub fn global_daemon_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CONTEXTD_GLOBAL_DAEMON_DIR") {
        return PathBuf::from(p);
    }
    if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("context-engine")
    } else {
        std::env::var("XDG_RUNTIME_DIR")
            .map(|p| PathBuf::from(p).join("context-engine"))
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(".cache/context-engine"))
                    .unwrap_or_else(|_| std::env::temp_dir().join("context-engine"))
            })
    }
}

pub fn global_daemon_file() -> PathBuf {
    global_daemon_dir().join("daemon.json")
}

pub fn global_lock_file() -> PathBuf {
    global_daemon_dir().join("daemon.lock")
}

pub async fn try_attach_global() -> Option<RemoteClient> {
    let df = global_daemon_file();
    let content = tokio::fs::read_to_string(&df).await.ok()?;
    let meta: DaemonMetadata = serde_json::from_str(&content).ok()?;
    let addr = format!("127.0.0.1:{}", meta.port);
    let conn = tokio::time::timeout(Duration::from_millis(800), TcpStream::connect(&addr)).await;
    match conn {
        Ok(Ok(_)) => Some(RemoteClient {
            addr,
            root: PathBuf::from(meta.root.clone()),
            pid: meta.pid,
        }),
        _ => None,
    }
}

pub async fn is_global_stale() -> bool {
    if try_attach_global().await.is_some() {
        return false;
    }
    tokio::fs::try_exists(global_daemon_file())
        .await
        .unwrap_or(false)
}

pub async fn cleanup_global_stale() {
    let _ = tokio::fs::remove_file(global_daemon_file()).await;
    let _ = tokio::fs::remove_file(global_lock_file()).await;
}

pub struct GlobalDaemon {
    pub registry: Arc<crate::registry::RepositoryRegistry>,
    pub listener: TcpListener,
    pub meta: DaemonMetadata,
}

impl GlobalDaemon {
    pub async fn new() -> Result<Self, anyhow::Error> {
        let dir = global_daemon_dir();
        tokio::fs::create_dir_all(&dir).await.ok();
        let budget = crate::config::memory_budget_bytes();
        let registry = Arc::new(crate::registry::RepositoryRegistry::new(budget));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let pid = std::process::id();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let meta = DaemonMetadata {
            pid,
            port,
            root: dir.display().to_string(),
            started_at: now,
        };
        Ok(Self {
            registry,
            listener,
            meta,
        })
    }

    pub async fn run(self) {
        let registry = self.registry.clone();
        // spawn idle eviction task
        let reg_clone = registry.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                reg_clone.evict_idle().await;
            }
        });
        info!(port=%self.listener.local_addr().map(|a|a.port()).unwrap_or(0), "global daemon listening");
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let reg = registry.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_global_client(stream, reg).await {
                            warn!(error=%e, "global daemon client error");
                        }
                    });
                }
                Err(e) => {
                    warn!(error=%e, "global daemon accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

async fn handle_global_client(
    mut stream: TcpStream,
    registry: Arc<crate::registry::RepositoryRegistry>,
) -> Result<()> {
    let (r, mut w) = stream.split();
    let mut reader = tokio::io::BufReader::new(r);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = serde_json::json!({"id": null, "error": format!("bad json: {}", e)});
                w.write_all(serde_json::to_string(&resp)?.as_bytes())
                    .await?;
                w.write_all(b"\n").await?;
                continue;
            }
        };
        let id = req.get("id").cloned().unwrap_or(serde_json::json!(null));
        let tool = req.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(serde_json::json!({}));
        let root_str = req
            .get("root")
            .and_then(|v| v.as_str())
            .or_else(|| params.get("root").and_then(|v| v.as_str()))
            .unwrap_or("");
        let root_path = if root_str.is_empty() {
            // fallback to resolve via ProjectRoot for this process's cwd? Use global dir's root is not correct; try to resolve via params
            // If no root, use current dir
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            PathBuf::from(root_str)
        };
        let canon = context_index::ProjectRoot::resolve(Some(&root_path))
            .map(|pr| pr.path().to_path_buf())
            .unwrap_or(root_path.clone());
        // ensure repo entry exists
        let svc = registry.get_or_create(canon.clone()).await;
        // touch for LRU
        registry.touch(&canon).await;
        // per-request client count not needed here; global daemon tracks via separate map, but we use registry client counts via inc/dec on connect/disconnect?
        // For now, we increment active query count
        // Handle tool
        let resp = match tool {
            "context_search" => {
                let q = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let maxr = params
                    .get("maxResults")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: maxr,
                    debug: false,
                };
                // global memory budget check before hot load: estimate needed bytes
                // For search, we need to know if hot vectors will be used; we approximate 6MB per repo as in test, but real is vector_count*dim*4
                // We can let service handle budget via hot load's internal check, but global check should be here
                // For now, just call service and let it handle per-repo budget; global eviction will be triggered if needed via ResourceManager
                // We should ensure ResourceManager is consulted
                // Acquire global semaphore for semantic indexing? Queries are not indexing, so not needed
                match svc.search(q, opts).await {
                    Ok(r) => {
                        let stats = serde_json::to_value(&r.stats).unwrap_or(serde_json::json!({}));
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": stats, "root": canon.display().to_string()}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "symbol_lookup" => {
                let sym = params.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: 10,
                    debug: false,
                };
                match svc.symbol(sym, opts).await {
                    Ok(r) => {
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": r.stats}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "dependency_trace" => {
                let sym = params.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                let dir = params
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("callers");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: 10,
                    debug: false,
                };
                let d = Direction::from_str(dir);
                match svc.dependency(sym, d, opts).await {
                    Ok(r) => {
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": r.stats}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "test_lookup" => {
                let q = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let budget = params
                    .get("budgetTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10000) as usize;
                let opts = SearchOptions {
                    budget_tokens: budget,
                    max_results: 10,
                    debug: false,
                };
                match svc.tests(q, opts).await {
                    Ok(r) => {
                        serde_json::json!({"id": id, "result": {"query": r.query, "type": r.query_type.as_str(), "context": r.packed.markdown, "evidence": r.evidence, "stats": r.stats}})
                    }
                    Err(e) => serde_json::json!({"id": id, "error": e.to_string()}),
                }
            }
            "context_status" => {
                // Global status: aggregate
                let st = svc.status().await.unwrap_or_else(|_| {
                    // dummy
                    crate::service::StatusReport {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        contextd_version: env!("CARGO_PKG_VERSION").to_string(),
                        rust_version: "1.80".to_string(),
                        pid: std::process::id(),
                        project_root: canon.display().to_string(),
                        git_branch: None,
                        index_generation: None,
                        files_indexed: 0,
                        symbols: 0,
                        bm25_documents: 0,
                        vector_count: 0,
                        embedding_model: "".to_string(),
                        embedding_dimension: 0,
                        embedding_runtime: "none".to_string(),
                        semantic_available: false,
                        semantic_backend_available: false,
                        semantic_index_ready: false,
                        eligible_chunk_count: 0,
                        semantic_ref_count: 0,
                        representation_version: "".to_string(),
                        missing_vector_count: 0,
                        stale_vector_count: 0,
                        watcher_state: "none".to_string(),
                        store_schema_version: None,
                        daemon_pid: Some(std::process::id()),
                        client_count: None,
                        shared_runtime: true,
                        memory_budget_mb: crate::config::memory_budget_bytes() / (1024 * 1024),
                        hot_bm25_loaded: false,
                        hot_vectors_loaded: false,
                        hot_vector_count: 0,
                        estimated_hot_vector_bytes: 0,
                        generation: None,
                        global_client_count: None,
                        repository_client_count: None,
                        repository_runtime_count: None,
                        global_memory_budget_mb: None,
                        estimated_hot_memory_bytes: None,
                        repository_estimated_hot_memory_bytes: None,
                    }
                });
                let mut v = serde_json::to_value(&st).unwrap_or(serde_json::json!({}));
                // augment with global registry info
                let global_clients = registry.global_client_count().await;
                let runtime_cnt = registry.runtime_count().await;
                let total_hot = registry.total_hot_bytes().await;
                v["globalClientCount"] = serde_json::json!(global_clients);
                v["repositoryRuntimeCount"] = serde_json::json!(runtime_cnt);
                v["globalMemoryBudgetMb"] =
                    serde_json::json!(crate::config::memory_budget_bytes() / (1024 * 1024));
                v["estimatedHotMemoryBytes"] = serde_json::json!(total_hot);
                v["sharedRuntime"] = serde_json::json!(true);
                v["daemonPid"] = serde_json::json!(std::process::id());
                serde_json::json!({"id": id, "result": v})
            }
            _ => serde_json::json!({"id": id, "error": format!("unknown tool {}", tool)}),
        };
        w.write_all(serde_json::to_string(&resp)?.as_bytes())
            .await?;
        w.write_all(b"\n").await?;
        w.flush().await?;
    }
    Ok(())
}

pub async fn try_become_global_daemon() -> Result<(TcpListener, DaemonMetadata), anyhow::Error> {
    let dir = global_daemon_dir();
    tokio::fs::create_dir_all(&dir).await.ok();
    let lock_path = global_lock_file();
    let mut opts = tokio::fs::OpenOptions::new();
    opts.create_new(true).write(true);
    let lock_res = opts.open(&lock_path).await;
    match lock_res {
        Ok(mut f) => {
            let pid = std::process::id();
            let _ = f.write_all(pid.to_string().as_bytes()).await;
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let port = listener.local_addr()?.port();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let meta = DaemonMetadata {
                pid,
                port,
                root: dir.display().to_string(),
                started_at: now,
            };
            let json = serde_json::to_string_pretty(&meta)?;
            tokio::fs::write(global_daemon_file(), json).await?;
            Ok((listener, meta))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("global lock exists")
        }
        Err(e) => Err(e.into()),
    }
}

// Singleton acquire
pub async fn try_become_daemon(
    root: &Path,
    _service: Arc<ContextService>,
) -> Result<(TcpListener, DaemonMetadata), anyhow::Error> {
    let lock_path = lock_file(root);
    let dir = daemon_dir(root);
    tokio::fs::create_dir_all(&dir).await.ok();
    // try create_new
    let mut opts = tokio::fs::OpenOptions::new();
    opts.create_new(true).write(true);
    let lock_res = opts.open(&lock_path).await;
    match lock_res {
        Ok(mut f) => {
            let pid = std::process::id();
            let _ = f.write_all(pid.to_string().as_bytes()).await;
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let port = listener.local_addr()?.port();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let meta = DaemonMetadata {
                pid,
                port,
                root: root.display().to_string(),
                started_at: now,
            };
            let json = serde_json::to_string_pretty(&meta)?;
            tokio::fs::write(daemon_file(root), json).await?;
            // spawn server in background is done by caller; we return listener
            // keep lock file open? we already wrote pid; keep file exists as lease
            Ok((listener, meta))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("lock exists")
        }
        Err(e) => Err(e.into()),
    }
}
