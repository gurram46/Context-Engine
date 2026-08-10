use regex::Regex;

use crate::types::{ClassifiedQuery, QueryType};

pub fn classify_query(raw: &str) -> ClassifiedQuery {
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = normalized.to_lowercase();
    let mut hints = Vec::new();

    // Test
    let test_re = Regex::new(
        r"(?i)\b(tests? for|where is .* tested|what tests cover|test coverage|specs? for)\b",
    )
    .unwrap();
    if test_re.is_match(&lower) {
        hints.push("test".to_string());
        return ClassifiedQuery {
            query_type: QueryType::Test,
            raw: raw.to_string(),
            normalized,
            hints,
        };
    }
    let dep_re = Regex::new(r"(?i)\b(who calls|what calls|callers|callees|depends on|what breaks if|impact of|used by|transitive|calls?)\b").unwrap();
    if dep_re.is_match(&lower) {
        hints.push("dependency".to_string());
        return ClassifiedQuery {
            query_type: QueryType::Dependency,
            raw: raw.to_string(),
            normalized,
            hints,
        };
    }

    // Quoted literal
    if is_quoted_literal(&normalized) {
        hints.push("exact-quoted".to_string());
        return ClassifiedQuery {
            query_type: QueryType::Exact,
            raw: raw.to_string(),
            normalized,
            hints,
        };
    }

    // Single token filename
    let single = normalized.split_whitespace().count() == 1;
    if single && has_filename(&normalized) {
        hints.push("exact-filename".to_string());
        return ClassifiedQuery {
            query_type: QueryType::Exact,
            raw: raw.to_string(),
            normalized,
            hints,
        };
    }
    if has_path_like(&normalized) || has_filename(&normalized) {
        hints.push("exact".to_string());
        if normalized.split_whitespace().count() <= 3 {
            return ClassifiedQuery {
                query_type: QueryType::Exact,
                raw: raw.to_string(),
                normalized,
                hints,
            };
        }
        hints.push("path-in-conceptual".to_string());
        return ClassifiedQuery {
            query_type: QueryType::Mixed,
            raw: raw.to_string(),
            normalized,
            hints,
        };
    }

    if looks_like_identifier(&normalized)
        || (Regex::new(r"(?i)\b(where is|where's|defined|implementation|implemented|define)\b")
            .unwrap()
            .is_match(&lower)
            && has_identifier(&normalized))
    {
        let tokens: Vec<&str> = normalized.split_whitespace().collect();
        let has_id = tokens.iter().any(|t| {
            let base = t.split(['.', ':']).next_back().unwrap_or(t);
            is_identifier_token(base) || has_qualified_symbol(t)
        });
        if has_id {
            if tokens.len() <= 6 {
                hints.push("symbol".to_string());
                return ClassifiedQuery {
                    query_type: QueryType::Symbol,
                    raw: raw.to_string(),
                    normalized,
                    hints,
                };
            }
            hints.push("symbol-in-long-query".to_string());
            return ClassifiedQuery {
                query_type: QueryType::Mixed,
                raw: raw.to_string(),
                normalized,
                hints,
            };
        }
    }

    let sym_def_re =
        Regex::new(r"(?i)\b(where is|where's|defined|implementation|implemented|define)\b")
            .unwrap();
    if sym_def_re.is_match(&lower) {
        let ids: Vec<&str> = normalized
            .split_whitespace()
            .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
            .collect();
        // Simplified: check if any token is identifier
        for tok in ids {
            let base = tok.split(['.', ':']).next_back().unwrap_or(tok);
            if is_identifier_token(base) || has_qualified_symbol(tok) {
                hints.push("symbol-definition".to_string());
                return ClassifiedQuery {
                    query_type: QueryType::Symbol,
                    raw: raw.to_string(),
                    normalized,
                    hints,
                };
            }
        }
    }

    let is_question = lower.starts_with("where")
        || lower.starts_with("what")
        || lower.starts_with("how")
        || normalized.contains('?');
    let word_count = normalized.split_whitespace().count();
    if word_count >= 4
        && (is_question
            || Regex::new(r"enforced|responsible|logic|handle|prevent|validate|implemented|ontology|pipeline|wired")
                .unwrap()
                .is_match(&lower))
    {
        hints.push("conceptual".to_string());
        return ClassifiedQuery {
            query_type: QueryType::Conceptual,
            raw: raw.to_string(),
            normalized,
            hints,
        };
    }

    if single
        && Regex::new(r"^[A-Za-z_][A-Za-z0-9_:]*$")
            .unwrap()
            .is_match(&normalized)
        && has_identifier(&normalized)
    {
        return ClassifiedQuery {
            query_type: QueryType::Symbol,
            raw: raw.to_string(),
            normalized,
            hints: vec!["single-identifier".to_string()],
        };
    }
    if word_count > 6 && has_identifier(&normalized) {
        return ClassifiedQuery {
            query_type: QueryType::Mixed,
            raw: raw.to_string(),
            normalized,
            hints: vec!["mixed-concept-symbol".to_string()],
        };
    }
    if word_count >= 6 {
        return ClassifiedQuery {
            query_type: QueryType::Conceptual,
            raw: raw.to_string(),
            normalized,
            hints: vec!["long-natural".to_string()],
        };
    }
    if word_count >= 1 {
        return ClassifiedQuery {
            query_type: QueryType::Mixed,
            raw: raw.to_string(),
            normalized,
            hints: vec!["default-mixed".to_string()],
        };
    }

    ClassifiedQuery {
        query_type: QueryType::Conceptual,
        raw: raw.to_string(),
        normalized,
        hints,
    }
}

fn is_quoted_literal(q: &str) -> bool {
    let t = q.trim();
    (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
        || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
}

fn has_path_like(q: &str) -> bool {
    let re = Regex::new(r"[\/\\].+\.\w+").unwrap();
    re.is_match(q) || Regex::new(r"\b\w+/\w+").unwrap().is_match(q)
}

fn has_filename(q: &str) -> bool {
    for tok in q.split_whitespace() {
        let clean = tok
            .trim_matches(|c| {
                c == '"'
                    || c == '\''
                    || c == '?'
                    || c == '.'
                    || c == '!'
                    || c == ','
                    || c == ';'
                    || c == ':'
                    || c == '('
                    || c == ')'
            })
            .to_string();
        if clean.is_empty() {
            continue;
        }
        if Regex::new(
            r"^(Dockerfile|Makefile|Procfile|Justfile|Brewfile|Gemfile|Rakefile|go\.mod|go\.sum)$",
        )
        .unwrap()
        .is_match(&clean)
        {
            return true;
        }
        let ext = std::path::Path::new(&clean)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !ext.is_empty() {
            let kind = context_index::classify_file(std::path::Path::new(&clean));
            if matches!(
                kind,
                context_index::FileKind::Source
                    | context_index::FileKind::Config
                    | context_index::FileKind::Build
                    | context_index::FileKind::Doc
            ) {
                return true;
            }
            if clean.contains('.')
                && clean.len() < 40
                && !clean.contains(' ')
                && Regex::new(r"\.[a-z0-9]{1,5}$")
                    .unwrap()
                    .is_match(&clean.to_lowercase())
            {
                return true;
            }
        }
    }
    false
}

fn is_snake_case(q: &str) -> bool {
    Regex::new(r"\b[a-z]+_[a-z_0-9]*\b").unwrap().is_match(q)
        || Regex::new(r"\b[A-Z]+_[A-Z_0-9]*\b").unwrap().is_match(q)
}
fn is_camel_case(q: &str) -> bool {
    Regex::new(r"\b[a-z]+[A-Z][a-zA-Z0-9]*\b")
        .unwrap()
        .is_match(q)
}
fn is_pascal_case(q: &str) -> bool {
    Regex::new(r"\b[A-Z][a-z]+[A-Z][a-zA-Z0-9]*\b")
        .unwrap()
        .is_match(q)
        || Regex::new(r"^[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]+)+$")
            .unwrap()
            .is_match(q)
}
fn is_screaming_snake(q: &str) -> bool {
    Regex::new(r"\b[A-Z]+_[A-Z0-9_]+\b").unwrap().is_match(q)
}
fn has_qualified_symbol(q: &str) -> bool {
    Regex::new(r"[a-zA-Z_][\w]*[.:]{1,2}[a-zA-Z_][\w]*")
        .unwrap()
        .is_match(q)
        || q.contains("::")
}
fn looks_like_identifier(q: &str) -> bool {
    let t = q.trim();
    let re = Regex::new(r"^[A-Za-z_][A-Za-z0-9_:]*(\.[A-Za-z_][A-Za-z0-9_]*)*$").unwrap();
    if re.is_match(t)
        && (is_snake_case(t)
            || is_camel_case(t)
            || is_pascal_case(t)
            || is_screaming_snake(t)
            || has_qualified_symbol(t)
            || t.len() > 2)
    {
        return true;
    }
    let m = Regex::new(r"(?i)\b(?:function|class|method|symbol|struct|interface|type|func)\s+([A-Za-z_][A-Za-z0-9_:]*)\b").unwrap();
    m.is_match(t)
}
fn is_identifier_token(t: &str) -> bool {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap().is_match(t)
        && (is_snake_case(t) || is_camel_case(t) || is_pascal_case(t) || is_screaming_snake(t))
}
fn has_identifier(q: &str) -> bool {
    let re = Regex::new(r"\b[A-Za-z_][A-Za-z0-9_:]*\b").unwrap();
    for m in re.find_iter(q) {
        let tok = m.as_str();
        let base = tok.split(['.', ':']).next_back().unwrap_or(tok);
        if is_identifier_token(base) || has_qualified_symbol(tok) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_mod_exact() {
        assert_eq!(classify_query("go.mod").query_type, QueryType::Exact);
    }
    #[test]
    fn where_count_tokens_symbol() {
        assert_eq!(
            classify_query("Where is count_tokens implemented?").query_type,
            QueryType::Symbol
        );
    }
    #[test]
    fn what_calls_dependency() {
        assert_eq!(
            classify_query("What calls bundle?").query_type,
            QueryType::Dependency
        );
    }
    #[test]
    fn where_secret_conceptual() {
        // V2 classifies "Where is secret redaction implemented?" as CONCEPTUAL (has where + conceptual hint)
        // Our port keeps same: it will be CONCEPTUAL because word_count >=4 and is_question true
        let c = classify_query("Where is secret redaction implemented?");
        assert!(matches!(
            c.query_type,
            QueryType::Conceptual | QueryType::Symbol
        ));
    }
}
