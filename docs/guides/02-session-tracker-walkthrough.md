# Session Tracker Walkthrough

The tracker is implemented in ackend/context_engine/core/session_tracker.py. When you run context-engine start-session --auto, the CLI forks a background process that performs three jobs:

1. **File event monitoring** – watchdog.Observer watches the project root, backend, ui, tests, and common source directories. Events are coalesced by SessionFileHandler and appended to .context/session.md with timestamps (Edited, Created, Deleted, Moved).
2. **CLI logging** – every command invoked via the Python bridge (see ackend/main.py) calls log_cli_command, which records Ran: <command> -> <status> in the same log. This keeps the tracker authoritative even when the Ink chat or scripts invoke commands.
3. **State persistence** – .context/session_state.json stores the last command/event plus tracker metadata so context session status can respond without re-reading the entire markdown file.

### Lifecycle

| Stage | Code Path | Artifact |
|-------|-----------|----------|
| Start | start_session_tracker (cli.py) -> _tracker_worker | PID written to .context/session.pid; session header appended. |
| Stop  | stop_session_tracker (cli.py) | Sends SIGTERM, waits for process, writes [timestamp] Session tracker stopped. |
| Status | show_session_status | Prints PID, watch list, and last event/command by reading the state file. |

### Configuration knobs

- **Ignored patterns** – modify the IGNORED_PATTERNS list if you need to ignore additional directories (e.g., build artifacts). Currently excludes 
ode_modules, __pycache__, .git, etc.
- **Watched directories** – _collect_watch_dirs collects common folders; add custom directories by editing the list in session_tracker.py.
- **Manual logging** – call log_cli_command("custom note", "Success") from any script to append to the log without running a CLI command.

### Troubleshooting tips

If context session status reports “not running,” check:

- PID file exists but process is dead → stop-session will remove the stale PID; then restart with start-session --auto.
- watchdog missing → the CLI prints an error prompting you to pip install watchdog.
- No events recorded → ensure you are editing files inside the same project root; the handler records absolute paths relative to Path.cwd().
