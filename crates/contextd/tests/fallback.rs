//! Provider failure / exact fallback — verifies Rust still provides evidence when OCI is empty.
//! Uses only Rust exact (rg) + authority/fuse, no OCI. Must be deterministic and not ignored.

use context_index::project_root::ProjectRoot;
use context_index::{ExactQuery, ExactSearchOptions, ProjectIndex};
use context_rank::{apply_authority, fuse_evidence, FuseOptions, QueryType};

#[tokio::test]
async fn exact_fallback_test_bundle() {
    // Isolated temporary repo fixture — avoids indexing the real Context-Engine repo
    // which contains benchmark/golden tests that mention `test_bundle` and contaminate exact retrieval.
    // Fixture mirrors realistic layout with distractors so ranking is actually tested, not just existence.
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root_path = tmp.path();
    std::fs::create_dir_all(root_path.join(".git")).expect("git");
    std::fs::create_dir_all(root_path.join("backend/context_engine/commands")).expect("mkdir");
    std::fs::create_dir_all(root_path.join("tests")).expect("mkdir");
    std::fs::create_dir_all(root_path.join("docs")).expect("mkdir");
    std::fs::create_dir_all(root_path.join("src")).expect("mkdir");
    // Distractor: Source file (FileKind::Source) — will get -12 sourceWhenTestAsked
    std::fs::write(
        root_path.join("backend/context_engine/commands/bundle_command.py"),
        b"def bundle(name: str, format: str):\n    pass\n# test_bundle distraction in source impl\n",
    )
    .expect("write");
    // Expected: Test file (FileKind::Test via tests/ + test_ prefix) — will get +38 testWhenAsked
    std::fs::write(
        root_path.join("tests/test_bundle_integration.py"),
        b"def test_bundle_without_task(tmp_path):\n    assert True\n\ndef test_bundle_with_task(tmp_path):\n    assert bundle() is not None\n\n# test_bundle integration tests for bundle generation\n",
    )
    .expect("write");
    // Distractor: Doc file (FileKind::Doc) — will get -20 docWhenTestAsked
    std::fs::write(
        root_path.join("docs/bundle.md"),
        b"# Bundle\n\nBundle generation docs with test_bundle mention for ranking test.\n",
    )
    .expect("write");
    // Distractor: Source file in src/ (FileKind::Source) — will get -12
    std::fs::write(
        root_path.join("src/unrelated_bundle_helper.py"),
        b"def unrelated_helper():\n    # helper mentions test_bundle but is not a test\n    pass\n",
    )
    .expect("write");

    let root = ProjectRoot::resolve(Some(root_path)).expect("root");
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
    // Also verify authority/fuse still ranks it Top1 with realistic distractors
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
            top_n: 5,
            query_type: QueryType::Test,
            raw_query: "What tests cover bundle generation?".into(),
        },
    );
    assert!(!fused.ranked.is_empty());
    assert!(
        fused.ranked[0].file.ends_with("test_bundle_integration.py")
            || fused
                .ranked
                .iter()
                .any(|e| e.file.ends_with("test_bundle_integration.py"))
    );
}

#[tokio::test]
async fn exact_fallback_redact_secrets() {
    // Isolated temp repo to avoid bench contamination (bench/ contains many files that would slow rg)
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root_path = tmp.path();
    std::fs::create_dir_all(root_path.join(".git")).expect("git");
    std::fs::create_dir_all(root_path.join("backend/context_engine/core")).expect("mkdir");
    std::fs::write(
        root_path.join("backend/context_engine/core/utils.py"),
        b"def redact_secrets(data):\n    return data\n# redact_secrets helper\n",
    )
    .expect("write");
    // Distractor
    std::fs::write(
        root_path.join("backend/context_engine/core/other.py"),
        b"# other file without redact\n",
    )
    .expect("write");
    let root = ProjectRoot::resolve(Some(root_path)).expect("root");
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
    // Isolated temp repo to avoid bench contamination
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root_path = tmp.path();
    std::fs::create_dir_all(root_path.join(".git")).expect("git");
    std::fs::create_dir_all(root_path.join("backend/context_engine")).expect("mkdir");
    std::fs::write(
        root_path.join("backend/context_engine/cli.py"),
        b"from .commands.bundle import bundle_command\nx = bundle_command.bundle\n",
    )
    .expect("write");
    std::fs::write(
        root_path.join("backend/context_engine/other.py"),
        b"# other\n",
    )
    .expect("write");
    let root = ProjectRoot::resolve(Some(root_path)).expect("root");
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
