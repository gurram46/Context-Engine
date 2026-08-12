# CLI vs MCP Integration Overhead (R5G)

Same repository: `Context-Engine` (207 files, 1177 symbols, 331 BM25 docs)
Same model: `all-minilm` via Ollama (or unavailable fallback)
Same question: `Where is count_tokens implemented?` and `Where is secret redaction implemented?`

## Permanent overhead

| Integration | Tokens (tiktoken cl100k) | Notes |
|-------------|--------------------------|-------|
| **A: MCP** — 5 tool definitions (`tools/list`) | ~1450 tokens (5 × ~290) | Loaded into model context per session (tool schema) |
| **B: Skill + CLI** — `skills/context-engine/SKILL.md` | ~220 tokens | One-time skill body, no tool schema |

*Skill is ~6× smaller than MCP tool schema (measured via `tiktoken::count_tokens` on `stdout_mcp2.txt` tools array vs `SKILL.md`).*

## Per-query overhead

| Metric | MCP `context_search` | CLI `contextd search --json` |
|--------|----------------------|------------------------------|
| Agent-visible tool calls | 1 (MCP) | 1 (shell) |
| Packed context tokens | ~850 (SYMBOL) | ~850 (SYMBOL) — identical pipeline |
| Files returned | 3–5 | 3–5 |
| Elapsed (warm, debug) | ~2.4s (includes MCP handshake 30ms) | ~2.3s (CLI startup 2.1s + retrieve 0.2s) |
| Top1 file | `backend/context_engine/core/utils.py` | `backend/context_engine/core/utils.py` (equivalent) |
| Retrievers used | `rust-exact, rust-symbol, rust-bm25:skipped, rust-semantic:skipped` | same |

*Measurement via `.\target\debug\contextd.exe search --json` vs `context_search` tool call through `.\target\debug\contextd.exe mcp` with same query. Both use identical `ContextService::search` → `retrieve_context` → `classify/build_plan → exact/structural/BM25/vector → authority/fuse/pack`. CLI/MCP result equivalence verified (evidence ordering equal, top1 identical for frozen queries).*

## Terminology

- **integration overhead / tool-definition tokens:** tokens for tool schemas (MCP) or skill body (CLI)
- **packed context tokens / repository-context tokens:** tokens in `packed.markdown` returned to model
- **agent-visible tool calls:** number of tool invocations the model sees

## Recommendation

**Preferred: Skill + CLI** — clearly cheaper integration overhead (~220 vs ~1450 tokens), equally reliable (same native service, 12/12 equivalence), simpler operational model (no long-lived MCP child for one-shot queries). MCP remains supported for interoperability (optional adapter, 5 tools only, stdout pure).

*No "tokens saved" marketing claims; actual product token-savings belong in later agent A/B benchmark with real Codex/OpenCode loops.*
