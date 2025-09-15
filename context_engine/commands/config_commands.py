"""Configuration commands for Context Engine"""

import json
from typing import Any

import click

from ..core import Config
from ..ui import info, success, warn, error


@click.group(name="config")
def config():
    """View and edit settings in .context/config.json"""
    pass


@config.command("show")
def show():
    """Show current configuration (merged defaults + file)"""
    cfg = Config()
    try:
        data = cfg._config  # merged view
        info(json.dumps(data, indent=2))
    except Exception:
        error("Failed to load configuration")


@config.command("path")
def path():
    """Print the config file path"""
    cfg = Config()
    info(str(cfg.config_file))


def _parse_value(value: str) -> Any:
    # Try JSON parsing to support numbers, bools, lists, objects
    try:
        return json.loads(value)
    except Exception:
        return value


@config.command("set")
@click.argument("key")
@click.argument("value")
def set_value(key: str, value: str):
    """Set KEY to VALUE (VALUE can be JSON)"""
    cfg = Config()
    parsed = _parse_value(value)
    cfg.set(key, parsed)
    success(f"Set {key}")


@config.command("unset")
@click.argument("key")
def unset_value(key: str):
    """Remove KEY from the config file"""
    cfg = Config()
    # Remove directly from file to avoid leaving a null
    data = {}
    if cfg.config_file.exists():
        try:
            data = json.loads(cfg.config_file.read_text(encoding="utf-8"))
        except Exception:
            data = {}
    if key in data:
        data.pop(key, None)
        cfg.context_dir.mkdir(parents=True, exist_ok=True)
        cfg.config_file.write_text(json.dumps(data, indent=2), encoding="utf-8")
        success(f"Unset {key}")
    else:
        warn(f"Key not found: {key}")

