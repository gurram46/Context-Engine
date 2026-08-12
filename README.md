# Context Engine

> **Context Engine retrieves context. It does not run the coding agent.**
> Local repository-intelligence/context connector for Codex / OpenCode / Claude Code / Zed.

> **Status:** **R5** `contextd` native connector **implemented** (`rust/contextd-r4` → `806551a` + R5). One native Rust service (`context-core/index/rank`) serving both CLI and MCP adapters. **R4** Rust native retrieval (**BM25**, **vector**, **watcher**, **incremental graph**) **implemented** (`context-index::bm25` + `vector` + `watcher` + `embed`, 12/12 Top1, 6/6 R2, 15/15 R3). **R5** adds native CLI (`contextd search/symbol/dependency/tests/status`), skills-first integration, MCP as optional adapter, Node/V2/OCI removed from production runtime. See `docs/architecture/contextd.md`.

**Boundary:** Context Engine owns repository state, indexing, retrieval, ranking, dependency/test context, packing. The agent owns reasoning, editing, shell, compilation, tests, planning. Protect this boundary throughout R5.

```
Codex/OpenCode/Claude ── Skill → contextd CLI ──┐
                                                ├── Context Engine (exact/structure/BM25/semantic → rank/fuse/pack → compact evidence)
                        MCP → contextd MCP ─────┘
```

Context Engine is a hybrid CLI that tracks development sessions, generates summaries, and bundles project context for AI handoffs. The tool ships as two packages:

- **npm**: [context-engine-cli](https://www.npmjs.com/package/context-engine-cli)
- **PyPI**: [context-engine-dev](https://pypi.org/project/context-engine-dev/)

After installation the Ink-based CLI launches the Python backend automatically, so a single install provides both halves.

## Installation — R5 `contextd` (Rust, Windows first)

Production `contextd` requires **no Node, npm, V2, OCI** — single binary + local model.

```bash
cargo build --release              # -> target/release/contextd.exe (Windows)
# or download release artifact
contextd --version                 # 0.1.0
contextd status --json
contextd search "Where is count_tokens implemented?" --json
contextd mcp                       # stdio MCP server (5 tools)
```

Skill (preferred):
```bash
bash skills/context-engine/install.sh --all   # installs to Codex/OpenCode/Claude
# then in agent:
contextd search "<natural language repository question>" --json
```

Legacy npm/PyPI installs remain for historical Python backend but are NOT required for R5 retrieval.

## Installation — Legacy (npm/PyPI, pre-R5)

### npm (recommended)
```bash
npm install -g context-engine-cli@1.2.1-2
```

### PyPI
```bash
pip install context-engine-dev==1.2.1
```

## Quick Start
```bash
# Initialise scaffolding (.context/ directory, default config)
context-engine init

# Start the session tracker in the background
context-engine start-session --auto

# Inspect tracker status
context-engine session status

# Capture a summary snapshot (AI if configured, static otherwise)
context-engine session save "Wrapped up dashboard wiring"

# Stop tracking when finished
context-engine stop-session

# Launch the interactive chat palette
context-engine chat
```
During a session the tracker writes to `.context/`:

| File | Purpose |
|------|---------|
| session.md | Log of file events and CLI commands. |
| session_summary.md | Markdown summary produced by `context-engine session save`. |
| session.pid | PID of the watchdog process. |
| session_state.json | Cache for rapid `context-engine session status` responses. |

## Project Structure
```text
Context-Engine/
|-- backend/                # Python package
|   |-- main.py             # CLI bridge invoked by Node
|   `-- context_engine/
|       |-- cli.py          # Click command definitions
|       |-- core/session_tracker.py
|       |-- core/ai_summary.py
|       `-- commands/       # Command modules (baseline, bundle, session, etc.)
|-- ui/                     # Node + Ink frontend
|   |-- index.js            # CLI entry and palette bootstrapper
|   |-- components/ChatApp.tsx
|   `-- lib/backend-bridge.js
|-- v2/                     # TypeScript behavioral reference (15b053e) — 41 tests, 5 MCP tools
|   |-- src/core/contextEngine.ts
|   |-- src/mcp/server.ts
|   `-- eval/retrieval-cases.json
|-- crates/
|   |-- contextd/           # Rust MCP shell (R0) — stdio + V2 bridge
|   `-- context-core/       # shared MCP types
`-- docs/                   # guides + architecture + audit
```

## Development Workflow

### Rust backend (contextd) — R0
```bash
cargo build --release              # -> target/release/contextd.exe
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
# MCP smoke (Node harness)
node v2/test_contextd_smoke.js
```
`contextd` requires `node` + `v2/dist/mcp/server.js` (`npm run build --prefix v2`). Configure one client:
`mcp.contextd.command = ["C:/path/to/target/release/contextd.exe"]` with `CONTEXT_ENGINE_PROJECT_ROOT`.

### Frontend (Node) tests & lint
```bash
npm install --prefix ui
npm test --prefix ui
npm run lint --prefix ui
```
Run the install command when dependencies change. Alternatively `cd ui` first and omit `--prefix`.

### Backend (Python) tests
```bash
python -m pytest -q
```
Execute from the repository root; there is no separate `scripts/run_test` helper.

### V2 behavioral reference
```bash
.\v2\node_modules\.bin\vitest run --config v2/vitest.config.ts  # 41 tests
.\v2\node_modules\.bin\tsx v2/eval/runner.ts                    # 5/5 Top1
```

## Publishing

1. Bump versions
   ```bash
   cd ui
   npm version <new-version> --no-git-tag-version
   cd ..
   python scripts/sync_versions.py <new-version>
   npm install --prefix ui          # refresh lockfile
   ```
2. Commit, tag, and push
   ```bash
   git add .
   git commit -m "chore: release <new-version>"
   git tag v<new-version>
   git push origin main
   git push origin v<new-version>
   ```
3. Publish packages
   ```bash
   cd ui
   npm publish --access public
   cd ..
   python -m build
   twine upload dist/*
   ```

## Documentation

Guides explaining the codebase live in `docs/`. Start with `docs/README.md` for the index and authoring principles.

## License

MIT
