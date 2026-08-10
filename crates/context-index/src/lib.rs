//! Rust file discovery, hashing, classification, exact search — R1.
//! Owns project root, ignore rules, file metadata, and `rg` orchestration.
//! Semantic/symbol/graph remain in V2/OCI for now.

pub mod classification;
pub mod discovery;
pub mod exact;
pub mod hash;
pub mod project_root;

pub use classification::{classify_file, FileKind};
pub use discovery::{FileRecord, ProjectIndex, ScanStats};
pub use exact::{exact_search, ExactEvidence, ExactQuery, ExactSearchOptions};
pub use hash::hash_file;
pub use project_root::{resolve_project_root, ProjectRoot};

/// Re-export for convenience.
pub use context_core::ContextError;
