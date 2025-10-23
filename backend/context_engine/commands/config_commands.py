"""Simplified configuration commands for Context Engine v1.2.1"""

import json
import click

from ..core.config import load_config, get_config_file, set_model, get_model, get_api_key
from ..ui import info, success, warn, error


@click.group(name="config")
def config():
    """View and edit settings in .context/config.json"""
    pass


@config.command("show")
def show():
    """Display current configuration values"""
    try:
        data = load_config()
        info(json.dumps(data, indent=2))
    except Exception:
        error("Failed to load configuration")


@config.command("set")
@click.argument("key")
@click.argument("value")
def set_value(key: str, value: str):
    """Set configuration value (only 'model' and 'api_key' supported)"""

    # Only allow essential config keys
    allowed_keys = ["model", "api_key"]

    if key not in allowed_keys:
        warn(f"Unsupported key: {key}. Only {allowed_keys} are supported.")
        return

    try:
        config = load_config()

        if key == "model":
            set_model(value)
            success(f"Model set to: {value}")
        elif key == "api_key":
            config[key] = value
            config_file = get_config_file()
            config_file.parent.mkdir(parents=True, exist_ok=True)
            with open(config_file, "w", encoding="utf-8") as f:
                json.dump(config, f, indent=2)
            success(f"API key set (hidden for security)")

    except Exception as e:
        error(f"Failed to set {key}: {e}")


@config.command("path")
def path():
    """Print the config file path"""
    info(str(get_config_file()))


# Remove unsupported commands: unset, and any advanced config options
# The config system now only supports: model and api_key

