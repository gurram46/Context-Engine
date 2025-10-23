"""Stop session command for Context Engine v1.2 Session Intelligence"""

import click
from datetime import datetime

from context_engine.core.task_manager import get_task, clear_task

from context_engine.ui import info, success, warn


@click.command("stop-session")
@click.option("--summarize/--no-summarize", default=True, help="Generate summary on session end")
def stop_session(summarize):
    """Stop current session gracefully"""
    from ..core import Config

    config = Config()
    session_file = config.context_dir / "session.md"

    # Check if session exists
    if not session_file.exists():
        warn("No active session found")
        return

    # Get current task
    current_task = get_task()
    if current_task:
        info(f"Stopping session for task: {current_task}")

    # Add session end marker
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    session_footer = f"\n---\n### Session ended at {timestamp}\n"

    # Append end marker to session file
    with open(session_file, "a", encoding="utf-8") as f:
        f.write(session_footer)

    success(f"Session stopped at {timestamp}")

    # Clear task from task manager
    clear_task()

    # Optionally trigger session save with summarization
    if summarize:
        info("Run 'context session save' to generate AI-powered summary of session activity")
        warn("Session logging stopped but session file remains for summarization")

    # Note: Would integrate with session save command for automatic summarization