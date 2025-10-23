"""Session management commands"""

from datetime import datetime

import click

from context_engine.ui import success, info
from context_engine.core import Config
from context_engine.models import OpenRouterClient
from context_engine.commands.bundle_command import bundle


@click.command(name="save")
@click.option("--summarize/--no-summarize", default=True, help="Generate AI-powered summary")
@click.argument("note", required=False)
def save(note, summarize):
    """Save a note to current session and generate AI-powered summary"""
    import asyncio
    from ..core.summary import ProjectSummarizer
    from ..core.config import get_model
    from ..ui import warn, error

    config = Config()
    config.context_dir.mkdir(parents=True, exist_ok=True)
    config.session_file.parent.mkdir(parents=True, exist_ok=True)
    config.session_file.touch()

    # Handle note input
    if note:
        # Sanitize and limit note size
        from ..core.utils import sanitize_note_input
        max_len = int(config.get("note_max_length", 2000))
        safe_note = sanitize_note_input(note, max_len=max_len)

        # Append note with timestamp
        timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        formatted_note = f"\n### [{timestamp}]\n{safe_note}\n"

        with open(config.session_file, "a", encoding="utf-8") as f:
            f.write(formatted_note)

        success("Saved note to session")
        info(f"Note: {safe_note}")
    else:
        info("No note provided, proceeding to summary generation...")

    # Generate AI-powered summary if requested
    if summarize and config.session_file.exists():
        info("Generating AI-powered session summary...")

        try:
            # Create session summary
            session_content = config.session_file.read_text(encoding='utf-8')

            # Get AI model from config and create summarizer
            ai_model = get_model()
            summarizer = ProjectSummarizer(model_choice=ai_model)

            # Create a simple project data structure from session
            session_data = {
                'content': {
                    'session.md': {
                        'content': session_content,
                        'size': len(session_content),
                        'type': 'session',
                        'language': 'Markdown'
                    }
                },
                'stats': {
                    'total_files': 1,
                    'total_size': len(session_content),
                    'scan_time': datetime.now().timestamp()
                }
            }

            # Generate summary
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            if ai_model == 'static':
                summary_text = summarizer.generate_static_summary(session_data)
            else:
                summary_text = loop.run_until_complete(
                    summarizer.generate_ai_summary(session_data)
                )

            loop.close()

            # Save session summary
            session_summary_file = config.context_dir / "session_summary.md"
            session_summary_file.write_text(summary_text, encoding='utf-8')

            success(f"Session summary generated: {session_summary_file.relative_to(config.project_root)}")

            # Display preview of summary
            preview_lines = summary_text.split('\n')[:10]
            info("Summary preview:")
            for line in preview_lines:
                print(f"  {line}")
            if len(summary_text.split('\n')) > 10:
                print("  ...")

        except Exception as e:
            error(f"Failed to generate session summary: {e}")
            warn("Session note was saved, but summary generation failed")
    elif not summarize:
        info("Summary generation skipped as requested")
    else:
        warn("No session file found to summarize")


@click.command(name="session-end")
@click.option("--refresh/--no-refresh", default=False, help="Refresh context bundle with AI")
def session_end(refresh):
    """End current session with optional AI refresh"""
    config = Config()

    if not config.session_file.exists():
        click.echo("No active session found.")
        return

    # Add session end marker
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    end_marker = f"\n---\n### Session ended at {timestamp}\n---\n"

    with open(config.session_file, "a", encoding="utf-8") as f:
        f.write(end_marker)

    success(f"Session ended at {timestamp}")

    if refresh or config.get("auto_refresh"):
        info("Refreshing context bundle...")
        ctx = click.get_current_context()
        ctx.invoke(bundle)
    else:
        info("Tip: Run 'context bundle' to refresh context for next session")

