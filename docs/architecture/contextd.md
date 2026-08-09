# contextd — Rust backend for Context Engine

**Status:** R0 implemented — Rust MCP shell over V2. `contextd.exe` is the backend; Zed / Codex / OpenCode are frontends.

## Current architecture (R0)

```
Zed / Codex / OpenCode
        │  MCP stdio JSON-RPC (rmcp)
        ▼
   contextd.exe (Rust, tokio, rmcp server)
        │  MCP stdio (TokioChildProcess)
        ▼
   v2/dist/mcp/server.js (Node)
        │  spawn
        ▼
   open-codebase-index MCP (Node, native addon)
        ├── SQLite (chunks, symbols, branch_chunks)
        ├── usearch mmap (vectors)
        ├── Tree-sitter native (parsing)
        └── Ollama (Go) → nomic-embed-text
```

- **Implemented (R0):** Rust MCP contract, 5 tools, project-root forwarding, one persistent V2 child, graceful shutdown, single restart, tracing to stderr.
- **Current limitation:** retrieval / ranking / indexing still run in Node. No Tantivy, no Tree-sitter in Rust, no notify watcher, no embedding in Rust.
- **Planned (R1-R5):** port `fileClassifier`/`exactSearch` → `context-index`, port `router/authority/fuse` → `context-rank`, bring `rusqlite`+`usearch`+`tantivy`+`tree-sitter` into `context-store`, replace Ollama with `ort` ONNX, add `notify` watcher, then remove Node.

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

## V2 compatibility bridge

- Location resolution order: `CONTEXTD_V2_PATH` env → exe-relative `v2/dist/mcp/server.js` → `CARGO_MANIFEST_DIR/../../v2/dist/mcp/server.js` → `cwd/v2/dist/mcp/server.js`.
- Delegation: each Rust tool handler builds the same JSON arguments as V2 and calls `V2Bridge::call_json(name, args)`, which forwards via `TokioChildProcess` RMCP client `call_tool`. The V2 JSON string inside `CallToolResult` content is parsed and re-emitted as `CallToolResult::success(vec![ContentBlock::text(pretty_json)])`.
- Error mapping: `ContextError::InvalidParams` → `McpError::invalid_params`, `ChildStart/ChildExited` → `internal_error`. No panics in request path.

## Project-root behavior

- `CONTEXT_ENGINE_PROJECT_ROOT` is the single source of truth. `contextd` reads it at `V2Bridge::ensure_client` time and passes it as `current_dir` + env to the V2 child. No `process.chdir`.
- If the env changes between requests, `contextd` cancels the old child and spawns a new one for the new root. Worktrees with `CONTEXT_ENGINE_PROJECT_ROOT=C:/tmp/Mulanous-Lens-...` therefore retrieve against that repo, not Context-Engine.

## R0 → R5 direction

- R0: shell + bridge (done).
- R1: `context-index` with `ignore` + `blake3` + `rg` (keep Node semantic).
- R2: port `classifyQuery/router/authority/fuse/evidencePacker`.
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
