"""Session management commands"""

import click
from datetime import datetime
from pathlib import Path

from ..core import Config
from ..models import OpenRouterClient
from .bundle_command import bundle

@click.command()
@click.argument('note')
def save(note):
    """Save a note to the current session"""
    config = Config()
    
    # Ensure session file exists
    config.session_file.touch()
    
    # Append note with timestamp
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    formatted_note = f"\n### [{timestamp}]\n{note}\n"
    
    with open(config.session_file, 'a', encoding='utf-8') as f:
        f.write(formatted_note)
    
    click.echo(f"✅ Saved note to session")
    click.echo(f"📝 Note: {note}")

@click.command(name="session-end")
@click.option('--refresh/--no-refresh', default=False, 
              help="Refresh context bundle with AI")
def session_end(refresh):
    """End current session with optional AI refresh"""
    config = Config()
    
    if not config.session_file.exists():
        click.echo("❌ No active session found.")
        return
    
    # Add session end marker
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    end_marker = f"\n---\n### Session ended at {timestamp}\n---\n"
    
    with open(config.session_file, 'a', encoding='utf-8') as f:
        f.write(end_marker)
    
    click.echo(f"✅ Session ended at {timestamp}")
    
    if refresh or config.get("auto_refresh"):
        click.echo("🔄 Refreshing context bundle...")
        ctx = click.get_current_context()
        ctx.invoke(bundle)
    else:
        click.echo("💡 Tip: Run 'context bundle' to refresh context for next session")
