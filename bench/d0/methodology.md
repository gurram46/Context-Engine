# D0 Methodology

## Arms
- A WITHOUT: opencode normal tools (read, grep, glob, shell, edit, test), CE MCP disabled
- B WITH: same + persistent Context Engine MCP (context_search, symbol_lookup, dependency_trace, test_lookup, context_status), E3 hot runtime

Both arms use identical neutral prompt: "If repository-intelligence tools are available in this environment, you may use them when useful."

## Tasks
5 tasks × 2 arms =10 sessions. One per repo (django, nestjs, ripgrep, lodash, gin) at frozen SHAs, each with single realistic bug mutation (see tasks/manifest.json). Frozen before any A/B run. After first run, no task edits. Worktrees isolated, mutation patch hash verified equal before run, .context not visible to A.

## Indexing
For WITH only, Context Engine indexed before timed session (initial index time/disk/RSS recorded separately, not in hot wall). Mutation reconciled, semantic ready, all-minilm 384d v2, missing_vectors 0, generation clean.

## Order
Counterbalanced via seed 20260819: django A_first, nestjs B_first, ripgrep B_first, lodash A_first, gin A_first. Recorded before execution.

## Limits
Each session max 20min wall, fresh session, no reuse, no human hints, one attempt. Infrastructure failure reruns both arms. Stop on agent declares completion or timeout. Capture final diff immediately.

## Logs
Every run preserves raw opencode transcript (format json), tool-call log, timestamps, model id, usage, shell/test commands, files read/edited, final response, git diff. CE internal trace where available.

## Token Accounting
Provider input/output/cache tokens where exposed else N/A; plus tool-output cl100k tokens and CE packed_tokens.

## Success
Primary: hidden evaluator PASS/FAIL + target regression PASS/FAIL. Secondary: wall, tokens, tool calls, files opened, CE usage.

## Interpretation
Pilot N=5 directional, not statistically conclusive. Pre-registered thresholds for POSITIVE/NEUTRAL/NEGATIVE.
