"""Initialize Context Engine in a project"""

import click

from ..ui import info, success
from ..core import Config


@click.command()
def init():
    """Initialize Context Engine in current project"""
    config = Config()

    # Create directory structure
    config.context_dir.mkdir(exist_ok=True)
    config.baseline_dir.mkdir(exist_ok=True)
    config.adrs_dir.mkdir(exist_ok=True)

    # Create empty files
    config.session_file.touch()
    config.cross_repo_file.touch()

    # Save initial config
    config.save()

    # Create sample ADR
    sample_adr = config.adrs_dir / "001-context-engine.md"
    if not sample_adr.exists():
        sample_adr.write_text(
            """# ADR-001: Context Engine Adoption

## Status
Accepted

## Context
We need a way to reduce token waste when starting AI coding sessions.

## Decision
Use Context Engine to manage project context.

## Consequences
- Reduced token usage by 20-30%
- Better session continuity
- Manual control over context
""",
            encoding="utf-8",
        )

    success(f"Initialized Context Engine in {config.context_dir}")
    info("\nNext steps:")
    info("1. Add baseline files: context baseline add <files>")
    info("2. Bundle context: context bundle")
    info("\nAI Tool Prompt:")
    info("---")
    info("Load .context/context_for_ai.md and continue working on current session")
    info("---")
