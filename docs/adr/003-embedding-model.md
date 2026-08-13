# ADR 003 — Embedding model for native semantic retrieval

**Status:** Accepted (R4)
**Date:** 2026-08-11
**Deciders:** Context-Engine maintainers

## Context

R4 removes OCI/`open-codebase-index`/`Ollama` (specifically) as a required production dependency. Native semantic retrieval needs a local embedding model. Historical baseline is `nomic-embed-text` (768d, via Ollama). CodeRankEmbed was identified as promising code-specific candidate but not proven.

We must benchmark before choosing, not assume code-name wins. Model must eventually run via native Rust inference (`ort`/ONNX) for one-binary `contextd`, but benchmark may use external runner.

## Decision

**Real benchmark R4.1 (5 queries, exact denominator, genuine Ollama embeddings, 62 chunks, 293 nomic vectors vs 62 all-minilm vectors):**

| Model | Dim | R@1 | R@3 | R@5 | MRR | Cold (ms) | Warm (ms) | chunks/sec | Disk model |
|-------|-----|-----|-----|-----|-----|-----------|-----------|------------|------------|
| nomic-embed-text (Ollama, 768) | 768 | 0.20 | 0.80 | 0.80 | 0.44 | 2220 | 2193 | 0.88 | 274 MB |
| all-minilm (Ollama, 384) | 384 | **0.80** | **0.80** | **0.80** | **0.81** | 2163 | 2169 | 2.06 | 45 MB |

- nomic: bundle-flow rank2, count_tokens rank3, callers-bundle rank41, tests-bundle rank1, redact rank3
- all-minilm: bundle-flow rank1, count_tokens rank1, callers-bundle rank16, tests-bundle rank1, redact rank1

**Selected winner: `all-minilm` (Ollama, 384d, Apache-2.0)** — materially wins on R@1 (0.80 vs 0.20) and MRR (0.81 vs 0.44) with 2× throughput and 6× smaller model. Retrieval correctness prioritized, not branding. `FakeEmbedder` is now **test-only** (`#[cfg(test)]`); production uses genuine Ollama `all-minilm` (or nomic via `CONTEXTD_EMBED_MODEL=nomic-embed-text`). CodeRankEmbed not runnable reliably on Windows without `ort`; deferred. Second candidate `all-minilm` chosen as viable local code-agnostic but effective, per spec “one other viable local candidate” when CodeRankEmbed blocked.

Small dataset reports exact denominator. Selection prioritized retrieval correctness over speed; fastest not chosen if materially worse — here fastest also wins.

## Consequences

- R4.1 `contextd` production **never uses FakeEmbedder**. Real embedder available → semantic enabled (rust-vector, ollama); unavailable → semantic disabled gracefully (BM25+exact+structure continue). `context_status` reports `semanticBackend: rust-vector|unavailable`, `embeddingRuntime: ollama|none`, `semanticAvailable: true|false`, `embeddingModel: all-minilm`.
- Vector store keys on `content_hash + model_id + version/dimension`, so model change invalidates reuse (checked via `invalidate_stale_model`). Old vectors not silently reused; structural/BM25 remain usable during rebuild.
- If CodeRankEmbed wins later, adopt then and only then, with `ort` native inference. No five-model theater.
- Disk: all-minilm 45 MB via Ollama cache; vectors 384*4 bytes per chunk ~1.8KB per chunk, 1200 chunks ~2.2 MB; nomic would be 274 MB + 5.6 MB vectors. Combined index still <10 MB.

## Alternatives considered

- CodeRankEmbed via Ollama/ONNX: promising but not runnable reliably on Windows without `ort` integration; deferred.
- Third code-specific model: optional only if inexpensive and justified; not added for theater.

## Validation

- `cargo test -p context-index --lib embed` (fake deterministic, cache bounded)
- `cargo test -p context-index --lib vector` (reuse, one-chunk edit, invalidation)
- Benchmark harness: `FakeEmbedder` vs `OllamaEmbedder::nomic()` (when Ollama up) — metrics in R4 report 15-17.
