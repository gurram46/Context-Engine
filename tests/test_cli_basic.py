"""Basic smoke tests for the Context Engine CLI."""

from __future__ import annotations

import os
import sys
from pathlib import Path

from click.testing import CliRunner

BACKEND_PATH = Path(__file__).resolve().parents[1] / "backend"
if str(BACKEND_PATH) not in sys.path:
    sys.path.insert(0, str(BACKEND_PATH))

from context_engine.cli import cli  # noqa: E402


def test_init_creates_context_directory():
    runner = CliRunner()
    with runner.isolated_filesystem():
        result = runner.invoke(cli, ["init"])
        assert result.exit_code == 0, result.output
        context_dir = Path(".context")
        assert context_dir.exists()
        assert (context_dir / "session.md").exists()


def test_session_save_generates_summary():
    runner = CliRunner()
    with runner.isolated_filesystem():
        context_dir = Path(".context")
        context_dir.mkdir()
        session_file = context_dir / "session.md"
        session_file.write_text("[12:00] Edited: backend/main.py\n", encoding="utf-8")

        # Ensure static fallback is used
        os.environ.pop("OPENROUTER_API_KEY", None)

        result = runner.invoke(cli, ["session", "save"])
        assert result.exit_code == 0, result.output
        summary_file = context_dir / "session_summary.md"
        assert summary_file.exists()
        content = summary_file.read_text(encoding="utf-8")
        assert "Session Summary" in content


def test_session_status_when_inactive():
    runner = CliRunner()
    with runner.isolated_filesystem():
        result = runner.invoke(cli, ["session", "status"])
        assert result.exit_code == 0, result.output
        assert "not running" in result.output.lower()

def test_baseline_list_command():
    runner = CliRunner()
    with runner.isolated_filesystem():
        result_init = runner.invoke(cli, ['init'])
        assert result_init.exit_code == 0, result_init.output
        result = runner.invoke(cli, ['baseline', 'list'])
        assert result.exit_code == 0, result.output
