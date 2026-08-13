use context_rank::types::{Evidence, EvidenceRelation, RetrievalSource};
use context_rank::{apply_authority, classify_query, fuse_evidence, FuseOptions};

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
        provenance: Some(format!("synthetic:{}", source.as_str())),
        metadata: None,
    }
}

fn run(query: &str, candidates: Vec<Evidence>) -> Vec<Evidence> {
    let classified = classify_query(query);
    let scored = apply_authority(candidates, classified.query_type, query);
    let fused = fuse_evidence(
        scored,
        FuseOptions {
            top_n: 5,
            query_type: classified.query_type,
            raw_query: query.to_string(),
        },
    );
    fused.ranked
}

/// TEST query: genuine test file must beat exact+source impl even with slightly weaker literal evidence.
#[test]
fn test_intent_prefers_genuine_test_over_exact_source() {
    let query = "What tests cover payment handling?";
    // Verify classifier sees TEST
    let cq = classify_query(query);
    assert_eq!(
        cq.query_type,
        context_rank::types::QueryType::Test,
        "query must classify as TEST, got {:?} hints {:?}",
        cq.query_type,
        cq.hints
    );

    let candidates = vec![
        // Candidate A: implementation/source, exact literal match, same-language impl
        raw(
            RetrievalSource::Exact,
            "src/payment/service.go",
            Some("HandlePayment"),
            Some("function_definition"),
            Some("func HandlePayment() {} // payment handling impl"),
            1.0,
            EvidenceRelation::Reference,
        ),
        // Candidate B: genuine test file, Test relation, slightly weaker literal evidence
        raw(
            RetrievalSource::Semantic,
            "tests/test_payment_service.py",
            Some("test_handle_payment"),
            Some("function_definition"),
            Some("def test_handle_payment():\n    assert handle_payment()"),
            0.92,
            EvidenceRelation::Test,
        ),
    ];

    let ranked = run(query, candidates);
    assert_eq!(
        ranked[0].file,
        "tests/test_payment_service.py",
        "TEST query must prefer genuine test file over exact source; ranked {:?}",
        ranked
            .iter()
            .map(|e| format!(
                "{} auth={:?} final={:?}",
                e.file, e.authority_score, e.final_score
            ))
            .collect::<Vec<_>>()
    );
}

/// Also test with Bm25 source and symbol match to ensure test bonus dominates.
#[test]
fn test_intent_prefers_test_even_when_impl_has_symbol_bonus() {
    let query = "What tests cover bundle generation?";
    let cq = classify_query(query);
    assert_eq!(cq.query_type, context_rank::types::QueryType::Test);

    let candidates = vec![
        raw(
            RetrievalSource::Exact,
            "backend/service/bundle.py",
            Some("bundle_generation"),
            Some("function_definition"),
            Some("def bundle_generation(): pass"),
            1.0,
            EvidenceRelation::Definition,
        ),
        raw(
            RetrievalSource::Bm25,
            "tests/test_bundle_generation.py",
            Some("test_bundle_generation"),
            Some("function_definition"),
            Some("def test_bundle_generation():\n    bundle_generation()"),
            0.85,
            EvidenceRelation::Test,
        ),
    ];
    let ranked = run(query, candidates);
    assert_eq!(
        ranked[0].file,
        "tests/test_bundle_generation.py",
        "TEST intent B should win even with Bm25 weaker score; got {:?}",
        ranked.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
}

/// Non-TEST query must NOT promote test files.
#[test]
fn non_test_intent_does_not_promote_test() {
    let query = "Where is payment handling implemented?";
    let cq = classify_query(query);
    // This should NOT be Test (Symbol or Mixed or Conceptual)
    assert_ne!(
        cq.query_type,
        context_rank::types::QueryType::Test,
        "non-test query must not classify as TEST"
    );

    let candidates = vec![
        raw(
            RetrievalSource::Exact,
            "src/payment/service.go",
            Some("HandlePayment"),
            Some("function_definition"),
            Some("func HandlePayment() {}"),
            1.0,
            EvidenceRelation::Reference,
        ),
        raw(
            RetrievalSource::Semantic,
            "tests/test_payment_service.py",
            Some("test_handle_payment"),
            Some("function_definition"),
            Some("def test_handle_payment():\n    assert handle_payment()"),
            0.95,
            EvidenceRelation::Test,
        ),
    ];

    let ranked = run(query, candidates);
    assert_eq!(
        ranked[0].file,
        "src/payment/service.go",
        "non-TEST query must prefer source impl, not test; ranked {:?}",
        ranked
            .iter()
            .map(|e| format!(
                "{} auth={:?} final={:?}",
                e.file, e.authority_score, e.final_score
            ))
            .collect::<Vec<_>>()
    );
}
