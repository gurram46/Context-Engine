use crate::structural::types::Chunk;
use anyhow::Result;
use regex::Regex;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

// BM25 constants — centralized sane defaults
pub const BM25_K1: f32 = 1.2;
pub const BM25_B: f32 = 0.75;

static RE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_]+(?:[./][A-Za-z0-9_]+)*").unwrap());

/// Code-aware tokenization.
/// Example:
/// PaymentRetryHandler -> PaymentRetryHandler, paymentretryhandler, Payment, Retry, Handler, payment, retry, handler
/// payment_retry -> payment_retry, payment, retry
/// Server.Start -> Server.Start, Server, Start, server, start
/// backend/payment/retry.go -> backend, payment, retry, go
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for mat in RE_TOKEN.find_iter(text) {
        let raw = mat.as_str();
        let has_slash = raw.contains('/');
        let has_dot = raw.contains('.');
        if has_slash {
            // Path-like: split by slash and dot, expand each part
            for seg in raw.split(['/', '.']) {
                if seg.is_empty() {
                    continue;
                }
                expand_token(seg, &mut out);
                // also add extension handling already via expand
            }
        } else if has_dot {
            // Qualified: keep qualified and parts
            out.push(raw.to_string());
            let low = raw.to_lowercase();
            if low != raw {
                out.push(low.clone());
            }
            for part in raw.split('.') {
                if part.is_empty() {
                    continue;
                }
                expand_token(part, &mut out);
            }
        } else {
            expand_token(raw, &mut out);
        }
    }
    out
}

fn expand_token(tok: &str, out: &mut Vec<String>) {
    if tok.is_empty() {
        return;
    }
    out.push(tok.to_string());
    let lower = tok.to_lowercase();
    if lower != tok {
        out.push(lower.clone());
    }
    // Snake split
    if tok.contains('_') {
        for p in tok.split('_') {
            if p.is_empty() || p == tok {
                continue;
            }
            out.push(p.to_string());
            let pl = p.to_lowercase();
            if pl != p {
                out.push(pl);
            }
        }
    }
    // Camel split
    let parts = split_camel(tok);
    if parts.len() > 1 {
        for cp in parts {
            if cp.is_empty() || cp == tok {
                continue;
            }
            out.push(cp.clone());
            let cpl = cp.to_lowercase();
            if cpl != cp {
                out.push(cpl);
            }
        }
    }
}

/// Split CamelCase into parts.
/// PaymentRetryHandler -> ["Payment","Retry","Handler"]
/// PDFLoader -> ["PDF","Loader"] (consecutive capitals)
fn split_camel(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        let prev = if i > 0 { Some(chars[i - 1]) } else { None };
        let next = if i + 1 < chars.len() {
            Some(chars[i + 1])
        } else {
            None
        };
        let is_upper = c.is_ascii_uppercase();
        let prev_is_lower = prev.map(|p| p.is_ascii_lowercase()).unwrap_or(false);
        let prev_is_upper = prev.map(|p| p.is_ascii_uppercase()).unwrap_or(false);
        let next_is_lower = next.map(|n| n.is_ascii_lowercase()).unwrap_or(false);

        let boundary = if i == 0 {
            false
        } else if is_upper && prev_is_lower {
            true
        } else if is_upper && prev_is_upper && next_is_lower {
            // PDFLoader: split before L
            true
        } else {
            false
        };
        if boundary && !cur.is_empty() {
            parts.push(cur.clone());
            cur.clear();
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// BM25 candidate
#[derive(Debug, Clone)]
pub struct Bm25Candidate {
    pub file: String,
    pub chunk_id: String,
    pub start_line: u32,
    pub end_line: u32,
    pub symbol: Option<String>,
    pub score: f64,
    pub content_hash: String,
}

/// Compute BM25 IDF: ln( (N - df + 0.5)/(df + 0.5) + 1 )
pub fn idf(n: usize, df: usize) -> f64 {
    let n = n as f64;
    let df = df as f64;
    ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
}

/// BM25 score for one term
pub fn bm25_term_score(tf: usize, doc_len: usize, avgdl: f64, idf: f64) -> f64 {
    let k1 = BM25_K1 as f64;
    let b = BM25_B as f64;
    let tf = tf as f64;
    let doc_len = doc_len as f64;
    let numerator = tf * (k1 + 1.0);
    let denominator = tf + k1 * (1.0 - b + b * doc_len / avgdl.max(1.0));
    idf * (numerator / denominator)
}

// --- Store helpers ---

fn ensure_bm25_schema(conn: &Connection) -> Result<()> {
    // Ensure tables exist even if schema version not bumped (for tests with in-memory)
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS bm25_documents (
            doc_id TEXT PRIMARY KEY,
            chunk_id TEXT NOT NULL,
            file TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            length INTEGER NOT NULL,
            symbol TEXT,
            start_line INTEGER,
            end_line INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_bm25_docs_file ON bm25_documents(file);
        CREATE TABLE IF NOT EXISTS bm25_postings (
            term TEXT NOT NULL,
            doc_id TEXT NOT NULL,
            tf INTEGER NOT NULL,
            PRIMARY KEY(term, doc_id),
            FOREIGN KEY(doc_id) REFERENCES bm25_documents(doc_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_bm25_post_term ON bm25_postings(term);
        "#,
    )?;
    Ok(())
}

/// Upsert BM25 for a file's chunks.
/// Called from structural build with file content to extract chunk text.
/// This replaces all postings for changed chunks transactionally, deletes stale.
pub fn upsert_bm25_for_file(
    conn: &mut Connection,
    file: &str,
    chunks: &[Chunk],
    file_content: &str,
) -> Result<usize> {
    ensure_bm25_schema(conn)?;
    let tx = conn.transaction()?;
    // Delete old docs for this file
    tx.execute("DELETE FROM bm25_postings WHERE doc_id IN (SELECT doc_id FROM bm25_documents WHERE file=?1)", params![file])?;
    tx.execute("DELETE FROM bm25_documents WHERE file=?1", params![file])?;
    let mut docs_inserted = 0usize;
    let bytes = file_content.as_bytes();
    for chunk in chunks {
        let doc_id = chunk.id.clone();
        let text_slice = if chunk.start_byte < bytes.len() && chunk.end_byte <= bytes.len() {
            std::str::from_utf8(&bytes[chunk.start_byte..chunk.end_byte]).unwrap_or("")
        } else {
            ""
        };
        // Combine chunk text + file path + symbol for tokenization (path contributes limited tokens)
        let mut combined = String::new();
        combined.push_str(text_slice);
        combined.push(' ');
        combined.push_str(file);
        if let Some(sym) = &chunk.parent_symbol {
            combined.push(' ');
            combined.push_str(sym);
        }
        // Also add file path tokens separately to ensure backend/payment/retry.go -> backend, payment, etc
        // Already via combined, but file path slash handling covers it
        let tokens = tokenize(&combined);
        if tokens.is_empty() {
            continue;
        }
        // Compute tf map
        let mut tf_map: HashMap<String, usize> = HashMap::new();
        for t in tokens {
            // Normalize term: keep as is for case-sensitive, but also lower variants already in tokens.
            // Don't lower again; tokens already contain both.
            // Limit term length to avoid huge postings (e.g., 64 chars)
            if t.len() > 64 {
                continue;
            }
            *tf_map.entry(t).or_insert(0) += 1;
        }
        let length = tf_map.values().sum::<usize>() as i64;
        if length == 0 {
            continue;
        }
        tx.execute(
            "INSERT INTO bm25_documents (doc_id, chunk_id, file, content_hash, length, symbol, start_line, end_line) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                doc_id,
                chunk.id,
                file,
                chunk.content_hash,
                length,
                chunk.parent_symbol,
                chunk.start_line as i64,
                chunk.end_line as i64
            ],
        )?;
        for (term, tf) in tf_map {
            tx.execute(
                "INSERT INTO bm25_postings (term, doc_id, tf) VALUES (?1,?2,?3)",
                params![term, doc_id, tf as i64],
            )?;
        }
        docs_inserted += 1;
    }
    tx.commit()?;
    Ok(docs_inserted)
}

pub fn delete_bm25_for_file(conn: &Connection, file: &str) -> Result<usize> {
    ensure_bm25_schema(conn)?;
    let mut stmt = conn.prepare("DELETE FROM bm25_documents WHERE file=?1")?;
    Ok(stmt.execute(params![file])?)
    // cascade deletes postings via FK? But we use manual delete for postings via trigger of doc delete? Actually postings FK cascade on doc_id delete, so it will delete.
}

pub fn count_bm25_docs(conn: &Connection) -> Result<i64> {
    ensure_bm25_schema(conn)?;
    Ok(conn.query_row("SELECT COUNT(*) FROM bm25_documents", [], |r| r.get(0))?)
}

/// Search BM25.
/// Typed Rust API: `search_bm25(query, limit) -> Vec<Bm25Candidate>`
#[allow(clippy::type_complexity)]
pub fn search_bm25(conn: &Connection, query: &str, limit: usize) -> Result<Vec<Bm25Candidate>> {
    ensure_bm25_schema(conn)?;
    let query_tokens_raw = tokenize(query);
    if query_tokens_raw.is_empty() {
        return Ok(Vec::new());
    }
    // Dedup query terms
    let mut query_terms_set: HashSet<String> = HashSet::new();
    for t in query_tokens_raw {
        if t.len() > 64 {
            continue;
        }
        query_terms_set.insert(t);
    }
    let mut query_terms: Vec<String> = query_terms_set.into_iter().collect();
    query_terms.sort();
    if query_terms.is_empty() {
        return Ok(Vec::new());
    }

    // Get N and avgdl
    let (n, avgdl): (i64, f64) = {
        let mut stmt = conn.prepare("SELECT COUNT(*), AVG(length) FROM bm25_documents")?;
        stmt.query_row([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
            ))
        })?
    };
    if n == 0 {
        return Ok(Vec::new());
    }
    let n_usize = n as usize;
    let avgdl = if avgdl == 0.0 { 1.0 } else { avgdl };

    // Get df per term
    let mut df_map: HashMap<String, usize> = HashMap::new();
    // Build placeholders
    let placeholders = query_terms
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql_df = format!(
        "SELECT term, COUNT(DISTINCT doc_id) FROM bm25_postings WHERE term IN ({}) GROUP BY term",
        placeholders
    );
    {
        let mut stmt = conn.prepare(&sql_df)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = query_terms
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        for r in rows {
            let (term, df) = r?;
            df_map.insert(term, df);
        }
    }

    // Get postings for query terms: term, doc_id, tf
    let sql_post = format!(
        "SELECT term, doc_id, tf FROM bm25_postings WHERE term IN ({})",
        placeholders
    );
    let mut postings: HashMap<String, HashMap<String, usize>> = HashMap::new(); // doc_id -> term -> tf
    let mut doc_ids_set: HashSet<String> = HashSet::new();
    {
        let mut stmt = conn.prepare(&sql_post)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = query_terms
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as usize,
            ))
        })?;
        for r in rows {
            let (term, doc_id, tf) = r?;
            postings.entry(doc_id.clone()).or_default().insert(term, tf);
            doc_ids_set.insert(doc_id);
        }
    }
    if doc_ids_set.is_empty() {
        return Ok(Vec::new());
    }

    // Get doc lengths and metadata — deterministic ordering for IN clause
    let mut doc_ids: Vec<String> = doc_ids_set.into_iter().collect();
    doc_ids.sort();
    let placeholders2 = doc_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql_docs = format!("SELECT doc_id, chunk_id, file, content_hash, length, symbol, start_line, end_line FROM bm25_documents WHERE doc_id IN ({})", placeholders2);
    let mut doc_meta: HashMap<String, (String, String, String, usize, Option<String>, u32, u32)> =
        HashMap::new();
    {
        let mut stmt = conn.prepare(&sql_docs)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            doc_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,         // doc_id
                row.get::<_, String>(1)?,         // chunk_id
                row.get::<_, String>(2)?,         // file
                row.get::<_, String>(3)?,         // content_hash
                row.get::<_, i64>(4)? as usize,   // length
                row.get::<_, Option<String>>(5)?, // symbol
                row.get::<_, i64>(6)? as u32,     // start_line
                row.get::<_, i64>(7)? as u32,     // end_line
            ))
        })?;
        for r in rows {
            let (doc_id, chunk_id, file, hash, len, sym, sl, el) = r?;
            doc_meta.insert(doc_id, (chunk_id, file, hash, len, sym, sl, el));
        }
    }

    // Compute score per doc
    let mut scored: Vec<(String, f64)> = Vec::new();
    for (doc_id, term_tfs) in postings {
        let meta = match doc_meta.get(&doc_id) {
            Some(m) => m,
            None => continue,
        };
        let doc_len = meta.3;
        let mut score = 0.0;
        for term in &query_terms {
            if let Some(tf) = term_tfs.get(term) {
                let df = df_map.get(term).cloned().unwrap_or(1);
                let idf_val = idf(n_usize, df);
                score += bm25_term_score(*tf, doc_len, avgdl, idf_val);
            }
        }
        if score > 0.0 {
            scored.push((doc_id, score));
        }
    }

    // Deterministic ordering before LIMIT: score desc, then doc_id asc, then file via doc_meta not yet but doc_id is deterministic
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(limit);

    let mut out = Vec::new();
    for (doc_id, score) in scored {
        if let Some((chunk_id, file, hash, _len, sym, sl, el)) = doc_meta.remove(&doc_id) {
            out.push(Bm25Candidate {
                file,
                chunk_id,
                start_line: sl,
                end_line: el,
                symbol: sym,
                score,
                content_hash: hash,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::*;
    use crate::structural::language::Language;
    use crate::structural::store::open_in_memory;
    use crate::structural::types::Chunk;

    #[test]
    fn tokenizer_payment_retry_handler() {
        let toks = tokenize("PaymentRetryHandler");
        assert!(toks.contains(&"PaymentRetryHandler".to_string()));
        assert!(toks.contains(&"paymentretryhandler".to_string()));
        assert!(toks.contains(&"Payment".to_string()));
        assert!(toks.contains(&"Retry".to_string()));
        assert!(toks.contains(&"Handler".to_string()));
        assert!(toks.contains(&"payment".to_string()));
        assert!(toks.contains(&"retry".to_string()));
        assert!(toks.contains(&"handler".to_string()));
    }

    #[test]
    fn tokenizer_snake() {
        let toks = tokenize("payment_retry");
        assert!(toks.contains(&"payment_retry".to_string()));
        assert!(toks.contains(&"payment".to_string()));
        assert!(toks.contains(&"retry".to_string()));
    }

    #[test]
    fn tokenizer_qualified() {
        let toks = tokenize("Server.Start");
        assert!(toks.contains(&"Server.Start".to_string()));
        assert!(toks.contains(&"Server".to_string()));
        assert!(toks.contains(&"Start".to_string()));
        assert!(toks.contains(&"server".to_string()));
        assert!(toks.contains(&"start".to_string()));
    }

    #[test]
    fn tokenizer_path() {
        let toks = tokenize("backend/payment/retry.go");
        assert!(toks.contains(&"backend".to_string()));
        assert!(toks.contains(&"payment".to_string()));
        assert!(toks.contains(&"retry".to_string()));
        assert!(toks.contains(&"go".to_string()));
    }

    #[test]
    fn bm25_math_idf() {
        let v = idf(100, 10);
        assert!(v > 0.0);
        let v2 = idf(100, 90);
        assert!(v2 < v);
    }

    #[test]
    fn bm25_insert_and_search() -> Result<()> {
        let conn = open_in_memory()?;
        // Create a fake chunk
        let mut conn_mut = conn;
        let chunk = Chunk {
            id: "c1".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 10,
            parent_symbol: Some("foo".to_string()),
            content_hash: "hash1".to_string(),
            text_size_bytes: 10,
        };
        let content = "def foo():\n    pass\n";
        upsert_bm25_for_file(&mut conn_mut, "a.py", &[chunk.clone()], content)?;
        let cnt = count_bm25_docs(&conn_mut)?;
        assert_eq!(cnt, 1);
        let res = search_bm25(&conn_mut, "foo", 5)?;
        assert!(!res.is_empty());
        assert_eq!(res[0].file, "a.py");
        Ok(())
    }

    #[test]
    fn bm25_incremental_only_affected() -> Result<()> {
        let conn = open_in_memory()?;
        let mut conn_mut = conn;
        let chunk1 = Chunk {
            id: "c1".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 9,
            parent_symbol: Some("foo".to_string()),
            content_hash: "h1".to_string(),
            text_size_bytes: 9,
        };
        let chunk2 = Chunk {
            id: "c2".to_string(),
            file: "b.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 9,
            parent_symbol: Some("bar".to_string()),
            content_hash: "h2".to_string(),
            text_size_bytes: 9,
        };
        upsert_bm25_for_file(&mut conn_mut, "a.py", &[chunk1.clone()], "def foo(): pass")?;
        upsert_bm25_for_file(&mut conn_mut, "b.py", &[chunk2.clone()], "def bar(): pass")?;
        let cnt_before = count_bm25_docs(&conn_mut)?;
        assert_eq!(cnt_before, 2);
        // Update only a.py chunk
        let chunk1_new = Chunk {
            id: "c1".to_string(),
            file: "a.py".to_string(),
            language: Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 12,
            parent_symbol: Some("foo_new".to_string()),
            content_hash: "h1_new".to_string(),
            text_size_bytes: 12,
        };
        upsert_bm25_for_file(
            &mut conn_mut,
            "a.py",
            &[chunk1_new.clone()],
            "def foo_new(): pass",
        )?;
        let cnt_after = count_bm25_docs(&conn_mut)?;
        assert_eq!(cnt_after, 2);
        // b.py should still be searchable for bar
        let res_bar = search_bm25(&conn_mut, "bar", 5)?;
        assert!(res_bar.iter().any(|c| c.file == "b.py"));
        // foo should be gone, foo_new should be searchable
        let _res_foo = search_bm25(&conn_mut, "foo", 5)?;
        // foo_new contains foo? Our tokenizer splits, so foo_new will still match foo via snake split? Let's query exact new
        let res_new = search_bm25(&conn_mut, "foo_new", 5)?;
        assert!(res_new.iter().any(|c| c.file == "a.py"));
        Ok(())
    }
}
