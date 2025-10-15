"""End-to-end workflow tests for Context Engine CLI"""

import tempfile
from pathlib import Path
import pytest
from click.testing import CliRunner

from context_engine.cli import cli
from context_engine.core.task_manager import get_task


class TestEndToEndWorkflow:
    """Test complete end-to-end CLI workflow"""

    def test_complete_workflow_with_task(self, tmp_path):
        """Test the complete workflow: init -> baseline auto -> start-session -> bundle -> status -> stop-session"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Step 1: context init
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0
            assert "Initialized Context Engine in" in result.output

            # Verify .context directory was created
            context_dir = Path(".context")
            assert context_dir.exists()
            assert context_dir.is_dir()

            # Step 2: context baseline auto
            result = runner.invoke(cli, ['baseline', 'auto'])
            assert result.exit_code == 0

            # If no architecture doc exists, one should be created
            baseline_dir = context_dir / "baseline"
            assert baseline_dir.exists()

            # Step 3: Create some source files for compression
            src_dir = Path("src")
            src_dir.mkdir()

            # Create well-documented source files
            payment_py = src_dir / "payment.py"
            payment_py.write_text('''
"""Payment processing module for handling refund webhooks."""

from typing import Dict, Any, Optional
import logging

logger = logging.getLogger(__name__)

class PaymentProcessor:
    """Handles payment processing including refunds and webhooks."""

    def __init__(self, config: Dict[str, Any]):
        """Initialize payment processor with configuration.

        Args:
            config: Configuration containing API keys, endpoints, etc.
        """
        self.config = config
        self.api_key = config.get("api_key", "sk-proj-fake-key-for-testing")
        self.webhook_secret = config.get("webhook_secret", "whsec_test_secret")

    def process_refund(self, transaction_id: str, amount: float) -> Dict[str, Any]:
        """Process a refund for a given transaction.

        Args:
            transaction_id: Unique identifier for the transaction
            amount: Amount to refund

        Returns:
            Dict containing refund status and details
        """
        try:
            # Call payment processor API
            result = self._call_payment_api("refund", {
                "transaction_id": transaction_id,
                "amount": amount
            })
            return {"success": True, "refund_id": result.get("id")}
        except Exception as e:
            logger.error(f"Refund failed: {e}")
            return {"success": False, "error": str(e)}

    def validate_webhook(self, payload: Dict[str, Any], signature: str) -> bool:
        """Validate incoming webhook signature.

        Args:
            payload: Webhook payload data
            signature: HMAC signature to verify

        Returns:
            bool: True if webhook is valid
        """
        # Implementation would verify HMAC signature
        return True

    def _call_payment_api(self, endpoint: str, data: Dict[str, Any]) -> Dict[str, Any]:
        """Internal method to call payment API.

        Args:
            endpoint: API endpoint to call
            data: Data to send

        Returns:
            Dict containing API response
        """
        # Mock API call
        return {"id": "refund_12345", "status": "processed"}
''')

            # Create API documentation
            api_md = src_dir / "api.md"
            api_md.write_text('''
# Payment API Documentation

## Refund Webhook Endpoint

### POST /webhooks/refund

Process refund notifications from payment provider.

**Request Body:**
```json
{
  "event": "refund.completed",
  "data": {
    "transaction_id": "txn_123456",
    "amount": 99.99,
    "currency": "USD",
    "status": "completed"
  }
}
```

**Response:**
```json
{
  "status": "success",
  "refund_id": "rfn_789012"
}
```

### Security

- Webhooks are signed with HMAC-SHA256
- Verify signature using `webhook_secret` from configuration
- Replay attacks are prevented with timestamp validation
''')

            # Step 4: context start-session --task "implement refund webhook"
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0
            assert f"[OK] Task set: {task}" in result.output
            assert "Compressing source code for task..." in result.output

            # Verify session_task.txt was created
            task_file = context_dir / "session_task.txt"
            assert task_file.exists()
            assert task_file.read_text() == task

            # Verify compressed_src directory was created
            compressed_src_dir = context_dir / "compressed_src"
            assert compressed_src_dir.exists()

            # Step 5: context bundle
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0
            assert f"Task-based session detected: {task}" in result.output
            assert "Generated context bundle" in result.output

            # Verify context_for_ai.md was created
            context_file = context_dir / "context_for_ai.md"
            assert context_file.exists()

            # Check bundle content structure
            bundle_content = context_file.read_text()
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
                assert section in bundle_content, f"Missing section: {section}"

            # Check that task is included
            assert task in bundle_content

            # Check that compressed source is included
            assert "### Compressed Source Files" in bundle_content
            assert "Payment processing module" in bundle_content
            assert "PaymentProcessor" in bundle_content
            assert "process_refund" in bundle_content
            assert "validate_webhook" in bundle_content

            # Check that secrets are not present in compressed content (removed with implementation code)
            assert "sk-proj-fake-key-for-testing" not in bundle_content
            assert "whsec_test_secret" not in bundle_content

            # Step 6: context status
            result = runner.invoke(cli, ['status'])
            assert result.exit_code == 0
            assert "Tokens:" in result.output

            # Step 7: context stop-session
            result = runner.invoke(cli, ['stop-session'])
            assert result.exit_code == 0
            assert f"Stopping session for task: {task}" in result.output
            assert "Session stopped and task cleared." in result.output

            # Verify cleanup
            assert not task_file.exists()

    def test_workflow_without_task(self, tmp_path):
        """Test workflow without task (baseline-only mode)"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Step 1: context init
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Step 2: Create and add baseline files
            arch_file = Path("architecture.md")
            arch_file.write_text("# System Architecture\n\nRefund webhook system.")

            result = runner.invoke(cli, ['baseline', 'add', 'architecture.md'])
            assert result.exit_code == 0

            # Step 3: Skip start-session (no task)

            # Step 4: context bundle (should be baseline-only)
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0
            assert "⚠ Compression disabled — baseline-only mode." in result.output

            # Verify bundle was created
            context_file = Path(".context/context_for_ai.md")
            assert context_file.exists()

            # Check content
            bundle_content = context_file.read_text()
            assert "## Task" in bundle_content
            assert "None" in bundle_content
            assert "# System Architecture" in bundle_content

            # Should not have compressed source section
            assert "### Compressed Source Files" not in bundle_content

    def test_workflow_with_realistic_project(self, tmp_path):
        """Test workflow with realistic project structure"""
        runner = CliRunner()
        with runner.isolated_filesystem(temp_dir=tmp_path):
            # Step 1: context init
            result = runner.invoke(cli, ['init'])
            assert result.exit_code == 0

            # Step 2: Create realistic project structure
            project_structure = {
                "docs/": {
                    "architecture.md": "# Payment System Architecture\n\nWebhook-based refund processing.",
                    "apis.md": "# API Documentation\n\nRefund webhook endpoint documentation."
                },
                "config/": {
                    "config.yml": """
# Application Configuration
app:
  name: payment-processor
  version: 1.0.0

payment:
  provider: stripe
  api_key: ${STRIPE_API_KEY}  # Will be redacted
  webhook_secret: ${WEBHOOK_SECRET}  # Will be redacted

database:
  host: localhost
  port: 5432
  name: payment_db
  user: admin
  password: ${DB_PASSWORD}  # Will be redacted
""",
                    "development.yml": """
# Development configuration
debug: true
log_level: DEBUG
test_mode: true
"""
                },
                "src/": {
                    "main.py": '''
"""Main application entry point."""

from payment_processor import PaymentProcessor
from config import load_config

def main():
    """Main application entry point."""
    config = load_config()
    processor = PaymentProcessor(config)
    processor.start_webhook_server()

if __name__ == "__main__":
    main()
''',
                    "payment_processor.py": '''
"""Core payment processing logic."""

class PaymentProcessor:
    """Handles payment processing and webhook handling."""

    def __init__(self, config):
        """Initialize with configuration."""
        self.config = config
        self.api_key = config.get("payment", {}).get("api_key")
        self.webhook_secret = config.get("payment", {}).get("webhook_secret")

    def process_refund_webhook(self, webhook_data):
        """Process incoming refund webhook."""
        # Validate webhook signature
        if not self._validate_webhook(webhook_data):
            return {"status": "error", "message": "Invalid webhook"}

        # Process refund
        return self._process_refund(webhook_data)

    def _validate_webhook(self, data):
        """Validate webhook signature."""
        # Implementation would verify HMAC signature
        return True

    def _process_refund(self, data):
        """Process the actual refund."""
        # Implementation would call payment provider API
        return {"status": "success", "refund_id": "rfn_12345"}
''',
                    "config.py": '''
"""Configuration management."""

import os
import yaml

def load_config():
    """Load configuration from YAML files."""
    config_path = os.path.join("config", "app.yml")
    with open(config_path, 'r') as f:
        return yaml.safe_load(f)
''',
                    "webhooks/": {
                        "refund_webhook.py": '''
"""Refund webhook handler."""

from flask import Flask, request, jsonify
from payment_processor import PaymentProcessor

app = Flask(__name__)
processor = PaymentProcessor(load_config())

@app.route('/webhooks/refund', methods=['POST'])
def handle_refund_webhook():
    """Handle refund webhook from payment provider."""
    try:
        webhook_data = request.get_json()
        result = processor.process_refund_webhook(webhook_data)
        return jsonify(result)
    except Exception as e:
        return jsonify({"status": "error", "message": str(e)}), 500
'''
                    }
                }
            }

            # Create project structure
            def create_structure(base_path, structure):
                for name, content in structure.items():
                    if name.endswith("/"):
                        # Directory
                        dir_path = base_path / name.rstrip("/")
                        dir_path.mkdir(parents=True, exist_ok=True)
                        if isinstance(content, dict):
                            create_structure(dir_path, content)
                    else:
                        # File
                        file_path = base_path / name
                        file_path.parent.mkdir(parents=True, exist_ok=True)
                        file_path.write_text(content if isinstance(content, str) else "")

            create_structure(Path.cwd(), project_structure)

            # Step 3: Add baseline files
            result = runner.invoke(cli, ['baseline', 'add', 'docs/architecture.md', 'docs/apis.md', 'config/config.yml'])
            assert result.exit_code == 0

            # Step 4: context baseline auto (should detect existing files)
            result = runner.invoke(cli, ['baseline', 'auto'])
            assert result.exit_code == 0
            assert "Architecture document already present" in result.output

            # Step 5: Start session
            task = "implement refund webhook"
            result = runner.invoke(cli, ['start-session', '--task', task])
            assert result.exit_code == 0

            # Step 6: context bundle
            result = runner.invoke(cli, ['bundle', '--no-ai'])
            assert result.exit_code == 0

            # Verify comprehensive bundle
            context_file = Path(".context/context_for_ai.md")
            bundle_content = context_file.read_text()

            # Check architecture and API docs are included
            assert "Payment System Architecture" in bundle_content
            assert "API Documentation" in bundle_content
            assert "Refund webhook endpoint" in bundle_content

            # Check compressed source code is included
            assert "### Compressed Source Files" in bundle_content
            assert "Main application entry point" in bundle_content
            assert "Core payment processing logic" in bundle_content
            assert "PaymentProcessor" in bundle_content
            assert "process_refund_webhook" in bundle_content

            # Check configuration is included
            assert "## Configuration" in bundle_content
            assert "payment-processor" in bundle_content

            # Check secrets were redacted
            assert "STRIPE_API_KEY" not in bundle_content
            assert "WEBHOOK_SECRET" not in bundle_content
            assert "DB_PASSWORD" not in bundle_content

            # Step 7: context status
            result = runner.invoke(cli, ['status'])
            assert result.exit_code == 0
            assert "Tokens:" in result.output

            # Step 8: context stop-session
            result = runner.invoke(cli, ['stop-session'])
            assert result.exit_code == 0

            # Verify cleanup
            task_file = Path(".context/session_task.txt")
            assert not task_file.exists()


if __name__ == "__main__":
    pytest.main([__file__, "-v"])