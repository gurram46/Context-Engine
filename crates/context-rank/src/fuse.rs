use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::authority::apply_authority;
use crate::types::{Evidence, QueryType};
use context_index::classify_file;
use std::path::Path;

static WANTS_IMPL_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"implemented|generation|bundle|logic|validation|handler|router|service|enforced|ontology|pipeline|wired|scoring|delivery|isolation").unwrap()
});
static WANTS_DOC_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"documented|explain the architecture|docs|documentation").unwrap()
});

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches("./").to_lowercase()
}

fn overlap(a: &Evidence, b: &Evidence) -> bool {
    if normalize_path(&a.file) != normalize_path(&b.file) {
        return false;
    }
    let (Some(s1), Some(s2)) = (a.start_line, b.start_line) else {
        return false;
    };
    let e1 = a.end_line.unwrap_or(s1);
    let e2 = b.end_line.unwrap_or(s2);
    std::cmp::max(s1, s2) <= std::cmp::min(e1, e2) + 2
}

#[derive(Debug, Clone)]
pub struct FuseOptions {
    pub top_n: usize,
    pub query_type: QueryType,
    pub raw_query: String,
}

#[derive(Debug, Clone)]
pub struct FuseResult {
    pub ranked: Vec<Evidence>,
    pub deduped: usize,
    pub collapsed: usize,
}

pub fn fuse_evidence(evidence: Vec<Evidence>, opts: FuseOptions) -> FuseResult {
    let top_n = opts.top_n;
    if evidence.is_empty() {
        return FuseResult {
            ranked: Vec::new(),
            deduped: 0,
            collapsed: 0,
        };
    }

    // 1. Authority
    let mut scored = apply_authority(evidence, opts.query_type, &opts.raw_query);
    // 2. Sort by final_score desc, then score, then file for determinism (stable)
    scored.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.file.cmp(&b.file))
    });
    // Retain best definition for preservation after collapse (must capture before dedup consumes scored)
    let best_def = scored
        .iter()
        .filter(|e| {
            e.relation == Some(crate::types::EvidenceRelation::Definition)
                && e.source == crate::types::RetrievalSource::Symbol
        })
        .max_by(|a, b| {
            a.final_score
                .partial_cmp(&b.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned();

    // 3. Dedup (deterministic via BTreeMap)
    let mut seen: BTreeMap<String, Evidence> = BTreeMap::new();
    let mut deduped = 0;
    for e in scored {
        let key = format!(
            "{}:{}:{}:{}:{:?}",
            normalize_path(&e.file),
            e.symbol.clone().unwrap_or_default(),
            e.start_line.unwrap_or(0),
            e.end_line.unwrap_or(0),
            e.source
        );
        if let Some(existing) = seen.get(&key) {
            deduped += 1;
            if e.final_score.unwrap_or(0.0) > existing.final_score.unwrap_or(0.0) {
                seen.insert(key, e);
            }
        } else {
            seen.insert(key, e);
        }
    }
    let deduped_list: Vec<Evidence> = seen.into_values().collect();

    // 4. Collapse per file (deterministic via BTreeMap)
    let mut by_file: BTreeMap<String, Vec<Evidence>> = BTreeMap::new();
    for e in deduped_list {
        let f = normalize_path(&e.file);
        by_file.entry(f).or_default().push(e);
    }
    let mut collapsed_list = Vec::new();
    let mut collapsed = 0;
    for (_file, mut list) in by_file {
        list.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut kept: Vec<Evidence> = Vec::new();
        for e in list {
            let has_overlap = kept.iter().any(|k| overlap(k, &e));
            if has_overlap {
                let is_def = e.relation == Some(crate::types::EvidenceRelation::Definition)
                    && e.source == crate::types::RetrievalSource::Symbol;
                let kept_has_def = kept
                    .iter()
                    .any(|k| k.relation == Some(crate::types::EvidenceRelation::Definition));
                if (is_def && !kept_has_def) || kept.len() < 2 {
                    kept.push(e);
                } else {
                    collapsed += 1;
                }
            } else if kept.len() < 4 || e.final_score.unwrap_or(0.0) > 15.0 {
                kept.push(e);
            } else {
                collapsed += 1;
            }
        }
        let final_len = kept.len();
        let final_kept = if kept.len() > 3 {
            kept.into_iter()
                .enumerate()
                .filter(|(i, e)| *i < 3 || e.authority_score.unwrap_or(0) > 10)
                .map(|(_, e)| e)
                .collect::<Vec<_>>()
        } else {
            kept
        };
        collapsed += final_len - final_kept.len();
        collapsed_list.extend(final_kept);
    }

    collapsed_list.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
    });

    // 5. Doc quota
    let lower_query = opts.raw_query.to_lowercase();
    let wants_impl = matches!(
        opts.query_type,
        QueryType::Symbol | QueryType::Dependency | QueryType::Mixed
    ) || (opts.query_type == QueryType::Conceptual
        && WANTS_IMPL_RE.is_match(&lower_query));
    let wants_doc = WANTS_DOC_RE.is_match(&lower_query);
    if wants_impl && !wants_doc {
        let mut doc_count = 0;
        let mut balanced = Vec::new();
        for e in collapsed_list {
            let kind = classify_file(Path::new(&e.file));
            if kind == context_index::FileKind::Doc {
                if doc_count < 2 {
                    balanced.push(e);
                    doc_count += 1;
                }
            } else {
                balanced.push(e);
            }
        }
        collapsed_list = balanced;
        collapsed_list.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.file.cmp(&b.file))
        });
    }

    // 6. Ensure definition survives
    let def_exists = collapsed_list.iter().any(|e| {
        e.relation == Some(crate::types::EvidenceRelation::Definition)
            && e.source == crate::types::RetrievalSource::Symbol
    });
    if !def_exists {
        if let Some(def) = best_def.clone() {
            collapsed_list.push(def);
            collapsed_list.sort_by(|a, b| {
                b.final_score
                    .partial_cmp(&a.final_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.file.cmp(&b.file))
            });
        }
    }

    let mut ranked: Vec<Evidence> = collapsed_list.into_iter().take(top_n).collect();
    let ranked_has_def = ranked.iter().any(|e| {
        e.relation == Some(crate::types::EvidenceRelation::Definition)
            && e.source == crate::types::RetrievalSource::Symbol
    });
    if !ranked_has_def {
        if let Some(def) = best_def {
            if ranked.len() < top_n {
                ranked.push(def);
            } else if !ranked.is_empty() {
                ranked.pop();
                ranked.push(def);
            } else {
                ranked.push(def);
            }
            ranked.sort_by(|a, b| {
                b.final_score
                    .partial_cmp(&a.final_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.file.cmp(&b.file))
            });
        }
    }

    FuseResult {
        ranked,
        deduped,
        collapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Evidence, EvidenceRelation, QueryType, RetrievalSource};

    fn mk_ev(
        file: &str,
        source: RetrievalSource,
        relation: EvidenceRelation,
        symbol: Option<&str>,
        text: &str,
        score: f64,
    ) -> Evidence {
        Evidence {
            source,
            file: file.into(),
            start_line: Some(10),
            end_line: Some(12),
            symbol: symbol.map(|s| s.to_string()),
            symbol_kind: Some("function_definition".into()),
            text: Some(text.into()),
            score: Some(score),
            relation: Some(relation),
            authority_score: None,
            final_score: None,
            provenance: None,
            metadata: None,
        }
    }

    #[test]
    fn definition_preserved_when_top_n_would_drop_it() {
        let query = "Where is foo implemented?";
        // High-scoring exact in backend (should outrank stale def)
        let doc_like = mk_ev(
            "backend/cli.py",
            RetrievalSource::Exact,
            EvidenceRelation::Reference,
            None,
            "cli add_command foo",
            1.0,
        );
        // Stale definition with lower authority
        let stale_def = mk_ev(
            "archive/old.rs",
            RetrievalSource::Symbol,
            EvidenceRelation::Definition,
            Some("foo"),
            "def foo():",
            1.0,
        );
        let opts = FuseOptions {
            top_n: 1,
            query_type: QueryType::Symbol,
            raw_query: query.into(),
        };
        let res = fuse_evidence(vec![doc_like, stale_def], opts);
        assert!(
            res.ranked
                .iter()
                .any(|e| e.relation == Some(EvidenceRelation::Definition)),
            "definition should be preserved even when top_n=1 and doc outranks initially"
        );
    }
}
