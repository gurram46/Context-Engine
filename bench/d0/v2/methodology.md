# D0-v2 Methodology

## Arms
- A WITHOUT: opencode normal tools (read, grep, glob, shell, edit, test), CE MCP disabled via --pure, no contextd, no .context
- B WITH: same + persistent Context Engine MCP (context_search,symbol_lookup,dependency_trace,test_lookup,context_status), E3 hot runtime, real index before wall

Both arms use identical neutral prompt: "If repository-intelligence tools are available in this environment, you may use them when useful."

## Tasks
5 tasks ×2 arms =10 scored sessions. One per repo (django, nestjs, ripgrep, lodash, gin) at frozen SHAs, each with single realistic human-like regression (see tasks/manifest.json). Frozen before any scored run. No task edits after first scored run. Worktrees isolated, mutation patch hash verified, source_tree_hash identity required before WITH .context.

## Indexing
For WITH only, Context Engine indexed before timed session (index wall measured separately, not in agent wall). Mutation included in current generation, semantic ready all-minilm 384d v2 missing 0 required. If not ready, arm BLOCKED (not lexical-only).

## Order
Counterbalanced via seed 20260819 (same as v1): django A_first, nestjs B_first, ripgrep B_first, lodash A_first, gin A_first. Recorded before execution.

## Limits
Each session max 20min wall, fresh session, no reuse, no human hints, one attempt. Infrastructure failure reruns both arms. Stop on completion or timeout. Capture final diff immediately.

## Logs
Every run preserves raw opencode JSONL transcript, tool-call log, timestamps, model id, usage, shell/test commands, files read/edited, final response, git diff. CE trace extracted from real logs only.

## Token Accounting
- Provider input/output/cache tokens where exposed else N/A (not invented)
- Tool-output tokens via Python tiktoken cl100k_base over actual textual tool outputs delivered to model (not chars/4). Fail setup if tiktoken unavailable.
- Both metrics kept separately, not conflated as "tokens saved".

## Tool Classification
Parse actual OpenCode JSON event structure (type, part.tool, part.tokens). Count exact tool names: read, grep/search, glob, bash/shell, edit/write, context_search, symbol_lookup, dependency_trace, test_lookup, context_status. Define native_repository_lookup_calls = read + grep + glob deterministically. Bash repository-discovery only counted if command matches deterministic patterns (grep, find, git). Persist per-run normalized tool events.

## Success
Primary: behavioral hidden evaluator PASS/FAIL (pytest, jest, cargo test, node, go test) on actual worktree, not source-string grep. Secondary: wall, tokens, tool calls, files opened, CE usage.

## Interpretation
Pilot N=5 directional, not statistically conclusive. Pre-registered thresholds for POSITIVE/NEUTRAL/NEGATIVE as in v1.

## Integrity
- No MUTATED/BENCHMARK markers
- No source-string evaluators
- No public prompt leaks of file/function/exact replacement/hidden test
- Pair source_tree_hash identity required
- CE smoke proves tool visibility before scored runs
- WITH semantic precondition enforced
- D0-v1 preserved, not mixed
