use crate::types::{Evidence, QueryType};
use std::collections::{HashMap, HashSet};

/// Count tokens using tiktoken cl100k_base (gpt-4). Fallback to chars/4.
pub fn count_tokens(text: &str) -> usize {
    // Use tiktoken-rs with cl100k_base
    // For R2, we use a simple approximation if tiktoken fails, but try to use it.
    // tiktoken-rs 0.12 uses `r50k_base` etc, but we can use `cl100k_base` via `get_bpe_from_model`
    // For simplicity, use `tiktoken_rs::cl100k_base` if available, else fallback.
    // The crate `tiktoken-rs` 0.12 provides `cl100k_base` function.
    // We use `tiktoken_rs::cl100k_base().unwrap().encode_with_special_tokens(text).len()`
    // But to avoid heavy init each call, we use a once_cell.

    // For R2, we approximate with chars/4 if tiktoken not available quickly.
    // Try to use tiktoken, fallback to chars/4.
    match tiktoken_rs::cl100k_base() {
        Ok(bpe) => bpe.encode_with_special_tokens(text).len(),
        Err(_) => (text.len() as f64 / 4.0).ceil() as usize,
    }
}

#[derive(Debug, Clone)]
pub struct PackOptions {
    pub budget: usize,
    pub max_files: usize,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            budget: 10000,
            max_files: 10,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PackedResult {
    pub markdown: String,
    pub token_estimate: usize,
    pub files: Vec<String>,
}

pub fn pack_evidence(
    ranked: &[Evidence],
    query: &str,
    query_type: QueryType,
    opts: PackOptions,
) -> PackedResult {
    let budget = opts.budget;
    let max_files = opts.max_files;

    let mut by_file: HashMap<String, Vec<Evidence>> = HashMap::new();
    for e in ranked.iter().take(max_files * 2) {
        by_file.entry(e.file.clone()).or_default().push(e.clone());
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("# Evidence Pack — {}", query_type.as_str()));
    lines.push(format!("> Query: {}", query));
    lines.push(String::new());

    let mut total_tokens = count_tokens(&lines.join("\n"));
    let mut files = Vec::new();

    // Deterministic order: sort files by first evidence final_score
    let mut file_order: Vec<(String, Vec<Evidence>)> = by_file.into_iter().collect();
    file_order.sort_by(|a, b| {
        let a_score =
            a.1.iter()
                .map(|e| e.final_score.unwrap_or(0.0))
                .fold(f64::NEG_INFINITY, f64::max);
        let b_score =
            b.1.iter()
                .map(|e| e.final_score.unwrap_or(0.0))
                .fold(f64::NEG_INFINITY, f64::max);
        b_score
            .partial_cmp(&a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for (file, mut items) in file_order.into_iter().take(max_files) {
        items.sort_by_key(|e| e.start_line.unwrap_or(0));
        let ranges = items
            .iter()
            .map(|e| {
                format!(
                    "{}-{}",
                    e.start_line.map(|n| n.to_string()).unwrap_or("?".into()),
                    e.end_line.map(|n| n.to_string()).unwrap_or("?".into())
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let symbols: String = {
            let mut set: HashSet<String> = HashSet::new();
            for e in &items {
                if let Some(s) = &e.symbol {
                    set.insert(s.clone());
                }
            }
            set.into_iter().collect::<Vec<_>>().join(", ")
        };
        let sources: String = {
            let mut set: HashSet<String> = HashSet::new();
            for e in &items {
                set.insert(e.source.as_str().to_string());
            }
            set.into_iter().collect::<Vec<_>>().join("+")
        };
        let header = format!(
            "## {} {} [{}] lines {}",
            file,
            if symbols.is_empty() {
                "".to_string()
            } else {
                format!("({})", symbols)
            },
            sources,
            ranges
        );
        let mut body_lines = vec![header];
        for e in &items {
            let loc = if let Some(s) = e.start_line {
                format!("{}:{}-{}", e.file, s, e.end_line.unwrap_or(s))
            } else {
                e.file.clone()
            };
            let score = format!(
                "score:{:.2} authority:{} final:{:.1}",
                e.score.unwrap_or(0.0),
                e.authority_score.unwrap_or(0),
                e.final_score.unwrap_or(0.0)
            );
            let text = e
                .text
                .as_deref()
                .map(|t| format!(" — {}", t.chars().take(120).collect::<String>()))
                .unwrap_or_default();
            body_lines.push(format!(
                "- {} {} {} ({}){} [{}]",
                loc,
                e.symbol_kind.clone().unwrap_or_default(),
                e.symbol.clone().unwrap_or_default(),
                score,
                text,
                e.provenance
                    .clone()
                    .unwrap_or_else(|| e.source.as_str().to_string())
            ));
        }
        body_lines.push(String::new());
        let chunk = body_lines.join("\n");
        let chunk_tokens = count_tokens(&chunk);
        if total_tokens + chunk_tokens > budget {
            break;
        }
        lines.push(chunk);
        total_tokens += chunk_tokens;
        files.push(file);
    }

    let markdown = lines.join("\n");
    PackedResult {
        markdown,
        token_estimate: total_tokens,
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Evidence, EvidenceRelation, QueryType, RetrievalSource};

    #[test]
    fn respects_budget() {
        let ev = vec![
            Evidence {
                source: RetrievalSource::Exact,
                file: "a.py".into(),
                start_line: Some(1),
                end_line: Some(1),
                symbol: Some("foo".into()),
                symbol_kind: Some("function_definition".into()),
                text: Some("def foo(): pass".into()),
                score: Some(1.0),
                relation: Some(EvidenceRelation::Definition),
                authority_score: Some(10),
                final_score: Some(30.0),
                provenance: Some("test".into()),
                metadata: None,
            };
            20
        ];
        let packed = pack_evidence(
            &ev,
            "test",
            QueryType::Symbol,
            PackOptions {
                budget: 100,
                max_files: 10,
            },
        );
        assert!(packed.token_estimate <= 100);
    }

    #[test]
    fn utf8_text_near_120_does_not_panic() {
        // 118 ASCII chars + 5 emoji (4 bytes each). The old byte-slice would index
        // byte 120 inside an emoji and panic; char iteration must stay valid.
        let text = format!("{}{}", "x".repeat(118), "🎉".repeat(5));
        let ev = Evidence {
            source: RetrievalSource::Exact,
            file: "x.rs".into(),
            start_line: Some(1),
            end_line: Some(1),
            symbol: None,
            symbol_kind: None,
            text: Some(text),
            score: Some(1.0),
            relation: None,
            authority_score: None,
            final_score: Some(1.0),
            provenance: None,
            metadata: None,
        };
        let packed = pack_evidence(
            &[ev],
            "test",
            QueryType::Symbol,
            PackOptions {
                budget: 10000,
                max_files: 10,
            },
        );
        assert!(packed.markdown.contains('🎉'));
        // String is valid UTF-8 by construction; the real assertion is that we did not panic.
    }
}
