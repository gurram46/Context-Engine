use std::path::Path;

/// Language enum for R3 — covers active repos.
/// Unknown files remain searchable via exact but not structurally parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Language {
    Rust,
    Python,
    Go,
    TypeScript,
    JavaScript,
    Unknown,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Go => "go",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "rust" => Self::Rust,
            "python" => Self::Python,
            "go" => Self::Go,
            "typescript" => Self::TypeScript,
            "javascript" => Self::JavaScript,
            _ => Self::Unknown,
        }
    }
}

/// Detect language from file path extension.
/// Uses extension only, not repo name.
/// Supports: .rs, .py, .go, .ts, .tsx, .js, .jsx, .mjs, .cjs
pub fn detect_language(path: &Path) -> Language {
    let fname = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();
    // Handle compound extensions if needed
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // For .tsx/.jsx we check full filename suffix
    if fname.ends_with(".tsx") || fname.ends_with(".ts") {
        // .tsx and .ts both map to TypeScript
        return Language::TypeScript;
    }
    if fname.ends_with(".jsx") || fname.ends_with(".mjs") || fname.ends_with(".cjs") {
        return Language::JavaScript;
    }

    match ext.as_str() {
        "rs" => Language::Rust,
        "py" => Language::Python,
        "go" => Language::Go,
        "ts" => Language::TypeScript,
        "tsx" => Language::TypeScript, // covered above but keep
        "js" => Language::JavaScript,
        "jsx" => Language::JavaScript,
        "mjs" => Language::JavaScript,
        "cjs" => Language::JavaScript,
        _ => Language::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detect() {
        assert_eq!(detect_language(Path::new("foo.rs")), Language::Rust);
        assert_eq!(detect_language(Path::new("a.py")), Language::Python);
        assert_eq!(detect_language(Path::new("b.go")), Language::Go);
        assert_eq!(detect_language(Path::new("c.ts")), Language::TypeScript);
        assert_eq!(detect_language(Path::new("d.tsx")), Language::TypeScript);
        assert_eq!(detect_language(Path::new("e.js")), Language::JavaScript);
        assert_eq!(detect_language(Path::new("f.jsx")), Language::JavaScript);
        assert_eq!(detect_language(Path::new("g.mjs")), Language::JavaScript);
        assert_eq!(detect_language(Path::new("h.md")), Language::Unknown);
        assert_eq!(detect_language(Path::new("README")), Language::Unknown);
    }
}
