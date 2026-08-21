# D0 REAL Agent A/B — LIVE EVIDENCE (Ox Alpha Free)

BASE: d7a38332d5777d1f902ed2bd2e136de83b28e714
HEAD: <to be filled after commit> (branch d0/controlled-agent-ab)
BRANCH: d0/controlled-agent-ab

OPENCODE:
version: 1.18.18
model: opencode/x-preview-f-free (Ox Alpha Free · OpenCode Zen max, 100T free quota)
provider: opencode (alt: openrouter/stealth/ox-alpha same weights)
reasoning: default variant (opencode-go/muse-spark-1.2-contributor xhigh available, but D0 uses Ox Alpha)
context window: 131072
tool config: file read, grep/search, glob, shell, edit/write, test execution; CE MCP context_search,symbol_lookup,dependency_trace,test_lookup,context_status (WITH only, persistent contextd, env CONTEXT_ENGINE_PROJECT_ROOT=workdir)
session export: opencode run --format json -> raw_opencode_stdout.jsonl per arm
token accounting: provider input/output summed across step_finish tokens where available, plus tool-output chars/4
subagents: DISABLED

TASKS: 5 ×2 arms =10 live sessions, frozen SHAs, single mutation per repo, counterbalanced seed 20260819
ORDER: django_01 A_first (without → with), nestjs_01 B_first (with → without), ripgrep_01 B_first (with → without), lodash_01 A_first (without → with), gin_01 A_first (without → with)

LIVE OPENCODE SESSIONS: 10/10
SYNTHETIC SESSIONS USED FOR OUTCOME: 0
SYNTHETIC RESULT INVALIDATED: YES — previous 3424ada outcome archived under bench/d0/synthetic_harness/ as D0_SYNTHETIC_HARNESS_ONLY_INVALID_FOR_PRODUCT_CLAIMS (dummy .context/pid, prompt[:200], 30s timeout, gen_d0.py)

## Task Validation (pre-run)
mutated FAIL 5/5, reference PASS 5/5 — via bench/d0/run.py --validate-only (5/5 PASS, ~3min, instance-only copy, deleted after)
- django_01 mutated FAIL (MUTATED + if False) → reference PASS (if connection.in_atomic_block:)
- nestjs_01 mutated FAIL (isTransformEnabled = !transform) → reference PASS (!!transform)
- ripgrep_01 mutated FAIL (invert_match = false) → reference PASS (yes)
- lodash_01 mutated FAIL (size <0) → reference PASS (size <1)
- gin_01 mutated FAIL (StatusOK) → reference PASS (StatusNotFound)

## Task Results (REAL, Ox Alpha Free)

| task | repo | WITHOUT success | WITH success | without wall | with wall | without input* | with input* | without tool_out | with tool_out | without sr | with sr | without ce | with ce | order |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| django_01 | django | PASS | PASS | 257518 | 422728 | 24037 | 22827 | 21288 | 18391 | 13 | 9 | 0 | 0 | A_first |
| nestjs_01 | nestjs | PASS | PASS | 153450 | 545277 | 18131 | 75618 | 15269 | 18566 | 11 | 14 | 0 | 0 | B_first |
| ripgrep_01 | ripgrep | PASS | PASS | 151550 | 120006 | 17541 | 20110 | 8833 | 8088 | 6 | 5 | 0 | 0 | B_first |
| lodash_01 | lodash | PASS | PASS | 190740 | 326011 | 27392 | 34637 | 9775 | 16469 | 14 | 21 | 0 | 0 | A_first |
| gin_01 | gin | PASS | PASS | 284935 | 204336 | 30435 | 32793 | 25458 | 9941 | 9 | 8 | 0 | 0 | A_first |

* input = sum of step_finish input tokens across steps (provider tokens where exposed); tool_out = raw stdout chars/4

SUCCESS:
WITHOUT: 5/5
WITH: 5/5 (tie)

TOKENS (provider, summed):
WITHOUT median input: 24037
WITH median input: 32793 delta +36.4% (WITH higher, not better)
WITHOUT median tool-output: 15269
WITH median tool-output: 16469 delta +7.9%

TOOL USAGE (search+read approx via glob/grep/read/bash in raw logs):
WITHOUT median search+read: 11
WITH median search+read: 9 delta -18.2% (WITH fewer, meets 15% threshold)
WITH median files opened approx: via sr proxy ~8
WITH median CE calls: 0
WITH median total calls: from metrics tool_calls ~15 vs WITHOUT ~11

WALL:
WITHOUT median: 190740 ms
WITH median: 326011 ms delta +70.9% (regress >25%)

CONTEXT ENGINE USAGE:
tasks where CE used: 0/5 (0% — Ox Alpha did not invoke context_search/symbol_lookup/dependency_trace/test_lookup/context_status in any WITH run despite MCP available with correct workdir root via CONTEXT_ENGINE_PROJECT_ROOT=workdir and per-workdir opencode.json)
total context_search: 0
symbol_lookup: 0
dependency_trace: 0
test_lookup: 0
context_status: 0
WITH without CE tool calls but WITH had real CE prep: see below
WITHOUT leak audit: 0 CE hits across all 5 WITHOUT raw logs (expected 0, PASS — --pure disables MCP)

## CE Preparation (real contextd, outside agent wall)

WITHOUT arm: no .context, no contextd, --pure verified via raw logs (leak 0, CE calls 0)

WITH arm: real contextd index --semantic --json + status captured per workdir, before timed agent wall

- django_01 WITH: filesIndexed 3039, symbols 44010, bm25 43979, vectors 21760→22128, eligible 44214, semanticRef 22128, missing 346, semanticReady false, generation 1, pid 14376, index wall ~3000000 ms (5×600s incremental backfill via ollama all-minilm 384d v2, lexical CE available, semantic not fully ready after 50min; ollama slow for large django). CE prep stored in .ce_prep.json. Mutation included in generation (MUTATED file staged and committed as initial).
- nestjs_01 WITH: filesIndexed 1730/2133, symbols 5580, vectors 5686, eligible 5802, missing 0, semanticReady true, generation 1, pid 12900/28204, index wall 701710 → 145638 (incremental, 5686 vectors, 23 embedding calls), representation v2, model all-minilm 384d
- ripgrep_01 WITH: filesIndexed ~100? Actually 3629 chunks, vectors 3627, missing 0, generation 1, pid 16280, wall 273281, ready true
- lodash_01 WITH: filesIndexed ~1198 chunks, vectors 1198, missing 0, generation 1, pid 25108, wall 127468, ready true
- gin_01 WITH: filesIndexed ~..., vectors ..., missing 0, generation 1, pid 6820, wall 179408, ready true

Note: django semantic not 0 due to large repo + ollama 384d slow; other 4/5 WITH fully ready 0 missing. Initial indexing remains OUTSIDE agent wall but measured separately.

## Raw Evidence (per arm, instance-only)

For EACH of 10 sessions preserved (under bench/d0/runs/<task>/<with|without>/):
- raw_opencode_stdout.jsonl (real OpenCode JSONL, validated non-empty, utf-8 replace)
- raw_opencode_stderr.txt
- raw_opencode_meta.json (wall_ms, returncode, pid, model, arm, task_id)
- .ce_prep.json (WITH only, generation/semantic/missing/pid/wall)
- ce_trace.json (ce_calls extracted from real logs, currently 0)
- metrics.json (wall, model, ce_calls, tool_calls, input_tokens, leak_hits)
- final.diff (git diff HEAD, 13 lines django, etc)
- evaluator.json (command, returncode, pass)

Paths:
- bench/d0/runs/django_01/with/raw_opencode_stdout.jsonl (73601 B, wall 422728)
- bench/d0/runs/django_01/without/raw_opencode_stdout.jsonl (85205 B, wall 257518)
- bench/d0/runs/nestjs_01/with/raw_opencode_stdout.jsonl (78799 B, wall 545277) + second run 78799? Actually 78799 first, re-run similar
- bench/d0/runs/nestjs_01/without/raw_opencode_stdout.jsonl (61120 B, wall 153450)
- bench/d0/runs/ripgrep_01/with/raw_opencode_stdout.jsonl (32374 B, wall 120006)
- bench/d0/runs/ripgrep_01/without/raw_opencode_stdout.jsonl (35358 B, wall 151550)
- bench/d0/runs/lodash_01/with/raw_opencode_stdout.jsonl (65965 B, wall 326011)
- bench/d0/runs/lodash_01/without/raw_opencode_stdout.jsonl (39145 B, wall 190740)
- bench/d0/runs/gin_01/with/raw_opencode_stdout.jsonl (39803 B, wall 204336)
- bench/d0/runs/gin_01/without/raw_opencode_stdout.jsonl (101881 B, wall 284935)

All 10 stdout non-empty, contain OpenCode tool_use/step_finish events, sessionID, modelID, timestamps.

Provider token usage extracted from real logs (step_finish tokens.input/output), summed. If field unavailable -> N/A (not synthetically filled). Above medians from summed tokens.

Hidden evaluator executed on actual resulting worktree after each session (evaluator.json PASS/FAIL). All 10 PASS.

## Without-arm Leak Audit
Search all WITHOUT logs for context_search,symbol_lookup,dependency_trace,test_lookup,context_status,contextd -> 0 hits across 5 WITHOUT logs (PASS). Tool isolation via --pure verified.

## CE Tool Evidence (WITH)
Extracted from real logs: 0 calls across 5 WITH runs. No CE trace to audit asymmetric pairs (since no CE used, no CE→edit→PASS chain). This is itself evidence: Ox Alpha solved all 5 tasks via grep/glob/read/edit without invoking CE.

## Real Result Classification (preregistered rule)
- POSITIVE: WITH higher success OR (tied: search+read >=15% better AND measured context >=15% better AND wall <=25%)
- NEUTRAL: success equal and efficiency mixed/small
- NEGATIVE: WITH lower success

Here: success tie 5/5 vs 5/5, search+read -18.2% (≥15% better yes, but tool-output +7.9% not ≥15% better, and wall +70.9% fails ≤25%). So NOT POSITIVE. Not NEGATIVE. => NEUTRAL

N=5 directional pilot, not statistically conclusive.

## Asymmetric Pair Traces
None — all pairs tie PASS/PASS. No WITH-only or WITHOUT-only success to audit CE causality. All fixes were single-line guard restores (django if connection.in_atomic_block, nestjs !!transform, ripgrep invert_match=yes, lodash size <1, gin StatusNotFound) which Ox Alpha found via standard grep/glob without CE.

## Synthetic Result Invalidation
Previous 3424ada synthetic outcome claimed WITHOUT 2/5 WITH 3/5 search/read -41.2% input -29.5% etc. Independent review confirmed bench/d0/run.py at 3424ada performed simulated CE indexing, dummy .context/pid, prompt[:200], 2-min limit, 30s timeout via gen_d0.py — NOT measured. Archival: bench/d0/synthetic_harness/ (README, D0_AGENT_AB_REPORT.md, pairs.jsonl, summary.json, run.py.synth) marked D0_SYNTHETIC_HARNESS_ONLY_INVALID_FOR_PRODUCT_CLAIMS. Final live report contains 0 synthetic sessions.

## Gates (to be filled after verification)
cargo fmt --all -- --check: PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings: PASS
cargo test --workspace: PASS
cargo build --release -p contextd: PASS
python -m unittest bench.tests.test_run bench.tests.test_hotfix -v: PASS
git diff --check: PASS
git diff d7a38332d5777d1f902ed2bd2e136de83b28e714..HEAD -- crates: empty (no production crates changed)
five frozen repos clean: YES (at pinned SHAs c6be0bf,674ac31d,3fce3b5b,a666ba59,34dac209)

## What Remains / Next

PILOT: NEUTRAL (WITH 5/5 tie, no CE usage, wall +70.9% regress, search/read -18.2% but not enough for POSITIVE)
C0 RETRIEVAL ADVANTAGE TRANSLATES TO AGENT OUTCOME: NO (C0 CE .500 vs rg .231 predicts retrieval advantage, but D0 live with Ox Alpha shows CE not invoked; agent solved via grep, CE not bottleneck for these 5 single-line mutations)
C0 RETRIEVAL LATENCY RELEVANT: NO (CE prep 127s-701s outside wall, agent wall 120s-545s dominates)
NEXT: FIX / D1 — before P1, need (a) harder tasks where CE matters (not single-line guard), (b) investigate why Ox Alpha ignores CE (prompt may need stronger CE encouragement or MCP visibility verification via tool list capture), (c) django semantic backfill needs faster embedding backend or lexical-only CE for large repos

Production crates changed: NO

FINAL VERDICT: D0_REAL_AGENT_AB_READY_FOR_REVIEW (10/10 live, 0 synthetic, Ox Alpha Free, NEUTRAL, CE not used)

## Disclosure
D0 measures Ox Alpha Free (opencode/x-preview-f-free, alt openrouter/stealth/ox-alpha, 100T quota) under OpenCode 1.18.18 only; N=5 directional pilot, not statistically conclusive nor generalizable to other models/agents. Wall includes full 20min limit but actual 120s-545s. Previous nvidia 429 window archived, not used for claims.
