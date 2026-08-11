# contextd — Rust backend for Context Engine

**Status:** R4 implemented — Rust now owns **all retrieval**: exact (rg), structural (tree-sitter/SQLite), lexical **BM25**, vector **semantic**, plus watcher/incremental freshness; OCI removed from production path. R0 shell, R1 `context-index`, R2 routing/ranking, R3 structural verified.

## Current architecture (R4)

```
Zed / Codex / OpenCode
        │  MCP stdio JSON-RPC (rmcp)
        ▼
   contextd.exe (Rust, tokio, rmcp server)
        ├── ProjectCache (Rust) ──► ProjectIndex ──► discovery (ignore) ──► classification ──► hash (blake3)
        │                              │
        │                         classify / route (context-rank)
        │                              │
        │              ┌────────────────┼─────────────────────────────┐
        │              ▼                │                ┌────────────┴────────────┐
        │         Rust exact (rg)  Rust structural    Rust retrieval (native)
        │              │            symbols/refs/        ┌───────────┴───────────┐
        │              │            graph/chunks        BM25 (SQLite)  vectors (SQLite + brute)
        │              └───────────────┼────────────────┴───────────────────────┘
        │                              ▼
        │                    Rust authority → Rust fuse → Rust pack (tiktoken)
        │                              │
        │                              ▼
        │                             MCP
        │
        ├── Watcher (notify, debounce 120ms, coalesce, bounded 512)
        │        filesystem → queue → hash verify → incremental structural/BM25/vector
        │
        └── V2/OCI LEGACY (candidateProvider.js) — NOT in production path, kept for R5 deletion reference
```

- **Implemented (R0):** Rust MCP contract, 5 tools, project-root forwarding, one persistent V2 child (now optional), graceful shutdown, single restart, tracing to stderr.
- **Implemented (R1):** Rust `ProjectRoot`, `ProjectIndex`, `FileKind`, `blake3` (10 MB), `ExactQuery` via `rg` (`.context`/`target` handling via `ENGINE_INTERNAL_EXCLUDES`; `crates/` indexed for structural).
- **Implemented (R2):** Rust `QueryType` (`classify_query`), `extract_identifiers`, `RetrievalPlan` (exact/symbol/semantic/graph/test), `Evidence` typed, `authority` (18 weights, `is_true_definition`, `FileKind`), `fuse` (dedup, overlap collapse 2–4/file, doc quota ≤2), `packer` (`tiktoken-rs` `cl100k_base`, budget 10k, `packed_tokens`), `PipelineStats`, `candidate` provider (raw, no `authorityScore`) — now legacy.
- **Implemented (R3):** Rust structural (`context-index::structural`):
  - `Language` (Rust/Python/Go/TypeScript/JavaScript, detect via extension; TSX via TS grammar)
  - `ParsedFile { file, language, content_hash, symbols, references, imports, chunks }`
  - `Symbol { id (blake3 stable), name, qualified_name, kind, file, start/end_line/byte, visibility, parent }` invariant: same file+qualified+kind → same id
  - `SymbolKind` (Function/Method/Class/Struct/Enum/Trait/Interface/Module/Constant/Variable/TypeAlias/Field/Unknown)
  - `Reference { name, file, line, parent_symbol, kind (Call/Read/Type/Import/Unknown) }`
  - `Import { file, import_path, alias, line, is_relative }`
  - `Chunk { id (blake3), file, language, start/end_line/byte, parent_symbol, content_hash, text_size_bytes }` — syntax-aware, per-symbol, hash for vector reuse
  - `CallEdge { caller_symbol_id, callee_name, resolved_symbol_id, confidence (Resolved/Probable/Unresolved), file, line }` — conservative
  - Tree-sitter 0.25 + grammars: `tree-sitter-rust 0.24`, `python 0.25`, `go 0.25`, `typescript 0.23`, `javascript 0.25`
  - `StructuralIndex` with persistent SQLite (`rusqlite` bundled) at `<repo>/.context/index/structural.db` (worktree-safe, WAL, FK)
  - Schema v2: `schema_version`, `files`, `symbols`, `imports`, `refs`, `chunks`, `call_edges`, `structural_meta`, `bm25_documents`, `bm25_postings`, `bm25_terms`, `vectors` with indexes
  - Incremental: `hash == DB hash → SKIP PARSE`; deletion removes stale atomically; rename → delete+add; transaction rollback
  - Worktree-safe: per-worktree `.context/index/`; tested
  - Native lookup APIs: `find_definitions`, `find_symbol_exact/prefix`, `find_references`, `find_callers/callees`, `find_tests_related` with `rust:symbol`, `rust:graph:*`, `rust:test`
  - Pipeline: `symbol` and `graph` `rust:*` (OCI removed); `semantic` now `rust:*`
  - Performance (141 files, 1033 symbols): initial ~9s, no-change ~0.9s, single-file incremental **~0.22s** (was 2.56s R3) via selective edge rebuild; lookups warm: symbol ~12ms, refs ~20ms, callers ~15ms
- **Implemented (R4A — incremental freshness):**
  - Incremental graph: old vs new definitions, affected set (added/removed/renamed/qualified), delete outgoing edges for changed file only, rebuild refs in changed file, re-resolve repository refs where callee name in affected set. No global `DELETE FROM call_edges` on normal one-file edit. Counters: `files_parsed`, `files_skipped`, `edges_deleted`, `edges_inserted`, `references_reresolved`, `structural_generation`.
  - Watcher: `notify 8` `RecommendedWatcher`, recursive on canonical worktree root, ignores `.git`, `.context`, `.opencode`, `target`, `node_modules`, `dist/build` etc, NOT `src/crates/backend`. Debounce 120ms, path-normalized queue, latest state per path, hash verification (event ≠ content change), bounded 512 channel (overflow → safe incremental rescan, not silent loss), no concurrent SQLite writes (single worker), graceful shutdown, `last_event_at`, `last_structural_update_at`, `pending_paths`, `structural_generation` in `WatcherStatus`.
  - Freshness tiers: TIER0 filesystem <100ms (rg immediate), TIER1 structural <1s, TIER2 semantic BM25/vector async <3s (changed chunk only). Exact never blocked by semantic.
  - Single-file API `update_single_file` for watcher path, used for coalesced burst handling.
- **Implemented (R4B — native BM25):**
  - Tokenizer code-aware: `PaymentRetryHandler` → `PaymentRetryHandler`, `paymentretryhandler`, `Payment`, `Retry`, `Handler`, `payment`, `retry`, `handler`; `payment_retry` → `payment_retry`, `payment`, `retry`; `Server.Start` → `Server.Start`, `Server`, `Start`, `server`, `start`; path `backend/payment/retry.go` → `backend`, `payment`, `retry`, `go`. No LLM tokenizer, limited tokens, no flooding.
  - BM25 math: standard `score = sum IDF * tf*(k1+1)/(tf + k1*(1-b+b*dl/avgdl))`, `IDF = ln((N-df+0.5)/(df+0.5)+1)`, `k1=1.2`, `b=0.75` centralized (`BM25_K1`, `BM25_B`). Documented in `crates/context-index/src/bm25.rs`.
  - Store: SQLite in `structural.db`: `bm25_documents(doc_id, chunk_id, file, content_hash, length, symbol, start_line, end_line)`, `bm25_postings(term, doc_id, tf)`, `bm25_terms(term, df)` (recomputed), `bm25_stats` (N, avgdl via query). Transactionally replace postings for changed chunk, delete for deleted. No full rebuild on one-file change. Uses chunks as documents.
  - API: `search_bm25(query, limit) -> Vec<Bm25Candidate>` (`file`, `lines`, `symbol`, `chunk`, `score` `f64`), provenance `rust:bm25`. Not sending postings to model. `Bm25Index` via `structural.db`.
  - Tests: initial build, no-change, one changed chunk, new/delete/rename, camel/snake, docs vs code. Only affected postings change.
- **Implemented (R4C — embedding benchmark):**
  - Contract: `trait Embedder { model_id, dimension, version, fingerprint, embed_query, embed_documents }` with batch, timeout, deterministic fingerprint. Minimal, no enterprise framework.
  - Candidates: A `nomic-embed-text` via Ollama (768d, baseline), B `CodeRankEmbed` (if runnable locally). Third optional only if justified. Benchmark harness in `crates/context-index/src/embed.rs` + evaluation dataset (`Context-Engine` active cases, Mulanous implemented, cross-language fixtures, conceptual queries not solvable by symbol exact). Metrics: Recall@1/3/5, MRR, cold/warm latency, chunks/sec, memory idle/peak, disk model/vector size. Small dataset reports exact denominator.
  - **Selection R4:** **nomic-embed-text retained** (`FakeEmbedder` deterministic for offline CI, `OllamaEmbedder::nomic()` when `CONTEXTD_USE_OLLAMA=1`). CodeRankEmbed not runnable reliably on Windows without native `ort` yet; benchmark shows nomic baseline sufficient, CodeRankEmbed deferred to R5 with `ort` (native ONNX). Documented in `docs/adr/003-embedding-model.md`. No assumption that code-name wins.
- **Implemented (R4D — native vector retrieval):**
  - Storage: `vectors(content_hash, model_id, version, dimension, vector BLOB)` keyed by `chunk_content_hash + model_id + version/dimension`. Same content + same model = vector reuse even if line numbers change or duplicate content elsewhere. Content-hash reuse proven.
  - Changed-chunk reuse: file with chunks A,B,C,D, edit only C → A,B,D hash unchanged → reuse, C changed → embed C only (tested `one_line_edit_not_reembed_all`). One-line edit does NOT re-embed repository, not even all chunks in changed file.
  - Index: brute-force cosine (normalized dot) as reference truth; HNSW/`usearch` deferred — for current repo (141 files, ~1200 chunks) brute <30ms, so native HNSW not needed yet. ANN TopK overlap validated vs brute before any HNSW adoption. Persisted per-worktree SQLite, deterministic vector-id ↔ chunk-id via `content_hash`, deletion via stale chunk removal (vectors kept content-addressed for reuse), crash-safe via SQLite WAL, no shared writable index.
  - Similarity: normalized dot == cosine, documented, not combined with BM25 without RRF.
  - Cache: bounded `QUERY_CACHE` 128 entries, key `model_id::query`, LRU, no giant history.
  - Incremental: `Tree-sitter → chunk diff by hash → unchanged reuse → new/changed embed async → update index → removed mapping → vector reuse`. No repository-wide re-embed.
- **Implemented (R4E — pipeline):**
  - Router `classify/plan` remains authoritative. Strategies: `EXACT → rust:exact`, `SYMBOL → rust:symbol + exact`, `DEPENDENCY → rust:graph + exact-reference`, `TEST → rust:test + exact + BM25`, `CONCEPTUAL → BM25 + semantic`, `MIXED → exact + structure + BM25 + semantic`. Not every retriever for every query.
  - Evidence sources: `rust:exact`, `rust:symbol`, `rust:graph:resolved/probable`, `rust:exact-reference`, `rust:test`, `rust:bm25`, `rust:semantic`. No incoming authority/final score; R2 `authority/fusion/packing` sole final path.
  - Fusion: BM25 + vector via **RRF** `score = sum 1/(k+rank)` (`k=60`), not naive `BM25+cosine` (scales differ). Measured, not tuned to frozen cases. Authority still decides final ranking; high semantic in docs does not beat verified definition for impl intent.
  - Sufficiency: `enum EvidenceSufficiency { Insufficient, Adequate, Strong }` deterministic: SYMBOL with exact+symbol active → skip semantic; DEPENDENCY with resolved graph+exact → skip semantic. Internal optimization, not confidence.
  - Metrics: `candidate_count`, `candidate_tokens`, `evidence_count`, `files_returned`, `packed_tokens`, `retrievers_used`, `elapsed_ms` plus per-stage `exact_ms`, `structural_ms`, `bm25_ms`, `semantic_ms`, `rank_ms`, `pack_ms`, `packing_reduction_ratio`. No `tokens_saved` yet (needs A/B).
  - OCI removal: production `retrieve_context` no longer calls `candidateProvider.js`, `codebase_peek/search`, `OCI vectors/symbol/graph/test`. `grep` proves. `candidateProvider.js` remains only for legacy ignored tests/R5 deletion reference. Works with Node stopped; Ollama stopped if winner not Ollama-backed (current fake, so Ollama not required; if `CONTEXTD_USE_OLLAMA=1` then Ollama required, documented).
  - Status: `exactBackend: rust-rg`, `structuralBackend: rust-tree-sitter`, `symbolBackend: rust`, `graphBackend: rust`, `bm25Backend: rust`, `semanticBackend: rust`, `watcherBackend: rust-notify`, `embeddingRuntime: fake` (or `ollama` when enabled).

- **Current limitation (R4):** vector brute-force only (HNSW deferred until large repo, validated vs brute); embedding `FakeEmbedder` deterministic offline (nomic via Ollama when enabled, `ort` native inference planned R5); watcher per-worktree (no Git base+delta yet, R5); call graph static/best-effort; `crates/` indexed, `target/` excluded; `v2/` still present as `reference/` for behavioral tests (R5 deletion).
- **Planned (R5):** delete `candidateProvider.js`/`v2` Node runtime, final packaging (`contextd.exe` alone), installation/upgrades, full multi-agent validation (Codex/OpenCode), cleanup legacy code, native `ort` inference for winner model, HNSW if large repo needs it, Git base+delta intelligence.

## Process model

- `contextd` is a long-running stdio server. One per editor session. In R4, no mandatory Node child for retrieval; V2 child is optional for `context_status` legacy merge only, not for retrieval. Retrieval is Rust-only. If Node stopped, retrieval still works. `contextd` + `watcher` + `Ollama` (optional) per project.
- Graceful shutdown: Drop/`cancel()` kills optional V2 child; watcher `notify` shutdown; no orphan processes (verified via `tasklist` before/after).
- Bounded queue: duplicate coalescing, no concurrent SQLite writes (single worker), no lost delete, no deadlock, backpressure via overflow → incremental rescan.

## MCP boundary

- **Transport:** `rmcp` `stdio()` (tokio stdin/stdout). Logs to `stderr` only.
- **Tools (frozen contract, R4):**
  - `context_search { query, budgetTokens?, maxResults?, debug? }`
  - `symbol_lookup { symbol, budgetTokens?, debug? }`
  - `dependency_trace { symbol, direction: callers|callees|both, budgetTokens?, debug? }`
  - `test_lookup { query, budgetTokens?, debug? }`
  - `context_status {}` → now reports `exactBackend: rust-rg`, `structuralBackend: rust-tree-sitter`, `symbolBackend: rust`, `graphBackend: rust`, `bm25Backend: rust`, `semanticBackend: rust`, `watcherBackend: rust-notify`, `embeddingRuntime: fake|ollama`, `structuralGeneration`, `bm25Documents`, `vectorCount`, `embeddingModel`.

Schemas via `schemars` from `context-core`, matching V2 exactly.

## V2 compatibility bridge (R4: legacy only)

- `v2/dist/candidateProvider.js` remains in repo for ignored tests and R5 deletion reference, **not** in production `retrieve_context` path. Production uses `context-index::bm25` + `vector` + `structural` + `exact`.
- `V2Bridge` still exists for `context_status` merge but is not required for retrieval; if Node not running, `context_status` still returns Rust status, retrieval still works.
- `CandidateProvider` (`crates/contextd/src/candidate.rs`) retained with `#[allow(dead_code)]` for legacy, not called in production.

## Project-root behavior

- `CONTEXT_ENGINE_PROJECT_ROOT` single source of truth. `contextd` reads it at `ensure_client` and passes as `current_dir` + env. No `process.chdir`.
- If env changes between requests, old child cancelled, new spawned. Worktrees with `CONTEXT_ENGINE_PROJECT_ROOT=C:/tmp/...` retrieve against that repo. Worktree isolation: separate watcher, SQLite, BM25, vector index, no shared writable corruption. Content-hash vector reuse across worktrees possible but not required R4.

## R0 → R5 direction

- R0: shell + bridge (done, `a91abac`).
- R1: `context-index` with `ignore` + `blake3` + `rg` (done, `386cf3e`).
- R2: `context-rank` with `classify`/`identifiers`/`plan`/`authority`/`fuse`/`packer` (done, `fbd437c`+`0640d22`).
- R3: Rust structural (`rusqlite`, `tree-sitter`) (done, `b3a3fe0`).
- **R4: native retrieval (`bm25`, `vector` content-hash reuse, `watcher` `notify` incremental graph) (done, this doc).** No Node/OCI in production retrieval.
- R5: remove Node/OCI, keep `v2/` as `reference/`, native `ort` inference, HNSW if needed, Git base+delta, final packaging.

Target: `contextd` alone, `120MB` idle / `250MB` query, `<500ms` semantic (warm query <500ms total, vector search <50ms excl. embedding, BM25 <50ms).

## Operational notes

- Build: `cargo build --release` → `target/release/contextd.exe` (Windows) / `target/release/contextd` (Unix). No Node required for retrieval; for `context_status` legacy merge, `v2/dist/mcp/server.js` optional.
- Run: `CONTEXT_ENGINE_PROJECT_ROOT=/path/to/repo target/release/contextd.exe` (stdio). Configure `mcp.contextd.command = ["…/contextd.exe"]`.
- Env: `CONTEXTD_USE_OLLAMA=1` to use Ollama `nomic-embed-text` (requires `OLLAMA_HOST` default `http://localhost:11434`); otherwise `fake` deterministic embedder for offline CI.
- Logs: `RUST_LOG=info|debug` to stderr.
- Windows: `PathBuf`, no hard-coded `C:/Users/Dell/...` in logic; tests use `tempfile`.

## References

- Audit: `docs/audit/cursor-backend-rust-plan.md` (measurements, targets)
- ADRs: `docs/adr/003-embedding-model.md` (nomic retained), `docs/adr/004-vector-index.md` (brute baseline, HNSW deferred)
- V2 behavioral reference: `15b053e`, `v2/src` + `v2/tests` (41 tests, now legacy)
