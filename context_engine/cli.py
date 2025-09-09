"""Main CLI interface for Context Engine"""

import click
from pathlib import Path

from .commands import (
    init_command,
    baseline_commands,
    session_commands,
    bundle_command,
    expand_command,
    status_command,
    cross_repo_command
)

@click.group()
@click.version_option(version="1.0.0", prog_name="context-engine")
def cli():
    """Context Engine - Reduce token waste in AI coding sessions"""
    pass

# Register all command groups
cli.add_command(init_command.init)
cli.add_command(baseline_commands.baseline)
cli.add_command(session_commands.save)
cli.add_command(session_commands.session_end)
cli.add_command(bundle_command.bundle)
cli.add_command(expand_command.expand)
cli.add_command(status_command.status)
cli.add_command(cross_repo_command.pull_cross)

def main():
    """Main entry point"""
    cli()

if __name__ == "__main__":
    main()
