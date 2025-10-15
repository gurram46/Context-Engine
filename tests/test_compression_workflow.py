"""Compression workflow tests for Context Engine"""

import tempfile
from pathlib import Path
import pytest
from click.testing import CliRunner

from context_engine.cli import cli
from context_engine.core.task_manager import get_task, clear_task
from context_engine.compressors.compress_src import (
    compress_for_task,
    _generate_compressed_content,
    _should_skip_file,
    _get_language_from_ext
)


class TestCompressionWorkflow:
    """Test compression workflow with and without tasks"""

    def test_compress_with_task(self, tmp_path):
        """Test compression when task is set"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create src directory with files
            src_dir = Path("src")
            src_dir.mkdir()

            # Create test files with documentation
            test_files = {
                "payment.py": '''
"""Payment processing module."""

class PaymentProcessor:
    """Handles payment processing."""

    def process_payment(self, amount):
        """Process payment transaction."""
        return True
''',
                "api.js": '''
/**
 * Payment API endpoints
 */

/**
 * Process a payment
 * @param {Object} paymentData - Payment data
 * @returns {Promise<Object>} Payment result
 */
function processPayment(paymentData) {
    // Implementation
}
''',
                "config.yaml": '''
# Payment configuration
payment:
  provider: stripe
  api_key: ${STRIPE_KEY}
'''
            }

            for filename, content in test_files.items():
                file_path = src_dir / filename
                file_path.write_text(content)

            # Set task and compress
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0

            # Check compressed_src directory was created
            compressed_src_dir = Path(".context/compressed_src")
            assert compressed_src_dir.exists()

            # Check compressed files were created
            compressed_files = list(compressed_src_dir.glob("*.md"))
            assert len(compressed_files) == 1

            # Check content of compressed file
            compressed_content = compressed_files[0].read_text()
            assert "Compressed Source Files for Task" in compressed_content
            assert task in compressed_content
            assert "Payment processing module" in compressed_content
            assert "PaymentProcessor" in compressed_content
            assert "processPayment" in compressed_content

    def test_compress_without_task(self, tmp_path):
        """Test compression when no task is set"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create src directory
            src_dir = Path("src")
            src_dir.mkdir()
            test_file = src_dir / "test.py"
            test_file.write_text('print("hello")')

            # Try to compress without task
            compress_for_task()

            # No compression should occur
            compressed_src_dir = Path(".context/compressed_src")
            assert not compressed_src_dir.exists()

    def test_compress_no_src_directory(self, tmp_path):
        """Test compression when src directory doesn't exist"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Set task but don't create src directory
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0

            # Check that compression was skipped
            assert "src/ directory not found. Skipping compression." in result.output

            # No compressed_src directory should be created
            compressed_src_dir = Path(".context/compressed_src")
            assert not compressed_src_dir.exists()

    def test_compress_skips_binary_files(self, tmp_path):
        """Test that binary files are skipped during compression"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Initialize context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Create src directory with mixed file types
            src_dir = Path("src")
            src_dir.mkdir()

            # Create various files
            files_to_create = {
                "main.py": '"""Main module."""\ndef main():\n    pass',
                "image.png": b"fake png data",  # Binary file
                "data.csv": "name,age\nJohn,30",  # Text file
                "archive.zip": b"fake zip data",  # Binary file
                "README.md": "# Project README",  # Markdown file
                "config.json": '{"key": "value"}',  # JSON file
                "test.pyc": b"compiled python",  # Should be skipped
                ".gitignore": "*.pyc\n__pycache__/"  # Should be processed
            }

            for filename, content in files_to_create.items():
                file_path = src_dir / filename
                if isinstance(content, bytes):
                    file_path.write_bytes(content)
                else:
                    file_path.write_text(content)

            # Set task and compress
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0

            # Check compressed file was created
            compressed_src_dir = Path(".context/compressed_src")
            assert compressed_src_dir.exists()

            compressed_files = list(compressed_src_dir.glob("*.md"))
            assert len(compressed_files) == 1

            # Check that only text files were included
            compressed_content = compressed_files[0].read_text()
            assert "main.py" in compressed_content
            assert "README.md" in compressed_content
            assert "config.json" in compressed_content
            assert "data.csv" in compressed_content
            assert ".gitignore" in compressed_content

            # Binary files should be skipped
            assert "image.png" not in compressed_content
            assert "archive.zip" not in compressed_content
            assert "test.pyc" not in compressed_content

    def test_compress_language_detection(self, tmp_path):
        """Test language detection for different file types"""
        assert _get_language_from_ext(".py") == "python"
        assert _get_language_from_ext(".js") == "javascript"
        assert _get_language_from_ext(".ts") == "typescript"
        assert _get_language_from_ext(".java") == "java"
        assert _get_language_from_ext(".cpp") == "cpp"
        assert _get_language_from_ext(".md") == "markdown"
        assert _get_language_from_ext(".yaml") == "yaml"
        assert _get_language_from_ext(".unknown") == ""

    def test_compress_skip_file_patterns(self, tmp_path):
        """Test file skip patterns"""
        # Files that should be skipped
        skip_files = [
            "test.pyc",
            "test.pyo",
            "test.class",
            "test.jar",
            "test.exe",
            "test.dll",
            "test.png",
            "test.jpg",
            "test.pdf",
            "test.zip",
            "test.log",
            "test.tmp",
            "__pycache__/test.py",
            ".git/config",
            "node_modules/package/test.js"
        ]

        for filename in skip_files:
            assert _should_skip_file(filename), f"Should skip: {filename}"

        # Files that should not be skipped
        keep_files = [
            "main.py",
            "app.js",
            "config.yml",
            "README.md",
            "data.csv",
            "Makefile"
        ]

        for filename in keep_files:
            assert not _should_skip_file(filename), f"Should keep: {filename}"

    def test_compress_docstring_extraction(self, tmp_path):
        """Test that only docstrings are extracted from code"""
        src_dir = Path("src")
        src_dir.mkdir(exist_ok=True)

        # Create a Python file with both code and docstrings
        py_file = src_dir / "payment.py"
        py_file.write_text('''
"""Payment processing module."""

import os

class PaymentProcessor:
    """Handles payment processing."""

    def __init__(self):
        """Initialize processor."""
        self.config = {}  # This should be removed
        self.api_key = "sk-123"  # This should be redacted

    def process_payment(self, amount):
        """Process payment transaction.

        Args:
            amount: Payment amount

        Returns:
            bool: Success status
        """
        # This implementation code should be removed
        result = self._call_api(amount)
        return result

    def _call_api(self, amount):
        """Internal API call.

        This should be kept as it's a docstring.
        """
        return True  # This code should be removed
''')

        # Generate compressed content
        task = "implement refund webhook"
        content = _generate_compressed_content(src_dir, task)

        # Check that docstrings are preserved
        assert "Payment processing module" in content
        assert "Handles payment processing" in content
        assert "Initialize processor" in content
        assert "Process payment transaction" in content
        assert "Internal API call" in content

        # Check that implementation code is removed
        assert "self.config = {}" not in content
        assert "self.api_key" not in content
        assert "result = self._call_api(amount)" not in content
        assert "return True" not in content
        assert "import os" not in content

        # Check that API key is redacted
        assert "sk-123" not in content

    def test_compress_secret_redaction(self, tmp_path):
        """Test secret redaction during compression"""
        src_dir = Path("src")
        src_dir.mkdir(exist_ok=True)

        # Create files with fake secrets in docstrings
        py_file = src_dir / "config.py"
        py_file.write_text('''
"""Configuration module.

Note: Uses API_KEY sk-proj-1234567890abcdefghij for authentication.
"""

class ConfigManager:
    """Manages configuration with DB_PASSWORD super_secret_password_123."""

    def __init__(self):
        """Initialize with AWS_ACCESS_KEY AKIAIOSFODNN7EXAMPLE."""
        pass
''')

        js_file = src_dir / "api.js"
        js_file.write_text('''
/**
 * API client configuration
 *
 * Uses bearer_token_1234567890 for authentication.
 *
 * @param {string} API_TOKEN - The authentication token
 */

/**
 * Normal function without secrets
 * @returns {Object} Configuration object
 */
function getConfig() {
    return {
        url: "https://api.example.com",
        timeout: 5000
    };
}
''')

        # Generate compressed content
        task = "implement refund webhook"
        content = _generate_compressed_content(src_dir, task)

        # Check that secrets are redacted in docstrings
        assert "[REDACTED]" in content or "[REDACTED_KEY]" in content or "[REDACTED_AWS]" in content
        assert "super_secret_password_123" not in content
        assert "sk-proj-1234567890abcdefghij" not in content
        assert "AKIAIOSFODNN7EXAMPLE" not in content
        assert "bearer_token_1234567890" not in content

        # Check that normal documentation is preserved
        assert "Configuration module" in content
        assert "ConfigManager" in content
        assert "API client configuration" in content
        assert "authentication token" in content
        assert "getConfig" in content


if __name__ == "__main__":
    pytest.main([__file__, "-v"])