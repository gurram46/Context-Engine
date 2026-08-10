#![allow(
    clippy::explicit_counter_loop,
    clippy::needless_borrow,
    clippy::if_same_then_else,
    clippy::regex_creation_in_loops,
    clippy::collapsible_else_if,
    clippy::useless_format
)]

pub mod authority;
pub mod classify;
pub mod fuse;
pub mod identifiers;
pub mod packer;
pub mod plan;
pub mod types;

pub use authority::{apply_authority, score_authority, AUTHORITY_WEIGHTS};
pub use classify::classify_query;
pub use fuse::{fuse_evidence, FuseOptions};
pub use identifiers::extract_identifiers;
pub use packer::{pack_evidence, PackOptions, PackedResult};
pub use plan::{build_retrieval_plan, RetrievalPlan};
pub use types::{Evidence, EvidenceRelation, QueryType, RetrievalSource};
