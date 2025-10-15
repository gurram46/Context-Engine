"""Start session command for Context Engine."""

import click

from context_engine.core.task_manager import set_task
from context_engine.compressors.compress_src import compress_for_task


@click.command("start-session")
@click.option("--task", prompt="Task description required for compression", help="Describe the task you're working on")
def start_session(task):
    """Start a new task-bound session and compress src/."""
    set_task(task)
    click.secho(f"[OK] Task set: {task}", fg="green")
    click.echo("Compressing source code for task...")
    compress_for_task(task)
    click.secho("Compression complete.", fg="cyan")