"""Main CLI interface for Context Engine v1.2 - Session Intelligence Model"""

import click
from pathlib import Path
from importlib import import_module
import asyncio
import signal

# Import core command modules
init_command = import_module('context_engine.commands.init_command')
baseline_commands = import_module('context_engine.commands.baseline_commands')
bundle_command = import_module('context_engine.commands.bundle_command')
compress_command = import_module('context_engine.commands.compress_command')
config_commands = import_module('context_engine.commands.config_commands')

# Import summary modules for AI summarization
from context_engine.core.summary import ProjectSummarizer
try:
    from context_engine.core.session_tracker import (
        start_session_tracker,
        stop_session_tracker,
        log_cli_command,
        show_session_status,
    )
    from context_engine.core.ai_summary import generate_session_summary, show_quick_summary
    SESSION_INTELLIGENCE_AVAILABLE = True
except ImportError as e:
    SESSION_INTELLIGENCE_AVAILABLE = False
    print(f"Warning: Session intelligence features disabled - {e}")


@click.group()
@click.version_option(version="1.2.0", prog_name="context-engine")
@click.option("--no-color", is_flag=True, default=False, help="Disable colored output")
@click.pass_context
def cli(ctx: click.Context, no_color: bool):
    """Context Engine v1.2 - AI-powered session intelligence for coding projects"""
    # Store shared options
    ctx.ensure_object(dict)
    ctx.obj["color"] = not no_color

# Register the base command set
cli.add_command(init_command.init)
cli.add_command(baseline_commands.baseline)
cli.add_command(bundle_command.bundle)
cli.add_command(compress_command.compress_cmd)
cli.add_command(config_commands.config)

@click.command()
@click.option('-m', '--model', default="ai", help="Use AI model to summarize current session")
@click.option('--project-root', type=click.Path(exists=True), help='Project root directory')
def summary(model, project_root):
    """Generate AI-powered project summary or display latest summary"""
    from context_engine.ui import info, success, warn, error
    from context_engine.core.config import get_model
    from pathlib import Path

    # If project_root not specified, use current working directory
    if not project_root:
        project_root = Path.cwd()
    else:
        project_root = Path(project_root)

    # Handle different summary modes
    if model == "ai":
        # Show quick AI summary of current session
        show_quick_summary()
    else:
        # Legacy project summary behavior
        ai_model = get_model()

        # Check for existing summary first
        summary_file = project_root / ".context" / "summary_report.md"

        if summary_file.exists():
            # Display existing summary
            info("Displaying latest project summary:")
            print("\n" + "="*60)
            print(summary_file.read_text(encoding='utf-8'))
            print("="*60)
            return

        # Generate new summary
        info(f"Generating {ai_model} summary...")

        try:
            # Create summarizer and generate summary
            summarizer = ProjectSummarizer(model_choice=ai_model)

            # Run async function in sync context
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            summary_text, summary_path = loop.run_until_complete(
                summarizer.generate_summary(project_root)
            )
            loop.close()

            success(f"Summary generated and saved to: {summary_path}")

            # Display summary
            print("\n" + "="*60)
            print(summary_text)
            print("="*60)

        except Exception as e:
            error(f"Failed to generate summary: {e}")

# Register the summary command
cli.add_command(summary)

if SESSION_INTELLIGENCE_AVAILABLE:
    @click.command("start-session")
    @click.option("--auto", is_flag=True, help="Enable automatic logging of file and CLI actions")
    def start_session(auto: bool):
        """Start background logging of file and CLI activity."""
        start_session_tracker(auto=auto)

    @click.command("stop-session")
    def stop_session():
        """Stop current session gracefully."""
        stop_session_tracker()

    @click.group()
    def session():
        """Manage Context Engine sessions."""
        pass

    @session.command("save")
    def session_save():
        """Generate an AI session summary."""
        from context_engine.ui import success
        generate_session_summary()
        success("Session summary generated")

    @session.command("status")
    def session_status():
        """Show tracker status."""
        show_session_status()

    cli.add_command(start_session)
    cli.add_command(stop_session)
    cli.add_command(session)

def main():
    """Main entry point"""
    cli()

if __name__ == "__main__":
    main()


