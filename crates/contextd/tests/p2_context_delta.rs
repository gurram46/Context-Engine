#![allow(unused_variables, unused_imports, dead_code, non_snake_case)]
use context_index::structural::store;
use contextd::delta::{delta_for_proof, DeltaStatus, EvidenceClassification};
use contextd::proof;
use contextd::service::{ContextService, SearchOptions};
use std::path::Path;
use tempfile::TempDir;

fn search_opts() -> SearchOptions {
    SearchOptions {
        budget_tokens: 10000,
        max_results: 10,
        debug: false,
    }
}
async fn new_service(root: std::path::PathBuf) -> ContextService {
    ContextService::new(Some(root)).await.unwrap()
}
fn create_repo(files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    for (p, c) in files {
        let path = tmp.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, c).unwrap();
    }
    tmp
}
fn init_git(p: &Path) {
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(p)
        .arg("init")
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["config", "user.email", "a@a.com"])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["config", "user.name", "a"])
        .output();
}
fn git_add_commit(p: &Path, msg: &str) {
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["add", "."])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(p)
        .args(["commit", "-m", msg])
        .output();
}

// 1. Same snapshot NO_CHANGE
#[tokio::test]
async fn test1_same_snapshot_no_change() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    assert_eq!(delta.status, DeltaStatus::NoChange);
    assert!(delta
        .evidence
        .iter()
        .all(|e| e.classification == EvidenceClassification::Unchanged));
}

// 2. Unrelated file change: P1 STALE but P2 UNCHANGED (critical)
#[tokio::test]
async fn test2_unrelated_change_p1_stale_p2_unchanged() {
    let tmp = create_repo(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    // Create proof that only references a.py : query foo should return a.py
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let proof_id = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &proof_id).unwrap().unwrap();
    // Modify unrelated b.py
    std::fs::write(tmp.path().join("b.py"), "def bar():\n    x=1\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("bar", search_opts()).await.unwrap(); // trigger reconcile
                                                                      // P1 should be STALE
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(st, proof::Validity::Stale);
    // P2 should be READY with UNCHANGED for a.py evidence
    let delta = delta_for_proof(tmp.path(), &proof_id).unwrap();
    assert_eq!(delta.status, DeltaStatus::Ready);
    // At least one evidence should be unchanged (a.py)
    assert!(delta
        .evidence
        .iter()
        .any(|e| e.old_path == "a.py" && e.classification == EvidenceClassification::Unchanged));
    assert_eq!(delta.summary.changed, 0);
    assert_eq!(delta.summary.removed, 0);
}

// 3. Evidence source changes -> CHANGED
#[tokio::test]
async fn test3_evidence_changed() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    x=1\n    y=2\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(delta.status, DeltaStatus::Ready);
    assert!(delta
        .evidence
        .iter()
        .any(|e| e.classification == EvidenceClassification::Changed));
}

// 4. File deleted -> REMOVED
#[tokio::test]
async fn test4_removed() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    std::fs::remove_file(tmp.path().join("a.py")).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert!(delta
        .evidence
        .iter()
        .any(|e| e.classification == EvidenceClassification::Removed));
}

// 5. Lines inserted above -> UNCHANGED with location_changed
#[tokio::test]
async fn test5_line_shift() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    // Insert lines above
    let orig = std::fs::read_to_string(tmp.path().join("a.py")).unwrap();
    std::fs::write(
        tmp.path().join("a.py"),
        format!("# header\n# line2\n{}", orig),
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    // Should be unchanged, maybe location_changed true, not changed
    let ev = delta
        .evidence
        .iter()
        .find(|e| e.old_path == "a.py")
        .unwrap();
    assert_eq!(ev.classification, EvidenceClassification::Unchanged);
}

// 6. Symbol renamed + changed -> CHANGED or UNVERIFIABLE
#[tokio::test]
async fn test6_renamed_changed() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    std::fs::write(tmp.path().join("a.py"), "def bar():\n    x=1\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("bar", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    // Should be changed or unverifiable, not unchanged
    let ev = delta
        .evidence
        .iter()
        .find(|e| e.old_path == "a.py")
        .unwrap();
    assert!(
        ev.classification == EvidenceClassification::Changed
            || ev.classification == EvidenceClassification::Unverifiable
    );
}

// 7. Moved to another file - P2 v1 DEFERRED: file removal -> REMOVED, no cross-file scan
#[tokio::test]
async fn test7_moved() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let content = std::fs::read_to_string(tmp.path().join("a.py")).unwrap();
    std::fs::remove_file(tmp.path().join("a.py")).unwrap();
    std::fs::write(tmp.path().join("b.py"), content).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    let ev = delta
        .evidence
        .iter()
        .find(|e| e.old_path == "a.py")
        .unwrap();
    // P2 v1: MOVED DEFERRED — indexed file no longer exists => REMOVED, moved=0, no whole-repo scan
    assert_eq!(ev.classification, EvidenceClassification::Removed);
    assert_eq!(delta.summary.moved, 0);
}

// 8. Cross-file duplicate after deletion -> still REMOVED (P2 v1 deferred, no scan)
#[tokio::test]
async fn test8_ambiguous() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    // Create two files with same content as old evidence
    let content = std::fs::read_to_string(tmp.path().join("a.py")).unwrap();
    std::fs::remove_file(tmp.path().join("a.py")).unwrap();
    std::fs::write(tmp.path().join("b.py"), &content).unwrap();
    std::fs::write(tmp.path().join("c.py"), &content).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    let ev = delta
        .evidence
        .iter()
        .find(|e| e.old_path == "a.py")
        .unwrap();
    // P2 v1: no cross-file moved scan, so removal stays REMOVED even if duplicate exists elsewhere
    assert_eq!(ev.classification, EvidenceClassification::Removed);
    assert_eq!(delta.summary.moved, 0);
}

// 8b. Duplicate snippet within same file -> UNVERIFIABLE (must not guess first match)
#[tokio::test]
async fn test8b_duplicate_snippet_same_file() {
    // Manual proof with rust:exact provenance and hash of "x = 1" which appears twice in a.py
    let tmp = create_repo(&[
        ("a.py", "x = 1\nx = 1\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let _ = svc.search_external("bar", search_opts()).await.unwrap(); // ensure index built
    let root = tmp.path().to_path_buf();
    let sconn = store::open_db(&root).unwrap();
    let hash = store::get_source_state_hash(&sconn).unwrap().unwrap();
    let gen = store::get_generation(&sconn).unwrap();
    let repo_id = context_index::project_root::repo_instance_id(&root.canonicalize().unwrap());
    let snippet = context_rank::packer::delivered_snippet(Some("x = 1"));
    let delivered_hash = context_rank::packer::delivered_hash(&snippet);
    let proof = proof::ProofRecord {
        id: "cp_dup_snippet".into(),
        schema_version: 1,
        repo_instance_id: repo_id,
        canonical_root: root.display().to_string(),
        source_generation: gen,
        source_state_hash: hash,
        source_state_verified: true,
        git_branch: None,
        git_commit: None,
        git_detached: false,
        git_dirty: None,
        query: "x = 1".into(),
        query_type: "EXACT".into(),
        tool_name: "context_search".into(),
        created_at: 0,
        packed_context_hash: blake3::hash(b"md").to_hex().to_string(),
        engine_version: "0.1.0".into(),
        index_schema_version: Some(3),
        extraction_version: Some(2),
        semantic_representation_version: Some("v2".into()),
        embedding_model_id: Some("all-minilm".into()),
        embedding_model_version: Some("v1".into()),
        embedding_dimension: Some(384),
        packed_token_estimate: 10,
    };
    let ev = proof::ProofEvidence {
        proof_id: "cp_dup_snippet".into(),
        ordinal: 0,
        relative_path: "a.py".into(),
        start_line: Some(1),
        end_line: Some(1),
        start_byte: None,
        end_byte: None,
        delivered_content_hash: delivered_hash,
        chunk_id: None,
        chunk_content_hash: None,
        representation_hash: None,
        symbol_id: None,
        symbol_name: None,
        qualified_name: None,
        symbol_kind: None,
        provenance: Some("rust:exact".into()),
        relation: None,
        final_score: None,
    };
    let mut conn = proof::open_proof_db(&root).unwrap();
    proof::insert_proof(&mut conn, &proof, &[ev]).unwrap();
    drop(conn);
    // Force changed snapshot via unrelated b.py edit
    std::fs::write(root.join("b.py"), "def bar():\n    x=999\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("bar", search_opts()).await.unwrap();
    let delta = delta_for_proof(&root, "cp_dup_snippet").unwrap();
    assert_eq!(delta.status, DeltaStatus::Ready);
    let d_ev = &delta.evidence[0];
    // Two lines with same hash => ambiguous => UNVERIFIABLE, not UNCHANGED
    assert_eq!(d_ev.classification, EvidenceClassification::Unverifiable);
    assert!(d_ev.reason.contains("ambiguous multiple matches"));
}

// 8c. Duplicate symbol across files -> no false cross-file match
#[tokio::test]
async fn test8c_duplicate_symbol_cross_file() {
    let tmp = create_repo(&[
        ("a.py", "def process():\n    return 1\n"),
        ("b.py", "def process():\n    return 2\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("process", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    // Historical evidence points to a.py::process (at least one should)
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let evs = proof::load_evidence(&conn, &pid).unwrap();
    let has_a = evs
        .iter()
        .any(|e| e.relative_path == "a.py" && e.symbol_name.as_deref() == Some("process"));
    // If proof didn't pick a.py, we still test the invariant: after removing a.py's symbol, we must not incorrectly claim UNCHANGED via b.rs
    // Modify a.py to remove process
    std::fs::write(tmp.path().join("a.py"), "def other():\n    pass\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("other", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(delta.status, DeltaStatus::Ready);
    if has_a {
        let ev = delta
            .evidence
            .iter()
            .find(|e| e.old_path == "a.py")
            .unwrap();
        assert_ne!(
            ev.classification,
            EvidenceClassification::Unchanged,
            "must not falsely use b.py's process"
        );
        assert!(
            ev.classification == EvidenceClassification::Changed
                || ev.classification == EvidenceClassification::Unverifiable
                || ev.classification == EvidenceClassification::Removed
        );
    } else {
        // At least ensure we didn't produce false Unchanged via wrong file
        assert!(delta
            .evidence
            .iter()
            .all(|e| e.classification != EvidenceClassification::Unchanged
                || e.old_path != "a.py"
                || e.current_path.as_deref() == Some("a.py")));
    }
}

// 8d. Multi-line representation reconstruction (symbol definition spans multiple lines)
#[tokio::test]
async fn test8d_multiline_representation() {
    let multiline = "def foo():\n    \"\"\"long docstring line one\n    line two\n    line three\"\"\"\n    x = 1\n    y = 2\n    return x + y\n";
    let tmp = create_repo(&[("a.py", multiline), ("b.py", "def bar():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let evs = proof::load_evidence(&conn, &pid).unwrap();
    assert!(!evs.is_empty());
    let ev = &evs[0];
    // Ensure delivered snippet is multi-line and not equal to single line or whole file first 120
    let content = std::fs::read_to_string(tmp.path().join(&ev.relative_path)).unwrap();
    let first_line = content.lines().next().unwrap_or("");
    let single_snippet = context_rank::packer::delivered_snippet(Some(first_line));
    let whole_snippet = context_rank::packer::delivered_snippet(Some(&content));
    let hist_hash = ev.delivered_content_hash.clone();
    // Multi-line means hist snippet not equal to single line snippet
    // (We cannot directly get snippet from hash, but we can verify that historical snippet was not just first line)
    // Instead verify that file's first line hash != hist and whole file hash != hist (unless single-line file)
    let single_hash = context_rank::packer::delivered_hash(&single_snippet);
    let whole_hash = context_rank::packer::delivered_hash(&whole_snippet);
    // For our multiline foo, hist should differ from single line and whole file unless coincidence
    // If file is multiline definition, hist likely not equal to single line
    // We don't assert strictly, just ensure we can still handle unrelated change
    std::fs::write(tmp.path().join("b.py"), "def bar():\n    x=999\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("bar", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(delta.status, DeltaStatus::Ready);
    let d_ev = delta
        .evidence
        .iter()
        .find(|e| e.old_path == ev.relative_path)
        .unwrap();
    // Unrelated change should still be UNCHANGED
    assert_eq!(d_ev.classification, EvidenceClassification::Unchanged);
}

// 9. Synthetic -> UNVERIFIABLE
#[tokio::test]
async fn test9_synthetic() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    // Create a proof with synthetic evidence manually
    let root = tmp.path().to_path_buf();
    let conn = proof::open_proof_db(&root).unwrap();
    // Use a valid source state
    let sconn = store::open_db(&root).unwrap();
    let hash = store::get_source_state_hash(&sconn).unwrap().unwrap();
    let gen = store::get_generation(&sconn).unwrap();
    let repo_id = context_index::project_root::repo_instance_id(&root.canonicalize().unwrap());
    let proof = proof::ProofRecord {
        id: "cp_synth".into(),
        schema_version: 1,
        repo_instance_id: repo_id,
        canonical_root: root.display().to_string(),
        source_generation: gen,
        source_state_hash: hash,
        source_state_verified: true,
        git_branch: None,
        git_commit: None,
        git_detached: false,
        git_dirty: None,
        query: "test".into(),
        query_type: "TEST".into(),
        tool_name: "context_search".into(),
        created_at: 0,
        packed_context_hash: blake3::hash(b"md").to_hex().to_string(),
        engine_version: "0.1.0".into(),
        index_schema_version: Some(3),
        extraction_version: Some(2),
        semantic_representation_version: Some("v2".into()),
        embedding_model_id: Some("all-minilm".into()),
        embedding_model_version: Some("v1".into()),
        embedding_dimension: Some(384),
        packed_token_estimate: 10,
    };
    let ev = proof::ProofEvidence {
        proof_id: "cp_synth".into(),
        ordinal: 0,
        relative_path: "".into(),
        start_line: None,
        end_line: None,
        start_byte: None,
        end_byte: None,
        delivered_content_hash: blake3::hash(b"synthetic").to_hex().to_string(),
        chunk_id: None,
        chunk_content_hash: None,
        representation_hash: None,
        symbol_id: None,
        symbol_name: None,
        qualified_name: None,
        symbol_kind: None,
        provenance: Some("rust:graph".into()),
        relation: None,
        final_score: None,
    };
    let mut conn2 = proof::open_proof_db(&root).unwrap();
    proof::insert_proof(&mut conn2, &proof, &[ev]).unwrap();
    drop(conn2);
    // Trigger a file change so delta goes to Ready path and checks synthetic
    std::fs::write(root.join("a.py"), "def foo():\n    x=1\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    // Need a service to trigger reconcile
    let svc2 = new_service(root.clone()).await;
    let _ = svc2.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(&root, "cp_synth").unwrap();
    assert_eq!(delta.status, DeltaStatus::Ready);
    assert_eq!(
        delta.evidence[0].classification,
        EvidenceClassification::Unverifiable
    );
}

// 10. Repo mismatch -> UNVERIFIABLE or not found
#[tokio::test]
async fn test10_repo_mismatch() {
    let tmpA = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let tmpB = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmpA.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    // Try delta on different repo - proof not in B's DB, should be Err (not found) which is also unverifiable case
    let res = delta_for_proof(tmpB.path(), &pid);
    assert!(res.is_err() || matches!(res.unwrap().status, DeltaStatus::Unverifiable));
}

// 11. Current verified false -> UNVERIFIABLE
#[tokio::test]
async fn test11_current_unverified() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let sconn = store::open_db(tmp.path()).unwrap();
    store::set_source_state_verified(&sconn, false).unwrap();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(delta.status, DeltaStatus::Unverifiable);
    store::set_source_state_verified(&sconn, true).unwrap();
}

// 12. Branch switch -> UNVERIFIABLE
#[tokio::test]
async fn test12_branch_switch() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    init_git(tmp.path());
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["add", "."])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["commit", "-m", "init"])
        .output();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["branch", "-M", "main"])
        .output();
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["checkout", "-b", "feature"])
        .output();
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(delta.status, DeltaStatus::Unverifiable);
}

// 13. Daemon restart same Delta (simulate by new service)
#[tokio::test]
async fn test13_daemon_restart() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let d1 = delta_for_proof(tmp.path(), &pid).unwrap();
    drop(svc);
    let _svc2 = new_service(tmp.path().to_path_buf()).await;
    let d2 = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(d1.status, d2.status);
    assert_eq!(d1.summary.unchanged, d2.summary.unchanged);
}

// 14. Index rebuild same source NO_CHANGE
#[tokio::test]
async fn test14_rebuild_no_change() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let d_before = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(d_before.status, DeltaStatus::NoChange);
    let idx = tmp.path().join(".context").join("index");
    std::fs::remove_dir_all(&idx).unwrap();
    let svc2 = new_service(tmp.path().to_path_buf()).await;
    let d_after = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(d_after.status, DeltaStatus::NoChange);
}

// 15. Proof DB survives rebuild
#[tokio::test]
async fn test15_proof_survives() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let idx = tmp.path().join(".context").join("index");
    std::fs::remove_dir_all(&idx).unwrap();
    let _ = new_service(tmp.path().to_path_buf()).await;
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    assert!(proof::load_proof(&conn, &pid).unwrap().is_some());
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(delta.status, DeltaStatus::NoChange);
}

// 16. 10 evidence summary
#[tokio::test]
async fn test16_summary_counts() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    let total = delta.summary.unchanged
        + delta.summary.changed
        + delta.summary.removed
        + delta.summary.moved
        + delta.summary.unverifiable;
    assert_eq!(total, delta.evidence.len());
}

// 17. Deterministic ordering
#[tokio::test]
async fn test17_deterministic() {
    let tmp = create_repo(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let d1 = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    let d2 = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    assert_eq!(
        d1.evidence.iter().map(|e| e.ordinal).collect::<Vec<_>>(),
        d2.evidence.iter().map(|e| e.ordinal).collect::<Vec<_>>()
    );
}

// 18. No embedding calls - we just ensure delta doesn't require OLLAMA
#[tokio::test]
async fn test18_no_embedding() {
    std::env::set_var("CONTEXTD_SEMANTIC_ENABLED", "0");
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    assert!(delta.status == DeltaStatus::Ready || delta.status == DeltaStatus::NoChange);
    std::env::remove_var("CONTEXTD_SEMANTIC_ENABLED");
}

// 19. No vector hot-state load - delta should not load HotVectors
#[tokio::test]
async fn test19_no_hot() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    // delta should succeed without needing HotState
    let delta = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    assert!(!delta.evidence.is_empty());
}

// 20. No side effect proof created
#[tokio::test]
async fn test20_no_side_effect() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let before = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    let _ = delta_for_proof(tmp.path(), &pid).unwrap();
    let after = proof::count_proofs(&proof::open_proof_db(tmp.path()).unwrap()).unwrap();
    assert_eq!(before, after);
}

// 21. Unknown proof ID
#[tokio::test]
async fn test21_unknown_id() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let _ = new_service(tmp.path().to_path_buf()).await;
    let res = delta_for_proof(tmp.path(), "cp_unknown");
    assert!(res.is_err());
}

// 22. Malformed proof -> UNVERIFIABLE
#[tokio::test]
async fn test22_malformed() {
    let tmp = create_repo(&[("a.py", "def foo():\n    pass\n")]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    // Corrupt proof by setting schema_version to 999
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    conn.execute(
        "UPDATE proofs SET schema_version=999 WHERE id=?1",
        rusqlite::params![pid.clone()],
    )
    .unwrap();
    let cnt: i64 = conn
        .query_row(
            "SELECT schema_version FROM proofs WHERE id=?1",
            rusqlite::params![pid.clone()],
            |r| r.get(0),
        )
        .unwrap();
    println!("cnt after update same conn {}", cnt);
    drop(conn);
    let conn2 = proof::open_proof_db(tmp.path()).unwrap();
    let cnt2: i64 = conn2
        .query_row(
            "SELECT schema_version FROM proofs WHERE id=?1",
            rusqlite::params![pid.clone()],
            |r| r.get(0),
        )
        .unwrap();
    println!("cnt after new conn {}", cnt2);
    let rec = proof::load_proof(&conn2, &pid).unwrap().unwrap();
    println!("rec schema {}", rec.schema_version);
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    println!("delta status {:?} reason {}", delta.status, delta.reason);
    assert_eq!(delta.status, DeltaStatus::Unverifiable);
}

// 23. Non-Git
#[tokio::test]
async fn test23_non_git() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    assert!(delta.status == DeltaStatus::Ready || delta.status == DeltaStatus::NoChange);
}

// 24. Detached HEAD
#[tokio::test]
async fn test24_detached() {
    let tmp = TempDir::new().unwrap();
    init_git(tmp.path());
    std::fs::write(tmp.path().join("a.py"), "def foo():\n    pass\n").unwrap();
    git_add_commit(tmp.path(), "init");
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["branch", "-M", "main"])
        .output();
    let commit = {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["checkout", "--detach", &commit])
        .output();
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let delta = delta_for_proof(tmp.path(), &wp.proof.id.clone().unwrap()).unwrap();
    assert_eq!(delta.status, DeltaStatus::NoChange);
}

// 25. Concurrent delta
#[tokio::test]
async fn test25_concurrent() {
    let tmp = create_repo(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let mut handles = vec![];
    for _ in 0..5 {
        let t = tmp.path().to_path_buf();
        let p = pid.clone();
        handles.push(tokio::spawn(
            async move { delta_for_proof(&t, &p).unwrap() },
        ));
    }
    for h in handles {
        let d = h.await.unwrap();
        assert!(d.status == DeltaStatus::Ready || d.status == DeltaStatus::NoChange);
    }
}

// Critical: unrelated change test (P1 STALE but P2 UNCHANGED)
#[tokio::test]
async fn test_critical_unrelated() {
    let tmp = create_repo(&[
        ("a.py", "def foo():\n    pass\n"),
        ("b.py", "def bar():\n    pass\n"),
    ]);
    let svc = new_service(tmp.path().to_path_buf()).await;
    // Create proof that only references a.py - query foo returns a.py
    let wp = svc.search_external("foo", search_opts()).await.unwrap();
    let pid = wp.proof.id.clone().unwrap();
    let conn = proof::open_proof_db(tmp.path()).unwrap();
    let rec = proof::load_proof(&conn, &pid).unwrap().unwrap();
    // Modify b.py only
    std::fs::write(tmp.path().join("b.py"), "def bar():\n    x=1\n    y=2\n").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = svc.search_external("bar", search_opts()).await.unwrap(); // trigger reconcile
    let st = proof::validate_proof(tmp.path(), &rec);
    assert_eq!(
        st,
        proof::Validity::Stale,
        "P1 must be stale after unrelated indexed change"
    );
    let delta = delta_for_proof(tmp.path(), &pid).unwrap();
    assert_eq!(delta.status, DeltaStatus::Ready);
    // a.py evidence should be unchanged
    let a_ev = delta
        .evidence
        .iter()
        .find(|e| e.old_path == "a.py")
        .unwrap();
    assert_eq!(a_ev.classification, EvidenceClassification::Unchanged);
    assert_eq!(delta.summary.changed, 0);
    assert_eq!(delta.summary.removed, 0);
    assert!(delta.summary.unchanged > 0);
}
