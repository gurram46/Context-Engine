use context_index::structural::store as structural_store;
use context_index::{exact_search, ExactSearchOptions, ProjectIndex};
use context_rank::types::Evidence;
use context_rank::{
    apply_authority, build_retrieval_plan, classify_query, fuse_evidence, pack_evidence,
    FuseOptions, PackOptions, QueryType,
};
use std::path::Path;
use std::time::Instant;

/// Retrieval providers — for R2, V2/OCI provides semantic/symbol/graph candidates via raw candidate provider.
pub struct Providers {
    pub candidate: std::sync::Arc<crate::candidate::CandidateProvider>,
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

    // Rust exact — larger budget for bundle-like queries that hit many docs
    for eq in &plan.exact_queries {
        let t = Instant::now();
        let opts = ExactSearchOptions {
            max_results: 50,
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

    // Native Rust structural symbol candidates (R3) — replaces OCI implementation_lookup
    for sym in &plan.symbol_queries {
        let t = Instant::now();
        let mut added = 0usize;
        // Open DB per query; cheap
        if let Ok(conn) = structural_store::open_db(&project.root) {
            if let Ok(defs) = structural_store::find_definitions(&conn, sym) {
                for def in defs.iter().take(5) {
                    let text = load_symbol_snippet(&project.root, def);
                    candidates.push(Evidence {
                        source: context_rank::types::RetrievalSource::Symbol,
                        file: def.file.clone(),
                        start_line: Some(def.start_line),
                        end_line: Some(def.end_line),
                        symbol: Some(def.name.clone()),
                        symbol_kind: Some(def.kind.as_str().to_string()),
                        text: Some(text),
                        score: Some(1.0),
                        relation: Some(context_rank::types::EvidenceRelation::Definition),
                        authority_score: None,
                        final_score: None,
                        provenance: Some("rust:symbol".into()),
                        metadata: None,
                    });
                    added += 1;
                }
            }
            // Fallback: if no exact, try prefix
            if added == 0 {
                if let Ok(pref) = structural_store::find_symbol_prefix(&conn, sym) {
                    for def in pref.iter().take(5) {
                        let text = load_symbol_snippet(&project.root, def);
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Symbol,
                            file: def.file.clone(),
                            start_line: Some(def.start_line),
                            end_line: Some(def.end_line),
                            symbol: Some(def.name.clone()),
                            symbol_kind: Some(def.kind.as_str().to_string()),
                            text: Some(text),
                            score: Some(0.9),
                            relation: Some(context_rank::types::EvidenceRelation::Definition),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("rust:symbol:prefix".into()),
                            metadata: None,
                        });
                        added += 1;
                    }
                }
            }
        }
        // Shadow OCI for debug comparison (not used in ranking) — kept behind feature if needed
        // For R3, OCI symbol is not authoritative; we intentionally don't call it in prod path.
        retrievers_used.push(format!("rust-symbol:{}:{}", sym, added));
        let _ = t.elapsed();
    }

    // Semantic (raw peek/search)
    for sq in &plan.semantic_queries {
        let t = Instant::now();
        let res = providers.candidate.semantic_candidates(sq, 5).await;
        if let Ok(arr) = res {
            for ev in arr.iter().take(5) {
                if let Some(file) = ev.get("file").and_then(|f| f.as_str()) {
                    candidates.push(Evidence {
                        source: context_rank::types::RetrievalSource::Semantic,
                        file: file.to_string(),
                        start_line: ev
                            .get("startLine")
                            .or_else(|| ev.get("start_line"))
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32),
                        end_line: ev
                            .get("endLine")
                            .or_else(|| ev.get("end_line"))
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32),
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
                        provenance: Some("oci:semantic".into()),
                        metadata: Some(ev.clone()),
                    });
                }
            }
        }
        retrievers_used.push(format!(
            "oci-semantic:{}",
            sq.chars().take(20).collect::<String>()
        ));
        let _ = t.elapsed();
    }

    // Native Rust structural graph + exact fallback (R3)
    for gq in &plan.graph_queries {
        let t = Instant::now();
        let mut added = 0usize;
        if let Ok(conn) = structural_store::open_db(&project.root) {
            if gq.direction == "callers" || gq.direction == "both" {
                if let Ok(callers) = structural_store::find_callers(&conn, &gq.symbol) {
                    for edge in callers.iter().take(5) {
                        // Caller evidence: file where call occurs
                        let caller_sym = edge.resolved_symbol_id.as_deref().and_then(|id| {
                            structural_store::find_symbol_by_id(&conn, id)
                                .ok()
                                .flatten()
                        });
                        // Try to load text for caller context
                        let text = caller_sym
                            .as_ref()
                            .map(|s| load_symbol_snippet(&project.root, s))
                            .or_else(|| {
                                // fallback to call site snippet
                                load_file_snippet(&project.root, &edge.file, edge.line)
                            });
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Graph,
                            file: edge.file.clone(),
                            start_line: Some(edge.line),
                            end_line: Some(edge.line),
                            symbol: Some(edge.callee_name.clone()),
                            symbol_kind: caller_sym.map(|s| s.kind.as_str().to_string()),
                            text,
                            score: Some(match edge.confidence {
                                context_index::structural::types::CallConfidence::Resolved => 1.0,
                                context_index::structural::types::CallConfidence::Probable => 0.8,
                                context_index::structural::types::CallConfidence::Unresolved => 0.6,
                            }),
                            relation: Some(context_rank::types::EvidenceRelation::Caller),
                            authority_score: None,
                            final_score: None,
                            provenance: Some(format!(
                                "rust:graph:{}:{}",
                                gq.direction,
                                edge.confidence.as_str()
                            )),
                            metadata: None,
                        });
                        added += 1;
                    }
                }
                // Exact fallback for callers is already covered by plan.exact_queries (Reference)
                // But we add explicit reference fallback if graph returned nothing
                if added == 0 {
                    // Use exact reference search as fallback (already in exact candidates, but we tag)
                    // No extra work: exact already provides caller references
                }
            }
            if gq.direction == "callees" || gq.direction == "both" {
                if let Ok(callees) = structural_store::find_callees(&conn, &gq.symbol) {
                    for edge in callees.iter().take(5) {
                        // Resolve callee symbol to its definition location
                        let callee_sym = edge
                            .resolved_symbol_id
                            .as_deref()
                            .and_then(|id| {
                                structural_store::find_symbol_by_id(&conn, id)
                                    .ok()
                                    .flatten()
                            })
                            .or_else(|| {
                                structural_store::find_definitions(&conn, &edge.callee_name)
                                    .ok()
                                    .and_then(|v| v.into_iter().next())
                            });
                        if let Some(sym) = callee_sym {
                            let text = load_symbol_snippet(&project.root, &sym);
                            candidates.push(Evidence {
                                source: context_rank::types::RetrievalSource::Graph,
                                file: sym.file.clone(),
                                start_line: Some(sym.start_line),
                                end_line: Some(sym.end_line),
                                symbol: Some(sym.name.clone()),
                                symbol_kind: Some(sym.kind.as_str().to_string()),
                                text: Some(text),
                                score: Some(match edge.confidence {
                                    context_index::structural::types::CallConfidence::Resolved => 1.0,
                                    context_index::structural::types::CallConfidence::Probable => 0.8,
                                    context_index::structural::types::CallConfidence::Unresolved => 0.6,
                                }),
                                relation: Some(context_rank::types::EvidenceRelation::Callee),
                                authority_score: None,
                                final_score: None,
                                provenance: Some(format!("rust:graph:callees:{}", edge.confidence.as_str())),
                                metadata: None,
                            });
                            added += 1;
                        } else {
                            // Unresolved callee: still provide caller context
                            candidates.push(Evidence {
                                source: context_rank::types::RetrievalSource::Graph,
                                file: edge.file.clone(),
                                start_line: Some(edge.line),
                                end_line: Some(edge.line),
                                symbol: Some(edge.callee_name.clone()),
                                symbol_kind: None,
                                text: load_file_snippet(&project.root, &edge.file, edge.line),
                                score: Some(0.6),
                                relation: Some(context_rank::types::EvidenceRelation::Callee),
                                authority_score: None,
                                final_score: None,
                                provenance: Some("rust:graph:callees:unresolved".into()),
                                metadata: None,
                            });
                            added += 1;
                        }
                    }
                }
            }
        }
        retrievers_used.push(format!("rust-graph:{}:{}", gq.symbol, added));
        let _ = t.elapsed();
    }

    // Native Rust structural test lookup + exact fallback (R3)
    for tq in &plan.test_queries {
        let t = Instant::now();
        let mut added = 0usize;
        if let Ok(conn) = structural_store::open_db(&project.root) {
            if let Ok(tests) = structural_store::find_tests_related(&conn, tq) {
                for sym in tests.iter().take(5) {
                    let text = load_symbol_snippet(&project.root, sym);
                    candidates.push(Evidence {
                        source: context_rank::types::RetrievalSource::Test,
                        file: sym.file.clone(),
                        start_line: Some(sym.start_line),
                        end_line: Some(sym.end_line),
                        symbol: Some(sym.name.clone()),
                        symbol_kind: Some(sym.kind.as_str().to_string()),
                        text: Some(text),
                        score: Some(1.0),
                        relation: Some(context_rank::types::EvidenceRelation::Test),
                        authority_score: None,
                        final_score: None,
                        provenance: Some("rust:test".into()),
                        metadata: None,
                    });
                    added += 1;
                }
            }
            // Also consider file-kind test files via discovery: find tests by filename pattern via ProjectIndex
            if added == 0 {
                // Use ProjectIndex to find test files matching query
                let lower = tq.to_lowercase();
                for f in project
                    .files
                    .iter()
                    .filter(|r| r.kind == context_index::FileKind::Test)
                {
                    if f.relative_path.to_lowercase().contains(&lower)
                        || f.relative_path
                            .to_lowercase()
                            .contains(&format!("test_{}", lower))
                    {
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Test,
                            file: f.relative_path.clone(),
                            start_line: Some(1),
                            end_line: Some(1),
                            symbol: Some(tq.clone()),
                            symbol_kind: None,
                            text: Some(format!("Test file: {}", f.relative_path)),
                            score: Some(0.8),
                            relation: Some(context_rank::types::EvidenceRelation::Test),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("rust:test:file".into()),
                            metadata: None,
                        });
                        added += 1;
                        if added >= 5 {
                            break;
                        }
                    }
                }
            }
        }
        // Keep OCI test as optional shadow for semantic fuzzy until R4; not used for ranking in R3 unless native empty
        // We could call OCI semantic for fuzzy test intent as fallback, but per R3 spec semantic OCI remains optional
        // For now, if native added ==0, try OCI test candidates as semantic fallback (low score)
        if added == 0 {
            if let Ok(arr) = providers.candidate.test_candidates(tq).await {
                for ev in arr.iter().take(2) {
                    if let Some(file) = ev.get("file").and_then(|f| f.as_str()) {
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Test,
                            file: file.to_string(),
                            start_line: ev
                                .get("startLine")
                                .or_else(|| ev.get("start_line"))
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32),
                            end_line: ev
                                .get("endLine")
                                .or_else(|| ev.get("end_line"))
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32),
                            symbol: ev
                                .get("symbol")
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string()),
                            symbol_kind: ev
                                .get("symbolKind")
                                .or_else(|| ev.get("symbol_kind"))
                                .and_then(|k| k.as_str())
                                .map(|k| k.to_string()),
                            text: ev
                                .get("text")
                                .and_then(|t| t.as_str())
                                .map(|t| t.to_string()),
                            score: ev.get("score").and_then(|s| s.as_f64()),
                            relation: Some(context_rank::types::EvidenceRelation::Test),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("oci:test:fallback".into()),
                            metadata: Some(ev.clone()),
                        });
                        added += 1;
                    }
                }
            }
        }
        retrievers_used.push(format!("rust-test:{}:{}", tq, added));
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

fn load_symbol_snippet(root: &Path, sym: &context_index::structural::types::Symbol) -> String {
    let path = root.join(&sym.file);
    if let Ok(content) = std::fs::read_to_string(&path) {
        let bytes = content.as_bytes();
        let start = sym.start_byte.min(bytes.len());
        let end = sym.end_byte.min(bytes.len());
        if end > start {
            let slice = &content[start..end];
            // Take first 400 chars, single line-ish
            let txt: String = slice.chars().take(400).collect();
            return txt.trim().to_string();
        }
        // fallback to line
        if let Some(line) = content
            .lines()
            .nth((sym.start_line as usize).saturating_sub(1))
        {
            return line.chars().take(400).collect();
        }
    }
    format!("{} {}", sym.kind.as_str(), sym.qualified_name)
}

fn load_file_snippet(root: &Path, file: &str, line: u32) -> Option<String> {
    let path = root.join(file);
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Some(l) = content.lines().nth((line as usize).saturating_sub(1)) {
            return Some(l.chars().take(400).collect());
        }
    }
    None
}
