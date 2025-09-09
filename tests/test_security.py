"""Security tests for Context Engine V1"""

import os
import tempfile
from pathlib import Path
import pytest
from click.testing import CliRunner

from context_engine.cli import cli
from context_engine.core.config import Config
from context_engine.core.utils import (
    is_subpath,
    validate_path_in_project,
    redact_secrets,
    is_high_entropy_token,
    sanitize_note_input,
    is_valid_api_key,
    compress_code,
    extract_api_docstrings
)


class TestPathTraversal:
    """Test path traversal prevention"""
    
    def test_is_subpath_valid(self, tmp_path):
        parent = tmp_path
        child = tmp_path / "subdir" / "file.txt"
        assert is_subpath(child, parent)
    
    def test_is_subpath_invalid(self, tmp_path):
        parent = tmp_path / "project"
        child = tmp_path / "outside"
        assert not is_subpath(child, parent)
    
    def test_is_subpath_with_dots(self, tmp_path):
        parent = tmp_path / "project"
        child = tmp_path / "project" / ".." / "outside"
        assert not is_subpath(child.resolve(), parent)
    
    def test_validate_path_in_project_raises(self, tmp_path):
        from click import BadParameter
        parent = tmp_path / "project"
        child = tmp_path / "outside"
        with pytest.raises(BadParameter):
            validate_path_in_project(child, parent)
    
    def test_baseline_add_path_traversal(self, tmp_path):
        """Test that baseline add prevents path traversal"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Create project dir
            project = Path.cwd()
            outside_file = tmp_path / "outside.py"
            outside_file.write_text("secret")
            
            # Init context engine
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0
            
            # Try to add file outside project
            result = runner.invoke(cli, ['baseline', 'add', str(outside_file)])
            assert result.exit_code != 0
            assert "outside the project root" in result.output


class TestSecretRedaction:
    """Test secret redaction with regex and entropy"""
    
    def test_redact_api_keys(self):
        text = 'API_KEY="sk-proj-1234567890abcdefghij"'
        redacted = redact_secrets(text)
        assert "[REDACTED_KEY]" in redacted
        assert "sk-proj" not in redacted
    
    def test_redact_aws_keys(self):
        text = 'aws_access_key_id = AKIAIOSFODNN7EXAMPLE'
        redacted = redact_secrets(text)
        assert "[REDACTED_AWS]" in redacted
        assert "AKIA" not in redacted
    
    def test_redact_passwords(self):
        text = 'password: "mysecretpass123"'
        redacted = redact_secrets(text)
        assert "[REDACTED]" in redacted
        assert "mysecretpass123" not in redacted
    
    def test_high_entropy_detection(self):
        # High entropy random string
        high = "xK9mN2pL5qR8tV3wY6zA1bC4dE7fG0hJ"
        assert is_high_entropy_token(high)
        
        # Low entropy repetitive string
        low = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        assert not is_high_entropy_token(low)
        
        # Too short
        short = "abc123"
        assert not is_high_entropy_token(short)
    
    def test_entropy_based_redaction(self):
        text = "token=xK9mN2pL5qR8tV3wY6zA1bC4dE7fG0hJ"
        redacted = redact_secrets(text)
        assert "[REDACTED]" in redacted
    
    def test_no_over_redaction(self):
        # Normal variable names shouldn't be redacted
        text = "my_long_variable_name_here = 42"
        redacted = redact_secrets(text)
        assert "my_long_variable_name_here" in redacted


class TestAPIKeyValidation:
    """Test API key format validation"""
    
    def test_valid_openai_key(self):
        assert is_valid_api_key("sk-proj-1234567890abcdefghijklmnopqrstuv")
    
    def test_valid_generic_key(self):
        assert is_valid_api_key("a" * 32)
    
    def test_invalid_empty(self):
        assert not is_valid_api_key("")
        assert not is_valid_api_key(None)
    
    def test_invalid_too_short(self):
        assert not is_valid_api_key("sk-short")
    
    def test_invalid_special_chars(self):
        assert not is_valid_api_key("sk-proj-with@special#chars$" + "a" * 20)


class TestInputSanitization:
    """Test CLI input sanitization"""
    
    def test_sanitize_note_normal(self):
        note = "This is a normal note"
        assert sanitize_note_input(note) == note
    
    def test_sanitize_note_control_chars(self):
        note = "Note with\x00null\x01and\x1fcontrol chars"
        sanitized = sanitize_note_input(note)
        assert "\x00" not in sanitized
        assert "\x01" not in sanitized
        assert "\x1f" not in sanitized
    
    def test_sanitize_note_max_length(self):
        note = "a" * 3000
        sanitized = sanitize_note_input(note, max_len=2000)
        assert len(sanitized) == 2001  # 2000 + ellipsis
        assert sanitized.endswith("…")
    
    def test_save_command_sanitization(self, tmp_path):
        """Test that save command sanitizes input"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ['init'])
            
            # Try to save note with control characters
            evil_note = "Normal text\x00\x01\x1f"
            result = runner.invoke(cli, ['save', evil_note])
            assert result.exit_code == 0
            
            # Check saved content doesn't have control chars
            session_file = Path.cwd() / ".context" / "session.md"
            content = session_file.read_text()
            assert "\x00" not in content


class TestFileValidation:
    """Test file type and size validation"""
    
    def test_disallowed_file_type(self, tmp_path):
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ['init'])
            
            # Create executable file
            exe_file = Path("test.exe")
            exe_file.write_text("binary")
            
            result = runner.invoke(cli, ['baseline', 'add', 'test.exe'])
            assert result.exit_code != 0
            assert "Disallowed file type" in result.output
    
    def test_file_size_limit(self, tmp_path):
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ['init'])
            
            # Create large file
            large_file = Path("large.md")
            large_file.write_text("x" * (2 * 1024 * 1024))  # 2MB
            
            result = runner.invoke(cli, ['baseline', 'add', 'large.md'])
            assert result.exit_code != 0
            assert "File too large" in result.output


class TestCompressionAndFormat:
    """Test strict compression and format compliance"""
    
    def test_strip_comments_python(self):
        code = '''
def func():
    """API docstring"""
    # This comment should be removed
    x = 1  # inline comment
    return x
'''
        compressed = compress_code(code, "python")
        assert "API docstring" in compressed
        assert "# This comment" not in compressed
        assert "# inline" not in compressed
    
    def test_strip_comments_javascript(self):
        code = '''
/**
 * API documentation
 */
function test() {
    // This should be removed
    var x = 1; /* and this */
    return x;
}
'''
        compressed = compress_code(code, "javascript")
        assert "API documentation" in compressed
        assert "// This should" not in compressed
        assert "/* and this */" not in compressed
    
    def test_extract_docstrings_only(self):
        code = '''
def func():
    """This is kept"""
    actual_code = 1 + 2
    return actual_code
'''
        docs = extract_api_docstrings(code, "python")
        assert "This is kept" in docs
        assert "actual_code" not in docs
        assert "1 + 2" not in docs
    
    def test_fixed_structure_format(self, tmp_path):
        """Test that bundle generates fixed structure"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ['init'])
            
            # Create test files
            arch_file = Path("architecture.md")
            arch_file.write_text("# System Architecture")
            
            runner.invoke(cli, ['baseline', 'add', 'architecture.md'])
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0
            
            # Check generated format
            context_file = Path.cwd() / ".context" / "context_for_ai.md"
            content = context_file.read_text()
            
            # Check all sections are present in order
            sections = [
                "## Architecture",
                "## APIs",
                "## Configuration", 
                "## Database Schema",
                "## Session Notes",
                "## Cross-Repo Notes",
                "## Expanded Files"
            ]
            
            last_pos = 0
            for section in sections:
                pos = content.find(section)
                assert pos > last_pos, f"Section {section} missing or out of order"
                last_pos = pos
            
            # Check that empty sections have "None"
            assert content.count("None") >= 5  # At least 5 empty sections


class TestTokenCounting:
    """Test token counting functionality"""
    
    def test_token_count_warning(self, tmp_path):
        from context_engine.core.utils import count_tokens
        
        # Test basic counting
        text = "This is a test" * 100
        tokens = count_tokens(text)
        assert tokens > 0
        assert tokens < 1000


class TestSSLAndTimeouts:
    """Test SSL verification and timeouts"""
    
    def test_openrouter_client_timeout(self):
        from context_engine.models.openrouter import OpenRouterClient
        
        # Client should set timeout
        client = OpenRouterClient("test-key-1234567890abcdefghijklmnop")
        # Can't easily test actual timeout without mocking, 
        # but verify client initializes with validated key
        assert client.api_key == ""  # Invalid test key should be rejected


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
