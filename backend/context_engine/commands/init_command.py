"""Initialize Context Engine in a project."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path

import click

from ..ui import info, success
from ..core.config import load_config, get_config_file


@click.command()
def init():
    """Initialize Context Engine assets in the current project."""
    project_root = Path.cwd()
    context_dir = project_root / ".context"
    baseline_dir = context_dir / "baseline"
    notes_dir = context_dir / "notes"

    context_dir.mkdir(exist_ok=True)
    baseline_dir.mkdir(exist_ok=True)
    notes_dir.mkdir(exist_ok=True)

    session_file = context_dir / "session.md"
    if not session_file.exists():
        session_file.write_text(
            "# Context Engine Session Log\n"
            f"# Created: {Path.cwd().name} - {datetime.now():%Y-%m-%d %H:%M:%S}\n\n",
            encoding="utf-8",
        )

    readme_file = context_dir / "README.md"
    if not readme_file.exists():
        readme_file.write_text(
            "# Context Engine Workspace\n\n"
            "- `session.md`: Running log of edits, commands, and notes.\n"
            "- `session_summary.md`: AI-generated summary snapshots.\n"
            "- `baseline/`: Store reference docs or baseline context inputs.\n"
            "- `notes/`: Add freeform notes you want to persist between sessions.\n",
            encoding="utf-8",
        )

    # Ensure config file exists (load_config creates default if missing)
    load_config()
    info(f"Configuration stored at {get_config_file().relative_to(project_root)}")

    success(f"Initialized Context Engine assets in {context_dir}")
    info("Next steps:")
    info("  - Start tracking with `context start-session --auto`")
    info("  - Add important files to `.context/baseline/` for summaries")
