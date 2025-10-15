"""Task manager for Context Engine sessions."""

from pathlib import Path
import os
from typing import Optional


def task_file_path() -> Path:
    """Return the path to the session task file."""
    return Path(".context/session_task.txt")


def _ensure_context_dir() -> None:
    """Ensure the .context directory exists."""
    context_dir = Path(".context")
    if not context_dir.exists():
        context_dir.mkdir(exist_ok=True)


def set_task(task: str) -> None:
    """Set the current task for the session.
    
    Args:
        task: Task description
    """
    _ensure_context_dir()
    
    task_file = task_file_path()
    task_file.write_text(task, encoding="utf-8")


def get_task() -> Optional[str]:
    """Get the current task for the session.
    
    Returns:
        Task description if set, None otherwise
    """
    task_file = task_file_path()
    
    if not task_file.exists():
        return None
        
    try:
        return task_file.read_text(encoding="utf-8").strip()
    except (IOError, OSError):
        return None


def clear_task() -> None:
    """Clear the current task for the session."""
    task_file = task_file_path()
    
    if task_file.exists():
        task_file.unlink()