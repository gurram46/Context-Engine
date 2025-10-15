"""Update task command for Context Engine."""

import click

from context_engine.core.task_manager import set_task, get_task
from context_engine.compressors.compress_src import compress_for_task


@click.command("update-task")
@click.option("--task", prompt="New task description", help="Update the current task")
def update_task(task):
    """Update the current task and recompress src/."""
    current_task = get_task()
    if current_task:
        click.echo(f"Updating task from: {current_task}")
    
    set_task(task)
    click.secho(f"[OK] Task updated to: {task}", fg="green")
    click.echo("Recompressing source code for new task...")
    compress_for_task(task)
    click.secho("Recompression complete.", fg="cyan")