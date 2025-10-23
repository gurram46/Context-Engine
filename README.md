# Context Engine

Context Engine is a hybrid CLI that tracks local development sessions, generates summaries, and bundles context for AI handoffs. The CLI is split into a Python backend (Click + watchdog tracker) and a Node/Ink frontend (interactive chat + command palette).

## Key Features

- Background session tracker that logs filesystem events and CLI commands to .context/session.md.
- context session save generates Markdown summaries (AI-assisted when an OpenRouter key is available, static otherwise).
- Ink chat interface mirrors all CLI commands (/start-session, /session status, /bundle, etc.).
- Ready-to-publish npm package (context-engine-dev) and Python package (context-engine).

## Quick Start

`ash
# Initialise scaffolding
context-engine init

# Start tracker in background
context-engine start-session --auto

# Inspect tracker status
context-engine session status

# Capture a summary snapshot
context-engine session save "Wrapped up dashboard wiring"

# Stop tracking
context-engine stop-session

# Launch chat palette
context-engine chat
`

Files created in .context/:

| File | Purpose |
|------|---------|
| session.md | Log of file events and CLI commands. |
| session_summary.md | AI/static summary written by session save. |
| session.pid | PID of watchdog process. |
| session_state.json | Cache used by context session status. |

## Project Structure

`
Context-Engine/
├── backend/                # Python package
│   ├── main.py             # CLI bridge invoked by Node
│   └── context_engine/
│       ├── cli.py          # Click command definitions
│       ├── core/session_tracker.py
│       ├── core/ai_summary.py
│       └── commands/       # Command modules (baseline, bundle, session, etc.)
├── ui/                     # Node + Ink frontend
│   ├── index.js            # CLI entry and palette bootstrapper
│   ├── components/ChatApp.tsx
│   └── lib/backend-bridge.js
└── docs/                   # Authoring guides for contributors
`

## Development Workflow

`ash
# Install deps and run lint/tests (Node)
cd ui
npm install
npm test
npm run lint

# Run Python tests
cd ..
python -m pytest -q
`

## Publishing

1. Bump versions:
   `ash
   cd ui
   npm version <new-version> --no-git-tag-version
   cd ..
   python scripts/sync_versions.py <new-version>
   `
2. Commit, tag, and push:
   `ash
   git add .
   git commit -m "chore: release <new-version>"
   git tag v<new-version>
   git push origin main
   git push origin v<new-version>
   `
3. Publish packages:
   `ash
   cd ui
   npm publish --access public
   cd ..
   python -m build
   twine upload dist/*
   `

## Documentation

See docs/README.md for writing principles and deep-dive guides.

## License

MIT
