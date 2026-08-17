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
pub struct McpAdapter {
    service: Arc<ContextService>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl McpAdapter {
    pub async fn new(root: Option<PathBuf>) -> Result<Self, anyhow::Error> {
        let svc = ContextService::new(root).await?;
        Ok(Self {
            service: Arc::new(svc),
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
        let res = self
            .service
            .search(
                &params.query,
                Self::opts(params.budgetTokens, params.maxResults),
            )
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
        let res = self
            .service
            .symbol(&params.symbol, Self::opts(params.budgetTokens, None))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
        let res = self
            .service
            .dependency(&params.symbol, dir, Self::opts(params.budgetTokens, None))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
        let res = self
            .service
            .tests(&params.query, Self::opts(params.budgetTokens, None))
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
        let st = self
            .service
            .status()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let v = serde_json::to_value(&st).unwrap_or(serde_json::json!({}));
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
    // tracing to stderr already? Ensure
    info!("starting contextd mcp adapter");
    let adapter = McpAdapter::new(root).await?;
    let server = adapter.serve(stdio()).await?;
    info!("contextd mcp serving on stdio");
    let quit = server.waiting().await?;
    info!(reason=?quit, "contextd mcp shutting down");
    Ok(())
}
