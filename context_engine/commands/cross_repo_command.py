"""Cross-repository sync commands"""

import click
import shutil
from pathlib import Path

from ..core import Config

@click.command(name="pull-cross")
def pull_cross():
    """Pull cross_repo.md from linked repositories"""
    config = Config()
    
    linked_repos = config.get("linked_repos", [])
    
    if not linked_repos:
        click.echo("❌ No linked repositories configured.")
        click.echo("Add repos to config.json in .context/")
        return
    
    combined_content = []
    pulled_count = 0
    
    for repo_path in linked_repos:
        repo = Path(repo_path).expanduser().resolve()
        
        if not repo.exists():
            click.echo(f"⚠️  Repo not found: {repo}")
            continue
        
        cross_file = repo / ".context" / "cross_repo.md"
        
        if cross_file.exists():
            content = cross_file.read_text(encoding='utf-8')
            if content.strip():
                combined_content.append(f"## From: {repo.name}\n")
                combined_content.append(content)
                combined_content.append("\n---\n")
                pulled_count += 1
                click.echo(f"✅ Pulled from {repo.name}")
        else:
            click.echo(f"⚠️  No cross_repo.md in {repo.name}")
    
    if combined_content:
        # Save combined content
        config.cross_repo_file.write_text('\n'.join(combined_content), encoding='utf-8')
        click.echo(f"\n📝 Pulled notes from {pulled_count} repo(s)")
        click.echo(f"💾 Saved to: {config.cross_repo_file}")
    else:
        click.echo("\n❌ No cross-repo notes found in linked repositories")
