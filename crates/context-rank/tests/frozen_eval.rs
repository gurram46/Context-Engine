use context_rank::types::{Evidence, EvidenceRelation, RetrievalSource};
use context_rank::{
    apply_authority, classify_query, fuse_evidence, pack_evidence, FuseOptions, PackOptions,
};

fn raw(
    source: RetrievalSource,
    file: &str,
    symbol: Option<&str>,
    kind: Option<&str>,
    text: Option<&str>,
    score: f64,
    relation: EvidenceRelation,
) -> Evidence {
    Evidence {
        source,
        file: file.to_string(),
        start_line: Some(10),
        end_line: Some(20),
        symbol: symbol.map(|s| s.to_string()),
        symbol_kind: kind.map(|k| k.to_string()),
        text: text.map(|t| t.to_string()),
        score: Some(score),
        relation: Some(relation),
        authority_score: None,
        final_score: None,
        provenance: Some(format!("fixture:{}", source.as_str())),
        metadata: None,
    }
}

fn assert_raw_is_clean(ev: &Evidence) {
    assert!(
        ev.authority_score.is_none(),
        "raw candidate must not have authority_score: {:?}",
        ev
    );
    assert!(
        ev.final_score.is_none(),
        "raw candidate must not have final_score: {:?}",
        ev
    );
    assert!(
        ev.metadata.is_none()
            || !ev
                .metadata
                .as_ref()
                .unwrap()
                .to_string()
                .contains("packed_context"),
        "raw must not carry packed_context"
    );
}

fn run_pipeline(query: &str, candidates: Vec<Evidence>) -> Vec<Evidence> {
    for c in &candidates {
        assert_raw_is_clean(c);
    }
    let classified = classify_query(query);
    // ensure we run real R2 logic
    let _plan = context_rank::build_retrieval_plan(query);
    let scored = apply_authority(candidates, classified.query_type, query);
    // scored must now have authority
    for s in &scored {
        assert!(s.authority_score.is_some(), "authority should be set");
        assert!(s.final_score.is_some(), "final_score should be set");
    }
    let fused = fuse_evidence(
        scored,
        FuseOptions {
            top_n: 5,
            query_type: classified.query_type,
            raw_query: query.to_string(),
        },
    );
    let _packed = pack_evidence(
        &fused.ranked,
        query,
        classified.query_type,
        PackOptions {
            budget: 10000,
            max_files: 5,
        },
    );
    fused.ranked
}

#[test]
fn bundle_generation_flow_top1() {
    let query = concat!(
        "Trace the Bundle Generation Flow ",
        "context bundle --no-ai to .context/context_for_ai.md"
    );
    // Deliberately put correct NOT first, with lower raw score
    let candidates = vec![
        raw(
            RetrievalSource::Semantic,
            "docs/architecture/bundle.md",
            None,
            None,
            Some("Bundle Generation Flow documented here"),
            1.0,
            EvidenceRelation::Unknown,
        ),
        raw(
            RetrievalSource::Semantic,
            "backend/context_engine/cli.py",
            None,
            None,
            Some("bundle generation imported"),
            0.85,
            EvidenceRelation::Reference,
        ),
        raw(
            RetrievalSource::Symbol,
            "backend/context_engine/commands/bundle_command.py",
            Some("bundle_command"),
            Some("function_definition"),
            Some("def bundle_command():\n    pass"),
            0.70,
            EvidenceRelation::Definition,
        ),
        raw(
            RetrievalSource::Semantic,
            "backend/context_engine/core/utils.py",
            Some("count_tokens"),
            Some("function_definition"),
            Some("def count_tokens():"),
            0.60,
            EvidenceRelation::Definition,
        ),
    ];
    let ranked = run_pipeline(query, candidates);
    assert!(!ranked.is_empty(), "no ranked");
    assert_eq!(
        ranked[0].file,
        "backend/context_engine/commands/bundle_command.py",
        "Bundle Generation Flow should be bundle_command.py, got {:?}",
        ranked.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
}

#[test]
fn count_tokens_top1() {
    let query = format!("Where is {} implemented?", "count_tokens");
    let candidates = vec![
        raw(
            RetrievalSource::Semantic,
            "docs/utils.md",
            None,
            None,
            Some("count_tokens documented in docs"),
            1.0,
            EvidenceRelation::Unknown,
        ),
        raw(
            RetrievalSource::Semantic,
            "backend/context_engine/commands/bundle_command.py",
            Some("count_tokens"),
            None,
            Some("from core.utils import count_tokens"),
            0.85,
            EvidenceRelation::Reference,
        ),
        raw(
            RetrievalSource::Symbol,
            "backend/context_engine/core/utils.py",
            Some("count_tokens"),
            Some("function_definition"),
            Some("def count_tokens(text: str):\n    return len(text)"),
            0.70,
            EvidenceRelation::Definition,
        ),
    ];
    let ranked = run_pipeline(&query, candidates);
    assert_eq!(
        ranked[0].file,
        "backend/context_engine/core/utils.py",
        "count_tokens should be core/utils.py, got {:?}",
        ranked.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
}

#[test]
fn what_calls_bundle_top1() {
    let query = format!("What calls {}?", "bundle");
    let candidates = vec![
        raw(
            RetrievalSource::Symbol,
            "backend/context_engine/commands/bundle_command.py",
            Some("bundle_command"),
            Some("function_definition"),
            Some("def bundle_command():\n    pass"),
            1.0,
            EvidenceRelation::Definition,
        ),
        raw(
            RetrievalSource::Semantic,
            "docs/cli.md",
            None,
            None,
            Some("bundle callers documented"),
            0.90,
            EvidenceRelation::Unknown,
        ),
        raw(
            RetrievalSource::Graph,
            "backend/context_engine/cli.py",
            Some("bundle_command"),
            Some("function_call"),
            Some("from commands.bundle_command import bundle_command\nbundle_command()\nadd_command('bundle')"),
            0.70,
            EvidenceRelation::Caller,
        ),
    ];
    let ranked = run_pipeline(&query, candidates);
    assert_eq!(
        ranked[0].file,
        "backend/context_engine/cli.py",
        "What calls bundle should be cli.py, got {:?}",
        ranked
            .iter()
            .map(|e| format!("{}:{:?}:{:?}", e.file, e.relation, e.final_score))
            .collect::<Vec<_>>()
    );
}

#[test]
fn tests_cover_bundle_top1() {
    let query = format!("What tests cover {} {}?", "bundle", "generation");
    let candidates = vec![
        raw(
            RetrievalSource::Semantic,
            "backend/context_engine/commands/bundle_command.py",
            Some("bundle_command"),
            Some("function_definition"),
            Some("def bundle_command(): pass  # bundle generation logic"),
            1.0,
            EvidenceRelation::Unknown,
        ),
        raw(
            RetrievalSource::Semantic,
            "docs/testing.md",
            None,
            None,
            Some("bundle generation testing docs"),
            0.90,
            EvidenceRelation::Unknown,
        ),
        raw(
            RetrievalSource::Test,
            "tests/test_bundle_integration.py",
            Some("test_bundle_generation"),
            Some("function_definition"),
            Some("def test_bundle_generation():\n    assert bundle()"),
            0.85,
            EvidenceRelation::Test,
        ),
    ];
    let ranked = run_pipeline(&query, candidates);
    assert_eq!(
        ranked[0].file,
        "tests/test_bundle_integration.py",
        "tests cover bundle should be test_bundle_integration.py, got {:?}",
        ranked.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
}

#[test]
fn secret_redaction_top1() {
    let query = format!("Where is {} {} implemented?", "secret", "redaction");
    let candidates = vec![
        raw(
            RetrievalSource::Semantic,
            "docs/security.md",
            None,
            None,
            Some("secret redaction documented here"),
            1.0,
            EvidenceRelation::Unknown,
        ),
        raw(
            RetrievalSource::Semantic,
            "backend/context_engine/commands/bundle_command.py",
            None,
            None,
            Some("secret handling in bundle"),
            0.85,
            EvidenceRelation::Reference,
        ),
        raw(
            RetrievalSource::Symbol,
            "backend/context_engine/core/utils.py",
            Some("redact_secrets"),
            Some("function_definition"),
            Some("def redact_secrets(data):\n    return data"),
            0.70,
            EvidenceRelation::Definition,
        ),
    ];
    let ranked = run_pipeline(&query, candidates);
    assert_eq!(
        ranked[0].file,
        "backend/context_engine/core/utils.py",
        "secret redaction should be core/utils.py, got {:?}",
        ranked
            .iter()
            .map(|e| format!("{} {:?}", e.file, e.final_score))
            .collect::<Vec<_>>()
    );
}

#[test]
fn badly_ordered_raw_candidates_regression() {
    let query = format!("Where is {} implemented?", "NewRouter");
    // Badly ordered: doc first with 1.0, correct second with 0.70
    let candidates = vec![
        raw(
            RetrievalSource::Semantic,
            "docs/router.md",
            None,
            None,
            Some("Router documentation for NewRouter"),
            1.0,
            EvidenceRelation::Unknown,
        ),
        raw(
            RetrievalSource::Symbol,
            "internal/router/router.go",
            Some("NewRouter"),
            Some("function_definition"),
            Some("func NewRouter() Router {\n    return &router{}\n}"),
            0.70,
            EvidenceRelation::Definition,
        ),
    ];
    // Assert raw cannot carry forbidden fields
    for c in &candidates {
        assert!(c.authority_score.is_none());
        assert!(c.final_score.is_none());
        assert!(
            c.metadata.is_none()
                || !c
                    .metadata
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("packed_context")
        );
        // Also ensure no final_score in raw JSON perspective
        let v = serde_json::to_value(c).unwrap();
        assert!(v.get("authority_score").is_none() || v.get("authority_score").unwrap().is_null());
        assert!(v.get("final_score").is_none() || v.get("final_score").unwrap().is_null());
        assert!(v.get("packed_context").is_none());
    }
    let ranked = run_pipeline(&query, candidates);
    assert_eq!(
        ranked[0].file,
        "internal/router/router.go",
        "NewRouter should be router.go not docs, got {:?}",
        ranked.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
    // Also verify provider response contract: raw candidates had no ranking
    // The test itself proves Rust ranking promotes correct despite input order
}
