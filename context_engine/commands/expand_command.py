"""Expand command to add files mid-session"""

import click
from pathlib import Path

from ..core import Config, redact_secrets, count_tokens

@click.command()
@click.argument('files', nargs=-1, required=True, type=click.Path(exists=True))
def expand(files):
    """Add files to context_for_ai.md mid-session"""
    config = Config()
    
    # Read existing context
    if config.context_file.exists():
        existing_content = config.context_file.read_text(encoding='utf-8')
    else:
        existing_content = "# Project Context for AI Tools\n\n"
    
    # Add expansion marker
    expansion_content = ["\n\n## Mid-Session Expansion\n"]
    
    for file_path in files:
        path = Path(file_path)
        content = path.read_text(encoding='utf-8')
        
        # Redact secrets
        content = redact_secrets(content)
        
        expansion_content.append(f"\n### {path.name}\n")
        expansion_content.append(f"```{path.suffix[1:] if path.suffix else 'text'}")
        expansion_content.append(content)
        expansion_content.append("```")
        
        click.echo(f"✅ Added {path.name}")
    
    # Append to context file
    full_content = existing_content + '\n'.join(expansion_content)
    config.context_file.write_text(full_content, encoding='utf-8')
    
    # Show token count
    tokens = count_tokens(full_content)
    click.echo(f"\n📊 Updated token count: {tokens:,}")
    click.echo(f"📝 Context updated: {config.context_file}")
    click.echo("\n🤖 In your AI tool, re-read: .context/context_for_ai.md")
