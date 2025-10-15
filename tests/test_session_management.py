"""Session management tests for Context Engine"""

import tempfile
from pathlib import Path
import pytest
from click.testing import CliRunner

from context_engine.cli import cli
from context_engine.core.task_manager import (
    task_file_path,
    set_task,
    get_task,
    clear_task
)


class TestTaskFileManagement:
    """Test session task file operations"""

    def test_task_file_path(self):
        """Test task file path generation"""
        task_path = task_file_path()
        assert task_path == Path(".context/session_task.txt")

    def test_set_and_get_task(self, tmp_path):
        """Test setting and retrieving task"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            task_description = "implement refund webhook"
            set_task(task_description)

            retrieved_task = get_task()
            assert retrieved_task == task_description

            # Verify file was created
            task_file = Path(".context/session_task.txt")
            assert task_file.exists()
            assert task_file.read_text() == task_description

    def test_clear_task(self, tmp_path):
        """Test clearing task"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Set a task first
            set_task("test task")
            assert get_task() == "test task"

            # Clear the task
            clear_task()
            assert get_task() is None

            # Verify file was removed
            task_file = Path(".context/session_task.txt")
            assert not task_file.exists()

    def test_get_task_no_file(self, tmp_path):
        """Test getting task when no file exists"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            task = get_task()
            assert task is None

    def test_context_dir_creation(self, tmp_path):
        """Test that .context directory is created when setting task"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            set_task("test task")

            context_dir = Path(".context")
            assert context_dir.exists()
            assert context_dir.is_dir()


class TestStartSessionCommand:
    """Test start-session CLI command"""

    def test_start_session_with_task(self, tmp_path):
        """Test starting session with explicit task"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Start session with task
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0
            assert f"[OK] Task set: {task}" in result.output
            assert "Compressing source code for task..." in result.output

            # Verify task was set
            retrieved_task = get_task()
            assert retrieved_task == task

            # Verify task file exists
            task_file = Path(".context/session_task.txt")
            assert task_file.exists()

    def test_start_session_prompt(self, tmp_path):
        """Test starting session with prompt"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Start session with prompt (simulating user input)
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session'], input=f"{task}\n")
            assert result.exit_code == 0
            assert f"[OK] Task set: {task}" in result.output

            # Verify task was set
            retrieved_task = get_task()
            assert retrieved_task == task


class TestStopSessionCommand:
    """Test stop-session CLI command"""

    def test_stop_session_with_task(self, tmp_path):
        """Test stopping session when task is set"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine and set task
            runner.invoke(cli, ['init'])
            task = "implement refund webhook"
            runner.invoke(cli, ['start-session', '--task', task])

            # Stop session
            result = runner.invoke(cli, ['stop-session'])
            assert result.exit_code == 0
            assert f"Stopping session for task: {task}" in result.output
            assert "Session stopped and task cleared." in result.output

            # Verify task was cleared
            retrieved_task = get_task()
            assert retrieved_task is None

            # Verify task file was removed
            task_file = Path(".context/session_task.txt")
            assert not task_file.exists()

    def test_stop_session_no_task(self, tmp_path):
        """Test stopping session when no task is set"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            runner.invoke(cli, ['init'])

            # Stop session without task
            result = runner.invoke(cli, ['stop-session'])
            assert result.exit_code == 0
            assert "Session stopped and task cleared." in result.output


if __name__ == "__main__":
    pytest.main([__file__, "-v"])