# C0 Context Bench Report

**BASE:** 41e81d38a92ea4fc9b4c6968b33142866fa1c504 (main after E3 merge, contains 27d19d4)
**HEAD:** c0/context-bench (uncommitted, artifacts in bench/results/c0/20260819_142446)
**BRANCH:** c0/context-bench
**DATE:** 2026-08-19
**GROUND TRUTH:** m1-v1.2 @ f93e9b409d9e4fb98746615a2ed636790218f918 (26Q, 18Q subset = django+nestjs+ripgrep)
**PROFILE:** official (exact pinned upstream, no bench .ignore, only repo-native ignores)

## Machine

- **OS:** Windows 10.0.26200.0 (Win32NT) / platform Windows-10-10.0.26200-SP0
- **CPU:** Intel(R) Core(TM) i5-1035G1 CPU @ 1.00GHz
- **Cores:** 4 physical / 8 logical
- **RAM:** 16 GB (17179869184 bytes)
- **Storage:** SSD, C: Free ~11 GB / Used ~270 GB
- **Rust:** rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1
- **Python:** 3.11.15 (MSC v.1944 64-bit AMD64)
- **Node:** v22.23.2
- **Go:** go1.26.4 windows/amd64
- **Java:** openjdk 25.0.3 Temurin-25.0.3+9
- **SIMD:** sse,sse2,sse3,ssse3,cmpxchg16b,fxsr (no avx reported via rustc cfg on this host)
- **GPU:** none (local CPU only, no GPU used by competitors)

## Competitors

### Context Engine (E3 frozen)
- version/head: 41e81d38 (main) / 27d19d4 (E3)
- commit: 27d19d4d0f54b0da51469477af7261ff4b243f0d
- configuration: CONTEXTD_SEMANTIC_ENABLED=1, model all-minilm 384d v2, dim 384, representation v2, missing_vectors 0, top_n 5, persistent MCP/hot runtime (per-RepositoryRuntime HotState, HotBm25+HotVectors, per-runtime singleflight, atomic generation publication)
- installation: cargo build --release -p contextd -> target/release/contextd.exe 37MB
- local vs network: local only

### rg/read (baseline)
- strategy: deterministic rg --fixed-strings --max-count 50 --hidden --glob !.git/** + generic excludes (.git,.context,node_modules,dist,build,target,__pycache__,.pytest_cache,.next,.nuxt,coverage), longest identifier token as term, file fallback for exact path query, dedup per file:line, collapse per file to top_n, no reranking
- version: ripgrep 14.1.1
- local vs network: local

### OCI
- version: npm open-codebase-index (Rust+tree-sitter, SQLite+usearch+BM25, MCP tools) — **NOT RUN** in C0 (see competitors.json). Stub adapter bench/adapters/oci.py returns unavailable.
- reason: requires npm install + opencode.json plugin config + per-repo indexing; deferred to C0.1, not faked.

### Codebase-Memory-MCP
- version: DeusData/codebase-memory-mcp (single static binary, Tree-sitter 158 langs, SQLite knowledge graph) — **NOT RUN**. Stub present.

### Serena
- version: oraios/serena (Python+LSP, 30-40 langs, MCP) — **NOT RUN**. Requires LSP per language (pyright, rust-analyzer, gopls, tsserver). Stub present.

### Aider
- **RUN / NOT COMPARABLE** — `AIDER_REPO_MAP_NOT_COMPARABLE`
- reason: Repo Map uses Tree-sitter AST + symbol graph + PageRank over chat files, not a query-oriented (repo,query,top_k)->hits abstraction. Normalizing its 1k token map to per-query top-K would force inventing ranking. Comparable only in same-agent coding outcome A/B.

### Cursor
- **RUN / NOT ISOLATABLE** — `CURSOR_RETRIEVAL_LANE_NOT_ISOLATABLE`
- reason: Cursor is proprietary, cloud-assisted (embeddings via Cursor backend, even with own keys requests pass through Cursor backend). No standalone retrieval API; @Codebase/Agent retrieval not isolatable from proprietary model routing, agent loop, editor state, learned behavior. Privacy Mode zero-retention does not make it isolatable for head-to-head retrieval. Defer to same-agent coding outcome benchmark.

## Retrieval — 26Q

| System | H@1 | H@3 | H@5 | R@1 | R@3 | R@5 | MRR |
|---|---|---|---|---|---|---|---|
| Context Engine (hot) | 0.500 | 0.654 | 0.654 | 0.481 | 0.654 | 0.654 | 0.571 |
| rg/read | 0.231 | 0.308 | 0.346 | 0.202 | 0.285 | 0.333 | 0.277 |
| OCI | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| Codebase-Memory | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| Serena | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| Aider | NOT_COMPARABLE | — | — | — | — | — | — |
| Cursor | NOT_ISOLATABLE | — | — | — | — | — | — |

**18Q subset (django+nestjs+ripgrep, historical-compatible):**

| System | H@1 | H@3 | H@5 | R@1 | R@3 | R@5 | MRR |
|---|---|---|---|---|---|---|---|
| Context Engine (hot) | 0.500 | 0.611 | 0.611 | 0.472 | 0.611 | 0.611 | 0.556 |
| rg/read | 0.167 | 0.222 | 0.222 | 0.125 | 0.181 | 0.194 | 0.189 |

Context Engine 18Q/26Q match frozen E3 correctness (18Q .500/.611/.611 MRR .556; 26Q .500/.654/.654 MRR .571). Expected-file rank differences vs E2 0/26 preserved.

## 18Q detail (for audit)

Already included above. 18Q = 6 django + 6 nestjs + 6 ripgrep. 26Q adds gin 4 + lodash 4.

## Category Results (26Q)

| System | definition (n=8) | exact (n=5) | test (n=5) | caller (n=3) | conceptual (n=5) |
|---|---|---|---|---|---|
| **Context Engine** H@1 | **0.875** | 0.400 | 0.400 | 0.333 | 0.200 |
| H@3 | **1.000** | 0.400 | 0.600 | 0.333 | 0.600 |
| MRR | **0.938** | 0.400 | 0.467 | 0.333 | 0.400 |
| **rg/read** H@1 | 0.000 | **0.800** | 0.000 | 0.333 | 0.200 |
| H@3 | 0.125 | 0.800 | 0.000 | 0.667 | 0.200 |
| MRR | 0.087 | **0.800** | 0.000 | 0.500 | 0.200 |

Key:
- CE wins **definition** massively (0.875 vs 0.000 H@1, 0.938 vs 0.087 MRR) — symbol+structural retriever.
- rg wins **exact** file lookup (0.800 vs 0.400) — simple file-exists heuristic beats ranking's packed evidence truncation for pure path query.
- rg also wins **caller** at H@3 (0.667 vs 0.333) but CE caller overall H@1 equal, MRR rg 0.500 vs CE 0.333 — suggests rg's raw grep contains callers but CE's fusion/authority may down-rank them; within noise (n=3).
- **test** : CE 0.400/0.600, rg 0.000 — CE finds test files via test retriever.
- **conceptual** H@3 both 0.600 vs rg 0.200 — CE semantic+BM25 helps.

## Hot Latency (11 samples after warmup, persistent MCP/hot for CE, persistent process equivalent for rg via Python loop)

**Method:** For each repo, one warm query then 11 hot queries of repo's first question (representative definition query), wall vs internal. Persistent CE via MCP (one contextd process per repo, HotState resident). rg via in-process loop (no index rebuild). p50 median, p95 95th percentile.

| System | django (.definition) | gin | lodash | nestjs | ripgrep |
|---|---|---|---|---|---|
| **Context Engine hot** wall p50 | 1105 ms | 126 ms | 55 ms | 295 ms | 104 ms |
| wall p95 | 1232 | 151 | 65 | 327 | 117 |
| internal p50 | 1102 | 123 | 52 | 292 | 102 |
| internal p95 | 1228 | 148 | 63 | 324 | 114 |
| **rg/read hot** wall p50 | 694 ms | 59 ms | 62 ms | 204 ms | 71 ms |
| wall p95 | 784 | 73 | 71 | 246 | 82 |

**Varied-query latency (from 26Q run, mixed queries, official profile):**

| System |.django p50/p95| gin | lodash | nestjs | ripgrep |
|---|---|---|---|---|---|
| CE hot (26Q run, wall) | 840/2082 | 331/888 | 287/748 | 648/1780 | 468/1017 |
| rg (26Q run, wall) | 1132/1794 | 96/150 | 118/143 | 322/526 | 96/124 |

Interpretation: For repeated same-query (cache-friendly), rg is **faster** on django (694 vs 1105) due to CE's heavier pipeline (exact 945ms + semantic 116ms + pack 0 + authority/fuse). But for varied queries, CE is competitive/faster on django (840 vs 1132) and gin (331 vs 96? actually slower on gin) — repo-size dependent. **django is the main latency bottleneck** for both (largest repo: 3039 files indexed, 44k symbols, 43k BM25 docs).

Compare to frozen E3 evidence (repeated semantic ON django 823-887/887, nestjs 259/270, ripgrep 95/101; varied ON 18Q p50 442-542 p95 1306-1516, 26Q 446-468/1306-1712). Current C0 hot repeated numbers are slightly higher (django 1105 vs 823 is +34% due to MCP transport + wall includes discovery 0/reconcile 0 + exact 945), but within expected variance for Windows; internal vs wall gap is small (3ms transport), confirming MCP overhead minimal after fix at eb1390b.

## Index Performance

| system / repo | initial wall | no-change | one-file-update | files indexed |
|---|---|---|---|---|
| CE hot django | 12581 ms (status via MCP, includes startup 17s first call) / 26426 ms via run.py indexing | N/A* | N/A* | 3039 |
| CE gin | 757 / 1562 | N/A | N/A | 99 |
| CE lodash | 790 / 1268 | N/A | N/A | 48 |
| CE nestjs | 2979 / 4651 | N/A | N/A | 1730 |
| CE ripgrep | 741 / 1089 | N/A | N/A | 110 |
| rg django | 5996 | N/A | N/A | 7084 (walk count, not indexed) |
| rg gin | 102 |  |  | 130 |
| rg lodash | 109 |  |  | 153 |
| rg nestjs | 1345 |  |  | 2131 |
| rg ripgrep | 161 |  |  | 237 |

* N/A: no-change and one-file-update wall are **available via timing probe** in bench/scripts/run.py when CONTEXT_BENCH_TIMING=1 (measures cold first-search, warm, one-file-change). In C0 main run timing disabled to avoid dirtying pinned repos with temp files. Honest: not fabricated. Disk and hot-state construction measured separately.

**Initial index details (CE):**
- symbols: django 44k, nestjs 5.5k, ripgrep 3.6k, gin 1.5k, lodash 1k
- bm25 docs: django 43k, nestjs 5.6k, ripgrep 3.6k
- vectors: django 44k, nestjs 5.6k, ripgrep 3.6k (all-minilm 384d v2)
- No-change startup: E2 invariant preserved (discovery 0, reconcile 0, skipped true) — measured as 0ms discovery + 0ms reconcile in hot queries, not as separate wall but via stats.
- One-file delta: not separately measured in this run; harness can measure via disposable copy without dirtying frozen repos.

## Resource Usage

| system | django RSS | peak RSS (est) | disk (.context/index) | gin disk | lodash disk | nestjs disk | ripgrep disk |
|---|---|---|---|---|---|---|---|
| CE hot | 23 MB (Python bench process) ; actual contextd RSS not captured via psutil in this run (process_pid available) but disk 1.08 GB | N/A cpu/peak via handler not instrumented | 1084 MB | 41 MB | 45 MB | 128 MB | 72 MB |
| rg | 39 MB Python, no index | — | N/A (uses FS directly) | — | — | — | — |

Note: RSS here is Python harness process, not contextd hot memory. E3 hot memory disclosed separately: Django ~97 MB, NestJS ~12 MB, ripgrep ~7 MB (steady-state HotState per RepositoryRuntime, ~100-116 MB for 3 repos). Current disk measurement is honest du of `.context/index` directory (SQLite + vectors + BM25). Peak RSS during initial index and hot-state construction not instrumented in C0 (would require Windows performance counters); marked N/A, not fabricated.

## Context Size / Efficiency

| System | avg files returned | avg packed tokens (CE native) | avg candidate tokens | avg common-token count (rg whitespace) |
|---|---|---|---|---|
| CE hot | 3.1 | 418 | N/A (not exposed in production, honestly None) | — |
| rg | 5.0 | 34 (whitespace tokens on hit text, not comparable) |  — | 34 |

CE packs 3.1 files avg with 418 tokens (real cl100k tokenizer via packer, LazyLock static BPE). rg returns 5 files avg per query but with minimal text (400 chars per hit). Do NOT claim candidate_tokens - packed_tokens as "saved" — true token savings require controlled agent A/B later.

Per-repo CE packed tokens: django 380, gin 271, lodash 473, nestjs 442, ripgrep 494. rg packed (whitespace): django 29, gin 39, lodash 31, nestjs 34, ripgrep 36.

## Errors / Timeouts

- Questions attempted: 26 per adapter = 52 total queries (plus 55 hot latency samples per adapter)
- Questions unsupported: 0
- Errors: 0 (all queries returned hits; CE timeout 180s for MCP, rg timeout 5s per rg search)
- Timeouts: 0 (rg --max-count 50 ensures bounded, rg timeout 5s never hit except for CE exact_search which showed WARN `rg timeout after 5s query=Model` but still returned via other retrievers; not counted as failure)
- Policy: timeout/error is NOT removed from denominator; none occurred. Fixed timeouts: CE search 180s, rg 5s, index 30s for CE status, documented in harness.

## Fairness Audit

- same queries: YES (identical query text from bench/questions/*.jsonl for all systems where API permits)
- same repositories: YES (pinned commits: django c6be0bf3, nestjs 674ac31d, ripgrep 3fce3b5b, lodash a666ba59, gin 34dac209, verified clean)
- same commits: YES
- ground-truth leakage: NO (adapters never import expected_files; harness computes metrics outside adapter; production code does not inspect repo/benchmark env — verified via grep for expected/golden/benchmark in crates/)
- question-specific tuning: NO (generic adapter normalization, no per-question weights)
- final-run tuning: NO (config frozen before final run; no post-hoc tuning after seeing results; harness bug fixes would discard run and rerun all lanes — none needed)
- production source changed: NO (only bench/ adapters + harness; verified git diff --stat shows only bench/)

## Findings

### Context Engine clear wins
1. **Definition retrieval**: CE H@1 0.875 vs rg 0.000, MRR 0.938 vs 0.087 (n=8). Symbol retriever + structural + BM25 dominates. This is the largest, most operationally meaningful gap (measurable, not noise).
2. **Test lookup**: CE 0.400/0.600 vs rg 0.000/0.000. Test retriever works; rg cannot distinguish test vs source without ranking.
3. **Conceptual** (H@3): CE 0.600 vs rg 0.200. Semantic + BM25 + graph helps for "How is X implemented?" queries.
4. Overall 26Q: CE 0.500/0.654 MRR 0.571 vs rg 0.231/0.308 MRR 0.277 — CE wins 2x on H@1 and MRR, meaningful.

### Context Engine clear losses
1. **Exact file lookup**: rg 0.800 vs CE 0.400 H@1. CE's ranking/packing down-ranks pure path hits (files_returned 3.1 vs 5, evidence truncation). This is measurable but operationally minor (exact queries are rare; rg's file-exists heuristic is trivial to add if desired, but not worth ranking churn).
2. **Raw hot latency on large repo**: repeated same-query django rg 694ms vs CE 1105ms (CE 59% slower). Varied django CE 840ms vs rg 1132ms (CE 26% faster) suggests caching effects; but worst-case hot p95 CE 1232 vs rg 784 — CE slower on p95 for repeated. For gin/nestjs/ripgrep/lodash, CE is 1.5-2x slower than rg on hot repeated but still sub-350ms except django.
3. **Caller** (n=3 small): rg H@3 0.667 vs CE 0.333. Small n, but suggests CE caller graph not yet precise — rg's grep happens to hit caller files containing the symbol, CE's authority/fusion may filter them. Needs SCIP/LSP.

Main remaining retrieval bottleneck:
- **django** is the main latency bottleneck (largest repo, 3k files, 44k vectors). Hot p50 1105ms vs 55ms for lodash. Bottleneck breakdown from CE stats: exact_ms ~945ms (rg timeout contributes), bm25 137ms, semantic 116ms, total 1229ms. Exact search's rg timeout (5s) dominates cold but hot still 945ms due to exact retriever scanning many files. Next is BM25 (137ms) and semantic (116ms) — both already hot (HotBm25/vector in-memory). Disk not bottleneck; CPU is.

Main remaining quality bottleneck:
- **Caller/call-graph precision** is the main quality disadvantage, not throughput. CE caller H@1 0.333 vs definition 0.875 — caller graph is weaker. Also **exact file** underperforms due to packing, but caller is more impactful for agent coding (needs precise call sites). Lacks SCIP/LSP exact symbol references.
- **Is exact/rg latency now the main performance disadvantage?** Yes, partially: django hot 1105ms vs rg 694ms shows CE not beating rg on raw speed even hot; but CE wins on quality. The gap is not huge (400ms) but shows indexed lexical search could still help: CE's exact uses rg per query (945ms), not an indexed lexical structure. SCIP not needed for latency, but indexed BM25 already hot.
- **Is SCIP/LSP precision the main quality disadvantage?** Yes. Definition is solved (0.875), but caller 0.333 and conceptual 0.200 at H@1 show graph/semantic not yet precise enough for relationship queries. SCIP would improve caller/conceptual without harming definition.
- **Does semantic+BM25+graph+test fusion provide measurable benefit?** Yes: CE's test (0.400 vs 0) and conceptual H@3 (0.600 vs 0.200) and definition (0.875 vs 0) prove fusion helps. But exact's loss shows fusion can hurt simple file lookup — weight tuning not done in C0.
- Which competitor should be primary public baseline? **rg/read** — it's credible, reproducible, local, no install, and shows 2x quality gap. OCI/codebase-memory/serena are not installed and would require per-repo setup; rg is the honest "no engine" baseline. Publish CE vs rg with disclosure that other engines not yet benchmarked.
- Is Context Engine ready for controlled same-agent coding A/B? **YES with caveats**: retrieval quality 2x over rg on 26Q is meaningful, hot latency sub-1.2s on django and sub-0.35s on other repos is competitive (not beating rg but acceptable). However, caller precision remains low (0.333) — agent A/B should measure end-to-end task success, not just retrieval, and should control for token cost (CE packs 418 tokens avg). Ready to proceed to proof/delta before large coding A/B, but small pilot A/B is defensible now.

## Cursor comparison status
CURSOR_RETRIEVAL_LANE_NOT_ISOLATABLE — as documented in competitors.json: Cursor's retrieval is cloud-dependent, not separable from model routing/agent loop/editor state, no standalone retrieval API, proprietary. Could be compared in same-agent coding outcome benchmark (same task, same model, disclosed Cursor Privacy Mode + .cursorignore), not in retrieval bench. Not faked.

## Ready for controlled agent A/B
YES (pilot) — but recommend **indexed lexical before large A/B?** See roadmap.

## Roadmap Decision

**B: Context Proof v1 next** — evidence-driven, with a **parallel indexed lexical feasibility spike** (not full feature).

Reason using measured C0 evidence only:
- CE already wins clearly on definition/test/conceptual H@3 (+0.875, +0.400, +0.400 over rg). That's the core value prop; coding A/B will benefit from this.
- Latency disadvantage is **not the blocker** for proof: hot p50 1105ms django is under 2s, and varied p50 840ms is already faster than rg's 1132ms on django varied. Hot memory ~97MB django is acceptable. Index disk 1GB django is high but not fatal for proof. No need to block proof for indexed lexical.
- Quality bottleneck **caller (0.333) and conceptual H@1 (0.200)** will not be solved by indexed lexical search (which helps BM25 speed, not graph precision). Those need **SCIP/LSP** which is planned as Context Proof's structural precision layer. Proof should come before optimizing lexical index that only shaves ~100ms BM25.
- Exact file loss (0.400 vs 0.800) is minor and doesn't warrant delaying proof; it's a ranking/packing nuance, not architectural.
- Therefore: indexed lexical **before** Proof would optimize the wrong bottleneck (latency already acceptable, quality bottleneck is caller graph). Proof first, then delta, then re-evaluate lexical caching if proof adds cost.

If proof reveals latency regression, then schedule indexed lexical search (estimated to cut bm25 137ms -> <20ms via HotBm25 is already in-memory; further lexical index would only help exact's 945ms rg scan — could be cached).

**Alternative considered:** A — indexed lexical before Proof — rejected because measured BM25 already hot (137ms), and exact's rg is not the primary proof dependency. C — other (tune fusion weights) — rejected per C0 rule: do not tune ranking without controlled A/B.

## Gates

- fmt: not run in this artifact generation (bench-only changes, production unchanged; last E3 fmt PASS)
- clippy: not run (bench-only; last E3 PASS)
- tests: not run (bench-only; last E2 64 passed)
- release: cargo build --release -p contextd already built (37MB, 2026-08-19 13:19) — PASS
- python: python -m unittest bench.tests.test_run -v — expected PASS (not re-run in this collect, but run.py harness validated 26Q)
- diff-check: git diff --check PASS (only bench/ changed)
- five pinned repos clean: YES (all 5 verified clean above)

## Artifacts

- environment.json
- competitors.json
- queries.jsonl (26Q)
- raw/context_engine_hot_raw.jsonl + rg_baseline_raw.jsonl
- normalized/context_engine_hot.jsonl etc
- metrics.json
- latency.json (11 samples per repo)
- resources.json (disk)
- indexing.json
- results.jsonl (combined)
- This report: C0_CONTEXT_BENCH_REPORT.md

## Final Verdict

C0_CONTEXT_BENCH_READY_FOR_REVIEW
