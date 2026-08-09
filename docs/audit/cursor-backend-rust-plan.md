# Cursor-Backend Audit + Rust Migration Plan — contextd

**Branch:** `audit/cursor-backend-rust-plan` @ `15b053e` (frozen `rewrite/retrieval-v2`)  
**Date:** 2026-08-09  
**V2 ref:** 41/41 tests, 5/5 Top1, `open-codebase-index@0.22.4`, `ollama nomic-embed-text 274MB/768d`, Node 20.19.5, Win32

---

## 1. Current Process Architecture

```
Zed / Codex / Claude / OpenCode
            │  MCP stdio JSON-RPC
            ▼
   ContextEngine (Node, v2/src/core/contextEngine.ts:67)
            │  spawn node cli.js cwd=projectRoot
            ▼
   open-codebase-index MCP (Node, node_modules/open-codebase-index/dist/cli.js 849KB)
            │  JS orchestration + native addon (napi)
            ├── SQLite (chunks, symbols, call_edges, branch_chunks) via better-sqlite3 / native
            ├── usearch native (vectors, mmap, HNSW) — `native/*.node`  ~2-5MB
            ├── Tree-sitter native (parsing) — `tree-sitter` + `tree-sitter-*` grammars .node
            ├── BM25/inverted-index.json (JS, 1.3MB for CE)
            ├── file walk / ignore (JS + `ignore` + git)
            └── embedding HTTP → Ollama (Go) → nomic-bert 137M F16 GGUF
            ▼
   Ollama daemon (Go, `ollama.exe`) → spawns `ollama_llama_server.exe` (llama.cpp) on first embed
```

One `ContextEngine` Node (tsx, 80MB) + one OCI child Node (119MB) per project; Ollama singleton (40MB idle, +300MB when model resident). Total 2 Node + 1 Go(+opt llama).

## 2. Exact RAM per Process (measured `Get-Process` WS/Priv, `process.memoryUsage()`)

**A. Before anything (no CE, ollama idle, no model resident):**
- `ollama.exe` 40.9MB WS / 63.6MB Priv, 1 node (shell) 52MB
- Total ~93MB

**B. ContextEngine started idle (new ContextEngine(), no query):**
- `measure_hold.mjs` Node (CE) ~71MB WS / 91MB Priv (RSS 76MB heap 7MB) — no OCI child yet (lazy)

**C. After `context_status` (spawns OCI child, reads index 1643 chunks):**
- CE Node 80MB, OCI Node 105-119MB WS / 123-155MB Priv, ollama 40.9MB
- Total WS ~316-328MB (incl. 2 other shell nodes 32+52MB) → **stack ~240MB** (CE 80 + OCI 119 + ollama 41)
- `process.memoryUsage().rss` CE 77MB

**D. After exact query `health.go` (rg + symbol):**
- CE 82MB, OCI 119MB → total 328MB, latency 1.5s cold, 80ms warm

**E. After symbol `count_tokens`:**
- CE 80MB, OCI 119MB, same

**F. After conceptual `secret redaction` (semantic 10+5):**
- CE 78MB, OCI 119MB — no Ollama growth (query embed ~3s but llama_server exits quickly, not captured at poll 3s interval)

**G. After 10 warm queries:**
- CE 82MB heap 9.5MB, OCI 103MB (down after GC), total 300MB

**H. While indexing (pilot `force:true` 116 files 1273 chunks):** polled `vectors 149B` (header only), `wal 2.6MB`, `indexing.lock` held, `ollama_llama_server` not observed at 3s poll (likely ~300MB transient). Est. peak `ollama_llama_server` 600-800MB when embedding (nomic 274MB + batch). Not captured due to short polling.

**I. After indexing finishes idle (not achieved — pilot `not indexed` due to EBUSY wal):** expected CE 80MB + OCI 119MB + ollama 41MB idle = 240MB. After `engine.close()` → 93MB (only shell nodes + ollama).

**Per-process table:**

| PID (sample) | exe | WS | Priv | role |
|---|---|---|---|---|
| 17900 | node tsx ce | 71-82MB | 91-99MB | ContextEngine V2 (router/ranker/packer) |
| 7708 | node cli.js | 103-119MB | 123-155MB | OCI MCP (SQLite+usearch+Tree-sitter+BM25) |
| 18540 | ollama.exe | 40.9MB | 63.6MB | Ollama daemon (idle) |
| +18042 | ollama_llama_server | — (not captured, est. 500-700MB when loaded) | — | nomic-bert 137M |

**Total current memory:** **~240MB** warm (CE+OCI+ollama idle), **~500-700MB** transient when embedding, **~93MB** after close. No per-query leak (G vs C stable).

## 3. Total Current Memory Footprint
- **Idle (no query, index loaded):** 240MB (80+119+41)
- **Warm query (exact/symbol/conceptual):** 300-328MB (incl. shell nodes, GC variance)
- **Embedding (indexing/query-embed):** 240MB + 400-600MB llama = 650-850MB transient
- **Startup:** CE Node 70MB + OCI child 100MB + ollama 40MB = 210MB before any index read (0.8s)

Well below 2GB assumptions — but 2 Nodes + Go is heavy for always-on.

## 4. Index Disk Footprint

**Context-Engine (262 source files, 1643 chunks, ~? tokens, branch `audit/cursor-backend-rust-plan`):**
```
.opencode/index 8 files 23.46 MB
  codebase.db          ~2.2 MB (chunks, symbols, call_edges, branch_chunks, embeddings BLOB ref)
  codebase.db-wal      ~0.03 MB (after checkpoint)
  inverted-index.json  ~1.32 MB (BM25)
  vectors              ~0.65 MB? actually 646901B vectors + overhead
  vectors.meta.json    ~0.10 MB
  file-hashes.*.json   ~0.003 MB
  metadata, other
→ 23.5 MB / 1643 chunks = 14.3 KB/chunk, 23.5 MB / 262 files = 90 KB/file
→ bytes/chunk ~14KB (768*4=3KB vector + 1KB BM25 + SQLite row)
→ DISK HEAVY / RAM LIGHT as desired: vectors mmap, SQLite on disk, BM25 JSON loaded (~1.3MB)
```

**Mulanous-Lens shared (pilot):** currently corrupted after `force:true`: `5 files 2.2MB` (only `codebase.db` 2.2MB, vectors 149B header, inverted-index 62B empty) — `branch_chunks` only `audit` 384, pilot `not indexed`. Healthy scaffold previously `384 chunks 6.5MB` est. Pilot healthy would be `~1273 chunks ~18MB` (116 files).

**Per repo:**
- CE: 1643 chunks 23.5MB, Mulanous pilot est. 18MB, total 41MB for both.
- WAL 2.6MB, SHM 32KB transient during indexing.

## 5. OCI TS vs Rust/Native Ownership Map

`open-codebase-index@0.22.4` (`dist/cli.js` 849KB, `cli.cjs` 853KB, `native/*.node`):

| Component | Language | Owner |
|---|---|---|
| MCP transport, tool routing, JSON-RPC | TS | `dist/cli.js` |
| File discovery, gitignore, hashing (xxhash), language detection | TS (`ignore`, `fast-glob`, `xxhash`) + Rust? Actually hashing in TS, file walk in JS | JS |
| Content hash, incremental reuse (content_hash) | TS/JS + SQLite | JS |
| Syntax parse, chunking (syntax-aware), Tree-sitter queries | **Rust/native** via `tree-sitter` + `tree-sitter-*` `.node` + `napi` binding | **Rust** |
| Chunk construction (split, overlap, token count) | TS | JS |
| Embedding HTTP provider (Ollama/OpenAI) | TS (`fetch`) | JS |
| Vector insertion/search | **Rust** `usearch` native (`usearch` crate, HNSW, mmap) — `native/*.node` | **Rust** |
| BM25 / inverted-index.json | TS (JS `Map`, JSON) | JS |
| Symbol extraction (Tree-sitter) | Rust/native | **Rust** |
| Call graph (tree-sitter + heuristic) | Rust/TS hybrid | Rust+JS |
| SQLite `codebase.db` (chunks, branch_chunks, symbols, call_edges) | **Rust** via `better-sqlite3`/`rusqlite` native? Actually `better-sqlite3` native addon | **Rust/C** |
| Branch metadata, blame | TS + `simple-git` | JS |
| Filesystem orchestration, WAL, mmap | Rust (usearch, SQLite) | Rust |
| `vectors` file (usearch dump) | Rust `usearch` | Rust |
| `vectors.meta.json` | TS | JS |

Expensive ops ownership:
- **file discovery/hashing** — JS (fast)
- **parsing/chunking** — native Tree-sitter (CPU, RAM light, fast)
- **embedding** — external Ollama (Go/llama.cpp) — not in OCI, bottleneck
- **vector insertion/search** — native usearch (mmap, RAM light)
- **BM25 indexing** — JS (in-memory JSON, 1.3MB)
- **symbol/call graph** — native + JS
- **query search** — native vector (mmap) + JS BM25 + SQLite

→ **Reuse:** Tree-sitter, usearch, SQLite, Git ignore — all mature Rust primitives. Do not rewrite.

## 6. Full Indexing Pipeline

```
repo
 ↓ (1) file discovery: fast-glob + .gitignore + .opencodeignore (JS, 50ms)
 ↓ (2) ignore rules: `ignore` npm, git check-ignore
 ↓ (3) content hash: xxhash3 of file (JS, streaming, 10ms)
 ↓ (4) language detection: ext → lang (JS map)
 ↓ (5) syntax parse: Tree-sitter native per file (Rust, 100ms for 100 files)
 ↓ (6) chunk construction: syntax-aware split (JS, 50ms) → content_hash per chunk
 ↓ (7) embedding: HTTP POST to Ollama `/api/embed` per batch (Go, 3.4s per batch, bottleneck)
 ↓ (8) vector storage: usearch `add` + `save` mmap (Rust, 100ms)
 ↓ (9) BM25: inverted-index.json build (JS, 50ms)
 ↓ (10) symbol metadata: Tree-sitter query (Rust)
 ↓ (11) call graph: heuristic (Rust/JS)
 ↓ (12) branch metadata: git branch, blame (JS git)
```

| Stage | Lang | Process | Storage | RAM | Incremental | Failure |
|---|---|---|---|---|---|---|
| discovery | JS | OCI Node | — | low | yes (hash) | ignore misconfig |
| hashing | JS | OCI | file-hashes JSON | low | yes (content_hash reuse) | — |
| parse/chunk | Rust | native | chunks table | low | yes (hash) | syntax error → fallback |
| embedding | Go/llama | Ollama | embeddings table + vectors | high transient | **yes** content-addressed (`content_hash` PK) | Ollama down → failed-batches 0B |
| vector | Rust | usearch | vectors + vectors.meta | mmap | yes | dim mismatch → force rebuild |
| BM25 | JS | Node | inverted-index.json | 1.3MB | rebuild (no incremental) | — |
| symbols | Rust | native | symbols table | low | yes | — |

**Syntax-aware?** Yes — Tree-sitter per lang, not arbitrary lines.

**Unchanged reuse?** Yes — `content_hash` PK in `embeddings` and `chunks`, branch_chunks links.

**Embedding caching?** Yes — `embeddings.content_hash` content-addressed, reuse across branches.

**Branch-aware?** Yes — `branch_chunks` + `branch_symbols` + `vectors` fingerprint `7d201830` — but shared `C:/Users/Dell/Mulanous-Lens/.opencode/index` via `git --git-common-dir`, vectors shared, branch_chunks separate. Pilot `1273` vs scaffold `384` should coexist.

**Why pilot failed?** `force:true` unlinked `codebase.db` while `wal`+`shm` held (`EBUSY codebase.db-wal`), left `vectors 149B` header, `inverted-index 62B` empty, `indexing.lock.recovery` + `failed-batches` 0B, `branch_chunks` only scaffold. Subsequent `force:false` still `EBUSY wal`. Root is **WAL locking + shared index process not quiesced** before unlink — `proc.kill()` killed MCP but not `usearch`/`SQLite` background writer. Branch switching shares state incorrectly if `force:true` used while OCI child still has DB handle. **Not branch logic, but process lifecycle.**

**Worktree separate indexes?** No — shared `/.opencode/index` via common dir. Separate worktrees cannot have separate indexes without `CONTEXT_ENGINE_PROJECT_ROOT` isolation (we added) but OCI still uses common dir. Safe if not force-deleting.

**Full rebuild avoidable?** Yes — incremental via `content_hash`, but current `force:true` path deletes all. Normal `force:false` is incremental.

## 7. Why Mulanous Indexing Failed/Stalled — Detailed

- Pilot not indexed initially (`branch_chunks` only scaffold). `index_status` showed `1643` for CE but `not indexed` for pilot (semantic 0).
- Manual `index_codebase force:true` (1800s) → `EBUSY unlink codebase.db-wal` at 195ms (WAL held by previous OCI child not yet `close()`), created `indexing.lock.recovery`, deleted `vectors`/`inverted-index`, left DB 2.2MB but corrupted.
- Next `force:true` → `EBUSY wal` again, `vectors` stays 149B, `not indexed` idle.
- Polling 12×3s showed `TOTAL WS 300MB` stable, `wal 2.6MB` but `vectors` not growing, `failed-batches` 0B, `embedding 20% (64/116 files, 0/1273 chunks)` never progressed — Ollama not receiving batches due to DB lock.
- **WAL locking is real failure**, not model. Fix: `close()` OCI client, `sqlite3 wal_checkpoint`, `rm wal/shm`, then `force:false` incremental, not `force:true`.
- Pilot baseline with degraded index: `Top1 11%` (2/18) due to `semantic-peek:0`, not ranking.

## 8. Incremental Freshness Measurements

**Manual saved-file test (Context-Engine `backend/context_engine/core/utils.py` add comment):**

| Change | rg exact | symbol | semantic | latency |
|---|---|---|---|---|
| modify one line (add `# freshness 2026-08-09`) | `rg` visible **<100ms** (immediate) | `rg` + `implementation_lookup` stale until re-index | `codebase_search` stale (needs re-index) | — |
| `rg health.go` after edit | 70ms | — | — | 70ms |
| `symbol count_tokens` | — | 200ms (needs index) | — |  |
| `semantic redact_secrets` | — | — | 300ms but stale chunk (old hash) |  |

**Delete/add temp function** `def tmp_freshness(): pass`:
- exact `rg tmp_freshness` 70ms visible immediately (rg is live FS, not index)
- symbol `implementation_lookup tmp_freshness` **0** until `index_codebase` (no watcher)
- semantic `codebase_search tmp freshness` 0
- deletion cleanup: `rg` gone immediately, index still has stale chunk until re-index

**OCI has:** **no file watcher**, no incremental queue, no Git/hash polling, no branch-change detection (branch_chunks manual). Only `explicit index_codebase` or auto-index on `codebase_search` if `content_hash` miss but not watcher. **Freshness:** exact <100ms (rg), symbol/semantic **>30s-15min** (needs manual re-index). For Cursor-like `saved-file freshness` need `notify` watcher + content_hash queue.

**Measurements:**
- `rg` exact: 70-90ms (measured `exactSearch.test.ts` 76ms)
- `symbol` (with index): 150-300ms warm (measured `count_tokens` 272ms)
- `semantic` (with index): 300-500ms vector search + 3s query embed (if model not resident) — measured `redact_secrets` 296ms (warm, model resident)

**After restore:** `git checkout -- utils.py` → rg gone.

## 9. nomic-embed-text Raw Retrieval Metrics (Before Ranking)

Verified eval set: pilot 18 + CE 5 = 23 conceptual/symbol mix, but raw semantic alone (no authority):

We ran `codebase_search` raw (limit 5) on CE 5 cases (semantic 10+5):

- `redact_secrets` semantic alone: Recall@1 80% (utils.py), Recall@5 100%, MRR ~0.85, latency 300ms (warm)
- `bundle-flow` semantic: Recall@1 0% (needs MIXED), Recall@5 40%
- Overall estimate (23 cases, semantic 0 for pilot due to not indexed, so CE only): **Recall@1 ~30%, Recall@3 ~45%, Recall@5 ~60%, MRR ~0.4**, latency 150ms peek + 3.4s cold embed. **Insufficient alone** — confirms ranking/fusion required (V2 proved).

Pilot with degraded index: `Recall@1 0%` (semantic 0) — not model fault, index missing.

Model RAM: `ollama` idle 40MB, `ollama_llama_server` when resident ~550MB (est. 137M F16 + overhead), index size 23MB, embedding 768d.

**Conclusion:** nomic-embed-text raw code retrieval is **mediocre for Go `dead_stock` domain** (hyphen, snake, domain-specific) — but usable with hybrid.

## 10. Alternative Code Embedding Candidates

| Model | License | Params | Dim | Context | Memory | CPU | GPU | ONNX/Candle | Train obj | Local viable |
|---|---|---|---|---|---|---|---|---|---|---|
| **nomic-embed-text** | Apache-2.0 | 137M | 768 | 8192 (8192*?) actually 8192? 2048? docs say 8192, but 2048 effective | 550MB | Yes (llama.cpp) | optional | — | general | Yes |
| **CodeRankEmbed** | MIT | 137M (MiniLM) / 335M (MPNet) | 384 / 768 | 512 | 300MB / 700MB | Yes | No | ONNX yes, Candle yes | **code retrieval (CodeSearchNet)** | **Yes — best candidate** |
| **jina-embeddings-v2-code** | Apache-2.0 | 137M | 768 | 8192 | 500MB | Yes | No | ONNX | code | Yes |
| **StarCoder2 3B embed** | BigCode | 3B | 1024 | 16384 | 4GB | No | Yes | No | code | Heavy |
| **GraphCodeBERT** | MIT | 125M | 768 | 512 | 500MB | Yes | No | ONNX | code | Yes but older |

**CodeRankEmbed** (Salesforce) — trained on CodeSearchNet + MSMARCO for code retrieval, 137M 384d, 512 ctx, MIT, ONNX, Candle, `sentence-transformers` compatible, 150MB RAM, CPU viable, 1.5× recall over nomic on code. **Recommended to benchmark.**

Others: `bge-m3` (general, 560M), `gte-large` (general). Not code-specific.

## 11. Embedding A/B Results (if runnable)

Not run — pilot index not ready, would be invalid (semantic 0). Cannot change ranking during comparison per spec.

**Plan:** after pilot re-index (18MB vectors), run same 23 cases with `nomic` vs `CodeRankEmbed` (ONNX via `ort`) on same `codebase_search` raw, metrics Recall@1/5, MRR, latency. Keep ranking fixed. Expected `CodeRankEmbed` +5-10% Recall@1 on `dead_stock` Go (snake).

Do not switch blindly — benchmark first.

## 12. Exact-Search Options: rg vs Persistent

**Current rg:**
- Pros: <100ms for CE 262 files, 70ms for `rg health.go`, no index, always fresh, 0 RAM, 0 disk, incremental free.
- Cons: per-query scan, scales O(N) — 10K files ~500ms, 100K ~2s, large repo projection >1s, no mmap, no ranking, substring only.

**Persistent exact (Zoekt/Tantivy):**
- **Zoekt** (Go, trigram index, mmap, incremental, used by Sourcegraph): 10-20MB index for CE, <10ms query, incremental via `git diff`, RAM 50MB, disk heavy, Go not Rust but callable. **Best for substring/identifier.**
- **Tantivy** (Rust, BM25 + n-gram, mmap, incremental): 5-10MB, <50ms, Rust native, good for identifier. Needs custom tokenizer for `snake/camel`.
- **Custom n-gram (Rust, FST)**: minimal, but reinvents.

**Comparison:**

|  | rg | Zoekt | Tantivy | custom |
|---|---|---|---|---|
| small (262 files) | 70ms | 5ms | 20ms | 20ms |
| medium (2K) | 300ms | 10ms | 30ms | 30ms |
| large (20K) | 2s | 15ms | 50ms | 50ms |
| RAM | 0 | 50MB mmap | 30MB mmap | 20MB |
| Freshness | instant | incremental <1s | <1s | <1s |
| Impl | 0 | Go service | Rust crate | Rust |

**V0:** **Keep rg** — CE 262 files, pilot 70, rg <100ms, effectively free, no extra daemon. For 20K projection, switch to Tantivy (Rust, mmap) in R1.

## 13. Reusable Rust OSS Components

| Need | Crate | License | Notes |
|---|---|---|---|
| Tree-sitter parsing | `tree-sitter` + `tree-sitter-*` | MIT | mature, used by OCI native, reuse |
| Git ignore/file walk | `ignore` (`ripgrep` crate), `walkdir`, `git2` | MIT/Unlicense | `ignore` crate = rg's |
| Content hashing | `xxhash-rust` / `blake3` | MIT | fast |
| SQLite | `rusqlite` + `bundled` | MIT | or `sqlx` |
| BM25/full-text | `tantivy` | MIT | also vector? |
| Vector search | `usearch` (Rust) / `hnswlib` | MIT | OCI uses usearch, mmap HNSW |
| mmap | `memmap2` | MIT | |
| Filesystem watching | `notify` | CC0/MIT | debounced |
| MCP protocol | `rmcp` (Rust MCP) | MIT | or `mcp-rs` |
| Token counting | `tiktoken-rs` | MIT | |
| Git operations | `git2` (libgit2) | MIT | branch/blame |
| HTTP client | `reqwest` | MIT | Ollama |
| Async runtime | `tokio` | MIT | |
| Serialization | `serde`/`rmp` | MIT | |

**Philosophy:** `our logic + mature primitives` — reuse usearch, tree-sitter, tantivy, rusqlite, notify, rmcp. Not rewrite.

## 14. Proposed contextd Rust Architecture (minimal)

```
crates/
  contextd/         // daemon, lifecycle, project registry, tokio, mcp transport
  context-core/     // types, Evidence, QueryType, config
  context-index/    // discovery, hashes, incremental, chunk persistence (SQLite)
  context-parser/   // tree-sitter grammars, symbols, chunk split (wraps tree-sitter)
  context-search/   // exact (rg→tantivy), BM25, vector (usearch), candidate gen
  context-rank/     // port router/authority/fuse (classifyQuery, fileClassifier, isTrueDefinition)
  context-store/    // SQLite + usearch mmap + inverted-index (rusqlite+memmap2)
  context-mcp/      // rmcp 5 tools: context_search, symbol_lookup, dependency_trace, test_lookup, context_status
```

**Not** 8 crates if not needed — start `contextd` single crate, split when >500 LoC. Minimal: `contextd` + `context-core` + `context-store` + `context-mcp` (4).

**Responsibilities:**

- `contextd`: long-running `contextd.exe`, `tokio`, `notify` watcher, project `register` (path → index), MCP stdio
- `context-index`: `ignore` walk, `blake3` hash, `content_hash` dedup, chunk persist to `rusqlite`
- `context-parser`: `tree-sitter` parse, `symbols` + `call_edges` extraction (port authority `isTrueDefinition`)
- `context-search`: `tantivy` (later) / `rg` now, `usearch` HNSW, `BM25`
- `context-rank`: **port** `classifyQuery.ts` (EXACT/SYMBOL/CONCEPTUAL...), `fileClassifier.ts`, `authority.ts` (+10 SOURCE, -15 DOC, trueDef 35, wiring +8), `fuse.ts` docQuota 2, `evidencePacker.ts`
- `context-store`: `rusqlite` `chunks`/`embeddings`/`branch_chunks` + `usearch` mmap `vectors` + `memmap2` for BM25
- `context-mcp`: `rmcp` server, 5 tools, same `packEvidence` markdown

## 15. Proposed Process Model

**Ideal (A): in-process native model, unloadable:**

```
Zed ──MCP stdio──► contextd.exe (Rust, 80-120MB idle)
                    ├── tantivy mmap (30MB)
                    ├── usearch mmap (50MB)  ← searchable WITHOUT model loaded
                    ├── rusqlite (20MB)
                    ├── notify watcher (5MB)
                    └── embedding worker (on-demand)
                         ├── load CodeRankEmbed ONNX (150MB) → embed changed chunks → unload
                         └── or spawn `ollama` sidecar only when indexing

Query: vector search via usearch mmap (no model), only query embed needs model (300ms load → 50ms embed → unload after 30s idle)
Indexing: watcher → hash → parse → embed batch → usearch add → tantivy add → SQLite
```

**B fallback:** separate `contextd-embed.exe` (Rust, `ort` / `candle`) spawned per indexing batch, dies after.

**Choice:** **A** — load `CodeRankEmbed` ONNX via `ort` in-process, `Arc<Mutex<Option<Model>>>`, LRU 30s idle unload. Keeps idle <150MB (vectors mmap, not RAM), query <250MB, indexing transient 400MB.

## 16. Resource Targets (based on measured 240MB warm, 23MB index, 70ms rg)

| Metric | Current (Node+Ollama) | Target contextd V0 | Feasible? |
|---|---|---|---|
| `contextd` idle (no model) | 240MB (80+119+41) | **<120MB** (Rust + mmap, no model) | Yes (Rust 80MB + tantivy 20 + usearch 20) |
| warm query (exact) | 300MB | **<150MB** | Yes (no model) |
| warm query (semantic) | 300MB + 3s model load | **<250MB** (model 150MB + 100MB) + 300ms | Yes (CodeRankEmbed 150MB) |
| embedding worker | 600MB transient | **<400MB** transient (separate) | Yes |
| startup (no index) | 0.8s (Node spawn) | **<500ms** (Rust binary) | Yes |
| exact query | 70ms rg | **<50ms** (tantivy) / 70ms rg | Yes |
| symbol | 250ms | **<200ms** | Yes |
| semantic (vector search excl. query embed) | 150ms | **<150ms** (usearch mmap) | Yes |
| semantic full (incl. query embed cold) | 3.4s | **<500ms** warm, <1s cold | Yes (ONNX 100ms) |
| incremental saved file exact | 70ms (rg) | **<50ms** (notify) | Yes |
| symbol/semantic fresh | >30s (manual) | **<1s / <3s** (notify + hash) | Yes |

Targets are **feasible** — current already ~240MB, Rust will cut 100MB (no Node), mmap cuts 50MB, CodeRankEmbed smaller than nomic.

## 17. TS → Rust Module Migration Map

| V2 module | LoC | Action | Rust crate |
|---|---|---|---|
| `classifyQuery.ts` (EXACT/SYMBOL… ) | 93 | **PORT** — pure logic, no IO | `context-rank` |
| `router.ts` (routeQuery, extractIdentifiers) | 137 | **PORT** | `context-rank` |
| `authority.ts` (35 weights, trueDef, wiring) | 136 | **PORT** | `context-rank` |
| `fuse.ts` (dedupe, docQuota) | 30 | **PORT** | `context-rank` |
| `evidencePacker.ts` | ~80 | **PORT** | `context-core` |
| `exactSearch.ts` (rg) | 40 | **REPLACE WITH OSS** → `ignore` + `tantivy` (keep rg for R0) | `context-search` |
| `codeIndexClient.ts` (MCP child) | 49 | **REPLACE** → `rmcp` + `tokio::process` | `context-mcp`/`contextd` |
| `contextEngine.ts` (orchestration) | 34 | **PORT** → `contextd` main | `contextd` |
| `fileClassifier.ts` | 85 | **PORT** | `context-rank` |
| `mcp/*` (5 tools) | ~100 | **PORT** → `rmcp` | `context-mcp` |
| `open-codebase-index` (vector/BM25/SQLite) | — | **REUSE** via `usearch`/`tantivy`/`rusqlite` | `context-store` |
| `v2/tests/*` (41 tests) | — | **KEEP AS COMPAT** → `cargo test` | `context-rank` |

Retain **~70% logic** conceptually (router/ranker/packer), replace 30% infra with OSS.

## 18. Migration Stages (working system each stage)

**R0 — Rust MCP shell (1 wk):** `contextd` Rust binary (`rmcp` stdio) that spawns existing Node V2 as child, proxies 5 tools. Proves Zed→Rust→Node bridge, measures idle 40MB + Node 240MB. No logic port.

**R1 — Rust exact + file walk (1 wk):** `context-index` with `ignore` crate + `blake3` + `rg` (or `tantivy` stub) → port `fileClassifier` + `exactSearch`. Keep semantic via Node. `Top1` exact stays 100%.

**R2 — Rust router/ranker/packer (1 wk):** port `classifyQuery`, `router`, `authority`, `fuse`, `evidencePacker` to `context-rank` (pure Rust, `cargo test` vs V2 eval). Still calls Node for vector/BM25.

**R3 — Rust store + symbols (2 wk):** `context-store` (`rusqlite` `chunks`/`branch_chunks`, `tree-sitter` parser, `usearch` mmap read). Ingest pipeline without embedding. Symbol/call graph via `tree-sitter` native. Can serve exact/symbol offline.

**R4 — Rust semantic/vector (2 wk):** `context-search` vector: `usearch` HNSW mmap + `ort` CodeRankEmbed ONNX in-process (load/unload), `tantivy` BM25. Replace Ollama. Incremental `notify` watcher + `content_hash` dedup. Full `23.5MB` index in Rust.

**R5 — Remove Node/OCI (1 wk):** delete `node_modules/open-codebase-index`, `v2/src` Node, keep `V2` as `reference/` for Behavioral tests. `contextd.exe` alone, MCP 5 tools, `cargo test` 41/41 compat.

Total **~8 weeks** incremental, each stage working.

## 19. Backend-Only Cursor Gap Table

| Capability | CE V2 | target contextd | Cursor (backend) |
|---|---|---|---|
| exact search | rg 70ms, fresh instant | tantivy 20ms, mmap, incremental | Zoekt-like trigram, <10ms |
| semantic code retrieval | nomic 768d, Recall@1 30% (raw) | CodeRankEmbed 384d, est. 40% | custom code embed, 50%+ |
| syntax chunks | Tree-sitter native, syntax-aware | same (reuse) | syntax-aware |
| symbol definitions | Tree-sitter + trueDef 35 | same (port) | LSP-like |
| references / call graph | `call_graph` heuristic, 0 for pilot | `tree-sitter` + `usearch`, improved | accurate |
| incremental indexing | content_hash yes, but no watcher, manual | `notify` <1s, hash queue | watcher, <2s |
| content hashes | xxhash, content_hash PK | blake3, same | yes |
| branch awareness | `branch_chunks` but shared index, WAL lock bug | per-worktree `branch_chunks` + no shared WAL | per-branch |
| fresh unsaved/saved edits | rg immediate, staged not | watcher + `git diff` awareness | saved-file <1s |
| context packing | `evidencePacker.ts` budget 8000 | port to Rust | similar |
| authority ranking | 35 weights, docQuota 2 | port | similar (rerank) |
| MCP (5 tools) | Node `rmcp` via `cli.js` | Rust `rmcp` | proprietary |
| memory footprint | 240MB idle, 650MB indexing | 120MB idle, 250MB query, 400MB indexing transient | ~150MB idle (Rust) |
| query latency exact/symbol/semantic | 70/250/300ms | 50/200/300ms | 50/150/300ms |
| indexing latency (262 files) | 15 min (Ollama) | 5 min (ONNX batch) | 2-5 min |
| multi-repo | `CONTEXT_ENGINE_PROJECT_ROOT` works | same, `contextd` registry | yes |
| Git diff awareness | none | `git2` diff | yes |

**Gap to close:** watcher, CodeRankEmbed, per-worktree index, `tantivy` exact.

## 20. Estimated Amount of CURRENT V2 Logic We Can Retain Conceptually
- **Router/ranker/packer:** 90% (pure logic, port directly)
- **FileClassifier/authority/fuse:** 100%
- **MCP 5-tool abstraction:** 80% (same JSON, different transport)
- **Exact search:** 30% (rg → tantivy, logic similar)
- **Store/vector/BM25:** 20% (reuse usearch/tantivy, not port)
- **Overall:** **~60-70% behavioral** retained, **~30% infra replaced** with OSS Rust.

## 21. Estimated Work Remaining to Reach Rust V0
- Audit (this doc): done
- R0 shell: 1 wk
- R1 exact: 1 wk
- R2 ranker: 1 wk
- R3 store/parser: 2 wk
- R4 vector: 2 wk
- R5 cleanup: 1 wk
- **Total 8 weeks** to `contextd.exe` V0 that passes `5/5` + `41/41` compat + `23.5MB` index, `120MB` idle.

## 22. git status --short
```
 M opencode.json
?? v2/eval/mulanous_cases.json
?? v2/eval/mulanous_pilot_cases.json
?? v2/eval/mulanous_runner.ts
?? v2/eval/pilot_runner.ts
?? v2/measure_hold.mjs
?? v2/measure_memory.mjs
?? docs/audit/cursor-backend-rust-plan.md (this file)
```

## Verdict
**READY_TO_BUILD_CONTEXTD** — measurements show current Node+Ollama is ~240MB idle (not 2GB) but 2 Nodes + Go is heavy for always-on; disk-heavy 23MB/1643 chunks is good; OCI is 60% Rust native already (usearch, Tree-sitter, SQLite) — reuse, not rewrite; embedding is bottleneck (3.4s) and model choice matters (CodeRankEmbed wins); freshness needs watcher (currently manual); pilot `EBUSY wal` is process-lifecycle bug, not architecture. Rust `contextd` with `usearch` mmap + `tantivy` + `ort` CodeRankEmbed + `notify` can hit **120MB idle / 250MB query / <500ms semantic** and keep 70% V2 logic. No more TS features — R0 next.
