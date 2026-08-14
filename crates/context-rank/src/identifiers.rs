use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static IDENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*(?:[.:]{1,2}[A-Za-z_][A-Za-z0-9_]*)*\b").unwrap()
});
static SCREAMING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z]+_[A-Z0-9_]+$").unwrap());
static CAMEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z]+[A-Z][a-zA-Z0-9]*$").unwrap());
static PASCAL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][a-z]+(?:[A-Z][a-z0-9]*)+$").unwrap());
static PASCAL_RE2: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][a-z0-9]*[A-Z][a-zA-Z0-9]*$").unwrap());
static LOWER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]{3,}$").unwrap());

/// Extract identifiers from query, deterministic.
/// Supports snake, camel, Pascal, SCREAMING, qualified.
pub fn extract_identifiers(q: &str) -> Vec<String> {
    let re = &*IDENT_RE;
    let mut raw_ids: Vec<String> = Vec::new();
    for m in re.find_iter(q) {
        let tok = m.as_str().to_string();
        raw_ids.push(tok.clone());
        if tok.contains('.') || tok.contains("::") {
            for part in tok.split(['.', ':']) {
                if part.len() >= 3 {
                    raw_ids.push(part.to_string());
                }
            }
        }
    }

    let stop: std::collections::HashSet<&str> = [
        "where",
        "what",
        "who",
        "how",
        "which",
        "when",
        "why",
        "the",
        "is",
        "are",
        "for",
        "and",
        "or",
        "to",
        "in",
        "of",
        "a",
        "an",
        "find",
        "show",
        "please",
        "example",
        "file",
        "trace",
        "flow",
        "generation",
        "calls",
        "callers",
        "callees",
        "tests",
        "test",
        "cover",
        "covers",
        "implemented",
        "implementation",
    ]
    .into_iter()
    .collect();

    let mut filtered: Vec<String> = Vec::new();
    for t in raw_ids {
        let low = t.to_lowercase();
        if stop.contains(low.as_str()) {
            continue;
        }
        // Allow single uppercase identifiers like "Q" for test queries (e.g., Q objects)
        let is_single_upper = t.len() == 1
            && t.chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false);
        if t.len() < 3 && !is_single_upper {
            continue;
        }
        let is_snake = t.contains('_');
        let is_screaming = SCREAMING_RE.is_match(&t);
        let is_camel = CAMEL_RE.is_match(&t);
        let is_pascal = PASCAL_RE.is_match(&t)
            || PASCAL_RE2.is_match(&t)
            || (t.len() >= 3 && {
                static SINGLE_PASCAL: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"^[A-Z][a-z][a-zA-Z0-9]*$").unwrap());
                SINGLE_PASCAL.is_match(&t)
            });
        let is_qualified = t.contains('.') || t.contains("::");
        let is_lower_generic = LOWER_RE.is_match(&low) && !stop.contains(low.as_str());
        let is_single_upper = t.len() == 1
            && t.chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false);
        if is_snake
            || is_screaming
            || is_camel
            || is_pascal
            || is_qualified
            || is_lower_generic
            || is_single_upper
        {
            filtered.push(t);
        }
    }

    // Dedup case-insensitive, keep first, prefer snake and lower over Pascal
    let mut seen: HashMap<String, String> = HashMap::new();
    for t in filtered {
        let k = t.to_lowercase();
        if !seen.contains_key(&k) {
            seen.insert(k, t);
        } else {
            let prev = seen.get(&k).unwrap().clone();
            let prefer_snake = t.contains('_') && !prev.contains('_');
            let prefer_lower = t == t.to_lowercase() && prev != prev.to_lowercase();
            if prefer_snake || prefer_lower {
                seen.insert(k, t);
            }
        }
    }
    let mut uniq: Vec<String> = seen.into_values().collect();
    uniq.sort_by(|a, b| {
        let a_snake = if a.contains('_') { 0 } else { 1 };
        let b_snake = if b.contains('_') { 0 } else { 1 };
        if a_snake != b_snake {
            return a_snake.cmp(&b_snake);
        }
        let a_pascal = if a.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            0
        } else {
            1
        };
        let b_pascal = if b.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            0
        } else {
            1
        };
        if a_pascal != b_pascal {
            return a_pascal.cmp(&b_pascal);
        }
        b.len().cmp(&a.len())
    });
    uniq.truncate(5);
    uniq
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_tokens() {
        let ids = extract_identifiers("Where is count_tokens implemented?");
        assert!(ids.contains(&"count_tokens".to_string()));
    }
    #[test]
    fn new_router() {
        let ids = extract_identifiers("Where is NewRouter implemented?");
        assert!(ids.iter().any(|s| s == "NewRouter"));
    }
    #[test]
    fn domain_terms_not_stop_words() {
        let ids = extract_identifiers("Where is secret redaction implemented?");
        assert!(
            ids.iter().any(|s| s == "secret"),
            "secret should be an identifier"
        );
        assert!(
            ids.iter().any(|s| s == "redaction"),
            "redaction should be an identifier"
        );
    }

    #[test]
    fn find_cargo_toml_no_find() {
        let ids = extract_identifiers("Find Cargo.toml for ripgrep");
        assert!(
            !ids.iter().any(|s| s.to_lowercase() == "find"),
            "Find should not be an identifier, got {:?}",
            ids
        );
    }

    #[test]
    fn find_app_module_foo_no_find_example() {
        let ids = extract_identifiers("Find app.module.ts in the foo example");
        assert!(
            !ids.iter().any(|s| s.to_lowercase() == "find"),
            "Find should not be identifier"
        );
        assert!(
            !ids.iter().any(|s| s.to_lowercase() == "example"),
            "example should not be identifier"
        );
    }

    #[test]
    fn where_model_contains_model() {
        let ids = extract_identifiers("Where is Model implemented?");
        assert!(
            ids.iter().any(|s| s == "Model"),
            "Model should be identifier"
        );
    }

    #[test]
    fn what_calls_search_contains_search() {
        let ids = extract_identifiers("What calls search?");
        assert!(ids.iter().any(|s| s.to_lowercase() == "search"));
    }
}
