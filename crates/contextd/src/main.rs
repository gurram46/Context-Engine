mod bridge;

use bridge::V2Bridge;
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
use serde_json::json;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct Contextd {
    bridge: Arc<V2Bridge>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl Contextd {
    pub fn new(bridge: Arc<V2Bridge>) -> Self {
        Self {
            bridge,
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
        let v = self
            .bridge
            .call_json("context_search", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
        let v = self
            .bridge
            .call_json("symbol_lookup", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
        let v = self
            .bridge
            .call_json("dependency_trace", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
        let v = self
            .bridge
            .call_json("test_lookup", args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
