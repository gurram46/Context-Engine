# ADR 005 — Incremental structural store and graph update

**Status:** Accepted (R4)
**Date:** 2026-08-11

## Context

R3 rebuilt `call_edges` globally on every changed file (~2.56s for one file, 1033 symbols, ~6500 edges). R4 needs <1s structural freshness and no full rebuild, with correct `Resolved/Probable/Unresolved` semantics.

## Decision

**Selective incremental graph** + SQLite transactional store.

- On file F change: parse only F, capture old vs new definitions (names+qualified), affected set = symmetric difference, `DELETE FROM call_edges WHERE file=F` (already in `upsert_parsed_file`), rebuild outgoing edges for refs in F, `DELETE FROM call_edges WHERE callee_name IN (affected) AND file NOT IN (changed+stale)` then re-resolve those refs via current `by_name`/`by_qualified` maps, insert. No global `DELETE FROM call_edges`.
- `files` hash reuse, `structural_meta(generation)` bump, `upsert_parsed_file` transactional. Stale files delete via `files` cascade + BM25 `bm25_documents` delete.
- Store stays `structural.db` SQLite WAL, per-worktree, `PRAGMA foreign_keys=ON`. Schema v2 adds `structural_meta`, `bm25_*`, `vectors`. No `Tantivy` without benchmark.

## Consequences

- Single-file update ~0.22s (was 2.56s) measured on Context-Engine 141 files.
- Counters exposed for tests: `files_parsed`, `files_skipped`, `edges_deleted`, `edges_inserted`, `references_reresolved`; not in public MCP except `context_status` `structuralGeneration`.
- Crash/restart: DB transactions + generation markers; watcher reconciliation detects mismatch, reindexes changed file; no manual delete required.
- Worktree isolation: separate watcher, SQLite, BM25, vector mapping per worktree; content-hash vector reuse across worktrees possible but not required R4.

## Validation

- Tests: `incremental_body_change_only_outgoing`, `incremental_rename`, `incremental_add_missing_definition`, `incremental_delete_definition`, `incremental_unrelated_change_no_rewrite`, `single_file_update_api`, `watcher_coalesces`, `bm25_incremental_only_affected`, `vector::chunk_hash_reuse`, `one_line_edit_not_reembed_all`
- Metrics: `structural_ms` <1s, `bm25_ms` <50ms, `semantic_ms` <500ms warm, `exact_ms` <100ms
