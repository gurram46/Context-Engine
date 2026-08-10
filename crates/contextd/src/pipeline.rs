use context_index::{exact_search, ExactSearchOptions, ProjectIndex};
use context_rank::types::Evidence;
use context_rank::{
    apply_authority, build_retrieval_plan, classify_query, fuse_evidence, pack_evidence,
    FuseOptions, PackOptions, QueryType,
};
use std::time::Instant;

/// Retrieval providers — for R2, V2/OCI provides semantic/symbol/graph candidates.
pub struct Providers {
    pub v2: std::sync::Arc<crate::bridge::V2Bridge>,
}

/// Context result after Rust pipeline.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContextResult {
    pub query: String,
    pub query_type: QueryType,
    pub evidence: Vec<Evidence>,
    pub packed: context_rank::packer::PackedResult,
    pub stats: PipelineStats,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineStats {
    pub candidate_count: usize,
    pub evidence_count: usize,
    pub files_returned: usize,
    pub packed_tokens: usize,
    pub retrievers_used: Vec<String>,
    pub elapsed_ms: u128,
}

/// Main Rust retrieval pipeline for R2.
/// 1. classify, 2. plan, 3. Rust exact, 4. V2 candidates, 5. authority, 6. fuse, 7. pack.
pub async fn retrieve_context(
    query: &str,
    project: &ProjectIndex,
    providers: &Providers,
    budget_tokens: usize,
    max_results: usize,
) -> Result<ContextResult, anyhow::Error> {
    let t0 = Instant::now();
    let classified = classify_query(query);
    let plan = build_retrieval_plan(query);

    let mut candidates: Vec<Evidence> = Vec::new();
    let mut retrievers_used = Vec::new();

    // Rust exact
    for eq in &plan.exact_queries {
        let t = Instant::now();
        let opts = ExactSearchOptions {
            max_results: 20,
            ..Default::default()
        };
        let res = exact_search(project, eq.clone(), opts)
            .await
            .unwrap_or_default();
        let cnt = res.len();
        // Convert ExactEvidence to Evidence
        for ev in res {
            candidates.push(Evidence {
                source: context_rank::types::RetrievalSource::Exact,
                file: ev.file,
                start_line: Some(ev.line),
                end_line: ev.end_line,
                symbol: None,
                symbol_kind: None,
                text: Some(ev.text),
                score: Some(1.0),
                relation: Some(context_rank::types::EvidenceRelation::Reference),
                authority_score: None,
                final_score: None,
                provenance: Some("rust:exact".into()),
                metadata: None,
            });
        }
        retrievers_used.push(format!("rust-exact:{}", cnt));
        let _ = t.elapsed();
    }

    // V2 candidates: semantic, symbol, graph, test
    // For each symbol query, call V2 symbol lookup
    for sym in &plan.symbol_queries {
        let t = Instant::now();
        let v = providers
            .v2
            .call_json("symbol_lookup", serde_json::json!({ "symbol": sym }))
            .await;
        if let Ok(val) = v {
            if let Some(arr) = val.get("evidence").and_then(|e| e.as_array()) {
                for ev in arr.iter().take(5) {
                    if let Some(file) = ev.get("file").and_then(|f| f.as_str()) {
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Symbol,
                            file: file.to_string(),
                            start_line: ev
                                .get("startLine")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32),
                            end_line: ev.get("endLine").and_then(|v| v.as_u64()).map(|v| v as u32),
                            symbol: ev
                                .get("symbol")
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string()),
                            symbol_kind: ev
                                .get("symbolKind")
                                .and_then(|k| k.as_str())
                                .map(|k| k.to_string()),
                            text: ev
                                .get("text")
                                .and_then(|t| t.as_str())
                                .map(|t| t.to_string()),
                            score: ev.get("score").and_then(|s| s.as_f64()),
                            relation: Some(context_rank::types::EvidenceRelation::Definition),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("v2:symbol".into()),
                            metadata: Some(ev.clone()),
                        });
                    }
                }
            }
        }
        retrievers_used.push(format!("v2-symbol:{}", sym));
        let _ = t.elapsed();
    }

    // Semantic
    for sq in &plan.semantic_queries {
        let t = Instant::now();
        let v = providers
            .v2
            .call_json("context_search", serde_json::json!({ "query": sq }))
            .await;
        if let Ok(val) = v {
            if let Some(arr) = val.get("evidence").and_then(|e| e.as_array()) {
                for ev in arr.iter().take(5) {
                    if let Some(file) = ev.get("file").and_then(|f| f.as_str()) {
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Semantic,
                            file: file.to_string(),
                            start_line: ev
                                .get("startLine")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32),
                            end_line: ev.get("endLine").and_then(|v| v.as_u64()).map(|v| v as u32),
                            symbol: ev
                                .get("symbol")
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string()),
                            symbol_kind: ev
                                .get("symbolKind")
                                .and_then(|k| k.as_str())
                                .map(|k| k.to_string()),
                            text: ev
                                .get("text")
                                .and_then(|t| t.as_str())
                                .map(|t| t.to_string()),
                            score: ev.get("score").and_then(|s| s.as_f64()),
                            relation: Some(context_rank::types::EvidenceRelation::Unknown),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("v2:semantic".into()),
                            metadata: Some(ev.clone()),
                        });
                    }
                }
            }
        }
        retrievers_used.push(format!(
            "v2-semantic:{}",
            sq.chars().take(20).collect::<String>()
        ));
        let _ = t.elapsed();
    }

    // Graph
    for gq in &plan.graph_queries {
        let t = Instant::now();
        let v = providers
            .v2
            .call_json(
                "dependency_trace",
                serde_json::json!({ "symbol": gq.symbol, "direction": gq.direction }),
            )
            .await;
        if let Ok(val) = v {
            if let Some(arr) = val.get("evidence").and_then(|e| e.as_array()) {
                for ev in arr.iter().take(5) {
                    if let Some(file) = ev.get("file").and_then(|f| f.as_str()) {
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Graph,
                            file: file.to_string(),
                            start_line: ev
                                .get("startLine")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32),
                            end_line: ev.get("endLine").and_then(|v| v.as_u64()).map(|v| v as u32),
                            symbol: ev
                                .get("symbol")
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string()),
                            symbol_kind: None,
                            text: ev
                                .get("text")
                                .and_then(|t| t.as_str())
                                .map(|t| t.to_string()),
                            score: Some(0.9),
                            relation: Some(if gq.direction == "callers" {
                                context_rank::types::EvidenceRelation::Caller
                            } else {
                                context_rank::types::EvidenceRelation::Callee
                            }),
                            authority_score: None,
                            final_score: None,
                            provenance: Some(format!("v2:graph:{}", gq.direction)),
                            metadata: Some(ev.clone()),
                        });
                    }
                }
            }
        }
        retrievers_used.push(format!("v2-graph:{}", gq.symbol));
        let _ = t.elapsed();
    }

    // Authority
    let t_auth = Instant::now();
    let scored = apply_authority(candidates, classified.query_type, query);
    let auth_ms = t_auth.elapsed().as_millis();

    // Fuse
    let t_fuse = Instant::now();
    let fused = fuse_evidence(
        scored,
        FuseOptions {
            top_n: max_results,
            query_type: classified.query_type,
            raw_query: query.to_string(),
        },
    );
    let fuse_ms = t_fuse.elapsed().as_millis();

    // Pack
    let t_pack = Instant::now();
    let packed = pack_evidence(
        &fused.ranked,
        query,
        classified.query_type,
        PackOptions {
            budget: budget_tokens,
            max_files: max_results,
        },
    );
    let pack_ms = t_pack.elapsed().as_millis();

    let elapsed_ms = t0.elapsed().as_millis();

    // Token metrics
    let candidate_count = fused.ranked.len() + fused.deduped + fused.collapsed;
    let stats = PipelineStats {
        candidate_count,
        evidence_count: fused.ranked.len(),
        files_returned: packed.files.len(),
        packed_tokens: packed.token_estimate,
        retrievers_used: retrievers_used
            .into_iter()
            .chain(vec![
                format!("authority:{}", auth_ms),
                format!("fuse:{}", fuse_ms),
                format!("pack:{}", pack_ms),
            ])
            .collect(),
        elapsed_ms,
    };

    Ok(ContextResult {
        query: query.to_string(),
        query_type: classified.query_type,
        evidence: fused.ranked,
        packed,
        stats,
    })
}
