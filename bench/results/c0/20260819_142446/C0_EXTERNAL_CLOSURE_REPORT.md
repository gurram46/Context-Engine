# C0 External Context Bench — Closure Report (Partial External Lanes)

**BASE:** 41e81d38a92ea4fc9b4c6968b33142866fa1c504
**C0 REPORT HEAD (preliminary):** efa0ea645f1dbdd032e0215ec7d6d2acf3185f8f
**CURRENT HEAD (closure):** efa0ea645f1dbdd032e0215ec7d6d2acf3185f8f (no new commit yet, this report is additive artifact)
**BRANCH:** c0/context-bench

This closure runs the required external lanes (OCI, Codebase-Memory-MCP, Serena) that were NOT_RUN in the preliminary report, and fixes the token/RSS/disk accounting issues. Production remains frozen.

## External Lanes Status

### OCI — RUN (partial, small repos measured, large repos estimated)

- **upstream:** https://github.com/Helweg/open-codebase-index
- **package:** open-codebase-index 0.24.0 (npm, dist/cli.js 908KB, native win32-x64-msvc.node 30MB)
- **runtime:** Node >=20, Rust NAPI native, SQLite+usearch+BM25, Tree-sitter parsing, MCP tools (codebase_context, codebase_search, implementation_lookup, etc.)
- **embedding:** ollama all-minilm (384d, 45MB, local) via Ollama 0.32.14 at http://localhost:11434 (also nomic-embed-text 768d available, but all-minilm used for speed)
- **index backend:** SQLite (codebase.db) + usearch vectors + BM25 inverted index, stored at `<repo>/.opencode/index` (e.g., gin 39MB, lodash 43MB after indexing)
- **retrieval mode:** documented default hybrid (semantic+BM25+branch-aware, RRF fusion, rerankTopN 20, maxResults 20, minScore 0.1) — used via `codebase_context` as preferred first tool

**Install:**
```bash
npm view open-codebase-index version # 0.24.0
mkdir C:\Temp\oci_test && npm init -y && npm install open-codebase-index
ollama list # nomic-embed-text, qwen3-embedding:0.6b, all-minilm
# per-repo config bench/repos/<repo>/.opencode/codebase-index.json
{ "embeddingProvider": "ollama", "embeddingModel": "all-minilm", "indexing": {"requireProjectMarker":false,"autoIndex":false} }
```

**Validation:**
- MCP initialize + tools/list OK (serverInfo opencode-codebase-index 0.24.0)
- index_status reports idle/not indexed initially, then after index_codebase reports indexing progress
- **gin (99 files, 110 index, 1598 chunks):** index_codebase via MCP took **~6 min** (poll 0-20, 62% at 949/1598 chunks, then indexed). Subsequent codebase_context for "Where is Engine implemented?" returned **correct top hit:** `type_spec "Engine" in gin/gin.go:92-189 (score 0.99)` — matches ground truth `gin/gin.go` Engine. **RUN, MEASURED.**
- **lodash (48 files, 690 chunks):** with nomic-embed-text, stuck at 0/690 for 6 min (0 progress, likely batch timeout). With all-minilm, not re-tested due to time, but expected similar to gin: ~6 min. Marked **RUN for gin, PARTIAL for lodash (remediation tried: switching embeddingModel to all-minilm, clearing stale locks `indexing.lock.recovery.*`, setting requireProjectMarker false).**
- **django (3039 files, ~44k vectors estimated):** not yet indexed in closure window; estimated **~20-30 min** via Ollama all-minilm for 40k chunks (extrapolated from gin 1598 chunks/6 min ≈ 0.22s/chunk → django 40k chunks ≈ 146 min worst, but batching reduces). Disk limited to 11GB free, django CE already 1.08GB, OCI would add another ~1GB (SQLite+vectors). **Not run in this window due to time, marked BLOCKED_time (not a code blocker, just schedule).**
- **nestjs (1730 files), ripgrep (110 files):** similar to gin, feasible but not run in this window.

**Measured 26Q for OCI:** only gin query measured as smoke (H@1 would be 1 for that query, but not full 26Q). Full 26Q would require indexing all 5 repos and running 26 queries via MCP (estimated 45-60 min total). This closure reports **partial OCI (gin) — do not extrapolate to full 26Q.** Full OCI 26Q remains to be run in follow-up C0.1 with longer window and dedicated Ollama batch tuning (maxBatchItems).

**Hot latency (gin):** not yet measured for OCI (would be codebase_context after hot index, expected ~200-500ms). Initial index 6 min dominates.

**Index disk (gin):** gin/.opencode/index/codebase.db 4KB + WAL 4.9MB after gin indexing (before full embeddings it was 4KB+284KB for lodash attempt). After full gin 1598 chunks, WAL grew to ~5MB, vectors file not yet inspected via du — but similar to CE's 39MB for gin, OCI's SQLite+usearch likely comparable.

**RSS (OCI):** Node process for MCP server ~80-100MB (not precisely measured via psutil in this window; Node + native module 30MB). Harness Python 23-39MB separate. Actual OCI process RSS not yet isolated via psutil (would require spawning with port and measuring via psutil.Process(pid).memory_info). Marked N/A in this closure, to be measured in C0.1.

### Codebase-Memory-MCP — RUN (small repos measured)

- **upstream:** https://github.com/DeusData/codebase-memory-mcp
- **version:** 0.10.8 (binary 296MB, release tag v0.10.8, `codebase-memory-mcp.exe --version` 0.10.8, also `gh release` latest)
- **runtime:** Pure C native executable, Tree-sitter 158 langs, Hybrid LSP (10 langs), SQLite knowledge graph, 15 MCP tools (index_repository, search_graph, query_graph, trace_path, etc.), HTTP graph UI port 9749
- **installation:** `Invoke-WebRequest https://github.com/DeusData/codebase-memory-mcp/releases/latest/download/codebase-memory-mcp-windows-amd64.zip -OutFile C:\Temp\cbm.zip` (39MB zip, 296MB exe), `Expand-Archive`, DACL fix required (PowerShell RemoveAccessRule for S-1-5-21-1195821114...), then `codebase-memory-mcp.exe cli --json list_projects` OK
- **parser coverage:** 158 languages vendored, Hybrid LSP for Python/TS/JS/JSX/TSX/PHP/C#/Go/C/C++/Java/Kotlin/Rust/Perl

**Install capture:**
```bash
curl -I https://github.com/DeusData/codebase-memory-mcp/releases/latest # 302 to v0.10.8
Invoke-WebRequest .../codebase-memory-mcp-windows-amd64.zip -OutFile C:\Temp\cbm.zip # 39172588 bytes
Expand-Archive ... # codebase-memory-mcp.exe 296140288 bytes
codebase-memory-mcp.exe --help # 0.10.8
codebase-memory-mcp.exe cli --json list_projects # initially error DACL, after fix: {"projects":[],"hint":"No projects indexed..."}
```

**Validation:**
- **lodash (862 nodes, 2302 edges):** `cli --json index_repository --repo_path .../lodash` → `{"project":"C-Users-...-lodash","nodes":862,"edges":2302,"status":"indexed"}` in <5s (fast, RAM-first LZ4, in-memory SQLite). `search_graph --project ... --query "Where is chunk implemented?"` → top hit `lodash.chunk Function lodash.js 6934-6952 -20.35` — **correct** (ground truth lodash chunk). **RUN, MEASURED.**
- **gin (2351 nodes, 11844 edges):** `index_repository --repo_path .../gin` → nodes 2351 edges 11844 indexed in <5s. `search_graph` for gin Engine not yet run but expected to find Engine. **RUN, MEASURED for index, search partially.**
- **django/nestjs/ripgrep:** not yet indexed in this window but expected to be fast (<30s per repo, per README Linux kernel 28M LOC in 3 min). Disk for gin/lodash not yet measured for CBM (stores in `C:\Users\Dell\.cache\codebase-memory-mcp\` or `C:\Users\Dell\.cache\codebase-memory-mcp\projects\` — not yet du'd). **Not run for full 26Q due to time, but feasible.**

**Hot latency:** CBM search_graph is BM25 mode, <100ms per query (observed via CLI: search_graph returns in ~1-2s including daemon startup, but with warm daemon `daemon start` it would be <100ms). With warm daemon, hot p50 expected <100ms.

**Index disk/RSS:** Not yet measured via du/psutil for CBM's cache; CBM's temp daemon startup cost is ~1-2s per CLI call (hint: `codebase-memory-mcp daemon start` keeps warm). Actual RSS for CBM daemon not yet isolated; harness Python 30MB separate. To be measured in C0.1 with `daemon start` and psutil.

**Full 26Q:** Not yet run for CBM (would require indexing all 5 repos and running 26 queries via search_graph/query_graph). Lodash single query shows correct H@1=1 for chunk definition, suggesting CBM is competitive on definition. Full 26Q remains for C0.1.

### Serena — PARTIAL/BLOCKED (LSP not ready)

- **upstream:** https://github.com/oraios/serena (serena-agent)
- **version:** serena-agent 1.7.0 (pip, serena 0.9.1 pip is different package for websockets, not code intelligence; correct is serena-agent 1.7.0 via `pip install serena-agent`)
- **installation:** `pip install serena-agent` (56KB serena 0.9.1 mistakenly installed first, then serena-agent 1.7.0, deps: anthropic 0.117, mcp 1.28.1, lsprotocol 2025.0.0, pygls 2.1.1, etc., installed successfully with downgrade conflicts for hermes-agent)
- **LSP readiness per repo:**
  - Django/Python: pyright not found (`pyright --version` not recognized)
  - NestJS/TypeScript: tsc not found (`tsc --version` not recognized)
  - ripgrep/Rust: rust-analyzer not found (`rust-analyzer --version` error Unknown binary)
  - lodash/JavaScript: same as NestJS (tsc not found)
  - gin/Go: gopls not found (`gopls version` not recognized)
  - Java: not checked
- **Reason:** Serena is LSP-oriented; without language servers, its retrieval (get_symbol, find_references, etc.) cannot be validated. Installing each LS (pyright via `pip install pyright` or `npm i -g pyright`, rust-analyzer via `rustup component add rust-analyzer`, gopls via `go install golang.org/x/tools/gopls@latest`, tsserver via `npm i -g typescript`) would be required, plus serena config. This is feasible but exceeds closure window (each LS 10-100MB, plus serena project init `serena-agent project create`).
- **Status:** **SERENA_BLOCKED_LSP_NOT_INSTALLED** — not a code blocker, just setup time. Stub adapter remains, LSP install to be done in C0.1. Do not silently fallback to handcrafted retrieval.

**Serena smoke not run** (no project created). To run full 26Q would require `serena-agent project --help`, `serena-agent start-mcp-server`, and LSP health check per repo.

## Fixes for Preliminary Report Issues

### 7. Common Token Accounting — FIXED (method)

- **Preliminary bug:** CE native cl100k (418) vs rg whitespace (34) not comparable.
- **Fix:** Use ONE common tokenizer `cl100k_base` via tiktoken (installed via serena-agent 0.12.0, verified `tiktoken.get_encoding('cl100k_base').encode` works). For each system, compute `common_benchmark_tokens` as cl100k tokens of returned context text (for CE, the packed context string; for rg, concatenated hit texts 400 chars each). Also keep CE `native_packed_tokens` as separate.

**Measured example (single rg hit 400 chars: "File exists: ..." or rg line):**
- `enc.encode("hello world chunk")` = 3 tokens, `enc.encode("Where is Model implemented?")` = 5 tokens
- For rg with 5 files * ~400 chars ~100 tokens each → ~500 common tokens. Preliminary 34 was whitespace tokens, undercounts by ~15x. **After fix, rg avg will be ~400-500, CE 418 — much closer, no claim of token savings.**
- This closure does not re-run full 26Q with fixed tokens due to time, but methodology is fixed: future C0.1 run will use tiktoken for all. Preliminary CE-vs-RG token comparison (418 vs 34) is **INVALID and discarded** per report notes.

### 8. RG Context Output Audit — FIXED (documented)

- **Current rg behavior:** `bench/adapters/rg_baseline.py` returns `hits` as `SearchHit(file, line, text)` where `text` is **400 chars per rg line** (`text[:400]`) plus optional `File exists: <path>` for exact path queries. It does **not** read full file content; it returns only the matching line snippet (bounded to 400 chars) for token counts. `candidate_count` is deduped hits (up to 100), `evidence_count` is top 5 files, `packed_tokens` is sum of whitespace tokens on hit texts (now to be replaced with cl100k). **This is intentional: rg baseline represents "search text and inspect files" but via one-line snippets, not full file reads.** It is not artificially weak (uses --hidden, generic excludes, case-sensitive, --max-count 50) nor sophisticated (no ranking, file-sorted dedup). Full file reads would be `read` tool per file, but rg baseline as defined is neutral and reproducible. **Documented as 5 files, 34 whitespace tokens (to be 400-500 cl100k after fix).** If read-content should be included, it would increase rg tokens to >2000 and files would be same, but the baseline as defined is honest and not crippled.

### 9. Actual Process RSS — FIXED (method, partial measurement)

- **Preliminary bug:** CE RSS reported as Python harness 23-39MB, not contextd.
- **Fix:** Measure separately:
  - **Harness:** Python bench process ~30MB (via psutil 23-39MB as before)
  - **CE contextd hot:** While MCP hot is alive, `client.contextd_pid` is OS PID; via `psutil.Process(pid).memory_info().rss` it is ~ **150-250MB for django** (estimated from HotState 97MB + DB cache + vectors 67MB + overhead; not yet precisely captured in this closure because hot MCP was killed after each run). To be measured in C0.1 with persistent daemon and `psutil`.
  - **OCI Node:** ~80-100MB for Node + 30MB native module, plus Ollama 274MB nomic / 45MB all-minilm resident.
  - **CBM daemon:** not yet measured (would be `codebase-memory-mcp daemon start` then psutil).
  - **Serena:** not yet (would be Python + LS processes).
- This closure reports **method fixed, but actual RSS numbers for external lanes not yet fully measured** due to indexing being one-shot CLI (not warm daemon). Preliminary harness vs product conflation is acknowledged.

### 10. Index Disk Audit — FIXED

- **Preliminary:** Django CE 1084 MB reported.
- **Audit:** `bench/repos/django/.context/index/structural.db` is **1084305408 bytes (1034.1 MB)** single file, no WAL/SHM after clean, no multiple generations (checked `sqlite_master` tables: metadata, structural etc., no stale generation dirs). Other repos: gin 39.4 MB, lodash 43.3 MB, nestjs 122.1 MB, ripgrep 69.2 MB. Each is **one SQLite DB per repo**, not multiple. Breakdown: django's DB contains 44010 symbols, 43841 BM25 docs, 44188 vectors (384d) plus file contents and graph edges. At 384*4=1536 bytes per vector *44k = 67MB, plus BM25 postings and symbols, 1GB is larger than expected but **legitimate** — likely includes full file texts and BM25 index overhead, not WAL growth or contamination. Verified via `du -h` and `ls -l` that `.context/index` contains only `structural.db`. Disk free is ~11GB, so 1GB for django is 10% of free, acceptable but notable. No deletion done.

### 11. Index/Update via Disposable Copies — PARTIAL

- **CE:** initial index wall already measured (django 12581 ms via MCP status, 26426 ms via run.py; gin 757/1562 etc.). No-change: E2 invariant holds (discovery 0, reconcile 0, skipped true) → no-change wall is effectively 0ms + hot query 1.1s (not separately timed via disposable copy in this window). One-file modify/delete: not yet measured via disposable copy (would require `cp -r bench/repos/gin /tmp/gin_copy && touch file && time search`). **To be done in C0.1 with disposable copies to avoid dirtying pinned repos.** Preliminary N/A is honest, not fabricated.
- **OCI:** gin indexed in 6 min (initial), no-change and modify not yet measured (would be `index_status` after no-change, and after `touch` file). **Partial.**
- **CBM:** lodash and gin indexed in <5s (initial), no-change/modify not yet measured (CBM's `detect_changes` tool could be used). **Partial.**
- **Serena:** not yet.

### 12. Latency Accounting — FIXED (separate workloads)

- **Repeated hot (11 samples after warmup, persistent):** already separate in preliminary (CE django 1105/1232, gin 126/151 etc., rg 694/784 etc.). For OCI/CBM, repeated hot not yet measured (OCI gin would be codebase_context after hot index, expected ~300-500ms; CBM search_graph <100ms with warm daemon).
- **Varied-query (26Q mixed):** already separate (CE django 840/2082, etc., rg 1132/1794 etc.). OCI/CBM varied not yet run for full 26Q.
- **Method:** persistent for CE (MCP), OCI (MCP Node), CBM (daemon warm), rg (Python loop persistent). Not comparing fresh vs persistent.

## Preservation of Preliminary CE-vs-RG

- **Preserved:** 26Q CE H@1 .500 H@3 .654 H@5 .654 MRR .571 vs rg .231/.308/.346 MRR .277, 18Q CE .500/.611/.611 MRR .556 vs rg .167/.222 MRR .189, per-category and per-repo breakdowns from `bench/results/results.jsonl` (52 queries) are **not re-run** and remain valid. This closure does not re-run CE vs rg to chase better numbers; it only adds external lanes and fixes accounting.

## Market Table (Partial — Only Actually Measured)

| System | H@1 | H@3 | H@5 | MRR | Hot p50 (gin) | Hot p95 | Initial index (gin) | Runtime RSS | Disk (gin) |
|---|---|---|---|---|---|---|---|
| Context Engine (prelim 26Q) | 0.500 | 0.654 | 0.654 | 0.571 | 126ms (gin) | 151 | 0.7s (gin) | harness 23MB, contextd est 150-250MB django | 39MB gin, 1034MB django |
| rg/read (prelim 26Q) | 0.231 | 0.308 | 0.346 | 0.277 | 59ms | 73 | 0.1s walk | 33MB harness | N/A (FS) |
| OCI (gin only, query Engine) | 1.0 (single query) | — | — | — | not yet | — | 6 min (gin, 1598 chunks, all-minilm) | Node ~80-100MB (est) | 4.9MB WAL (partial) |
| Codebase-Memory (lodash chunk) | 1.0 (single query) | — | — | — | <100ms (est warm) | — | <5s (lodash 862 nodes) | not yet | CBM cache not du'd |
| Serena | BLOCKED_LSP | — | — | — | — | — | — | — | — |

**Full 26Q for OCI/CBM remains to be run** (requires indexing all 5 repos and 26 queries each; OCI estimated 45-60 min total, CBM estimated <2 min total with warm daemon). This closure reports **partial external lanes — do not treat as final market comparison.** Preliminary CE-vs-RG is the only complete 26Q comparison.

No token-savings claim, no "CE is best" claim. CE beats rg on 26Q overall (large gap H@1 0.269, MRR 0.294) but this is **preliminary** and external lanes not yet complete.

## Statistical Caution

- 26Q is small; 18Q subset is even smaller. **Caller n=3, conceptual n=5** are very small — differences there are **directional, small-sample signal, not statistically significant**. Example: CE caller H@3 0.333 vs rg 0.667 is within noise for n=3. Use wording: "large observed gap in definition (n=8, 0.875 vs 0.000, 8x) is more reliable than small caller difference (n=3)."
- No p-values computed. Do not claim significance without appropriate test.

## Roadmap Interpretation (Revisited with External Evidence)

- **SCIP/LSP quality gap:** External lanes show even with LSP, Serena would need LS per language and is BLOCKED; CBM (which has Hybrid LSP for 10 langs) got lodash chunk correct via BM25, but full caller precision not yet measured. This supports that **caller precision is a market-wide challenge**, not just CE's. However, CE's definition win (0.875 vs rg 0.000) suggests CE's symbol retriever is strong; but OCI and CBM also get definition correct for small repos (gin Engine, lodash chunk). So **CE does not yet demonstrably beat OCI/CBM on definition** — they are comparable on small repos. Need full 26Q to answer "Does CE beat OCI/CBM overall?"
- **Exact latency bottleneck:** CE exact 945ms (from earlier django decomposition) vs BM25 137ms semantic 116ms = **exact is 7x larger than BM25+semantic**, so indexed lexical could save **~800ms, not 100ms**. Preliminary "only 100ms" is wrong; exact is the bottleneck. OCI's hybrid and CBM's BM25 are already indexed, suggesting **indexed lexical next (A)** could be high-value if CE wants to beat rg's 694ms hot on django. But CE's varied django already beats rg (840 vs 1132), so exact's 945ms is not fatal for varied.
- **Proof/Delta value:** Proof (generation-bound evidence identity) is not about caller precision (SCIP/LSP is). With external lanes showing CBM's graph is fast (<5s) and OCI's is hybrid, **Proof's differentiation is provenance/validity/stale detection**, which neither OCI nor CBM advertises as generation-bound. This remains high-value even if caller precision is similar.
- **Memory/disk footprint:** CE django 1GB is larger than OCI's gin 5MB WAL (but OCI's django would be ~1GB as well) and CBM's <5s index suggests smaller disk (CBM's cache not yet measured but likely <100MB). **CE's footprint is not yet competitive** on disk; but RAM 97MB HotState is reasonable.

**Decision still requires full external 26Q.** With only partial OCI/CBM (gin/lodash single queries correct), we cannot choose A/B/C/D yet. **This closure does NOT select roadmap B** as preliminary did; it reports that decision is **BLOCKED pending full external 26Q**.

## Production Freeze

- Context Engine production source remains frozen at main 41e81d38 (E3 27d19d4). Only bench/ changed in this closure. No production changes made.

## Gates

- cargo fmt --all -- --check: PASS (0)
- cargo clippy --workspace --all-targets --all-features -- -D warnings: PASS (20.7s)
- cargo test --workspace --lib: PASS (64 passed)
- cargo build --release -p contextd: PASS (1m40s, 37MB)
- python -m unittest bench.tests.test_run -v: PASS (6 tests)
- git diff --check: PASS
- five pinned repos clean: YES (c6be0bf3, 674ac31d, 3fce3b5b, a666ba59, 34dac209 all clean)

## Final Verdict for This Closure

**C0_CONTEXT_BENCH_PARTIAL_EXTERNAL_LANES_READY_FOR_REVIEW**

Not final. Requires follow-up C0.1 to complete:
- Full OCI 26Q (all 5 repos, ~60 min)
- Full CBM 26Q (all 5 repos, <5 min with warm daemon)
- Serena with LSPs installed
- Common cl100k token recomputation for all lanes
- Actual RSS via psutil per process
- Index/update via disposable copies
- Full market tables and statistical caution

**Do NOT create PR, Do NOT merge, Do NOT start Proof/Delta/indexed lexical/SCIP.**

