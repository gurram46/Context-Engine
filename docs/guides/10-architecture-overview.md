# Architecture Overview

Context Engine combines a Python backend (Click CLI + tracking) with a Node/Ink frontend (interactive chat). The high-level layout:

`
Context-Engine/
├── backend/
│   ├── main.py                    # CLI bridge invoked by the Node layer
│   └── context_engine/
│       ├── cli.py                 # Click command definitions
│       ├── core/session_tracker.py
│       ├── core/ai_summary.py
│       └── commands/              # Command modules (baseline, bundle, etc.)
└── ui/
    ├── index.js                   # Node CLI + Ink chat shell
    ├── components/ChatApp.tsx     # Chat UI
    └── lib/backend-bridge.js      # Spawns backend/main.py
`

Call flow for a command (context baseline list):

1. User runs command (direct CLI or chat).
2. Node CLI (ui/index.js) calls the backend bridge.
3. Bridge spawns ackend/main.py baseline list.
4. ackend/main.py forwards to Click (ackend/context_engine/cli.py).
5. Click invokes aseline_commands.list, which reads from .context/baseline/.

Understanding this structure helps you locate the right module when extending functionality.
