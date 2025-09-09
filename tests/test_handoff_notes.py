"""Test cases for handoff notes functionality."""

import unittest
import tempfile
import json
from pathlib import Path
from unittest.mock import patch, MagicMock, mock_open
from datetime import datetime

from context_engine.scripts.handoff_notes import HandoffNotes
from context_engine.core.config import ContextConfig

class TestHandoffNotes(unittest.TestCase):
    """Test cases for handoff notes functionality."""
    
    def setUp(self):
        """Set up test environment."""
        self.temp_dir = tempfile.mkdtemp()
        self.project_path = Path(self.temp_dir)
        
        # Create mock config
        self.mock_config = MagicMock(spec=ContextConfig)
        self.mock_config.project_root = self.project_path
        self.mock_config.data_dir = self.project_path / '.context' / 'data'
        self.mock_config.handoff_dir = self.project_path / '.context' / 'handoffs'
        
        # Create directories
        self.mock_config.data_dir.mkdir(parents=True, exist_ok=True)
        self.mock_config.handoff_dir.mkdir(parents=True, exist_ok=True)
        
    def tearDown(self):
        """Clean up test environment."""
        import shutil
        shutil.rmtree(self.temp_dir)
    
    def test_handoff_notes_initialization(self):
        """Test HandoffNotes initialization."""
        handoff = HandoffNotes(self.mock_config)
        
        self.assertEqual(handoff.config, self.mock_config)
        self.assertTrue(handoff.handoff_dir.exists())
    
    def test_create_handoff_note(self):
        """Test creating a handoff note."""
        handoff = HandoffNotes(self.mock_config)
        
        # Mock session data
        session_data = {
            'session_id': 'test-session-123',
            'start_time': '2024-01-01T10:00:00',
            'end_time': '2024-01-01T11:30:00',
            'commands_executed': [
                'npm install',
                'npm start',
                'git add .',
                'git commit -m "Add new feature"'
            ],
            'files_modified': [
                'src/components/Header.js',
                'src/styles/main.css',
                'package.json'
            ],
            'errors_encountered': [
                {
                    'type': 'ModuleNotFoundError',
                    'message': 'Cannot find module react-router',
                    'severity': 'high',
                    'timestamp': '2024-01-01T10:15:00'
                }
            ]
        }
        
        # Create handoff note
        note_path = handoff.create_handoff_note(
            session_data=session_data,
            summary="Implemented user authentication feature",
            next_steps=[
                "Add password reset functionality",
                "Implement email verification",
                "Add user profile page"
            ],
            blockers=[
                "Need API key for email service",
                "Database migration pending"
            ]
        )
        
        # Verify note was created
        self.assertTrue(note_path.exists())
        self.assertTrue(note_path.name.endswith('.md'))
        
        # Verify content
        content = note_path.read_text()
        self.assertIn('# Development Handoff Notes', content)
        self.assertIn('Implemented user authentication feature', content)
        self.assertIn('Add password reset functionality', content)
        self.assertIn('Need API key for email service', content)
        self.assertIn('npm install', content)
        self.assertIn('src/components/Header.js', content)
    
    def test_load_handoff_note(self):
        """Test loading an existing handoff note."""
        handoff = HandoffNotes(self.mock_config)
        
        # Create a test handoff note
        test_content = """# Development Handoff Notes

## Session: test-session-456
**Date:** 2024-01-01
**Duration:** 1.5 hours

## Summary
Fixed critical bug in payment processing

## Next Steps
- [ ] Add unit tests for payment module
- [ ] Deploy to staging environment

## Blockers
- Payment gateway API documentation missing

## Files Modified
- src/payment/processor.js
- tests/payment.test.js

## Commands Executed
```bash
npm test
git add .
git commit -m "Fix payment bug"
```

## Errors Encountered
- **TypeError**: Cannot read property 'amount' of undefined
  - **Severity**: High
  - **Time**: 2024-01-01T14:30:00
  - **Resolution**: Added null check for payment object
"""
        
        note_file = self.mock_config.handoff_dir / 'handoff_2024-01-01_14-00-00.md'
        note_file.write_text(test_content)
        
        # Load the note
        loaded_data = handoff.load_handoff_note(note_file)
        
        self.assertIsNotNone(loaded_data)
        self.assertEqual(loaded_data['session_id'], 'test-session-456')
        self.assertEqual(loaded_data['summary'], 'Fixed critical bug in payment processing')
        self.assertEqual(len(loaded_data['next_steps']), 2)
        self.assertEqual(len(loaded_data['blockers']), 1)
        self.assertEqual(len(loaded_data['files_modified']), 2)
    
    def test_list_handoff_notes(self):
        """Test listing all handoff notes."""
        handoff = HandoffNotes(self.mock_config)
        
        # Create multiple test notes
        note_files = [
            'handoff_2024-01-01_10-00-00.md',
            'handoff_2024-01-01_14-30-00.md',
            'handoff_2024-01-02_09-15-00.md'
        ]
        
        for note_file in note_files:
            (self.mock_config.handoff_dir / note_file).write_text(
                f"# Development Handoff Notes\n\n## Session: {note_file}\n"
            )
        
        # List notes
        notes = handoff.list_handoff_notes()
        
        self.assertEqual(len(notes), 3)
        
        # Verify sorting (newest first)
        note_names = [note['filename'] for note in notes]
        self.assertEqual(note_names[0], 'handoff_2024-01-02_09-15-00.md')
        self.assertEqual(note_names[-1], 'handoff_2024-01-01_10-00-00.md')
    
    def test_get_recent_handoffs(self):
        """Test getting recent handoff notes."""
        handoff = HandoffNotes(self.mock_config)
        
        # Create test notes with different dates
        from datetime import datetime, timedelta
        
        dates = [
            datetime.now() - timedelta(days=1),
            datetime.now() - timedelta(days=3),
            datetime.now() - timedelta(days=7),
            datetime.now() - timedelta(days=10)
        ]
        
        for i, date in enumerate(dates):
            note_file = self.mock_config.handoff_dir / f'handoff_{date.strftime("%Y-%m-%d_%H-%M-%S")}.md'
            note_file.write_text(
                f"# Development Handoff Notes\n\n## Session: session-{i}\n"
            )
        
        # Get recent handoffs (last 5 days)
        recent_notes = handoff.get_recent_handoffs(days=5)
        
        # Should only include notes from last 5 days
        self.assertEqual(len(recent_notes), 2)
    
    def test_search_handoffs(self):
        """Test searching handoff notes by content."""
        handoff = HandoffNotes(self.mock_config)
        
        # Create test notes with different content
        test_notes = [
            {
                'filename': 'handoff_2024-01-01_10-00-00.md',
                'content': """# Development Handoff Notes

## Summary
Implemented user authentication with JWT tokens

## Files Modified
- src/auth/login.js
- src/middleware/auth.js
"""
            },
            {
                'filename': 'handoff_2024-01-01_14-00-00.md',
                'content': """# Development Handoff Notes

## Summary
Fixed payment processing bug in checkout flow

## Files Modified
- src/payment/processor.js
- src/components/Checkout.js
"""
            },
            {
                'filename': 'handoff_2024-01-02_09-00-00.md',
                'content': """# Development Handoff Notes

## Summary
Added user profile management features

## Files Modified
- src/components/Profile.js
- src/api/user.js
"""
            }
        ]
        
        for note in test_notes:
            (self.mock_config.handoff_dir / note['filename']).write_text(note['content'])
        
        # Search for authentication-related notes
        auth_results = handoff.search_handoffs('authentication')
        self.assertEqual(len(auth_results), 1)
        self.assertIn('authentication', auth_results[0]['content'].lower())
        
        # Search for payment-related notes
        payment_results = handoff.search_handoffs('payment')
        self.assertEqual(len(payment_results), 1)
        self.assertIn('payment', payment_results[0]['content'].lower())
        
        # Search for user-related notes (should match multiple)
        user_results = handoff.search_handoffs('user')
        self.assertEqual(len(user_results), 2)
    
    def test_generate_context_summary(self):
        """Test generating context summary from handoff notes."""
        handoff = HandoffNotes(self.mock_config)
        
        # Create test handoff notes
        test_notes = [
            {
                'session_id': 'session-1',
                'summary': 'Implemented user authentication',
                'files_modified': ['src/auth/login.js', 'src/middleware/auth.js'],
                'next_steps': ['Add password reset', 'Implement 2FA'],
                'blockers': ['Need SMTP configuration']
            },
            {
                'session_id': 'session-2',
                'summary': 'Fixed payment processing bug',
                'files_modified': ['src/payment/processor.js'],
                'next_steps': ['Add payment tests', 'Deploy to staging'],
                'blockers': []
            }
        ]
        
        # Mock the loading of notes
        with patch.object(handoff, 'get_recent_handoffs') as mock_get_recent:
            mock_get_recent.return_value = [
                {'data': note, 'filename': f'handoff_{i}.md'} 
                for i, note in enumerate(test_notes)
            ]
            
            # Generate context summary
            summary = handoff.generate_context_summary(days=7)
            
            self.assertIsNotNone(summary)
            self.assertIn('recent_work', summary)
            self.assertIn('pending_tasks', summary)
            self.assertIn('active_blockers', summary)
            self.assertIn('modified_files', summary)
            
            # Verify content
            self.assertEqual(len(summary['recent_work']), 2)
            self.assertIn('Add password reset', summary['pending_tasks'])
            self.assertIn('Need SMTP configuration', summary['active_blockers'])
            self.assertIn('src/auth/login.js', summary['modified_files'])
    
    def test_export_handoffs(self):
        """Test exporting handoff notes to different formats."""
        handoff = HandoffNotes(self.mock_config)
        
        # Create test notes
        test_data = [
            {
                'session_id': 'session-1',
                'date': '2024-01-01',
                'summary': 'Implemented authentication',
                'files_modified': ['auth.js'],
                'next_steps': ['Add tests']
            }
        ]
        
        with patch.object(handoff, 'list_handoff_notes') as mock_list:
            mock_list.return_value = [{'data': data} for data in test_data]
            
            # Export to JSON
            json_output = handoff.export_handoffs(format='json')
            json_data = json.loads(json_output)
            
            self.assertEqual(len(json_data), 1)
            self.assertEqual(json_data[0]['session_id'], 'session-1')
            
            # Export to CSV
            csv_output = handoff.export_handoffs(format='csv')
            
            self.assertIn('session_id,date,summary', csv_output)
            self.assertIn('session-1,2024-01-01,Implemented authentication', csv_output)
    
    def test_validate_handoff_data(self):
        """Test validation of handoff note data."""
        handoff = HandoffNotes(self.mock_config)
        
        # Valid data
        valid_data = {
            'session_id': 'test-session',
            'summary': 'Test summary',
            'next_steps': ['Step 1', 'Step 2'],
            'blockers': [],
            'files_modified': ['file1.js']
        }
        
        self.assertTrue(handoff._validate_handoff_data(valid_data))
        
        # Invalid data - missing required fields
        invalid_data = {
            'session_id': 'test-session'
            # Missing summary
        }
        
        self.assertFalse(handoff._validate_handoff_data(invalid_data))
        
        # Invalid data - wrong types
        invalid_types = {
            'session_id': 'test-session',
            'summary': 'Test summary',
            'next_steps': 'Should be a list',  # Wrong type
            'blockers': [],
            'files_modified': ['file1.js']
        }
        
        self.assertFalse(handoff._validate_handoff_data(invalid_types))
    
    def test_cleanup_old_handoffs(self):
        """Test cleanup of old handoff notes."""
        handoff = HandoffNotes(self.mock_config)
        
        # Create old and new handoff notes
        from datetime import datetime, timedelta
        
        old_date = datetime.now() - timedelta(days=35)
        recent_date = datetime.now() - timedelta(days=5)
        
        old_file = self.mock_config.handoff_dir / f'handoff_{old_date.strftime("%Y-%m-%d_%H-%M-%S")}.md'
        recent_file = self.mock_config.handoff_dir / f'handoff_{recent_date.strftime("%Y-%m-%d_%H-%M-%S")}.md'
        
        old_file.write_text('Old handoff note')
        recent_file.write_text('Recent handoff note')
        
        # Cleanup notes older than 30 days
        cleaned_count = handoff.cleanup_old_handoffs(days=30)
        
        self.assertEqual(cleaned_count, 1)
        self.assertFalse(old_file.exists())
        self.assertTrue(recent_file.exists())

class TestHandoffNotesIntegration(unittest.TestCase):
    """Integration tests for handoff notes with session management."""
    
    def setUp(self):
        """Set up test environment."""
        self.temp_dir = tempfile.mkdtemp()
        self.project_path = Path(self.temp_dir)
        
        # Create mock config
        self.mock_config = MagicMock(spec=ContextConfig)
        self.mock_config.project_root = self.project_path
        self.mock_config.data_dir = self.project_path / '.context' / 'data'
        self.mock_config.handoff_dir = self.project_path / '.context' / 'handoffs'
        
        # Create directories
        self.mock_config.data_dir.mkdir(parents=True, exist_ok=True)
        self.mock_config.handoff_dir.mkdir(parents=True, exist_ok=True)
    
    def tearDown(self):
        """Clean up test environment."""
        import shutil
        shutil.rmtree(self.temp_dir)
    
    @patch('context_engine.scripts.session.SessionManager')
    def test_integration_with_session_manager(self, mock_session_manager):
        """Test integration between handoff notes and session management."""
        # Mock session data
        mock_session = MagicMock()
        mock_session.get_session_data.return_value = {
            'session_id': 'integration-test',
            'start_time': '2024-01-01T10:00:00',
            'commands_executed': ['npm install', 'npm start'],
            'files_modified': ['package.json', 'src/app.js']
        }
        
        mock_session_manager.return_value = mock_session
        
        handoff = HandoffNotes(self.mock_config)
        
        # Create handoff note from session
        note_path = handoff.create_handoff_from_session(
            session_id='integration-test',
            summary='Integration test session',
            next_steps=['Continue testing']
        )
        
        self.assertTrue(note_path.exists())
        
        content = note_path.read_text()
        self.assertIn('integration-test', content)
        self.assertIn('npm install', content)
        self.assertIn('package.json', content)

if __name__ == '__main__':
    unittest.main()