#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

struct TestDaemon {
    child: tokio::process::Child,
    dir: TempDir,
    port: u16,
}

impl TestDaemon {
    async fn start(dir: TempDir) -> Self {
        let mut exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        exe.push("../../target/debug/contextd.exe");
        assert!(exe.exists(), "contextd binary not found: {exe:?}");
        let mut child = tokio::process::Command::new(&exe)
            .arg("daemon")
            .env("CONTEXTD_GLOBAL_DAEMON_DIR", dir.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("spawn daemon");
        let deadline = Instant::now() + Duration::from_secs(5);
        let metadata_path = dir.path().join("daemon.json");
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                panic!("daemon exited before TCP readiness: {status}");
            }
            if let Ok(text) = tokio::fs::read_to_string(&metadata_path).await {
                if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(port) = meta.get("port").and_then(|v| v.as_u64()) {
                        if tokio::time::timeout(
                            Duration::from_millis(200),
                            tokio::net::TcpStream::connect(("127.0.0.1", port as u16)),
                        )
                        .await
                        .is_ok_and(|r| r.is_ok())
                        {
                            let pid = meta.get("pid").and_then(|v| v.as_u64()).unwrap();
                            assert_eq!(pid, child.id().unwrap() as u64);
                            return Self {
                                child,
                                dir,
                                port: port as u16,
                            };
                        }
                    }
                }
            }
            assert!(Instant::now() < deadline, "daemon startup timeout");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn call(&self, tool: &str, params: serde_json::Value, root: &Path) -> serde_json::Value {
        let mut stream = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect(("127.0.0.1", self.port)),
        )
        .await
        .expect("RPC connect timeout")
        .expect("RPC connect failed");
        let request = serde_json::json!({
            "id": 1,
            "tool": tool,
            "params": params,
            "root": root.display().to_string()
        });
        stream
            .write_all(serde_json::to_string(&request).unwrap().as_bytes())
            .await
            .unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();
        let mut reader = tokio::io::BufReader::new(stream);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .expect("RPC read timeout")
            .unwrap();
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(response.get("error").is_none(), "RPC error: {response}");
        response.get("result").cloned().unwrap()
    }

    async fn stop(mut self) -> TempDir {
        let pid = self.child.id().unwrap_or(0);
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .expect("daemon shutdown timeout")
            .unwrap();
        contextd::daemon::cleanup_global_daemon_artifacts_if_owned(pid).await;
        let _ = tokio::fs::remove_file(self.dir.path().join("daemon.json")).await;
        let _ = tokio::fs::remove_file(self.dir.path().join("daemon.lock")).await;
        self.dir
    }
}

#[tokio::test]
async fn p2_real_daemon_lifecycle_and_delta_flow() {
    let global = TempDir::new().unwrap();
    let previous = std::env::var_os("CONTEXTD_GLOBAL_DAEMON_DIR");
    std::env::set_var("CONTEXTD_GLOBAL_DAEMON_DIR", global.path());

    // A live owner must not have its lock removed by cleanup for another PID.
    std::fs::write(
        global.path().join("daemon.lock"),
        std::process::id().to_string(),
    )
    .unwrap();
    contextd::daemon::cleanup_global_daemon_artifacts_if_owned(std::process::id() + 1).await;
    assert!(global.path().join("daemon.lock").exists());

    // Dead orphan lock, malformed metadata, and stale metadata are all recoverable.
    std::fs::write(global.path().join("daemon.lock"), "4294967294").unwrap();
    std::fs::write(
        global.path().join("daemon.json"),
        r#"{"pid":4294967294,"port":1,"root":"orphan","started_at":0}"#,
    )
    .unwrap();
    let daemon = TestDaemon::start(global).await;
    assert_eq!(
        tokio::fs::read_to_string(daemon.dir.path().join("daemon.lock"))
            .await
            .unwrap()
            .trim(),
        daemon.child.id().unwrap().to_string()
    );

    let repo = TempDir::new().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    std::fs::write(repo.path().join("a.py"), "def foo():\n    return 1\n").unwrap();
    std::fs::write(repo.path().join("b.py"), "def bar():\n    return 2\n").unwrap();
    let search = daemon
        .call(
            "context_search",
            serde_json::json!({"query":"foo","budgetTokens":10000,"maxResults":10}),
            repo.path(),
        )
        .await;
    let proof_id = search["proof"]["id"].as_str().unwrap().to_string();

    std::fs::write(repo.path().join("b.py"), "def bar():\n    return 999\n").unwrap();
    let _ = daemon
        .call(
            "context_search",
            serde_json::json!({"query":"bar"}),
            repo.path(),
        )
        .await;
    let unrelated = daemon
        .call(
            "context_delta",
            serde_json::json!({"proof_id":proof_id}),
            repo.path(),
        )
        .await;
    assert!(unrelated["status"] == "Ready" || unrelated["status"] == "ready");
    assert!(unrelated["evidence"].as_array().unwrap().iter().any(|e| {
        e["old_path"].as_str() == Some("a.py") && e["classification"].as_str() == Some("Unchanged")
    }));

    std::fs::write(repo.path().join("a.py"), "def foo():\n    return 999\n").unwrap();
    let _ = daemon
        .call(
            "context_search",
            serde_json::json!({"query":"foo"}),
            repo.path(),
        )
        .await;
    let changed = daemon
        .call(
            "context_delta",
            serde_json::json!({"proof_id":proof_id}),
            repo.path(),
        )
        .await;
    assert!(changed["evidence"].as_array().unwrap().iter().any(|e| {
        e["old_path"].as_str() == Some("a.py") && e["classification"].as_str() == Some("Changed")
    }));

    std::fs::remove_file(repo.path().join("a.py")).unwrap();
    let _ = daemon
        .call(
            "context_search",
            serde_json::json!({"query":"bar"}),
            repo.path(),
        )
        .await;
    let removed = daemon
        .call(
            "context_delta",
            serde_json::json!({"proof_id":proof_id}),
            repo.path(),
        )
        .await;
    assert!(removed["evidence"].as_array().unwrap().iter().any(|e| {
        e["old_path"].as_str() == Some("a.py") && e["classification"].as_str() == Some("Removed")
    }));

    let global = daemon.stop().await;
    let daemon = TestDaemon::start(global).await;
    let after_restart = daemon
        .call(
            "context_delta",
            serde_json::json!({"proof_id": proof_id}),
            repo.path(),
        )
        .await;
    assert!(after_restart["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| {
            e["old_path"].as_str() == Some("a.py")
                && e["classification"].as_str() == Some("Removed")
        }));
    let global = daemon.stop().await;
    // A stale but valid metadata file paired with the same dead lock is recoverable.
    std::fs::write(global.path().join("daemon.lock"), "4294967294").unwrap();
    std::fs::write(
        global.path().join("daemon.json"),
        r#"{"pid":4294967294,"port":1,"root":"orphan","started_at":0}"#,
    )
    .unwrap();
    let daemon = TestDaemon::start(global).await;
    daemon.stop().await;
    if let Some(value) = previous {
        std::env::set_var("CONTEXTD_GLOBAL_DAEMON_DIR", value);
    } else {
        std::env::remove_var("CONTEXTD_GLOBAL_DAEMON_DIR");
    }
}
