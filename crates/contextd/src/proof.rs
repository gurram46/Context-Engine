#![allow(dead_code)]
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub const PROOF_SCHEMA_VERSION: i32 = 1;

pub fn proof_db_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".context")
        .join("proofs")
        .join("proofs.db")
}

pub fn ensure_proof_dir(project_root: &Path) -> Result<PathBuf> {
    let dir = project_root.join(".context").join("proofs");
    std::fs::create_dir_all(&dir)?;
    let gitignore = project_root.join(".context").join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(&gitignore, "*\n");
    }
    Ok(dir)
}

pub fn open_proof_db(project_root: &Path) -> Result<Connection> {
    ensure_proof_dir(project_root)?;
    let db_path = proof_db_path(project_root);
    let conn = Connection::open(&db_path)?;
    init_proof_schema(&conn)?;
    Ok(conn)
}

fn init_proof_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        PRAGMA foreign_keys=ON;
        PRAGMA busy_timeout=2000;
        CREATE TABLE IF NOT EXISTS proof_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS proofs (
            id TEXT PRIMARY KEY,
            schema_version INTEGER NOT NULL,
            repo_instance_id TEXT NOT NULL,
            canonical_root TEXT NOT NULL,
            source_generation INTEGER NOT NULL,
            source_state_hash TEXT NOT NULL,
            source_state_verified INTEGER NOT NULL,
            git_branch TEXT,
            git_commit TEXT,
            git_detached INTEGER NOT NULL,
            git_dirty INTEGER,
            query TEXT NOT NULL,
            query_type TEXT,
            tool_name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            packed_context_hash TEXT NOT NULL,
            engine_version TEXT NOT NULL,
            index_schema_version INTEGER,
            extraction_version INTEGER,
            semantic_representation_version TEXT,
            embedding_model_id TEXT,
            embedding_model_version TEXT,
            embedding_dimension INTEGER,
            packed_token_estimate INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_proofs_repo ON proofs(repo_instance_id);
        CREATE INDEX IF NOT EXISTS idx_proofs_hash ON proofs(source_state_hash);
        CREATE TABLE IF NOT EXISTS proof_evidence (
            proof_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            relative_path TEXT NOT NULL,
            start_line INTEGER,
            end_line INTEGER,
            start_byte INTEGER,
            end_byte INTEGER,
            delivered_content_hash TEXT NOT NULL,
            chunk_id TEXT,
            chunk_content_hash TEXT,
            representation_hash TEXT,
            symbol_id TEXT,
            symbol_name TEXT,
            qualified_name TEXT,
            symbol_kind TEXT,
            provenance TEXT,
            relation TEXT,
            final_score REAL,
            PRIMARY KEY (proof_id, ordinal),
            FOREIGN KEY (proof_id) REFERENCES proofs(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_evidence_proof ON proof_evidence(proof_id);
        "#,
    )?;
    let existing: Option<i32> = conn
        .query_row(
            "SELECT value FROM proof_meta WHERE key='schema_version'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|s| s.parse::<i32>().ok());
    // Simplify: store schema version if not exists
    if existing.is_none() {
        conn.execute(
            "INSERT OR REPLACE INTO proof_meta (key, value) VALUES ('schema_version', ?1)",
            params![PROOF_SCHEMA_VERSION.to_string()],
        )?;
    } else if let Some(v) = existing {
        if v != PROOF_SCHEMA_VERSION {
            // For now, if version mismatch, bail
            anyhow::bail!(
                "unsupported proof schema version {} (current {})",
                v,
                PROOF_SCHEMA_VERSION
            );
        }
    }
    Ok(())
}

pub fn open_proof_db_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init_proof_schema(&conn)?;
    Ok(conn)
}

#[derive(Debug, Clone)]
pub struct GitSnapshot {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub detached: bool,
    pub dirty: Option<bool>,
}

pub fn git_snapshot(project_root: &Path) -> GitSnapshot {
    // Try to get branch and commit
    // If not a git repo, return nulls
    let branch_out = std::process::Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output();
    let commit_out = std::process::Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output();
    let dirty_out = std::process::Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("status")
        .arg("--porcelain")
        .output();

    match (branch_out, commit_out) {
        (Ok(b), Ok(c)) if b.status.success() && c.status.success() => {
            let branch_raw = String::from_utf8_lossy(&b.stdout).trim().to_string();
            let commit_raw = String::from_utf8_lossy(&c.stdout).trim().to_string();
            let detached = branch_raw == "HEAD";
            let branch = if detached { None } else { Some(branch_raw) };
            let commit = if commit_raw.is_empty() {
                None
            } else {
                Some(commit_raw)
            };
            let dirty = match dirty_out {
                Ok(d) if d.status.success() => {
                    let out = String::from_utf8_lossy(&d.stdout);
                    Some(!out.trim().is_empty())
                }
                _ => None,
            };
            GitSnapshot {
                branch,
                commit,
                detached,
                dirty,
            }
        }
        _ => GitSnapshot {
            branch: None,
            commit: None,
            detached: false,
            dirty: None,
        },
    }
}

#[derive(Debug, Clone)]
pub struct ProofRecord {
    pub id: String,
    pub schema_version: i32,
    pub repo_instance_id: String,
    pub canonical_root: String,
    pub source_generation: u64,
    pub source_state_hash: String,
    pub source_state_verified: bool,
    pub git_branch: Option<String>,
    pub git_commit: Option<String>,
    pub git_detached: bool,
    pub git_dirty: Option<bool>,
    pub query: String,
    pub query_type: String,
    pub tool_name: String,
    pub created_at: i64,
    pub packed_context_hash: String,
    pub engine_version: String,
    pub index_schema_version: Option<i32>,
    pub extraction_version: Option<i32>,
    pub semantic_representation_version: Option<String>,
    pub embedding_model_id: Option<String>,
    pub embedding_model_version: Option<String>,
    pub embedding_dimension: Option<i32>,
    pub packed_token_estimate: usize,
}

#[derive(Debug, Clone)]
pub struct ProofEvidence {
    pub proof_id: String,
    pub ordinal: usize,
    pub relative_path: String,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
    pub delivered_content_hash: String,
    pub chunk_id: Option<String>,
    pub chunk_content_hash: Option<String>,
    pub representation_hash: Option<String>,
    pub symbol_id: Option<String>,
    pub symbol_name: Option<String>,
    pub qualified_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub provenance: Option<String>,
    pub relation: Option<String>,
    pub final_score: Option<f64>,
}

pub fn generate_proof_id() -> String {
    let u = uuid::Uuid::now_v7();
    format!("cp_{}", u)
}

pub fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn insert_proof(
    conn: &mut Connection,
    proof: &ProofRecord,
    evidences: &[ProofEvidence],
) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO proofs (id, schema_version, repo_instance_id, canonical_root, source_generation, source_state_hash, source_state_verified, git_branch, git_commit, git_detached, git_dirty, query, query_type, tool_name, created_at, packed_context_hash, engine_version, index_schema_version, extraction_version, semantic_representation_version, embedding_model_id, embedding_model_version, embedding_dimension, packed_token_estimate) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)",
        params![
            proof.id,
            proof.schema_version,
            proof.repo_instance_id,
            proof.canonical_root,
            proof.source_generation as i64,
            proof.source_state_hash,
            if proof.source_state_verified { 1 } else { 0 },
            proof.git_branch,
            proof.git_commit,
            if proof.git_detached { 1 } else { 0 },
            proof.git_dirty.map(|b| if b { 1 } else { 0 }),
            proof.query,
            proof.query_type,
            proof.tool_name,
            proof.created_at,
            proof.packed_context_hash,
            proof.engine_version,
            proof.index_schema_version,
            proof.extraction_version,
            proof.semantic_representation_version,
            proof.embedding_model_id,
            proof.embedding_model_version,
            proof.embedding_dimension,
            proof.packed_token_estimate as i64
        ],
    )?;
    for ev in evidences {
        tx.execute(
            "INSERT INTO proof_evidence (proof_id, ordinal, relative_path, start_line, end_line, start_byte, end_byte, delivered_content_hash, chunk_id, chunk_content_hash, representation_hash, symbol_id, symbol_name, qualified_name, symbol_kind, provenance, relation, final_score) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![
                ev.proof_id,
                ev.ordinal as i64,
                ev.relative_path,
                ev.start_line.map(|v| v as i64),
                ev.end_line.map(|v| v as i64),
                ev.start_byte.map(|v| v as i64),
                ev.end_byte.map(|v| v as i64),
                ev.delivered_content_hash,
                ev.chunk_id,
                ev.chunk_content_hash,
                ev.representation_hash,
                ev.symbol_id,
                ev.symbol_name,
                ev.qualified_name,
                ev.symbol_kind,
                ev.provenance,
                ev.relation,
                ev.final_score
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn load_proof(conn: &Connection, id: &str) -> Result<Option<ProofRecord>> {
    let mut stmt = conn.prepare("SELECT id, schema_version, repo_instance_id, canonical_root, source_generation, source_state_hash, source_state_verified, git_branch, git_commit, git_detached, git_dirty, query, query_type, tool_name, created_at, packed_context_hash, engine_version, index_schema_version, extraction_version, semantic_representation_version, embedding_model_id, embedding_model_version, embedding_dimension, packed_token_estimate FROM proofs WHERE id=?1")?;
    let opt = stmt
        .query_row(params![id], |row| {
            Ok(ProofRecord {
                id: row.get(0)?,
                schema_version: row.get(1)?,
                repo_instance_id: row.get(2)?,
                canonical_root: row.get(3)?,
                source_generation: row.get::<_, i64>(4)? as u64,
                source_state_hash: row.get(5)?,
                source_state_verified: row.get::<_, i64>(6)? != 0,
                git_branch: row.get(7)?,
                git_commit: row.get(8)?,
                git_detached: row.get::<_, i64>(9)? != 0,
                git_dirty: row.get::<_, Option<i64>>(10)?.map(|v| v != 0),
                query: row.get(11)?,
                query_type: row.get(12)?,
                tool_name: row.get(13)?,
                created_at: row.get::<_, i64>(14)?,
                packed_context_hash: row.get(15)?,
                engine_version: row.get(16)?,
                index_schema_version: row.get(17)?,
                extraction_version: row.get(18)?,
                semantic_representation_version: row.get(19)?,
                embedding_model_id: row.get(20)?,
                embedding_model_version: row.get(21)?,
                embedding_dimension: row.get(22)?,
                packed_token_estimate: row.get::<_, i64>(23)? as usize,
            })
        })
        .optional()?;
    Ok(opt)
}

pub fn count_proofs(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM proofs", [], |r| r.get(0))?)
}

pub fn load_evidence(conn: &Connection, proof_id: &str) -> Result<Vec<ProofEvidence>> {
    let mut stmt = conn.prepare("SELECT proof_id, ordinal, relative_path, start_line, end_line, start_byte, end_byte, delivered_content_hash, chunk_id, chunk_content_hash, representation_hash, symbol_id, symbol_name, qualified_name, symbol_kind, provenance, relation, final_score FROM proof_evidence WHERE proof_id=?1 ORDER BY ordinal ASC")?;
    let rows = stmt.query_map(params![proof_id], |row| {
        Ok(ProofEvidence {
            proof_id: row.get(0)?,
            ordinal: row.get::<_, i64>(1)? as usize,
            relative_path: row.get(2)?,
            start_line: row.get::<_, Option<i64>>(3)?.map(|v| v as u32),
            end_line: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
            start_byte: row.get::<_, Option<i64>>(5)?.map(|v| v as usize),
            end_byte: row.get::<_, Option<i64>>(6)?.map(|v| v as usize),
            delivered_content_hash: row.get(7)?,
            chunk_id: row.get(8)?,
            chunk_content_hash: row.get(9)?,
            representation_hash: row.get(10)?,
            symbol_id: row.get(11)?,
            symbol_name: row.get(12)?,
            qualified_name: row.get(13)?,
            symbol_kind: row.get(14)?,
            provenance: row.get(15)?,
            relation: row.get(16)?,
            final_score: row.get(17)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validity {
    Current,
    Stale,
    Unverifiable,
}

impl Validity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Unverifiable => "unverifiable",
        }
    }
}

/// Strict snapshot validation
pub fn validate_proof(project_root: &Path, proof: &ProofRecord) -> Validity {
    // Schema check
    if proof.schema_version != PROOF_SCHEMA_VERSION {
        return Validity::Unverifiable;
    }
    // Repo instance check
    let current_repo_id = context_index::project_root::repo_instance_id(project_root);
    // Also compute via canonical if project_root is canonical; but we use direct path
    // For comparison, we need canonical root's hash. Use provided canonical_root's hash as stored vs current.
    // Recompute current via canonicalized path
    let canon_current = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let current_id_via_canon = context_index::project_root::repo_instance_id(&canon_current);
    if proof.repo_instance_id != current_repo_id && proof.repo_instance_id != current_id_via_canon {
        return Validity::Unverifiable;
    }
    // Load current source state
    let current_state = match get_current_source_state(project_root) {
        Ok(s) => s,
        Err(_) => return Validity::Unverifiable,
    };
    if !current_state.verified {
        return Validity::Unverifiable;
    }
    if !proof.source_state_verified {
        return Validity::Unverifiable;
    }
    // Git branch boundary
    let current_git = git_snapshot(project_root);
    // Branch check: normal branch requires same branch name; detached requires same commit+detached
    if proof.git_detached != current_git.detached {
        return Validity::Stale;
    }
    if proof.git_detached {
        if proof.git_commit != current_git.commit {
            return Validity::Stale;
        }
    } else {
        if proof.git_branch != current_git.branch {
            return Validity::Stale;
        }
    }
    // Source hash check (strict snapshot)
    if proof.source_state_hash != current_state.hash {
        return Validity::Stale;
    }
    Validity::Current
}

pub struct CurrentState {
    pub hash: String,
    pub generation: u64,
    pub verified: bool,
}

pub fn get_current_source_state(project_root: &Path) -> Result<CurrentState> {
    let conn = context_index::structural::store::open_db(project_root)?;
    let hash = context_index::structural::store::get_source_state_hash(&conn)?
        .ok_or_else(|| anyhow::anyhow!("source_state_hash missing"))?;
    let gen = context_index::structural::store::get_generation(&conn).unwrap_or(0);
    let verified = context_index::structural::store::get_source_state_verified(&conn)
        .unwrap_or(Some(false))
        .unwrap_or(false);
    Ok(CurrentState {
        hash,
        generation: gen,
        verified,
    })
}

/// Build proof record from runtime snapshot + packed result
#[allow(clippy::too_many_arguments)]
pub fn build_proof_record(
    _project_root: &Path,
    canonical_root: &str,
    repo_instance_id: &str,
    generation: u64,
    source_state_hash: &str,
    source_state_verified: bool,
    git: GitSnapshot,
    query: &str,
    query_type: &str,
    packed_token_estimate: usize,
    packed_context_hash: &str,
) -> ProofRecord {
    let fp = context_index::embed::configured_fingerprint();
    ProofRecord {
        id: generate_proof_id(),
        schema_version: PROOF_SCHEMA_VERSION,
        repo_instance_id: repo_instance_id.to_string(),
        canonical_root: canonical_root.to_string(),
        source_generation: generation,
        source_state_hash: source_state_hash.to_string(),
        source_state_verified,
        git_branch: git.branch,
        git_commit: git.commit,
        git_detached: git.detached,
        git_dirty: git.dirty,
        query: query.to_string(),
        query_type: query_type.to_string(),
        tool_name: "context_search".to_string(),
        created_at: now_unix_ms(),
        packed_context_hash: packed_context_hash.to_string(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        index_schema_version: Some(context_index::structural::store::SCHEMA_VERSION),
        extraction_version: Some(context_index::structural::STRUCTURAL_EXTRACTION_VERSION as i32),
        semantic_representation_version: Some(
            context_index::vector::SEMANTIC_REPRESENTATION_VERSION.to_string(),
        ),
        embedding_model_id: Some(fp.model_id.clone()),
        embedding_model_version: Some(fp.version.clone()),
        embedding_dimension: Some(fp.dimension as i32),
        packed_token_estimate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_tables_create() {
        let conn = open_proof_db_in_memory().unwrap();
        let cnt = count_proofs(&conn).unwrap();
        assert_eq!(cnt, 0);
    }
    #[test]
    fn insert_and_load() {
        let mut conn = open_proof_db_in_memory().unwrap();
        let proof = ProofRecord {
            id: "cp_test".into(),
            schema_version: 1,
            repo_instance_id: "repo1".into(),
            canonical_root: "/tmp/repo".into(),
            source_generation: 1,
            source_state_hash: "hash1".into(),
            source_state_verified: true,
            git_branch: Some("main".into()),
            git_commit: Some("abc".into()),
            git_detached: false,
            git_dirty: Some(false),
            query: "test".into(),
            query_type: "EXACT".into(),
            tool_name: "context_search".into(),
            created_at: 0,
            packed_context_hash: blake3::hash(b"markdown").to_hex().to_string(),
            engine_version: "0.1.0".into(),
            index_schema_version: Some(3),
            extraction_version: Some(2),
            semantic_representation_version: Some("v2".into()),
            embedding_model_id: Some("all-minilm".into()),
            embedding_model_version: Some("v1".into()),
            embedding_dimension: Some(384),
            packed_token_estimate: 100,
        };
        let ev = ProofEvidence {
            proof_id: "cp_test".into(),
            ordinal: 0,
            relative_path: "a.py".into(),
            start_line: Some(1),
            end_line: Some(2),
            start_byte: None,
            end_byte: None,
            delivered_content_hash: blake3::hash(b"snippet").to_hex().to_string(),
            chunk_id: None,
            chunk_content_hash: None,
            representation_hash: None,
            symbol_id: None,
            symbol_name: Some("foo".into()),
            qualified_name: None,
            symbol_kind: None,
            provenance: Some("rust:exact".into()),
            relation: None,
            final_score: Some(1.0),
        };
        insert_proof(&mut conn, &proof, &[ev]).unwrap();
        let loaded = load_proof(&conn, "cp_test").unwrap().unwrap();
        assert_eq!(loaded.id, "cp_test");
        let evs = load_evidence(&conn, "cp_test").unwrap();
        assert_eq!(evs.len(), 1);
    }
}
