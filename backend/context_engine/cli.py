"""Main CLI interface for Context Engine"""

import click
from pathlib import Path
from importlib import import_module

# Import subcommand modules explicitly to avoid package import edge cases
init_command = import_module('context_engine.commands.init_command')
baseline_commands = import_module('context_engine.commands.baseline_commands')
session_commands = import_module('context_engine.commands.session_commands')
bundle_command = import_module('context_engine.commands.bundle_command')
expand_command = import_module('context_engine.commands.expand_command')
status_command = import_module('context_engine.commands.status_command')
cross_repo_command = import_module('context_engine.commands.cross_repo_command')
config_commands = import_module('context_engine.commands.config_commands')
compress_command = import_module('context_engine.commands.compress_command')
start_session_command = import_module('context_engine.commands.start_session_command')
update_task_command = import_module('context_engine.commands.update_task_command')
show_task_command = import_module('context_engine.commands.show_task_command')
stop_session_command = import_module('context_engine.commands.stop_session_command')


@click.group()
@click.version_option(version="1.0.0", prog_name="context-engine")
@click.option("--no-color", is_flag=True, default=False, help="Disable colored output")
@click.pass_context
def cli(ctx: click.Context, no_color: bool):
    """Context Engine - Reduce token waste in AI coding sessions"""
    # Store shared options
    ctx.ensure_object(dict)
    ctx.obj["color"] = not no_color

# Register all command groups
cli.add_command(init_command.init)
cli.add_command(baseline_commands.baseline)
cli.add_command(session_commands.save)
cli.add_command(session_commands.session_end)
cli.add_command(bundle_command.bundle)
cli.add_command(expand_command.expand)
cli.add_command(status_command.status)
cli.add_command(cross_repo_command.pull_cross)
cli.add_command(config_commands.config)
cli.add_command(compress_command.compress_cmd)
cli.add_command(start_session_command.start_session)
cli.add_command(update_task_command.update_task)
cli.add_command(show_task_command.show_task)
cli.add_command(stop_session_command.stop_session)

def main():
    """Main entry point"""
    cli()

if __name__ == "__main__":
    main()
