"""Baseline management commands"""

import click
import shutil
from pathlib import Path
from datetime import datetime

from ..core import Config, load_hashes, save_hashes, check_staleness, update_hash

@click.group()
def baseline():
    """Manage baseline files"""
    pass

@baseline.command()
@click.argument('files', nargs=-1, required=True, type=click.Path(exists=True))
def add(files):
    """Add files to baseline"""
    import click as _click
    config = Config()
    config.baseline_dir.mkdir(parents=True, exist_ok=True)
    
    hashes = load_hashes(config.hashes_file)
    
    allowed_exts = set(config.get("allowed_extensions", []))
    size_limit = int(config.get("max_file_size_kb", 1024)) * 1024
    
    for file_path in files:
        source = Path(file_path).resolve()
        # Security: path traversal prevention
        from ..core.utils import validate_path_in_project
        validate_path_in_project(source, config.project_root)
        
        # Validate file type and size
        if source.suffix.lower() not in allowed_exts:
            raise _click.BadParameter(f"Disallowed file type: {source.suffix}")
        if source.stat().st_size > size_limit:
            raise _click.BadParameter(f"File too large (> {size_limit//1024} KB): {source.name}")
        
        dest = config.baseline_dir / source.name
        
        # Copy file to baseline
        shutil.copy2(source, dest)
        
        # Update hash
        update_hash(dest, hashes)
        
        click.echo(f"✅ Added {source.name} to baseline")
    
    save_hashes(config.hashes_file, hashes)
    click.echo(f"\n📂 Baseline files saved to {config.baseline_dir}")

@baseline.command()
def list():
    """List baseline files"""
    config = Config()
    
    if not config.baseline_dir.exists():
        click.echo("❌ No baseline directory found. Run 'context init' first.")
        return
    
    files = list(config.baseline_dir.glob("*"))
    
    if not files:
        click.echo("📂 No baseline files found.")
        click.echo("Add files with: context baseline add <files>")
        return
    
    click.echo("📂 Baseline files:")
    for file in files:
        size = file.stat().st_size / 1024  # KB
        click.echo(f"  • {file.name} ({size:.1f} KB)")

@baseline.command()
def review():
    """Review baseline with staleness warnings"""
    config = Config()
    
    if not config.baseline_dir.exists():
        click.echo("❌ No baseline directory found. Run 'context init' first.")
        return
    
    files = list(config.baseline_dir.glob("*"))
    if not files:
        click.echo("📂 No baseline files found.")
        return
    
    hashes = load_hashes(config.hashes_file)
    
    click.echo("📂 Baseline Review:")
    stale_count = 0
    
    for file in files:
        is_stale = check_staleness(file, hashes)
        status = "⚠️  STALE" if is_stale else "✅"
        
        if is_stale:
            stale_count += 1
        
        # Get last modified time
        mtime = datetime.fromtimestamp(file.stat().st_mtime)
        time_str = mtime.strftime("%Y-%m-%d %H:%M")
        
        click.echo(f"  {status} {file.name} (modified: {time_str})")
    
    if stale_count > 0:
        click.echo(f"\n⚠️  {stale_count} file(s) have changed since last hash.")
        click.echo("Re-add files to update: context baseline add <files>")
