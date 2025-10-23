# Quickstart Guide

This walkthrough uses the real CLI surfaces to bootstrap a working Context Engine project and highlights which files are created or mutated at each step. You can run these commands on a blank directory; they match the assertions in 	ests/test_cli_basic.py.

`ash
# 1. Initialise the workspace
context-engine init
# Files: .context/session.md, .context/config.json, .context/README.md

# 2. Start the session tracker (background watchdog)
context-engine start-session --auto
# Files: .context/session.pid, .context/session_state.json, session.md header updated

# 3. Inspect health
context-engine session status
# Output: PID, watched directories, last event/command from session_state.json

# 4. Capture a summary snapshot (generates static summary if no API key)
context-engine session save "Wrapped up dashboard wiring"
# Files: .context/session_summary.md, session log gains note + summary marker

# 5. Stop tracking when the session ends
context-engine stop-session
`

### Where everything lives

| File | Produced by | Purpose |
|------|-------------|---------|
| .context/session.md | init, tracker | Canonical log of file events and CLI commands. |
| .context/session_summary.md | session save | Markdown summary written by ackend/context_engine/core/ai_summary.py. |
| .context/session.pid | tracker process | PID of the watchdog started in another process. |
| .context/session_state.json | tracker process | Lightweight cache used by session status for fast reporting. |

### Optional: launch the chat palette

The Ink UI mirrors the same commands via the Node bridge:

`ash
context-engine chat
`

Inside the chat panel, try typing /summary -m ai or /bundle. These map to the same Click commands defined in ackend/context_engine/cli.py and are executed through ui/lib/backend-bridge.js.
