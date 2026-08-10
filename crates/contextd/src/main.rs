mod bridge;
mod exact_shadow;
mod project;

use bridge::V2Bridge;
use context_core::{
    ContextSearchParams, DependencyTraceParams, SymbolLookupParams, TestLookupParams,
};
use project::ProjectCache;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use serde_json::json;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct Contextd {
    bridge: Arc<V2Bridge>,
    project_cache: Arc<ProjectCache>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl Contextd {
    pub fn new(bridge: Arc<V2Bridge>) -> Self {
        Self {
            bridge,
            project_cache: Arc::new(ProjectCache::new()),
            tool_router: Self::tool_router(),
        }
    }

    #[cfg(test)]
    pub fn with_cache(bridge: Arc<V2Bridge>, cache: Arc<ProjectCache>) -> Self {
        Self {
            bridge,
            project_cache: cache,
            tool_router: Self::tool_router(),
        }
    }

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
        let args = json!({
            "query": params.query,
            "budgetTokens": params.budgetTokens,
            "maxResults": params.maxResults,
            "debug": params.debug,
        });
        // Ensure project index (for shadow) — failures are non-fatal for R1
        let project = self.project_cache.ensure().await.ok();
        let v = self
            .bridge
            .call_json("context_search", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        // Shadow mode: compare Rust exact vs V2 exact (non-EXACT queries still run shadow for metrics)
        if let Some(idx) = project {
            let v_evidence = v
                .get("evidence")
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default();
            // Run shadow in background (don't block response)
            let q = params.query.clone();
            tokio::spawn(async move {
                let _ = exact_shadow::shadow_exact(&idx, &q, &v_evidence).await;
            });
        }
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
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
        let args = json!({
            "symbol": params.symbol,
            "budgetTokens": params.budgetTokens,
            "debug": params.debug,
        });
        let project = self.project_cache.ensure().await.ok();
        let v = self
            .bridge
            .call_json("symbol_lookup", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let Some(idx) = project {
            let v_evidence = v
                .get("evidence")
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default();
            let sym = params.symbol.clone();
            tokio::spawn(async move {
                let _ = exact_shadow::shadow_exact(&idx, &sym, &v_evidence).await;
            });
        }
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
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
        let dir = params.direction.clone();
        if !["callers", "callees", "both"].contains(&dir.as_str()) {
            return Err(McpError::invalid_params(
                "direction must be callers|callees|both",
                None,
            ));
        }
        let args = json!({
            "symbol": params.symbol,
            "direction": dir,
            "budgetTokens": params.budgetTokens,
            "debug": params.debug,
        });
        let project = self.project_cache.ensure().await.ok();
        let v = self
            .bridge
            .call_json("dependency_trace", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let Some(idx) = project {
            let v_evidence = v
                .get("evidence")
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default();
            let sym = params.symbol.clone();
            tokio::spawn(async move {
                let _ = exact_shadow::shadow_exact(&idx, &sym, &v_evidence).await;
            });
        }
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
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
        let args = json!({
            "query": params.query,
            "budgetTokens": params.budgetTokens,
            "debug": params.debug,
        });
        let project = self.project_cache.ensure().await.ok();
        let v = self
            .bridge
            .call_json("test_lookup", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let Some(idx) = project {
            let v_evidence = v
                .get("evidence")
                .and_then(|e| e.as_array())
                .cloned()
                .unwrap_or_default();
            let q = params.query.clone();
            tokio::spawn(async move {
                let _ = exact_shadow::shadow_exact(&idx, &q, &v_evidence).await;
            });
        }
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
            ),
        ]))
    }

    #[tool(description = "Diagnostics: version, branch, index, rg, node.")]
    async fn context_status(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .bridge
            .call_json("context_status", json!({}))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut merged = v.clone();
        if let Some(obj) = merged.as_object_mut() {
            let project_root = std::env::var("CONTEXT_ENGINE_PROJECT_ROOT").unwrap_or_else(|_| {
                std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| ".".into())
            });
            obj.insert("contextdVersion".to_string(), json!(VERSION));
            obj.insert("rustVersion".to_string(), json!(env!("CARGO_PKG_VERSION")));
            obj.insert("pid".to_string(), json!(std::process::id()));
            obj.insert("projectRoot".to_string(), json!(project_root));
            // R1: add Rust discovery stats if available
            if let Ok(idx) = self.project_cache.ensure().await {
                obj.insert(
                    "rustDiscoveredFiles".to_string(),
                    json!(idx.stats.discovered),
                );
                obj.insert("rustSourceFiles".to_string(), json!(idx.stats.source));
                obj.insert(
                    "rustIndexRoot".to_string(),
                    json!(idx.root.display().to_string()),
                );
            }
        }

        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&merged).unwrap_or_else(|_| merged.to_string()),
            ),
        ]))
    }
}

#[tool_handler]
impl ServerHandler for Contextd {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Context Engine Rust MCP shell (R0) over V2 backend")
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    info!(version = %VERSION, pid = %std::process::id(), "starting contextd");

    let bridge = Arc::new(V2Bridge::new()?);
    let service = Contextd::new(bridge.clone());

    let server = service.serve(stdio()).await?;
    info!("contextd serving on stdio");

    let quit = server.waiting().await?;
    info!(reason = ?quit, "contextd shutting down");

    bridge.shutdown().await;
    Ok(())
}
