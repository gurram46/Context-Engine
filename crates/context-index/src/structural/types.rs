use crate::structural::language::Language;
use serde::{Deserialize, Serialize};

/// SymbolKind generic across languages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Module,
    Constant,
    Variable,
    TypeAlias,
    Field,
    Unknown,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::Module => "module",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::TypeAlias => "type_alias",
            Self::Field => "field",
            Self::Unknown => "unknown",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "function" => Self::Function,
            "method" => Self::Method,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "interface" => Self::Interface,
            "module" => Self::Module,
            "constant" => Self::Constant,
            "variable" => Self::Variable,
            "type_alias" => Self::TypeAlias,
            "field" => Self::Field,
            _ => Self::Unknown,
        }
    }
}

/// Visibility — best effort.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Unknown,
}
impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Unknown => "unknown",
        }
    }
}

/// Symbol — deterministic identity.
/// Invariant: id = blake3( relative_path \0 language \0 qualified_name \0 kind \0 parent )
/// Minor body edits do NOT change id when symbol name/path remains stable.
/// Position not included, so formatting changes don't churn ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String, // blake3 hex
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file: String, // relative posix
    pub language: Language,
    pub start_line: u32, // 1-indexed
    pub end_line: u32,
    pub start_byte: usize,
    pub end_byte: usize,
    pub visibility: Visibility,
    pub parent: Option<String>, // parent qualified_name
}

/// Reference — conservative call/read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReferenceKind {
    Call,
    Read,
    Type,
    Import,
    Unknown,
}
impl ReferenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Read => "read",
            Self::Type => "type",
            Self::Import => "import",
            Self::Unknown => "unknown",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "call" => Self::Call,
            "read" => Self::Read,
            "type" => Self::Type,
            "import" => Self::Import,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub parent_symbol: Option<String>, // symbol id
    pub kind: ReferenceKind,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Import representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub file: String,
    pub import_path: String, // raw import string e.g. "crate::foo::Bar" or "x"
    pub alias: Option<String>,
    pub line: u32,
    pub is_relative: bool,
}

/// Chunk — syntax-aware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String, // blake3(file + start_byte + end_byte + content_hash) or content_hash itself?
    pub file: String,
    pub language: Language,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: usize,
    pub end_byte: usize,
    pub parent_symbol: Option<String>, // symbol id
    pub content_hash: String,          // blake3 of chunk text slice
    pub text_size_bytes: usize,
}

/// ParsedFile — compact typed model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedFile {
    pub file: String,
    pub language: Language,
    pub content_hash: String, // blake3 of file
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub imports: Vec<Import>,
    pub chunks: Vec<Chunk>,
    pub parse_error: Option<String>,
}

/// Call edge — best-effort static graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub caller_symbol_id: String,
    pub callee_name: String,
    pub resolved_symbol_id: Option<String>,
    pub confidence: CallConfidence,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallConfidence {
    Resolved,
    Probable,
    Unresolved,
}
impl CallConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Probable => "probable",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Compute stable symbol id.
/// Uses blake3 over: file \0 language \0 qualified_name \0 kind \0 parent
/// Does NOT use position or content hash, so body edits preserve identity.
pub fn symbol_id(
    file: &str,
    language: Language,
    qualified_name: &str,
    kind: &SymbolKind,
    parent: Option<&str>,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(file.as_bytes());
    hasher.update(b"\0");
    hasher.update(language.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(qualified_name.as_bytes());
    hasher.update(b"\0");
    hasher.update(kind.as_str().as_bytes());
    hasher.update(b"\0");
    if let Some(p) = parent {
        hasher.update(p.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

/// Chunk id = blake3(file \0 start_byte \0 end_byte \0 content_hash)
pub fn chunk_id(file: &str, start_byte: usize, end_byte: usize, content_hash: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(file.as_bytes());
    hasher.update(b"\0");
    hasher.update(start_byte.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(end_byte.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(content_hash.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_id() {
        let id1 = symbol_id(
            "a.py",
            Language::Python,
            "Foo.bar",
            &SymbolKind::Method,
            Some("Foo"),
        );
        let id2 = symbol_id(
            "a.py",
            Language::Python,
            "Foo.bar",
            &SymbolKind::Method,
            Some("Foo"),
        );
        assert_eq!(id1, id2);
        let id3 = symbol_id(
            "a.py",
            Language::Python,
            "Foo.other",
            &SymbolKind::Method,
            Some("Foo"),
        );
        assert_ne!(id1, id3);
    }
    #[test]
    fn chunk_hash_stable() {
        let h = blake3::hash(b"hello").to_hex().to_string();
        let c1 = chunk_id("a.py", 0, 5, &h);
        let c2 = chunk_id("a.py", 0, 5, &h);
        assert_eq!(c1, c2);
    }
}
