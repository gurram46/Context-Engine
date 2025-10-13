"""Baseline management commands"""

import shutil
from datetime import datetime
from pathlib import Path

import click
from builtins import list as builtin_list

from ..ui import success, info, warn
from ..core import (
    Config,
    load_hashes,
    save_hashes,
    check_staleness,
    update_hash,
)
from ..core.auto_architecture import generate_auto_architecture
from ..core.utils import validate_path_in_project


@click.group()
def baseline():
    """Manage baseline files"""
    pass




@baseline.command()
def auto():
    """Auto-generate architecture doc when none exists."""
    config = Config()
    config.baseline_dir.mkdir(parents=True, exist_ok=True)

    existing_docs = builtin_list(config.baseline_dir.glob('architecture*.md'))
    if existing_docs:
        info('Architecture document already present in baseline:')
        for doc in existing_docs:
            click.echo(f"  - {doc.name}")
        return

    generated_path = generate_auto_architecture(config.project_root)

    hashes = load_hashes(config.hashes_file)
    update_hash(generated_path, hashes)
    save_hashes(config.hashes_file, hashes)

    info('No architecture doc found - auto-generated architecture_auto.md.')

@baseline.command()
@click.argument("files", nargs=-1, required=True, type=click.Path(exists=True))
def add(files):
    """Add files to baseline"""
    from click import BadParameter

    config = Config()
    config.baseline_dir.mkdir(parents=True, exist_ok=True)

    hashes = load_hashes(config.hashes_file)

    allowed_exts = set(config.get("allowed_extensions", []))
    size_limit = int(config.get("max_file_size_kb", 1024)) * 1024

    for file_path in files:
        source = Path(file_path).resolve()
        # Security: path traversal prevention
        validate_path_in_project(source, config.project_root)

        # Validate file type and size
        if source.suffix.lower() not in allowed_exts:
            raise BadParameter(f"Disallowed file type: {source.suffix}")
        if source.stat().st_size > size_limit:
            raise BadParameter(
                f"File too large (> {size_limit // 1024} KB): {source.name}"
            )

        dest = config.baseline_dir / source.name

        # Copy file to baseline
        shutil.copy2(source, dest)

        # Update hash
        update_hash(dest, hashes)

        success(f"Added {source.name} to baseline")

    save_hashes(config.hashes_file, hashes)
    info(f"\nBaseline files saved to {config.baseline_dir}")


@baseline.command()
def list():  # noqa: A001 - Click command name
    """List baseline files"""
    config = Config()

    if not config.baseline_dir.exists():
        warn("No baseline directory found. Run 'context init' first.")
        return

    files = builtin_list(config.baseline_dir.glob("*"))

    if not files:
        warn("No baseline files found.")
        info("Add files with: context baseline add <files>")
        return

    info("Baseline files:")
    for file in files:
        size = file.stat().st_size / 1024  # KB
        click.echo(f"  - {file.name} ({size:.1f} KB)")


@baseline.command()
def review():
    """Review baseline with staleness warnings"""
    config = Config()

    if not config.baseline_dir.exists():
        warn("No baseline directory found. Run 'context init' first.")
        return

    files = builtin_list(config.baseline_dir.glob("*"))
    if not files:
        warn("No baseline files found.")
        return

    hashes = load_hashes(config.hashes_file)

    info("Baseline Review:")
    stale_count = 0

    for file in files:
        is_stale = check_staleness(file, hashes)
        status = "[STALE]" if is_stale else "[OK]"

        if is_stale:
            stale_count += 1

        # Get last modified time
        mtime = datetime.fromtimestamp(file.stat().st_mtime)
        time_str = mtime.strftime("%Y-%m-%d %H:%M")

        click.echo(f"  {status} {file.name} (modified: {time_str})")

    if stale_count > 0:
        warn(f"\n{stale_count} file(s) have changed since last hash.")
        info("Re-add files to update: context baseline add <files>")

