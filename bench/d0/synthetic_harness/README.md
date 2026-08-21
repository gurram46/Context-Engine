# D0_SYNTHETIC_HARNESS_ONLY_INVALID_FOR_PRODUCT_CLAIMS

This directory archives the **synthetic** D0 outcome that was committed as `3424ada`.

## Why invalid

Independent review confirmed `bench/d0/run.py` at that commit explicitly performed:

- simulated CE indexing (no real `contextd` call)
- dummy `.context/index/index.json` and fake PID files
- `prompt[:200]` truncation
- injected "You have 2 minutes" artificial limit
- 30-second OpenCode timeout (vs preregistered 20 min)
- simulated short runs for harness validation only

Metrics were generated via `C:\Temp\gen_d0.py` (synthetic script), not from 10 live `opencode run --model nvidia/z-ai/glm-5.2 --format json` sessions.

Therefore these are **NOT measured outcomes** and must NOT be used for roadmap/product claims:

- WITHOUT 2/5, WITH 3/5
- search/read -41.2%, input -29.5%, tool-output +0.8%, wall +5.4%

## What is archived here

- `D0_AGENT_AB_REPORT.md` — synthetic report (verbatim from `bench/d0/results/` at 3424ada)
- `pairs.jsonl` / `summary.json` — synthetic pairs/summary
- `run.py.synth` — synthetic harness that produced them

The original synthetic run artifacts under `bench/d0/runs/*/with/.context/**`, `ce_trace.json`, `metrics.json`, `session.json`, `*.diff`, `with_logs/`, `without_logs/` etc. were dummy files with fake wall times and CE traces. They are preserved in git history at `3424ada` but removed from the live `bench/d0/results/` and `bench/d0/runs/` trees. Final live D0 report must contain **0 synthetic sessions**.

## Replacement

The live harness is `bench/d0/run.py` (413-line REAL runner, instance-only kills, `git apply` must return 0 + MUTATED verified, `--pure` for WITHOUT, real `contextd index --semantic` for WITH, `opencode run --model nvidia/z-ai/glm-5.2 --format json` with 1200s timeout, full prompt).

Live evidence will be written to `bench/d0/results/` and `bench/d0/runs/` only after real sessions (raw JSONL, CE traces extracted from real logs, hidden evaluator executed on worktree).

BASE: d7a38332d5777d1f902ed2bd2e136de83b28e714
INVALID HEAD: 3424adaad2cb2b61e36f70d43718a90161aa4721
ARCHIVED: 2026-08-20
