//! Provider failure / exact fallback — verifies Rust still provides evidence when OCI is empty.
//! Uses only Rust exact (rg) + authority/fuse, no OCI. Must be deterministic and not ignored.

use context_index::project_root::ProjectRoot;
use context_index::{ExactQuery, ExactSearchOptions, ProjectIndex};
use context_rank::{apply_authority, fuse_evidence, FuseOptions, QueryType};
use std::path::Path;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[tokio::test]
async fn exact_fallback_test_bundle() {
    let root = ProjectRoot::resolve(Some(workspace_root().as_path())).expect("root");
    let idx = ProjectIndex::discover(&root).expect("index");
    // Simulate OCI empty: only exact for test_bundle
    let res = context_index::exact::exact_search(
        &idx,
        ExactQuery::Literal("test_bundle".to_string()),
        ExactSearchOptions {
            max_results: 50,
            ..Default::default()
        },
    )
    .await
    .expect("exact");
    assert!(
        !res.is_empty(),
        "exact fallback for test_bundle should find test file"
    );
    let has_test = res
        .iter()
        .any(|e| e.file.ends_with("test_bundle_integration.py"));
    assert!(
        has_test,
        "should find test_bundle_integration.py via exact, got {:?}",
        res.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
    // Also verify authority/fuse still ranks it
    let evidences: Vec<context_rank::types::Evidence> = res
        .into_iter()
        .map(|e| context_rank::types::Evidence {
            source: context_rank::types::RetrievalSource::Exact,
            file: e.file,
            start_line: Some(e.line),
            end_line: e.end_line,
            symbol: None,
            symbol_kind: None,
            text: Some(e.text),
            score: Some(1.0),
            relation: Some(context_rank::types::EvidenceRelation::Reference),
            authority_score: None,
            final_score: None,
            provenance: Some("rust:exact".into()),
            metadata: None,
        })
        .collect();
    let scored = apply_authority(
        evidences,
        QueryType::Test,
        "What tests cover bundle generation?",
    );
    let fused = fuse_evidence(
        scored,
        FuseOptions {
            top_n: 50,
            query_type: QueryType::Test,
            raw_query: "What tests cover bundle generation?".into(),
        },
    );
    assert!(!fused.ranked.is_empty());
    assert!(
        fused
            .ranked
            .iter()
            .any(|e| e.file.ends_with("test_bundle_integration.py")),
        "test_bundle_integration.py should be in ranked results, got {:?}",
        fused.ranked.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn exact_fallback_redact_secrets() {
    let root = ProjectRoot::resolve(Some(workspace_root().as_path())).expect("root");
    let idx = ProjectIndex::discover(&root).expect("index");
    let res = context_index::exact::exact_search(
        &idx,
        ExactQuery::Literal("redact_secrets".to_string()),
        ExactSearchOptions {
            max_results: 100,
            ..Default::default()
        },
    )
    .await
    .expect("exact");
    assert!(
        !res.is_empty(),
        "exact fallback for redact_secrets should find core/utils.py"
    );
    let has_core = res.iter().any(|e| e.file.ends_with("core/utils.py"));
    assert!(
        has_core,
        "should find core/utils.py via exact, got {:?}",
        res.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn exact_fallback_bundle_wiring() {
    let root = ProjectRoot::resolve(Some(workspace_root().as_path())).expect("root");
    let idx = ProjectIndex::discover(&root).expect("index");
    let res = context_index::exact::exact_search(
        &idx,
        ExactQuery::Literal("bundle_command.bundle".to_string()),
        ExactSearchOptions {
            max_results: 50,
            ..Default::default()
        },
    )
    .await
    .expect("exact");
    assert!(
        !res.is_empty(),
        "exact fallback for bundle_command.bundle should find cli.py"
    );
    let has_cli = res.iter().any(|e| e.file.ends_with("cli.py"));
    assert!(
        has_cli,
        "should find cli.py via exact wiring, got {:?}",
        res.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
}
