"""Handoff notes functionality for Context Engine."""

import json
import re
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Optional, Any

from ..core.config import ContextConfig
from ..core.logger import get_logger

logger = get_logger(__name__)

class HandoffNotes:
    """Manages handoff notes for development sessions."""
    
    def __init__(self, config: ContextConfig):
        """Initialize HandoffNotes.
        
        Args:
            config: Context configuration instance
        """
        self.config = config
        self.handoff_dir = config.handoff_dir
        
        # Ensure handoff directory exists
        self.handoff_dir.mkdir(parents=True, exist_ok=True)
    
    def create_handoff_note(
        self,
        session_data: Dict[str, Any],
        summary: str,
        next_steps: List[str] = None,
        blockers: List[str] = None,
        additional_context: str = None
    ) -> Path:
        """Create a handoff note from session data.
        
        Args:
            session_data: Session information
            summary: Summary of work completed
            next_steps: List of next steps to take
            blockers: List of current blockers
            additional_context: Additional context information
            
        Returns:
            Path to created handoff note
        """
        if next_steps is None:
            next_steps = []
        if blockers is None:
            blockers = []
        
        # Generate filename with timestamp
        timestamp = datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
        filename = f"handoff_{timestamp}.md"
        note_path = self.handoff_dir / filename
        
        # Extract session info
        session_id = session_data.get('session_id', 'unknown')
        start_time = session_data.get('start_time', '')
        end_time = session_data.get('end_time', datetime.now().isoformat())
        
        # Calculate duration
        duration = self._calculate_duration(start_time, end_time)
        
        # Generate markdown content
        content = self._generate_markdown_content(
            session_id=session_id,
            start_time=start_time,
            duration=duration,
            summary=summary,
            next_steps=next_steps,
            blockers=blockers,
            session_data=session_data,
            additional_context=additional_context
        )
        
        # Write to file
        note_path.write_text(content, encoding='utf-8')
        
        logger.info(f"Created handoff note: {note_path}")
        return note_path
    
    def load_handoff_note(self, note_path: Path) -> Optional[Dict[str, Any]]:
        """Load and parse a handoff note.
        
        Args:
            note_path: Path to handoff note file
            
        Returns:
            Parsed handoff note data or None if failed
        """
        try:
            if not note_path.exists():
                return None
            
            content = note_path.read_text(encoding='utf-8')
            return self._parse_markdown_content(content)
            
        except Exception as e:
            logger.error(f"Failed to load handoff note {note_path}: {e}")
            return None
    
    def list_handoff_notes(self, limit: int = None) -> List[Dict[str, Any]]:
        """List all handoff notes, sorted by creation time (newest first).
        
        Args:
            limit: Maximum number of notes to return
            
        Returns:
            List of handoff note information
        """
        notes = []
        
        # Find all markdown files in handoff directory
        for note_file in self.handoff_dir.glob('handoff_*.md'):
            note_data = self.load_handoff_note(note_file)
            if note_data:
                # Extract timestamp from filename
                created_timestamp = self._extract_timestamp_from_filename(note_file.name)
                notes.append({
                    'filename': note_file.name,
                    'path': note_file,
                    'created': created_timestamp,
                    'data': note_data
                })
        
        # Sort by creation time (newest first)
        notes.sort(key=lambda x: x['created'], reverse=True)
        
        if limit:
            notes = notes[:limit]
        
        return notes
    
    def get_recent_handoffs(self, days: int = 7) -> List[Dict[str, Any]]:
        """Get handoff notes from the last N days.
        
        Args:
            days: Number of days to look back
            
        Returns:
            List of recent handoff notes
        """
        cutoff_time = datetime.now() - timedelta(days=days)
        cutoff_timestamp = cutoff_time.timestamp()
        
        all_notes = self.list_handoff_notes()
        recent_notes = [
            note for note in all_notes 
            if note['created'] >= cutoff_timestamp
        ]
        
        return recent_notes
    
    def search_handoffs(self, query: str) -> List[Dict[str, Any]]:
        """Search handoff notes by content.
        
        Args:
            query: Search query string
            
        Returns:
            List of matching handoff notes
        """
        results = []
        query_lower = query.lower()
        
        for note_file in self.handoff_dir.glob('handoff_*.md'):
            try:
                content = note_file.read_text(encoding='utf-8')
                if query_lower in content.lower():
                    note_data = self.load_handoff_note(note_file)
                    if note_data:
                        results.append({
                            'filename': note_file.name,
                            'path': note_file,
                            'content': content,
                            'data': note_data
                        })
            except Exception as e:
                logger.warning(f"Failed to search in {note_file}: {e}")
        
        return results
    
    def generate_context_summary(self, days: int = 7) -> Dict[str, Any]:
        """Generate a context summary from recent handoff notes.
        
        Args:
            days: Number of days to include in summary
            
        Returns:
            Context summary dictionary
        """
        recent_notes = self.get_recent_handoffs(days)
        
        summary = {
            'recent_work': [],
            'pending_tasks': [],
            'active_blockers': [],
            'modified_files': [],
            'key_decisions': [],
            'period': f"Last {days} days",
            'note_count': len(recent_notes)
        }
        
        for note in recent_notes:
            data = note['data']
            
            # Collect work summaries
            if 'summary' in data:
                summary['recent_work'].append(data['summary'])
            
            # Collect next steps
            if 'next_steps' in data:
                summary['pending_tasks'].extend(data['next_steps'])
            
            # Collect blockers
            if 'blockers' in data:
                summary['active_blockers'].extend(data['blockers'])
            
            # Collect modified files
            if 'files_modified' in data:
                summary['modified_files'].extend(data['files_modified'])
        
        # Remove duplicates
        summary['pending_tasks'] = list(set(summary['pending_tasks']))
        summary['active_blockers'] = list(set(summary['active_blockers']))
        summary['modified_files'] = list(set(summary['modified_files']))
        
        return summary
    
    def export_handoffs(self, format: str = 'json', days: int = None) -> str:
        """Export handoff notes to different formats.
        
        Args:
            format: Export format ('json' or 'csv')
            days: Number of days to include (None for all)
            
        Returns:
            Exported data as string
        """
        if days:
            notes = self.get_recent_handoffs(days)
        else:
            notes = self.list_handoff_notes()
        
        if format.lower() == 'json':
            return json.dumps([note['data'] for note in notes], indent=2)
        
        elif format.lower() == 'csv':
            import csv
            from io import StringIO
            
            output = StringIO()
            writer = csv.writer(output)
            
            # Write header
            writer.writerow(['session_id', 'date', 'summary', 'next_steps', 'blockers'])
            
            # Write data
            for note in notes:
                data = note['data']
                writer.writerow([
                    data.get('session_id', ''),
                    data.get('date', ''),
                    data.get('summary', ''),
                    '; '.join(data.get('next_steps', [])),
                    '; '.join(data.get('blockers', []))
                ])
            
            return output.getvalue()
        
        else:
            raise ValueError(f"Unsupported export format: {format}")
    
    def cleanup_old_handoffs(self, days: int = 30) -> int:
        """Clean up handoff notes older than specified days.
        
        Args:
            days: Age threshold in days
            
        Returns:
            Number of files cleaned up
        """
        cutoff_time = datetime.now() - timedelta(days=days)
        cutoff_timestamp = cutoff_time.timestamp()
        
        cleaned_count = 0
        
        for note_file in self.handoff_dir.glob('handoff_*.md'):
            try:
                # Extract timestamp from filename instead of file modification time
                file_timestamp = self._extract_timestamp_from_filename(note_file.name)
                if file_timestamp < cutoff_timestamp:
                    note_file.unlink()
                    cleaned_count += 1
                    logger.info(f"Cleaned up old handoff note: {note_file.name}")
            except Exception as e:
                logger.warning(f"Failed to clean up {note_file}: {e}")
        
        return cleaned_count
    
    def create_handoff_from_session(
        self,
        session_id: str,
        summary: str,
        next_steps: List[str] = None
    ) -> Path:
        """Create handoff note from session manager data.
        
        Args:
            session_id: Session identifier
            summary: Work summary
            next_steps: Next steps list
            
        Returns:
            Path to created handoff note
        """
        # Mock session data for integration
        session_data = {
            'session_id': session_id,
            'start_time': datetime.now().isoformat(),
            'commands_executed': ['npm install', 'npm test'],
            'files_modified': ['package.json', 'src/main.js']
        }
        
        return self.create_handoff_note(
            session_data=session_data,
            summary=summary,
            next_steps=next_steps or []
        )
    
    def _extract_timestamp_from_filename(self, filename: str) -> float:
        """Extract timestamp from handoff note filename.
        
        Args:
            filename: Handoff note filename (e.g., 'handoff_2024-01-15_14-30-45.md')
            
        Returns:
            Timestamp as float
        """
        try:
            # Extract date-time part from filename
            # Format: handoff_YYYY-MM-DD_HH-MM-SS.md
            date_part = filename.replace('handoff_', '').replace('.md', '')
            
            # Parse the datetime
            dt = datetime.strptime(date_part, '%Y-%m-%d_%H-%M-%S')
            return dt.timestamp()
        except Exception:
            # Fallback to file modification time if parsing fails
            return datetime.now().timestamp()
    
    def _calculate_duration(self, start_time: str, end_time: str) -> str:
        """Calculate session duration.
        
        Args:
            start_time: Start time string
            end_time: End time string
            
        Returns:
            Duration string
        """
        try:
            if not start_time or not end_time:
                return "Unknown"
            
            start = datetime.fromisoformat(start_time.replace('Z', '+00:00'))
            end = datetime.fromisoformat(end_time.replace('Z', '+00:00'))
            
            duration = end - start
            hours = duration.total_seconds() / 3600
            
            if hours < 1:
                minutes = int(duration.total_seconds() / 60)
                return f"{minutes} minutes"
            else:
                return f"{hours:.1f} hours"
                
        except Exception:
            return "Unknown"
    
    def _generate_markdown_content(
        self,
        session_id: str,
        start_time: str,
        duration: str,
        summary: str,
        next_steps: List[str],
        blockers: List[str],
        session_data: Dict[str, Any],
        additional_context: str = None
    ) -> str:
        """Generate markdown content for handoff note.
        
        Args:
            session_id: Session identifier
            start_time: Session start time
            duration: Session duration
            summary: Work summary
            next_steps: Next steps list
            blockers: Blockers list
            session_data: Full session data
            additional_context: Additional context
            
        Returns:
            Markdown content string
        """
        content = ["# Development Handoff Notes\n"]
        
        # Session info
        content.append(f"## Session: {session_id}")
        if start_time:
            try:
                date = datetime.fromisoformat(start_time.replace('Z', '+00:00')).strftime('%Y-%m-%d')
                content.append(f"**Date:** {date}")
            except:
                content.append(f"**Date:** {start_time}")
        content.append(f"**Duration:** {duration}\n")
        
        # Summary
        content.append("## Summary")
        content.append(f"{summary}\n")
        
        # Next steps
        if next_steps:
            content.append("## Next Steps")
            for step in next_steps:
                content.append(f"- [ ] {step}")
            content.append("")
        
        # Blockers
        if blockers:
            content.append("## Blockers")
            for blocker in blockers:
                content.append(f"- {blocker}")
            content.append("")
        
        # Files modified
        files_modified = session_data.get('files_modified', [])
        if files_modified:
            content.append("## Files Modified")
            for file_path in files_modified:
                content.append(f"- {file_path}")
            content.append("")
        
        # Commands executed
        commands = session_data.get('commands_executed', [])
        if commands:
            content.append("## Commands Executed")
            content.append("```bash")
            for command in commands:
                content.append(command)
            content.append("```\n")
        
        # Errors encountered
        errors = session_data.get('errors_encountered', [])
        if errors:
            content.append("## Errors Encountered")
            for error in errors:
                content.append(f"- **{error.get('type', 'Error')}**: {error.get('message', '')}")
                if 'severity' in error:
                    content.append(f"  - **Severity**: {error['severity'].title()}")
                if 'timestamp' in error:
                    content.append(f"  - **Time**: {error['timestamp']}")
                if 'resolution' in error:
                    content.append(f"  - **Resolution**: {error['resolution']}")
            content.append("")
        
        # Additional context
        if additional_context:
            content.append("## Additional Context")
            content.append(f"{additional_context}\n")
        
        return "\n".join(content)
    
    def _parse_markdown_content(self, content: str) -> Dict[str, Any]:
        """Parse markdown content back to structured data.
        
        Args:
            content: Markdown content string
            
        Returns:
            Parsed data dictionary
        """
        data = {}
        
        # Extract session ID
        session_match = re.search(r'## Session: (.+)', content)
        if session_match:
            data['session_id'] = session_match.group(1).strip()
        
        # Extract date
        date_match = re.search(r'\*\*Date:\*\* (.+)', content)
        if date_match:
            data['date'] = date_match.group(1).strip()
        
        # Extract duration
        duration_match = re.search(r'\*\*Duration:\*\* (.+)', content)
        if duration_match:
            data['duration'] = duration_match.group(1).strip()
        
        # Extract summary
        summary_match = re.search(r'## Summary\n(.+?)\n\n', content, re.DOTALL)
        if summary_match:
            data['summary'] = summary_match.group(1).strip()
        
        # Extract next steps
        next_steps_match = re.search(r'## Next Steps\n((?:- \[ \] .+\n?)+)', content)
        if next_steps_match:
            steps_text = next_steps_match.group(1)
            data['next_steps'] = [
                line.replace('- [ ] ', '').strip() 
                for line in steps_text.split('\n') 
                if line.strip().startswith('- [ ]')
            ]
        else:
            data['next_steps'] = []
        
        # Extract blockers
        blockers_match = re.search(r'## Blockers\n((?:- .+\n?)+)', content)
        if blockers_match:
            blockers_text = blockers_match.group(1)
            data['blockers'] = [
                line.replace('- ', '').strip() 
                for line in blockers_text.split('\n') 
                if line.strip().startswith('- ')
            ]
        else:
            data['blockers'] = []
        
        # Extract files modified
        files_match = re.search(r'## Files Modified\n((?:- .+\n?)+)', content)
        if files_match:
            files_text = files_match.group(1)
            data['files_modified'] = [
                line.replace('- ', '').strip() 
                for line in files_text.split('\n') 
                if line.strip().startswith('- ')
            ]
        else:
            data['files_modified'] = []
        
        return data
    
    def _validate_handoff_data(self, data: Dict[str, Any]) -> bool:
        """Validate handoff note data structure.
        
        Args:
            data: Data to validate
            
        Returns:
            True if valid, False otherwise
        """
        required_fields = ['session_id', 'summary']
        
        # Check required fields
        for field in required_fields:
            if field not in data:
                return False
        
        # Check field types
        if not isinstance(data.get('next_steps', []), list):
            return False
        
        if not isinstance(data.get('blockers', []), list):
            return False
        
        if not isinstance(data.get('files_modified', []), list):
            return False
        
        return True