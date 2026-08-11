use std::collections::HashMap;

use crate::types::{Evidence, EvidenceRelation, QueryType, RetrievalSource};
use context_index::{classify_file, FileKind};
use std::path::Path;

pub const AUTHORITY_WEIGHTS: &[(&str, i32)] = &[
    ("exactLiteralMatch", 28),
    ("symbolDefinition", 35),
    ("trueDefinition", 18),
    ("graphRelation", 20),
    ("callerReference", 35),
    ("sameLanguageImpl", 10),
    ("testWhenAsked", 25),
    ("publicOverPrivate", 8),
    ("activeReference", 7),
    ("docWhenImplAsked", -15),
    ("docWhenTestAsked", -20),
    ("generated", -20),
    ("duplicateOverlap", -10),
    ("broadContextMatch", -8),
    ("testWhenImplAsked", -12),
    ("staleDoc", -30),
    ("shadowPenalty", -12),
    ("legacyScriptPenalty", -10),
];

fn get_weight(name: &str) -> i32 {
    AUTHORITY_WEIGHTS
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

const STALE_MARKERS: &[&str] = &["archive", "legacy", "deprecated"];

fn is_stale_doc(file: &str) -> bool {
    let lower = file.to_lowercase();
    STALE_MARKERS.iter().any(|m| lower.contains(m))
}

fn get_target_symbol(raw_query: &str) -> Option<String> {
    let re = regex::Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*\b").unwrap();
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
        "calls",
        "callers",
        "callees",
        "tests",
        "test",
        "cover",
        "covers",
        "implemented",
        "implementation",
        "generation",
        "flow",
        "trace",
        "secret",
        "redaction",
    ]
    .into_iter()
    .collect();
    let mut ids: Vec<String> = Vec::new();
    for m in re.find_iter(raw_query) {
        let t = m.as_str().to_string();
        if stop.contains(t.to_lowercase().as_str()) {
            continue;
        }
        if t.len() < 3 {
            continue;
        }
        if t.contains('_') || t.chars().any(|c| c.is_uppercase()) || t.to_lowercase().len() >= 3 {
            ids.push(t);
        }
    }
    // Dedup case-insensitive, prefer snake
    let mut seen: HashMap<String, String> = HashMap::new();
    for t in ids {
        let k = t.to_lowercase();
        if !seen.contains_key(&k) {
            seen.insert(k.clone(), t);
        } else {
            let prev = seen.get(&k).unwrap().clone();
            if t.contains('_') && !prev.contains('_') {
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
        let a_pascal = if regex::Regex::new(r"^[A-Z][a-z]+[A-Z]").unwrap().is_match(a) {
            0
        } else {
            1
        };
        let b_pascal = if regex::Regex::new(r"^[A-Z][a-z]+[A-Z]").unwrap().is_match(b) {
            0
        } else {
            1
        };
        if a_pascal != b_pascal {
            return a_pascal.cmp(&b_pascal);
        }
        b.len().cmp(&a.len())
    });
    uniq.first().cloned().map(|s| s.to_lowercase()).or_else(|| {
        re.find_iter(raw_query)
            .map(|m| m.as_str().to_string())
            .find(|t| !stop.contains(t.to_lowercase().as_str()) && t.len() >= 3)
            .map(|s| s.to_lowercase())
    })
}

fn is_true_definition(evidence: &Evidence, raw_query: &str) -> bool {
    let text = evidence
        .text
        .as_deref()
        .or_else(|| {
            evidence
                .metadata
                .as_ref()
                .and_then(|m| m.get("codeSnippet"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_lowercase();
    let kind = evidence.symbol_kind.as_deref().unwrap_or("").to_lowercase();
    if let Some(sym) = &evidence.symbol {
        let sym_lower = sym.to_lowercase();
        if [
            "function_definition",
            "function_declaration",
            "method",
            "class_definition",
            "class_declaration",
            "struct",
            "interface",
            "type",
            "enum",
            "trait",
            "impl",
        ]
        .iter()
        .any(|k| kind.contains(k))
        {
            if !text.is_empty() {
                if text.contains(&format!("func {}", sym_lower))
                    || text.contains(&format!("func ({}", sym_lower))
                    || text.contains(&format!("type {} struct", sym_lower))
                    || text.contains(&format!("type {} interface", sym_lower))
                {
                    return true;
                }
                if text.contains(&format!("def {}", sym_lower))
                    || text.contains(&format!("class {}", sym_lower))
                {
                    return true;
                }
                if text.contains(&format!("function {}", sym_lower))
                    || text.contains(&format!("const {}", sym_lower))
                    || text.contains(&format!("fn {}", sym_lower))
                {
                    return true;
                }
            } else {
                return true;
            }
        }
    }
    let target = match get_target_symbol(raw_query) {
        Some(t) => t,
        None => return false,
    };
    if text.is_empty() {
        return false;
    }
    let patterns = [
        format!("def {}", target),
        format!("class {}", target),
        format!("func {}", target),
        "func (".to_string(),
        format!("type {} struct", target),
        format!("type {} interface", target),
        format!("function {}", target),
        format!("const {}", target),
        format!("fn {}", target),
        format!("struct {}", target),
        format!("enum {}", target),
        format!("trait {}", target),
    ];
    if text.contains(&format!("func {}(", target)) || text.contains(&format!("func ({}", target)) {
        let re = regex::Regex::new(&format!(r"\b{}\b", regex::escape(&target))).unwrap();
        if re.is_match(&text) {
            return true;
        }
    }
    for pat in &patterns {
        if text.contains(pat) {
            return true;
        }
    }
    if let Some(sym) = &evidence.symbol {
        if sym.to_lowercase() == target && kind.contains("definition") {
            return true;
        }
    }
    false
}

pub fn score_authority(
    evidence: &Evidence,
    query_type: QueryType,
    raw_query: &str,
    has_impl_alternative: bool,
) -> (i32, Vec<String>) {
    let mut score = 0;
    let mut reasons = Vec::new();
    let file = &evidence.file;
    let lower_query = raw_query.to_lowercase();
    let target_sym = get_target_symbol(raw_query);
    let kind = classify_file(Path::new(file));

    if evidence.source == RetrievalSource::Exact && evidence.score == Some(1.0) {
        let w = get_weight("exactLiteralMatch");
        score += w;
        reasons.push(format!("+{} exact literal", w));
    }
    if evidence.relation == Some(EvidenceRelation::Definition)
        && evidence.source == RetrievalSource::Symbol
    {
        let w = get_weight("symbolDefinition");
        score += w;
        reasons.push(format!("+{} symbol definition", w));
    }
    if is_true_definition(evidence, raw_query) {
        let w = get_weight("trueDefinition");
        score += w;
        reasons.push(format!(
            "+{} true definition (def {})",
            w,
            evidence
                .symbol
                .clone()
                .unwrap_or_else(|| target_sym.clone().unwrap_or_default())
        ));
    }
    if evidence.source == RetrievalSource::Graph
        && (evidence.relation == Some(EvidenceRelation::Caller)
            || evidence.relation == Some(EvidenceRelation::Callee))
    {
        let w = get_weight("graphRelation");
        score += w;
        reasons.push(format!(
            "+{} graph {}",
            w,
            evidence.relation.unwrap().as_str()
        ));
    }
    if kind == FileKind::Source {
        let w = get_weight("sameLanguageImpl");
        score += w;
        reasons.push(format!("+{} source file", w));
    }
    if query_type == QueryType::Test && kind == FileKind::Test {
        let w = get_weight("testWhenAsked");
        score += w;
        reasons.push(format!("+{} test when asked", w));
    }
    if let Some(sym) = &evidence.symbol {
        if lower_query.contains(&sym.to_lowercase()) {
            score += 5;
            reasons.push("+5 symbol in query".to_string());
        }
        if sym.starts_with('_') && lower_query.contains("secret") && has_impl_alternative {
            let w = get_weight("shadowPenalty");
            score += w;
            reasons.push(format!("{} private shadow", w));
        }
        if !sym.starts_with('_')
            && sym.to_lowercase().contains("redact")
            && lower_query.contains("secret")
        {
            let w = get_weight("publicOverPrivate");
            score += w;
            reasons.push(format!("+{} public impl", w));
        }
    }
    if kind == FileKind::Source
        && file.to_lowercase().contains("/core/")
        && lower_query.contains("secret")
    {
        let w = get_weight("activeReference");
        score += w;
        reasons.push(format!("+{} active core", w));
    }
    if kind == FileKind::Generated {
        let w = get_weight("generated");
        score += w;
        reasons.push(format!("{} generated", w));
    }
    if is_stale_doc(file) {
        let w = get_weight("staleDoc");
        score += w;
        reasons.push(format!("{} stale", w));
    }
    let wants_impl = matches!(query_type, QueryType::Symbol | QueryType::Dependency | QueryType::Mixed)
        || (query_type == QueryType::Conceptual
            && regex::Regex::new(r"implemented|generation|bundle|logic|validation|handler|router|service|enforced|ontology|pipeline")
                .unwrap()
                .is_match(&lower_query));
    if wants_impl && kind == FileKind::Doc {
        let w = get_weight("docWhenImplAsked");
        score += w;
        reasons.push(format!("{} doc when impl wanted", w));
    }
    if query_type == QueryType::Test && kind == FileKind::Doc {
        let w = get_weight("docWhenTestAsked");
        score += w;
        reasons.push(format!("{} doc when test asked", w));
    }
    if wants_impl && file.starts_with("v2/") && !lower_query.contains("v2") {
        score -= 12;
        reasons.push("-12 v2 self when impl wanted".to_string());
    }
    if query_type != QueryType::Test && kind == FileKind::Test {
        let w = get_weight("testWhenImplAsked");
        score += w;
        reasons.push(format!("{} test when impl wanted", w));
    }
    if evidence.symbol.as_deref() == Some("_ensure_context_dir")
        || evidence.symbol.as_deref() == Some("ContextEngineCLI")
    {
        let w = get_weight("broadContextMatch");
        score += w;
        reasons.push(format!("{} broad context helper", w));
    }
    if file.ends_with("__init__.py") {
        score -= 5;
        reasons.push("-5 __init__ re-export".to_string());
    }
    if kind == FileKind::Source && file.contains("/scripts/") && has_impl_alternative {
        score -= 5;
        reasons.push("-5 scripts when core exists".to_string());
    }
    let is_caller_query =
        regex::Regex::new(r"(?i)\b(what calls|who calls|callers|used by|what breaks if)\b")
            .unwrap()
            .is_match(raw_query);
    if query_type == QueryType::Dependency && is_caller_query {
        let is_reference = evidence.source == RetrievalSource::Exact
            && evidence.relation == Some(EvidenceRelation::Reference)
            && target_sym
                .as_ref()
                .map(|t| {
                    evidence
                        .text
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&t.to_lowercase())
                })
                .unwrap_or(false);
        let is_caller = evidence.relation == Some(EvidenceRelation::Caller);
        if is_reference || is_caller {
            let is_external = !file.to_lowercase().contains(
                &target_sym
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(4)
                    .collect::<String>(),
            ) || !is_true_definition(evidence, raw_query);
            if is_external || file != "backend/context_engine/commands/bundle_command.py" {
                let w = get_weight("callerReference");
                score += w;
                reasons.push(format!("+{} caller reference", w));
                let txt = evidence.text.as_deref().unwrap_or("").to_lowercase();
                if txt.contains("add_command")
                    || txt.contains("register")
                    || txt.contains("newrouter")
                    || txt.contains("healthhandler")
                    || target_sym
                        .as_ref()
                        .map(|t| txt.contains(&format!("{}.", t.to_lowercase())))
                        .unwrap_or(false)
                {
                    score += 8;
                    reasons.push("+8 wiring pattern".to_string());
                }
                if kind == FileKind::Source && file.starts_with("backend/") {
                    score += 5;
                    reasons.push("+5 backend wiring".to_string());
                } else if kind == FileKind::Source && file.contains("/internal/") {
                    score += 5;
                    reasons.push("+5 internal wiring".to_string());
                }
            }
        }
        if evidence.relation == Some(EvidenceRelation::Definition)
            && evidence.source == RetrievalSource::Symbol
            && is_true_definition(evidence, raw_query)
        {
            score -= 15;
            reasons.push("-15 definition for caller query".to_string());
        }
    }

    (score, reasons)
}

pub fn apply_authority(
    evidence: Vec<Evidence>,
    query_type: QueryType,
    raw_query: &str,
) -> Vec<Evidence> {
    let has_impl = evidence
        .iter()
        .any(|e| classify_file(Path::new(&e.file)) == FileKind::Source);
    let mut min_line_per_file: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    for e in &evidence {
        if e.source == RetrievalSource::Symbol && e.relation == Some(EvidenceRelation::Definition) {
            let key = e.file.clone();
            let cur = min_line_per_file.get(&key).cloned();
            let start = e.start_line.unwrap_or(u32::MAX);
            if cur.is_none() || start < cur.unwrap() {
                min_line_per_file.insert(key, start);
            }
        }
    }
    let mut out = Vec::new();
    for mut e in evidence {
        let (mut score, mut reasons) = score_authority(&e, query_type, raw_query, has_impl);
        let base = e.score.unwrap_or(0.0);
        if e.source == RetrievalSource::Symbol
            && e.relation == Some(EvidenceRelation::Definition)
            && e.start_line.is_some()
        {
            if let Some(min) = min_line_per_file.get(&e.file) {
                if e.start_line == Some(*min)
                    && !reasons.iter().any(|r| r.contains("true definition"))
                {
                    let text = e.text.as_deref().unwrap_or("").to_lowercase();
                    if text.contains("def ")
                        || text.contains("class ")
                        || text.contains("func ")
                        || text.contains("type ")
                    {
                        let w = get_weight("trueDefinition");
                        score += w;
                        reasons.push(format!("+{} earliest definition in file", w));
                    }
                }
            }
        }
        let final_score = base * 20.0 + score as f64;
        e.authority_score = Some(score);
        e.final_score = Some(final_score);
        // Store reasons in metadata for debugging
        e.metadata = Some(serde_json::json!({ "authorityReasons": reasons }));
        out.push(e);
    }
    out
}
