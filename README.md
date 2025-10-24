# Context Engine

Context Engine is a hybrid CLI that tracks development sessions, generates summaries, and bundles project context for AI handoffs. The tool ships as two packages:

- **npm**: [context-engine-cli](https://www.npmjs.com/package/context-engine-cli)
- **PyPI**: [context-engine-dev](https://pypi.org/project/context-engine-dev/)

After installation the Ink-based CLI launches the Python backend automatically, so a single install provides both halves.

## Installation

### npm (recommended)
```bash
npm install -g context-engine-cli@1.2.1-2
```
This installs the Node/Ink CLI, bundles the Python backend, and runs `pip install -r backend/requirements.txt` during postinstall (requires Python 3.8+ on PATH).

### PyPI
```bash
pip install context-engine-dev==1.2.1
```
This provides the Python modules and console scripts. Pair it with the npm package if you prefer to manage the frontend separately.

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
`-- docs/                   # Authoring guides for contributors
```

## Development Workflow

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
