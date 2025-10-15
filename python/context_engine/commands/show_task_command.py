"""Show task command for Context Engine."""

import click

from context_engine.core.task_manager import get_task


@click.command("show-task")
def show_task():
    """Show the current task."""
    task = get_task()
    if task:
        click.secho(f"Current task: {task}", fg="green")
    else:
        click.secho("No task currently set. Use 'context start-session' to set a task.", fg="yellow")