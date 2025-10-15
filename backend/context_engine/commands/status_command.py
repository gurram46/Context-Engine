"""Status command to show current state"""

from datetime import datetime

import click

from ..ui import info, warn
from ..core import Config, count_tokens, load_hashes, check_staleness


@click.command()
def status():
    """Show Context Engine status and warnings"""
    config = Config()

    info("Context Engine Status\n")

    # Check initialization
    if not config.context_dir.exists():
        warn("Not initialized. Run 'context init' to start.")
        return

    # Context file status
    if config.context_file.exists():
        content = config.context_file.read_text(encoding="utf-8")
        tokens = count_tokens(content)
        mtime = datetime.fromtimestamp(config.context_file.stat().st_mtime)

        info("Context Bundle:")
        click.echo(f"   - File: {config.context_file}")
        click.echo(f"   - Tokens: {tokens:,}")
        click.echo(f"   - Updated: {mtime.strftime('%Y-%m-%d %H:%M:%S')}")

        if tokens > config.get("max_tokens", 100000):
            warn(
                f"   Token limit exceeded ({config.get('max_tokens', 100000):,})"
            )
    else:
        info("Context Bundle: Not generated yet")
        click.echo("   Run 'context bundle' to create")

    # Baseline status
    if config.baseline_dir.exists():
        files = list(config.baseline_dir.glob("*"))
        if files:
            hashes = load_hashes(config.hashes_file)
            stale_count = sum(1 for f in files if check_staleness(f, hashes))

            info("\nBaseline:")
            click.echo(f"   - Files: {len(files)}")
            if stale_count > 0:
                warn(f"   Stale files: {stale_count}")
        else:
            info("\nBaseline: No files added")
    else:
        info("\nBaseline: No files added")

    # Session status
    if config.session_file.exists() and config.session_file.stat().st_size > 0:
        lines = (
            config.session_file.read_text(encoding="utf-8").strip().split("\n")
        )

        # Find last timestamp
        last_save = None
        for line in reversed(lines):
            if line.startswith("### ["):
                try:
                    timestamp_str = line[5:24]  # Extract timestamp
                    last_save = datetime.strptime(timestamp_str, "%Y-%m-%d %H:%M:%S")
                    break
                except Exception:
                    pass

        notes_count = sum(1 for l in lines if l.startswith("### ["))
        info("\nSession:")
        click.echo(f"   - Notes: {notes_count}")
        if last_save:
            click.echo(f"   - Last save: {last_save.strftime('%Y-%m-%d %H:%M:%S')}")
    else:
        info("\nSession: No notes saved")

    # ADRs status
    if config.adrs_dir.exists():
        adr_files = list(config.adrs_dir.glob("*.md"))
        if adr_files:
            info(f"\nADRs: {len(adr_files)} document(s)")

    # Cross-repo status
    if config.cross_repo_file.exists() and config.cross_repo_file.stat().st_size > 0:
        info("\nCross-repo notes: Present")

    # Configuration
    info("\nConfiguration:")
    click.echo(
        f"   - API Key: {'Configured' if config.openrouter_api_key else 'Not set'}"
    )
    click.echo(f"   - Model: {config.get('model', 'qwen/qwen3-coder:free')}")
    click.echo(f"   - Auto-refresh: {config.get('auto_refresh', False)}")

    linked_repos = config.get("linked_repos", [])
    if linked_repos:
        click.echo(f"   - Linked repos: {len(linked_repos)}")

