//! V2 child bridge — one persistent Node process.
//! For R0 we delegate to the existing TypeScript backend.
//! Handles lifecycle, project-root forwarding, single restart, clean shutdown.

use anyhow::{Context, Result};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ContentBlock},
    service::{RunningService, ServiceExt},
    transport::TokioChildProcess,
    RoleClient,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use context_core::ContextError;
use rmcp::transport::ConfigureCommandExt;

/// Resolve v2/dist/mcp/server.js path.
fn resolve_v2_path() -> Result<PathBuf> {
    for key in ["CONTEXTD_V2_PATH", "CONTEXT_ENGINE_V2_PATH"] {
        if let Ok(p) = std::env::var(key) {
            let pb = PathBuf::from(&p);
            if pb.exists() {
                return Ok(pb);
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent().map(PathBuf::from);
        for _ in 0..6 {
            if let Some(c) = cur.clone() {
                let cand = c.join("v2/dist/mcp/server.js");
                if cand.exists() {
                    return Ok(cand);
                }
                cur = c.parent().map(PathBuf::from);
            } else {
                break;
            }
        }
    }
    let manifest_cand =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../v2/dist/mcp/server.js");
    if manifest_cand.exists() {
        return Ok(manifest_cand.canonicalize().unwrap_or(manifest_cand));
    }
    if let Ok(cwd) = std::env::current_dir() {
        let cand = cwd.join("v2/dist/mcp/server.js");
        if cand.exists() {
            return Ok(cand);
        }
        if let Some(parent) = cwd.parent() {
            let cand = parent.join("v2/dist/mcp/server.js");
            if cand.exists() {
                return Ok(cand);
            }
        }
    }
    let fallback = PathBuf::from("C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js");
    if fallback.exists() {
        return Ok(fallback);
    }
    anyhow::bail!("cannot find v2/dist/mcp/server.js — set CONTEXTD_V2_PATH");
}

fn project_root() -> PathBuf {
    if let Ok(p) = std::env::var("CONTEXT_ENGINE_PROJECT_ROOT") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return pb;
        }
        warn!(path = %pb.display(), "CONTEXT_ENGINE_PROJECT_ROOT does not exist, using cwd");
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Extract text from CallToolResult content (V2 returns single text block with JSON).
fn extract_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct V2Bridge {
    v2_path: PathBuf,
    /// current project root for which client was spawned
    current_root: Mutex<PathBuf>,
    client: Mutex<Option<RunningService<RoleClient, ()>>>,
    restart_count: Mutex<u32>,
}

impl V2Bridge {
    pub fn new() -> Result<Self> {
        let v2_path = resolve_v2_path().context("resolve v2 path")?;
        let root = project_root();
        info!(v2_path = %v2_path.display(), root = %root.display(), "V2Bridge created");
        Ok(Self {
            v2_path,
            current_root: Mutex::new(root),
            client: Mutex::new(None),
            restart_count: Mutex::new(0),
        })
    }

    /// For tests: create with explicit paths
    #[cfg(test)]
    pub fn new_with_paths(v2_path: PathBuf, root: PathBuf) -> Self {
        Self {
            v2_path,
            current_root: Mutex::new(root),
            client: Mutex::new(None),
            restart_count: Mutex::new(0),
        }
    }

    async fn ensure_client(&self) -> Result<()> {
        let desired_root = project_root();
        let mut cur = self.current_root.lock().await;
        let mut guard = self.client.lock().await;
        let needs_restart = if let Some(_c) = guard.as_ref() {
            *cur != desired_root
        } else {
            true
        };
        if !needs_restart {
            return Ok(());
        }
        if guard.is_some() {
            info!(old = %cur.display(), new = %desired_root.display(), "project root changed, restarting V2 child");
            if let Some(old) = guard.take() {
                let _ = old.cancel().await;
            }
        }
        // Spawn new child via rmcp transport
        let transport = TokioChildProcess::new(Command::new("node").configure(|cmd| {
            cmd.arg(&self.v2_path)
                .current_dir(&desired_root)
                .env("CONTEXT_ENGINE_PROJECT_ROOT", &desired_root);
        }))
        .map_err(|e| {
            anyhow::anyhow!(format!(
                "failed to start V2 MCP child: command=node {} project_root={} source={}",
                self.v2_path.display(),
                desired_root.display(),
                e
            ))
        })?;
        let client = ().serve(transport).await.map_err(|e| {
            anyhow::anyhow!(format!(
                "V2 child handshake failed: project_root={} source={}",
                desired_root.display(),
                e
            ))
        })?;
        info!(root = %desired_root.display(), "V2 child started");
        *guard = Some(client);
        *cur = desired_root;
        Ok(())
    }

    async fn call_tool_raw(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<CallToolResult, ContextError> {
        self.ensure_client()
            .await
            .map_err(|e| ContextError::ChildStart(e.to_string()))?;

        let mut guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| ContextError::ChildStart("client not initialized".into()))?;

        let mut params = CallToolRequestParams::new(name.to_string());
        if let Some(obj) = arguments.as_object().cloned() {
            params = params.with_arguments(obj);
        }

        // Try once, with single restart on failure
        let res = client.call_tool(params.clone()).await;
        match res {
            Ok(r) => Ok(r),
            Err(e) => {
                let msg = e.to_string();
                error!(tool = %name, error = %msg, "V2 call failed, attempting restart");
                let mut rc = self.restart_count.lock().await;
                if *rc >= 1 {
                    return Err(ContextError::ChildExited(format!(
                        "V2 child failed and restart already attempted: {}",
                        msg
                    )));
                }
                *rc += 1;
                // Drop client and retry
                if let Some(old) = guard.take() {
                    let _ = old.cancel().await;
                }
                drop(guard);
                // Reset root to force respawn
                {
                    let mut cur = self.current_root.lock().await;
                    *cur = PathBuf::from("__needs_restart__");
                }
                self.ensure_client().await.map_err(|e2| {
                    ContextError::ChildStart(format!("restart failed: {} (original: {})", e2, msg))
                })?;
                let guard2 = self.client.lock().await;
                let client2 = guard2
                    .as_ref()
                    .ok_or_else(|| ContextError::ChildStart("restart client missing".into()))?;
                client2
                    .call_tool(params)
                    .await
                    .map_err(|e2| ContextError::ChildExited(format!("V2 retry failed: {}", e2)))
            }
        }
    }

    pub async fn call_json(&self, name: &str, arguments: Value) -> Result<Value, ContextError> {
        let result = self.call_tool_raw(name, arguments).await?;
        if result.is_error.unwrap_or(false) {
            let text = extract_text(&result);
            return Err(ContextError::Internal(format!(
                "V2 tool {} returned error: {}",
                name, text
            )));
        }
        let text = extract_text(&result);
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Ok(v),
            Err(_) => Ok(json!({"text": text})),
        }
    }

    pub async fn shutdown(&self) {
        let mut guard = self.client.lock().await;
        if let Some(c) = guard.take() {
            let _ = c.cancel().await;
            info!("V2 child shutdown requested");
        }
    }

    #[cfg(test)]
    pub async fn is_started(&self) -> bool {
        self.client.lock().await.is_some()
    }

    #[cfg(test)]
    pub async fn restart_count(&self) -> u32 {
        *self.restart_count.lock().await
    }
}

impl Drop for V2Bridge {
    fn drop(&mut self) {
        tracing::debug!("V2Bridge dropped");
    }
}
