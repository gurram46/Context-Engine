#![allow(clippy::needless_borrows_for_generic_args)]

use contextd::proof;
use contextd::service::{ContextService, SearchOptions};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn opts() -> SearchOptions {
    SearchOptions {
        budget_tokens: 10_000,
        max_results: 10,
        debug: false,
    }
}

fn repo(files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    for (name, content) in files {
        let path = tmp.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["init", "-q"])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["config", "user.email", "p3@example.test"])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["config", "user.name", "p3"])
        .output();
    tmp
}

async fn service(root: &TempDir) -> ContextService {
    ContextService::new(Some(root.path().to_path_buf()))
        .await
        .unwrap()
}

fn proof_count(root: &TempDir) -> i64 {
    proof::count_proofs(&proof::open_proof_db(root.path()).unwrap()).unwrap()
}

#[tokio::test]
async fn ordinary_search_returns_handoff_reference() {
    let root = repo(&[("retry.rs", "fn retry_policy() { /* retry policy */ }\n")]);
    let svc = service(&root).await;
    let result = svc.search_external("retry policy", opts()).await.unwrap();
    let handoff = result.handoff.unwrap();
    assert_eq!(handoff.version, 1);
    assert_eq!(handoff.proof_id, result.proof.id.clone().unwrap());
    assert_eq!(handoff.query, "retry policy");
    assert_eq!(
        handoff.source_state_hash,
        result.proof.source_state_hash.unwrap()
    );
    assert_eq!(proof_count(&root), 1);
    assert!(result.continuity.is_none());
}

#[tokio::test]
async fn same_snapshot_creates_one_new_proof_and_no_change_continuity() {
    let root = repo(&[("retry.rs", "fn retry_policy() { /* retry policy */ }\n")]);
    let svc = service(&root).await;
    let first = svc.search_external("retry policy", opts()).await.unwrap();
    let first_id = first.proof.id.clone().unwrap();
    let second = svc
        .search_external_with_continuity("retry policy", opts(), Some(&first_id))
        .await
        .unwrap();
    let continuity = second.continuity.unwrap();
    assert_eq!(continuity.from_proof_id, first_id);
    assert_eq!(continuity.to_proof_id, second.proof.id.clone().unwrap());
    assert_eq!(continuity.status, "no_change");
    assert!(!continuity.source_state_changed);
    assert_eq!(
        continuity.to_source_state_hash,
        second.proof.source_state_hash.clone().unwrap()
    );
    assert_eq!(
        continuity.to_generation,
        second.proof.source_generation.unwrap()
    );
    assert_ne!(first.proof.id, second.proof.id);
    assert_eq!(proof_count(&root), 2);
}

#[tokio::test]
async fn unrelated_change_is_updated_but_historical_evidence_is_unchanged() {
    let root = repo(&[
        ("retry.rs", "fn retry_policy() { /* retry policy */ }\n"),
        ("unrelated.rs", "fn unrelated() {}\n"),
    ]);
    let svc = service(&root).await;
    let first = svc.search_external("retry policy", opts()).await.unwrap();
    let first_id = first.proof.id.clone().unwrap();
    std::fs::write(
        root.path().join("unrelated.rs"),
        "fn unrelated() { let x = 1; }\n",
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let second = svc
        .search_external_with_continuity("retry policy", opts(), Some(&first_id))
        .await
        .unwrap();
    let continuity = second.continuity.unwrap();
    assert_eq!(continuity.status, "updated");
    assert!(continuity.source_state_changed);
    assert_eq!(
        continuity.to_source_state_hash,
        second.proof.source_state_hash.clone().unwrap()
    );
    assert!(continuity
        .delta
        .evidence
        .iter()
        .any(|e| e.old_path == "retry.rs" && e.classification.as_str() == "unchanged"));
    assert_eq!(proof_count(&root), 2);
}

#[tokio::test]
async fn changed_evidence_and_chain_produce_new_anchors() {
    let root = repo(&[("retry.rs", "fn retry_policy() { /* retry policy v1 */ }\n")]);
    let svc = service(&root).await;
    let a = svc.search_external("retry policy", opts()).await.unwrap();
    let a_id = a.proof.id.clone().unwrap();
    std::fs::write(
        root.path().join("retry.rs"),
        "fn retry_policy() { /* retry policy v2 */ }\n",
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let b = svc
        .search_external_with_continuity("retry policy", opts(), Some(&a_id))
        .await
        .unwrap();
    let b_id = b.proof.id.clone().unwrap();
    assert_ne!(a_id, b_id);
    assert_eq!(b.continuity.as_ref().unwrap().from_proof_id, a_id);
    assert!(b.continuity.as_ref().unwrap().delta.summary.changed > 0);
    std::fs::write(
        root.path().join("retry.rs"),
        "fn retry_policy() { /* retry policy v3 */ }\n",
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let c = svc
        .search_external_with_continuity("retry policy", opts(), Some(&b_id))
        .await
        .unwrap();
    assert_ne!(b_id, c.proof.id.clone().unwrap());
    assert_eq!(c.continuity.as_ref().unwrap().from_proof_id, b_id);
    assert_eq!(
        c.continuity.as_ref().unwrap().to_proof_id,
        c.proof.id.clone().unwrap()
    );
    assert_eq!(proof_count(&root), 3);
}

#[tokio::test]
async fn failed_handoff_does_not_search_or_create_proof() {
    let root = repo(&[("retry.rs", "fn retry_policy() { /* retry policy */ }\n")]);
    let svc = service(&root).await;
    let first = svc.search_external("retry policy", opts()).await.unwrap();
    let id = first.proof.id.clone().unwrap();
    let before = proof_count(&root);
    let mismatch = svc
        .search_external_with_continuity("authentication", opts(), Some(&id))
        .await
        .unwrap_err();
    assert!(mismatch.to_string().contains("HANDOFF_QUERY_MISMATCH"));
    let unknown = svc
        .search_external_with_continuity("retry policy", opts(), Some("cp_does_not_exist"))
        .await
        .unwrap_err();
    assert!(unknown.to_string().contains("HANDOFF_PROOF_NOT_FOUND"));
    assert_eq!(proof_count(&root), before);
}

#[tokio::test]
async fn removed_historical_evidence_is_reported() {
    let root = repo(&[("retry.rs", "fn retry_policy() { /* retry policy */ }\n")]);
    let svc = service(&root).await;
    let first = svc.search_external("retry policy", opts()).await.unwrap();
    let id = first.proof.id.clone().unwrap();
    std::fs::remove_file(root.path().join("retry.rs")).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let second = svc
        .search_external_with_continuity("retry policy", opts(), Some(&id))
        .await
        .unwrap();
    assert!(second.continuity.unwrap().delta.summary.removed > 0);
}

struct DaemonProcess {
    child: tokio::process::Child,
    global: TempDir,
    client: contextd::daemon::RemoteClient,
}

async fn start_daemon(global: TempDir) -> DaemonProcess {
    let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/contextd.exe");
    assert!(exe.exists(), "contextd binary not found: {exe:?}");
    let child = tokio::process::Command::new(exe)
        .arg("daemon")
        .env("CONTEXTD_GLOBAL_DAEMON_DIR", global.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let metadata = global.path().join("daemon.json");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = tokio::fs::read_to_string(&metadata).await {
            if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) {
                if let (Some(port), Some(pid)) = (meta["port"].as_u64(), meta["pid"].as_u64()) {
                    if tokio::time::timeout(
                        Duration::from_millis(200),
                        tokio::net::TcpStream::connect(("127.0.0.1", port as u16)),
                    )
                    .await
                    .is_ok_and(|r| r.is_ok())
                    {
                        assert_eq!(pid, child.id().unwrap() as u64);
                        return DaemonProcess {
                            child,
                            global,
                            client: contextd::daemon::RemoteClient {
                                addr: format!("127.0.0.1:{port}"),
                                root: PathBuf::new(),
                                pid: pid as u32,
                            },
                        };
                    }
                }
            }
        }
        assert!(Instant::now() < deadline, "daemon startup timeout");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn stop_daemon(mut daemon: DaemonProcess) -> TempDir {
    let pid = daemon.child.id().unwrap();
    let _ = daemon.child.kill().await;
    let _ = tokio::time::timeout(Duration::from_secs(3), daemon.child.wait()).await;
    contextd::daemon::cleanup_global_daemon_artifacts_if_owned(pid).await;
    daemon.global
}

#[tokio::test]
async fn independent_daemon_clients_and_restart_continue_from_proof() {
    let root = repo(&[
        ("retry.rs", "fn retry_policy() { /* retry policy */ }\n"),
        ("unrelated.rs", "fn unrelated() {}\n"),
    ]);
    let global = TempDir::new().unwrap();
    let mut daemon = start_daemon(global).await;
    daemon.client.root = root.path().to_path_buf();
    let a = daemon
        .client
        .call(
            "context_search",
            serde_json::json!({"query":"retry policy"}),
        )
        .await
        .unwrap();
    let proof_a = a["proof"]["id"].as_str().unwrap().to_string();
    drop(daemon.client.clone());
    std::fs::write(
        root.path().join("unrelated.rs"),
        "fn unrelated() { let x = 1; }\n",
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let b = daemon
        .client
        .call(
            "context_search",
            serde_json::json!({"query":"retry policy", "fromProofId":proof_a}),
        )
        .await
        .unwrap();
    let proof_b = b["proof"]["id"].as_str().unwrap().to_string();
    assert_ne!(proof_a, proof_b);
    assert_eq!(
        b["continuity"]["fromProofId"].as_str(),
        Some(proof_a.as_str())
    );
    assert_eq!(
        b["continuity"]["toProofId"].as_str(),
        Some(proof_b.as_str())
    );
    assert_eq!(
        b["continuity"]["toSourceStateHash"],
        b["proof"]["sourceStateHash"]
    );
    let global = stop_daemon(daemon).await;
    let mut restarted = start_daemon(global).await;
    restarted.client.root = root.path().to_path_buf();
    let c = restarted
        .client
        .call(
            "context_search",
            serde_json::json!({"query":"retry policy", "fromProofId":proof_b}),
        )
        .await
        .unwrap();
    assert_ne!(c["proof"]["id"], serde_json::Value::String(proof_b.clone()));
    assert_eq!(
        c["continuity"]["fromProofId"].as_str(),
        Some(proof_b.as_str())
    );
    let _ = stop_daemon(restarted).await;
}
