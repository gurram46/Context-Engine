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
        if t.len() < 3 {
            continue;
        }
        let is_snake = t.contains('_');
        let is_screaming = SCREAMING_RE.is_match(&t);
        let is_camel = CAMEL_RE.is_match(&t);
        let is_pascal = PASCAL_RE.is_match(&t) || PASCAL_RE2.is_match(&t);
        let is_qualified = t.contains('.') || t.contains("::");
        let is_lower_generic = LOWER_RE.is_match(&low) && !stop.contains(low.as_str());
        if is_snake || is_screaming || is_camel || is_pascal || is_qualified || is_lower_generic {
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
}
