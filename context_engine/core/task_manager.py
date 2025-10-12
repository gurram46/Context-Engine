"""Task management for Context Engine sessions."""

import os
from pathlib import Path
from typing import Optional


class TaskManager:
    """Manages task state for Context Engine sessions."""
    
    def __init__(self, project_root: Optional[Path] = None):
        """Initialize task manager with project root."""
        self.project_root = project_root or Path.cwd().resolve()
        self.context_dir = self.project_root / ".context"
        self.task_file_path = self.context_dir / "session_task.txt"
    
    def set_task(self, task: str) -> None:
        """Set the current task."""
        # Create context directory if it doesn't exist
        self.context_dir.mkdir(exist_ok=True)
        
        # Write task to file with UTF-8 encoding
        self.task_file_path.write_text(task, encoding="utf-8")
    
    def get_task(self) -> Optional[str]:
        """Get the current task, or None if not set."""
        if not self.task_file_path.exists():
            return None
        
        try:
            return self.task_file_path.read_text(encoding="utf-8").strip()
        except (UnicodeDecodeError, IOError):
            return None
    
    def clear_task(self) -> None:
        """Clear the current task."""
        if self.task_file_path.exists():
            self.task_file_path.unlink()


# Global instance for convenience
_task_manager = TaskManager()


def set_task(task: str) -> None:
    """Set the current task."""
    _task_manager.set_task(task)


def get_task() -> Optional[str]:
    """Get the current task, or None if not set."""
    return _task_manager.get_task()


def clear_task() -> None:
    """Clear the current task."""
    _task_manager.clear_task()