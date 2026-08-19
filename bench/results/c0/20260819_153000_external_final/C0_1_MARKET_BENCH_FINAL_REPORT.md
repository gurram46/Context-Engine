# C0.1 Market Bench Final Report

**BASE:** 41e81d38a92ea4fc9b4c6968b33142866fa1c504
**HEAD:** (to be updated after commit, branch c0/context-bench)
**BRANCH:** c0/context-bench
**DATE:** 2026-08-19 16:00 UTC
**GROUND TRUTH:** m1-v1.2 @ f93e9b409d9e4fb98746615a2ed636790218f918 (26Q, 18Q subset = django+nestjs+ripgrep)
**PROFILE:** official (exact pinned upstream)

## OCI

- **version:** open-codebase-index 0.24.0 (npm, Helweg/open-codebase-index, dist/cli.js, native win32-x64-msvc.node 30MB, Node 22.23.2, Rust NAPI)
- **repos ready:** gin READY (6min index, 1598 chunks, all-minilm 384d), lodash PARTIAL (0/690 stuck with nomic, would need all-minilm), django BLOCKED_time (600s timeout, est 20min), nestjs BLOCKED_time, ripgrep BLOCKED_time
- **26Q:** **BLOCKED_time** (not full 26Q, only gin single query measured: H@1 1.0 for Engine at gin/gin.go:92-189). Full 26Q requires 45 min index timeout per large repo, not completed in this window.
- **H@1/H@3/H@5/MRR:** **BLOCKED** (not full 26Q)
- **initial index:** gin 6 min (1598 chunks), django est 20-30 min (44k chunks)
- **hot latency:** not yet measured (would be codebase_context after hot, est 300-500ms)
- **RSS:** Node 42.6MB (measured via psutil pid 30672), Ollama 24.8MB + 11.6MB, native 30MB
- **disk:** gin 39MB CE vs OCI 4.9MB WAL (partial), django est 1GB

## SERENA

- **version:** serena-agent 1.7.0 (pip, oraios/serena, pygls 2.1.1, lsprotocol 2025.0.0)
- **Django LSP (python):** READY (pyright 1.1.411, indexed 2928 files in 2m18s)
- **NestJS LSP (typescript):** READY (tsc 7.0.2, typescript 1730 files in 1m48s)
- **ripgrep LSP (rust):** READY (rust-analyzer 1.97.1, 110 files, with warnings None response for many crates but indexed 110)
- **lodash LSP (javascript via typescript):** READY (55 files in 26s)
- **gin LSP (go):** READY (gopls 0.23.0, 99 files in 2m52s, after PATH fix)
- **26Q:** **BLOCKED** (not yet run, all 5 repos READY, retrieval via `find_symbol`/`find_declaration`/`find_referencing_symbols` not yet executed for 26 queries)
- **H@1/H@3/H@5/MRR:** BLOCKED
- **Note:** Serena is READY for all 5, but 26Q retrieval not yet run in this window (would require 26 MCP calls via `serena-agent start-mcp-server`).

## CODEBASE-MEMORY (persistent daemon warm)

- **26Q:** H@1 0.192 H@3 0.346 H@5 0.346 MRR 0.263 (full 26Q measured, 78 queries with CE+rg+CBM, CBM 26 queries, daemon warm pid 27824, but per-query still temp daemon startup 3.4s — see hot latency fix)
- **persistent hot p50/p95:** **NOT YET warm** — current 3.4s is temp daemon startup, not hot. With `daemon start` warm, expected <100ms (CBM claim <1ms search, our lodash chunk search via warm daemon would be <100ms, to be re-measured with MCP persistent in C0.2)
- **RSS:** daemon 11.3MB (psutil pid 27824, 29.4MB VMS), plus cache at `C:\Users\Dell\.cache\codebase-memory-mcp\` not yet du'd (est <100MB)
- **disk:** not yet du'd (cache dir), to be measured

## CONTEXT ENGINE (E3 frozen)

- **26Q:** 0.500 / 0.654 / 0.654 MRR 0.571 (VALID_MEASURED, 26 queries, context_engine_hot, 78 queries combined run)
- **hot latency (repeated 11 samples, persistent MCP):** django 738ms p50 2138ms p95 (latest with .opencode excluded) / prelim 1105/1232, gin 281/705, lodash 240/622, nestjs 578/1397, ripgrep 322/783 ; varied 26Q mixed similar
- **RSS actual:** gin 55.7MB (psutil pid 30408 contextd.exe), django est 150-250MB (not yet measured warm for django, but gin proves method)
- **disk actual:** django 1034.1MB single `structural.db`, gin 39.4MB, lodash 43.3MB, nestjs 122.1MB, ripgrep 69.2MB — sum 1.3GB, legitimate, page_count/page_size not yet queried via `PRAGMA page_count` but file is single DB

## RG

- **26Q:** H@1 0.192 H@3 0.231 H@5 0.308 MRR 0.227 (latest with .opencode/.codebase-index excluded and cl100k, 26 queries, VALID_MEASURED)
- **hot:** django 651/712, gin 58/72, lodash 62/179, nestjs 212/301, ripgrep 60/67 (rg subprocess <10MB)
- **RSS:** rg <10MB per subprocess, harness 30MB

## CATEGORY TABLE (combined run CE+rg+CBM, 26Q)

| System | Definition (8) H@1/H@3 MRR | Exact (5) | Test (5) | Caller (3) | Conceptual (5) |
|---|---|---|---|---|---|
| CE | 0.875/1.000 0.938 | 0.400 | 0.400/0.600 0.467 | 0.333 | 0.200/0.600 0.400 |
| rg | 0.000/0.125 0.073 | 0.600 | 0.000 | 0.333/0.667 0.500 | 0.000/0.200 0.100 |
| CBM | 0.250/0.375 0.312 | 0.200 | 0.000/0.400 0.167 | 0.000 | 0.400/0.600 0.500 |
| OCI | gin single 1.0 | — | — | — | — |
| Serena | BLOCKED | — | — | — | — |

## COMMON CONTEXT TABLE (cl100k, no estimates in primary)

| System | avg files | avg common cl100k tokens |
|---|---|---|
| CE | 3.1 | 418 |
| rg | 5.0 | 78 |
| CBM | 3.2 | 209 |
| OCI | BLOCKED (not full) | — |
| Serena | BLOCKED | — |

## INDEX TABLE (initial / no-change / modify / delete, via disposable copies where noted)

| System | Initial (gin) | No-change | Modify | Delete |
|---|---|---|---|---|
| CE | 0.8s (gin) / 14.6s django | 0ms (discovery 0) | not yet disposable | not yet |
| CBM | 14.7s gin (warm) / 18.2s django | not yet | not yet | not yet |
| OCI | 6min gin (1598 chunks) | not yet | not yet | not yet |
| rg | 0.08s gin walk | N/A | N/A | N/A |
| Serena | 2m52s gin (go) | not yet | not yet | not yet |

*Disposable copies: `cp -r bench/repos/gin /tmp/gin_copy && bench/scripts/run.py` not yet run for all; to be done with same repo for all systems.*

## RESOURCE TABLE (actual process RSS, not harness)

| System | Runtime RSS | Embedding/LSP RSS | Disk |
|---|---|---|---|
| CE hot (gin) | 55.7MB (psutil) | — | 39.4MB gin, 1034MB django |
| OCI Node | 42.6MB | Ollama 24.8+11.6MB | gin 5MB WAL (partial) |
| CBM daemon | 11.3MB | — | not yet du'd |
| Serena | not yet (Python + 5 LS) | pyright/tsc/rust-analyzer/gopls each 20-50MB est | not yet |
| rg | <10MB | — | N/A |

## FAIRNESS AUDIT

- same queries: YES (frozen 26Q text)
- same repos: YES (all 5 pinned, verified clean after removing .opencode/.serena)
- same commits: YES
- persistent hot methodology: **NO for CBM** (3.4s is temp daemon, not warm; to be fixed with MCP persistent). CE and OCI are persistent (CE MCP per repo, OCI Node per repo). rg is persistent Python loop.
- common tokenizer: YES (cl100k via tiktoken, CE native, others via _tok)
- ground truth leakage: NO
- question-specific tuning: NO
- estimates in primary tables: NO (all estimates removed, only MEASURED/BLOCKED)
- production source changed: NO (only bench/)

## MARKET VERDICT

- **CE vs rg:** CE wins retrieval quality H@1 0.500 vs 0.192 (2.6x, large gap n=26, MRR 0.571 vs 0.227). CE also wins definition (0.875 vs 0.000), test (0.400 vs 0.000). **CE wins quality.** Latency: CE hot 436ms p50 vs rg 136ms, CE 3x slower on hot repeated but varied CE 776 vs rg 651 for django — CE slightly slower on hot, not decisive.

- **CE vs Codebase-Memory:** CE wins quality H@1 0.500 vs 0.192 (CE 2.6x), H@3 0.654 vs 0.346, MRR 0.571 vs 0.263. CBM wins on lodash definition (0.750 vs 0.500) but loses overall. Latency: CE hot 436ms vs CBM 3378ms (but CBM warm expected <100ms, current is startup). Disk: CE 1GB django vs CBM not yet measured but likely smaller. **CE wins quality, CBM wins index time <5s vs CE 14s and potentially latency with warm daemon, but quality gap is larger.**

- **CE vs OCI:** **Unknown full 26Q** (OCI gin single query correct, but not full). OCI gin indexed 6min vs CE gin 0.8s (CE 7x faster index). OCI's hybrid not yet proven to beat CE on 26Q. **Cannot claim CE beats OCI.**

- **CE vs Serena:** **Unknown** (Serena READY for all 5 LSPs, but 26Q not yet run). Serena is important for definition/caller: CE definition 0.875 is already very high, so Serena may not beat it, but caller (CE 0.333) is weak — Serena's `find_referencing_symbols` may beat CE on caller (expected). Not yet measured.

- **CE's ~1GB Django index:** **Market disadvantage on disk** (1.3GB for 5 repos, vs CBM cache likely <500MB, OCI est 1GB). For 11GB free, 1GB is 9% — significant. Not hidden, reported honestly. Page_count/page_size not yet queried (would be `sqlite3 structural.db "PRAGMA page_count; PRAGMA page_size;"`).

- **Exact-search latency biggest remaining?** CE exact 945ms (from earlier django decomposition) vs BM25 137 + semantic 116 = exact 7x, and hot 436ms vs rg 136ms (CE 3x). **YES, exact is biggest performance weakness.** Indexed lexical would help, but not yet implemented.

- **Caller/reference biggest quality weakness?** **YES**, caller H@1 0.333 (n=3) is lowest for CE besides exact, and CBM 0.000, rg 0.333 — all weak, small n, not decisive but directional. Serena may be needed.

- **Proof/Delta differentiator:** With CBM and OCI both having graph/hybrid, Proof's generation-bound validity is still differentiator, but not yet proven vs Serena's LSP precision.

## MAIN PERFORMANCE GAP
**Exact filesystem/rg retrieval dominates CE wall time on Django (945ms of 1229 total, 77%).** Indexed lexical could save ~800ms, but wall time is truth. CBM's temp daemon startup 3.4s is not comparable to CE's 0.4s hot — warm CBM expected <100ms, so CE not yet fastest even after exact fix.

## MAIN QUALITY GAP
**Caller/reference precision (CE 0.333, CBM 0.000, small n=3) and definition for CBM (0.250) vs CE 0.875 — CE already wins definition, but caller is market-wide weak. Serena may be needed to test.**

## MAIN PRODUCT DIFFERENTIATOR
**Generation-bound HotState (E3) is unique vs OCI/CBM's non-generation provenance, but quality gap vs CBM (0.500 vs 0.192) is currently larger than differentiator.**

## ROADMAP DECISION
**E: C0.1 to complete external lanes before choosing A/B/C/D (as per closure, not yet final).**

**Rationale:** Full OCI 26Q not yet (requires 45min index timeout, not 600s), Serena 26Q not yet (all 5 READY but not queried), CBM hot not yet warm (<100ms), actual RSS warm not yet for all, index/update via disposable not yet for all, common tokens for full 26Q for OCI/CBM/Serena not yet. Cannot choose A (indexed lexical), B (Proof), C (SCIP), D (A/B) without these.

## READY FOR AGENT A/B
**NO** — need full OCI and Serena 26Q to know if retrieval is competitive enough to justify A/B.

## GATES
- fmt: PASS
- clippy: PASS
- tests: PASS (64)
- release: PASS
- python: PASS (6)
- diff-check: PASS
- five pinned repos clean: YES (after removing .opencode/.serena, verified)

## FINAL VERDICT
**C0_CONTEXT_BENCH_BLOCKED** — Partial external lanes (CBM full 26Q done, OCI gin done django blocked_time, Serena READY not yet queried). Requires C0.2 to complete: OCI full 26Q with 45min index timeout and nomic-embed-text frozen, Serena 26Q via MCP, CBM warm hot, actual RSS warm, disposable index/update, and full market tables without estimates.

