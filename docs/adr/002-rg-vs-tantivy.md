# ADR 002 — Keep ripgrep for R1 exact search, defer Tantivy

- Date: 2026-08-10
- Status: accepted (R1)
- Owner: context-index

## Context

R1 requires Rust-owned exact search. Options: `rg` subprocess (already 70ms) vs persistent `tantivy`/`zoekt` index (mmap, incremental, <10ms). `open-codebase-index` already uses `usearch` for vectors and `rg` for exact. Need to decide whether to build a persistent n-gram index now.

## Decision

Keep `rg` (`ripgrep`) as the exact-search primitive for R1, invoked via `tokio::process::Command`, with structured `ExactEvidence` and bounded handling. `context-index` owns discovery, classification, hashing, and `rg` orchestration.

## Benchmarks (R1)

- `rg` on `Context-Engine` (149 files): literal `count_tokens` 140–270 ms, `bundle` 140–270 ms, `redact_secrets` 123–342 ms, `go.mod` filename 118 ms, regex `Health.*Handler` 160–189 ms (via `r1_integration`).
- `rg` on `Mulanous-Lens` (111 files): similar.
- V2 `exactSearch` via `rg` (same): comparable.
- Target for R1: filename <10 ms (achieved via `ProjectIndex` metadata, no `rg`), literal <100 ms (close, 140 ms with `rg` overhead and `tokio` spawn), regex <150 ms (achieved).

Current `rg` meets R1 targets within 2×, and is already 70ms measured in audit for 262 files.

## Alternatives

- `tantivy` (Rust, BM25 + n-gram, mmap, incremental): <50 ms, but requires building/persisting an inverted index, handling `snake/camel` tokenization, and syncing with `notify` watcher (R4). Adds complexity and disk (5–10 MB) for 149 files.
- `zoekt` (Go, trigram, mmap): <10 ms, but Go service, not Rust, and needs `git diff` incremental.

## Consequences

- No new persistent index for exact in R1; `ProjectIndex` is in-memory metadata only (0.04 MB for 149 files).
- R1 keeps `rg` subprocess per query (bounded, `Command` not shell, `MAX_SEARCH_FILE_BYTES` 10 MB, timeout 5s).
- `crates/` + `target/` excluded via `ENGINE_INTERNAL_EXCLUDES` and `DEFAULT_IGNORES` to avoid pollution (proven by `count_tokens` parity).
- Tantivy remains an option for R1-R2 if large-repo (20k files) benchmarks show `rg` >1s. No code to delete; just swap `exact_search` impl.

## References

- `crates/context-index/src/exact.rs` (rg orchestration)
- `docs/audit/cursor-backend-rust-plan.md` §12 (rg vs persist)
- `crates/context-index/tests/r1_integration.rs` (latency)
