"""Tiny UI helper around Click for consistent, optional colors."""

import click


def _color_param():
    """Return Click color parameter: False to force off, None to auto."""
    try:
        ctx = click.get_current_context(silent=True)
        if ctx and isinstance(ctx.obj, dict) and ctx.obj.get("color") is False:
            return False
    except Exception:
        pass
    # None lets Click auto-detect color support
    return None


def success(message: str) -> None:
    click.secho(message, fg="green", color=_color_param())


def info(message: str) -> None:
    click.secho(message, fg="cyan", color=_color_param())


def warn(message: str) -> None:
    click.secho(message, fg="yellow", color=_color_param())


def error(message: str) -> None:
    click.secho(message, fg="red", color=_color_param())
