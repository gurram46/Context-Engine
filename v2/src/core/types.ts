/**
 * ADR: OCI integration choice
 * open-codebase-index@0.22.4 exports only an opencode plugin (dist/index.js -> default plugin).
 * No stable direct JS API for search/lookup/callGraph is exported.
 * Stable interface is MCP stdio via dist/cli.js -> tools: implementation_lookup, codebase_peek, etc.
 * Decision: use local MCP stdio client (B), one child per CE process, reuse, timeouts, graceful shutdown.
 * This isolates us from OCI internals and survives internal refactors.
 */

export type RetrievalSource = "exact" | "symbol" | "semantic" | "graph" | "test";

export type EvidenceRelation =
  | "definition"
  | "caller"
  | "callee"
  | "reference"
  | "test"
  | "unknown";

export interface Evidence {
  source: RetrievalSource;
  file: string; // normalized posix relative to project root
  startLine?: number;
  endLine?: number;
  symbol?: string;
  symbolKind?: string; // chunkType: function, class, method, block, etc
  text?: string; // matching line or chunk content (truncated)
  score?: number; // raw retrieval score (0-1 or rg rank)
  relation?: EvidenceRelation;
  authorityScore?: number; // deterministic authority adjustment
  finalScore?: number; // score + authority
  metadata?: Record<string, unknown>;
  provenance?: string; // e.g., "rg:literal", "oci:implementation_lookup"
}

export type QueryType = "EXACT" | "SYMBOL" | "CONCEPTUAL" | "DEPENDENCY" | "TEST" | "MIXED";

export interface ClassifiedQuery {
  type: QueryType;
  raw: string;
  normalized: string;
  hints: string[];
}

export interface RetrievalTiming {
  retriever: string;
  ms: number;
  count: number;
}

export interface RankedEvidence extends Evidence {
  finalScore: number;
  authorityScore: number;
}

export interface PackedEvidence {
  query: string;
  queryType: QueryType;
  evidence: RankedEvidence[];
  packedMarkdown: string;
  tokenEstimate: number;
  tokenBudget: number;
}
