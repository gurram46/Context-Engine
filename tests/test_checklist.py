"""Test cases for checklist command functionality."""

import unittest
import tempfile
import json
from pathlib import Path
from unittest.mock import patch, MagicMock

from context_engine.commands.checklist import checklist_command
from context_engine.core.config import ContextConfig

class TestChecklistCommand(unittest.TestCase):
    """Test cases for checklist command."""
    
    def setUp(self):
        """Set up test environment."""
        self.temp_dir = tempfile.mkdtemp()
        self.project_path = Path(self.temp_dir)
        
        # Create mock config
        self.mock_config = MagicMock(spec=ContextConfig)
        self.mock_config.project_root = self.project_path
        
    def tearDown(self):
        """Clean up test environment."""
        import shutil
        shutil.rmtree(self.temp_dir)
    
    def create_test_files(self, files_dict):
        """Create test files in the project directory."""
        for file_path, content in files_dict.items():
            full_path = self.project_path / file_path
            full_path.parent.mkdir(parents=True, exist_ok=True)
            full_path.write_text(content)
    
    @patch('context_engine.commands.checklist.ContextConfig.load')
    def test_checklist_all_present(self, mock_config_load):
        """Test checklist when all documentation is present."""
        mock_config_load.return_value = self.mock_config
        
        # Create all required documentation
        files = {
            'docs/adr/001-architecture-decision.md': '# ADR 001: Architecture Decision',
            'docs/architecture.md': '# System Architecture',
            'docs/user-flows.md': '# User Flows',
            'docs/api-spec.yaml': 'openapi: 3.0.0',
            'docs/team-roles.md': '# Team Roles and Responsibilities',
            'README.md': '# Project README'
        }
        self.create_test_files(files)
        
        # Mock args
        args = MagicMock()
        args.output = None
        args.format = 'text'
        
        with patch('builtins.print') as mock_print:
            result = checklist_command(args)
            
            self.assertEqual(result, 0)
            # Check that success messages were printed
            print_calls = [call[0][0] for call in mock_print.call_args_list]
            self.assertTrue(any('✓' in call for call in print_calls))
    
    @patch('context_engine.commands.checklist.ContextConfig.load')
    def test_checklist_missing_docs(self, mock_config_load):
        """Test checklist when documentation is missing."""
        mock_config_load.return_value = self.mock_config
        
        # Create only some documentation
        files = {
            'README.md': '# Project README'
        }
        self.create_test_files(files)
        
        # Mock args
        args = MagicMock()
        args.output = None
        args.format = 'text'
        
        with patch('builtins.print') as mock_print:
            result = checklist_command(args)
            
            self.assertEqual(result, 0)  # Command succeeds but shows missing items
            # Check that missing items were reported
            print_calls = [call[0][0] for call in mock_print.call_args_list]
            self.assertTrue(any('✗' in call for call in print_calls))
    
    @patch('context_engine.commands.checklist.ContextConfig.load')
    def test_checklist_json_output(self, mock_config_load):
        """Test checklist with JSON output format."""
        mock_config_load.return_value = self.mock_config
        
        files = {
            'docs/adr/001-test.md': '# ADR 001',
            'README.md': '# Project README'
        }
        self.create_test_files(files)
        
        # Mock args
        args = MagicMock()
        args.output = None
        args.format = 'json'
        
        with patch('builtins.print') as mock_print:
            result = checklist_command(args)
            
            self.assertEqual(result, 0)
            # Check that JSON was printed
            print_calls = [call[0][0] for call in mock_print.call_args_list]
            json_output = None
            for call in print_calls:
                try:
                    json_output = json.loads(call)
                    break
                except (json.JSONDecodeError, TypeError):
                    continue
            
            self.assertIsNotNone(json_output)
            self.assertIn('checklist_results', json_output)
    
    @patch('context_engine.commands.checklist.ContextConfig.load')
    def test_checklist_output_file(self, mock_config_load):
        """Test checklist with output to file."""
        mock_config_load.return_value = self.mock_config
        
        files = {
            'README.md': '# Project README'
        }
        self.create_test_files(files)
        
        output_file = self.project_path / 'checklist_output.json'
        
        # Mock args
        args = MagicMock()
        args.output = str(output_file)
        args.format = 'json'
        
        result = checklist_command(args)
        
        self.assertEqual(result, 0)
        self.assertTrue(output_file.exists())
        
        # Verify output file content
        output_data = json.loads(output_file.read_text())
        self.assertIn('checklist_results', output_data)
    
    def test_checklist_patterns(self):
        """Test that checklist patterns correctly identify files."""
        from context_engine.commands.checklist import ChecklistChecker
        
        checker = ChecklistChecker(self.project_path)
        
        # Test ADR pattern
        files = {
            'docs/adr/001-decision.md': '# ADR 001',
            'docs/decisions/002-choice.md': '# Decision 002',
            'architecture-decisions.md': '# Architecture Decisions'
        }
        self.create_test_files(files)
        
        adr_files = checker._find_files_by_patterns(checker.checklist_items['adrs']['patterns'])
        self.assertEqual(len(adr_files), 3)
        
        # Test API spec pattern
        api_files = {
            'api-spec.yaml': 'openapi: 3.0.0',
            'docs/api.json': '{"openapi": "3.0.0"}',
            'swagger.yml': 'swagger: "2.0"'
        }
        self.create_test_files(api_files)
        
        api_specs = checker._find_files_by_patterns(checker.checklist_items['api_specs']['patterns'])
        self.assertGreaterEqual(len(api_specs), 3)

if __name__ == '__main__':
    unittest.main()