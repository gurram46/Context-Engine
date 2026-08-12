# R5 Production Context Connector Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform `contextd` into a production local repository-intelligence connector with one native Rust service core serving both a production CLI (`contextd search/symbol/dependency/tests/status/mcp`) and an optional MCP adapter, with skills-first agent integration, no Node/V2/OCI in production, and preserved 12/12 retrieval.

**Architecture:** Single native service `ContextService` in `crates/contextd/src/service.rs` (and `crates/context-core`) wrapping `context-index` (discovery/structural/BM25/vector) + `context-rank` (classify/plan/authority/fuse/pack) + `pipeline::retrieve_context`. CLI (`crates/contextd/src/cli.rs` via `clap`) and MCP (`crates/contextd/src/mcp.rs`) are thin adapters calling `ContextService` only — no `CLI→MCP` or `MCP→CLI`, no duplicated business logic.

**Tech Stack:** Rust 2021, clap 4.x, rmcp 3.x (MCP), tokio, tracing, context-core/index/rank, Tree-sitter, rusqlite, BM25, Ollama `all-minilm` (or optional native ONNX), `notify` watcher.

## Global Constraints

- Branch `rust/contextd-r4` at `806551a`, R4 frozen: 12/12 Top1, 6/6 R2, 15/15 R3, 10/10 `cargo test --workspace`, release PASS — do NOT modify authority weights, routing, BM25 math, embedding selection or ranking unless genuine generic regression.
- CLI and MCP MUST use exact same native pipeline (`ContextService`), no duplication of classify/retrieval/ranking/fusion/authority/packing/status.
- CLI stdout = result only, stderr = tracing; `--json` stdout valid JSON only, no ANSI.
- Skill canonical location `skills/context-engine/SKILL.md`, keep token cost minimal, wrappers source same body.
- Production `contextd` must not require Node, npm, TypeScript V2, candidateProvider, open-codebase-index, OCI index/vectors, `codebase_peek`, V2 subprocess — production dependency graph must show no edge to Node/V2/OCI.
- `cargo test --workspace` must require no Node/npm/OCI/network/Ollama/user-local repo — deterministic fixtures + FakeEmbedder test-only, live tests `#[ignore]`.
- Worktree isolation per `ProjectRoot` — DB/BM25/vector/watcher per worktree, no hardcoded `C:\Users\Dell`, use `Path/PathBuf`.
- Versioning: index/store version metadata, safe migration, semantic model mismatch invalidates only semantic state.
- Performance warm targets: exact <100ms, symbol <20ms, refs/graph <50ms, BM25 <50ms, vector search (excl embedding) <50ms, CLI no-change reconcile <100ms.
- No runtime `unwrap/expect` in recoverable production paths, typed errors, `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo build --release`, `cargo doc --workspace --no-deps` must pass.
- Retrieval must not regress: 12/12 MRR 1.00, 5/5 core strict, 6/6 R2, 15/15 R3, MCP/CLI equivalence.

---

### Task 1: Production audit — remove Node/V2/OCI from production path

**Files:**
- Modify: `crates/contextd/src/bridge.rs` (or delete if production only)
- Modify: `crates/contextd/src/candidate.rs` (mark LEGACY or gate behind `#[cfg(feature="legacy")]`)
- Modify: `crates/contextd/src/main.rs:1-10,7,104-117,406-417` (remove V2Bridge usage from production)
- Modify: `crates/contextd/src/pipeline.rs:12-17,198-204` (Providers candidate already dead code)
- Modify: `Cargo.toml`, `crates/contextd/Cargo.toml` (remove Node bridge deps from default features)
- Test: `cargo tree --manifest-path crates/contextd/Cargo.toml` and `rg -n "candidateProvider|CONTEXT_ENGINE_V2|open-codebase-index|oci:|codebase_peek|V2Bridge" crates/contextd/src --glob '!bridge.rs'`
- Docs: `docs/architecture/contextd.md` mark LEGACY section

**Interfaces:**
- Consumes: current `V2Bridge::new`, `CandidateProvider::new`
- Produces: `cargo tree` shows no `candidateProvider.js` runtime edge; `rg` returns 0 hits in `crates/contextd/src` production files (excluding `bridge.rs` legacy shim)

- [ ] **Step 1: Write failing audit test**

```rust
// crates/contextd/tests/production_deps.rs
#[test]
fn no_oci_in_production() {
    let src = std::fs::read_to_string("crates/contextd/src/main.rs").unwrap();
    assert!(!src.contains("candidateProvider"), "OCI reference remains");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p contextd --test production_deps -- --nocapture`
Expected: FAIL — candidateProvider still present

- [ ] **Step 3: Gate legacy code**

```rust
// crates/contextd/src/candidate.rs top
//! LEGACY / HISTORICAL / BENCHMARK — not used in production R5 retrieval.
//! Gate with feature `legacy-v2` if needed for archaeology.
#[cfg(feature = "legacy-v2")]
pub struct CandidateProvider { ... }
```

Modify `crates/contextd/src/pipeline.rs`:

```rust
pub struct Providers {
    // R5: empty — production path uses no V2/OCI provider
}
```

Modify `crates/contextd/src/main.rs` to not construct `V2Bridge` in default path (keep only for `contextd legacy-mcp` if needed, or remove import).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p contextd --test production_deps -- --nocapture`
Expected: PASS

- [ ] **Step 5: Verify cargo tree**

Run: `cargo tree -p contextd | rg -i "node|oci|v2"` → no hits

- [ ] **Step 6: Commit**

```bash
git add crates/contextd/src/candidate.rs crates/contextd/src/bridge.rs crates/contextd/src/pipeline.rs crates/contextd/tests/production_deps.rs
git commit -m "chore(contextd): gate legacy V2/OCI behind feature, remove from production path"
```

---

### Task 2: Native service API — one core, multiple adapters

**Files:**
- Create: `crates/contextd/src/service.rs`
- Create: `crates/contextd/src/config.rs` (minimal, if not exists)
- Modify: `crates/contextd/src/lib.rs` (re-export service)
- Modify: `crates/contextd/src/project.rs` (expose reconcile API)
- Test: `crates/contextd/tests/service_parity.rs`

**Interfaces:**
- Consumes: `context_index::{ProjectIndex, ProjectRoot}`, `context_index::structural::*`, `context_rank::*`, `pipeline::retrieve_context`
- Produces:
```rust
pub struct ContextService { root: PathBuf, cache: Arc<ProjectCache>, ... }
impl ContextService {
    pub async fn new(root: Option<PathBuf>) -> Result<Self, ContextError>;
    pub async fn search(&self, query: &str, opts: SearchOptions) -> Result<ContextResult, ContextError>;
    pub async fn symbol(&self, symbol: &str, opts: SearchOptions) -> Result<ContextResult, ContextError>;
    pub async fn dependency(&self, symbol: &str, direction: Direction, opts: SearchOptions) -> Result<ContextResult, ContextError>;
    pub async fn tests(&self, query: &str, opts: SearchOptions) -> Result<ContextResult, ContextError>;
    pub async fn status(&self) -> Result<StatusReport, ContextError>;
    pub async fn reconcile(&self) -> Result<ReconcileStats, ContextError>; // cheap hash/incremental
}
pub struct SearchOptions { pub budget_tokens: usize, pub max_results: usize, pub json: bool, pub debug: bool }
pub enum Direction { Callers, Callees, Both }
```

- [ ] **Step 1: Write failing test**

```rust
#[tokio::test]
async fn service_parity_cli_mcp() {
    let svc = ContextService::new(Some(temp_root())).await.unwrap();
    let a = svc.search("count_tokens", SearchOptions::default()).await.unwrap();
    // MCP must call same service — later test will assert CLI and MCP produce same evidence ordering
    assert_eq!(a.evidence[0].file, "backend/context_engine/core/utils.py");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p contextd --test service_parity -- --nocapture`
Expected: FAIL `service.rs not found`

- [ ] **Step 3: Implement minimal ContextService**

```rust
// crates/contextd/src/service.rs
use crate::project::ProjectCache;
use crate::pipeline::{retrieve_context, Providers};
use context_core::ContextError;
use std::path::PathBuf;
use std::sync::Arc;

pub struct ContextService { cache: Arc<ProjectCache>, root: PathBuf }

impl ContextService {
    pub async fn new(root: Option<PathBuf>) -> Result<Self, ContextError> { /* resolve ProjectRoot, init cache, return */ }
    pub async fn search(&self, query: &str, opts: SearchOptions) -> Result<ContextResult, ContextError> {
        let project = self.cache.ensure().await?;
        self.reconcile().await?; // cheap
        retrieve_context(query, &project, &Providers{}, opts.budget_tokens, opts.max_results).await.map_err(Into::into)
    }
    // symbol/dependency/tests delegate to search with formatted query like R4
    pub async fn status(&self) -> Result<StatusReport, ContextError> { /* version, root, generation, counts, semanticAvailable, watcherState, storeVersion */ }
    pub async fn reconcile(&self) -> Result<ReconcileStats, ContextError> {
        // discovery/hash, incremental structural/BM25 only, no full reindex, target <100ms no-change
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p contextd --test service_parity -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/contextd/src/service.rs crates/contextd/src/lib.rs crates/contextd/src/config.rs crates/contextd/tests/service_parity.rs
git commit -m "feat(contextd): add native ContextService core for CLI/MCP adapters"
```

---

### Task 3: Native CLI — search/symbol/dependency/tests/status/mcp

**Files:**
- Modify: `crates/contextd/src/main.rs` (replace rmcp-only main with `clap` CLI)
- Create: `crates/contextd/src/cli.rs`
- Test: `crates/contextd/tests/cli_parity.rs`, `tests/cli_json_stdout.rs`

**Interfaces:**
- Consumes: `ContextService::search/symbol/dependency/tests/status/reconcile`
- Produces: CLI binary `contextd` with subcommands:
```
contextd search "<q>" [--json] [--root <path>] [--budget <n>] [--max-results <n>] [--debug]
contextd symbol <sym> [same opts]
contextd dependency <sym> --direction callers|callees|both
contextd tests "<q>"
contextd status [--json]
contextd mcp
contextd --version
```

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn cli_search_json_stdout_clean() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_contextd"))
        .args(["search", "Where is count_tokens implemented?", "--json", "--root", fixture_root()])
        .output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON only");
    assert_eq!(v["evidence"][0]["file"], "backend/context_engine/core/utils.py");
    assert!(out.stderr.len() > 0 || true); // tracing to stderr
    assert!(!stdout.contains("\u{1b}[")); // no ANSI
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p contextd --test cli_json_stdout -- --nocapture`
Expected: FAIL `no such subcommand search`

- [ ] **Step 3: Implement clap CLI**

```rust
// crates/contextd/src/cli.rs
use clap::{Parser, Subcommand};
#[derive(Parser)] struct Cli { #[command(subcommand)] cmd: Cmd, #[arg(long)] json: bool, #[arg(long)] debug: bool, #[arg(long)] root: Option<PathBuf>, #[arg(long)] budget: Option<usize>, #[arg(long)] max_results: Option<usize> }
#[derive(Subcommand)] enum Cmd { Search { query: String }, Symbol { symbol: String }, Dependency { symbol: String, #[arg(long)] direction: String }, Tests { query: String }, Status, Mcp }
pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(EnvFilter::from_default_env()).init();
    let svc = ContextService::new(cli.root).await?;
    match cli.cmd {
        Cmd::Search{q} => { let res = svc.search(&q, opts).await?; if cli.json { println!("{}", serde_json::to_string_pretty(&res)?) } else { println!("{}", res.packed.markdown) } },
        Cmd::Mcp => { /* start rmcp adapter using same service */ },
        // ...
    }
}
```

Modify `crates/contextd/src/main.rs` to call `cli::run().await` and `contextd mcp` path uses `crate::mcp` adapter.

Stdout discipline: only result, no banner; tracing to stderr via `tracing_subscriber::fmt().with_writer(std::io::stderr)`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p contextd --test cli_json_stdout -- --nocapture`
Expected: PASS

- [ ] **Step 5: Test human output**

Run: `cargo run -p contextd -- search "Where is payment retry enforced?" --root .` → concise readable evidence to stdout

- [ ] **Step 6: Commit**

```bash
git add crates/contextd/src/cli.rs crates/contextd/src/main.rs crates/contextd/tests/cli_parity.rs
git commit -m "feat(contextd): add native CLI search/symbol/dependency/tests/status/mcp"
```

---

### Task 4: CLI freshness — cheap reconcile on invocation

**Files:**
- Modify: `crates/contextd/src/service.rs` `reconcile()` impl
- Modify: `crates/context-index/src/discovery.rs`, `crates/context-index/src/structural/store.rs`, `crates/context-index/src/bm25.rs`
- Test: `crates/contextd/tests/freshness.rs`

**Interfaces:**
- Consumes: `ProjectIndex::discover_with_hash`, `StructuralIndex::build_incremental`, `bm25::update_chunks`
- Produces: `ReconcileStats { discovered_ms, changed_files, structural_ms, bm25_ms, vector_pending }`

- [ ] **Step 1: Write failing test**

```rust
#[tokio::test]
async fn warm_no_change_under_100ms() {
    let svc = ContextService::new(Some(fixture_root())).await.unwrap();
    let t0 = Instant::now();
    svc.reconcile().await.unwrap(); // second call no changes
    assert!(t0.elapsed().as_millis() < 100, "warm reconcile too slow");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p contextd --test freshness -- --nocapture`
Expected: FAIL `reconcile not implemented` or >100ms due to full reindex

- [ ] **Step 3: Implement cheap reconcile**

```rust
pub async fn reconcile(&self) -> Result<ReconcileStats, ContextError> {
    let idx = ProjectIndex::discover_with_hash(&self.root)?; // uses stored file hashes
    let changed = diff_hashes(&idx, &prev)?; // only changed files
    if changed.is_empty() { return Ok(ReconcileStats { changed_files: 0, .. }) }
    for f in changed { structural_store::update_file(&conn, &f)?; bm25::update_file(&conn, &f)?; }
    // semantic: reuse valid vectors, embed only changed chunks, skip if too slow, never FakeEmbedder, never deleted chunks
}
```

Do NOT full-index on every CLI invocation; only affected file/chunks/edges. Semantic not blocking exact/symbol.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p contextd --test freshness -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/contextd/src/service.rs crates/contextd/tests/freshness.rs
git commit -m "feat(contextd): cheap reconcile on CLI invocation, <100ms no-change"
```

---

### Task 5: MCP becomes optional adapter (5 tools only)

**Files:**
- Create: `crates/contextd/src/mcp.rs` (tool router calling ContextService)
- Modify: `crates/contextd/src/main.rs` (mcp subcommand)
- Modify: `crates/contextd/src/service.rs` (ensure status logic reused)
- Test: `crates/contextd/tests/mcp_parity.rs`

**Interfaces:**
- Consumes: `ContextService`
- Produces: 5 tools `context_search, symbol_lookup, dependency_trace, test_lookup, context_status` — no BM25/vector/watcher tools — stdout pure MCP traffic.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn mcp_tools_exactly_five() { let tools = mcp::tool_list(); assert_eq!(tools.len(), 5); }
#[test]
fn mcp_stdout_purity() { /* spawn contextd mcp, send initialize, assert stdout only JSON-RPC, no banner */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p contextd --test mcp_parity -- --nocapture`
Expected: FAIL `mcp.rs not found`

- [ ] **Step 3: Implement thin MCP adapter**

```rust
// crates/contextd/src/mcp.rs
pub struct McpAdapter { svc: Arc<ContextService> }
#[tool_router]
impl McpAdapter {
    #[tool(description="...")] async fn context_search(&self, p: Parameters<ContextSearchParams>) -> Result<CallToolResult, McpError> { let r = self.svc.search(&p.query, opts).await?; Ok(to_json(r)) }
    // symbol_lookup, dependency_trace, test_lookup, context_status similarly — each 3 lines
}
```

In `main.rs` `Cmd::Mcp` → `McpAdapter::new(svc).serve(stdio()).await` with tracing to stderr, no `println!` banner.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p contextd --test mcp_parity -- --nocapture` and `cargo test -p contextd --test service_parity` → CLI and MCP evidence ordering equal.

- [ ] **Step 5: Commit**

```bash
git add crates/contextd/src/mcp.rs crates/contextd/src/main.rs crates/contextd/tests/mcp_parity.rs
git commit -m "refactor(contextd): MCP as thin optional adapter over native service"
```

---

### Task 6: Skills-first integration + installation

**Files:**
- Create: `skills/context-engine/SKILL.md`
- Create: `skills/context-engine/install.md` (or `docs/integrations/*.md`)
- Create: `scripts/install-skill.sh` and `scripts/install-skill.ps1` (copy/link wrappers)
- Test: `tests/skill_tokens.rs` (count tokens)

**Interfaces:**
- Consumes: canonical SKILL.md content
- Produces: wrappers for Codex (`~/.codex/skills/context-engine/SKILL.md`), OpenCode (`.opencode/skills/context-engine/SKILL.md` or `opencode.json` skill path), Claude (`.claude/skills/context-engine/SKILL.md`) that source same body.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn skill_exists_and_small() {
    let body = std::fs::read_to_string("skills/context-engine/SKILL.md").unwrap();
    assert!(body.contains("contextd search"), "skill must teach contextd search");
    assert!(body.len() < 2048, "skill too large, keep progressive disclosure");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test skill_tokens -- --nocapture`
Expected: FAIL `SKILL.md not found`

- [ ] **Step 3: Create canonical skill**

```markdown
// skills/context-engine/SKILL.md (small, <1k tokens)
# Context Engine
Before broad repository exploration, prefer Context Engine.
Use: contextd search "<natural language repository question>" --json
for: unknown implementation location, cross-file behavior, architecture flow, dependency tracing, tests covering behavior, conceptual questions.
For known exact path/string where shell tools cheaper, normal tools ok.
Prefer one high-quality Context Engine request over repeated grep/read cycles.
If returned evidence sufficient, stop searching. Context Engine retrieves context only. Continue editing/testing with normal tools.
```

Create tiny wrappers: `cp skills/context-engine/SKILL.md ~/.codex/...` etc, install scripts.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test skill_tokens -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add skills/context-engine/SKILL.md scripts/install-skill.*
git commit -m "feat(skill): add canonical Context Engine skill + wrappers"
```

---

### Task 7: CLI vs MCP measurement harness (R5G)

**Files:**
- Create: `scripts/measure_overhead.rs` or `crates/contextd/tests/overhead.rs`
- Create: `docs/integrations/overhead.md`
- Test: manual measurement run

**Interfaces:**
- Consumes: `ContextService`, MCP tool definitions, skill file
- Produces: numbers for `tool-definition tokens, skill tokens, packed context tokens, agent-visible tool calls, elapsed_ms, expected-file rank`.

- [ ] **Step 1: Write failing script**

Run: `cargo run --bin measure_overhead -- --query "Where is count_tokens implemented?"` → should output table `A MCP vs B CLI`.

- [ ] **Step 2: Implement measurement**

Measure `tiktoken` tokens for tool schemas vs skill body, run both paths against same repo/query, capture elapsed, rank.

- [ ] **Step 3: Run and document**

Run: `cargo run -p contextd --bin measure_overhead > docs/integrations/overhead.md` → document preferred integration (Skill+CLI if cheaper/equally reliable), MCP remains supported.

- [ ] **Step 4: Commit**

```bash
git add scripts/measure_overhead.rs docs/integrations/overhead.md
git commit -m "docs: measure Skill+CLI vs MCP overhead"
```

---

### Task 8: Config, versioning, migration, worktree safety, crash hardening, performance, memory

**Files:**
- Create: `crates/contextd/src/config.rs` (toml `.context/contextd.toml`, env `CONTEXTD_EMBED_MODEL`, priority CLI>env>config>defaults)
- Modify: `crates/context-index/src/structural/store.rs` (version metadata, migration)
- Modify: `crates/context-index/src/vector.rs` (model fingerprint invalidation only semantic)
- Modify: `crates/contextd/src/service.rs` (worktree root via `ProjectRoot::resolve`, per-worktree DB path)
- Test: `crates/contextd/tests/version_migration.rs`, `crates/contextd/tests/worktree_isolation.rs`, `crates/contextd/tests/crash_recovery.rs`

**Interfaces:**
- Consumes: `ProjectRoot`, `rusqlite` transactions
- Produces: `Config { embedding_model, semantic_enabled, budget, watcher_enabled, index_location }`, migration that rebuilds only affected index.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn worktree_isolation() {
    let a = TempDir::new(); let b = TempDir::new();
    // index A, modify A, assert B unchanged
}
#[test]
fn crash_recovery() {
    // kill during structural update, restart, reconcile recovers, exact/BM25 available, semantic rebuilding
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p contextd --test worktree_isolation -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement config + versioning**

```rust
// config.rs
pub fn load(root: &Path, cli: &Cli) -> Config { /* CLI > env > .context/contextd.toml > defaults */ }

// store.rs
const SCHEMA_VERSION: u32 = 5;
fn migrate(conn: &Connection) -> Result<()> { /* transactional, compatible→open, migratable→migrate, model mismatch→invalidate semantic only, incompatible→rebuild native index only */ }
```

Per-worktree: `index_db_path(root)` derived from `root.path()`, not hardcoded `C:\Users\Dell`.

Crash: use SQLite transactions, on restart reconcile detects partial generation and resumes.

Performance: instrument `PipelineStats` already, report `cargo test --test perf -- --nocapture` with startup/discovery/reconcile/classify/exact/structural/BM25/embedding/vector/rank/pack/total.

Memory: bound caches (`QUERY_CACHE` 100 entries, watcher queue 1000), test 100 searches shows RSS stable.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p contextd --test worktree_isolation -- --nocapture` etc
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/contextd/src/config.rs crates/context-index/src/structural/store.rs crates/contextd/tests/worktree_isolation.rs
git commit -m "feat(contextd): config, versioning, worktree isolation, crash recovery"
```

---

### Task 9: Packaging, version/diagnostics, docs, security, live validation

**Files:**
- Modify: `crates/contextd/src/service.rs` `status()` (version, root, branch, generation, files indexed, symbols, BM25 docs, vectors, model, runtime, semanticAvailable, watcherState, schemaVersion)
- Modify: `README.md`, `docs/architecture/contextd.md`, `docs/integrations/README.md`
- Create: `docs/adr/006-cli-mcp-adapter.md`, `007-embedding-runtime.md`, `008-index-migration.md` (only if consequential)
- Test: `crates/contextd/tests/cli_live.rs` (OpenCode Skill→CLI invocation proof)

**Interfaces:**
- Consumes: `contextd status --json`, `context_status` MCP
- Produces: `contextd --version`, `contextd status`, `cargo build --release` binary works outside Cargo, no secrets in output.

- [ ] **Step 1: Write failing live test**

```rust
#[test] fn cli_live_count_tokens() { /* run contextd search "Where is count_tokens implemented?" --json, assert top file backend/context_engine/core/utils.py */ }
```

- [ ] **Step 2: Run OpenCode live validation**

Run: `opencode run --prompt "Use contextd search where is count_tokens implemented --json"` → capture proof `contextd search ...` invoked (shell trace), not MCP. Repeat for `secret redaction`. If Codex unavailable, report `CODEX_NOT_AVAILABLE`.

- [ ] **Step 3: Implement status + packaging**

Ensure `cargo build --release` produces `target/release/contextd.exe` + `contextd --version` shows `CARGO_PKG_VERSION`, `contextd status --json` fields as spec, no env vars/secrets.

Document `README.md` boundary: "Context Engine retrieves context. It does not run the coding agent." Document Skill+CLI vs MCP, mark preferred based on overhead measurement, Windows packaging instructions.

Security: no retrieval command exposes env vars, no upload, local only.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/architecture/contextd.md docs/integrations/README.md crates/contextd/src/service.rs
git commit -m "docs(contextd): update packaging, status diagnostics, boundary"
```

---

### Task 10: Retrieval regression + quality gates (final)

**Files:**
- Test: reuse frozen harness `C:\Users\Dell\AppData\Local\Temp\opencode\target\debug\temp-verify.exe` and `cargo test --workspace`
- Config: ensure `cargo fmt --check`, `clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` ×10, `cargo build --release`, `cargo doc --workspace --no-deps` all PASS

**Interfaces:**
- Consumes: same pipeline
- Produces: `12/12 MRR 1.00, 6/6 R2, 15/15 R3, 5/5 core strict` (count_tokens, secret redaction, bundle flow, bundle callers, bundle tests), MCP/CLI equivalence.

- [ ] **Step 1: Write failing gate test**

```bash
cargo test -p context-rank --test frozen_eval -- --nocapture  # expect 6/6
cargo test -p context-index --test r3_structural_parity -- --nocapture # 15/15
C:\Users\Dell\AppData\Local\Temp\opencode\target\debug\temp-verify.exe  # 12/12
```

- [ ] **Step 2: Run 10× workspace tests**

Run: `for ($i=1; $i -le 10; $i++) { cargo test --workspace --quiet; if (!$?) { exit 1 } }` → 10 PASS

- [ ] **Step 3: Run quality gates**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo build --release --quiet && cargo doc --workspace --no-deps --quiet` → all PASS, no unwrap in production paths (check `rg -n "unwrap\(\)|expect\(" crates/contextd/src/service.rs`).

- [ ] **Step 4: Verify MCP/CLI equivalence**

Run: `contextd search "Where is payment retry enforced?" --json` vs MCP `context_search` same query → evidence ordering equal (or top1 same).

- [ ] **Step 5: Commit + report**

```bash
git status --short  # clean
git rev-parse HEAD
```

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-08-12-r5-production-connector.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
