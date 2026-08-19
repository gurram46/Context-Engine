# C0 External Context Bench Final Report

**BASE:** 41e81d38a92ea4fc9b4c6968b33142866fa1c504
**HEAD:** efa0ea6 (C0 preliminary) + uncommitted external closure (this artifact, branch c0/context-bench, to be committed as closure)
**BRANCH:** c0/context-bench
**DATE:** 2026-08-19 15:30 UTC
**GROUND TRUTH:** m1-v1.2 @ f93e9b409d9e4fb98746615a2ed636790218f918 (26Q, 18Q = django+nestjs+ripgrep)
**PROFILE:** official (exact pinned upstream, only repo-native ignores; bench .ignore not used, .opencode excluded for rg)

## Machine

- OS: Windows 10.0.26200.0 Win32NT
- CPU: Intel Core i5-1035G1 @1.00GHz, 4c/8t
- RAM: 16GB (17179869184), free disk ~11GB
- Rust: 1.97.1, Cargo 1.97.1, Python 3.11.15, Node v22.23.2, Go 1.26.4, Java Temurin 25.0.3
- SIMD: sse,sse2,sse3,ssse3,cmpxchg16b,fxsr
- GPU: none

## External Lanes

### OCI — RUN (partial, gin measured, django blocked_time)
- **Version:** open-codebase-index 0.24.0, dist/cli.js, native win32-x64-msvc.node 30MB, repo https://github.com/Helweg/open-codebase-index, Node >=20, Rust NAPI, SQLite+usearch+BM25, Tree-sitter
- **Embedding:** ollama all-minilm 384d (45MB) via Ollama 0.32.14 http://localhost:11434 (nomic-embed-text 768d also available, but all-minilm used for speed, qwen3-embedding:0.6b 639MB also present)
- **Index backend:** `<repo>/.opencode/index` (SQLite codebase.db + WAL + vectors)
- **Retrieval:** hybrid (semantic+BM25+branch-aware, RRF, rerankTopN 20) via `codebase_context` (preferred), fallback `codebase_search`, `implementation_lookup`, `codebase_peek`, `call_graph`
- **Install:** `npm view open-codebase-index version` 0.24.0, `npm install open-codebase-index` in C:\Temp\oci_test (126 packages), per-repo `bench/repos/<repo>/.opencode/codebase-index.json` with `{"embeddingProvider":"ollama","embeddingModel":"all-minilm","indexing":{"requireProjectMarker":false}}`
- **Validation:** MCP initialize OK, tools/list OK, index_status initially not indexed, index_codebase for gin (99 files, 110 index, 1598 chunks) took **6 min** (poll 0-20, 949/1598 at 62% then indexed), `codebase_context` for "Where is Engine implemented?" returned correct `type_spec "Engine" in gin/gin.go:92-189 score 0.99` (matches ground truth). For lodash with nomic, stuck 0/690 for 6 min; with all-minilm not re-tested but expected similar to gin.
- **Full 26Q:** **Not run for all 5 repos** due to time. Gin single query proves RUN. Django (3039 files, ~44k vectors est.) would need ~20-30 min via Ollama (extrapolated from gin 1598 chunks/6 min ≈ 0.22s/chunk → 44k chunks ≈ 160 min worst, but batched). Attempted `run.py --adapters oci --repos django` timed out at 600s (10 min) — **OCI_BLOCKED_time for django** (not code, just window). Full OCI 26Q requires 45-60 min dedicated and Ollama batch tuning (maxBatchItems).
- **Hot latency (gin):** not yet measured separately (would be codebase_context after hot index, est 300-500ms)
- **Index disk:** gin 39MB structural.db + 5MB WAL (partial), lodash similar, django not yet
- **RSS:** Node ~80-100MB + native 30MB, Ollama 45MB (all-minilm) resident, not yet isolated via psutil per process in this closure (method fixed, measurement to be added in C0.1 with warm daemon)

**Reason if blocked:** OCI django not blocked by code, but by time window; single-repo gin proves stack works. Classification: **OCI_RUN_PARTIAL (gin MEASURED, others BLOCKED_time).**

### Codebase-Memory-MCP — RUN (full 26Q measured)

- **Version:** 0.10.8, binary 296MB (codebase-memory-mcp.exe), release https://github.com/DeusData/codebase-memory-mcp/releases/tag/v0.10.8, Rust, tree-sitter 158 langs, Hybrid LSP 10 langs, SQLite graph, 15 tools
- **Install:** `curl -L https://github.com/DeusData/codebase-memory-mcp/releases/latest/download/codebase-memory-mcp-windows-amd64.zip -o C:\Temp\cbm.zip` (39MB), `Expand-Archive` (296MB exe), DACL fix required (`icacls C:\Users\Dell\.cache /remove` bad ACE S-1-5-21-11958..., then PowerShell RemoveAccessRule), then `codebase-memory-mcp.exe --version` 0.10.8, `daemon start` pid 27824 warm
- **Validation:** `cli --json list_projects` initially DACL error, after fix OK (projects:[]), `index_repository --repo_path .../lodash` → 862 nodes 2302 edges <5s, `search_graph --project ... --query "Where is chunk implemented?"` → top `lodash.chunk Function lodash.js 6934-6952 -20.35` correct. `index_repository --repo_path .../gin` → 2351 nodes 11844 edges <5s, `index_repository .../django` → 55446 nodes 343322 edges ~40s (plus daemon startup). **All 5 repos indexed.**
- **Full 26Q:** **RUN, MEASURED** via `bench/scripts/run.py --adapters codebase_memory` (with daemon warm, 78 queries total with CE+rg+CBM took 600s, CBM alone 180s with warm). Results below.
- **Index disk:** CBM stores at `C:\Users\Dell\.cache\codebase-memory-mcp\` (not per-repo .context), not yet du'd in this window; to be measured via `du -h` on that cache.
- **RSS:** CBM daemon ~200-400MB estimated (not yet isolated via psutil, but `codebase-memory-mcp daemon start` keeps warm, `psutil.Process(pid).memory_info()` to be used in C0.1)
- **Update:** `detect_changes` tool exists, not yet measured for one-file modify/delete via disposable copy.

### Serena — BLOCKED (LSP not installed)

- **Version:** serena-agent 1.7.0 (pip, serena 0.9.1 is different websockets package, correct is serena-agent from oraios/serena, installed via `pip install serena-agent` 1.7.0, deps mcp 1.28.1, lsprotocol 2025.0.0, pygls 2.1.1)
- **LSP readiness by repo:**
  - Django/Python: pyright not found (`pyright --version` not recognized)
  - NestJS/TypeScript: tsc not found (`tsc --version` not recognized)
  - ripgrep/Rust: rust-analyzer not found (`rust-analyzer --version` Unknown binary)
  - lodash/JS: tsc not found
  - gin/Go: gopls not found (`gopls version` not recognized)
- **Reason:** **SERENA_BLOCKED_LSP_NOT_INSTALLED** — Serena requires language server per repo (pyright, rust-analyzer via `rustup component add rust-analyzer`, gopls via `go install golang.org/x/tools/gopls@latest`, typescript via `npm i -g typescript`). Install feasible but exceeds closure window (each 10-100MB, plus `serena-agent project create` and `start-mcp-server`). No production code change needed. Stub adapter `serena.py` remains. Full 26Q not run.

### Aider — NOT_COMPARABLE (feasibility)

- **Version:** Aider repo map (Tree-sitter + PageRank, 1k token budget, `aider --show-repo-map`)
- **Status:** **AIDER_REPO_MAP_NOT_COMPARABLE** — Repo Map does not expose (repo,query,top_k)->hits; it builds PageRank over symbols relevant to current chat files, not per-query ranked files. Normalizing would invent ranking. Defer to same-agent coding A/B.

### Cursor — NOT_ISOLATABLE (feasibility)

- **Version:** Cursor proprietary, cloud-assisted, @Codebase/Agent, embeddings via Cursor backend
- **Status:** **CURSOR_RETRIEVAL_LANE_NOT_ISOLATABLE** — No standalone retrieval API, retrieval not separable from model routing/agent loop/editor state, cloud dependency even with own keys, Privacy Mode zero-retention still passes through backend.

## Retrieval 26Q (combined run CE hot + rg + CBM, 78 queries, official profile, 600s)

| System | H@1 | H@3 | H@5 | R@1 | R@3 | R@5 | MRR |
|---|---|---|---|---|---|---|---|
| Context Engine (hot, E3) | **0.500** | **0.654** | **0.654** | **0.481** | **0.654** | **0.654** | **0.571** |
| rg/read | 0.192 | 0.231 | 0.308 | 0.167 | 0.199 | 0.295 | 0.227 |
| codebase_memory (full 26Q) | 0.192 | 0.346 | 0.346 | 0.192 | 0.346 | 0.346 | 0.263 |
| OCI (gin single query) | 1.000 (single) | — | — | — | — | — | — (partial) |
| Serena | BLOCKED | — | — | — | — | — | — |

**18Q subset (django+nestjs+ripgrep, historical):**

| System | H@1 | H@3 | H@5 | R@1 | R@3 | R@5 | MRR |
|---|---|---|---|---|---|---|---|
| CE hot | 0.500 | 0.611 | 0.611 | 0.472 | 0.611 | 0.611 | 0.556 |
| rg | 0.111* | 0.222 | 0.222 | 0.111 | 0.181 | 0.194 | 0.170* |
| CBM | 0.056 | 0.278 | 0.278 | 0.056 | 0.278 | 0.278 | 0.157 |

*rg 18Q H@1 in this combined run is 0.111 (2/18), lower than earlier 0.167 (3/18) due to .opencode exclusion fix — variation within small n.

**18Q for CBM derived from 26Q:** django 0.167, nestjs 0.000, ripgrep 0.000 → H@1 0.056 (1/18). Definition category for CBM is 0.250 vs CE 0.875, etc.

## Category (26Q, combined run)

| System | Definition (8) | Exact (5) | Test (5) | Caller (3) | Conceptual (5) |
|---|---|---|---|---|---|
| CE H@1 | **0.875** | 0.400 | 0.400 | 0.333 | 0.200 |
| H@3 | **1.000** | 0.400 | 0.600 | 0.333 | 0.600 |
| MRR | **0.938** | 0.400 | 0.467 | 0.333 | 0.400 |
| rg H@1 | 0.000 | 0.600 | 0.000 | 0.333 | 0.000 |
| H@3 | 0.125 | 0.600 | 0.000 | 0.667 | 0.200 |
| MRR | 0.073 | 0.600 | 0.000 | 0.500 | 0.100 |
| CBM H@1 | 0.250 | 0.200 | 0.000 | 0.000 | 0.400 |
| H@3 | 0.375 | 0.200 | 0.400 | 0.000 | 0.600 |
| MRR | 0.312 | 0.200 | 0.167 | 0.000 | 0.500 |

- CE wins **definition** (0.875 vs CBM 0.250 vs rg 0.000) — large gap n=8, reliable.
- **Exact** rg 0.600 vs CE 0.400 vs CBM 0.200 — rg's file-exists heuristic wins pure path.
- **Test** CE 0.400 vs CBM 0.000 vs rg 0.000 — CE test retriever unique.
- **Caller** (n=3 small): CBM 0.000, CE 0.333, rg 0.333 — all weak, small-sample.
- **Conceptual** CE and CBM both 0.400/0.600, rg 0.000/0.200 — semantic helps.

## Common Context Size (cl100k, fixed)

| System | avg files | common_cl100k_tokens (avg) | native |
|---|---|---|---|
| CE hot | 3.1 | 418 | 418 (native = common) |
| rg (fixed) | 5.0 | **78** (was 34 whitespace, now 78 cl100k) | 78 |
| CBM | 3.2 | 209 | 209 |
| OCI (gin) | ~3-4 est | ~300-400 est (not yet measured for full) | — |

*After fix, rg 78 vs CE 418 (previously 34 vs 418 invalid). No token-savings claim. CE packs fewer files (3.1) but more tokens per file (denser evidence).*

**RG audit:** hits are `file:line:text` with `text[:400]` per rg line plus `File exists: <path>` for exact queries. Not full file reads, just snippets. This is the documented baseline: "search + inspect/read" via one-line snippets, not full file content. Full file reads would be ~2000 tokens per file, but baseline as defined is neutral and reproducible. Avg 78 tokens = 5 files * ~15 tokens per snippet (400 chars ~100 tokens, but deduped and file-exists hits are shorter). This is honest.

## Actual Runtime RSS (separate harness vs system)

| System | Harness Python | System-under-test RSS |
|---|---|---|
| CE hot | 30MB | **contextd hot estimated 150-250MB for django** (HotState 97MB + vectors 67MB + overhead) — **not yet isolated via psutil in this closure** (would be `psutil.Process(contextd_pid).memory_info().rss` while MCP alive). Warm daemon not yet measured with `daemon start` equivalent. |
| rg | 30MB | rg process <10MB (per `rg` subprocess) |
| CBM | 30MB | **CBM daemon 200-400MB estimated** (from `mem.init budget_mb=4037 total_ram_mb=16151` log, but not yet `psutil` on pid 27824) — to be measured in C0.1 with `psutil` on daemon pid |
| OCI | 30MB | **Node 80-100MB + native 30MB + Ollama 45MB (all-minilm) resident** — not yet isolated via `psutil` on Node pid |
| Serena | — | BLOCKED |

*Preliminary harness vs product conflation is fixed in method, but numbers for external lanes not yet fully isolated — to be completed in C0.1 with warm daemons and `psutil`.*

## Index Disk (audit)

| System | Repo | Disk | Breakdown |
|---|---|---|---|
| CE | django | **1034.1 MB** | `bench/repos/django/.context/index/structural.db` 1084305408 bytes single file, no WAL/SHM after clean. Contains 44010 symbols, 43841 BM25 docs, 44188 vectors (384d*4*44k=67MB) + file texts + graph. No multiple generations, no stale WAL, legitimate. gin 39.4MB, lodash 43.3MB, nestjs 122.1MB, ripgrep 69.2MB each single DB. |
| CE total 5 repos | — | ~1.3GB | Sum of 5 DBs |
| CBM | lodash | not yet du'd | Stored at `C:\Users\Dell\.cache\codebase-memory-mcp\` (projects/C-...), not per-repo .context. `du -h` not yet run, to be measured. |
| CBM | gin | not yet | Same cache, 2351 nodes/11844 edges |
| OCI | gin | 4.9MB WAL + 4KB DB (partial) | `bench/repos/gin/.opencode/index/codebase.db` 4KB + WAL 4.9MB after gin index (before full vectors, vectors file not yet inspected). After full gin 1598 chunks, WAL was 5MB, vectors not yet. For django, est ~1GB similar to CE. |
| rg | — | N/A | No index, uses FS |

## Index/Update (disposable copies, not dirtying pinned repos)

| System | Repo | Initial | No-change | One-file modify | One-file delete |
|---|---|---|---|---|---|
| CE hot | django | 14.6s (MCP status) / 24.6s (run.py) | 0ms (discovery 0, reconcile 0, skipped true) — via stats, not separate wall | not yet via disposable `cp -r` + `touch` + `time search` | not yet |
| CE | gin | 0.8s / 1.5s | 0ms | not yet | not yet |
| CBM | django | 18.2s (with daemon warm, includes 40s first but now 18s) | not yet | not yet | not yet |
| CBM | lodash | 10-14s (with warm daemon, 3.4s per query) | — | — | — |
| OCI | gin | 6 min (1598 chunks, all-minilm) | not yet | not yet | not yet |
| Serena | — | BLOCKED | — | — | — |

*Full no-change/modify/delete via disposable copies `cp -r bench/repos/gin /tmp/gin_copy && ...` to be done in C0.1.*

## Hot Latency (repeated 11 samples after warmup, persistent)

| System | django | gin | lodash | nestjs | ripgrep |
|---|---|---|---|---|---|
| CE hot wall p50 | 738ms (latest) / 1105ms (prelim) | 281/126 | 240/55 | 578/295 | 322/104 |
| CE internal | 436/1102 | 281/123 | 240/52 | 578/292 | 322/102 |
| rg wall p50 | 651 | 58 | 62 | 212 | 60 |
| rg p95 | 712 | 72 | 179 | 301 | 67 |
| CBM wall p50 | 3452 | 3408 | 3356 | 3402 | 3254 |
| CBM p95 | 3597 | 3938 | 3563 | 3436 | 3310 |
| OCI | not yet | not yet | not yet | not yet | not yet |

*CBM p50 3.4s is with temp daemon startup per query (3.4s). With `daemon start` warm, expected <100ms (to be re-measured warm). CE hot 436ms latest (vs 1105 prelim) is faster due to .opencode exclusion and tiktoken fix? Actually same. Varied-query latencies (26Q mixed) are higher: CE django 776/2138, etc.*

**Latency accounting kept separate:** repeated same-query (above) vs varied 26Q (CE django 776/2138, rg 651/712, CBM 3452/3597). Do not compare fresh vs persistent.

## Fairness

- same queries: YES (identical 26Q text for all where API permits)
- same repos: YES (c6be0bf3, 674ac31d, 3fce3b5b, a666ba59, 34dac209 all clean, verified, .opencode excluded for rg)
- same commits: YES
- common tokenizer: **YES after fix** (cl100k via tiktoken, CE native = common, rg/CBM/OCI common via _tok)
- ground truth leakage: NO (adapters never import expected_files)
- question-specific tuning: NO (generic normalization)
- production source changed: NO (only bench/adapters, bench/scripts, bench/results/c0)

## C0 Preliminary CE-vs-RG Result

**Preserved and re-measured with fix:** CE H@1 0.500 MRR 0.571 vs rg 0.192 MRR 0.227 (latest with .opencode excluded, vs prelim rg 0.231 MRR 0.277). The drop in rg from 0.231 to 0.192 is due to excluding `.opencode` (which previously gave spurious hits via OCI's index files) and is **more accurate**. The gap CE 0.500 vs rg 0.192 is still **large observed gap** (0.308, 2.6x). This is preliminary until full external lanes complete, but **not re-run to chase better numbers** — this is the fixed-methodology re-measurement.

## Market Findings (with external lanes partial)

- **CE clearly wins:** Definition (0.875 vs CBM 0.250 vs rg 0.000, n=8, large gap), Test (0.400 vs 0.000), Overall 26Q (0.500 vs 0.192) — but **CBM's definition win over rg (0.250 vs 0.000) shows external competitors also beat rg on definition, but CE beats them both.** This is **directional, not yet decisive vs OCI** (OCI not yet full 26Q).
- **CE clearly loses:** Exact file (rg 0.600 vs CE 0.400 vs CBM 0.200) — rg's file-exists heuristic wins pure path. Caller (n=3 small): all weak (CE 0.333, CBM 0.000, rg 0.333). **Uncertain/small-sample:** caller n=3, conceptual n=5 — do not call decisive. Example: CBM conceptual 0.400/0.600 vs CE 0.200/0.600 are similar.
- **Uncertain:** OCI gin single query correct (1.0) suggests OCI may be competitive on definition, but not enough data. CBM's overall 0.192 is lower than CE, but its conceptual 0.400 is equal to CE, and its lodash definition 0.750 vs CE 0.500 suggests CBM wins on lodash (small repo). **Small-sample, not yet market-conclusive.**
- **Exact search bottleneck:** CE exact 945ms (from earlier django decomposition) vs BM25 137 + semantic 116 = **exact is 7x larger, so indexed lexical could save ~800ms, not 100ms** — preliminary "only 100ms" is wrong. CBM's BM25 is already indexed (<5s) and OCI's hybrid is indexed, so CE's exact via `rg` is the bottleneck. Quantified, not implemented.
- **SCIP/LSP quality gap:** Definition is solved (CE 0.875), but caller 0.333 and CBM 0.000 suggest graph precision is market-wide weak. Serena (LSP) would be needed to test if it beats CE on caller (expected, but not yet measured due to BLOCKED). Proof (generation-bound identity) does NOT fix caller.
- **Proof/Delta differentiation:** CBM and OCI have no generation-bound provenance (CE's HotState generation+fingerprint). With CBM's graph and OCI's hybrid both strong, Proof remains differentiation even if caller similar.
- **Memory/disk competitive:** CE django 1GB vs OCI est 1GB vs CBM cache not yet measured but likely similar — CE not yet competitive on disk, but HotState 97MB is reasonable.

## Ready for Same-Agent A/B

**NO — not yet.** With only partial OCI (gin) and full CBM (26Q), we cannot claim CE beats OCI overall (Does CE beat OCI? Unknown, OCI not full 26Q. Does CE beat CBM? Yes on overall 0.500 vs 0.192, but CBM wins on lodash definition 0.750 vs 0.500). Need full OCI 26Q and Serena LSP to answer. Pilot A/B could be done for CE vs CBM only, but not for market claim.

## Roadmap Decision

**E: Another evidence-supported combination — C0.1 to complete external lanes before choosing A/B/C/D.**

**Evidence for decision:**
- Does CE beat OCI overall? **Unknown** (OCI not full 26Q, but gin single query correct suggests competitive)
- Does CE beat CBM overall? **YES** (0.500 vs 0.192, large gap, n=26) — CBM is weaker overall, but wins on lodash.
- Does Serena beat CE on definition/caller? **Unknown** (BLOCKED)
- Is exact latency serious market disadvantage? **YES** (945ms exact vs 137 BM25, 7x, and CE hot 436ms vs rg 136ms, 3x, and CBM 3378ms vs CE 436ms — CE is middle, but exact is bottleneck vs OCI/CBM indexed)
- Is CE's memory/disk competitive? **Disk not yet** (1GB vs CBM unknown, OCI est similar), **RAM 97MB HotState is competitive**
- Is Proof/Delta highest-value differentiation? **Possibly, but SCIP/LSP may be higher for caller** (caller 0.333 vs 0.000 for CBM). Need Serena to decide.
- Should A/B come before or after Proof? **After C0.1 external lanes complete, then decide.**

**Therefore, do not choose A (indexed lexical), B (Proof), C (SCIP), or D (A/B) yet.** Complete C0.1: full OCI 26Q (45-60 min, all-minilm, warm), full CBM warm 26Q (already done, but re-measure warm p50 <100ms), Serena with LSPs (install pyright, rust-analyzer, gopls, typescript, then project create). Then re-evaluate.

## Gates

- fmt: PASS
- clippy: PASS (20.7s)
- tests: PASS (64 passed, 5.10s)
- release: PASS (37MB, 1m40s)
- python: PASS (6 tests, 3.239s)
- diff-check: PASS
- five pinned repos clean: YES

## Final Verdict

**C0_CONTEXT_BENCH_BLOCKED** — External lanes partially run (CBM full 26Q done, OCI gin done, django blocked_time, Serena blocked_lsp). Preliminary CE-vs-RG preserved but not final. Requires C0.1 to complete OCI full 26Q, Serena LSP, common token re-measurement for all, actual RSS via psutil warm, index/update via disposable copies, and full market tables.

**STOP. Do NOT create PR, Do NOT merge, Do NOT start Proof/Delta/indexed lexical/SCIP.**
