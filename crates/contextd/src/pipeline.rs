use context_index::structural::store as structural_store;
use context_index::{exact_search, ExactSearchOptions, ProjectIndex};
use context_rank::types::Evidence;
use context_rank::{
    apply_authority, build_retrieval_plan, classify_query, fuse_evidence, pack_evidence,
    FuseOptions, PackOptions, QueryType,
};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Retrieval providers — R5 production uses no V2/OCI providers.
/// Kept as empty struct for API compatibility; legacy CandidateProvider is LEGACY only.
pub struct Providers {}

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
    pub exact_ms: u128,
    pub structural_ms: u128,
    pub bm25_ms: u128,
    pub semantic_ms: u128,
    pub rank_ms: u128,
    pub pack_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSufficiency {
    Insufficient,
    Adequate,
    Strong,
}

/// Determine if we have sufficient evidence to skip heavier retrievers.
/// Conservative deterministic signals — no invented confidence.
fn sufficiency(
    query_type: QueryType,
    candidates: &[Evidence],
    raw_query: &str,
) -> EvidenceSufficiency {
    // Helper to check definition presence
    let has_strong_symbol = candidates.iter().any(|e| {
        e.source == context_rank::types::RetrievalSource::Symbol
            && e.relation == Some(context_rank::types::EvidenceRelation::Definition)
            && (e.file.to_lowercase().contains("src/")
                || e.file.to_lowercase().contains("crates/")
                || e.file.to_lowercase().contains("backend/"))
    });
    let has_resolved_graph = candidates.iter().any(|e| {
        e.source == context_rank::types::RetrievalSource::Graph
            && e.relation == Some(context_rank::types::EvidenceRelation::Caller)
            && e.provenance
                .as_deref()
                .map(|p| p.contains("resolved"))
                .unwrap_or(false)
    });
    let has_exact = candidates
        .iter()
        .any(|e| e.source == context_rank::types::RetrievalSource::Exact);

    match query_type {
        QueryType::Symbol => {
            // SYMBOL with exact definition + symbol definition + active source + high authority? Simplified: if we have symbol definition, we are Strong
            if has_strong_symbol && has_exact {
                return EvidenceSufficiency::Strong;
            }
            if has_strong_symbol {
                return EvidenceSufficiency::Adequate;
            }
            EvidenceSufficiency::Insufficient
        }
        QueryType::Dependency => {
            if has_resolved_graph && has_exact {
                return EvidenceSufficiency::Strong;
            }
            if has_resolved_graph {
                return EvidenceSufficiency::Adequate;
            }
            EvidenceSufficiency::Insufficient
        }
        QueryType::Test => {
            // Test queries need at least some test evidence
            let has_test = candidates
                .iter()
                .any(|e| e.source == context_rank::types::RetrievalSource::Test);
            if has_test && candidates.len() >= 3 {
                return EvidenceSufficiency::Strong;
            }
            if has_test {
                return EvidenceSufficiency::Adequate;
            }
            EvidenceSufficiency::Insufficient
        }
        QueryType::Exact => EvidenceSufficiency::Strong, // exact already sufficient
        QueryType::Conceptual => {
            // Conceptual needs semantic; never strong without BM25/semantic
            if !candidates.is_empty() && raw_query.to_lowercase().contains("test") {
                // placeholder
            }
            EvidenceSufficiency::Insufficient
        }
        QueryType::Mixed => {
            if has_strong_symbol && has_exact && has_resolved_graph {
                return EvidenceSufficiency::Adequate;
            }
            EvidenceSufficiency::Insufficient
        }
    }
}

/// RRF fusion for BM25 + vector.
/// Do NOT naively add BM25 score + cosine (scales differ). Use rank normalization.
fn fuse_rrf(
    bm25: Vec<(Evidence, usize, f64)>,
    vector: Vec<(Evidence, usize, f64)>,
    k: usize,
) -> Vec<Evidence> {
    let k = k as f64;
    let mut map: HashMap<String, (Evidence, f64)> = HashMap::new(); // key = file::chunk_id or file::line
    for (ev, rank, _score) in bm25 {
        let key = format!(
            "{}:{}:{:?}",
            ev.file,
            ev.symbol.clone().unwrap_or_default(),
            ev.start_line.unwrap_or(0)
        );
        let rrf = 1.0 / (k + rank as f64);
        let entry = map.entry(key.clone()).or_insert_with(|| (ev.clone(), 0.0));
        entry.1 += rrf;
        // Keep highest raw score provenance
        if ev.score.unwrap_or(0.0) > entry.0.score.unwrap_or(0.0) {
            entry.0.score = ev.score;
        }
    }
    for (ev, rank, _score) in vector {
        let key = format!(
            "{}:{}:{:?}",
            ev.file,
            ev.symbol.clone().unwrap_or_default(),
            ev.start_line.unwrap_or(0)
        );
        let rrf = 1.0 / (k + rank as f64);
        let entry = map.entry(key.clone()).or_insert_with(|| (ev.clone(), 0.0));
        entry.1 += rrf;
        // Merge provenance to indicate fused
        if let Some(existing) = map.get_mut(&key) {
            if ev.provenance.as_deref() == Some("rust:semantic") {
                existing.0.provenance = Some("rust:semantic+bm25".into());
            }
        }
    }
    // Convert to Evidence with RRF as score, preserve provenance
    let mut fused: Vec<Evidence> = map
        .into_values()
        .map(|(mut ev, rrf)| {
            ev.score = Some(rrf);
            ev.provenance = Some(match ev.provenance.as_deref() {
                Some(p) if p.contains("bm25") && p.contains("semantic") => {
                    "rust:bm25+semantic".into()
                }
                Some(p) => p.to_string(),
                None => "rust:bm25+semantic".into(),
            });
            // Source remains Bm25 or Semantic, but for fused we keep original source; authority will handle
            ev
        })
        .collect();
    // Sort by RRF desc
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fused
}

/// Main Rust retrieval pipeline for R4.
/// 1. classify, 2. plan, 3. Rust exact, 4. Rust structural, 5. BM25 + vector (fused), 6. authority, 7. fuse, 8. pack.
#[allow(unused_assignments, unused_variables, clippy::too_many_lines)]
pub async fn retrieve_context(
    query: &str,
    project: &ProjectIndex,
    _providers: &Providers,
    budget_tokens: usize,
    max_results: usize,
) -> Result<ContextResult, anyhow::Error> {
    let t0 = Instant::now();
    let classified = classify_query(query);
    let plan = build_retrieval_plan(query);

    let mut candidates: Vec<Evidence> = Vec::new();
    let mut retrievers_used = Vec::new();

    // Rust exact
    let t_exact = Instant::now();
    for eq in &plan.exact_queries {
        let opts = ExactSearchOptions {
            max_results: 50,
            ..Default::default()
        };
        let res = match exact_search(project, eq.clone(), opts).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, query = %eq.as_str(), "exact_search failed");
                Vec::new()
            }
        };
        let cnt = res.len();
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
    }
    let exact_ms = t_exact.elapsed().as_millis();

    // Native Rust structural symbol candidates (R3)
    let t_struct = Instant::now();
    for sym in &plan.symbol_queries {
        let mut added = 0usize;
        let conn = structural_store::open_db_async(project.root.clone()).await?;
        if let Ok(defs) = structural_store::find_definitions(&conn, sym) {
            for def in defs.iter().take(5) {
                let text = load_symbol_snippet(&project.root, def).await;
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
        if added == 0 {
            if let Ok(pref) = structural_store::find_symbol_prefix(&conn, sym) {
                for def in pref.iter().take(5) {
                    let text = load_symbol_snippet(&project.root, def).await;
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
        retrievers_used.push(format!("rust-symbol:{}:{}", sym, added));
    }

    // Native Rust structural graph
    for gq in &plan.graph_queries {
        let mut added = 0usize;
        let conn = structural_store::open_db_async(project.root.clone()).await?;
        if gq.direction == "callers" || gq.direction == "both" {
            if let Ok(callers) = structural_store::find_callers(&conn, &gq.symbol) {
                for edge in callers.iter().take(5) {
                    let caller_sym = if edge.caller_symbol_id.is_empty() {
                        None
                    } else {
                        structural_store::find_symbol_by_id(&conn, &edge.caller_symbol_id)
                            .ok()
                            .flatten()
                    };
                    let text = if let Some(s) = caller_sym.as_ref() {
                        Some(load_symbol_snippet(&project.root, s).await)
                    } else {
                        load_file_snippet(&project.root, &edge.file, edge.line).await
                    };
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
        }
        if gq.direction == "callees" || gq.direction == "both" {
            if let Ok(callees) = structural_store::find_callees(&conn, &gq.symbol) {
                for edge in callees.iter().take(5) {
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
                        let text = load_symbol_snippet(&project.root, &sym).await;
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
                            provenance: Some(format!(
                                "rust:graph:callees:{}",
                                edge.confidence.as_str()
                            )),
                            metadata: None,
                        });
                        added += 1;
                    } else {
                        candidates.push(Evidence {
                            source: context_rank::types::RetrievalSource::Graph,
                            file: edge.file.clone(),
                            start_line: Some(edge.line),
                            end_line: Some(edge.line),
                            symbol: Some(edge.callee_name.clone()),
                            symbol_kind: None,
                            text: load_file_snippet(&project.root, &edge.file, edge.line).await,
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
        retrievers_used.push(format!("rust-graph:{}:{}", gq.symbol, added));
    }

    // Native Rust structural test lookup
    for tq in &plan.test_queries {
        let mut added = 0usize;
        let conn = structural_store::open_db_async(project.root.clone()).await?;
        if let Ok(tests) = structural_store::find_tests_related(&conn, tq) {
            for sym in tests.iter().take(5) {
                let text = load_symbol_snippet(&project.root, sym).await;
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
        if added == 0 {
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
        retrievers_used.push(format!("rust-test:{}:{}", tq, added));
    }
    let structural_ms = t_struct.elapsed().as_millis();

    // Determine sufficiency before heavy retrieval
    let suff = sufficiency(classified.query_type, &candidates, query);
    // For R4, BM25+semantic for CONCEPTUAL, MIXED, TEST; for others only if insufficient
    let run_bm25 = match classified.query_type {
        QueryType::Conceptual | QueryType::Mixed | QueryType::Test => true,
        QueryType::Symbol | QueryType::Dependency if suff == EvidenceSufficiency::Insufficient => {
            true
        }
        QueryType::Exact => false,
        _ => suff == EvidenceSufficiency::Insufficient,
    };
    let run_semantic = match classified.query_type {
        QueryType::Conceptual | QueryType::Mixed => true,
        QueryType::Test => false, // semantic only if useful for test — we skip for now
        QueryType::Symbol if suff == EvidenceSufficiency::Insufficient => true,
        _ => suff == EvidenceSufficiency::Insufficient && run_bm25,
    };

    let mut bm25_candidates: Vec<(Evidence, usize, f64)> = Vec::new();
    let mut vector_candidates: Vec<(Evidence, usize, f64)> = Vec::new();
    let mut bm25_ms: u128 = 0;
    let mut semantic_ms: u128 = 0;

    // BM25 native — for SYMBOL/DEPENDENCY insufficient, fallback to raw query if no semantic_queries
    if run_bm25 {
        let t_bm25 = Instant::now();
        let conn = structural_store::open_db_async(project.root.clone()).await?;
        let bm25_queries: Vec<String> = if !plan.semantic_queries.is_empty() {
            plan.semantic_queries.clone()
        } else if classified.query_type == QueryType::Symbol
            || classified.query_type == QueryType::Dependency
        {
            vec![query.to_string()]
        } else {
            vec![]
        };
        for sq in &bm25_queries {
            match context_index::bm25::search_bm25(&conn, sq, 10) {
                Ok(results) => {
                    for (rank, bm) in results.into_iter().enumerate() {
                        let text = load_chunk_snippet(&project.root, &bm).await;
                        let ev = Evidence {
                            source: context_rank::types::RetrievalSource::Bm25,
                            file: bm.file.clone(),
                            start_line: Some(bm.start_line),
                            end_line: Some(bm.end_line),
                            symbol: bm.symbol.clone(),
                            symbol_kind: None,
                            text: Some(text),
                            score: Some(bm.score),
                            relation: Some(context_rank::types::EvidenceRelation::Unknown),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("rust:bm25".into()),
                            metadata: None,
                        };
                        bm25_candidates.push((ev, rank + 1, bm.score));
                    }
                    retrievers_used.push(format!(
                        "rust-bm25:{}:{}",
                        sq.chars().take(15).collect::<String>(),
                        bm25_candidates.len()
                    ));
                    break; // only first semantic query for BM25
                }
                Err(e) => {
                    tracing::debug!(error=%e, "bm25 search failed");
                    retrievers_used.push("rust-bm25:0".into());
                }
            }
        }
        // Also BM25 for test queries if needed
        if bm25_candidates.is_empty() && !plan.test_queries.is_empty() {
            for tq in &plan.test_queries {
                if let Ok(res) = context_index::bm25::search_bm25(&conn, tq, 5) {
                    for (rank, bm) in res.into_iter().enumerate() {
                        let text = load_chunk_snippet(&project.root, &bm).await;
                        let ev = Evidence {
                            source: context_rank::types::RetrievalSource::Bm25,
                            file: bm.file.clone(),
                            start_line: Some(bm.start_line),
                            end_line: Some(bm.end_line),
                            symbol: bm.symbol.clone(),
                            symbol_kind: None,
                            text: Some(text),
                            score: Some(bm.score),
                            relation: Some(context_rank::types::EvidenceRelation::Test),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("rust:bm25".into()),
                            metadata: None,
                        };
                        bm25_candidates.push((ev, rank + 1, bm.score));
                    }
                    if !bm25_candidates.is_empty() {
                        break;
                    }
                }
            }
        }
        bm25_ms = t_bm25.elapsed().as_millis();
    } else {
        bm25_ms = 0;
        retrievers_used.push("rust-bm25:skipped".into());
    }

    // Native vector retrieval — REAL semantic only, FakeEmbedder is test-only
    if run_semantic {
        let t_sem = Instant::now();
        // Production uses genuine Ollama embedding; Fake is test-only and never used here
        // Selected winner from real benchmark: all-minilm (384d, Apache-2.0) beats nomic on R@1/MRR with lower memory/disk
        let embedder: std::sync::Arc<dyn context_index::embed::Embedder> = {
            let model =
                std::env::var("CONTEXTD_EMBED_MODEL").unwrap_or_else(|_| "all-minilm".to_string());
            if model == "nomic-embed-text" {
                std::sync::Arc::new(context_index::embed::OllamaEmbedder::nomic())
            } else if model == "all-minilm" {
                std::sync::Arc::new(context_index::embed::OllamaEmbedder::with_model(
                    "all-minilm",
                    384,
                ))
            } else {
                std::sync::Arc::new(context_index::embed::OllamaEmbedder::with_model(
                    &model, 768,
                ))
            }
        };
        let fp = embedder.fingerprint();
        // Check model change invalidation
        let conn = structural_store::open_db_async(project.root.clone()).await?;
        let _ = context_index::vector::invalidate_stale_model(&conn, &fp);
        let semantic_queries: Vec<String> = if !plan.semantic_queries.is_empty() {
            plan.semantic_queries.clone()
        } else if classified.query_type == QueryType::Symbol
            && suff == EvidenceSufficiency::Insufficient
        {
            vec![query.to_string()]
        } else {
            vec![]
        };
        for sq in &semantic_queries {
            let q = sq.clone();
            // Embed query without holding DB connection (Connection is !Send)
            let qvec = {
                let cached = context_index::embed::QUERY_CACHE.get(&fp, &q).await;
                if let Some(v) = cached {
                    v
                } else {
                    match embedder.embed_query(&q).await {
                        Ok(v) => {
                            context_index::embed::QUERY_CACHE
                                .insert(&fp, &q, v.clone())
                                .await;
                            v
                        }
                        Err(e) => {
                            tracing::debug!(error=%e, "query embed failed");
                            retrievers_used.push("rust-semantic:0".into());
                            continue;
                        }
                    }
                }
            };
            // Now open DB for brute search (moved off async executor thread)
            let conn = structural_store::open_db_async(project.root.clone()).await?;
            let cnt = context_index::vector::count_vectors(&conn, &fp).unwrap_or(0);
            if cnt == 0 {
                tracing::debug!("no vectors for model {}, skipping semantic", fp.model_id);
                retrievers_used.push(format!("rust-semantic:0:{}", fp.model_id));
                continue;
            }
            match context_index::vector::search_brute(&conn, &qvec, &fp, 10) {
                Ok(results) => {
                    for (rank, vc) in results.into_iter().enumerate() {
                        let text = load_file_snippet(&project.root, &vc.file, vc.start_line)
                            .await
                            .unwrap_or_else(|| {
                                format!("{} {}", vc.file, vc.symbol.clone().unwrap_or_default())
                            });
                        let ev = Evidence {
                            source: context_rank::types::RetrievalSource::Semantic,
                            file: vc.file.clone(),
                            start_line: Some(vc.start_line),
                            end_line: Some(vc.end_line),
                            symbol: vc.symbol.clone(),
                            symbol_kind: None,
                            text: Some(text),
                            score: Some(vc.score),
                            relation: Some(context_rank::types::EvidenceRelation::Unknown),
                            authority_score: None,
                            final_score: None,
                            provenance: Some("rust:semantic".into()),
                            metadata: None,
                        };
                        vector_candidates.push((ev, rank + 1, vc.score));
                    }
                    retrievers_used.push(format!(
                        "rust-semantic:{}:{}",
                        q.chars().take(15).collect::<String>(),
                        vector_candidates.len()
                    ));
                    break;
                }
                Err(e) => {
                    tracing::debug!(error=%e, "vector search failed");
                    retrievers_used.push("rust-semantic:0".into());
                }
            }
        }
        semantic_ms = t_sem.elapsed().as_millis();
    } else {
        retrievers_used.push("rust-semantic:skipped".into());
    }

    // Fuse BM25 + vector via RRF (rank-normalized)
    if !bm25_candidates.is_empty() || !vector_candidates.is_empty() {
        let fused = fuse_rrf(bm25_candidates, vector_candidates, 60);
        // Add fused evidences — high semantic docs must not auto-beat verified definitions, but RRF already rank-normalized
        // Authority will still penalize docs when impl wanted, so we keep.
        for ev in fused {
            candidates.push(ev);
        }
    }

    // Authority
    let t_auth = Instant::now();
    let scored = apply_authority(candidates, classified.query_type, query);
    let rank_ms = t_auth.elapsed().as_millis();

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

    let candidate_count = fused.ranked.len() + fused.deduped + fused.collapsed;
    let stats = PipelineStats {
        candidate_count,
        evidence_count: fused.ranked.len(),
        files_returned: packed.files.len(),
        packed_tokens: packed.token_estimate,
        retrievers_used: retrievers_used
            .into_iter()
            .chain(vec![
                format!("authority:{}", rank_ms),
                format!("fuse:{}", fuse_ms),
                format!("pack:{}", pack_ms),
                format!("exact_ms:{}", exact_ms),
                format!("structural_ms:{}", structural_ms),
                format!("bm25_ms:{}", bm25_ms),
                format!("semantic_ms:{}", semantic_ms),
            ])
            .collect(),
        elapsed_ms,
        exact_ms,
        structural_ms,
        bm25_ms,
        semantic_ms,
        rank_ms,
        pack_ms,
    };

    Ok(ContextResult {
        query: query.to_string(),
        query_type: classified.query_type,
        evidence: fused.ranked,
        packed,
        stats,
    })
}

async fn load_symbol_snippet(
    root: &Path,
    sym: &context_index::structural::types::Symbol,
) -> String {
    let root = root.to_path_buf();
    let sym = sym.clone();
    tokio::task::spawn_blocking(move || {
        let path = root.join(&sym.file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let bytes = content.as_bytes();
            let start = sym.start_byte.min(bytes.len());
            let end = sym.end_byte.min(bytes.len());
            if end > start {
                let slice = &content[start..end];
                let txt: String = slice.chars().take(400).collect();
                return txt.trim().to_string();
            }
            if let Some(line) = content
                .lines()
                .nth((sym.start_line as usize).saturating_sub(1))
            {
                return line.chars().take(400).collect();
            }
        }
        format!("{} {}", sym.kind.as_str(), sym.qualified_name)
    })
    .await
    .unwrap()
}

async fn load_file_snippet(root: &Path, file: &str, line: u32) -> Option<String> {
    let root = root.to_path_buf();
    let file = file.to_string();
    tokio::task::spawn_blocking(move || {
        let path = root.join(&file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(l) = content.lines().nth((line as usize).saturating_sub(1)) {
                return Some(l.chars().take(400).collect());
            }
        }
        None
    })
    .await
    .unwrap()
}

async fn load_chunk_snippet(root: &Path, bm: &context_index::bm25::Bm25Candidate) -> String {
    let root = root.to_path_buf();
    let bm = bm.clone();
    tokio::task::spawn_blocking(move || {
        let path = root.join(&bm.file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = content.lines().collect();
            let start = (bm.start_line as usize).saturating_sub(1);
            let end = (bm.end_line as usize).min(lines.len());
            if start < end {
                let slice = lines[start..end].join("\n");
                return slice.chars().take(600).collect();
            }
            if let Some(l) = lines.get(start) {
                return l.chars().take(400).collect();
            }
        }
        format!("{}:{}", bm.file, bm.start_line)
    })
    .await
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn caller_evidence_uses_caller_not_callee() {
        use context_index::structural::language::Language;
        use context_index::structural::store;
        use context_index::structural::types::{CallConfidence, Symbol, SymbolKind, Visibility};
        let tmp = tempfile::TempDir::new().unwrap();
        let mut conn = store::open_db(tmp.path()).unwrap();
        let caller_sym = Symbol {
            id: "caller_id".into(),
            name: "caller_fn".into(),
            qualified_name: "caller_fn".into(),
            kind: SymbolKind::Function,
            file: "caller.rs".into(),
            language: Language::Rust,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 10,
            visibility: Visibility::Private,
            parent: None,
        };
        let callee_sym = Symbol {
            id: "callee_id".into(),
            name: "target".into(),
            qualified_name: "target".into(),
            kind: SymbolKind::Function,
            file: "callee.rs".into(),
            language: Language::Rust,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 10,
            visibility: Visibility::Private,
            parent: None,
        };
        conn.execute(
            "INSERT INTO files (path, hash, language, size_bytes) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["caller.rs", "hash1", "rust", 10],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO files (path, hash, language, size_bytes) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["callee.rs", "hash2", "rust", 10],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbols (id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![caller_sym.id, caller_sym.name, caller_sym.qualified_name, caller_sym.kind.as_str(), caller_sym.file, "rust", caller_sym.start_line, caller_sym.end_line, caller_sym.start_byte as i64, caller_sym.end_byte as i64, caller_sym.visibility.as_str(), caller_sym.parent],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbols (id, name, qualified_name, kind, file, language, start_line, end_line, start_byte, end_byte, visibility, parent) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![callee_sym.id, callee_sym.name, callee_sym.qualified_name, callee_sym.kind.as_str(), callee_sym.file, "rust", callee_sym.start_line, callee_sym.end_line, callee_sym.start_byte as i64, callee_sym.end_byte as i64, callee_sym.visibility.as_str(), callee_sym.parent],
        )
        .unwrap();
        let edge = context_index::structural::types::CallEdge {
            caller_symbol_id: "caller_id".into(),
            callee_name: "target".into(),
            resolved_symbol_id: Some("callee_id".into()),
            confidence: CallConfidence::Resolved,
            file: "caller.rs".into(),
            line: 2,
        };
        store::insert_call_edges(&mut conn, std::slice::from_ref(&edge)).unwrap();
        let loaded_caller = store::find_symbol_by_id(&conn, &edge.caller_symbol_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            loaded_caller.file, "caller.rs",
            "caller evidence must load caller file, not callee"
        );
        let loaded_callee =
            store::find_symbol_by_id(&conn, edge.resolved_symbol_id.as_deref().unwrap())
                .unwrap()
                .unwrap();
        assert_eq!(loaded_callee.file, "callee.rs");
        assert_ne!(loaded_caller.file, loaded_callee.file);
    }

    #[test]
    fn non_symbol_crates_path_is_not_strong_symbol() {
        let candidates = vec![Evidence {
            source: context_rank::types::RetrievalSource::Exact,
            file: "crates/foo/src/lib.rs".into(),
            start_line: None,
            end_line: None,
            symbol: None,
            symbol_kind: None,
            text: None,
            score: None,
            relation: Some(context_rank::types::EvidenceRelation::Definition),
            authority_score: None,
            final_score: None,
            provenance: None,
            metadata: None,
        }];
        let suff = sufficiency(QueryType::Symbol, &candidates, "Foo");
        assert_eq!(suff, EvidenceSufficiency::Insufficient);
    }
}
