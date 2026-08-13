use crate::classify::classify_query;
use crate::identifiers::extract_identifiers;
use crate::types::{ClassifiedQuery, QueryType};
use context_index::ExactQuery;

#[derive(Debug, Clone)]
pub struct RetrievalPlan {
    pub classified: ClassifiedQuery,
    pub exact_queries: Vec<ExactQuery>,
    pub symbol_queries: Vec<String>,
    pub semantic_queries: Vec<String>,
    pub graph_queries: Vec<GraphRequest>,
    pub test_queries: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GraphRequest {
    pub symbol: String,
    pub direction: String, // callers|callees
}

pub fn build_retrieval_plan(raw: &str) -> RetrievalPlan {
    let classified = classify_query(raw);
    let mut exact_queries = Vec::new();
    let mut symbol_queries = Vec::new();
    let mut semantic_queries = Vec::new();
    let mut graph_queries = Vec::new();
    let mut test_queries = Vec::new();

    let ids = extract_identifiers(raw);

    match classified.query_type {
        QueryType::Exact => {
            // Single filename/path or quoted literal
            let q = raw.trim().trim_matches(|c| c == '"' || c == '\'');
            if q.contains('/') || q.contains('.') {
                if q.contains('/') {
                    exact_queries.push(ExactQuery::Path(q.to_string()));
                } else {
                    exact_queries.push(ExactQuery::FileName(q.to_string()));
                }
            } else {
                exact_queries.push(ExactQuery::Literal(q.to_string()));
            }
        }
        QueryType::Symbol => {
            symbol_queries = ids.clone();
            // Also exact verify for symbol
            if let Some(first) = ids.first() {
                exact_queries.push(ExactQuery::Literal(first.clone()));
            }
        }
        QueryType::Dependency => {
            let id = ids
                .first()
                .cloned()
                .unwrap_or_else(|| raw.split_whitespace().last().unwrap_or("").to_string());
            symbol_queries.push(id.clone());
            graph_queries.push(GraphRequest {
                symbol: id.clone(),
                direction: "callers".to_string(),
            });
            if raw.to_lowercase().contains("callees") || raw.to_lowercase().contains("what does") {
                graph_queries.push(GraphRequest {
                    symbol: id.clone(),
                    direction: "callees".to_string(),
                });
            }
            exact_queries.push(ExactQuery::Literal(id.clone()));
        }
        QueryType::Test => {
            semantic_queries.push(raw.to_string());
            for id in ids.iter().take(2) {
                test_queries.push(id.clone());
                exact_queries.push(ExactQuery::Literal(id.clone()));
                // Generic test file variant: test_<id>
                let snake = to_snake_case(id);
                if snake != *id {
                    exact_queries.push(ExactQuery::Literal(format!("test_{}", snake)));
                } else {
                    exact_queries.push(ExactQuery::Literal(format!("test_{}", id.to_lowercase())));
                }
            }
        }
        QueryType::Conceptual => {
            semantic_queries.push(raw.to_string());
            for id in ids.iter().take(2) {
                if id.len() >= 4 {
                    exact_queries.push(ExactQuery::Literal(id.clone()));
                }
            }
        }
        QueryType::Mixed => {
            semantic_queries.push(raw.to_string());
            symbol_queries = ids.clone();
            for id in ids.iter().take(3) {
                exact_queries.push(ExactQuery::Literal(id.clone()));
            }
            // Path-like
            if let Some(m) = regex::Regex::new(r"[\w.-]+\.(py|ts|js|md)\b")
                .unwrap()
                .find(raw)
            {
                exact_queries.push(ExactQuery::Path(m.as_str().to_string()));
            }
        }
    }

    RetrievalPlan {
        classified,
        exact_queries,
        symbol_queries,
        semantic_queries,
        graph_queries,
        test_queries,
    }
}

fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_flow_mixed() {
        let p = build_retrieval_plan(
            "Trace the Bundle Generation Flow context bundle --no-ai to .context/context_for_ai.md",
        );
        assert_eq!(p.classified.query_type, QueryType::Mixed);
        assert!(p.symbol_queries.contains(&"bundle".to_string()));
    }
    #[test]
    fn count_tokens_symbol() {
        let p = build_retrieval_plan("Where is count_tokens implemented?");
        assert_eq!(p.classified.query_type, QueryType::Symbol);
        assert!(p.symbol_queries.contains(&"count_tokens".to_string()));
    }
}
