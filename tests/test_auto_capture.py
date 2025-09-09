"""Test cases for auto-capture functionality."""

import unittest
import tempfile
import json
import time
import threading
from pathlib import Path
from unittest.mock import patch, MagicMock, mock_open

from context_engine.scripts.auto_capture import AutoCapture
from context_engine.core.config import ContextConfig

class TestAutoCapture(unittest.TestCase):
    """Test cases for auto-capture functionality."""
    
    def setUp(self):
        """Set up test environment."""
        self.temp_dir = tempfile.mkdtemp()
        self.project_path = Path(self.temp_dir)
        
        # Create mock config
        self.mock_config = MagicMock(spec=ContextConfig)
        self.mock_config.project_root = self.project_path
        self.mock_config.data_dir = self.project_path / '.context' / 'data'
        self.mock_config.logs_dir = self.project_path / '.context' / 'logs'
        
        # Create directories
        self.mock_config.data_dir.mkdir(parents=True, exist_ok=True)
        self.mock_config.logs_dir.mkdir(parents=True, exist_ok=True)
        
    def tearDown(self):
        """Clean up test environment."""
        import shutil
        shutil.rmtree(self.temp_dir)
    
    def test_auto_capture_initialization(self):
        """Test AutoCapture initialization."""
        capture = AutoCapture(self.mock_config)
        
        self.assertEqual(capture.config, self.mock_config)
        self.assertFalse(capture.is_capturing)
        self.assertEqual(len(capture.active_processes), 0)
    
    @patch('subprocess.Popen')
    def test_start_capture_process(self, mock_popen):
        """Test starting a capture process."""
        # Mock process
        mock_process = MagicMock()
        mock_process.pid = 12345
        mock_process.poll.return_value = None  # Process is running
        mock_process.stdout.readline.side_effect = [
            b'Starting server...\n',
            b'Server running on port 3000\n',
            b''  # EOF
        ]
        mock_process.stderr.readline.side_effect = [b'', b'', b'']  # No errors
        mock_popen.return_value = mock_process
        
        capture = AutoCapture(self.mock_config)
        
        # Start capture
        session_id = capture.start_capture(
            name='test-server',
            command=['npm', 'start'],
            server_type='node'
        )
        
        self.assertIsNotNone(session_id)
        self.assertTrue(capture.is_capturing)
        self.assertEqual(len(capture.active_processes), 1)
        
        # Verify process was started
        mock_popen.assert_called_once()
        
        # Stop capture
        capture.stop_capture(session_id)
        
        self.assertFalse(capture.is_capturing)
    
    def test_capture_output_parsing(self):
        """Test output parsing and log storage."""
        capture = AutoCapture(self.mock_config)
        
        # Test log parsing
        test_output = "Error: Cannot find module 'express'\n"
        
        with patch.object(capture, '_parse_and_save_errors') as mock_parse:
            capture._capture_process_output(
                process=MagicMock(),
                session_id='test-session',
                name='test-app',
                server_type='node'
            )
            
            # Simulate output
            capture._save_output('test-session', 'stdout', test_output)
            
            # Verify log file creation
            log_file = self.mock_config.logs_dir / 'test-session.log'
            self.assertTrue(log_file.parent.exists())
    
    def test_get_parsed_errors(self):
        """Test retrieving parsed errors."""
        capture = AutoCapture(self.mock_config)
        
        # Create mock error data
        session_id = 'test-session'
        error_data = {
            'errors': [
                {
                    'type': 'ModuleNotFoundError',
                    'message': 'Cannot find module express',
                    'severity': 'high',
                    'timestamp': '2024-01-01T12:00:00'
                }
            ],
            'summary': {
                'total_errors': 1,
                'high_severity': 1,
                'medium_severity': 0,
                'low_severity': 0
            }
        }
        
        # Mock the error file
        error_file = self.mock_config.data_dir / f'{session_id}_errors.json'
        error_file.write_text(json.dumps(error_data))
        
        # Test retrieval
        errors = capture.get_parsed_errors(session_id)
        
        self.assertIsNotNone(errors)
        self.assertEqual(len(errors['errors']), 1)
        self.assertEqual(errors['summary']['total_errors'], 1)
    
    def test_list_active_sessions(self):
        """Test listing active capture sessions."""
        capture = AutoCapture(self.mock_config)
        
        # Initially no sessions
        sessions = capture.list_active_sessions()
        self.assertEqual(len(sessions), 0)
        
        # Add mock session
        session_data = {
            'session_id': 'test-session',
            'name': 'test-app',
            'command': ['npm', 'start'],
            'server_type': 'node',
            'pid': 12345,
            'start_time': time.time(),
            'status': 'running'
        }
        
        capture.active_processes['test-session'] = session_data
        
        sessions = capture.list_active_sessions()
        self.assertEqual(len(sessions), 1)
        self.assertEqual(sessions[0]['name'], 'test-app')
    
    @patch('subprocess.Popen')
    def test_stop_capture(self, mock_popen):
        """Test stopping a capture session."""
        # Mock process
        mock_process = MagicMock()
        mock_process.pid = 12345
        mock_process.poll.return_value = None
        mock_process.terminate = MagicMock()
        mock_popen.return_value = mock_process
        
        capture = AutoCapture(self.mock_config)
        
        # Start capture
        session_id = capture.start_capture(
            name='test-server',
            command=['npm', 'start'],
            server_type='node'
        )
        
        # Stop capture
        result = capture.stop_capture(session_id)
        
        self.assertTrue(result)
        self.assertNotIn(session_id, capture.active_processes)
        mock_process.terminate.assert_called_once()
    
    def test_server_type_detection(self):
        """Test automatic server type detection."""
        capture = AutoCapture(self.mock_config)
        
        # Test various commands
        test_cases = [
            (['npm', 'start'], 'node'),
            (['python', 'manage.py', 'runserver'], 'django'),
            (['python', 'app.py'], 'python'),
            (['java', '-jar', 'app.jar'], 'java'),
            (['./gradlew', 'bootRun'], 'spring'),
            (['unknown', 'command'], 'generic')
        ]
        
        for command, expected_type in test_cases:
            detected_type = capture._detect_server_type(command)
            self.assertEqual(detected_type, expected_type, 
                           f"Failed for command: {command}")
    
    def test_log_rotation(self):
        """Test log file rotation functionality."""
        capture = AutoCapture(self.mock_config)
        
        session_id = 'test-session'
        
        # Create a large log file
        large_content = 'x' * (10 * 1024 * 1024)  # 10MB
        
        with patch.object(capture, '_should_rotate_log', return_value=True):
            with patch.object(capture, '_rotate_log_file') as mock_rotate:
                capture._save_output(session_id, 'stdout', large_content)
                
                # Verify rotation was called
                mock_rotate.assert_called()
    
    def test_error_handling_invalid_command(self):
        """Test error handling for invalid commands."""
        capture = AutoCapture(self.mock_config)
        
        # Try to start capture with invalid command
        with patch('subprocess.Popen', side_effect=FileNotFoundError()):
            session_id = capture.start_capture(
                name='invalid-app',
                command=['nonexistent-command'],
                server_type='generic'
            )
            
            self.assertIsNone(session_id)
            self.assertFalse(capture.is_capturing)
    
    def test_concurrent_captures(self):
        """Test handling multiple concurrent capture sessions."""
        capture = AutoCapture(self.mock_config)
        
        with patch('subprocess.Popen') as mock_popen:
            # Mock multiple processes
            mock_processes = []
            for i in range(3):
                mock_process = MagicMock()
                mock_process.pid = 12345 + i
                mock_process.poll.return_value = None
                mock_processes.append(mock_process)
            
            mock_popen.side_effect = mock_processes
            
            # Start multiple captures
            session_ids = []
            for i in range(3):
                session_id = capture.start_capture(
                    name=f'app-{i}',
                    command=['echo', f'test-{i}'],
                    server_type='generic'
                )
                session_ids.append(session_id)
            
            # Verify all sessions are active
            self.assertEqual(len(capture.active_processes), 3)
            
            # Stop all sessions
            for session_id in session_ids:
                if session_id:
                    capture.stop_capture(session_id)
            
            self.assertEqual(len(capture.active_processes), 0)

class TestAutoCaptureIntegration(unittest.TestCase):
    """Integration tests for auto-capture with parsers."""
    
    def setUp(self):
        """Set up test environment."""
        self.temp_dir = tempfile.mkdtemp()
        self.project_path = Path(self.temp_dir)
        
        # Create mock config
        self.mock_config = MagicMock(spec=ContextConfig)
        self.mock_config.project_root = self.project_path
        self.mock_config.data_dir = self.project_path / '.context' / 'data'
        self.mock_config.logs_dir = self.project_path / '.context' / 'logs'
        
        # Create directories
        self.mock_config.data_dir.mkdir(parents=True, exist_ok=True)
        self.mock_config.logs_dir.mkdir(parents=True, exist_ok=True)
    
    def tearDown(self):
        """Clean up test environment."""
        import shutil
        shutil.rmtree(self.temp_dir)
    
    @patch('context_engine.parsers.parser_factory.parser_factory')
    def test_integration_with_parsers(self, mock_parser_factory):
        """Test integration between auto-capture and log parsers."""
        # Mock parser
        mock_parser = MagicMock()
        mock_parser.parse_content.return_value = {
            'errors': [
                {
                    'type': 'SyntaxError',
                    'message': 'Unexpected token',
                    'severity': 'high',
                    'line': 42,
                    'file': 'app.js'
                }
            ],
            'summary': {
                'total_errors': 1,
                'high_severity': 1
            }
        }
        
        mock_parser_factory.get_parser_by_type.return_value = mock_parser
        
        capture = AutoCapture(self.mock_config)
        
        # Test error parsing
        test_log_content = "SyntaxError: Unexpected token at app.js:42"
        
        # Simulate parsing
        capture._parse_and_save_errors('test-session', test_log_content, 'node')
        
        # Verify parser was called
        mock_parser_factory.get_parser_by_type.assert_called_with('node')
        mock_parser.parse_content.assert_called_with(test_log_content)
        
        # Verify error file was created
        error_file = self.mock_config.data_dir / 'test-session_errors.json'
        self.assertTrue(error_file.exists())

if __name__ == '__main__':
    unittest.main()