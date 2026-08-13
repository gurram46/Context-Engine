# Context Bench v1 — Summary

Generated from `C:\Users\Dell\context\Context-Engine\bench\results\results.jsonl` — 36 queries

Note: FILE-LEVEL evaluation uses ranked UNIQUE files (deduplicated preserving first occurrence). Hit@K is binary, Recall@K is fractional (relevant retrieved / total relevant).

## SYSTEM

| SYSTEM | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR | P50 | P95 | AVG PACKED | AVG FILES |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| context_engine | 18 | 0.167 | 0.222 | 0.222 | 0.167 | 0.222 | 0.222 | 0.194 | 2816 | 15016 | 381 | 3.1 |
| rg_baseline | 18 | 0.111 | 0.167 | 0.222 | 0.111 | 0.167 | 0.222 | 0.153 | 144 | 713 | 31 | 4.8 |

## REPO

| SYSTEM | REPO | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR | P50 | P95 | PACKED |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| context_engine | django | 6 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 10714 | 26688 | 433 |
| context_engine | nestjs | 6 | 0.167 | 0.333 | 0.333 | 0.167 | 0.333 | 0.333 | 0.250 | 2468 | 2858 | 285 |
| context_engine | ripgrep | 6 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 4144 | 7612 | 426 |
| rg_baseline | django | 6 | 0.167 | 0.333 | 0.333 | 0.167 | 0.333 | 0.333 | 0.250 | 660 | 781 | 23 |
| rg_baseline | nestjs | 6 | 0.000 | 0.000 | 0.167 | 0.000 | 0.000 | 0.167 | 0.042 | 144 | 481 | 28 |
| rg_baseline | ripgrep | 6 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 82 | 85 | 42 |

## CATEGORY

| SYSTEM | CATEGORY | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR |
|---|---|---|---|---|---|---|---|---|---|
| context_engine | caller | 3 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 |
| context_engine | conceptual | 3 | 0.000 | 0.333 | 0.333 | 0.000 | 0.333 | 0.333 | 0.167 |
| context_engine | definition | 6 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 |
| context_engine | exact | 3 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| context_engine | test | 3 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| rg_baseline | caller | 3 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| rg_baseline | conceptual | 3 | 0.000 | 0.333 | 0.333 | 0.000 | 0.333 | 0.333 | 0.167 |
| rg_baseline | definition | 6 | 0.000 | 0.000 | 0.167 | 0.000 | 0.000 | 0.167 | 0.042 |
| rg_baseline | exact | 3 | 0.667 | 0.667 | 0.667 | 0.667 | 0.667 | 0.667 | 0.667 |
| rg_baseline | test | 3 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |

## MACRO AVERAGES

- macro Hit@1: 0.139
- macro Hit@3: 0.194
- macro Hit@5: 0.222
- macro Recall@1: 0.139
- macro Recall@3: 0.194
- macro Recall@5: 0.222
- macro MRR: 0.174

## INDEXING

| ADAPTER | REPO | WALL MS | FILES | SYMBOLS | BM25 | VECTORS | DISK | UNAVAILABLE |
|---|---|---|---|---|---|---|---|---|
| context_engine | django | 8586 | 5887 | 42135 | 42250 | 0 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| context_engine | nestjs | 2684 | 945 | 3565 | 3646 | 0 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| context_engine | ripgrep | 1157 | 237 | 3602 | 3629 | 0 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | django | 6719 | 7085 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | nestjs | 1693 | 2132 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | ripgrep | 277 | 237 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |

## NOTES

- FILE-LEVEL: hits are deduplicated to unique files preserving first rank before Hit/Recall/MRR.
- Hit@K = binary (1 if any expected file in first K unique files). Recall@K = |relevant ∩ first K| / |expected|.
- packed_tokens is real tokenizer count from contextd; candidate_tokens unavailable in production (marked None).
- No benchmark-specific production logic. Failures are roadmap evidence.
- Previous Python-proxy metrics are INVALID / DISCARDED.
