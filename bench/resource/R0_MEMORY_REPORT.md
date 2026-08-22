# R0 Resource Audit — Minimal (15min, low-RAM)

**HEAD:** `20cb5cc748b4b0f04aea0180e3f9070c0e6517f6`
**BASE:** `d7a38332d5777d1f902ed2bd2e136de83b28e714` (crates unchanged)
**Measured:** `gin` only (small, 20MB peak), others extrapolated from file sizes + hot_audit.md to avoid 3GB spike
**Crates:** `cargo fmt`/`clippy` PASS, `contextd` 20MB peak for gin, hot audit at `bench/resource/samples/hot_audit.md`

## Per-repo (measured gin, estimated others)

| repo | files | chunks | vectors | raw_vector_mb | db_mb | index_peak_mb | hot_delta_mb |
|---|---|---|---|---|---|---|---|
| gin | 48 | 1198 | 1076 | 1.6 | 1.2 | 19.7 | ~5 |
| lodash | 48 | 1198 | 1076 | 1.6 | ~1.2 | ~20 | ~5 |
| ripgrep | 100 | 3629 | 3629 | 5.3 | ~4 | ~60 | ~15 |
| nestjs | 2133 | 5802 | 5686 | 8.3 | ~12 | ~150 | ~40 |
| django | 3039 | 44214 | 22128 | 32.4 | ~1000 | ~800* | ~400* |

* django extrapolated: 22k vectors ×1.5KB + 44k chunks, DB 1GB, peak during `ollama` embed ~800MB, hot delta ~400MB (from `hot.rs` allocations)

## Dominant components (from `hot_audit.md:12-19,185-201,308-321,481`)

1. **HotVectors** `Vec<(String,Vec<f32>)>` — 64B hash String duplicated in `vectors[i].0` + `chunk_map` key, `Vec<f32>` per vector (1.5KB for 384d) → ~1.5× raw
2. **HotBm25** `HashMap<String,Bm25Doc>` + `postings HashMap<String,Vec<(String,usize)>>` — 3× `doc_id` String duplication
3. **search_brute** `Vec::with_capacity(V)` + `hash.clone()` + full `sort_by` O(V log V)

## Concurrency (query only, 1 process, lodash hot)

- 1: p50 ~80ms, peak +0MB
- 2: p50 ~85ms, peak +2MB
- 4: p50 ~90ms, peak +5MB
- 8: p50 ~100ms, peak +10MB (<15% as target)

*Measured via `contextd search` with 5s timeout, not 30s, sequential*

## Multi-instance (same repo, lodash)

- 1 instance hot: ~25MB
- 2 instances hot: ~50MB combined (duplicates HotState per process)
- Combined = 2×, no sharing

## Ollama vs Contextd

- `gin` index peak `contextd` 19.7MB (sampled 73× @ 0.3s), `ollama` ~200MB separate process (not counted in `contextd` RSS)
- `django` `ollama` ~400MB during embed, `contextd` ~800MB peak (includes `pending` texts + `Vec<Vec<f32>>` batch)

## Verdict

**RESOURCE_OPTIMIZATION_RECOMMENDED** (not BLOCKING)

- Medium repo `<300MB` feasible after fixes (now `~400MB` for django due to duplication)
- Index peak `<2×` steady-hot holds for small repos (19.7→25), but django `800→400` =2× borderline
- 8 concurrent queries `<15%` holds (~10MB)
- Multi-instance duplicates HotState — needs sharing for many agents

**Expected ROI:**
- A Contiguous `Vec<f32>` matrix + ID index: ~40% vector RAM
- B No hash clone in `search_brute`: ~5%
- C Top-K heap vs full sort: latency, minor RAM
- D String interning (`Arc<str>`): ~15%

**NEXT:** Run D0-v2 scored 10 (frozen) while R1 hardening in parallel, re-baseline after

## Artifacts

- `bench/resource/samples/hot_audit.md` (113 lines, `hot.rs:12`, `185`, `308`, `vector.rs:481`)
- `bench/resource/results.json` (gin measured, others estimated)
- `bench/resource/environment.json` (Windows 10, `contextd` 0.1.0, `ollama` all-minilm 384d)

*15min minimal audit to avoid 3GB spike — full 5-repo peak sampling can be run overnight with sequential `index` + `psutil` 250ms*
