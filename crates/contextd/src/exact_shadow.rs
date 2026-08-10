use context_index::{ExactEvidence, ExactQuery, ExactSearchOptions, ProjectIndex};
use tracing::{debug, info};

/// Parse a raw query string into an `ExactQuery` for Rust exact search.
/// Mirrors `v2/src/router/classifyQuery.ts` EXACT detection but minimal.
pub fn parse_exact_query(raw: &str) -> Option<ExactQuery> {
    let q = raw.trim();
    if q.is_empty() {
        return None;
    }
    // Quoted literal
    if (q.starts_with('"') && q.ends_with('"') && q.len() >= 2)
        || (q.starts_with('\'') && q.ends_with('\'') && q.len() >= 2)
    {
        let inner = q[1..q.len() - 1].to_string();
        return Some(ExactQuery::Literal(inner));
    }
    // Path-like or filename
    if q.contains('/') {
        // If it looks like a path with extension, treat as Path
        if q.contains('.') {
            return Some(ExactQuery::Path(q.to_string()));
        }
        return Some(ExactQuery::Literal(q.to_string()));
    }
    if q.split_whitespace().count() == 1 && q.contains('.') {
        // Single token filename
        return Some(ExactQuery::FileName(q.to_string()));
    }
    // Single identifier (snake, etc) — treat as Identifier
    if q.split_whitespace().count() == 1
        && q.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Some(ExactQuery::Identifier(q.to_string()));
    }
    // Multi-word literal
    Some(ExactQuery::Literal(q.to_string()))
}

/// Run Rust exact search in shadow mode and compare with V2 exact.
/// Returns Rust evidence; logs mismatch metrics.
pub async fn shadow_exact(
    project: &ProjectIndex,
    raw_query: &str,
    v2_evidence: &[serde_json::Value],
) -> Vec<ExactEvidence> {
    let parsed = match parse_exact_query(raw_query) {
        Some(q) => q,
        None => return Vec::new(),
    };
    let opts = ExactSearchOptions {
        max_results: 20,
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    let rust_res = match context_index::exact::exact_search(project, parsed.clone(), opts).await {
        Ok(r) => r,
        Err(e) => {
            debug!(query = %raw_query, error = %e, "rust exact failed");
            return Vec::new();
        }
    };
    let elapsed = t0.elapsed();

    // Compare with V2: check top file presence
    let rust_files: Vec<String> = rust_res.iter().map(|e| e.file.clone()).collect();
    let v2_files: Vec<String> = v2_evidence
        .iter()
        .filter_map(|v| {
            v.get("file")
                .and_then(|f| f.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    let top_match = rust_files
        .first()
        .zip(v2_files.first())
        .map(|(r, v)| r == v)
        .unwrap_or(false);
    let any_match = rust_files.iter().any(|rf| v2_files.contains(rf));

    info!(
        query = %raw_query,
        parsed = ?parsed,
        rust_count = %rust_res.len(),
        v2_count = %v2_files.len(),
        top_match = %top_match,
        any_match = %any_match,
        elapsed_ms = %elapsed.as_millis(),
        "shadow exact"
    );

    // For now, just return Rust evidence; caller decides whether to use it.
    rust_res
}
