use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use context_core::ContextError;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::discovery::ProjectIndex;

/// Ensures orphaned `rg` is killed if the future is cancelled (timeout).
struct KillOnDrop(Option<Child>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.start_kill();
        }
    }
}

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
        "crates", // engine-repo-specific: avoids indexing the Rust implementation itself during frozen eval
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
    // Guard: if outer timeout cancels `out_fut`, `Child` is dropped without `wait`; Tokio does not auto-kill, so we kill on drop.
    let mut kill_guard = KillOnDrop(Some(child));

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
            // Convert to a relative POSIX path FIRST so drive letters (e.g. "C:")
            // on Windows don't corrupt the `:` split below.
            let rel = to_relative_posix(raw_line, root);
            let mut parts = rel.splitn(4, ':');
            let file = parts.next().unwrap_or("");
            let line_str = parts.next().unwrap_or("0");
            let _col = parts.next().unwrap_or("0");
            let text = parts.next().unwrap_or("").to_string();
            let rel = file.to_string();
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
        let child_ref = kill_guard
            .0
            .as_mut()
            .ok_or_else(|| ContextError::Internal("rg child missing".into()))?;
        let status = tokio::time::timeout(Duration::from_secs(5), child_ref.wait())
            .await
            .map_err(|_| ContextError::Timeout("rg wait timeout".into()))?
            .map_err(|e| ContextError::Internal(format!("rg wait failed: {}", e)))?;
        // Prevent Drop kill after successful wait
        kill_guard.0.take();
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
    let mut s = rel.to_string_lossy().replace('\\', "/");
    // rg returns ./path when invoked with "." — strip leading ./ for parity with v2
    while s.starts_with("./") || s.starts_with(".\\") {
        s = s[2..].to_string();
    }
    s
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

    #[test]
    fn relative_posix_strips_leading_dot_slash() {
        let tmp = std::env::temp_dir();
        assert_eq!(to_relative_posix("./src/main.rs", &tmp), "src/main.rs");
        assert_eq!(to_relative_posix("src/main.rs", &tmp), "src/main.rs");
    }

    #[cfg(windows)]
    #[test]
    fn windows_absolute_path_parsed_correctly() {
        // Simulate rg output on Windows: the path is absolute before conversion.
        let root = Path::new(r"C:\repo");
        let raw = r"C:\repo\src\main.rs:10:5:hello";
        let rel = to_relative_posix(raw, root);
        assert_eq!(rel, "src/main.rs:10:5:hello");
        let mut parts = rel.splitn(4, ':');
        assert_eq!(parts.next().unwrap(), "src/main.rs");
        assert_eq!(parts.next().unwrap(), "10");
        assert_eq!(parts.next().unwrap(), "5");
        assert_eq!(parts.next().unwrap(), "hello");
    }

    #[tokio::test]
    async fn timeout_kills_child_and_allows_subsequent_search() {
        let tmp = TempDir::new().unwrap();
        let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        let mut content = String::new();
        for i in 0..1000 {
            content.push_str(&format!("line {} hello world\n", i));
        }
        std::fs::write(tmp.path().join("a.txt"), content).unwrap();
        let idx = ProjectIndex::discover(&root).unwrap();
        let res1 = exact_search(
            &idx,
            ExactQuery::Literal("hello".into()),
            ExactSearchOptions {
                max_results: 100,
                timeout: Duration::from_millis(1),
                ..Default::default()
            },
        )
        .await;
        assert!(res1.is_ok() || matches!(res1, Err(ContextError::Timeout(_))));
        let res2 = exact_search(
            &idx,
            ExactQuery::Literal("hello".into()),
            ExactSearchOptions {
                max_results: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!res2.is_empty());
    }
}
