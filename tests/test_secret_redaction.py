"""Secret redaction tests for Context Engine"""

import tempfile
from pathlib import Path
import pytest
from click.testing import CliRunner

from context_engine.cli import cli
from context_engine.core.utils import redact_secrets


class TestSecretRedaction:
    """Test secret redaction functionality"""

    def test_redact_openai_keys(self):
        """Test OpenAI API key redaction"""
        # Test various formats of OpenAI keys
        test_cases = [
            ('API_KEY="sk-proj-1234567890abcdefghij"', 'API_KEY="[REDACTED_KEY]"'),
            ("API_KEY='sk-proj-1234567890abcdefghij'", "API_KEY='[REDACTED_KEY]'"),
            ('API_KEY=sk-proj-1234567890abcdefghij', 'API_KEY=[REDACTED_KEY]'),
            ('"sk-proj-1234567890abcdefghij"', '"[REDACTED_KEY]"'),
            ("sk-proj-1234567890abcdefghij", "[REDACTED_KEY]"),  # Standalone
        ]

        for input_text, expected_output in test_cases:
            result = redact_secrets(input_text)
            assert result == expected_output, f"Failed for: {input_text}"

    def test_redact_aws_keys(self):
        """Test AWS access key redaction"""
        test_cases = [
            ('aws_access_key_id = AKIAIOSFODNN7EXAMPLE', 'aws_access_key_id = [REDACTED_AWS]'),
            ('"AKIAIOSFODNN7EXAMPLE"', '"[REDACTED_AWS]"'),
            ("'AKIAIOSFODNN7EXAMPLE'", "'[REDACTED_AWS]'"),
            ('AWS_KEY: AKIAIOSFODNN7EXAMPLE', 'AWS_KEY: [REDACTED_AWS]'),
        ]

        for input_text, expected_output in test_cases:
            result = redact_secrets(input_text)
            assert result == expected_output, f"Failed for: {input_text}"

    def test_redact_passwords(self):
        """Test password redaction"""
        test_cases = [
            ('password: "mysecretpass123"', 'password: [REDACTED]'),
            ("password: 'mysecretpass123'", "password: [REDACTED]"),
            ('password = mysecretpass123', 'password = [REDACTED]'),
            ('passwd: secret123', 'passwd: [REDACTED]'),
            ('pwd = "password!"', 'pwd = [REDACTED]'),
            ('PASS = mypass', 'PASS = [REDACTED]'),
        ]

        for input_text, expected_output in test_cases:
            result = redact_secrets(input_text)
            assert result == expected_output, f"Failed for: {input_text}"

    def test_redact_tokens_and_bearers(self):
        """Test token and bearer token redaction"""
        test_cases = [
            ('token: "abc123def456ghi789"', 'token: [REDACTED]'),
            ('jwt: "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"', 'jwt: [REDACTED]'),
            ('bearer: "Bearer eyJhbGciOiJIUzI1NiIs"', 'bearer: [REDACTED]'),
            ('auth_token = xyz789abc123', 'auth_token = [REDACTED]'),
        ]

        for input_text, expected_output in test_cases:
            result = redact_secrets(input_text)
            assert result == expected_output, f"Failed for: {input_text}"

    def test_redact_environment_variables(self):
        """Test environment variable redaction"""
        test_cases = [
            ('API_KEY=sk-proj-1234567890abcdefghij', 'API_KEY=[REDACTED_KEY]'),
            ('SECRET_KEY = "supersecret"', 'SECRET_KEY = [REDACTED]'),
            ('TOKEN=abc123def456', 'TOKEN=[REDACTED]'),
            ('PASSWORD=mypass123', 'PASSWORD=[REDACTED]'),
            ('PASSWD=secret', 'PASSWD=[REDACTED]'),
        ]

        for input_text, expected_output in test_cases:
            result = redact_secrets(input_text)
            assert result == expected_output, f"Failed for: {input_text}"

    def test_redact_entropy_based_secrets(self):
        """Test entropy-based secret redaction"""
        # High entropy strings should be redacted
        high_entropy_strings = [
            "xK9mN2pL5qR8tV3wY6zA1bC4dE7fG0hJ",  # High entropy
            "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6",  # High entropy hex-like
            "sk-proj-1234567890abcdefghijklmnopqrstuv",  # OpenAI key (already covered by regex)
        ]

        for secret in high_entropy_strings:
            result = redact_secrets(f"token={secret}")
            assert "[REDACTED]" in result, f"Should redact: {secret}"

        # Low entropy strings should not be redacted
        low_entropy_strings = [
            "my_variable_name",
            "password123",  # Too short and low entropy
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",  # Repetitive
            "test_value",
            "config_string",
            "database_name",
        ]

        for safe_string in low_entropy_strings:
            result = redact_secrets(f"var={safe_string}")
            assert safe_string in result, f"Should not redact: {safe_string}"

    def test_no_over_redaction(self):
        """Test that normal code is not over-redacted"""
        code_samples = [
            # Normal variables and function names
            "def calculate_total(amount, tax_rate):",
            "my_long_variable_name_here = 42",
            "config.database_connection_string = 'localhost:5432'",
            "user_api_endpoint = 'https://api.example.com/users'",
            "max_retry_count = 3",

            # URLs and safe strings
            "https://github.com/user/repo",
            "localhost:8080",
            "user@example.com",
            "file_path = '/path/to/file.txt'",

            # Configuration values
            "DEBUG = True",
            "PORT = 8000",
            "TIMEOUT = 30",
            "MAX_CONNECTIONS = 100",
        ]

        for code in code_samples:
            result = redact_secrets(code)
            assert result == code, f"Over-redacted: {code}"

    def test_redact_in_real_config(self, tmp_path):
        """Test secret redaction in realistic configuration files"""
        config_content = '''
# Application Configuration

# Database settings
DB_HOST = "localhost"
DB_PORT = 5432
DB_NAME = "payment_db"
DB_USER = "admin"
DB_PASSWORD = "super_secret_password_123"  # This should be redacted

# API Configuration
API_BASE_URL = "https://api.example.com"
API_KEY = "sk-proj-1234567890abcdefghij"  # This should be redacted
API_TIMEOUT = 30

# AWS Configuration
AWS_REGION = "us-east-1"
AWS_ACCESS_KEY_ID = "AKIAIOSFODNN7EXAMPLE"  # This should be redacted
AWS_SECRET_ACCESS_KEY = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"  # This should be redacted

# JWT Configuration
JWT_SECRET = "your-256-bit-secret"  # This should be redacted
JWT_EXPIRY = 3600

# Normal configuration
DEBUG = False
LOG_LEVEL = "INFO"
MAX_RETRIES = 3
'''

        expected_redacted = '''
# Application Configuration

# Database settings
DB_HOST = "localhost"
DB_PORT = 5432
DB_NAME = "payment_db"
DB_USER = "admin"
DB_PASSWORD = [REDACTED]  # This should be redacted

# API Configuration
API_BASE_URL = "https://api.example.com"
API_KEY = [REDACTED_KEY]  # This should be redacted
API_TIMEOUT = 30

# AWS Configuration
AWS_REGION = "us-east-1"
AWS_ACCESS_KEY_ID = [REDACTED_AWS]  # This should be redacted
AWS_SECRET_ACCESS_KEY = [REDACTED_AWS]  # This should be redacted

# JWT Configuration
JWT_SECRET = [REDACTED]  # This should be redacted
JWT_EXPIRY = 3600

# Normal configuration
DEBUG = False
LOG_LEVEL = "INFO"
MAX_RETRIES = 3
'''

        result = redact_secrets(config_content)
        assert result == expected_redacted

    def test_redact_in_code_files(self, tmp_path):
        """Test secret redaction in code files"""
        python_code = '''
"""Payment processing module."""

import os

class PaymentProcessor:
    """Handles payment processing."""

    def __init__(self):
        """Initialize processor."""
        self.api_key = "sk-proj-1234567890abcdefghij"  # This should be redacted
        self.db_password = "database_secret_123"  # This should be redacted
        self.config = {
            "timeout": 30,
            "retries": 3,
            "auth_token": "bearer_token_1234567890"  # This should be redacted
        }

    def process_payment(self, amount):
        """Process payment transaction."""
        # Normal variables should not be redacted
        payment_amount = amount * 1.1  # Include tax
        return self._call_api(payment_amount)

    def _call_api(self, amount):
        """Internal API call."""
        # This is a normal comment, not a secret
        endpoint = "https://api.example.com/payment"
        headers = {"Content-Type": "application/json"}
        return {"success": True, "amount": amount}
'''

        result = redact_secrets(python_code)

        # Check that secrets are redacted
        assert "[REDACTED_KEY]" in result
        assert "[REDACTED]" in result

        # Check that secrets are not present
        assert "sk-proj-1234567890abcdefghij" not in result
        assert "database_secret_123" not in result
        assert "bearer_token_1234567890" not in result

        # Check that normal code is preserved
        assert "import os" in result
        assert "class PaymentProcessor:" in result
        assert "def __init__(self):" in result
        assert "self.config = {" in result
        assert '"timeout": 30' in result
        assert "payment_amount = amount * 1.1" in result
        assert "endpoint = \"https://api.example.com/payment\"" in result

    def test_redact_with_multiple_secrets(self, tmp_path):
        """Test redaction when multiple secrets are present"""
        text_with_secrets = '''
Configuration:
API_KEY=sk-proj-1234567890abcdefghij
DB_PASSWORD=mysecretpassword
AWS_KEY=AKIAIOSFODNN7EXAMPLE
BEARER_TOKEN=xyz789abc123def456

Normal variables:
APP_NAME=payment-processor
DEBUG=True
PORT=8000
'''

        result = redact_secrets(text_with_secrets)

        # Check that all secrets are redacted
        assert "API_KEY=[REDACTED_KEY]" in result
        assert "DB_PASSWORD=[REDACTED]" in result
        assert "AWS_KEY=[REDACTED_AWS]" in result
        assert "BEARER_TOKEN=[REDACTED]" in result

        # Check that normal variables are preserved
        assert "APP_NAME=payment-processor" in result
        assert "DEBUG=True" in result
        assert "PORT=8000" in result

        # Check that secrets are not present
        assert "sk-proj-1234567890abcdefghij" not in result
        assert "mysecretpassword" not in result
        assert "AKIAIOSFODNN7EXAMPLE" not in result
        assert "xyz789abc123def456" not in result


if __name__ == "__main__":
    pytest.main([__file__, "-v"])