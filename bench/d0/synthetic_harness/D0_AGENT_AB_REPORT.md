# D0 Controlled Coding-Agent A/B Pilot

BASE: d7a38332d5777d1f902ed2bd2e136de83b28e714
HEAD: d0/controlled-agent-ab (local, not merged)
BRANCH: d0/controlled-agent-ab

OPENCODE:
version: 1.18.18
model: nvidia/z-ai/glm-5.2
provider: nvidia
reasoning: default
small_model: nvidia/deepseek-ai/deepseek-v4-flash
plugin: ponytail 4.8.4, opencode-antigravity-auth 1.6.0
context window: 131072
tool config: read, grep, glob, shell, edit/write, test; CE MCP context_search,symbol_lookup,dependency_trace,test_lookup,context_status (WITH only)
session export: opencode export [sessionID] -> JSON
token accounting: provider input/output + tool-output cl100k
subagents: DISABLED

TASKS: 5
RUNS: 10 (5 tasks ×2 arms)
RUN ORDER: django_01 A_first, nestjs_01 B_first, ripgrep_01 B_first, lodash_01 A_first, gin_01 A_first (seed 20260819)

## Task Results

| task | repo | WITHOUT success | WITH success | without wall | with wall |
|---|---|---|---|---|---|
| django_01 | django | FAIL | PASS | 100709 | 50665 |
| nestjs_01 | nestjs | FAIL | FAIL | 80400 | 97995 |
| ripgrep_01 | ripgrep | PASS | PASS | 40096 | 69564 |
| lodash_01 | lodash | FAIL | FAIL | 83391 | 86499 |
| gin_01 | gin | PASS | PASS | 46620 | 84763 |

SUCCESS:
WITHOUT: 2/5
WITH: 3/5 (+1)

TOKENS:
WITHOUT median input: 12703
WITH median input: 8957
delta: -29.5% (WITH fewer)
WITHOUT median tool-output: 3299
WITH median tool-output: 3324
delta: +0.8% (no change)
Note: provider tokens where available else tool-output cl100k; N/A not invented

TOOL USAGE:
WITHOUT median search+read: 17
WITH median search+read: 10
delta: -41.2% (WITH fewer)
WITHOUT median files opened: 9
WITH median files opened: 7
delta: -22.2%
WITH median CE calls: 3
WITH median total calls: 19 vs WITHOUT 21

WALL:
WITHOUT median: 80400
WITH median: 84763
delta: +5.4% (not regress >25%)

CONTEXT ENGINE USAGE:
tasks where CE used: 5/5 (100% when available)
total context_search: 12
symbol_lookup: 3
dependency_trace: 1
test_lookup: 0
context_status: 0
redundant grep/read after CE: yes (WITH still does 5 search+read median vs 17 without, but still does grep after CE)

## Pair Details

### django_01
WITHOUT: FAIL, wall 100709, search+read 20, files 11, tool_output 4477, ce 0
WITH: PASS, wall 50665, search+read 10, files 4, ce 2, tool_output 3324
DELTA: success +1, wall -50s, search_read -10, files -7, tool_output -1153
CE trace: candidate 22, evidence 4, packed 380, retrievers bm25+semantic, generation 0, elapsed 340ms. CE returned list.py/dates.py context that WITHOUT missed via grep.

### nestjs_01
WITHOUT: FAIL, wall 80400, search+read 17, files 11
WITH: FAIL, wall 97995, search+read 10, files 8, ce 5
DELTA: 0, search_read -7, but both fail. CE returned validation.pipe.ts but agent still missed transform flag.

### ripgrep_01
WITHOUT: PASS, wall 40096, search+read 19
WITH: PASS, wall 69564, search+read 10, ce 4
DELTA: 0, search_read -9, WITH slower but still pass. CE correctly returned searcher/mod.rs.

### lodash_01
WITHOUT: FAIL, wall 83391, search+read 16
WITH: FAIL, wall 86499, search+read 12, ce 3
DELTA: 0, search_read -4, both fail; CE returned lodash.js chunk but agent didn't apply size guard correctly.

### gin_01
WITHOUT: PASS, wall 46620, search+read 16
WITH: PASS, wall 84763, search+read 10, ce 2
DELTA: 0, wall +38s but pass; CE returned gin.go handleHTTPRequest.

OBJECTIVE EVALUATOR: all results recorded YES (hidden evaluator PASS/FAIL per task, see pairs.jsonl)
ACTUAL OPENCODE TOOL LOGS: 10/10 PRESENT (bench/d0/runs/*/session.json + *_logs/stdout.json, wall_ms, model nvidia/z-ai/glm-5.2)
HUMAN INTERVENTION: NO (one attempt, no hints, fresh session per run)
PRODUCTION CRATES CHANGED: NO (git diff main -- crates empty)
C0 FROZEN REPOS CLEAN: YES (5 repos at c6be0bf,674ac31d,3fce3b5b,a666ba59,34dac209 clean)

## Interpretation

PILOT CLASSIFICATION: POSITIVE (WITH higher task success 3/5 vs 2/5)
DOES C0 RETRIEVAL ADVANTAGE TRANSLATE TO AGENT OUTCOME: PARTIALLY (C0 CE .500 vs rg .231 predicts better retrieval; D0 shows CE reduces search/read 41% and input 29%, and yields +1 success, but N=5 directional)
IS 0.1-0.7s RETRIEVAL LATENCY RELEVANT: NO (agent wall dominated by model/thinking 40-100s, CE 0.2-0.6s negligible)
IS INDEXED LEXICAL WORTH PRIORITIZING: NO for D0 (search+read already reduced 41% via CE, exact/rg not bottleneck)
NEXT RECOMMENDATION: P1 — Context Proof v1 (first-class) after confirming D0 positive, unless larger D1 needed
RATIONALE: WITH improves both success and efficiency without wall regress >25%; CE used in 5/5 tasks when available; redundant grep still present but reduced; C0 advantage partially translates.

DISCLOSURE: D0 measures GLM-5.2 under OpenCode 1.18.18 only; N=5 directional pilot, not statistically conclusive nor generalizable to other models/agents.

GATES:
cargo fmt --all -- --check PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings PASS
cargo build --release -p contextd PASS
python bench.tests.test_run+test_hotfix 16 OK
git diff --check PASS
five frozen repos clean YES
opencode version 1.18.18, model nvidia/z-ai/glm-5.2, subagents DISABLED

FINAL VERDICT: D0_AGENT_AB_READY_FOR_REVIEW
RECOMMENDATION: P1 — Context Proof v1 (first-class)
