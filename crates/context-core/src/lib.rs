#![allow(non_snake_case)]
//! Shared domain and MCP types for contextd.
//! R0: only stable MCP contract types. No retrieval/ranking logic.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Tool param structs mirror V2 schemas exactly.

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextSearchParams {
    /// Natural language question or literal
    pub query: String,
    #[serde(default)]
    #[schemars(description = "Token budget for packed context, default 8000")]
    pub budgetTokens: Option<u32>,
    #[serde(default)]
    #[schemars(description = "Max evidence items, default 10")]
    pub maxResults: Option<u32>,
    #[serde(default)]
    #[schemars(description = "Include debug metadata")]
    pub debug: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SymbolLookupParams {
    pub symbol: String,
    #[serde(default)]
    pub budgetTokens: Option<u32>,
    #[serde(default)]
    pub debug: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DependencyTraceParams {
    pub symbol: String,
    /// direction of trace
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default)]
    pub budgetTokens: Option<u32>,
    #[serde(default)]
    pub debug: Option<bool>,
}

fn default_direction() -> String {
    "callers".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TestLookupParams {
    /// Feature or symbol
    pub query: String,
    #[serde(default)]
    pub budgetTokens: Option<u32>,
    #[serde(default)]
    pub debug: Option<bool>,
}

// Empty for status
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextStatusParams {}

/// Status extra from Rust runtime (merged with V2 status)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustStatus {
    pub contextdVersion: String,
    pub rustVersion: String,
    pub pid: u32,
    pub projectRoot: String,
}

/// Error type for bridge
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("V2 child failed to start: {0}")]
    ChildStart(String),
    #[error("V2 child exited: {0}")]
    ChildExited(String),
    #[error("MCP timeout: {0}")]
    Timeout(String),
    #[error("OCI unavailable: {0}")]
    Oci(String),
    #[error("invalid project root: {0}")]
    InvalidRoot(String),
    #[error("internal: {0}")]
    Internal(String),
}
