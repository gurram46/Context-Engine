use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use context_core::ContextError;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::discovery::ProjectIndex;

/// Max file size for normal content search (10 MB).
/// Larger files are not loaded fully; filename lookup still works.
const MAX_SEARCH_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum ExactQuery {
    Literal(String),
    Regex(String),
    Identifier(String),
    FileName(String),
    Path(String),
}

impl ExactQuery {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Literal(s)
            | Self::Regex(s)
            | Self::Identifier(s)
            | Self::FileName(s)
            | Self::Path(s) => s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExactSearchOptions {
    pub max_results: usize,
    pub case_sensitive: bool,
    pub timeout: Duration,
}

impl Default for ExactSearchOptions {
    fn default() -> Self {
        Self {
            max_results: 50,
            case_sensitive: true,
            timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExactEvidence {
    pub file: String, // relative posix
    pub line: u32,
    pub end_line: Option<u32>,
    pub text: String,
    pub match_text: Option<String>,
    pub kind: crate::classification::FileKind,
}

/// Check `rg` availability.
pub fn rg_available() -> bool {
    std::process::Command::new("rg")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Rust exact search — owns `rg` invocation, parses matches, returns structured evidence.
pub async fn exact_search(
    project: &ProjectIndex,
    query: ExactQuery,
    opts: ExactSearchOptions,
) -> Result<Vec<ExactEvidence>, ContextError> {
    let root = &project.root;
    let raw = query.as_str().trim();
    if raw.is_empty() {
        return Err(ContextError::InvalidParams("empty query".into()));
    }

    // Filename/path fast path: use ProjectIndex metadata, no rg needed, <10ms.
    match &query {
        ExactQuery::FileName(name) | ExactQuery::Path(name) => {
            let t0 = Instant::now();
            let found = if matches!(query, ExactQuery::Path(_)) {
                project
                    .find_by_path(name)
                    .map(|f| vec![f])
                    .unwrap_or_default()
            } else {
                project
                    .find_by_filename(name)
                    .into_iter()
                    .collect::<Vec<_>>()
            };
            let mut out = Vec::new();
            for f in found.into_iter().take(opts.max_results) {
                out.push(ExactEvidence {
                    file: f.relative_path.clone(),
                    line: 1,
                    end_line: Some(1),
                    text: format!("File exists: {}", f.relative_path),
                    match_text: Some(name.clone()),
                    kind: f.kind,
                });
            }
            tracing::debug!(elapsed_ms = %t0.elapsed().as_millis(), found = %out.len(), "filename lookup");
            if out.is_empty() {
                // Fallback to glob for fuzzy? For now return empty.
            }
            return Ok(out);
        }
        _ => {}
    }

    // For Literal/Regex/Identifier, use rg.
    let is_regex = matches!(query, ExactQuery::Regex(_));
    let search_term = match &query {
        ExactQuery::Identifier(s) => s.clone(),
        ExactQuery::Literal(s) => s.clone(),
        ExactQuery::Regex(s) => s.clone(),
        _ => unreachable!(),
    };

    // Validate regex early
    if is_regex {
        if let Err(e) = regex::Regex::new(&search_term) {
            return Err(ContextError::InvalidParams(format!("invalid regex: {}", e)));
        }
    }

    if !rg_available() {
        return Err(ContextError::Internal("rg not available".into()));
    }

    let mut args: Vec<String> = vec![
        "--line-number".into(),
        "--column".into(),
        "--no-heading".into(),
        "--color".into(),
        "never".into(),
        "--max-count".into(),
        opts.max_results.to_string(),
        "--hidden".into(),
        "--glob".into(),
        "!.git/**".into(),
    ];
    if !is_regex {
        args.push("--fixed-strings".into());
    }
    if !opts.case_sensitive {
        args.push("--ignore-case".into());
    }
    for pat in &[
        ".git",
        ".opencode/index",
        "node_modules",
        "dist",
        "build",
        "target",
        "crates", // for engine repo frozen eval, hidden via discovery but also here for safety
        "__pycache__",
        ".pytest_cache",
        "coverage",
        ".next",
        ".nuxt",
    ] {
        args.push("-g".into());
        args.push(format!("!{}/**", pat));
        args.push("-g".into());
        args.push(format!("!**/{}/**", pat));
    }
    args.push("--".into());
    args.push(search_term.clone());
    args.push(".".into());

    let mut child = Command::new("rg")
        .args(&args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ContextError::Internal(format!("failed to spawn rg: {}", e)))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ContextError::Internal("rg stdout not piped".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ContextError::Internal("rg stderr not piped".into()))?;

    // Bounded stdout handling with timeout
    let t0 = Instant::now();
    let out_fut = async {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let mut evid = Vec::new();
        let stderr_reader = BufReader::new(stderr);
        // Spawn stderr collector
        let stderr_task = tokio::spawn(async move {
            let mut s = String::new();
            let mut l = String::new();
            let mut r = stderr_reader;
            while let Ok(n) = r.read_line(&mut l).await {
                if n == 0 {
                    break;
                }
                s.push_str(&l);
                l.clear();
            }
            s
        });

        while evid.len() < opts.max_results {
            line.clear();
            let n = reader
                .read_line(&mut line)
                .await
                .map_err(|e| ContextError::Internal(format!("rg read failed: {}", e)))?;
            if n == 0 {
                break;
            }
            let raw_line = line.trim_end_matches(&['\r', '\n'][..]);
            if raw_line.is_empty() {
                continue;
            }
            // rg --column gives file:line:column:text
            let mut parts = raw_line.splitn(4, ':');
            let file_raw = parts.next().unwrap_or("");
            let line_str = parts.next().unwrap_or("0");
            let _col = parts.next().unwrap_or("0");
            let text = parts.next().unwrap_or("").to_string();
            let rel = to_relative_posix(file_raw, root);
            // Skip if file is too large (check via ProjectIndex)
            if let Some(rec) = project.files.iter().find(|f| f.relative_path == rel) {
                if rec.size_bytes > MAX_SEARCH_FILE_BYTES {
                    continue;
                }
            }
            let line_num: u32 = line_str.parse().unwrap_or(1);
            let kind = crate::classification::classify_file(Path::new(&rel));
            evid.push(ExactEvidence {
                file: rel,
                line: line_num,
                end_line: Some(line_num),
                text: text.chars().take(400).collect(),
                match_text: Some(search_term.clone()),
                kind,
            });
        }

        // Wait for child with timeout (rg should finish quickly after stdout closed)
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .map_err(|_| ContextError::Timeout("rg wait timeout".into()))?
            .map_err(|e| ContextError::Internal(format!("rg wait failed: {}", e)))?;

        let stderr_text = stderr_task.await.unwrap_or_default();
        if !status.success() && status.code() != Some(1) {
            // 1 = no matches, 0 = matches, else error
            if status.code() == Some(127) || stderr_text.contains("not found") {
                return Err(ContextError::Internal("rg not found".into()));
            }
            // For regex errors, rg returns 2
            if stderr_text.to_lowercase().contains("regex") {
                return Err(ContextError::InvalidParams(format!(
                    "rg regex error: {}",
                    stderr_text.trim()
                )));
            }
        }

        Ok::<Vec<ExactEvidence>, ContextError>(evid)
    };

    let res = tokio::time::timeout(opts.timeout, out_fut)
        .await
        .map_err(|_| ContextError::Timeout(format!("rg timeout after {:?}", opts.timeout)))?
        .map_err(|e| match e {
            ContextError::Timeout(_) => e,
            _ => e,
        })?;

    tracing::debug!(
        elapsed_ms = %t0.elapsed().as_millis(),
        found = %res.len(),
        query = %search_term,
        "exact_search done"
    );
    Ok(res)
}

fn to_relative_posix(file_raw: &str, root: &Path) -> String {
    let p = Path::new(file_raw);
    let rel = if p.is_absolute() {
        p.strip_prefix(root).unwrap_or(p).to_path_buf()
    } else {
        p.to_path_buf()
    };
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::ProjectIndex;
    use crate::project_root::ProjectRoot;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn literal_search() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("a.txt"), b"hello\nworld\nhello again").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let res = exact_search(
            &idx,
            ExactQuery::Literal("hello".into()),
            ExactSearchOptions {
                max_results: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!res.is_empty());
        assert!(res[0].file.ends_with("a.txt"));
    }

    #[tokio::test]
    async fn invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let r = exact_search(
            &idx,
            ExactQuery::Regex("[".into()),
            ExactSearchOptions::default(),
        )
        .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn filename_lookup() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        fs::write(tmp.path().join("go.mod"), b"module foo").unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let res = exact_search(
            &idx,
            ExactQuery::FileName("go.mod".into()),
            ExactSearchOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].file, "go.mod");
    }
}
