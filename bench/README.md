# Context Bench v1

Reproducible benchmark harness for Context Engine retrieval.

**C0 scope: benchmark infrastructure only — no production ranking changes.**

## Goal

Measure on *real external repos* (pinned commits, never floating main):

1. retrieval accuracy (Top1, R@3, R@5, MRR)
2. structural accuracy (definition / caller / callee)
3. test lookup accuracy
4. latency (per-query + per-stage when exposed)
5. indexing cost (initial, no-change reconcile, one-file change, disk, RSS)
6. context efficiency (candidate tokens, packed tokens, compression ratio)

Agent task success will be compared in a later milestone, not C0.

## Design

```
bench/
  README.md
  manifest.json              # pinned repos (name, url, commit, language)
  schema/question.schema.json
  questions/*.jsonl          # versioned, machine-readable, ground truth outside prod
  adapters/
    interface.py             # BenchmarkAdapter contract
    context_engine.py        # harness-side re-impl (C0) — later delegates to real contextd
    rg_baseline.py           # plain ripgrep baseline (reproducible, not crippled)
    oci.py / codebase_memory.py / serena.py  # placeholders, not mandatory in C0
  results/                   # JSONL raw + summary.md (gitignored except .gitkeep)
  scripts/
    checkout.py              # shallow clone at pinned commit
    run.py                   # harness runner
    report.py                # summary generator
  repos/                     # gitignored; shallow checkouts at pinned commits
```

Adapter interface (`adapters/interface.py`) is stable:
- `index(repo_path) -> IndexingMetrics`
- `search(query, repo_path, top_n) -> SearchResult`
Future adapters (OCI, Codebase-Memory-MCP, Serena) subclass without runner changes.

## Question format (v1)

```json
{
  "id": "django_model_definition_001",
  "repo": "django",
  "category": "definition|caller|callee|test|conceptual|exact",
  "query": "Where is Model implemented?",
  "expected_files": ["django/db/models/base.py"],
  "expected_symbols": ["Model"],
  "notes": "verifiable via grep",
  "ground_truth_source": "manual|compiler|repository-test|other"
}
```

Schema: `schema/question.schema.json`. Ground truth lives in `questions/`, never in production code. Production may not inspect `repo`, `expected_*`, `benchmark` env.

## First milestone (C0 M1)

Prove harness end-to-end on:

- `django/django` @ `c6be0bf3`
- `nestjs/nest` @ `674ac31d`
- `BurntSushi/ripgrep` @ `3fce3b5b`

5+ questions per repo, 15+ total. Run:

- `context_engine` vs `rg_baseline`
- produce `results/results.jsonl` + `results/summary.md` with SYSTEM/REPO/CATEGORY + macro averages, P50/P95 latency, tokens.

Then stop and review methodology before scaling to 100–150.

## Metrics (per query)

- `top1_correct`, `recall@1`, `recall@3`, `recall@5`, `mrr`, `rank` (first expected)
- `candidate_count`, `evidence_count`, `files_returned`
- `candidate_tokens`, `packed_tokens`, `compression_ratio` (packed/candidate, not "tokens saved")
- `retrievers_used`, `elapsed_ms`, plus per-stage `exact_ms`/`structural_ms`/`bm25_ms`/`semantic_ms`/`rank_ms`/`pack_ms` when exposed

No fake `candidate_tokens - packed_tokens` as "saved" — real savings need controlled agent A/B later.

### Indexing (per repo per adapter, when applicable)

- initial wall, CPU, peak RSS, disk, files_indexed, symbols, bm25_docs, vector_count
- no-change reconcile wall
- one-file change wall, affected updates, size delta

Mark unavailable as `null` + `unavailable: [...]`.

## Plain rg baseline

Uses `rg --fixed-strings --max-count 50 --hidden -g !.git/**` plus generic excludes (`node_modules`, `dist`, `target`, `__pycache__`, `.pytest_cache`, `.next`, `coverage`). No semantic, no index. Behavior documented in `adapters/rg_baseline.py` for reproducibility. Not crippled.

## Integrity

- No production code may check `repo`, `benchmark`, `expected`, `fixture`, `golden`, or benchmark env to adjust ranking.
- No weight change to make benchmark pass. If Context Engine fails, **record the failure**.
- Benchmark metadata is harness-only. If you need to verify, do it outside the adapter.

## Usage

```bash
# 1. checkout pinned repos (shallow, fixed commit)
python bench/scripts/checkout.py

# 2. run harness (writes bench/results/results.jsonl + summary.md)
python bench/scripts/run.py --adapters context_engine,rg_baseline --top-n 5

# 3. report
python bench/scripts/report.py --input bench/results/results.jsonl
cat bench/results/summary.md
```

## Scaling

Question workflow is in `questions/*.jsonl` — add 80–120 more high-confidence questions after M1 review. Ground truth must be independently verifiable (manual + `rg`/`compiler`/`repository-test`). Do not manufacture 150 low-quality.

## Quality

- Production `backend/` unchanged in C0.
- `cargo fmt/clippy/test` (if workspace exists) must still pass.
- Existing frozen retrieval tests unchanged.

See `bench/manifest.json` for pinned SHAs, `bench/schema/` for versioned schema.
