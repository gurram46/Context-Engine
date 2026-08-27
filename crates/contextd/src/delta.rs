#![allow(dead_code)]
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::path::Path;

use crate::proof::{self, ProofEvidence};
use context_index::structural::store as structural_store;
use context_rank::packer::{delivered_hash, delivered_snippet};

// ============================================================================
// DeltaStatus / EvidenceClassification
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum DeltaStatus {
    Ready,
    NoChange,
    Unverifiable,
}

impl DeltaStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NoChange => "no_change",
            Self::Unverifiable => "unverifiable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EvidenceClassification {
    Unchanged,
    Changed,
    Removed,
    Moved,
    Unverifiable,
}

impl EvidenceClassification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Changed => "changed",
            Self::Removed => "removed",
            Self::Moved => "moved",
            Self::Unverifiable => "unverifiable",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeltaEvidence {
    pub ordinal: usize,
    pub old_path: String,
    pub current_path: Option<String>,
    pub old_start_line: Option<u32>,
    pub current_start_line: Option<u32>,
    pub old_content_hash: String,
    pub current_content_hash: Option<String>,
    pub classification: EvidenceClassification,
    pub symbol: Option<String>,
    pub reason: String,
    pub location_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeltaSummary {
    pub unchanged: usize,
    pub changed: usize,
    pub removed: usize,
    pub moved: usize,
    pub unverifiable: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextDelta {
    pub proof_id: String,
    pub repo_instance_id: String,
    pub from_source_state_hash: String,
    pub to_source_state_hash: Option<String>,
    pub from_generation: u64,
    pub to_generation: Option<u64>,
    pub status: DeltaStatus,
    pub summary: DeltaSummary,
    pub evidence: Vec<DeltaEvidence>,
    pub query: String,
    pub packed_context_hash: String,
    pub reason: String,
}

// ============================================================================
// Reconstruction contracts — inspected P1 pathways (pipeline.rs)
// ============================================================================
//
// P1 Evidence.text derivation per provenance (pipeline.rs):
// - rust:exact        : exact_search line text (single line, rg match, chars.take(400)) -> DIRECT
// - rust:symbol       : load_symbol_snippet(start_byte..end_byte) (symbol definition slice, up to 400 chars, may be multi-line) -> SYMBOL
// - rust:symbol:prefix: same as rust:symbol (prefix search) -> SYMBOL
// - rust:graph:*      : caller/callee symbol snippet or fallback file line (depends on symbol resolution) -> UNSAFE (insufficient persisted identity)
// - rust:test*, rust:test:file, rust:test:inline : symbol snippet or file snippet (test file heuristics) -> UNSAFE
// - hot:bm25 / rust:bm25 : load_chunk_snippet(chunk slice) -> UNSAFE (no chunk_id persisted in proof, cannot uniquely identify chunk)
// - hot:semantic / rust:semantic / fused bm25+semantic : vector chunk snippet -> UNSAFE
// - synthetic/fixture/test packer provenance like "test" -> UNSAFE
//
// Deterministic reconstruction requires:
// - For DIRECT: relative_path + start_line identify the line; snippet = delivered_snippet(line). Reproducible via reading same file's line.
// - For SYMBOL: relative_path + symbol_name identify symbol definition in same file; snippet = delivered_snippet(definition slice). Reproducible via structural symbols in same file.
// - All other provenances lack stable identity in proof (no chunk_id/symbol_id persisted) → UNVERIFIABLE.
//
// Spec requires preferring UNVERIFIABLE over guessed matches.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReconstructionContract {
    Direct,
    Symbol,
    Unsafe,
}

fn contract_for(ev: &ProofEvidence) -> ReconstructionContract {
    match ev.provenance.as_deref() {
        Some("rust:exact") => ReconstructionContract::Direct,
        Some("rust:symbol") | Some("rust:symbol:prefix") => ReconstructionContract::Symbol,
        // All other provenances including graph, test, bm25, semantic, hot:*, fused, and None are unsafe for P2 v1
        _ => ReconstructionContract::Unsafe,
    }
}

// ============================================================================
// Helpers — same-file exact match with cardinality
// ============================================================================

/// Result of same-file search for historical hash among valid P1 candidates for a contract.
enum ExactMatchCardinality {
    NoMatch,
    Unique { start_line: u32, snippet: String },
    Ambiguous { count: usize },
}

fn find_direct_matches(root: &Path, rel: &str, target_hash: &str) -> ExactMatchCardinality {
    let path = root.join(rel);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return ExactMatchCardinality::NoMatch,
    };
    let mut matches: Vec<(u32, String)> = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let snippet = delivered_snippet(Some(line));
        if snippet.is_empty() {
            continue;
        }
        let h = delivered_hash(&snippet);
        if h == target_hash {
            matches.push(((idx as u32) + 1, snippet));
            if matches.len() > 1 {
                return ExactMatchCardinality::Ambiguous {
                    count: matches.len(),
                };
            }
        }
    }
    match matches.len() {
        0 => ExactMatchCardinality::NoMatch,
        1 => {
            let (line, snippet) = matches.into_iter().next().unwrap();
            ExactMatchCardinality::Unique {
                start_line: line,
                snippet,
            }
        }
        _ => ExactMatchCardinality::Ambiguous {
            count: matches.len(),
        },
    }
}

fn find_symbol_matches(
    conn: &rusqlite::Connection,
    root: &Path,
    rel: &str,
    symbol_name: &Option<String>,
    target_hash: &str,
) -> ExactMatchCardinality {
    let Some(name) = symbol_name else {
        return ExactMatchCardinality::NoMatch;
    };
    // Only symbols in same file with same name/qualified_name
    let symbols = match query_symbols_in_file(conn, rel, name) {
        Ok(v) => v,
        Err(_) => return ExactMatchCardinality::NoMatch,
    };
    if symbols.is_empty() {
        return ExactMatchCardinality::NoMatch;
    }
    // Load file once
    let path = root.join(rel);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return ExactMatchCardinality::NoMatch,
    };
    let bytes = content.as_bytes();
    let mut matches: Vec<(u32, String)> = Vec::new();
    for sym in &symbols {
        let start = sym.start_byte.min(bytes.len());
        let end = sym.end_byte.min(bytes.len());
        if end <= start {
            continue;
        }
        let slice = match std::str::from_utf8(&bytes[start..end]) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // P1 used load_symbol_snippet: slice chars take 400 trimmed; we replicate delivered_snippet which takes 120 chars of that slice
        // For correctness we use delivered_snippet on the slice truncated to 400 chars as P1 does, but slice may be shorter than 400 already.
        let truncated: String = slice
            .chars()
            .take(400)
            .collect::<String>()
            .trim()
            .to_string();
        let snippet = delivered_snippet(Some(&truncated));
        if snippet.is_empty() {
            continue;
        }
        let h = delivered_hash(&snippet);
        if h == target_hash {
            matches.push((sym.start_line, snippet));
            if matches.len() > 1 {
                return ExactMatchCardinality::Ambiguous {
                    count: matches.len(),
                };
            }
        }
    }
    match matches.len() {
        0 => ExactMatchCardinality::NoMatch,
        1 => {
            let (line, snippet) = matches.into_iter().next().unwrap();
            ExactMatchCardinality::Unique {
                start_line: line,
                snippet,
            }
        }
        _ => ExactMatchCardinality::Ambiguous {
            count: matches.len(),
        },
    }
}

struct SymbolInfo {
    start_line: u32,
    end_line: u32,
    start_byte: usize,
    end_byte: usize,
}

fn query_symbols_in_file(
    conn: &rusqlite::Connection,
    file: &str,
    name: &str,
) -> Result<Vec<SymbolInfo>> {
    let mut stmt = conn.prepare(
        "SELECT start_line, end_line, start_byte, end_byte FROM symbols WHERE file=?1 AND (name=?2 OR qualified_name=?2)",
    )?;
    let rows = stmt.query_map(rusqlite::params![file, name], |row| {
        Ok(SymbolInfo {
            start_line: row.get(0)?,
            end_line: row.get(1)?,
            start_byte: row.get::<_, i64>(2)? as usize,
            end_byte: row.get::<_, i64>(3)? as usize,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn current_symbol_hash_and_line(
    conn: &rusqlite::Connection,
    root: &Path,
    rel: &str,
    symbol_name: &str,
) -> Option<(u32, String, String)> {
    // Returns unique definition's line and hash/snippet if exactly one definition in same file
    let symbols = query_symbols_in_file(conn, rel, symbol_name).ok()?;
    if symbols.len() != 1 {
        return None;
    }
    let sym = &symbols[0];
    let path = root.join(rel);
    let content = std::fs::read_to_string(&path).ok()?;
    let bytes = content.as_bytes();
    let start = sym.start_byte.min(bytes.len());
    let end = sym.end_byte.min(bytes.len());
    if end <= start {
        return None;
    }
    let slice = std::str::from_utf8(&bytes[start..end]).ok()?;
    let truncated: String = slice
        .chars()
        .take(400)
        .collect::<String>()
        .trim()
        .to_string();
    let snippet = delivered_snippet(Some(&truncated));
    if snippet.is_empty() {
        return None;
    }
    let h = delivered_hash(&snippet);
    Some((sym.start_line, snippet, h))
}

fn current_line_hash(root: &Path, rel: &str, line_no: u32) -> Option<(String, String)> {
    let path = root.join(rel);
    let content = std::fs::read_to_string(&path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let idx = (line_no as usize).saturating_sub(1);
    if idx >= lines.len() {
        return None;
    }
    let snippet = delivered_snippet(Some(lines[idx]));
    if snippet.is_empty() {
        return None;
    }
    let h = delivered_hash(&snippet);
    Some((snippet, h))
}

// ============================================================================
// Main delta
// ============================================================================

pub fn delta_for_proof(root: &Path, proof_id: &str) -> Result<ContextDelta> {
    // Load proof
    let proof_conn = proof::open_proof_db(root)?;
    let proof = proof::load_proof(&proof_conn, proof_id)?
        .ok_or_else(|| anyhow!("proof not found: {}", proof_id))?;
    if proof.schema_version != proof::PROOF_SCHEMA_VERSION {
        return Ok(ContextDelta {
            proof_id: proof.id.clone(),
            repo_instance_id: proof.repo_instance_id.clone(),
            from_source_state_hash: proof.source_state_hash.clone(),
            to_source_state_hash: None,
            from_generation: proof.source_generation,
            to_generation: None,
            status: DeltaStatus::Unverifiable,
            summary: DeltaSummary {
                unchanged: 0,
                changed: 0,
                removed: 0,
                moved: 0,
                unverifiable: 0,
            },
            evidence: vec![],
            query: proof.query.clone(),
            packed_context_hash: proof.packed_context_hash.clone(),
            reason: "proof schema unsupported".into(),
        });
    }
    // Validate repo instance - handle both stripped and non-stripped canonical forms (Windows \\?\ prefix)
    let current_repo_id = context_index::project_root::repo_instance_id(root);
    let canon_current = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let current_id_via_canon = context_index::project_root::repo_instance_id(&canon_current);
    // Also handle stripped versions (Self::canon logic)
    let stripped_root = {
        let s = root.to_string_lossy().to_string();
        let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
        blake3::hash(stripped.as_bytes()).to_hex().to_string()
    };
    let stripped_canon = {
        let s = canon_current.to_string_lossy().to_string();
        let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
        blake3::hash(stripped.as_bytes()).to_hex().to_string()
    };
    if proof.repo_instance_id != current_repo_id
        && proof.repo_instance_id != current_id_via_canon
        && proof.repo_instance_id != stripped_root
        && proof.repo_instance_id != stripped_canon
    {
        return Ok(ContextDelta {
            proof_id: proof.id.clone(),
            repo_instance_id: proof.repo_instance_id.clone(),
            from_source_state_hash: proof.source_state_hash.clone(),
            to_source_state_hash: None,
            from_generation: proof.source_generation,
            to_generation: None,
            status: DeltaStatus::Unverifiable,
            summary: DeltaSummary {
                unchanged: 0,
                changed: 0,
                removed: 0,
                moved: 0,
                unverifiable: 0,
            },
            evidence: vec![],
            query: proof.query.clone(),
            packed_context_hash: proof.packed_context_hash.clone(),
            reason: "repo_instance_id mismatch".into(),
        });
    }
    // Get current source state
    let current_state = match proof::get_current_source_state(root) {
        Ok(s) => s,
        Err(e) => {
            return Ok(ContextDelta {
                proof_id: proof.id.clone(),
                repo_instance_id: proof.repo_instance_id.clone(),
                from_source_state_hash: proof.source_state_hash.clone(),
                to_source_state_hash: None,
                from_generation: proof.source_generation,
                to_generation: None,
                status: DeltaStatus::Unverifiable,
                summary: DeltaSummary {
                    unchanged: 0,
                    changed: 0,
                    removed: 0,
                    moved: 0,
                    unverifiable: 0,
                },
                evidence: vec![],
                query: proof.query.clone(),
                packed_context_hash: proof.packed_context_hash.clone(),
                reason: format!("current source state unavailable: {}", e),
            })
        }
    };
    if !current_state.verified {
        return Ok(ContextDelta {
            proof_id: proof.id.clone(),
            repo_instance_id: proof.repo_instance_id.clone(),
            from_source_state_hash: proof.source_state_hash.clone(),
            to_source_state_hash: Some(current_state.hash.clone()),
            from_generation: proof.source_generation,
            to_generation: Some(current_state.generation),
            status: DeltaStatus::Unverifiable,
            summary: DeltaSummary {
                unchanged: 0,
                changed: 0,
                removed: 0,
                moved: 0,
                unverifiable: 0,
            },
            evidence: vec![],
            query: proof.query.clone(),
            packed_context_hash: proof.packed_context_hash.clone(),
            reason: "current source snapshot unverified".into(),
        });
    }
    // Branch boundary
    let current_git = proof::git_snapshot(root);
    if proof.git_detached != current_git.detached {
        return Ok(ContextDelta {
            proof_id: proof.id.clone(),
            repo_instance_id: proof.repo_instance_id.clone(),
            from_source_state_hash: proof.source_state_hash.clone(),
            to_source_state_hash: Some(current_state.hash.clone()),
            from_generation: proof.source_generation,
            to_generation: Some(current_state.generation),
            status: DeltaStatus::Unverifiable,
            summary: DeltaSummary {
                unchanged: 0,
                changed: 0,
                removed: 0,
                moved: 0,
                unverifiable: 0,
            },
            evidence: vec![],
            query: proof.query.clone(),
            packed_context_hash: proof.packed_context_hash.clone(),
            reason: "branch detached mismatch".into(),
        });
    }
    if proof.git_detached {
        if proof.git_commit != current_git.commit {
            return Ok(ContextDelta {
                proof_id: proof.id.clone(),
                repo_instance_id: proof.repo_instance_id.clone(),
                from_source_state_hash: proof.source_state_hash.clone(),
                to_source_state_hash: Some(current_state.hash.clone()),
                from_generation: proof.source_generation,
                to_generation: Some(current_state.generation),
                status: DeltaStatus::Unverifiable,
                summary: DeltaSummary {
                    unchanged: 0,
                    changed: 0,
                    removed: 0,
                    moved: 0,
                    unverifiable: 0,
                },
                evidence: vec![],
                query: proof.query.clone(),
                packed_context_hash: proof.packed_context_hash.clone(),
                reason: "detached commit mismatch".into(),
            });
        }
    } else {
        if proof.git_branch != current_git.branch {
            return Ok(ContextDelta {
                proof_id: proof.id.clone(),
                repo_instance_id: proof.repo_instance_id.clone(),
                from_source_state_hash: proof.source_state_hash.clone(),
                to_source_state_hash: Some(current_state.hash.clone()),
                from_generation: proof.source_generation,
                to_generation: Some(current_state.generation),
                status: DeltaStatus::Unverifiable,
                summary: DeltaSummary {
                    unchanged: 0,
                    changed: 0,
                    removed: 0,
                    moved: 0,
                    unverifiable: 0,
                },
                evidence: vec![],
                query: proof.query.clone(),
                packed_context_hash: proof.packed_context_hash.clone(),
                reason: "branch mismatch".into(),
            });
        }
    }
    // Fast path same snapshot
    if proof.source_state_hash == current_state.hash {
        let evidences = proof::load_evidence(&proof_conn, proof_id)?;
        let mut delta_evidences = Vec::new();
        for ev in &evidences {
            delta_evidences.push(DeltaEvidence {
                ordinal: ev.ordinal,
                old_path: ev.relative_path.clone(),
                current_path: Some(ev.relative_path.clone()),
                old_start_line: ev.start_line,
                current_start_line: ev.start_line,
                old_content_hash: ev.delivered_content_hash.clone(),
                current_content_hash: Some(ev.delivered_content_hash.clone()),
                classification: EvidenceClassification::Unchanged,
                symbol: ev.symbol_name.clone(),
                reason: "same verified indexed snapshot".into(),
                location_changed: false,
            });
        }
        let summary = DeltaSummary {
            unchanged: delta_evidences.len(),
            changed: 0,
            removed: 0,
            moved: 0,
            unverifiable: 0,
        };
        return Ok(ContextDelta {
            proof_id: proof.id.clone(),
            repo_instance_id: proof.repo_instance_id.clone(),
            from_source_state_hash: proof.source_state_hash.clone(),
            to_source_state_hash: Some(current_state.hash.clone()),
            from_generation: proof.source_generation,
            to_generation: Some(current_state.generation),
            status: DeltaStatus::NoChange,
            summary,
            evidence: delta_evidences,
            query: proof.query.clone(),
            packed_context_hash: proof.packed_context_hash.clone(),
            reason: "same snapshot".into(),
        });
    }
    // Changed snapshot path - evidence-level comparison (deterministic, no cross-file scan)
    let evidences = proof::load_evidence(&proof_conn, proof_id)?;
    // Open structural DB once
    let structural_conn = match structural_store::open_db(root) {
        Ok(c) => c,
        Err(e) => {
            // If structural DB unavailable, all evidence becomes unverifiable
            let mut delta_evidences = Vec::new();
            for ev in evidences.iter() {
                delta_evidences.push(DeltaEvidence {
                    ordinal: ev.ordinal,
                    old_path: ev.relative_path.clone(),
                    current_path: Some(ev.relative_path.clone()),
                    old_start_line: ev.start_line,
                    current_start_line: None,
                    old_content_hash: ev.delivered_content_hash.clone(),
                    current_content_hash: None,
                    classification: EvidenceClassification::Unverifiable,
                    symbol: ev.symbol_name.clone(),
                    reason: format!("structural DB unavailable: {}", e),
                    location_changed: false,
                });
            }
            let summary = DeltaSummary {
                unchanged: 0,
                changed: 0,
                removed: 0,
                moved: 0,
                unverifiable: delta_evidences.len(),
            };
            return Ok(ContextDelta {
                proof_id: proof.id.clone(),
                repo_instance_id: proof.repo_instance_id.clone(),
                from_source_state_hash: proof.source_state_hash.clone(),
                to_source_state_hash: Some(current_state.hash.clone()),
                from_generation: proof.source_generation,
                to_generation: Some(current_state.generation),
                status: DeltaStatus::Ready,
                summary,
                evidence: delta_evidences,
                query: proof.query.clone(),
                packed_context_hash: proof.packed_context_hash.clone(),
                reason: "changed snapshot".into(),
            });
        }
    };
    let mut delta_evidences = Vec::new();
    let mut unchanged = 0;
    let mut changed = 0;
    let mut removed = 0;
    let moved_cnt: usize = 0;
    let mut unverifiable = 0;
    for ev in evidences {
        let contract = contract_for(&ev);
        // Handle synthetic / empty path quickly: mark unverifiable deterministically
        if ev.relative_path.is_empty() {
            delta_evidences.push(DeltaEvidence {
                ordinal: ev.ordinal,
                old_path: ev.relative_path.clone(),
                current_path: None,
                old_start_line: ev.start_line,
                current_start_line: None,
                old_content_hash: ev.delivered_content_hash.clone(),
                current_content_hash: None,
                classification: EvidenceClassification::Unverifiable,
                symbol: ev.symbol_name.clone(),
                reason:
                    "historical evidence representation cannot be reconstructed deterministically"
                        .into(),
                location_changed: false,
            });
            unverifiable += 1;
            continue;
        }
        // File existence check via indexed state (single connection, no per-evidence open)
        let current_hash_opt = structural_store::get_file_hash(&structural_conn, &ev.relative_path)
            .ok()
            .flatten();
        if current_hash_opt.is_none() {
            // Historical indexed file no longer exists -> REMOVED (P2 v1: no cross-file MOVED scan)
            delta_evidences.push(DeltaEvidence {
                ordinal: ev.ordinal,
                old_path: ev.relative_path.clone(),
                current_path: None,
                old_start_line: ev.start_line,
                current_start_line: None,
                old_content_hash: ev.delivered_content_hash.clone(),
                current_content_hash: None,
                classification: EvidenceClassification::Removed,
                symbol: ev.symbol_name.clone(),
                reason: "indexed file no longer exists".into(),
                location_changed: false,
            });
            removed += 1;
            continue;
        }
        // Contract check — unsafe provenances are always unverifiable
        if contract == ReconstructionContract::Unsafe {
            delta_evidences.push(DeltaEvidence {
                ordinal: ev.ordinal,
                old_path: ev.relative_path.clone(),
                current_path: Some(ev.relative_path.clone()),
                old_start_line: ev.start_line,
                current_start_line: None,
                old_content_hash: ev.delivered_content_hash.clone(),
                current_content_hash: None,
                classification: EvidenceClassification::Unverifiable,
                symbol: ev.symbol_name.clone(),
                reason:
                    "historical evidence representation cannot be reconstructed deterministically"
                        .into(),
                location_changed: false,
            });
            unverifiable += 1;
            continue;
        }
        // Deterministic contracts: same-file exact match with cardinality
        let cardinality = match contract {
            ReconstructionContract::Direct => {
                find_direct_matches(root, &ev.relative_path, &ev.delivered_content_hash)
            }
            ReconstructionContract::Symbol => find_symbol_matches(
                &structural_conn,
                root,
                &ev.relative_path,
                &ev.symbol_name,
                &ev.delivered_content_hash,
            ),
            ReconstructionContract::Unsafe => unreachable!(),
        };
        match cardinality {
            ExactMatchCardinality::Unique {
                start_line,
                snippet: _,
            } => {
                let location_changed = Some(start_line) != ev.start_line;
                let current_hash = ev.delivered_content_hash.clone(); // historical hash equals current unique match hash
                delta_evidences.push(DeltaEvidence {
                    ordinal: ev.ordinal,
                    old_path: ev.relative_path.clone(),
                    current_path: Some(ev.relative_path.clone()),
                    old_start_line: ev.start_line,
                    current_start_line: Some(start_line),
                    old_content_hash: ev.delivered_content_hash.clone(),
                    current_content_hash: Some(current_hash),
                    classification: EvidenceClassification::Unchanged,
                    symbol: ev.symbol_name.clone(),
                    reason: if location_changed {
                        "exact match elsewhere in same file"
                    } else {
                        "exact delivered hash matched at same location"
                    }
                    .into(),
                    location_changed,
                });
                unchanged += 1;
                continue;
            }
            ExactMatchCardinality::Ambiguous { count: _ } => {
                delta_evidences.push(DeltaEvidence {
                    ordinal: ev.ordinal,
                    old_path: ev.relative_path.clone(),
                    current_path: Some(ev.relative_path.clone()),
                    old_start_line: ev.start_line,
                    current_start_line: None,
                    old_content_hash: ev.delivered_content_hash.clone(),
                    current_content_hash: None,
                    classification: EvidenceClassification::Unverifiable,
                    symbol: ev.symbol_name.clone(),
                    reason: "ambiguous multiple matches for delivered hash in historical file"
                        .into(),
                    location_changed: false,
                });
                unverifiable += 1;
                continue;
            }
            ExactMatchCardinality::NoMatch => {
                // No exact match in same file — try to determine CHANGED vs UNVERIFIABLE
                match contract {
                    ReconstructionContract::Direct => {
                        // For rust:exact, the representation is the line at historical start_line.
                        // If that line still exists, we can compare its current hash to historical.
                        if let Some(hist_line) = ev.start_line {
                            if let Some((_snippet, cur_hash)) =
                                current_line_hash(root, &ev.relative_path, hist_line)
                            {
                                // Found a current line at same historical location, but its hash differs (since NoMatch)
                                // This is deterministic CHANGED for direct contract.
                                delta_evidences.push(DeltaEvidence {
                                    ordinal: ev.ordinal,
                                    old_path: ev.relative_path.clone(),
                                    current_path: Some(ev.relative_path.clone()),
                                    old_start_line: ev.start_line,
                                    current_start_line: Some(hist_line),
                                    old_content_hash: ev.delivered_content_hash.clone(),
                                    current_content_hash: Some(cur_hash),
                                    classification: EvidenceClassification::Changed,
                                    symbol: ev.symbol_name.clone(),
                                    reason: "same location content differs".into(),
                                    location_changed: false,
                                });
                                changed += 1;
                                continue;
                            }
                        }
                        delta_evidences.push(DeltaEvidence {
                            ordinal: ev.ordinal,
                            old_path: ev.relative_path.clone(),
                            current_path: Some(ev.relative_path.clone()),
                            old_start_line: ev.start_line,
                            current_start_line: None,
                            old_content_hash: ev.delivered_content_hash.clone(),
                            current_content_hash: None,
                            classification: EvidenceClassification::Unverifiable,
                            symbol: ev.symbol_name.clone(),
                            reason:
                                "historical delivered hash not found uniquely in historical file"
                                    .into(),
                            location_changed: false,
                        });
                        unverifiable += 1;
                        continue;
                    }
                    ReconstructionContract::Symbol => {
                        // For rust:symbol, look for unique definition in same file
                        let Some(name) = ev.symbol_name.clone() else {
                            delta_evidences.push(DeltaEvidence {
                                ordinal: ev.ordinal,
                                old_path: ev.relative_path.clone(),
                                current_path: Some(ev.relative_path.clone()),
                                old_start_line: ev.start_line,
                                current_start_line: None,
                                old_content_hash: ev.delivered_content_hash.clone(),
                                current_content_hash: None,
                                classification: EvidenceClassification::Unverifiable,
                                symbol: ev.symbol_name.clone(),
                                reason: "historical evidence representation cannot be reconstructed deterministically".into(),
                                location_changed: false,
                            });
                            unverifiable += 1;
                            continue;
                        };
                        let defs =
                            query_symbols_in_file(&structural_conn, &ev.relative_path, &name)
                                .unwrap_or_default();
                        if defs.len() == 1 {
                            // Unique current definition exists but hash differs (since NoMatch) => CHANGED
                            if let Some((cur_line, _snippet, cur_hash)) =
                                current_symbol_hash_and_line(
                                    &structural_conn,
                                    root,
                                    &ev.relative_path,
                                    &name,
                                )
                            {
                                delta_evidences.push(DeltaEvidence {
                                    ordinal: ev.ordinal,
                                    old_path: ev.relative_path.clone(),
                                    current_path: Some(ev.relative_path.clone()),
                                    old_start_line: ev.start_line,
                                    current_start_line: Some(cur_line),
                                    old_content_hash: ev.delivered_content_hash.clone(),
                                    current_content_hash: Some(cur_hash),
                                    classification: EvidenceClassification::Changed,
                                    symbol: ev.symbol_name.clone(),
                                    reason: "same location content differs".into(),
                                    location_changed: false,
                                });
                                changed += 1;
                                continue;
                            }
                        } else if defs.len() > 1 {
                            delta_evidences.push(DeltaEvidence {
                                ordinal: ev.ordinal,
                                old_path: ev.relative_path.clone(),
                                current_path: Some(ev.relative_path.clone()),
                                old_start_line: ev.start_line,
                                current_start_line: None,
                                old_content_hash: ev.delivered_content_hash.clone(),
                                current_content_hash: None,
                                classification: EvidenceClassification::Unverifiable,
                                symbol: ev.symbol_name.clone(),
                                reason: "ambiguous symbol definitions in historical file".into(),
                                location_changed: false,
                            });
                            unverifiable += 1;
                            continue;
                        }
                        // 0 definitions or other ambiguous case
                        delta_evidences.push(DeltaEvidence {
                            ordinal: ev.ordinal,
                            old_path: ev.relative_path.clone(),
                            current_path: Some(ev.relative_path.clone()),
                            old_start_line: ev.start_line,
                            current_start_line: None,
                            old_content_hash: ev.delivered_content_hash.clone(),
                            current_content_hash: None,
                            classification: EvidenceClassification::Unverifiable,
                            symbol: ev.symbol_name.clone(),
                            reason: if defs.is_empty() {
                                "symbol no longer present in historical file".into()
                            } else {
                                "historical delivered hash not found uniquely in historical file"
                                    .into()
                            },
                            location_changed: false,
                        });
                        unverifiable += 1;
                        continue;
                    }
                    ReconstructionContract::Unsafe => unreachable!(),
                }
            }
        }
    }
    let summary = DeltaSummary {
        unchanged,
        changed,
        removed,
        moved: moved_cnt,
        unverifiable,
    };
    // Ensure summary invariant
    debug_assert_eq!(
        summary.unchanged
            + summary.changed
            + summary.removed
            + summary.moved
            + summary.unverifiable,
        delta_evidences.len()
    );
    Ok(ContextDelta {
        proof_id: proof.id.clone(),
        repo_instance_id: proof.repo_instance_id.clone(),
        from_source_state_hash: proof.source_state_hash.clone(),
        to_source_state_hash: Some(current_state.hash.clone()),
        from_generation: proof.source_generation,
        to_generation: Some(current_state.generation),
        status: DeltaStatus::Ready,
        summary,
        evidence: delta_evidences,
        query: proof.query.clone(),
        packed_context_hash: proof.packed_context_hash.clone(),
        reason: "changed snapshot".into(),
    })
}
