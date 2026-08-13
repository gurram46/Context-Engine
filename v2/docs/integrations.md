# Context Engine V2 — MCP Integrations

> **Transport:** STDIO only for V0. One `node` process, one OCI child, many queries.
> Built artifact: `v2/dist/mcp/server.js` (Node 20.19.5).

## Build

```bash
npm --prefix v2 install --ignore-scripts --no-workspaces
npm --prefix v2 run build   # -> v2/dist/
node v2/dist/mcp/server.js  # STDIO MCP server
```

## Tools (5)

* `context_search {query, budgetTokens?, maxResults?, debug?}`
* `symbol_lookup {symbol, budgetTokens?, debug?}`
* `dependency_trace {symbol, direction: "callers"|"callees"|"both", budgetTokens?, debug?}`
* `test_lookup {query, budgetTokens?, debug?}`
* `context_status {}`

All tools call the same `ContextEngine` core (`v2/src/core/contextEngine.ts`) — CLI and MCP are adapters.

### Output (debug=false)

```json
{
  "query": "...",
  "type": "CONCEPTUAL",
  "context": "# Evidence Pack ...",
  "evidence": [{ "file": "backend/.../utils.py", "lines": "77-88", "symbol": "redact_secrets", "relation": "definition", "source": "symbol" }],
  "stats": { "retrievers": ["symbol:1"], "elapsedMs": 320, "tokenEstimate": 1200, "warnings": [] },
  "warnings": []
}
```

With `debug:true` adds `debug: {rawEvidenceCount, timings, decisions, authorityWeights}` and per-evidence `provenance, score, authorityScore, finalScore, authorityReasons`.

## OpenCode

OpenCode has two mechanisms:

* **Native plugin** (existing): `opencode.json -> plugin: ["open-codebase-index"]` for the upstream `open-codebase-index` indexers. Keep it.
* **Our Context Engine MCP** (new): separate `mcp` entry. Verified against `open-codebase-index` skill docs for `opencode.json` shape; the `mcp` key is the generic MCP client config used by OpenCode/Codex/Claude.

Add to `opencode.json` (project root or `~/.config/opencode/opencode.json` for global):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": ["open-codebase-index"],
  "mcp": {
    "context-engine-v2": {
      "type": "local",
      "command": ["node", "C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js"],
      "enabled": true
    }
  }
}
```

*Use absolute path or `${workspaceFolder}/v2/dist/mcp/server.js` if supported. The `type: "local"` + `command` shape is the same as Codex/Claude local STDIO.*

Verify: `opencode mcp list` or check `opencode` logs for `context-engine-v2` tools.

## Codex

Codex MCP config is typically `~/.codex/config.toml` or `.codex/config.toml`:

```toml
[mcp_servers.context-engine-v2]
command = "node"
args = ["C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js"]
enabled = true
```

*UNVERIFIED: Codex MCP TOML shape varies by version. If your Codex uses `codex mcp add` CLI, use:*

```bash
codex mcp add context-engine-v2 -- node C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js
```

## Claude Code

Claude Code MCP is configured via `claude mcp add` or `.mcp.json`:

```bash
claude mcp add context-engine-v2 -- node C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js
```

Or `.mcp.json` in project root:

```json
{
  "mcpServers": {
    "context-engine-v2": {
      "command": "node",
      "args": ["C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js"]
    }
  }
}
```

*UNVERIFIED: Claude's `.mcp.json` schema may be `mcpServers` vs `mcp` key. Use `claude mcp list` to verify.*

## Zed

Zed `settings.json`:

```json
{
  "context_servers": {
    "context-engine-v2": {
      "command": "node",
      "args": ["C:/Users/Dell/context/Context-Engine/v2/dist/mcp/server.js"]
    }
  }
}
```

*UNVERIFIED: Zed's MCP key is `context_servers` per Zed docs, but verify via Zed settings UI.*

## Notes

* **Node 20.19.5 required** for `open-codebase-index` native module. Use `nvm use 20.19.5`.
* **One OCI child**: `ContextEngine` keeps one `open-codebase-index-mcp` child; do not spawn per query.
* **Freshness**: `rg` is immediate (~0.01s), OCI incremental ~3-4s for new chunk, ~18s for delete GC. `context_status` reports `ociChunks` and `warnings`.
* **No `tsx` for agents**: Use built `dist/mcp/server.js` with `node`, not `tsx src/mcp/server.ts`.
