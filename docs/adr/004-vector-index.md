# ADR 004 — Vector index choice (brute vs HNSW)

**Status:** Accepted (R4)
**Date:** 2026-08-11

## Context

R4 needs per-worktree native vector search. Candidate is `usearch`/HNSW for large repos, but brute-force cosine is the correctness baseline for small fixtures. We must benchmark before adding complexity.

## Decision

**Brute-force cosine as R4 production path**, with HNSW/`usearch` deferred.

- Vectors stored content-hash-keyed in `vectors` SQLite (`structural.db`), normalized dot == cosine.
- `search_brute` is reference truth; for current Context-Engine (141 files, ~1200 chunks) brute is <30ms (vector search excl. query embedding <50ms target met), so HNSW not needed yet.
- HNSW (`usearch`) would be persisted per-worktree, deterministic `vector-id ↔ chunk-id` via `content_hash`, deletion via stale chunk removal, crash-safe via SQLite WAL, no shared writable index. ANN TopK overlap vs brute must be validated before adopting. Do not silently accept approximate errors.

## Consequences

- R4 satisfies latency: warm query total <500ms (including fake embedding), vector search <50ms, BM25 <50ms.
- Large-repo path: if future repo shows brute >50ms, add `usearch` HNSW with persisted index and overlap validation. No `Tantivy`/`usearch` added without measurement.
- No shared worktree corruption; separate `.context/index/structural.db` per worktree.

## Validation

- `vector::tests::chunk_hash_reuse`, `one_line_edit_not_reembed_all`, `model_change_invalidation`
- Benchmark: brute vs hypothetical HNSW overlap (deferred, baseline established)
- Disk: `structural.db` includes `vectors` (≈ 4.7KB per 768d vector, 1200 chunks ≈ 5.6MB)

## Alternatives

- `Tantivy` for lexical already rejected for BM25 (native SQLite suffices).
- `usearch` HNSW immediately: deferred until measurement proves need.
