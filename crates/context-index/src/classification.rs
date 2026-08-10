use std::path::Path;

/// Generic file kind for R1.
/// Mirrors `v2/src/core/fileClassifier.ts` but idiomatic Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Source,
    Test,
    Doc,
    Config,
    Build,
    Generated,
    Unknown,
}

impl FileKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Test => "test",
            Self::Doc => "doc",
            Self::Config => "config",
            Self::Build => "build",
            Self::Generated => "generated",
            Self::Unknown => "unknown",
        }
    }
}

/// Classify by extension and filename conventions.
/// Generic, not repo-specific.
pub fn classify_file(path: &Path) -> FileKind {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let lower = file_name.to_lowercase();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // Special filenames
    match lower.as_str() {
        "go.mod" | "go.sum" | "cargo.toml" | "cargo.lock" | "package.json"
        | "package-lock.json" | "dockerfile" | "makefile" | "procfile" | "justfile"
        | "brewfile" | "gemfile" | "rakefile" => {
            return FileKind::Build;
        }
        _ => {}
    }
    // Dockerfile variants
    if lower == "dockerfile" || lower.starts_with("dockerfile.") {
        return FileKind::Build;
    }

    // Test file conventions
    let path_str = path.to_string_lossy().to_lowercase();
    if path_str.contains("/tests/")
        || path_str.contains("/test/")
        || path_str.contains("/__tests__/")
    {
        // Still check if it's actually source-like, but mark as Test
        // Use file name to confirm
        if matches!(
            ext.as_str(),
            "py" | "ts" | "js" | "tsx" | "jsx" | "go" | "rs" | "java" | "kt" | "rb" | "php"
        ) {
            return FileKind::Test;
        }
        // Even for other ext, treat as Test if in tests/ dir
        return FileKind::Test;
    }
    if lower.contains("_test.") || lower.starts_with("test_") {
        return FileKind::Test;
    }
    if lower.ends_with(".test.ts")
        || lower.ends_with(".test.js")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".test.jsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.js")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with(".spec.jsx")
    {
        return FileKind::Test;
    }

    // Generated / cache dirs (generic)
    if path_str == "dist"
        || path_str.starts_with("dist/")
        || path_str.contains("/dist/")
        || path_str == "build"
        || path_str.starts_with("build/")
        || path_str.contains("/build/")
        || path_str == "target"
        || path_str.starts_with("target/")
        || path_str.contains("/target/")
        || path_str == "node_modules"
        || path_str.starts_with("node_modules/")
        || path_str.contains("/node_modules/")
        || path_str.contains("/.next/")
        || path_str.contains("/.nuxt/")
    {
        return FileKind::Generated;
    }

    // Source extensions
    match ext.as_str() {
        "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "rs" | "java" | "kt" | "c" | "cc" | "cpp"
        | "h" | "hpp" | "cs" | "rb" | "php" | "swift" | "sql" | "sh" | "ps1" => {
            return FileKind::Source;
        }
        "json" | "yaml" | "yml" | "toml" | "ini" | "env" | "xml" | "proto" => {
            return FileKind::Config;
        }
        "md" | "markdown" | "rst" | "txt" | "adoc" => {
            return FileKind::Doc;
        }
        _ => {}
    }

    // Fallback: check if it's a known config/build file without ext
    if matches!(lower.as_str(), "dockerfile" | "makefile" | "procfile") {
        return FileKind::Build;
    }

    FileKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn source_py() {
        assert_eq!(classify_file(Path::new("backend/foo.py")), FileKind::Source);
    }
    #[test]
    fn test_py() {
        assert_eq!(
            classify_file(Path::new("tests/test_foo.py")),
            FileKind::Test
        );
        assert_eq!(classify_file(Path::new("src/foo.test.ts")), FileKind::Test);
    }
    #[test]
    fn config_json() {
        assert_eq!(classify_file(Path::new("package.json")), FileKind::Build);
        assert_eq!(classify_file(Path::new("config.yaml")), FileKind::Config);
    }
    #[test]
    fn doc_md() {
        assert_eq!(classify_file(Path::new("README.md")), FileKind::Doc);
    }
    #[test]
    fn generated() {
        assert_eq!(
            classify_file(Path::new("dist/bundle.js")),
            FileKind::Generated
        );
        assert_eq!(
            classify_file(Path::new("target/debug/foo")),
            FileKind::Generated
        );
    }
    #[test]
    fn go_mod() {
        assert_eq!(classify_file(Path::new("go.mod")), FileKind::Build);
    }
}
