"""Session management commands"""

from datetime import datetime

import click

from ..ui import success, info
from ..core import Config
from ..models import OpenRouterClient
from .bundle_command import bundle


@click.command()
@click.argument("note")
def save(note):
    """Save a note to the current session"""
    config = Config()

    # Ensure session file exists
    config.session_file.touch()

    # Sanitize and limit note size
    from ..core.utils import sanitize_note_input

    max_len = int(config.get("note_max_length", 2000))
    safe_note = sanitize_note_input(note, max_len=max_len)

    # Append note with timestamp
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    formatted_note = f"\n### [{timestamp}]\n{safe_note}\n"

    with open(config.session_file, "a", encoding="utf-8") as f:
        f.write(formatted_note)

    success("Saved note to session")
    info(f"Note: {safe_note}")


@click.command(name="session-end")
@click.option("--refresh/--no-refresh", default=False, help="Refresh context bundle with AI")
def session_end(refresh):
    """End current session with optional AI refresh"""
    config = Config()

    if not config.session_file.exists():
        click.echo("No active session found.")
        return

    # Add session end marker
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    end_marker = f"\n---\n### Session ended at {timestamp}\n---\n"

    with open(config.session_file, "a", encoding="utf-8") as f:
        f.write(end_marker)

    success(f"Session ended at {timestamp}")

    if refresh or config.get("auto_refresh"):
        info("Refreshing context bundle...")
        ctx = click.get_current_context()
        ctx.invoke(bundle)
    else:
        info("Tip: Run 'context bundle' to refresh context for next session")

