#![allow(dead_code, unused_imports, clippy::all)]
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use context_core::ContextError;

/// LEGACY / HISTORICAL / BENCHMARK — NOT used in production R5 retrieval.
/// Production uses native Rust pipeline only. Kept for archaeology/benchmark comparison.
/// Raw candidate provider — talks directly to `open-codebase-index` via `codeIndexClient` methods,
/// not via V2's `context_search` etc which already do ranking.
/// Spawns a Node child running `v2/dist/candidateProvider.js` (new) that directly calls
/// `lookupImplementation`, `codebase_peek`, `codebase_search`, `call_graph` etc and returns
/// raw `Evidence` JSON.
pub struct CandidateProvider {
    child: Mutex<Option<Child>>,
    pending:
        std::sync::Arc<Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<Value>>>>,
    next_id: std::sync::atomic::AtomicU64,
    v2_path: PathBuf,
    current_root: Mutex<PathBuf>,
}

impl CandidateProvider {
    pub fn new() -> Result<Self, ContextError> {
        let v2_path = Self::resolve_candidate_path()?;
        let project_root = Self::resolve_project_root()?;
        Ok(Self {
            child: Mutex::new(None),
            pending: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            next_id: std::sync::atomic::AtomicU64::new(1),
            v2_path,
            current_root: Mutex::new(project_root),
        })
    }

    fn resolve_candidate_path() -> Result<PathBuf, ContextError> {
        for key in ["CONTEXTD_CANDIDATE_PATH", "CONTEXTD_V2_PATH"] {
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
                    let cand = c.join("v2/dist/candidateProvider.js");
                    if cand.exists() {
                        return Ok(cand);
                    }
                    let cand2 = c.join("v2").join("dist").join("candidateProvider.js");
                    if cand2.exists() {
                        return Ok(cand2);
                    }
                    cur = c.parent().map(PathBuf::from);
                } else {
                    break;
                }
            }
        }
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(ws) = manifest_dir.parent().and_then(|p| p.parent()) {
            let cand = ws.join("v2/dist/candidateProvider.js");
            if cand.exists() {
                return Ok(cand.canonicalize().unwrap_or(cand));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            let cand = cwd.join("v2/dist/candidateProvider.js");
            if cand.exists() {
                return Ok(cand);
            }
            if let Some(parent) = cwd.parent() {
                let cand = parent.join("v2/dist/candidateProvider.js");
                if cand.exists() {
                    return Ok(cand);
                }
            }
        }
        let fallback =
            PathBuf::from("C:/Users/Dell/context/Context-Engine/v2/dist/candidateProvider.js");
        if fallback.exists() {
            return Ok(fallback);
        }
        Err(ContextError::Internal(
            "cannot find v2/dist/candidateProvider.js — run npm run build --prefix v2".into(),
        ))
    }

    fn resolve_project_root() -> Result<PathBuf, ContextError> {
        if let Ok(p) = std::env::var("CONTEXT_ENGINE_PROJECT_ROOT") {
            let pb = PathBuf::from(&p);
            if pb.exists() {
                return Ok(pb.canonicalize().unwrap_or(pb));
            }
        }
        std::env::current_dir()
            .map(|p| p.canonicalize().unwrap_or(p))
            .map_err(|e| ContextError::InvalidRoot(format!("cannot get cwd: {}", e)))
    }

    async fn ensure_child(&self) -> Result<()> {
        let desired_root = Self::resolve_project_root().unwrap_or_else(|_| PathBuf::from("."));
        {
            let mut cur = self.current_root.lock().await;
            let mut guard = self.child.lock().await;
            if let Some(_child) = guard.as_ref() {
                if *cur == desired_root {
                    return Ok(());
                }
                // Root changed, kill old child
                if let Some(mut old) = guard.take() {
                    let _ = old.kill().await;
                }
                *cur = desired_root.clone();
            } else {
                *cur = desired_root.clone();
            }
        }
        let mut child = Command::new("node")
            .arg(&self.v2_path)
            .current_dir(&desired_root)
            .env("CONTEXT_ENGINE_PROJECT_ROOT", &desired_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().unwrap();
        let pending = self.pending.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let mut buf = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        buf.push_str(&line);
                        // Try to parse JSON per line (candidate provider uses line-delimited JSON)
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                            if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
                                if let Some(tx) = pending.lock().await.remove(&id) {
                                    let _ = tx.send(v);
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Do initialize handshake
        let mut pending_map = self.pending.lock().await;
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (tx, rx) = tokio::sync::oneshot::channel();
        pending_map.insert(id, tx);
        drop(pending_map);
        let init = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "contextd-candidate", "version": "0.1.0" }
            }
        });
        let stdin = child.stdin.as_mut().unwrap();
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(format!("{}\n", serde_json::to_string(&init)?).as_bytes())
            .await?;
        stdin.flush().await?;
        // Wait for init response (with timeout)
        let _resp = tokio::time::timeout(std::time::Duration::from_secs(10), rx)
            .await
            .map_err(|_| anyhow::anyhow!("candidate provider init timeout"))?
            .map_err(|_| anyhow::anyhow!("candidate provider init channel closed"))?;
        // Send initialized notification
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        stdin
            .write_all(format!("{}\n", serde_json::to_string(&notif)?).as_bytes())
            .await?;
        stdin.flush().await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        *self.child.lock().await = Some(child);
        Ok(())
    }

    async fn call_raw(&self, method: &str, params: Value) -> Result<Value, ContextError> {
        self.ensure_child()
            .await
            .map_err(|e| ContextError::Internal(e.to_string()))?;
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let line = serde_json::to_string(&req).unwrap() + "\n";
        {
            let mut guard = self.child.lock().await;
            let child = guard
                .as_mut()
                .ok_or_else(|| ContextError::Internal("candidate child not running".into()))?;
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| ContextError::Internal("candidate stdin not piped".into()))?;
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| ContextError::Internal(e.to_string()))?;
            stdin
                .flush()
                .await
                .map_err(|e| ContextError::Internal(e.to_string()))?;
        }
        let resp = tokio::time::timeout(std::time::Duration::from_secs(15), rx)
            .await
            .map_err(|_| ContextError::Timeout(format!("candidate {} timeout", method)))?
            .map_err(|_| ContextError::Internal("candidate channel closed".into()))?;
        if let Some(err) = resp.get("error") {
            return Err(ContextError::Internal(format!(
                "candidate {} error: {}",
                method, err
            )));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    fn extract_candidates_from_tool_result(res: Value) -> Vec<Value> {
        // res is CallToolResult { content: [{ type: "text", text: "{\"candidates\": [...]}" }], ... }
        // Try direct {candidates} first (for backwards compat), then parse content[0].text
        if let Some(arr) = res.get("candidates").and_then(|v| v.as_array()) {
            return arr.clone();
        }
        if let Some(arr) = res.as_array() {
            return arr.clone();
        }
        if let Some(content) = res.get("content").and_then(|v| v.as_array()) {
            for item in content {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                        if let Some(arr) = parsed.get("candidates").and_then(|v| v.as_array()) {
                            return arr.clone();
                        }
                        if let Some(arr) = parsed.as_array() {
                            return arr.clone();
                        }
                    }
                }
            }
        }
        // Fallback: if result is object with text field
        if let Some(text) = res.get("text").and_then(|t| t.as_str()) {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                if let Some(arr) = parsed.get("candidates").and_then(|v| v.as_array()) {
                    return arr.clone();
                }
            }
        }
        vec![]
    }

    #[allow(dead_code)]
    pub async fn symbol_candidates(&self, symbol: &str) -> Result<Vec<Value>, ContextError> {
        let res = self
            .call_raw(
                "tools/call",
                serde_json::json!({ "name": "symbol_candidates", "arguments": { "symbol": symbol } }),
            )
            .await?;
        Ok(Self::extract_candidates_from_tool_result(res))
    }

    #[allow(dead_code)]
    pub async fn semantic_candidates(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Value>, ContextError> {
        let res = self
            .call_raw(
                "tools/call",
                serde_json::json!({ "name": "semantic_candidates", "arguments": { "query": query, "limit": limit } }),
            )
            .await?;
        Ok(Self::extract_candidates_from_tool_result(res))
    }

    #[allow(dead_code)]
    pub async fn graph_candidates(
        &self,
        symbol: &str,
        direction: &str,
    ) -> Result<Vec<Value>, ContextError> {
        let res = self
            .call_raw(
                "tools/call",
                serde_json::json!({ "name": "graph_candidates", "arguments": { "symbol": symbol, "direction": direction } }),
            )
            .await?;
        Ok(Self::extract_candidates_from_tool_result(res))
    }

    #[allow(dead_code)]
    pub async fn test_candidates(&self, query: &str) -> Result<Vec<Value>, ContextError> {
        let res = self
            .call_raw(
                "tools/call",
                serde_json::json!({ "name": "test_candidates", "arguments": { "query": query } }),
            )
            .await?;
        Ok(Self::extract_candidates_from_tool_result(res))
    }

    #[allow(dead_code)]
    pub async fn pid(&self) -> Option<u32> {
        self.child.lock().await.as_ref().and_then(|c| c.id())
    }

    #[allow(dead_code)]
    pub async fn is_alive(&self) -> bool {
        if let Some(child) = self.child.lock().await.as_mut() {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}
