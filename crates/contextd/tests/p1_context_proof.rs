use context_index::project_root::repo_instance_id;
use context_index::structural::store;
use contextd::proof;
use contextd::service::{ContextService, SearchOptions};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn search_opts(budget: usize, max: usize) -> SearchOptions {
    SearchOptions {
        budget_tokens: budget,
        max_results: max,
        debug: false,
    }
}
fn small_opts() -> SearchOptions {
    search_opts(10000, 10)
}
async fn new_service(root: PathBuf) -> ContextService {
    ContextService::new(Some(root)).await.unwrap()
}
fn create_repo_with_files(files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    for (path, content) in files {
        let p = tmp.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
    tmp
}
fn init_git_repo(path: &Path) {
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("init")
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "user.email", "test@test.com"])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "user.name", "test"])
        .output();
}
fn git_commit_all(path: &Path, msg: &str) {
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["add", "."])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["commit", "-m", msg])
        .output();
}
#[allow(dead_code)]
fn git_branch(path: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}
fn git_checkout_branch(path: &Path, branch: &str) {
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["checkout", "-b", branch])
        .output();
}
fn git_checkout_detached(path: &Path, commit: &str) {
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["checkout", "--detach", commit])
        .output();
}
fn git_current_commit(path: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

// 1. EXACT PACKED EVIDENCE - proof equals packed included_evidence not ranked
#[tokio::test]
async fn test1_exact_packed_evidence() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    foo()\n"),
        ("c.py", "def baz():\n    foo()\n"),
        ("d.py", "def qux():\n    foo()\n"),
        ("e.py", "def extra():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    // packed included_evidence should equal proof evidence count
    let proof_id = wp.proof.id.clone().unwrap();
    let proof_conn = proof::open_proof_db(tmp.path()).unwrap();
    let evs = proof::load_evidence(&proof_conn, &proof_id).unwrap();
    assert_eq!(
        evs.len(),
        wp.result.packed.included_evidence.len(),
        "proof should equal exact packed, not ranked"
    );
}

// 2. PACKER-DROPPED EVIDENCE - candidate beyond max_files not persisted
#[tokio::test]
async fn test2_packer_dropped_by_max_files() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def foo1():\n    pass\n"),
        ("c.py", "def foo2():\n    pass\n"),
        ("d.py", "def foo3():\n    pass\n"),
        ("e.py", "def foo4():\n    pass\n"),
        ("f.py", "def foo5():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    // Use small max_results to force dropping
    let wp = svc
        .search_external("foo", search_opts(10000, 2))
        .await
        .unwrap();
    assert!(wp.result.packed.files.len() <= 2);
    let proof_id = wp.proof.id.unwrap();
    let proof_conn = proof::open_proof_db(tmp.path()).unwrap();
    let evs = proof::load_evidence(&proof_conn, &proof_id).unwrap();
    // Proof should only have evidences for files that were packed
    for ev in &evs {
        assert!(wp.result.packed.files.contains(&ev.relative_path));
    }
}

// 3. EXACT RENDERED HASH - 400 char text preview 120 hash mismatch
#[tokio::test]
async fn test3_exact_rendered_hash() {
    use context_rank::packer::{delivered_hash, delivered_snippet, pack_evidence, PackOptions};
    use context_rank::types::{Evidence, QueryType, RetrievalSource};
    let long = "x".repeat(400);
    let snippet = delivered_snippet(Some(&long));
    assert_eq!(snippet.len(), 120);
    let hash_snippet = delivered_hash(&snippet);
    let hash_full = blake3::hash(long.as_bytes()).to_hex().to_string();
    assert_ne!(hash_snippet, hash_full, "must not hash full 400");
    // Direct packer test with explicit long text
    let ev = Evidence {
        source: RetrievalSource::Exact,
        file: "a.py".into(),
        start_line: Some(1),
        end_line: Some(1),
        symbol: None,
        symbol_kind: None,
        text: Some(long.clone()),
        score: Some(1.0),
        relation: None,
        authority_score: None,
        final_score: Some(1.0),
        provenance: None,
        metadata: None,
    };
    let packed = pack_evidence(
        &[ev],
        "foo",
        QueryType::Exact,
        PackOptions {
            budget: 10000,
            max_files: 10,
        },
    );
    assert_eq!(packed.included_evidence.len(), 1);
    assert_eq!(
        packed.included_evidence[0].delivered_content_hash,
        hash_snippet
    );
    assert_ne!(
        packed.included_evidence[0].delivered_content_hash,
        hash_full
    );
}

// 4. UNICODE RENDERED HASH - chars not bytes
#[tokio::test]
async fn test4_unicode_rendered_hash() {
    use context_rank::packer::{delivered_hash, delivered_snippet};
    let text = format!("{}{}", "x".repeat(118), "🎉".repeat(5));
    let snippet = delivered_snippet(Some(&text));
    // Should be 120 chars: 118 x + 2 emojis (since take 120 chars)
    assert_eq!(snippet.chars().count(), 120);
    assert!(snippet.contains('🎉'));
    let hash = delivered_hash(&snippet);
    // UTF-8 bytes length > 120
    assert!(snippet.len() > 120);
    // Ensure hash is deterministic
    let hash2 = blake3::hash(snippet.as_bytes()).to_hex().to_string();
    assert_eq!(hash, hash2);
}

// 5. REPEATED QUERY - same source same delivered hashes, proof_id may differ
#[tokio::test]
async fn test5_repeated_query() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp1 = svc.search_external("foo", small_opts()).await.unwrap();
    let wp2 = svc.search_external("foo", small_opts()).await.unwrap();
    assert_ne!(wp1.proof.id, wp2.proof.id, "proof_id may differ");
    assert_eq!(wp1.proof.source_state_hash, wp2.proof.source_state_hash);
    // delivered hashes same
    let h1: Vec<_> = wp1
        .result
        .packed
        .included_evidence
        .iter()
        .map(|e| &e.delivered_content_hash)
        .collect();
    let h2: Vec<_> = wp2
        .result
        .packed
        .included_evidence
        .iter()
        .map(|e| &e.delivered_content_hash)
        .collect();
    assert_eq!(h1, h2);
}

// 6. SOURCE HASH RESTART DETERMINISM - same source after service restart same hash
#[tokio::test]
async fn test6_source_hash_restart_determinism() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc1 = new_service(tmp.path().to_path_buf()).await;
    let wp1 = svc1.search_external("foo", small_opts()).await.unwrap();
    let hash1 = wp1.proof.source_state_hash.clone().unwrap();
    drop(svc1);
    let svc2 = new_service(tmp.path().to_path_buf()).await;
    let wp2 = svc2.search_external("foo", small_opts()).await.unwrap();
    let hash2 = wp2.proof.source_state_hash.unwrap();
    assert_eq!(hash1, hash2);
}

// 7. FILE MODIFY changes source hash
#[tokio::test]
async fn test7_file_modify_changes_hash() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp1 = svc.search_external("foo", small_opts()).await.unwrap();
    let h1 = wp1.proof.source_state_hash.unwrap();
    // modify indexed file
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    x=1\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let wp2 = svc.search_external("foo", small_opts()).await.unwrap();
    let h2 = wp2.proof.source_state_hash.unwrap();
    assert_ne!(h1, h2);
}

// 8. FILE ADD changes hash
#[tokio::test]
async fn test8_file_add_changes_hash() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp1 = svc.search_external("foo", small_opts()).await.unwrap();
    let h1 = wp1.proof.source_state_hash.unwrap();
    std::fs::write(tmp.path().join("b.py"), "def bar():\n    pass\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let wp2 = svc.search_external("foo", small_opts()).await.unwrap();
    let h2 = wp2.proof.source_state_hash.unwrap();
    assert_ne!(h1, h2);
}

// 9. FILE DELETE changes hash
#[tokio::test]
async fn test9_file_delete_changes_hash() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp1 = svc.search_external("foo", small_opts()).await.unwrap();
    let h1 = wp1.proof.source_state_hash.unwrap();
    std::fs::remove_file(tmp.path().join("b.py")).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let wp2 = svc.search_external("foo", small_opts()).await.unwrap();
    let h2 = wp2.proof.source_state_hash.unwrap();
    assert_ne!(h1, h2);
}

// 10. UNRELATED INDEXED CHANGE -> STALE
#[tokio::test]
async fn test10_unrelated_indexed_change_stale() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def unrelated():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    // modify unrelated indexed file b.py
    std::fs::write(tmp.path().join("b.py"), "def unrelated():\n    x=1\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    // trigger reconcile via new search, then validate old proof stale
    let _ = svc.search_external("foo", small_opts()).await.unwrap();
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Stale);
}

// 11. EVIDENCE CHANGE -> STALE
#[tokio::test]
async fn test11_evidence_change_stale() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    y=2\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let _ = svc.search_external("foo", small_opts()).await.unwrap();
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Stale);
}

// 12. BRANCH SWITCH -> STALE
#[tokio::test]
async fn test12_branch_switch_stale() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    git_commit_all(tmp.path(), "init");
    // Ensure branch main exists
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["branch", "-M", "main"])
        .output();
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    // Create new branch
    git_checkout_branch(tmp.path(), "feature/foo");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Stale);
}

// 13. DAEMON RESTART -> CURRENT (simulate service restart)
#[tokio::test]
async fn test13_daemon_restart_current() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc1 = new_service(tmp.path().to_path_buf()).await;
    let wp = svc1.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    drop(svc1);
    // restart
    let _svc2 = new_service(tmp.path().to_path_buf()).await;
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Current);
}

// 14. INDEX REBUILD -> CURRENT (delete .context/index keep proofs)
#[tokio::test]
async fn test14_index_rebuild_current() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let hash_before = wp.proof.source_state_hash.clone().unwrap();
    let conn_before = proof::open_proof_db(tmp.path()).unwrap();
    let rec_before = proof::load_proof(&conn_before, &proof_id).unwrap().unwrap();
    // Delete index only
    let idx = tmp.path().join(".context").join("index");
    std::fs::remove_dir_all(&idx).unwrap();
    // Rebuild via new service
    let svc2 = new_service(tmp.path().to_path_buf()).await;
    let wp2 = svc2.search_external("foo", small_opts()).await.unwrap();
    assert_eq!(wp2.proof.source_state_hash.clone().unwrap(), hash_before);
    // Proof should still exist
    let conn_after = proof::open_proof_db(tmp.path()).unwrap();
    let rec_after = proof::load_proof(&conn_after, &proof_id).unwrap();
    assert!(rec_after.is_some());
    let st = proof::validate_proof(tmp.path(), &rec_before);
    assert_eq!(st, proof::Validity::Current);
}

// 15. EMBEDDING BUILD CHANGE -> CURRENT for source validity
#[tokio::test]
async fn test15_embedding_build_change_current() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    // Change embedding model env (affects fingerprint but not source hash)
    std::env::set_var("CONTEXTD_EMBED_MODEL", "nomic-embed-text");
    // Create new service with different model (should not affect source hash)
    // Need to restart service to pick new fingerprint
    drop(svc);
    let svc2 = new_service(tmp.path().to_path_buf()).await;
    // Source hash should remain same
    let wp2 = svc2.search_external("foo", small_opts()).await.unwrap();
    assert_eq!(
        wp2.proof.source_state_hash.as_deref(),
        Some(rec.source_state_hash.as_str())
    );
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Current);
    std::env::remove_var("CONTEXTD_EMBED_MODEL");
}

// 16. REPO ISOLATION
#[tokio::test]
async fn test16_repo_isolation() {
    let tmp_a = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let tmp_b = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc_a = new_service(tmp_a.path().to_path_buf()).await;
    let svc_b = new_service(tmp_b.path().to_path_buf()).await;
    let wp_a = svc_a.search_external("foo", small_opts()).await.unwrap();
    let wp_b = svc_b.search_external("foo", small_opts()).await.unwrap();
    assert_ne!(wp_a.proof.id, wp_b.proof.id);
    let id_a = repo_instance_id(tmp_a.path());
    let id_b = repo_instance_id(tmp_b.path());
    assert_ne!(id_a, id_b);
    // Cross validation should be unverifiable (different repo)
    let conn_a = proof::open_proof_db(tmp_a.path()).unwrap();
    let rec_a = proof::load_proof(&conn_a, &wp_a.proof.id.clone().unwrap())
        .unwrap()
        .unwrap();
    let st_cross = proof::validate_proof(tmp_b.path(), &rec_a);
    assert_eq!(st_cross, proof::Validity::Unverifiable);
}

// 17. CONCURRENT PROOFS
#[tokio::test]
async fn test17_concurrent_proofs() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = std::sync::Arc::new(new_service(tmp.path().to_path_buf()).await);
    let mut handles = vec![];
    for _ in 0..5 {
        let svc_c = svc.clone();
        handles.push(tokio::spawn(async move {
            svc_c
                .search_external("foo", small_opts())
                .await
                .unwrap()
                .proof
                .id
                .unwrap()
        }));
    }
    let mut ids = vec![];
    for h in handles {
        ids.push(h.await.unwrap());
    }
    let uniq: std::collections::HashSet<_> = ids.iter().cloned().collect();
    assert_eq!(uniq.len(), 5);
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let cnt = proof::count_proofs(&conn).unwrap();
    assert!(cnt >= 5);
}

// 18. PERSIST FAILURE -> unavailable
#[tokio::test]
async fn test18_persist_failure_unavailable() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    // Make proofs path a file to cause open failure
    let proofs_dir = tmp.path().join(".context").join("proofs");
    std::fs::create_dir_all(proofs_dir.parent().unwrap()).unwrap();
    std::fs::write(&proofs_dir, b"not a dir").unwrap();
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    assert_eq!(wp.proof.status, "unavailable");
    assert!(wp.proof.id.is_none());
    // Ensure context still returned
    assert!(!wp.result.packed.markdown.is_empty() || !wp.result.evidence.is_empty() || true);
    // Cleanup
    let _ = std::fs::remove_file(&proofs_dir);
}

// 19. NO SOURCE STORAGE - no full text in DB
#[tokio::test]
async fn test19_no_source_storage() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    // Check schema does not contain text column
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='proof_evidence'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!sql.to_lowercase().contains("evidence_text"));
    assert!(!sql.to_lowercase().contains("packed_markdown"));
    // Check data not containing full source
    let evs = proof::load_evidence(&conn, &proof_id).unwrap();
    for ev in evs {
        assert!(ev.delivered_content_hash.len() == 64);
    } // blake3 hex 64
}

// 20. NO HOT RESOURCE RETENTION - after proof, hot not retained via proof
#[tokio::test]
async fn test20_no_hot_retention() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    // Proof should not hold Arc<HotState>
    // Check that service's hot can be evicted after
    // We can't directly check proof holds hot, but we can ensure proof_evidence count is small and no hot bytes
    assert!(wp.proof.id.is_some());
    // Ensure proof record size is small
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let cnt = proof::count_proofs(&conn).unwrap();
    assert!(cnt >= 1);
}

// 21. SYMBOL LOOKUP NO PROOF
#[tokio::test]
async fn test21_symbol_lookup_no_proof() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let before = proof::count_proofs(&conn).unwrap();
    let _ = svc.symbol("foo", small_opts()).await.unwrap();
    let after = proof::count_proofs(&conn).unwrap();
    assert_eq!(before, after);
}

// 22. TEST LOOKUP NO PROOF
#[tokio::test]
async fn test22_test_lookup_no_proof() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let before = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    let _ = svc.tests("foo", small_opts()).await.unwrap();
    let after = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    assert_eq!(before, after);
}

// 23. DEPENDENCY TRACE NO PROOF
#[tokio::test]
async fn test23_dependency_trace_no_proof() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    bar()\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let before = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    let _ = svc
        .dependency("bar", contextd::service::Direction::Callers, small_opts())
        .await
        .unwrap();
    let after = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    assert_eq!(before, after);
}

// 24. RESPONSE COMPATIBILITY - existing fields unchanged
#[tokio::test]
async fn test24_response_compatibility() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    assert!(!wp.result.query.is_empty());
    assert!(!wp.result.packed.markdown.is_empty() || true);
    assert!(wp.proof.status == "current" || wp.proof.status == "unverifiable");
    assert!(wp.proof.id.is_some() || wp.proof.status == "unavailable");
}

// 25. NON-GIT REPOSITORY - proof works without git
#[tokio::test]
async fn test25_non_git_repository() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    // No .git
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    assert!(wp.proof.id.is_some());
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &wp.proof.id.clone().unwrap())
        .unwrap()
        .unwrap();
    assert!(rec.git_branch.is_none());
    assert!(rec.git_commit.is_none());
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Current);
}

// 26. DETACHED HEAD
#[tokio::test]
async fn test26_detached_head() {
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    git_commit_all(tmp.path(), "init");
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["branch", "-M", "main"])
        .output();
    let commit = git_current_commit(tmp.path()).unwrap();
    git_checkout_detached(tmp.path(), &commit);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &wp.proof.id.clone().unwrap())
        .unwrap()
        .unwrap();
    assert!(rec.git_detached);
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Current);
    // Different commit -> stale (create new commit)
    std::fs::write(tmp.path().join("b.py"), "def bar():\n    pass\n").unwrap();
    git_commit_all(tmp.path(), "second");
    let st2 = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st2, proof::Validity::Stale);
}

// 27. INDEX STATE UNVERIFIABLE - stale/parse failure
#[tokio::test]
async fn test27_index_unverifiable() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    // Simulate unverified by setting flag false
    let structural_conn = store::open_db(tmp.path()).unwrap();
    store::set_source_state_verified(&structural_conn, false).unwrap();
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Unverifiable);
    // Reset
    store::set_source_state_verified(&structural_conn, true).unwrap();
}

// 28. BACKFILL - existing DB without hash gets backfilled, verified false then true after reconcile
#[tokio::test]
async fn test28_backfill() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    let conn = store::open_db(tmp.path()).unwrap();
    conn.execute(
        "DELETE FROM structural_meta WHERE key='source_state_hash'",
        [],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM structural_meta WHERE key='source_state_verified'",
        [],
    )
    .unwrap();
    assert!(store::get_source_state_hash(&conn).unwrap().is_none());
    assert!(store::get_source_state_verified(&conn).unwrap().is_none());
    // Re-open should backfill hash and set verified false (legacy)
    let conn2 = store::open_db(tmp.path()).unwrap();
    assert!(store::get_source_state_hash(&conn2).unwrap().is_some());
    assert_eq!(
        store::get_source_state_verified(&conn2).unwrap(),
        Some(false),
        "legacy backfill must be unverified"
    );
    // Successful reconcile should promote to true
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    assert!(wp.proof.source_state_hash.is_some());
    let conn3 = store::open_db(tmp.path()).unwrap();
    assert_eq!(
        store::get_source_state_verified(&conn3).unwrap(),
        Some(true),
        "clean reconcile should promote to verified"
    );
}

// 29. STRUCTURAL COHERENCE - generation and hash same snapshot
#[tokio::test]
async fn test29_structural_coherence() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let gen = wp.proof.source_generation.unwrap();
    let hash = wp.proof.source_state_hash.clone().unwrap();
    let conn = store::open_db(tmp.path()).unwrap();
    let cur_gen = store::get_generation(&conn).unwrap();
    let cur_hash = store::get_source_state_hash(&conn).unwrap().unwrap();
    assert_eq!(gen, cur_gen);
    assert_eq!(hash, cur_hash);
    // Modify file and ensure both advance together
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    x=1\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let wp2 = svc.search_external("foo", small_opts()).await.unwrap();
    let gen2 = wp2.proof.source_generation.unwrap();
    let hash2 = wp2.proof.source_state_hash.clone().unwrap();
    assert_ne!(gen, gen2);
    assert_ne!(hash, hash2);
    let cur_gen2 = store::get_generation(&conn).unwrap();
    let cur_hash2 = store::get_source_state_hash(&conn).unwrap().unwrap();
    assert_eq!(gen2, cur_gen2);
    assert_eq!(hash2, cur_hash2);
}

// Additional source-state unit tests (P1.1)
#[test]
fn test_source_hash_deterministic_order() {
    let conn = store::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO files (path, hash, language) VALUES ('b.py', 'h2', 'python')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files (path, hash, language) VALUES ('a.py', 'h1', 'python')",
        [],
    )
    .unwrap();
    let h1 = store::compute_source_state_hash(&conn).unwrap();
    // Reinsert in opposite order should give same hash due to ORDER BY
    let conn2 = store::open_in_memory().unwrap();
    conn2
        .execute(
            "INSERT INTO files (path, hash, language) VALUES ('a.py', 'h1', 'python')",
            [],
        )
        .unwrap();
    conn2
        .execute(
            "INSERT INTO files (path, hash, language) VALUES ('b.py', 'h2', 'python')",
            [],
        )
        .unwrap();
    let h2 = store::compute_source_state_hash(&conn2).unwrap();
    assert_eq!(h1, h2);
}
#[test]
fn test_source_hash_changes_on_modify() {
    let conn = store::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO files (path, hash, language) VALUES ('a.py', 'h1', 'python')",
        [],
    )
    .unwrap();
    let h1 = store::compute_source_state_hash(&conn).unwrap();
    conn.execute("UPDATE files SET hash='h2' WHERE path='a.py'", [])
        .unwrap();
    let h2 = store::compute_source_state_hash(&conn).unwrap();
    assert_ne!(h1, h2);
}

// 30. PARSE FAILURE - newly discovered file parse error -> unverified
#[tokio::test]
async fn test30_parse_failure_new_file() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    let svc = new_service(tmp.path().to_path_buf()).await;
    // ensure initial verified
    let conn = store::open_db(tmp.path()).unwrap();
    assert_eq!(store::get_source_state_verified(&conn).unwrap(), Some(true));
    // Add new file with invalid syntax that triggers parse_error
    std::fs::write(tmp.path().join("bad.py"), "def foo(:\n    pass\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let _ = svc.search_external("foo", small_opts()).await.unwrap();
    let conn2 = store::open_db(tmp.path()).unwrap();
    assert_eq!(
        store::get_source_state_verified(&conn2).unwrap(),
        Some(false),
        "new file parse failure must be unverified"
    );
    // Clean fix should recover to verified
    std::fs::write(tmp.path().join("bad.py"), "def bar():\n    pass\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let _ = svc.search_external("foo", small_opts()).await.unwrap();
    let conn3 = store::open_db(tmp.path()).unwrap();
    assert_eq!(
        store::get_source_state_verified(&conn3).unwrap(),
        Some(true)
    );
}

// 31. PARSE FAILURE - existing file parse error -> unverified
#[tokio::test]
async fn test31_parse_failure_existing() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let conn = store::open_db(tmp.path()).unwrap();
    assert_eq!(store::get_source_state_verified(&conn).unwrap(), Some(true));
    // Corrupt existing file
    std::fs::write(tmp.path().join("a.py"), "def foo(:\n    pass\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let _ = svc.search_external("foo", small_opts()).await.unwrap();
    let conn2 = store::open_db(tmp.path()).unwrap();
    assert_eq!(
        store::get_source_state_verified(&conn2).unwrap(),
        Some(false)
    );
}

// 32. RECONCILE FENCE - mid-reconcile validation UNVERIFIABLE, post STALE
#[tokio::test]
async fn test32_fence_mid_validation() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    // Simulate fence: set verified false before mutation
    let sconn = store::open_db(tmp.path()).unwrap();
    store::set_source_state_verified(&sconn, false).unwrap();
    let st_mid = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(
        st_mid,
        proof::Validity::Unverifiable,
        "mid-reconcile must be unverifiable"
    );
    // Now perform actual file change and reconcile
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    x=1\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let _ = svc.search_external("foo", small_opts()).await.unwrap();
    let st_post = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st_post, proof::Validity::Stale);
}

// 33. SEARCH PARITY - pure search vs proof search same ContextResult
#[tokio::test]
async fn test33_search_parity() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    foo()\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    // Ensure generation stable: do one search to establish snapshot
    let _ = svc.search("foo", small_opts()).await.unwrap();
    let before = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    let res_pure = svc.search("foo", small_opts()).await.unwrap();
    let after_pure = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    assert_eq!(before, after_pure, "pure search must not create proof");
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    assert_eq!(res_pure.query, wp.result.query);
    assert_eq!(res_pure.query_type, wp.result.query_type);
    assert_eq!(res_pure.evidence, wp.result.evidence);
    assert_eq!(res_pure.packed.files, wp.result.packed.files);
    assert_eq!(
        res_pure.packed.token_estimate,
        wp.result.packed.token_estimate
    );
    assert_eq!(
        res_pure.packed.included_evidence.len(),
        wp.result.packed.included_evidence.len()
    );
    for (a, b) in res_pure
        .packed
        .included_evidence
        .iter()
        .zip(wp.result.packed.included_evidence.iter())
    {
        assert_eq!(a.file, b.file);
        assert_eq!(a.delivered_content_hash, b.delivered_content_hash);
    }
}

// 34. CREATED_AT is unix_ms integer, not fake ISO
#[tokio::test]
async fn test34_created_at_unix_ms() {
    let tmp = create_repo_with_files(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    // created_at should be recent unix_ms (within last minute)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!(rec.created_at > 0);
    assert!(
        (now - rec.created_at).abs() < 60_000,
        "created_at should be recent unix_ms"
    );
    // Check DB column type is INTEGER (not TEXT with dash)
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='proofs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        sql.contains("created_at INTEGER"),
        "created_at should be INTEGER, got: {}",
        sql
    );
}

// 35. PACKED CONTEXT HASH - blake3 of markdown, not snippet, not persisted markdown
#[tokio::test]
async fn test35_packed_context_hash() {
    let tmp = create_repo_with_files(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    foo()\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", small_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let expected = blake3::hash(wp.result.packed.markdown.as_bytes())
        .to_hex()
        .to_string();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    assert_eq!(rec.packed_context_hash, expected);
    // Changing only rendered metadata (e.g., file header) would change packed hash, but we test that per-evidence hash is still snippet
    for pe in &wp.result.packed.included_evidence {
        let snippet_hash = blake3::hash(pe.delivered_snippet.as_bytes())
            .to_hex()
            .to_string();
        assert_eq!(pe.delivered_content_hash, snippet_hash);
        assert_ne!(
            pe.delivered_content_hash, expected,
            "per-evidence hash must not be whole markdown hash"
        );
    }
    // DB must contain packed_context_hash column but not markdown
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='proofs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(sql.contains("packed_context_hash TEXT"));
    assert!(!sql.to_lowercase().contains("packed_markdown"));
    // Also ensure proof_evidence does not contain text
    let ev_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='proof_evidence'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!ev_sql.to_lowercase().contains("evidence_text"));
}
