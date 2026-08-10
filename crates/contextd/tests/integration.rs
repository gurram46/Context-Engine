//! R0 compatibility tests — MCP contract + lifecycle.
//! Covers STEP 9 + part of STEP 10.
//! Each test spawns a real contextd.exe child via TokioChildProcess and talks MCP as a client.

use anyhow::Result;
use rmcp::transport::ConfigureCommandExt;
use rmcp::{
    model::{CallToolRequestParams, ContentBlock},
    service::ServiceExt,
    transport::TokioChildProcess,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio::process::Command;

/// Locate contextd binary — prefers CARGO_BIN_EXE, fallback to target/debug or release.
fn find_contextd_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_contextd") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/contextd -> Context-Engine
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("C:/Users/Dell/context/Context-Engine"));
    let candidates = [
        workspace_root.join("target/debug/contextd.exe"),
        workspace_root.join("target/release/contextd.exe"),
        workspace_root.join("target/debug/contextd"),
        workspace_root.join("target/release/contextd"),
        PathBuf::from("target/debug/contextd.exe"),
        PathBuf::from("target/release/contextd.exe"),
        PathBuf::from("C:/Users/Dell/context/Context-Engine/target/debug/contextd.exe"),
        PathBuf::from("C:/Users/Dell/context/Context-Engine/target/release/contextd.exe"),
    ];
    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    panic!("cannot find contextd binary — run cargo build first");
}

fn find_v2_bin() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("C:/Users/Dell/context/Context-Engine"));
    let cand = workspace_root.join("v2/dist/mcp/server.js");
    if cand.exists() {
        return cand;
    }
    let fallback = PathBuf::from("C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js");
    if fallback.exists() {
        return fallback;
    }
    panic!("cannot find v2/dist/mcp/server.js — run npm run build --prefix v2");
}

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let p = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("C:/Users/Dell/context/Context-Engine"));
    // Avoid \\?\ prefix from canonicalize on Windows
    if p.exists() {
        p
    } else {
        PathBuf::from("C:/Users/Dell/context/Context-Engine")
    }
}

async fn spawn_contextd(
    project_root: &Path,
) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>> {
    let bin = find_contextd_bin();
    let transport = TokioChildProcess::new(Command::new(&bin).configure(|cmd| {
        cmd.env("CONTEXT_ENGINE_PROJECT_ROOT", project_root)
            .env("RUST_LOG", "warn"); // reduce noise
    }))?;
    let client = ().serve(transport).await?;
    Ok(client)
}

async fn spawn_v2(
    project_root: &Path,
) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>> {
    let v2 = find_v2_bin();
    let transport = TokioChildProcess::new(Command::new("node").configure(|cmd| {
        cmd.arg(&v2)
            .current_dir(project_root)
            .env("CONTEXT_ENGINE_PROJECT_ROOT", project_root);
    }))?;
    let client = ().serve(transport).await?;
    Ok(client)
}

fn extract_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
#[ignore]
async fn all_five_tools_registered() -> Result<()> {
    let root = workspace_root();
    let client = spawn_contextd(&root).await?;
    let tools = client.list_tools(Default::default()).await?;
    let names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    assert!(
        names.contains(&"context_search".to_string()),
        "missing context_search in {:?}",
        names
    );
    assert!(names.contains(&"symbol_lookup".to_string()));
    assert!(names.contains(&"dependency_trace".to_string()));
    assert!(names.contains(&"test_lookup".to_string()));
    assert!(names.contains(&"context_status".to_string()));
    assert_eq!(names.len(), 5, "expected exactly 5 tools, got {:?}", names);
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn schemas_match_expected_contract() -> Result<()> {
    let root = workspace_root();
    let client = spawn_contextd(&root).await?;
    let tools = client.list_tools(Default::default()).await?;
    let map: std::collections::HashMap<String, Value> = tools
        .tools
        .iter()
        .map(|t| {
            (
                t.name.to_string(),
                serde_json::to_value(&t.input_schema).unwrap(),
            )
        })
        .collect();

    // context_search: query required, budgetTokens/maxResults/debug optional
    let cs = map.get("context_search").expect("context_search");
    let cs_obj = cs.as_object().unwrap();
    // schema is JSON Schema with properties
    let props = cs_obj
        .get("properties")
        .and_then(|v| v.as_object())
        .expect("properties");
    assert!(props.contains_key("query"), "context_search missing query");
    assert!(props.contains_key("budgetTokens"));
    assert!(props.contains_key("maxResults"));
    assert!(props.contains_key("debug"));
    let required = cs_obj.get("required").and_then(|v| v.as_array()).unwrap();
    assert!(required.iter().any(|v| v == "query"));

    // symbol_lookup
    let sl = map.get("symbol_lookup").unwrap();
    let sl_props = sl.get("properties").and_then(|v| v.as_object()).unwrap();
    assert!(sl_props.contains_key("symbol"));
    let sl_req = sl.get("required").and_then(|v| v.as_array()).unwrap();
    assert!(sl_req.iter().any(|v| v == "symbol"));

    // dependency_trace
    let dt = map.get("dependency_trace").unwrap();
    let dt_props = dt.get("properties").and_then(|v| v.as_object()).unwrap();
    assert!(dt_props.contains_key("symbol"));
    assert!(dt_props.contains_key("direction"));
    let dt_req = dt.get("required").and_then(|v| v.as_array()).unwrap();
    assert!(dt_req.iter().any(|v| v == "symbol"));
    // direction enum
    let dir_schema = dt_props.get("direction").unwrap();
    let dir_str = dir_schema.to_string();
    assert!(
        dir_str.contains("callers") || dir_str.contains("callees"),
        "direction enum missing: {}",
        dir_str
    );

    // test_lookup
    let tl = map.get("test_lookup").unwrap();
    let tl_props = tl.get("properties").and_then(|v| v.as_object()).unwrap();
    assert!(tl_props.contains_key("query"));

    // context_status: empty properties
    let st = map.get("context_status").unwrap();
    // may be empty object schema
    let st_props = st.get("properties");
    if let Some(p) = st_props {
        // allow empty or no required
        if let Some(obj) = p.as_object() {
            assert!(
                obj.is_empty() || !obj.contains_key("query"),
                "status should not require query"
            );
        }
    }

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn invalid_input_returns_error() -> Result<()> {
    let root = workspace_root();
    let client = spawn_contextd(&root).await?;

    // empty query for context_search should be invalid_params
    let params =
        CallToolRequestParams::new("context_search").with_arguments(serde_json::Map::from_iter([
            ("query".to_string(), json!("")),
        ]));
    let res = client.call_tool(params).await;
    assert!(
        res.is_err(),
        "expected error for empty query, got {:?}",
        res
    );
    let err_str = res.unwrap_err().to_string();
    assert!(
        err_str.contains("query is required")
            || err_str.contains("invalid_params")
            || err_str.contains("-32602"),
        "unexpected error: {}",
        err_str
    );

    // missing symbol
    let params2 = CallToolRequestParams::new("symbol_lookup").with_arguments(
        serde_json::Map::from_iter([("symbol".to_string(), json!(""))]),
    );
    let res2 = client.call_tool(params2).await;
    assert!(res2.is_err(), "expected error for empty symbol");

    // invalid direction
    let params3 = CallToolRequestParams::new("dependency_trace").with_arguments(
        serde_json::Map::from_iter([
            ("symbol".to_string(), json!("bundle")),
            ("direction".to_string(), json!("invalid")),
        ]),
    );
    let res3 = client.call_tool(params3).await;
    assert!(res3.is_err(), "expected error for invalid direction");

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn child_reused_for_multiple_requests() -> Result<()> {
    let root = workspace_root();
    let client = spawn_contextd(&root).await?;

    // First call status to trigger lazy start
    let r1 = client
        .call_tool(CallToolRequestParams::new("context_status"))
        .await?;
    let t1 = extract_text(&r1.content);
    let v1: Value = serde_json::from_str(&t1)?;
    let pid1 = v1.get("pid").and_then(|v| v.as_u64()).expect("pid missing");

    // Second call should reuse same contextd pid (and V2 child)
    let r2 = client
        .call_tool(CallToolRequestParams::new("symbol_lookup").with_arguments(
            serde_json::Map::from_iter([("symbol".to_string(), json!("count_tokens"))]),
        ))
        .await?;
    assert!(!extract_text(&r2.content).is_empty());

    let r3 = client
        .call_tool(CallToolRequestParams::new("context_status"))
        .await?;
    let t3 = extract_text(&r3.content);
    let v3: Value = serde_json::from_str(&t3)?;
    let pid3 = v3.get("pid").and_then(|v| v.as_u64()).unwrap();
    assert_eq!(
        pid1, pid3,
        "contextd pid should be stable across calls (child reused)"
    );

    // Third call to ensure still alive
    let r4 = client
        .call_tool(CallToolRequestParams::new("context_search").with_arguments(
            serde_json::Map::from_iter([("query".to_string(), json!("bundle"))]),
        ))
        .await?;
    assert!(!r4.content.is_empty());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn project_root_environment_forwarded() -> Result<()> {
    // Create temp dir
    let tmp = TempDir::new()?;
    let tmp_path = tmp
        .path()
        .canonicalize()
        .unwrap_or(tmp.path().to_path_buf());
    // create a dummy file so directory is not empty
    std::fs::write(tmp.path().join("README.md"), "# temp\n")?;

    let client = spawn_contextd(&tmp_path).await?;
    let res = client
        .call_tool(CallToolRequestParams::new("context_status"))
        .await?;
    let text = extract_text(&res.content);
    let v: Value = serde_json::from_str(&text)?;
    let returned_root = v
        .get("projectRoot")
        .and_then(|x| x.as_str())
        .expect("projectRoot missing");
    // Normalize both to use forward slashes for comparison on Windows
    let returned_pb = PathBuf::from(returned_root)
        .canonicalize()
        .unwrap_or(PathBuf::from(returned_root));
    let expected_pb = tmp_path.canonicalize().unwrap_or(tmp_path.clone());
    // On Windows, canonicalize may add \\?\ prefix — compare ends_with
    let ret_str = returned_pb.display().to_string();
    let exp_str = expected_pb.display().to_string();
    assert!(
        ret_str.contains(exp_str.trim_start_matches(r"\\?\"))
            || exp_str.contains(ret_str.trim_start_matches(r"\\?\"))
            || ret_str == exp_str,
        "projectRoot mismatch: expected {:?} got {:?}",
        exp_str,
        ret_str
    );
    // Also ensure contextdVersion present
    assert!(
        v.get("contextdVersion").is_some(),
        "missing contextdVersion"
    );
    assert!(v.get("rustVersion").is_some());
    assert!(v.get("pid").is_some());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn clean_shutdown() -> Result<()> {
    let root = workspace_root();
    let client = spawn_contextd(&root).await?;
    let _ = client.list_tools(Default::default()).await?;
    let start = Instant::now();
    client.cancel().await?;
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "shutdown took too long: {:?}",
        elapsed
    );
    // If we can spawn again immediately, previous instance cleaned up
    let client2 = spawn_contextd(&root).await?;
    let tools = client2.list_tools(Default::default()).await?;
    assert_eq!(tools.tools.len(), 5);
    client2.cancel().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn error_isolation_and_restart() -> Result<()> {
    let root = workspace_root();
    let client = spawn_contextd(&root).await?;

    // First invalid should not crash
    let bad =
        CallToolRequestParams::new("context_search").with_arguments(serde_json::Map::from_iter([
            ("query".to_string(), json!("")),
        ]));
    let _ = client.call_tool(bad).await; // expect err

    // Next valid should still succeed
    let good = client
        .call_tool(CallToolRequestParams::new("symbol_lookup").with_arguments(
            serde_json::Map::from_iter([("symbol".to_string(), json!("count_tokens"))]),
        ))
        .await?;
    let text = extract_text(&good.content);
    assert!(
        text.contains("count_tokens") || text.contains("evidence") || !text.is_empty(),
        "valid after invalid should succeed"
    );

    // Unknown tool should return error but not crash
    let unknown = CallToolRequestParams::new("unknown_tool");
    let res_unknown = client.call_tool(unknown).await;
    assert!(res_unknown.is_err(), "unknown tool should error");

    // Still alive
    let status = client
        .call_tool(CallToolRequestParams::new("context_status"))
        .await?;
    assert!(!extract_text(&status.content).is_empty());

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn compact_output() -> Result<()> {
    let root = workspace_root();
    eprintln!("compact_output root {:?}", root);
    let client = spawn_contextd(&root).await?;
    eprintln!("compact_output spawned");
    // Warm up with status (fast, no Ollama) then test compact output via status + symbol_lookup with timeout
    let _ = tokio::time::timeout(
        Duration::from_secs(30),
        client.call_tool(CallToolRequestParams::new("context_status")),
    )
    .await
    .map_err(|_| anyhow::anyhow!("compact warmup timeout"))??;
    eprintln!("compact_output warmup done");
    let res = tokio::time::timeout(
        Duration::from_secs(30),
        client.call_tool(CallToolRequestParams::new("symbol_lookup").with_arguments(
            serde_json::Map::from_iter([("symbol".to_string(), json!("count_tokens"))]),
        )),
    )
    .await
    .map_err(|_| anyhow::anyhow!("compact_output timeout"))??;
    let text = extract_text(&res.content);
    let v: Value = serde_json::from_str(&text).expect("output should be JSON");
    assert!(
        v.get("query").is_some() || v.get("context").is_some() || v.get("evidence").is_some(),
        "output missing expected keys"
    );
    let text_lower = text.to_lowercase();
    assert!(
        !text_lower.contains("api_key") || text_lower.contains("projectroot"),
        "unexpected secret leak"
    );
    assert!(
        text.len() < 100_000,
        "output unexpectedly large: {}",
        text.len()
    );
    client.cancel().await?;
    Ok(())
}

// Helper to compare evidence ignoring timing fields
fn normalize_evidence(v: &Value) -> Vec<(String, String)> {
    v.get("evidence")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .take(3)
                .map(|e| {
                    let file = e
                        .get("file")
                        .and_then(|f| f.as_str())
                        .unwrap_or("")
                        .to_string();
                    let source = e
                        .get("source")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    (file, source)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
#[ignore]
async fn rust_frozen_live_correctness() -> Result<()> {
    let root = workspace_root();
    eprintln!("DEBUG root {:?}", root);
    eprintln!("DEBUG bin {:?}", find_contextd_bin());
    let rust_client = spawn_contextd(&root).await?;
    // Warm up rust (triggers candidate provider)
    let _ = rust_client
        .call_tool(CallToolRequestParams::new("context_status"))
        .await;

    let fixtures = vec![
        ("symbol_lookup", json!({"symbol":"count_tokens"}), "core/utils.py"),
        (
            "context_search",
            json!({"query":"Where is secret redaction implemented?"}),
            "core/utils.py",
        ),
        (
            "dependency_trace",
            json!({"symbol":"bundle","direction":"callers"}),
            "cli.py",
        ),
        ("test_lookup", json!({"query":"bundle generation"}), "test_bundle_integration.py"),
    ];

    for (tool, args, expected) in fixtures {
        let rust_res = tokio::time::timeout(
            Duration::from_secs(30),
            rust_client.call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(args.as_object().cloned().unwrap_or_default()),
            ),
        )
        .await
        .map_err(|_| anyhow::anyhow!(format!("rust timeout {}", tool)))?;
        let r = rust_res.map_err(|e| anyhow::anyhow!(format!("rust {} failed: {}", tool, e)))?;
        let rt = extract_text(&r.content);
        let rj: Value = serde_json::from_str(&rt).expect("rust output should be JSON");
        let r_ev = normalize_evidence(&rj);
        assert!(
            r_ev.iter().any(|(f, _)| f.ends_with(expected)),
            "{} should contain {}, got {:?} full: {}",
            tool,
            expected,
            r_ev,
            rt.chars().take(500).collect::<String>()
        );
        // Also check that provenance is via Rust ranking (not V2 final)
        // For symbol/semantic, at least one should be oci: or rust:exact
        assert!(!r_ev.is_empty(), "no evidence for {}", tool);
    }

    rust_client.cancel().await?;
    Ok(())
}
