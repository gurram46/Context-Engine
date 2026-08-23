use std::path::PathBuf;
use std::sync::Arc;

use context_core::{
    ContextSearchParams, DependencyTraceParams, SymbolLookupParams, TestLookupParams,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use tracing::{info, warn};

use crate::service::{ContextService, Direction, SearchOptions};

#[derive(Clone)]
enum ServiceKind {
    Local(Arc<ContextService>),
    Remote(crate::daemon::RemoteClient),
}

#[derive(Clone)]
pub struct McpAdapter {
    service: ServiceKind,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    // real client tracking
    client_id: Option<String>,
    client_root: Option<PathBuf>,
}

fn is_global_daemon_disabled() -> bool {
    std::env::var("CONTEXTD_GLOBAL_DAEMON_DISABLE")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

fn spawn_detached_daemon() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon");
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        cmd.creation_flags(0x00000008 | 0x00000200 | 0x08000000);
    }
    #[cfg(unix)]
    {
        // nothing extra, spawn detached via setsid is not needed; just spawn
    }
    let _ = cmd.spawn()?;
    Ok(())
}

impl McpAdapter {
    pub async fn new(root: Option<PathBuf>) -> Result<Self, anyhow::Error> {
        let canon = crate::daemon::resolve_canonical_root(root.clone()).unwrap_or_else(|_| {
            root.clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        });
        // If explicitly disabled, fallback to local
        if is_global_daemon_disabled() {
            warn!("global daemon disabled via CONTEXTD_GLOBAL_DAEMON_DISABLE, using local");
            let svc = ContextService::new(root.clone()).await?;
            return Ok(Self {
                service: ServiceKind::Local(Arc::new(svc)),
                tool_router: Self::tool_router(),
                client_id: None,
                client_root: None,
            });
        }

        // Bounded retry/backoff for daemon startup
        // Try attach first
        if let Some(client) = crate::daemon::try_attach_global().await {
            let remote = crate::daemon::RemoteClient {
                addr: client.addr.clone(),
                root: canon.clone(),
                pid: client.pid,
            };
            // register real client
            let client_id = Self::register_with_daemon(&remote, &canon).await;
            info!(addr=%remote.addr, root=%canon.display(), client_id=?client_id, "attached to global daemon");
            return Ok(Self {
                service: ServiceKind::Remote(remote),
                tool_router: Self::tool_router(),
                client_id,
                client_root: Some(canon),
            });
        }

        // No daemon, try to spawn detached with startup singleflight
        // Use global lock file as startup guard: try to create startup lock
        // But we use spawn_detached and retry, avoiding many heavy fallbacks
        let mut spawned = false;
        for attempt in 0..30 {
            if attempt == 0 {
                // First attempt: try spawn if no daemon
                if crate::daemon::is_global_stale().await {
                    crate::daemon::cleanup_global_stale().await;
                }
                // Only spawn if still no daemon
                if crate::daemon::try_attach_global().await.is_none() {
                    if let Err(e) = spawn_detached_daemon() {
                        warn!(error=%e, "failed to spawn detached daemon");
                    } else {
                        spawned = true;
                        info!("spawned detached daemon");
                    }
                }
            }
            // wait backoff 100ms * attempt factor
            let backoff = std::time::Duration::from_millis(100 * (attempt as u64 / 5 + 1));
            tokio::time::sleep(backoff).await;
            if let Some(client) = crate::daemon::try_attach_global().await {
                let remote = crate::daemon::RemoteClient {
                    addr: client.addr.clone(),
                    root: canon.clone(),
                    pid: client.pid,
                };
                let client_id = Self::register_with_daemon(&remote, &canon).await;
                info!(addr=%remote.addr, root=%canon.display(), client_id=?client_id, attempt=%attempt, "attached after spawn");
                return Ok(Self {
                    service: ServiceKind::Remote(remote),
                    tool_router: Self::tool_router(),
                    client_id,
                    client_root: Some(canon),
                });
            }
            if attempt == 15 && !spawned {
                // mid-retry: try spawn again if still not up
                if crate::daemon::try_attach_global().await.is_none() {
                    let _ = spawn_detached_daemon();
                }
            }
        }

        // Bounded retry exhausted: check if fatal
        if crate::daemon::is_global_stale().await {
            crate::daemon::cleanup_global_stale().await;
        }
        // Only fallback to local if daemon truly not available after bounded retry
        // This must be observable
        warn!("global daemon not available after bounded retry, falling back to local heavy (observable)");
        let svc = ContextService::new(root.clone()).await?;
        Ok(Self {
            service: ServiceKind::Local(Arc::new(svc)),
            tool_router: Self::tool_router(),
            client_id: None,
            client_root: None,
        })
    }

    async fn register_with_daemon(
        remote: &crate::daemon::RemoteClient,
        root: &std::path::Path,
    ) -> Option<String> {
        // Generate stable client id
        let id = format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                % 1_000_000,
            rand_u32()
        );
        let params = serde_json::json!({"client_id": id, "root": root.display().to_string()});
        match remote.call("register_client", params).await {
            Ok(v) => {
                // daemon may return client_id echo
                if let Some(cid) = v.get("client_id").and_then(|x| x.as_str()) {
                    Some(cid.to_string())
                } else {
                    Some(id)
                }
            }
            Err(_) => Some(id), // still track locally
        }
    }

    fn opts(params_budget: Option<u32>, params_max: Option<u32>) -> SearchOptions {
        SearchOptions {
            budget_tokens: params_budget.unwrap_or(10000) as usize,
            max_results: params_max.unwrap_or(10) as usize,
            debug: false,
        }
    }
}

fn rand_u32() -> u32 {
    // simple xorshift based on time and pid
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let pid = std::process::id() as u64;
    let mut x = t ^ (pid << 32);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    (x & 0xFFFFFFFF) as u32
}

impl Drop for McpAdapter {
    fn drop(&mut self) {
        if let (Some(client_id), Some(root), ServiceKind::Remote(remote)) = (
            self.client_id.clone(),
            self.client_root.clone(),
            self.service.clone(),
        ) {
            let remote_clone = remote.clone();
            tokio::task::spawn(async move {
                let params =
                    serde_json::json!({"client_id": client_id, "root": root.display().to_string()});
                let _ = remote_clone.call("unregister_client", params).await;
            });
        }
    }
}

#[tool_router]
impl McpAdapter {
    #[tool(
        description = "General codebase question. Uses hybrid retrieval (ripgrep + semantic + symbol) with authority ranking."
    )]
    async fn context_search(
        &self,
        Parameters(params): Parameters<ContextSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.query.trim().is_empty() {
            return Err(McpError::invalid_params("query is required", None));
        }
        let out = match &self.service {
            ServiceKind::Local(svc) => {
                let wp = svc
                    .search_external(
                        &params.query,
                        Self::opts(params.budgetTokens, params.maxResults),
                    )
                    .await
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                let stats_json =
                    serde_json::to_value(&wp.result.stats).unwrap_or(serde_json::json!({}));
                let proof_json = serde_json::to_value(&wp.proof).unwrap_or(serde_json::json!({}));
                serde_json::json!({
                    "query": wp.result.query,
                    "type": wp.result.query_type.as_str(),
                    "context": wp.result.packed.markdown,
                    "evidence": wp.result.evidence.iter().map(|e| serde_json::json!({
                        "file": e.file,
                        "lines": e.start_line.map(|s| format!("{}-{}", s, e.end_line.unwrap_or(s))).unwrap_or_default(),
                        "symbol": e.symbol,
                        "relation": e.relation.map(|r| r.as_str()),
                        "source": e.source.as_str(),
                        "score": e.score,
                        "authorityScore": e.authority_score,
                        "finalScore": e.final_score,
                    })).collect::<Vec<_>>(),
                    "stats": stats_json,
                    "proof": proof_json
                })
            }
            ServiceKind::Remote(client) => {
                // Use raw call to preserve proof field from daemon
                let params_json = serde_json::json!({"query": params.query, "budgetTokens": params.budgetTokens, "maxResults": params.maxResults});
                let v = client
                    .call("context_search", params_json)
                    .await
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                // v is already result object containing query/type/context/evidence/stats/proof
                v
            }
        };
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.to_string()),
            ),
        ]))
    }

    #[tool(description = "Find authoritative definition for a symbol (function/class).")]
    async fn symbol_lookup(
        &self,
        Parameters(params): Parameters<SymbolLookupParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.symbol.trim().is_empty() {
            return Err(McpError::invalid_params("symbol is required", None));
        }
        let res = match &self.service {
            ServiceKind::Local(svc) => svc
                .symbol(&params.symbol, Self::opts(params.budgetTokens, None))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            ServiceKind::Remote(client) => client
                .symbol(&params.symbol, Self::opts(params.budgetTokens, None))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        };
        let out = serde_json::json!({
            "query": res.query,
            "type": res.query_type.as_str(),
            "context": res.packed.markdown,
            "evidence": res.evidence,
            "stats": res.stats
        });
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.to_string()),
            ),
        ]))
    }

    #[tool(
        description = "Who calls / what does it call. Uses graph + exact fallback for dynamic registrations."
    )]
    async fn dependency_trace(
        &self,
        Parameters(params): Parameters<DependencyTraceParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.symbol.trim().is_empty() {
            return Err(McpError::invalid_params("symbol is required", None));
        }
        if !["callers", "callees", "both"].contains(&params.direction.as_str()) {
            return Err(McpError::invalid_params(
                "direction must be callers|callees|both",
                None,
            ));
        }
        let dir = Direction::from_str(&params.direction);
        let res = match &self.service {
            ServiceKind::Local(svc) => svc
                .dependency(&params.symbol, dir, Self::opts(params.budgetTokens, None))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            ServiceKind::Remote(client) => client
                .dependency(&params.symbol, dir, Self::opts(params.budgetTokens, None))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        };
        let out = serde_json::json!({
            "query": res.query,
            "type": res.query_type.as_str(),
            "context": res.packed.markdown,
            "evidence": res.evidence,
            "stats": res.stats
        });
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.to_string()),
            ),
        ]))
    }

    #[tool(description = "Find tests covering feature/symbol.")]
    async fn test_lookup(
        &self,
        Parameters(params): Parameters<TestLookupParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.query.trim().is_empty() {
            return Err(McpError::invalid_params("query is required", None));
        }
        let res = match &self.service {
            ServiceKind::Local(svc) => svc
                .tests(&params.query, Self::opts(params.budgetTokens, None))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            ServiceKind::Remote(client) => client
                .tests(&params.query, Self::opts(params.budgetTokens, None))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        };
        let out = serde_json::json!({
            "query": res.query,
            "type": res.query_type.as_str(),
            "context": res.packed.markdown,
            "evidence": res.evidence,
            "stats": res.stats
        });
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.to_string()),
            ),
        ]))
    }

    #[tool(description = "Diagnostics: version, branch, index, rg, node.")]
    async fn context_status(&self) -> Result<CallToolResult, McpError> {
        let v = match &self.service {
            ServiceKind::Local(svc) => {
                let st = svc
                    .status()
                    .await
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                serde_json::to_value(&st).unwrap_or(serde_json::json!({}))
            }
            ServiceKind::Remote(client) => client
                .status()
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        };
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
            ),
        ]))
    }
}

#[tool_handler]
impl ServerHandler for McpAdapter {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Context Engine Rust native service (R5) — exact/structure/BM25/semantic",
        )
    }
}

pub async fn run_mcp(root: Option<PathBuf>) -> anyhow::Result<()> {
    info!("starting contextd mcp adapter");
    let adapter = McpAdapter::new(root).await?;
    let server = adapter.serve(stdio()).await?;
    info!("contextd mcp serving on stdio");
    let quit = server.waiting().await?;
    info!(reason=?quit, "contextd mcp shutting down");
    Ok(())
}
