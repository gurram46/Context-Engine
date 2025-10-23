"""Start session command for Context Engine v1.2 Session Intelligence"""

import click
from datetime import datetime
from pathlib import Path

from context_engine.core.task_manager import set_task
from context_engine.ui import info, success, warn


@click.command("start-session")
@click.option("--task", help="Optional task description for session tracking")
@click.option("--background/--no-background", default=True, help="Enable background logging")
def start_session(task, background):
    """Start background logging of CLI and file activity"""
    from ..core import Config

    config = Config()

    # Initialize session file
    session_file = config.context_dir / "session.md"

    # Create session start marker
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    session_header = f"""# Session Started at {timestamp}

## Task
{task if task else 'No specific task defined'}

## Activity Log
---
"""

    # Write session header
    session_file.write_text(session_header, encoding='utf-8')

    # Set task in task manager if provided
    if task:
        set_task(task)
        info(f"Task set: {task}")

    success(f"Session started and logging to: {session_file.relative_to(config.project_root)}")

    if background:
        info("Background logging enabled - all CLI activity will be tracked")
        warn("Run 'context stop-session' to end the session gracefully")
    else:
        info("Session started without background logging")

    # Note: Actual background file monitoring would require additional implementation