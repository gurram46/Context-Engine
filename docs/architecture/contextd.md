# contextd — Rust backend for Context Engine

**Status:** R3 implemented — Rust now owns structural repository intelligence (tree-sitter, symbols, chunks, graph) plus all R2 capabilities; `contextd.exe` remains the backend; Zed / Codex / OpenCode are frontends. R0 shell, R1 `context-index`, R2 routing/ranking verified.

## Current architecture (R3)

```
Zed / Codex / OpenCode
        │  MCP stdio JSON-RPC (rmcp)
        ▼
   contextd.exe (Rust, tokio, rmcp server)
        ├── ProjectCache (Rust) ──► ProjectIndex ──► discovery (ignore) ──► classification ──► hash (blake3)
        │                              │
        │                         classify / route (context-rank)
        │                              │
        │              ┌────────────────┼────────────────┐
        │              ▼                │                ▼
        │         Rust exact (rg)  Rust structural  OCI semantic (candidateProvider.js)
        │              │            symbols/refs/          │
        │              │            graph/chunks       semantic only
        │              └───────────────┼────────────────┘
        │                              ▼
        │                    Rust authority → Rust fuse → Rust pack (tiktoken)
        │                              │
        │                              ▼
        │                             MCP
        │
        └── V2/OCI TEMP (candidateProvider.js) — semantic only
```

- **Implemented (R0):** Rust MCP contract, 5 tools, project-root forwarding, one persistent V2 child, graceful shutdown, single restart, tracing to stderr.
- **Implemented (R1):** Rust `ProjectRoot`, `ProjectIndex`, `FileKind`, `blake3` (10 MB), `ExactQuery` via `rg` (`.context`/`target` handling via `ENGINE_INTERNAL_EXCLUDES`; `crates/` now indexed for structural).
- **Implemented (R2):** Rust `QueryType` (`classify_query`), `extract_identifiers`, `RetrievalPlan` (exact/symbol/semantic/graph/test), `Evidence` typed, `authority` (18 weights, `is_true_definition`, `FileKind`), `fuse` (dedup, overlap collapse 2–4/file, doc quota ≤2), `packer` (`tiktoken-rs` `cl100k_base`, budget 10k, `packed_tokens`), `PipelineStats`, `candidate` provider (raw, no `authorityScore`).
- **Implemented (R3):** Rust structural (`context-index::structural`):
  - `Language` (Rust/Python/Go/TypeScript/JavaScript, detect via extension; TSX via TS grammar)
  - `ParsedFile { file, language, content_hash, symbols, references, imports, chunks }`
  - `Symbol { id (blake3 stable), name, qualified_name, kind, file, start/end_line/byte, visibility, parent }` with invariant: same file+qualified+kind → same id across body edits
  - `SymbolKind` (Function/Method/Class/Struct/Enum/Trait/Interface/Module/Constant/Variable/TypeAlias/Field/Unknown)
  - `Reference { name, file, line, parent_symbol, kind (Call/Read/Type/Import/Unknown) }`
  - `Import { file, import_path, alias, line, is_relative }`
  - `Chunk { id (blake3), file, language, start/end_line/byte, parent_symbol, content_hash, text_size_bytes }` — syntax-aware, per-symbol, hash for R4 vector reuse
  - `CallEdge { caller_symbol_id, callee_name, resolved_symbol_id, confidence (Resolved/Probable/Unresolved), file, line }` — conservative, never lies
  - Tree-sitter 0.25 + grammars: `tree-sitter-rust 0.24`, `python 0.25`, `go 0.25`, `typescript 0.23`, `javascript 0.25`
  - `StructuralIndex` with persistent SQLite (`rusqlite` bundled) at `<repo>/.context/index/structural.db` (worktree-safe, per-worktree dir, `.context/.gitignore` prevents commit, WAL mode, foreign_keys)
  - Schema v1: `schema_version`, `files (path, hash, language, size_bytes, parse_error)`, `symbols`, `imports`, `refs`, `chunks`, `call_edges` with indexes; version check with clear error for newer schema
  - Incremental: `hash == DB hash → SKIP PARSE`; single changed file reparses only that file; deletion removes stale state atomically; rename → delete+add; transaction rollback on failure via `rusqlite` transaction
  - Worktree-safe: `<repo>/.context/index/` per worktree; two worktrees don't share writable DB (tested)
  - Native lookup APIs: `find_definitions`, `find_symbol_exact/prefix`, `find_references`, `find_callers/callees`, `find_tests_related` — normalize directly into `Evidence` with provenance `rust:symbol`, `rust:graph:resolved|probable|unresolved`, `rust:test`, `rust:exact-reference` fallback retained
  - Pipeline: `symbol` and `graph` now `rust:*` (OCI symbol/graph no longer on production path; kept behind `#[allow(dead_code)]` for R4 shadow); `test` uses `rust:test` + `FileKind::Test` fallback; `semantic` remains `oci:semantic`
  - Performance (Context-Engine, 141 files, 1033 symbols): initial 9.3s, no-change 0.95s, single-file 2.56s; lookups warm: symbol ~16ms, refs ~27ms, callers ~20ms
  - No embeddings/tantivy/watcher yet (R4); no Node removal yet (R5)
- **Current limitation (R3):** structural call graph is static/best-effort (dynamic Click/DI wiring falls back to `rust:exact`); semantic still OCI; no automatic filesystem watcher/queue (manual `ensure()` rebuild); no native vector index (R4); `crates/` indexed for Rust but `target/` still excluded
- **Planned (R4-R5):** R4 `tantivy`/`ort`/`notify` (vector/BM25, embeddings, watcher), R5 remove Node/OCI, keep `v2/` as `reference/`.

## Process model

- `contextd` is a long-running stdio server. One `contextd` per editor session.
- On first tool call (or at startup) it spawns **one** `node v2/dist/mcp/server.js` child via `TokioChildProcess`. The child in turn spawns one OCI child. Total per project: `contextd` + `Node V2` + `Node OCI` + `Ollama` (singleton).
- `contextd` owns the V2 child. Drop / `cancel()` kills it; V2 kills OCI. No orphan processes on Windows (verified via `tasklist` before/after).
- Only one automatic restart is allowed after an unexpected V2 exit; subsequent failures return a structured MCP error. No infinite loop.

## MCP boundary

- **Transport:** `rmcp` `stdio()` (tokio stdin/stdout). Logs go to `stderr` only.
- **Tools (frozen contract):**
  - `context_search { query, budgetTokens?, maxResults?, debug? }`
  - `symbol_lookup { symbol, budgetTokens?, debug? }`
  - `dependency_trace { symbol, direction: callers|callees|both, budgetTokens?, debug? }`
  - `test_lookup { query, budgetTokens?, debug? }`
  - `context_status {}` → merges V2 `version/projectRoot/gitBranch/rgAvailable/oci*` with Rust `contextdVersion/rustVersion/pid/projectRoot`.

Schemas are generated via `schemars` from `context-core` types and match V2 exactly (order may differ, required fields identical).

## V2 compatibility bridge (R2: raw candidate provider)

- **Candidate provider:** `v2/dist/candidateProvider.js` (new, internal, not exposed to Zed) — Node `StdioServer` with tools `symbol_candidates` (`implementation_lookup`), `semantic_candidates` (`codebase_peek`), `graph_candidates` (`call_graph`), `test_candidates` (`codebase_peek` filtered), `index_status`. Directly wraps `codeIndexClient` (`open-codebase-index` MCP) and returns `{candidates: Evidence[]}` raw (no `authorityScore`/`finalScore`/`packed`).
- **V2 MCP (`v2/dist/mcp/server.js`)** still used for `context_status` only; **not** for `context_search`/`symbol_lookup`/`dependency_trace`/`test_lookup` final ranking. Rust `pipeline.rs` owns those via `candidateProvider` + Rust `exact` + Rust `authority`/`fuse`/`packer`.
- Location: `CONTEXTD_V2_PATH` → exe-relative `v2/dist/candidateProvider.js` → `CARGO_MANIFEST_DIR/../../v2/dist/candidateProvider.js` → `cwd`.
- `CandidateProvider` (`crates/contextd/src/candidate.rs`) — `TokioChildProcess` with `pending: Arc<Mutex<HashMap>>`, `current_root` check (respawn on `CONTEXT_ENGINE_PROJECT_ROOT` change), `initialize` handshake, `call_raw` with `timeout 15s`, single restart, `shutdown` kills child. Logs to `stderr`.
- Production path: `classify` → `plan` → `Rust exact (rg)` + `OCI raw` → `Rust authority` → `Rust fuse` → `Rust pack` → `MCP`. No double-ranking.

## Project-root behavior

- `CONTEXT_ENGINE_PROJECT_ROOT` is the single source of truth. `contextd` reads it at `V2Bridge::ensure_client` time and passes it as `current_dir` + env to the V2 child. No `process.chdir`.
- If the env changes between requests, `contextd` cancels the old child and spawns a new one for the new root. Worktrees with `CONTEXT_ENGINE_PROJECT_ROOT=C:/tmp/Mulanous-Lens-...` therefore retrieve against that repo, not Context-Engine.

## R0 → R5 direction

- R0: shell + bridge (done, `a91abac`).
- R1: `context-index` with `ignore` + `blake3` + `rg` (done, `386cf3e`).
- **R2: `context-rank` with `classify`/`identifiers`/`plan`/`authority`/`fuse`/`packer` (done, this doc, `fbd437c`+`0640d22`).** `candidateProvider.js` temporary.
- R3: `context-store` (`rusqlite`, `tree-sitter` symbols, `usearch` mmap read).
- R4: vector/BM25 (`usearch` HNSW, `tantivy`, `ort` CodeRankEmbed, `notify`).
- R5: remove Node/OCI, keep `v2/` as `reference/` for behavioral tests.

Target: `contextd` alone, `120MB` idle / `250MB` query, `<500ms` semantic.

## Operational notes

- Build: `cargo build --release` → `target/release/contextd.exe` (Windows) / `target/release/contextd` (Unix). Requires Node 20 + `v2/dist/mcp/server.js` built (`npm run build --prefix v2`).
- Run: `CONTEXT_ENGINE_PROJECT_ROOT=/path/to/repo target/release/contextd.exe` (stdio). Configure Codex/OpenCode `mcp.contextd.command = ["…/contextd.exe"]`.
- Logs: `RUST_LOG=info|debug` to stderr.
- Windows: use `PathBuf`, no hard-coded `C:/Users/Dell/...` in logic; tests use `tempfile` and canonicalized compares.

## References

- Audit: `docs/audit/cursor-backend-rust-plan.md` (measurements, targets)
- V2 behavioral reference: `15b053e`, `v2/src` + `v2/tests` (41 tests)
