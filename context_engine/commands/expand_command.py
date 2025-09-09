"""Expand command to add files mid-session"""

import click
from pathlib import Path

from ..core import Config, redact_secrets, count_tokens

@click.command()
@click.argument('files', nargs=-1, required=True, type=click.Path(exists=True))
def expand(files):
    """Add compressed summaries of files to the Expanded Files section"""
    import click as _click
    config = Config()

    allowed_exts = set(config.get("allowed_extensions", []))
    size_limit = int(config.get("max_file_size_kb", 1024)) * 1024

    # Read existing context
    existing = config.context_file.read_text(encoding='utf-8') if config.context_file.exists() else "# Project Context for AI Tools\n\n## Architecture\nNone\n\n## APIs\nNone\n\n## Configuration\nNone\n\n## Database Schema\nNone\n\n## Session Notes\nNone\n\n## Cross-Repo Notes\nNone\n\n## Expanded Files\n"

    # Ensure we append to the Expanded Files section
    if "\n## Expanded Files\n" not in existing:
        existing = existing.rstrip() + "\n\n## Expanded Files\n"

    addition_lines = []
    from ..core.utils import validate_path_in_project, compress_code, summarize_config

    for file_path in files:
        path = Path(file_path).resolve()
        # Security validations
        validate_path_in_project(path, config.project_root)
        if path.suffix.lower() not in allowed_exts:
            raise _click.BadParameter(f"Disallowed file type: {path.suffix}")
        if path.stat().st_size > size_limit:
            raise _click.BadParameter(f"File too large (> {size_limit//1024} KB): {path.name}")

        raw = path.read_text(encoding='utf-8')
        raw = redact_secrets(raw)

        language = (path.suffix[1:] or 'text').lower()
        compressed = ""
        if path.suffix.lower() in {'.py', '.js', '.ts', '.java', '.c', '.cpp'}:
            compressed = compress_code(raw, 'python' if path.suffix.lower()=='.py' else 'javascript')
        elif path.suffix.lower() in {'.json', '.yml', '.yaml', '.toml', '.ini', '.env'}:
            compressed = summarize_config(raw)
        else:
            # For markdown and others, keep concise preview
            lines = [l.strip() for l in raw.split('\n') if l.strip()]
            compressed = "\n".join(lines[:50])

        addition_lines.append(f"### {path.name}")
        addition_lines.append(compressed if compressed.strip() else "(no summary)")
        click.echo(f"✅ Added {path.name}")

    # Append to the Expanded Files section at the end
    full_content = existing.rstrip() + "\n" + "\n\n".join(addition_lines) + "\n"
    config.context_file.write_text(full_content, encoding='utf-8')

    # Show token count
    tokens = count_tokens(full_content)
    click.echo(f"\n📊 Updated token count: {tokens:,}")
    click.echo(f"📝 Context updated: {config.context_file}")
    click.echo("\n🤖 In your AI tool, re-read: .context/context_for_ai.md")
