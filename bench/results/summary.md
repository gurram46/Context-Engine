# Context Bench v1 — Summary

Generated from `C:\Users\Dell\context\Context-Engine\bench\results\results.jsonl` — 104 queries

Profile: official — OFFICIAL = exact pinned upstream (only repo-native ignores); for public claims
Note: FILE-LEVEL evaluation uses ranked UNIQUE files (deduplicated preserving first occurrence). Hit@K is binary, Recall@K is fractional (relevant retrieved / total relevant).

## SYSTEM

| SYSTEM | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR | P50 | P95 | AVG PACKED | AVG FILES |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| codebase_memory | 26 | 0.192 | 0.346 | 0.346 | 0.192 | 0.346 | 0.346 | 0.263 | 3378 | 3612 | 209 | 3.2 |
| context_engine_hot | 26 | 0.500 | 0.654 | 0.654 | 0.481 | 0.654 | 0.654 | 0.571 | 436 | 1460 | 418 | 3.1 |
| rg_baseline | 26 | 0.231 | 0.269 | 0.346 | 0.205 | 0.237 | 0.333 | 0.265 | 136 | 667 | 78 | 4.7 |
| serena | 26 | 0.423 | 0.538 | 0.538 | 0.404 | 0.519 | 0.519 | 0.481 | 5600 | 103300 | 99 | 2.2 |

## REPO

| SYSTEM | REPO | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR | P50 | P95 | PACKED |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| codebase_memory | django | 6 | 0.167 | 0.333 | 0.333 | 0.167 | 0.333 | 0.333 | 0.250 | 3452 | 3597 | 226 |
| codebase_memory | gin | 4 | 0.250 | 0.250 | 0.250 | 0.250 | 0.250 | 0.250 | 0.250 | 3408 | 3938 | 162 |
| codebase_memory | lodash | 4 | 0.750 | 0.750 | 0.750 | 0.750 | 0.750 | 0.750 | 0.750 | 3356 | 3563 | 147 |
| codebase_memory | nestjs | 6 | 0.000 | 0.167 | 0.167 | 0.000 | 0.167 | 0.167 | 0.083 | 3402 | 3436 | 251 |
| codebase_memory | ripgrep | 6 | 0.000 | 0.333 | 0.333 | 0.000 | 0.333 | 0.333 | 0.139 | 3254 | 3310 | 225 |
| context_engine_hot | django | 6 | 0.667 | 0.833 | 0.833 | 0.667 | 0.833 | 0.833 | 0.750 | 738 | 2138 | 380 |
| context_engine_hot | gin | 4 | 0.500 | 1.000 | 1.000 | 0.500 | 1.000 | 1.000 | 0.708 | 281 | 705 | 271 |
| context_engine_hot | lodash | 4 | 0.500 | 0.500 | 0.500 | 0.500 | 0.500 | 0.500 | 0.500 | 240 | 622 | 473 |
| context_engine_hot | nestjs | 6 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 | 0.333 | 578 | 1397 | 442 |
| context_engine_hot | ripgrep | 6 | 0.500 | 0.667 | 0.667 | 0.417 | 0.667 | 0.667 | 0.583 | 322 | 783 | 494 |
| rg_baseline | django | 6 | 0.167 | 0.333 | 0.333 | 0.167 | 0.250 | 0.333 | 0.250 | 651 | 712 | 64 |
| rg_baseline | gin | 4 | 0.250 | 0.250 | 0.500 | 0.250 | 0.250 | 0.500 | 0.300 | 58 | 72 | 66 |
| rg_baseline | lodash | 4 | 0.500 | 0.500 | 0.750 | 0.500 | 0.500 | 0.750 | 0.550 | 62 | 179 | 167 |
| rg_baseline | nestjs | 6 | 0.167 | 0.167 | 0.167 | 0.056 | 0.111 | 0.111 | 0.167 | 212 | 301 | 54 |
| rg_baseline | ripgrep | 6 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 0.167 | 60 | 67 | 64 |
| serena | django | 6 | 0.667 | 0.667 | 0.667 | 0.667 | 0.667 | 0.667 | 0.667 | 25519 | 104192 | 142 |
| serena | gin | 4 | 0.250 | 0.250 | 0.250 | 0.250 | 0.250 | 0.250 | 0.250 | 1617 | 2926 | 6 |
| serena | lodash | 4 | 0.500 | 0.750 | 0.750 | 0.500 | 0.750 | 0.750 | 0.625 | 2192 | 2906 | 155 |
| serena | nestjs | 6 | 0.167 | 0.333 | 0.333 | 0.167 | 0.333 | 0.333 | 0.250 | 10270 | 76772 | 120 |
| serena | ripgrep | 6 | 0.500 | 0.667 | 0.667 | 0.417 | 0.583 | 0.583 | 0.583 | 1908 | 79637 | 62 |

## CATEGORY

| SYSTEM | CATEGORY | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR |
|---|---|---|---|---|---|---|---|---|---|
| codebase_memory | caller | 3 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| codebase_memory | conceptual | 5 | 0.400 | 0.600 | 0.600 | 0.400 | 0.600 | 0.600 | 0.500 |
| codebase_memory | definition | 8 | 0.250 | 0.375 | 0.375 | 0.250 | 0.375 | 0.375 | 0.312 |
| codebase_memory | exact | 5 | 0.200 | 0.200 | 0.200 | 0.200 | 0.200 | 0.200 | 0.200 |
| codebase_memory | test | 5 | 0.000 | 0.400 | 0.400 | 0.000 | 0.400 | 0.400 | 0.167 |
| context_engine_hot | caller | 3 | 0.333 | 0.333 | 0.333 | 0.167 | 0.333 | 0.333 | 0.333 |
| context_engine_hot | conceptual | 5 | 0.200 | 0.600 | 0.600 | 0.200 | 0.600 | 0.600 | 0.400 |
| context_engine_hot | definition | 8 | 0.875 | 1.000 | 1.000 | 0.875 | 1.000 | 1.000 | 0.938 |
| context_engine_hot | exact | 5 | 0.400 | 0.400 | 0.400 | 0.400 | 0.400 | 0.400 | 0.400 |
| context_engine_hot | test | 5 | 0.400 | 0.600 | 0.600 | 0.400 | 0.600 | 0.600 | 0.467 |
| rg_baseline | caller | 3 | 0.333 | 0.667 | 0.667 | 0.111 | 0.389 | 0.556 | 0.500 |
| rg_baseline | conceptual | 5 | 0.000 | 0.000 | 0.200 | 0.000 | 0.000 | 0.200 | 0.040 |
| rg_baseline | definition | 8 | 0.125 | 0.125 | 0.250 | 0.125 | 0.125 | 0.250 | 0.150 |
| rg_baseline | exact | 5 | 0.800 | 0.800 | 0.800 | 0.800 | 0.800 | 0.800 | 0.800 |
| rg_baseline | test | 5 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| serena | caller | 3 | 0.333 | 0.333 | 0.333 | 0.167 | 0.167 | 0.167 | 0.333 |
| serena | conceptual | 5 | 0.200 | 0.400 | 0.400 | 0.200 | 0.400 | 0.400 | 0.300 |
| serena | definition | 8 | 0.625 | 0.750 | 0.750 | 0.625 | 0.750 | 0.750 | 0.688 |
| serena | exact | 5 | 0.600 | 0.800 | 0.800 | 0.600 | 0.800 | 0.800 | 0.700 |
| serena | test | 5 | 0.200 | 0.200 | 0.200 | 0.200 | 0.200 | 0.200 | 0.200 |

## MACRO AVERAGES

- macro Hit@1: 0.337
- macro Hit@3: 0.452
- macro Hit@5: 0.471
- macro Recall@1: 0.321
- macro Recall@3: 0.439
- macro Recall@5: 0.463
- macro MRR: 0.395

## INDEXING

| ADAPTER | REPO | PROFILE | WALL MS | FILES | SYMBOLS | BM25 | VECTORS | DISK | UNAVAILABLE |
|---|---|---|---|---|---|---|---|---|---|
| context_engine_hot | django | official | 15483 | 3039 | 44010 | 43841 | 44188 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| context_engine_hot | gin | official | 877 | 99 | 1529 | 1533 | 1531 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| context_engine_hot | lodash | official | 937 | 48 | 1012 | 916 | 1076 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| context_engine_hot | nestjs | official | 3411 | 1730 | 5580 | 5650 | 5686 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| context_engine_hot | ripgrep | official | 1066 | 110 | 3602 | 3629 | 3627 | None | index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | django | official | 5177 | 7084 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | gin | official | 114 | 130 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | lodash | official | 91 | 153 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | nestjs | official | 1363 | 2131 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| rg_baseline | ripgrep | official | 163 | 237 | None | None | None | None | symbols,bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| codebase_memory | django | official | 114884 | None | None | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| codebase_memory | gin | official | 10315 | None | 12747 | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| codebase_memory | lodash | official | 10192 | None | 862 | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| codebase_memory | nestjs | official | 14518 | None | 12415 | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| codebase_memory | ripgrep | official | 9534 | None | 5009 | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| serena | django | official | 13433 | 7171 | None | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| serena | gin | official | 13298 | 176 | None | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| serena | lodash | official | 11483 | 197 | None | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| serena | nestjs | official | 11314 | 2175 | None | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |
| serena | ripgrep | official | 11767 | 1181 | None | None | None | None | bm25_docs,vector_count,index_disk_bytes,cpu_ms,peak_rss_mb,no_change_wall_ms,one_file_wall_ms |

## NOTES

- FILE-LEVEL: hits are deduplicated to unique files preserving first rank before Hit/Recall/MRR.
- Hit@K = binary (1 if any expected file in first K unique files). Recall@K = |relevant ∩ first K| / |expected|.
- packed_tokens is real tokenizer count from contextd; candidate_tokens unavailable in production (marked None).
- No benchmark-specific production logic. Failures are roadmap evidence.
- Previous Python-proxy metrics are INVALID / DISCARDED.
