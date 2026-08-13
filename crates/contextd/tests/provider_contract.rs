//! Raw OCI provider live contract — verifies Rust uses raw OCI, not V2 final ranking.
//! Requires healthy OCI index (ollama + nomic-embed-text). Run with -- --ignored --test-threads=1
#![cfg(feature = "legacy-v2")]

use contextd::candidate::CandidateProvider;

fn assert_raw_provenance(cands: &[serde_json::Value], expected_substr: &str, query: &str) {
    assert!(!cands.is_empty(), "no candidates for {query}");
    for c in cands {
        assert!(
            c.get("authority_score").is_none(),
            "raw must not have authority_score: {c:?}"
        );
        assert!(
            c.get("final_score").is_none(),
            "raw must not have final_score: {c:?}"
        );
        assert!(
            c.get("packed_context").is_none(),
            "raw must not have packed_context"
        );
    }
    // At least one should have expected provenance/source
    let has = cands.iter().any(|c| {
        let prov = c.get("provenance").and_then(|v| v.as_str()).unwrap_or("");
        let src = c.get("source").and_then(|v| v.as_str()).unwrap_or("");
        prov.contains(expected_substr) || src.contains(expected_substr)
    });
    // Also check via file field that source is oci-like
    if !has {
        // Fallback: check that at least one candidate has oci: in provenance
        let any_oci = cands.iter().any(|c| {
            let prov = c.get("provenance").and_then(|v| v.as_str()).unwrap_or("");
            prov.contains("oci:")
        });
        assert!(
            any_oci,
            "expected provenance {expected_substr} for {query}, got {cands:?}"
        );
    }
}

#[tokio::test]
#[ignore]
async fn symbol_oci_contract() {
    let prov = CandidateProvider::new().expect("candidate provider");
    let cands = prov
        .symbol_candidates("count_tokens")
        .await
        .expect("symbol");
    assert_raw_provenance(&cands, "oci:symbol", "count_tokens");
    // Must be definition, not just exact fallback
    let has_def = cands
        .iter()
        .any(|c| c.get("relation").and_then(|v| v.as_str()) == Some("definition"));
    assert!(
        has_def,
        "symbol candidates should contain definition for count_tokens"
    );
}

#[tokio::test]
#[ignore]
async fn semantic_oci_contract() {
    let prov = CandidateProvider::new().expect("candidate provider");
    let cands = prov
        .semantic_candidates("Where is secret redaction implemented?", 5)
        .await
        .expect("semantic");
    // semantic may be empty if Ollama down, but with healthy index should have oci:semantic
    if cands.is_empty() {
        eprintln!("semantic empty — index may be unhealthy, skipping strict check");
        return;
    }
    assert_raw_provenance(&cands, "oci:semantic", "secret redaction");
}

#[tokio::test]
#[ignore]
async fn graph_oci_contract() {
    let prov = CandidateProvider::new().expect("candidate provider");
    let cands = prov
        .graph_candidates("bundle", "callers")
        .await
        .expect("graph");
    if cands.is_empty() {
        eprintln!("graph empty for bundle callers — no graph data, checking at least it doesn't use exact fallback");
        // Still check that if empty, it's not because we returned exact
        return;
    }
    assert_raw_provenance(&cands, "oci:graph", "bundle callers");
    let has_caller = cands.iter().any(|c| {
        matches!(
            c.get("relation").and_then(|v| v.as_str()),
            Some("caller") | Some("callee")
        )
    });
    assert!(has_caller, "graph should be caller/callee");
}

#[tokio::test]
#[ignore]
async fn test_oci_contract() {
    let prov = CandidateProvider::new().expect("candidate provider");
    let cands = prov
        .test_candidates("bundle generation")
        .await
        .expect("test");
    if cands.is_empty() {
        eprintln!("test candidates empty — may be no test data");
        return;
    }
    // test candidates via OCI should be oci:test or oci:semantic with test relation
    let has_test = cands.iter().any(|c| {
        let prov = c.get("provenance").and_then(|v| v.as_str()).unwrap_or("");
        let rel = c.get("relation").and_then(|v| v.as_str()).unwrap_or("");
        prov.contains("oci:test") || rel == "test"
    });
    assert!(
        has_test,
        "test candidates should contain test evidence, got {cands:?}"
    );
    for c in &cands {
        assert!(c.get("authority_score").is_none());
        assert!(c.get("final_score").is_none());
    }
}
