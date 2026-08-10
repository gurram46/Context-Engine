mod bridge;
mod pipeline;
mod project;

use bridge::V2Bridge;
use context_core::{
    ContextSearchParams, DependencyTraceParams, SymbolLookupParams, TestLookupParams,
};
use pipeline::{retrieve_context, Providers};
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
        let project = self
            .project_cache
            .ensure()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let providers = Providers {
            v2: self.bridge.clone(),
        };
        let res = retrieve_context(
            &params.query,
            &project,
            &providers,
            params.budgetTokens.unwrap_or(10000) as usize,
            params.maxResults.unwrap_or(10) as usize,
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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
            "stats": {
                "candidate_count": res.stats.candidate_count,
                "evidence_count": res.stats.evidence_count,
                "files_returned": res.stats.files_returned,
                "packed_tokens": res.stats.packed_tokens,
                "retrievers": res.stats.retrievers_used,
                "elapsed_ms": res.stats.elapsed_ms,
            }
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
        let project = self
            .project_cache
            .ensure()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let providers = Providers {
            v2: self.bridge.clone(),
        };
        let res = retrieve_context(
            &params.symbol,
            &project,
            &providers,
            params.budgetTokens.unwrap_or(10000) as usize,
            10,
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let out = serde_json::json!({
            "query": res.query,
            "type": res.query_type.as_str(),
            "context": res.packed.markdown,
            "evidence": res.evidence.iter().map(|e| serde_json::json!({
                "file": e.file,
                "symbol": e.symbol,
                "relation": e.relation.map(|r| r.as_str()),
                "source": e.source.as_str(),
                "finalScore": e.final_score,
            })).collect::<Vec<_>>(),
            "stats": res.stats,
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
        let dir = params.direction.clone();
        if !["callers", "callees", "both"].contains(&dir.as_str()) {
            return Err(McpError::invalid_params(
                "direction must be callers|callees|both",
                None,
            ));
        }
        let project = self
            .project_cache
            .ensure()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let providers = Providers {
            v2: self.bridge.clone(),
        };
        let query = match dir.as_str() {
            "callers" => format!("What calls {}?", params.symbol),
            "callees" => format!("What does {} call?", params.symbol),
            _ => format!("dependency of {}", params.symbol),
        };
        let res = retrieve_context(
            &query,
            &project,
            &providers,
            params.budgetTokens.unwrap_or(10000) as usize,
            10,
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let out = serde_json::json!({
            "query": res.query,
            "type": res.query_type.as_str(),
            "context": res.packed.markdown,
            "evidence": res.evidence,
            "stats": res.stats,
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
        let project = self
            .project_cache
            .ensure()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let providers = Providers {
            v2: self.bridge.clone(),
        };
        let q = if params.query.to_lowercase().contains("test") {
            params.query.clone()
        } else {
            format!("What tests cover {}?", params.query)
        };
        let res = retrieve_context(
            &q,
            &project,
            &providers,
            params.budgetTokens.unwrap_or(10000) as usize,
            10,
        )
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let out = serde_json::json!({
            "query": res.query,
            "type": res.query_type.as_str(),
            "context": res.packed.markdown,
            "evidence": res.evidence,
            "stats": res.stats,
        });
        Ok(CallToolResult::success(vec![
            rmcp::model::ContentBlock::text(
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.to_string()),
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
            // R1/R2: Rust discovery stats
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
            // R2: routing/ranking backends
            obj.insert("routingBackend".to_string(), json!("rust"));
            obj.insert("rankingBackend".to_string(), json!("rust"));
            obj.insert("packingBackend".to_string(), json!("rust"));
            obj.insert("exactBackend".to_string(), json!("rust-rg"));
            obj.insert("semanticBackend".to_string(), json!("v2-oci"));
            obj.insert("symbolBackend".to_string(), json!("v2-oci"));
            obj.insert("graphBackend".to_string(), json!("v2-oci"));
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
