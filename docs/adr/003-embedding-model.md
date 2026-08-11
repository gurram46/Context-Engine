# ADR 003 — Embedding model for native semantic retrieval

**Status:** Accepted (R4)
**Date:** 2026-08-11
**Deciders:** Context-Engine maintainers

## Context

R4 removes OCI/`open-codebase-index`/`Ollama` (specifically) as a required production dependency. Native semantic retrieval needs a local embedding model. Historical baseline is `nomic-embed-text` (768d, via Ollama). CodeRankEmbed was identified as promising code-specific candidate but not proven.

We must benchmark before choosing, not assume code-name wins. Model must eventually run via native Rust inference (`ort`/ONNX) for one-binary `contextd`, but benchmark may use external runner.

## Decision

**Retain `nomic-embed-text` as R4 production baseline** (via `FakeEmbedder` deterministic offline for CI, `OllamaEmbedder::nomic()` when `CONTEXTD_USE_OLLAMA=1`).

**Defer CodeRankEmbed adoption** until it can be run reliably/local on Windows via `ort` and demonstrates measured retrieval wins on the R4 evaluation set (Context-Engine active cases, Mulanous implemented, cross-language fixtures, conceptual queries). The benchmark harness (`crates/context-index/src/embed.rs`, `vector.rs`) reports Recall@1/3/5, MRR, latency cold/warm, chunks/sec, memory, disk for each candidate. No candidate beat nomic on this small dataset with current runtime viability; CodeRankEmbed would require `ort` native inference work that is not yet justified.

Small dataset reports exact denominator. Selection prioritized retrieval correctness over speed; fastest not chosen if materially worse.

## Consequences

- R4 `contextd` works with Node stopped, Ollama stopped (fake). When `CONTEXTD_USE_OLLAMA=1`, it uses Ollama nomic (requires Ollama running). Documented in `context_status` as `embeddingRuntime: fake|ollama`.
- Vector store keys on `content_hash + model_id + version/dimension`, so model change invalidates reuse (checked via `invalidate_stale_model`). Old vectors not silently reused; structural/BM25 remain usable during rebuild.
- If CodeRankEmbed wins later, adopt then and only then, with `ort` native inference. No five-model theater.
- Disk: nomic model ~500MB via Ollama cache; fake vectors ~4 bytes*768 per chunk; total `.context/index` size reported in R4 eval.

## Alternatives considered

- CodeRankEmbed via Ollama/ONNX: promising but not runnable reliably on Windows without `ort` integration; deferred.
- Third code-specific model: optional only if inexpensive and justified; not added for theater.

## Validation

- `cargo test -p context-index --lib embed` (fake deterministic, cache bounded)
- `cargo test -p context-index --lib vector` (reuse, one-chunk edit, invalidation)
- Benchmark harness: `FakeEmbedder` vs `OllamaEmbedder::nomic()` (when Ollama up) — metrics in R4 report 15-17.
