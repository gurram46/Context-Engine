use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RetrievalSource {
    Exact,
    Symbol,
    Semantic,
    Graph,
    Test,
}

impl RetrievalSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Symbol => "symbol",
            Self::Semantic => "semantic",
            Self::Graph => "graph",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EvidenceRelation {
    Definition,
    Caller,
    Callee,
    Reference,
    Test,
    Unknown,
}

impl EvidenceRelation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Caller => "caller",
            Self::Callee => "callee",
            Self::Reference => "reference",
            Self::Test => "test",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub source: RetrievalSource,
    pub file: String,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub symbol: Option<String>,
    pub symbol_kind: Option<String>,
    pub text: Option<String>,
    pub score: Option<f64>,
    pub relation: Option<EvidenceRelation>,
    pub authority_score: Option<i32>,
    pub final_score: Option<f64>,
    pub provenance: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl Evidence {
    pub fn new(source: RetrievalSource, file: impl Into<String>) -> Self {
        Self {
            source,
            file: file.into(),
            start_line: None,
            end_line: None,
            symbol: None,
            symbol_kind: None,
            text: None,
            score: None,
            relation: None,
            authority_score: None,
            final_score: None,
            provenance: None,
            metadata: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueryType {
    Exact,
    Symbol,
    Conceptual,
    Dependency,
    Test,
    Mixed,
}

impl QueryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Exact => "EXACT",
            Self::Symbol => "SYMBOL",
            Self::Conceptual => "CONCEPTUAL",
            Self::Dependency => "DEPENDENCY",
            Self::Test => "TEST",
            Self::Mixed => "MIXED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedQuery {
    pub query_type: QueryType,
    pub raw: String,
    pub normalized: String,
    pub hints: Vec<String>,
}
