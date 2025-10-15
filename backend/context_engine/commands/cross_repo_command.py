"""Cross-repository sync commands"""

from pathlib import Path

import click

from ..ui import warn, info, success
from ..core import Config


@click.command(name="pull-cross")
def pull_cross():
    """Pull cross_repo.md from linked repositories"""
    config = Config()

    linked_repos = config.get("linked_repos", [])

    if not linked_repos:
        warn("No linked repositories configured.")
        info("Add repos to config.json in .context/")
        return

    combined_content = []
    pulled_count = 0

    for repo_path in linked_repos:
        repo = Path(repo_path).expanduser().resolve()

        if not repo.exists():
            warn(f"Repo not found: {repo}")
            continue

        cross_file = repo / ".context" / "cross_repo.md"

        if cross_file.exists():
            content = cross_file.read_text(encoding="utf-8")
            if content.strip():
                combined_content.append(f"## From: {repo.name}\n")
                combined_content.append(content)
                combined_content.append("\n---\n")
                pulled_count += 1
                success(f"Pulled from {repo.name}")
        else:
            warn(f"No cross_repo.md in {repo.name}")

    if combined_content:
        # Save combined content
        config.cross_repo_file.write_text("\n".join(combined_content), encoding="utf-8")
        info(f"\nPulled notes from {pulled_count} repo(s)")
        info(f"Saved to: {config.cross_repo_file}")
    else:
        warn("\nNo cross-repo notes found in linked repositories")

