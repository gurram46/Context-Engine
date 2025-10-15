"""Bundle integration tests for Context Engine"""

import tempfile
from pathlib import Path
import pytest
from click.testing import CliRunner

from context_engine.cli import cli
from context_engine.core.task_manager import get_task
from context_engine.core.config import Config


class TestBundleIntegration:
    """Test bundle command integration with session and compression"""

    def test_bundle_without_task(self, tmp_path):
        """Test bundle generation without active task (baseline-only mode)"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create some baseline files
            arch_file = Path("architecture.md")
            arch_file.write_text("# System Architecture\n\nThis is the system architecture.")

            result = runner.invoke(cli, ['baseline', 'add', 'architecture.md'])
            assert result.exit_code == 0

            # Generate bundle without task
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0
            assert "Generated context bundle" in result.output
            assert "Token count:" in result.output

            # Check that warning is displayed
            assert "⚠ Compression disabled — baseline-only mode." in result.output

            # Verify bundle file was created
            context_file = Path(".context/context_for_ai.md")
            assert context_file.exists()

            # Check bundle content
            content = context_file.read_text()
            assert "## Architecture" in content
            assert "# System Architecture" in content
            assert "## Task" in content
            assert "None" in content  # No task set

    def test_bundle_with_task(self, tmp_path):
        """Test bundle generation with active task (includes compression)"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create baseline files
            arch_file = Path("architecture.md")
            arch_file.write_text("# System Architecture\n\nThis is the system architecture.")

            result = runner.invoke(cli, ['baseline', 'add', 'architecture.md'])
            assert result.exit_code == 0

            # Create src directory with some files
            src_dir = Path("src")
            src_dir.mkdir()
            test_file = src_dir / "test.py"
            test_file.write_text('''
def test_function():
    """This is a test function"""
    return True

class TestClass:
    """Test class documentation"""
    def method(self):
        """Method documentation"""
        pass
''')

            # Start session with task
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0

            # Generate bundle with task
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0
            assert f"Task-based session detected: {task}" in result.output
            assert "Generated context bundle" in result.output

            # Verify bundle file was created
            context_file = Path(".context/context_for_ai.md")
            assert context_file.exists()

            # Check bundle content
            content = context_file.read_text()
            assert "## Architecture" in content
            assert "# System Architecture" in content
            assert "## Task" in content
            assert task in content
            assert "## Session Notes" in content

            # Check if compressed source is included
            assert "## Expanded Files" in content

    def test_bundle_with_compressed_src(self, tmp_path):
        """Test bundle includes compressed source files"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create src directory with well-documented files
            src_dir = Path("src")
            src_dir.mkdir()

            # Create a Python file with docstrings
            py_file = src_dir / "payment.py"
            py_file.write_text('''
"""Payment processing module for handling transactions."""

from typing import Dict, Any

class PaymentProcessor:
    """Handles payment processing for various payment methods."""

    def __init__(self, config: Dict[str, Any]):
        """Initialize payment processor with configuration.

        Args:
            config: Configuration dictionary containing payment settings
        """
        self.config = config

    def process_refund(self, transaction_id: str, amount: float) -> bool:
        """Process a refund for a given transaction.

        Args:
            transaction_id: Unique identifier for the transaction
            amount: Amount to refund

        Returns:
            bool: True if refund was successful, False otherwise
        """
        # Implementation would go here
        return True

    def validate_webhook(self, webhook_data: Dict[str, Any]) -> bool:
        """Validate incoming webhook data from payment provider.

        Args:
            webhook_data: Dictionary containing webhook payload

        Returns:
            bool: True if webhook is valid, False otherwise
        """
        # Implementation would go here
        return True
''')

            # Start session with task
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0

            # Check that compressed source was created
            compressed_src_dir = Path(".context/compressed_src")
            assert compressed_src_dir.exists()

            compressed_files = list(compressed_src_dir.glob("*.md"))
            assert len(compressed_files) > 0

            # Generate bundle
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0

            # Verify bundle includes compressed source
            context_file = Path(".context/context_for_ai.md")
            content = context_file.read_text()

            # Check that the compressed source section is present
            assert "### Compressed Source Files" in content
            assert "Compressed Source Files for Task" in content
            assert "Payment processing module" in content
            assert "PaymentProcessor" in content
            assert "process_refund" in content
            assert "validate_webhook" in content

    def test_bundle_fixed_structure(self, tmp_path):
        """Test that bundle generates fixed structure with all sections"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create various baseline files
            files_to_create = {
                "architecture.md": "# Architecture\nSystem design",
                "apis.md": "# APIs\nREST endpoints",
                "config.yml": "# Configuration\napp:\n  debug: false",
                "schema.py": "# Database Schema\n# CREATE TABLE users (id INT);"
            }

            for filename, content in files_to_create.items():
                file_path = Path(filename)
                file_path.write_text(content)
                result = runner.invoke(cli, ['baseline', 'add', filename])
                assert result.exit_code == 0

            # Start session with task
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0

            # Generate bundle
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0

            # Check fixed structure
            context_file = Path(".context/context_for_ai.md")
            content = context_file.read_text()

            expected_sections = [
                "# Project Context for AI Tools",
                "*Generated by Context Engine V1*",
                "## Architecture",
                "## APIs",
                "## Configuration",
                "## Database Schema",
                "## Task",
                "## Session Notes",
                "## Cross-Repo Notes",
                "## Expanded Files"
            ]

            for section in expected_sections:
                assert section in content, f"Missing section: {section}"

            # Check section order
            arch_pos = content.find("## Architecture")
            apis_pos = content.find("## APIs")
            config_pos = content.find("## Configuration")
            schema_pos = content.find("## Database Schema")
            task_pos = content.find("## Task")

            assert arch_pos < apis_pos < config_pos < schema_pos < task_pos

    def test_bundle_token_count_warning(self, tmp_path):
        """Test bundle token count warning"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create a large baseline file that exceeds token limit but stays under file size limit
            # Create unique lines to avoid deduplication
            lines = []
            for i in range(8000):
                lines.append(f"This is unique line {i} with enough content to generate many tokens and test the token limit warning functionality.")
            large_content = "# Large Architecture\n" + "\n".join(lines)
            arch_file = Path("architecture.md")
            arch_file.write_text(large_content)

            result = runner.invoke(cli, ['baseline', 'add', 'architecture.md'])
            assert result.exit_code == 0

            # Start a session to enable compression (include full content)
            result = runner.invoke(cli, ['start-session', '--task', 'test task'])
            assert result.exit_code == 0

            # Generate bundle
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0

            # Check for token count and warning
            assert "Token count:" in result.output
            # Large file should trigger warning
            assert "Token count exceeds limit" in result.output


if __name__ == "__main__":
    pytest.main([__file__, "-v"])