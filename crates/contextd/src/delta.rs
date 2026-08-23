#![allow(dead_code)]
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::path::Path;

use crate::proof::{self, ProofEvidence};
use context_index::structural::store as structural_store;
use context_rank::packer::{delivered_hash, delivered_snippet};

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

fn current_file_exists_and_hash(root: &Path, rel: &str) -> Option<String> {
    let conn = structural_store::open_db(root).ok()?;
    structural_store::get_file_hash(&conn, rel).ok().flatten()
}

fn reconstruct_current_snippet(root: &Path, evidence: &ProofEvidence) -> Option<String> {
    // Try symbol first if available and provenance suggests symbol
    if let Some(sym) = &evidence.symbol_name {
        // Try to find symbol definition
        if let Ok(conn) = structural_store::open_db(root) {
            if let Ok(defs) = structural_store::find_definitions(&conn, sym) {
                // Find best matching file (prefer old path)
                for def in &defs {
                    if def.file == evidence.relative_path {
                        // Load symbol snippet
                        let p = root.join(&def.file);
                        if let Ok(content) = std::fs::read_to_string(&p) {
                            let start = def.start_byte.min(content.len());
                            let end = def.end_byte.min(content.len());
                            if end > start {
                                if let Some(slice) = content.get(start..end) {
                                    let snippet = delivered_snippet(Some(slice));
                                    if !snippet.is_empty() {
                                        return Some(snippet);
                                    }
                                }
                            }
                        }
                    }
                }
                // Fallback to any definition of that symbol
                if let Some(def) = defs.into_iter().next() {
                    let p = root.join(&def.file);
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        let start = def.start_byte.min(content.len());
                        let end = def.end_byte.min(content.len());
                        if end > start {
                            if let Some(slice) = content.get(start..end) {
                                let snippet = delivered_snippet(Some(slice));
                                if !snippet.is_empty() {
                                    return Some(snippet);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Try file line based
    let path = root.join(&evidence.relative_path);
    if let Some(line) = evidence.start_line {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = content.lines().collect();
            let idx = (line as usize).saturating_sub(1);
            if idx < lines.len() {
                // For start_line, take that line's content as snippet basis? But original Evidence.text for non-symbol is line content or chunk
                // Use the line's content
                let snippet = delivered_snippet(Some(lines[idx]));
                if !snippet.is_empty() {
                    return Some(snippet);
                }
            }
            // Fallback to whole file first 120 chars?
            let snippet = delivered_snippet(Some(&content));
            if !snippet.is_empty() {
                return Some(snippet);
            }
        }
    } else {
        // No line, try reading file
        if let Ok(content) = std::fs::read_to_string(&path) {
            let snippet = delivered_snippet(Some(&content));
            if !snippet.is_empty() {
                return Some(snippet);
            }
        }
    }
    None
}

fn find_snippet_in_file(root: &Path, rel: &str, target_hash: &str) -> Option<(u32, String)> {
    let path = root.join(rel);
    let content = std::fs::read_to_string(&path).ok()?;
    // Try per-line matching: each line's snippet hash
    for (idx, line) in content.lines().enumerate() {
        let snippet = delivered_snippet(Some(line));
        let h = delivered_hash(&snippet);
        if h == target_hash {
            return Some(((idx as u32) + 1, snippet));
        }
    }
    // Try per-chunk via structural chunks
    if let Ok(conn) = structural_store::open_db(root) {
        if let Ok(chunks) = load_chunks_for_file(&conn, rel) {
            for ch in chunks {
                let p = root.join(&ch.file);
                if let Ok(file_content) = std::fs::read_to_string(&p) {
                    let start = ch.start_byte.min(file_content.len());
                    let end = ch.end_byte.min(file_content.len());
                    if end > start {
                        if let Some(slice) = file_content.get(start..end) {
                            let snippet = delivered_snippet(Some(slice));
                            let h = delivered_hash(&snippet);
                            if h == target_hash {
                                return Some((ch.start_line, snippet));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn load_chunks_for_file(
    conn: &rusqlite::Connection,
    file: &str,
) -> Result<Vec<context_index::structural::types::Chunk>> {
    let mut stmt = conn.prepare("SELECT id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes FROM chunks WHERE file=?1")?;
    let rows = stmt.query_map(rusqlite::params![file], |row| {
        Ok(context_index::structural::types::Chunk {
            id: row.get(0)?,
            file: row.get(1)?,
            language: {
                let s: String = row.get(2)?;
                context_index::structural::language::Language::from_str(&s)
            },
            start_line: row.get(3)?,
            end_line: row.get(4)?,
            start_byte: row.get::<_, i64>(5)? as usize,
            end_byte: row.get::<_, i64>(6)? as usize,
            parent_symbol: row.get(7)?,
            content_hash: row.get(8)?,
            text_size_bytes: row.get::<_, i64>(9)? as usize,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn is_synthetic(ev: &ProofEvidence) -> bool {
    // Synthetic if provenance suggests graph/test and no reliable file/line
    if let Some(prov) = &ev.provenance {
        if prov.contains("graph") || prov.contains("test") {
            // But graph evidence still has file/line, we consider it reconstructible via symbol
            // For P2 v1, we treat synthetic as unverifiable if no file exists or symbol missing
            // For now, only mark as synthetic if file has no valid path
            if ev.relative_path.is_empty() {
                return true;
            }
        }
    }
    false
}

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
        // No need to inspect evidence
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
    // Changed snapshot path - evidence-level comparison
    let evidences = proof::load_evidence(&proof_conn, proof_id)?;
    let mut delta_evidences = Vec::new();
    let mut unchanged = 0;
    let mut changed = 0;
    let mut removed = 0;
    let mut moved_cnt = 0;
    let mut unverifiable = 0;
    for ev in evidences {
        // Synthetic check
        if is_synthetic(&ev) {
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
                reason: "synthetic evidence cannot be reconstructed".into(),
                location_changed: false,
            });
            unverifiable += 1;
            continue;
        }
        // File existence check via indexed state
        let current_hash_opt = current_file_exists_and_hash(root, &ev.relative_path);
        if current_hash_opt.is_none() {
            // File no longer exists in index - try moved detection (cross-file unique)
            // For P2 v1, defer cross-file moved to P2.1, so treat as REMOVED
            // But we attempt bounded cross-file search for unique exact delivered hash
            // Search all indexed files for matching delivered hash (expensive but bounded to 10 evidence)
            let mut found = None;
            let mut ambiguity = 0;
            // Try to find unique file with same delivered hash via scanning indexed files
            // List files via structural store
            if let Ok(conn) = structural_store::open_db(root) {
                if let Ok(files) = structural_store::list_files(&conn) {
                    for (path, _hash) in files {
                        if let Some((line, _snippet)) =
                            find_snippet_in_file(root, &path, &ev.delivered_content_hash)
                        {
                            if found.is_none() {
                                found = Some((path.clone(), line));
                            } else {
                                ambiguity += 1;
                                break;
                            }
                        }
                    }
                }
            }
            if let Some((new_path, new_line)) = found {
                if ambiguity == 0 {
                    // Unique moved
                    delta_evidences.push(DeltaEvidence {
                        ordinal: ev.ordinal,
                        old_path: ev.relative_path.clone(),
                        current_path: Some(new_path.clone()),
                        old_start_line: ev.start_line,
                        current_start_line: Some(new_line),
                        old_content_hash: ev.delivered_content_hash.clone(),
                        current_content_hash: Some(ev.delivered_content_hash.clone()),
                        classification: EvidenceClassification::Moved,
                        symbol: ev.symbol_name.clone(),
                        reason: "unique cross-file exact match".into(),
                        location_changed: true,
                    });
                    moved_cnt += 1;
                    continue;
                } else {
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
                        reason: "ambiguous multiple matches for moved".into(),
                        location_changed: false,
                    });
                    unverifiable += 1;
                    continue;
                }
            }
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
        // File exists - try same location reconstruction
        if let Some(current_snippet) = reconstruct_current_snippet(root, &ev) {
            let current_hash = delivered_hash(&current_snippet);
            if current_hash == ev.delivered_content_hash {
                // Check if location shifted within same file via find
                let mut location_changed = false;
                let mut current_line = ev.start_line;
                if ev.start_line.is_some() {
                    if let Some((found_line, _)) =
                        find_snippet_in_file(root, &ev.relative_path, &ev.delivered_content_hash)
                    {
                        if Some(found_line) != ev.start_line {
                            location_changed = true;
                            current_line = Some(found_line);
                        }
                    }
                }
                delta_evidences.push(DeltaEvidence {
                    ordinal: ev.ordinal,
                    old_path: ev.relative_path.clone(),
                    current_path: Some(ev.relative_path.clone()),
                    old_start_line: ev.start_line,
                    current_start_line: current_line,
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
            } else {
                // Same location but content differs -> check if same snippet exists elsewhere in same file (location shift)
                if let Some((found_line, _)) =
                    find_snippet_in_file(root, &ev.relative_path, &ev.delivered_content_hash)
                {
                    delta_evidences.push(DeltaEvidence {
                        ordinal: ev.ordinal,
                        old_path: ev.relative_path.clone(),
                        current_path: Some(ev.relative_path.clone()),
                        old_start_line: ev.start_line,
                        current_start_line: Some(found_line),
                        old_content_hash: ev.delivered_content_hash.clone(),
                        current_content_hash: Some(ev.delivered_content_hash.clone()),
                        classification: EvidenceClassification::Unchanged,
                        symbol: ev.symbol_name.clone(),
                        reason: "exact match elsewhere in same file".into(),
                        location_changed: true,
                    });
                    unchanged += 1;
                    continue;
                }
                // Content changed at same location
                delta_evidences.push(DeltaEvidence {
                    ordinal: ev.ordinal,
                    old_path: ev.relative_path.clone(),
                    current_path: Some(ev.relative_path.clone()),
                    old_start_line: ev.start_line,
                    current_start_line: ev.start_line,
                    old_content_hash: ev.delivered_content_hash.clone(),
                    current_content_hash: Some(current_hash),
                    classification: EvidenceClassification::Changed,
                    symbol: ev.symbol_name.clone(),
                    reason: "same location content differs".into(),
                    location_changed: false,
                });
                changed += 1;
                continue;
            }
        } else {
            // Could not reconstruct - try bounded same-file search for exact hash
            if let Some((found_line, _)) =
                find_snippet_in_file(root, &ev.relative_path, &ev.delivered_content_hash)
            {
                delta_evidences.push(DeltaEvidence {
                    ordinal: ev.ordinal,
                    old_path: ev.relative_path.clone(),
                    current_path: Some(ev.relative_path.clone()),
                    old_start_line: ev.start_line,
                    current_start_line: Some(found_line),
                    old_content_hash: ev.delivered_content_hash.clone(),
                    current_content_hash: Some(ev.delivered_content_hash.clone()),
                    classification: EvidenceClassification::Unchanged,
                    symbol: ev.symbol_name.clone(),
                    reason: "found exact snippet elsewhere same file".into(),
                    location_changed: true,
                });
                unchanged += 1;
                continue;
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
                reason: "cannot reconstruct current snippet".into(),
                location_changed: false,
            });
            unverifiable += 1;
            continue;
        }
    }
    let summary = DeltaSummary {
        unchanged,
        changed,
        removed,
        moved: moved_cnt,
        unverifiable,
    };
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
