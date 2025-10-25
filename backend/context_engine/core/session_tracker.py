"""Session tracking utilities for Context Engine.

Tracks filesystem activity and optional CLI commands to `.context/session.md`,
persists tracker state in `.context/session_state.json`, and manages a
background watchdog process whose PID is written to `.context/session.pid`.
"""

from __future__ import annotations

import json
import multiprocessing
import os
import signal
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional

try:
    from watchdog.events import FileSystemEventHandler  # type: ignore
    from watchdog.observers import Observer  # type: ignore
    WATCHDOG_AVAILABLE = True
except ImportError:  # pragma: no cover - optional dependency
    FileSystemEventHandler = object  # type: ignore
    Observer = None  # type: ignore
    WATCHDOG_AVAILABLE = False

from context_engine.ui import error, info, success, warn
from context_engine.core.config import get_config_file

SESSION_DIR = get_config_file().parent
SESSION_FILE = SESSION_DIR / "session.md"
PID_FILE = SESSION_DIR / "session.pid"
STATE_FILE = SESSION_DIR / "session_state.json"

IGNORED_PATTERNS = [
    "__pycache__",
    "node_modules",
    ".git",
    ".vscode",
    ".idea",
    ".pytest_cache",
    ".coverage",
    "htmlcov",
    ".context/session_summary.md",
    ".context/session_state.json",
    ".context/session.pid",
]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _ensure_context_dir() -> None:
    SESSION_DIR.mkdir(parents=True, exist_ok=True)


def _create_session_header() -> None:
    if SESSION_FILE.exists():
        return
    _ensure_context_dir()
    with SESSION_FILE.open("w", encoding="utf-8") as handle:
        handle.write("# Context Engine Session Log\n")
        handle.write(f"# Started: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n\n")


def _atomic_write_json(path: Path, data: Dict[str, Any]) -> None:
    """Atomically write data to a JSON file with Windows compatibility."""
    import time
    import random

    # Generate unique temporary filename
    random_id = f"{int(time.time())}_{random.randint(1000, 9999)}"
    tmp_path = path.with_suffix(f"{path.suffix}.tmp_{random_id}")

    try:
        with tmp_path.open("w", encoding="utf-8") as handle:
            json.dump(data, handle, indent=2)

        # Windows-specific file replacement with retry logic
        max_retries = 5
        for attempt in range(max_retries):
            try:
                tmp_path.replace(path)
                return
            except PermissionError as e:
                if attempt < max_retries - 1:
                    time.sleep(0.1)  # Brief delay before retry
                    continue
                else:
                    # As a last resort, write directly without atomic operation
                    with path.open("w", encoding="utf-8") as handle:
                        json.dump(data, handle, indent=2)
                    return
    finally:
        # Clean up temporary file if it exists
        if tmp_path.exists():
            try:
                tmp_path.unlink()
            except:
                pass  # Ignore cleanup errors


def _read_state() -> Dict[str, Any]:
    if not STATE_FILE.exists():
        return {}
    try:
        with STATE_FILE.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except json.JSONDecodeError:
        return {}


def _update_state(updates: Dict[str, Any]) -> None:
    state = _read_state()
    for key, value in updates.items():
        if isinstance(value, dict):
            node = state.setdefault(key, {})
            if isinstance(node, dict):
                node.update(value)
            else:
                state[key] = value
        else:
            state[key] = value
    _atomic_write_json(STATE_FILE, state)


def _append_session_line(line: str) -> None:
    _ensure_context_dir()
    with SESSION_FILE.open("a", encoding="utf-8") as handle:
        handle.write(f"{line}\n")
        handle.flush()


def _normalize_path(path: str) -> str:
    return os.path.abspath(path)


def _pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        # `os.kill(pid, 0)` checks for the existence of the process.
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError:
        return False
    else:
        return True


def _write_pid(pid: int) -> None:
    _ensure_context_dir()
    PID_FILE.write_text(str(pid), encoding="utf-8")


def _clear_pid_file() -> None:
    try:
        PID_FILE.unlink()
    except FileNotFoundError:
        pass


def _collect_watch_dirs(root: Path) -> List[str]:
    candidates = [
        root,
        root / "backend",
        root / "ui",
        root / "src",
        root / "lib",
        root / "tests",
    ]
    return [
        _normalize_path(str(path))
        for path in candidates
        if path.exists()
    ]


# ---------------------------------------------------------------------------
# Watchdog event handler
# ---------------------------------------------------------------------------

class SessionFileHandler(FileSystemEventHandler):
    """Append filesystem events to the session log with simple debouncing."""

    def __init__(self, project_root: str):
        self.project_root = Path(project_root)
        self._pending: Dict[str, Dict[str, Any]] = {}
        self._last_flush: float = 0.0

    def _should_ignore(self, path: str) -> bool:
        lowered = path.lower()
        return any(pattern in lowered for pattern in IGNORED_PATTERNS)

    def _queue_event(self, event_type: str, src_path: str, dest_path: Optional[str] = None) -> None:
        if self._should_ignore(src_path):
            return
        key = f"{event_type}:{src_path}:{dest_path or ''}"
        self._pending[key] = {
            "type": event_type,
            "src": src_path,
            "dest": dest_path,
            "timestamp": datetime.now().strftime("%H:%M:%S"),
        }
        now = time.time()
        if now - self._last_flush > 2:
            self._flush()

    def _flush(self) -> None:
        if not self._pending:
            return
        events = list(self._pending.values())
        self._pending.clear()
        self._last_flush = time.time()
        for event in events:
            try:
                rel_src = Path(event["src"]).relative_to(self.project_root)
            except ValueError:
                rel_src = Path(event["src"]).name
            message = ""
            if event["type"] == "modified":
                message = f"Edited: {rel_src}"
            elif event["type"] == "created":
                message = f"Created: {rel_src}"
            elif event["type"] == "deleted":
                message = f"Deleted: {rel_src}"
            elif event["type"] == "moved" and event["dest"]:
                try:
                    rel_dest = Path(event["dest"]).relative_to(self.project_root)
                except ValueError:
                    rel_dest = Path(event["dest"]).name
                message = f"Moved: {rel_src} -> {rel_dest}"
            else:
                continue
            _append_session_line(f"[{event['timestamp']}] {message}")
            _update_state({
                "last_event": {
                    "timestamp": event["timestamp"],
                    "detail": message,
                }
            })

    # Watchdog callbacks --------------------------------------------------
    def on_modified(self, event):
        if not event.is_directory:
            self._queue_event("modified", event.src_path)

    def on_created(self, event):
        if not event.is_directory:
            self._queue_event("created", event.src_path)

    def on_deleted(self, event):
        if not event.is_directory:
            self._queue_event("deleted", event.src_path)

    def on_moved(self, event):
        if not event.is_directory:
            self._queue_event("moved", event.src_path, event.dest_path)


# ---------------------------------------------------------------------------
# Tracker worker
# ---------------------------------------------------------------------------

def _tracker_worker(project_root: str, watch_dirs: Iterable[str]) -> None:
    if not WATCHDOG_AVAILABLE:
        return
    project_path = Path(project_root)
    handler = SessionFileHandler(project_root)
    observer = Observer()

    stop_flag = multiprocessing.Event()

    def _stop(signum, frame):
        stop_flag.set()

    signal.signal(signal.SIGTERM, _stop)
    signal.signal(signal.SIGINT, _stop)

    for directory in watch_dirs:
        observer.schedule(handler, directory, recursive=True)

    observer.start()
    _append_session_line(f"[{datetime.now().strftime('%H:%M:%S')}] Session tracker started")
    _update_state({
        "status": "running",
        "started_at": datetime.now().isoformat(),
        "watching": list(watch_dirs),
        "last_event": None,
    })

    try:
        while not stop_flag.is_set():
            handler._flush()
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        observer.stop()
        observer.join()
        handler._flush()
        _append_session_line(f"[{datetime.now().strftime('%H:%M:%S')}] Session tracker stopped")
        _update_state({
            "status": "stopped",
            "stopped_at": datetime.now().isoformat(),
        })


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def start_session_tracker(auto: bool = False) -> None:
    """Launch the background tracker process."""
    if _pid_alive(_read_pid()):
        warn("Session tracking is already running")
        return

    if not auto:
        warn("Automatic logging requires the --auto flag.")
        return

    if not WATCHDOG_AVAILABLE:
        error("watchdog is not installed. Install with `pip install watchdog`.")
        return

    _create_session_header()
    project_root = _normalize_path(str(Path.cwd()))
    watch_dirs = _collect_watch_dirs(Path(project_root))

    process = multiprocessing.Process(
        target=_tracker_worker,
        args=(project_root, watch_dirs),
        daemon=False,
    )
    process.start()
    _write_pid(process.pid)

    success("Session tracker started in background")
    info(f"PID: {process.pid}")
    info("Tracking file changes and CLI commands")


def _read_pid() -> int:
    if not PID_FILE.exists():
        return -1
    try:
        return int(PID_FILE.read_text(encoding="utf-8").strip())
    except ValueError:
        return -1


def stop_session_tracker() -> None:
    """Terminate the tracker process and mark the session as stopped."""
    pid = _read_pid()
    if pid == -1 or not _pid_alive(pid):
        warn("No active session tracker was found")
        _clear_pid_file()
        return

    try:
        os.kill(pid, signal.SIGTERM)
    except OSError as exc:
        error(f"Failed to stop tracker (PID {pid}): {exc}")
        return

    # Wait briefly for process to exit
    for _ in range(10):
        if not _pid_alive(pid):
            break
        time.sleep(0.5)

    if _pid_alive(pid):
        warn("Tracker did not stop gracefully; terminating.")
        try:
            os.kill(pid, signal.SIGKILL if hasattr(signal, "SIGKILL") else signal.SIGTERM)
        except OSError:
            pass

    _clear_pid_file()
    success("Session tracker stopped")


def show_session_status() -> None:
    """Print tracker status information for `context session status`."""
    pid = _read_pid()
    state = _read_state()
    if pid == -1 or not _pid_alive(pid):
        print("Session tracking is not running.")
        if state.get("stopped_at"):
            print(f"Last stopped: {state['stopped_at']}")
        return

    print(f"Session tracking active (PID {pid})")
    watching = state.get("watching") or []
    if watching:
        print("Watching directories:")
        for path in watching:
            print(f"  - {path}")
    last_event = state.get("last_event")
    if last_event:
        print(f"Last event at {last_event.get('timestamp')}: {last_event.get('detail')}")
    last_command = state.get("last_command")
    if last_command:
        print(f"Last command at {last_command.get('timestamp')}: {last_command.get('command')} ({last_command.get('status')})")


def log_cli_command(command: str, result: str = "") -> None:
    """Append a CLI command entry to the session log."""
    _create_session_header()
    timestamp = datetime.now().strftime("%H:%M:%S")
    status = "Success" if result.strip() == "" else result.strip()
    line = f"[{timestamp}] Ran: {command} -> {status}"
    _append_session_line(line)
    _update_state({
        "last_command": {
            "timestamp": timestamp,
            "command": command,
            "status": status,
        }
    })


def is_session_active() -> bool:
    """Return True if a tracker PID file exists and the process is alive."""
    pid = _read_pid()
    return _pid_alive(pid)
