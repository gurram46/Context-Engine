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
use std::path::PathBuf;
use tracing::info;

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
}

impl McpAdapter {
    pub async fn new(root: Option<PathBuf>) -> Result<Self, anyhow::Error> {
        // Resolve canonical root for daemon identity
        let canon = crate::daemon::resolve_canonical_root(root.clone()).unwrap_or_else(|_| {
            root.clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        });
        // Try attach to existing daemon first (thin client)
        if let Some(client) = crate::daemon::try_attach(&canon).await {
            info!(addr=%client.addr, root=%canon.display(), "attaching to shared daemon");
            return Ok(Self {
                service: ServiceKind::Remote(client),
                tool_router: Self::tool_router(),
            });
        }
        // No daemon, create local service (heavy) and try to become daemon
        let svc = ContextService::new(root.clone()).await?;
        let svc_arc = Arc::new(svc);
        let canon_clone = canon.clone();
        let svc_clone = svc_arc.clone();
        // Try to become daemon in background (non-blocking)
        tokio::spawn(async move {
            // small delay to reduce race
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            match crate::daemon::try_become_daemon(&canon_clone, svc_clone.clone()).await {
                Ok((listener, meta)) => {
                    info!(port=%meta.port, pid=%meta.pid, root=%canon_clone.display(), "became shared daemon");
                    let server = crate::daemon::DaemonServer {
                        // we need to construct via public fields; for now we use try_become_daemon that returns listener and then run
                        // Instead we handle here: create DaemonServer and run
                        // This path is simplified: we already have listener, need to run
                        // We'll create a new DaemonServer via struct literal if fields are pub, but they are private.
                        // So we re-implement: just use daemon to serve
                        // For minimal, we leak and spawn handle via daemon module's helper
                        // We'll call a helper that takes listener and service
                        listener,
                        service: svc_clone,
                        root: canon_clone,
                        client_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    };
                    // This requires DaemonServer fields to be pub; if not, we fallback to simple serve loop
                    // For now, just run via daemon::handle
                    server.run().await;
                }
                Err(e) => {
                    // Check if someone else became daemon, try attach cleanup not needed
                    tracing::debug!(error=%e, "not daemon, will remain local");
                    // If stale lock, cleanup
                    if crate::daemon::is_stale(&canon_clone).await {
                        crate::daemon::cleanup_stale(&canon_clone).await;
                    }
                }
            }
        });
        Ok(Self {
            service: ServiceKind::Local(svc_arc),
            tool_router: Self::tool_router(),
        })
    }

    fn opts(params_budget: Option<u32>, params_max: Option<u32>) -> SearchOptions {
        SearchOptions {
            budget_tokens: params_budget.unwrap_or(10000) as usize,
            max_results: params_max.unwrap_or(10) as usize,
            debug: false,
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
        let res = match &self.service {
            ServiceKind::Local(svc) => svc
                .search(
                    &params.query,
                    Self::opts(params.budgetTokens, params.maxResults),
                )
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            ServiceKind::Remote(client) => client
                .search(
                    &params.query,
                    Self::opts(params.budgetTokens, params.maxResults),
                )
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        };
        let stats_json = serde_json::to_value(&res.stats).unwrap_or(serde_json::json!({}));
        let out = serde_json::json!({
            "query": res.query,
            "type": res.query_type.as_str(),
            "context": res.packed.markdown,
            "evidence": res.evidence.iter().map(|e| serde_json::json!({
                "file": e.file,
                "lines": e.start_line.map(|s| format!("{}-{}", s, e.end_line.unwrap_or(s))).unwrap_or_default(),
                "symbol": e.symbol,
                "relation": e.relation.map(|r| r.as_str()),
                "source": e.source.as_str(),
                "score": e.score,
                "authorityScore": e.authority_score,
                "finalScore": e.final_score,
            })).collect::<Vec<_>>(),
            "stats": stats_json
        });
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
