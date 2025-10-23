# Context Engine

Context Engine is a hybrid Python/Node.js toolkit that records local development activity, generates AI-ready summaries, and packages project context for handoffs.

## Highlights

- **Automatic session tracking** using a background watchdog process that logs file edits and CLI commands.
- **Session management commands** (`start-session`, `stop-session`, `session save`, `session status`) exposed through a single Click CLI entry point.
- **AI-assisted summaries** with OpenRouter support and a deterministic fallback when no API key is provided.
- **Interactive chat palette** (Ink + React) that mirrors CLI commands and displays responses inline.
- **Cross-platform pipeline** validated on Ubuntu and Windows runners via GitHub Actions.

## Installation

```bash
# Install the Node.js CLI globally
npm install -g context-engine

# (Optional) install Python dependencies for local development
python -m pip install -r backend/requirements.txt
```

## Quick Start

```bash
# 1. Initialise project scaffolding (.context directory and session log)
context-engine init

# 2. Launch the tracker in the background
context-engine start-session --auto

# 3. Check tracker health
context-engine session status

# 4. Save a snapshot summary (static or AI-powered depending on configuration)
context-engine session save

# 5. Stop tracking once finished
context-engine stop-session

# 6. Generate a quick session recap for the terminal
context-engine summary -m ai

# 7. Create compressed/bundled context when handing off to another AI agent
context-engine compress
context-engine bundle

# 8. Launch the chat-enabled palette (optional)
context-engine chat
```

The Ink chat interface mirrors the palette, lets you run commands by typing `/command`, and streams responses from the backend.

## CLI Commands

| Command | Description |
|---------|-------------|
| `init` | Create `.context/` scaffolding and default configuration. |
| `start-session --auto` | Start the watchdog tracker in a background process. |
| `stop-session` | Terminate the tracker and write a stop marker to the log. |
| `session save` | Generate a markdown summary of `.context/session.md`. |
| `session status` | Report tracker PID, watched directories, and last events. |
| `summary -m ai` | Print a brief session recap (AI if available, static otherwise). |
| `compress` | Run LongCodeZip compression (optional workflow). |
| `bundle` | Produce a context bundle for downstream tooling. |
| `config show` | Display `.context/config.json` values. |

## Architecture Overview

```
Context-Engine/
+-- ui/               # Node + Ink frontend
¦   +-- index.js      # CLI entry and chat bootstrapper
¦   +-- components/   # Ink chat + palette components
¦   +-- lib/          # Backend bridge
+-- backend/          # Python backend
¦   +-- main.py       # CLI bridge entry
¦   +-- context_engine/
¦       +-- cli.py               # Click command surface
¦       +-- core/session_tracker.py  # Watchdog process manager
¦       +-- core/ai_summary.py       # AI/static summary helpers
+-- tests/            # Pytest-based smoke tests
```

Node commands spawn `backend/main.py`, which forwards invocations to `context_engine.cli`. Background tracking runs in a separate process whose PID is stored in `.context/session.pid`.

## Configuration

A minimal configuration is created automatically:

```json
{
  "model": "qwen-1.5-mini",
  "api_key": null
}
```

Set `OPENROUTER_API_KEY` or update `.context/config.json` to enable AI summaries. Without an API key the summariser falls back to a deterministic markdown summary.

## Testing

```
# Node unit tests
cd ui
npm test

# Python smoke tests
cd ..
python -m pytest -q
```

## Contributing

1. Fork the repository and create a feature branch.
2. Make your changes (ensure sessions remain cross-platform and ASCII-clean).
3. Add or update tests (`npm test`, `pytest -q`).
4. Submit a pull request.

## License

MIT. See `LICENSE` for details.

## Documentation

See docs/README.md for guide principles and per-topic documentation files.

