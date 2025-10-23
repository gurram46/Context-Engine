# CLI Snippets and Anatomy

The Click CLI (ackend/context_engine/cli.py) is the canonical entry point for all Python commands. It registers command groups from context_engine/commands/ and wires in the session tracker helpers.

## Command registration

`python
import click
from importlib import import_module

baseline_commands = import_module('context_engine.commands.baseline_commands')
bundle_command = import_module('context_engine.commands.bundle_command')

@click.group()
def cli():
    pass

cli.add_command(baseline_commands.baseline)
cli.add_command(bundle_command.bundle)
`

When you run context baseline list, Click resolves the aseline group defined in ackend/context_engine/commands/baseline_commands.py.

## Programmatic execution

Use CliRunner to invoke commands from tests or scripts. Example from our smoke tests:

`python
from click.testing import CliRunner
from context_engine.cli import cli

runner = CliRunner()
result = runner.invoke(cli, ['baseline', 'list'])
assert result.exit_code == 0
`

This approach avoids spawning subprocesses and makes it easy to assert on command output.

## Bridging from Node

ackend/main.py uses CliRunner as well. The Ink chat sends JSON commands to the Python side, which map to CLI invocations:

`python
result = runner.invoke(cli, [command] + args)
if TRACKING_AVAILABLE:
    log_cli_command(command_string, result.output if result.exit_code else '')
`

Understanding this flow lets you add new commands in Python and immediately expose them both in the terminal and the chat UI.
