"""Stop session command for Context Engine."""

import click

from context_engine.core.task_manager import clear_task


@click.command("stop-session")
def stop_session():
    """Stop the current session and clear the task."""
    from context_engine.core.task_manager import get_task
    current_task = get_task()
    
    if current_task:
        click.echo(f"Stopping session for task: {current_task}")
    
    clear_task()
    click.secho("Session stopped and task cleared.", fg="green")