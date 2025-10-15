"""Session management for Context Engine."""

import json
import os
import shutil
import uuid
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Any, Set

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.embeddings_store import EmbeddingsStore
from context_engine.scripts.embedder import FileIndexer

logger = setup_logger(__name__)

class SessionManager:
    """Manages AI tool sessions and payload generation."""
    
    def __init__(self, config: ContextConfig, project_root: Optional[Path] = None):
        self.config = config
        self.project_root = project_root or Path.cwd()
        self.sessions_dir = self.project_root / "context_engine" / "sessions"
        self.payload_dir = self.project_root / ".context_payload"
        self.active_session_file = self.sessions_dir / "active_session.json"
        
        # Ensure directories exist
        self.sessions_dir.mkdir(parents=True, exist_ok=True)
        self.payload_dir.mkdir(parents=True, exist_ok=True)
        
        # Initialize components
        self.indexer = FileIndexer(config, project_root)
        
        # Current session state
        self.current_session = None
        self.scoped_paths: Set[str] = set()
        
        # Load active session if exists
        self._load_active_session()
    
    def _load_active_session(self):
        """Load the currently active session."""
        if self.active_session_file.exists():
            try:
                with open(self.active_session_file, 'r', encoding='utf-8') as f:
                    session_data = json.load(f)
                    self.current_session = session_data
                    self.scoped_paths = set(session_data.get('scoped_paths', []))
                    logger.info(f"Loaded active session: {session_data['session_id']}")
            except Exception as e:
                logger.warning(f"Could not load active session: {e}")
    
    def _save_active_session(self):
        """Save the current active session."""
        if self.current_session:
            try:
                with open(self.active_session_file, 'w', encoding='utf-8') as f:
                    json.dump(self.current_session, f, indent=2)
            except Exception as e:
                logger.error(f"Could not save active session: {e}")
    
    def start_session(self, session_name: Optional[str] = None, 
                     description: Optional[str] = None) -> str:
        """Start a new AI tool session."""
        session_id = str(uuid.uuid4())[:8]
        timestamp = datetime.now().isoformat()
        
        if not session_name:
            session_name = f"session_{timestamp.split('T')[0]}_{session_id}"
        
        session_data = {
            'session_id': session_id,
            'session_name': session_name,
            'description': description or '',
            'created_at': timestamp,
            'last_updated': timestamp,
            'scoped_paths': [],
            'injected_files': [],
            'captured_logs': [],
            'status': 'active'
        }
        
        # Stop any existing session
        if self.current_session:
            self.stop_session()
        
        # Set as current session
        self.current_session = session_data
        self.scoped_paths = set()
        
        # Save session
        session_file = self.sessions_dir / f"{session_id}.json"
        with open(session_file, 'w', encoding='utf-8') as f:
            json.dump(session_data, f, indent=2)
        
        self._save_active_session()
        
        logger.info(f"Started session '{session_name}' ({session_id})")
        return session_id
    
    def stop_session(self) -> bool:
        """Stop the current session."""
        if not self.current_session:
            logger.warning("No active session to stop")
            return False
        
        session_id = self.current_session['session_id']
        
        # Create agent handoff notes
        self._create_handoff_notes()
        
        # Update session status
        self.current_session['status'] = 'completed'
        self.current_session['completed_at'] = datetime.now().isoformat()
        
        # Save final session state
        session_file = self.sessions_dir / f"{session_id}.json"
        with open(session_file, 'w', encoding='utf-8') as f:
            json.dump(self.current_session, f, indent=2)
        
        # Clear active session
        if self.active_session_file.exists():
            self.active_session_file.unlink()
        
        logger.info(f"Stopped session {session_id}")
        
        self.current_session = None
        self.scoped_paths = set()
        
        return True

    def _create_handoff_notes(self):
        """Create agent handoff notes for the current session."""
        if not self.current_session:
            return
        
        session_id = self.current_session['session_id']
        session_name = self.current_session.get('session_name', session_id)
        timestamp = datetime.now().isoformat()
        
        # Create handoff notes directory
        handoff_dir = self.project_root / "context_engine" / "summaries" / "session_summaries"
        handoff_dir.mkdir(parents=True, exist_ok=True)
        
        # Generate handoff content
        handoff_content = f"""# Agent Handoff Notes

**Session:** {session_name} ({session_id})
**Timestamp:** {timestamp}

## LAST_AGENT
- Session Duration: {self.current_session.get('created_at', 'Unknown')} to {timestamp}
- Scoped Files: {len(self.scoped_paths)} files
- Captured Logs: {len(self.current_session.get('captured_logs', []))} entries

## LAST_ACTIONS
- Files in scope: {', '.join(list(self.scoped_paths)[:5])}{'...' if len(self.scoped_paths) > 5 else ''}
- Injected files: {len(self.current_session.get('injected_files', []))} files
- Log captures: {len(self.current_session.get('captured_logs', []))} entries

## NEXT_STEPS
- Review scoped files for context
- Check captured logs for any errors or important information
- Continue development based on session scope and previous actions

## SESSION_DATA
```json
{json.dumps(self.current_session, indent=2)}
```
"""
        
        # Save handoff notes
        handoff_file = handoff_dir / f"handoff_{session_id}_{timestamp.split('T')[0]}.md"
        try:
            with open(handoff_file, 'w', encoding='utf-8') as f:
                f.write(handoff_content)
            logger.info(f"Created handoff notes: {handoff_file}")
        except Exception as e:
            logger.error(f"Failed to create handoff notes: {e}")

    def _load_previous_handoff_notes(self) -> str:
        """Load the most recent handoff notes if available."""
        handoff_dir = self.project_root / "context_engine" / "summaries" / "session_summaries"
        if not handoff_dir.exists():
            return ""
        
        # Find the most recent handoff file
        handoff_files = list(handoff_dir.glob("handoff_*.md"))
        if not handoff_files:
            return ""
        
        # Sort by modification time, get the most recent
        latest_handoff = max(handoff_files, key=lambda f: f.stat().st_mtime)
        
        try:
            with open(latest_handoff, 'r', encoding='utf-8') as f:
                content = f.read()
            logger.info(f"Loaded previous handoff notes: {latest_handoff}")
            return content
        except Exception as e:
            logger.error(f"Failed to load handoff notes: {e}")
            return ""

    def set_scope(self, paths: List[str], append: bool = False) -> bool:
        """Set the scope for the current session."""
        if not self.current_session:
            logger.error("No active session. Start a session first.")
            return False
        
        # Validate and normalize paths
        valid_paths = []
        for path_str in paths:
            path = Path(path_str)
            if not path.is_absolute():
                path = self.project_root / path
            
            if path.exists():
                # Store relative path for portability
                try:
                    relative_path = str(path.relative_to(self.project_root))
                    valid_paths.append(relative_path)
                except ValueError:
                    # Path is outside project root
                    logger.warning(f"Path outside project root: {path}")
                    valid_paths.append(str(path))
            else:
                logger.warning(f"Path does not exist: {path}")
        
        if append:
            # Add to existing scope
            self.scoped_paths.update(valid_paths)
        else:
            # Replace existing scope
            self.scoped_paths = set(valid_paths)
        
        self.current_session['scoped_paths'] = list(self.scoped_paths)
        self.current_session['last_updated'] = datetime.now().isoformat()
        
        self._save_active_session()
        
        action = "Added to" if append else "Set"
        logger.info(f"{action} scope: {len(valid_paths)} paths (total: {len(self.scoped_paths)})")
        return True
    
    def inject_file(self, file_path: str, content: Optional[str] = None) -> bool:
        """Inject a file into the current session."""
        if not self.current_session:
            logger.error("No active session. Start a session first.")
            return False
        
        path = Path(file_path)
        if not path.is_absolute():
            path = self.project_root / path
        
        if not path.exists():
            logger.error(f"File does not exist: {path}")
            return False
        
        # Read content if not provided
        if content is None:
            try:
                with open(path, 'r', encoding='utf-8', errors='ignore') as f:
                    content = f.read()
            except Exception as e:
                logger.error(f"Could not read file {path}: {e}")
                return False
        
        # Store relative path
        try:
            relative_path = str(path.relative_to(self.project_root))
        except ValueError:
            relative_path = str(path)
        
        injection_data = {
            'file_path': relative_path,
            'content': content,
            'injected_at': datetime.now().isoformat(),
            'size': len(content)
        }
        
        # Add to session
        if 'injected_files' not in self.current_session:
            self.current_session['injected_files'] = []
        
        # Remove existing injection of same file
        self.current_session['injected_files'] = [
            inj for inj in self.current_session['injected_files'] 
            if inj['file_path'] != relative_path
        ]
        
        self.current_session['injected_files'].append(injection_data)
        self.current_session['last_updated'] = datetime.now().isoformat()
        
        self._save_active_session()
        
        logger.info(f"Injected file: {relative_path} ({len(content)} chars)")
        return True
    
    def capture_log(self, log_content: str, log_type: str = 'runtime', 
                   source: Optional[str] = None) -> bool:
        """Capture log content for the current session."""
        if not self.current_session:
            logger.error("No active session. Start a session first.")
            return False
        
        log_data = {
            'content': log_content,
            'log_type': log_type,  # runtime, build, test, error
            'source': source or 'manual',
            'captured_at': datetime.now().isoformat(),
            'size': len(log_content)
        }
        
        if 'captured_logs' not in self.current_session:
            self.current_session['captured_logs'] = []
        
        self.current_session['captured_logs'].append(log_data)
        self.current_session['last_updated'] = datetime.now().isoformat()
        
        self._save_active_session()
        
        logger.info(f"Captured {log_type} log: {len(log_content)} chars from {source}")
        return True
    
    def _get_relevant_chunks(self, query: Optional[str] = None, 
                           max_chunks: int = 50) -> List[Dict[str, Any]]:
        """Get relevant code chunks for the session payload."""
        chunks = []
        
        try:
            # If we have a query, use semantic search
            if query:
                store = EmbeddingsStore(self.config, self.project_root)
                results = store.search(query, top_k=max_chunks)
                
                for result in results:
                    chunk_data = {
                        'file_path': result['metadata']['file_path'],
                        'chunk_index': result['metadata']['chunk_index'],
                        'content': result['metadata']['content'],
                        'similarity_score': result['score']
                    }
                    chunks.append(chunk_data)
            
            else:
                # Get chunks from scoped paths or all indexed files
                sync_data = self.indexer._load_sync_data()
                target_files = set()
                
                if self.scoped_paths:
                    # Only include files in scope
                    for scoped_path in self.scoped_paths:
                        scoped_full_path = self.project_root / scoped_path
                        if scoped_full_path.is_file():
                            target_files.add(scoped_path)
                        elif scoped_full_path.is_dir():
                            # Add all files in directory
                            for file_path in sync_data.get('files', {}).keys():
                                if file_path.startswith(scoped_path):
                                    target_files.add(file_path)
                else:
                    # Include all indexed files
                    target_files = set(sync_data.get('files', {}).keys())
                
                # Load chunks from target files
                chunks_dir = self.project_root / "context_engine" / "chunks"
                chunk_count = 0
                
                for file_path in target_files:
                    if chunk_count >= max_chunks:
                        break
                    
                    file_chunks_dir = chunks_dir / file_path.replace('/', '_').replace('\\', '_')
                    if file_chunks_dir.exists():
                        for chunk_file in sorted(file_chunks_dir.glob('chunk_*.json')):
                            if chunk_count >= max_chunks:
                                break
                            
                            try:
                                with open(chunk_file, 'r', encoding='utf-8') as f:
                                    chunk_data = json.load(f)
                                    chunks.append(chunk_data)
                                    chunk_count += 1
                            except Exception as e:
                                logger.warning(f"Could not load chunk {chunk_file}: {e}")
        
        except Exception as e:
            logger.warning(f"Could not get relevant chunks: {e}")
        
        return chunks
    
    def generate_payload(self, query: Optional[str] = None, 
                        include_summaries: bool = True,
                        include_logs: bool = True,
                        max_chunks: int = 50) -> Dict[str, Any]:
        """Generate AI tool payload for the current session."""
        if not self.current_session:
            logger.error("No active session. Start a session first.")
            return {}

        # Load previous handoff notes if available
        handoff_notes = self._load_previous_handoff_notes()

        payload = {
            'session_info': {
                'session_id': self.current_session['session_id'],
                'session_name': self.current_session['session_name'],
                'description': self.current_session['description'],
                'created_at': self.current_session['created_at'],
                'generated_at': datetime.now().isoformat(),
                'scoped_paths': list(self.scoped_paths),
                'previous_handoff_notes': handoff_notes
            },
            'project_context': {
                'project_root': str(self.project_root),
                'config': self.config.to_dict()
            },
            'code_chunks': [],
            'file_summaries': [],
            'injected_files': self.current_session.get('injected_files', []),
            'captured_logs': [],
            'metadata': {
                'total_chunks': 0,
                'total_summaries': 0,
                'total_logs': 0,
                'payload_size_chars': 0
            }
        }
        
        # Get relevant code chunks
        chunks = self._get_relevant_chunks(query, max_chunks)
        payload['code_chunks'] = chunks
        payload['metadata']['total_chunks'] = len(chunks)
        
        # Include file summaries if requested
        if include_summaries:
            summaries_dir = self.project_root / "context_engine" / "summaries"
            summaries = []
            
            target_files = self.scoped_paths if self.scoped_paths else set()
            if not target_files:
                # Include all summary files
                if summaries_dir.exists():
                    for summary_file in summaries_dir.glob('*.json'):
                        try:
                            with open(summary_file, 'r', encoding='utf-8') as f:
                                summary_data = json.load(f)
                                summaries.append(summary_data)
                        except Exception as e:
                            logger.warning(f"Could not load summary {summary_file}: {e}")
            else:
                # Include summaries for scoped files
                for scoped_path in target_files:
                    safe_path = scoped_path.replace('/', '_').replace('\\', '_')
                    summary_file = summaries_dir / f"{safe_path}.json"
                    if summary_file.exists():
                        try:
                            with open(summary_file, 'r', encoding='utf-8') as f:
                                summary_data = json.load(f)
                                summaries.append(summary_data)
                        except Exception as e:
                            logger.warning(f"Could not load summary {summary_file}: {e}")
            
            payload['file_summaries'] = summaries
            payload['metadata']['total_summaries'] = len(summaries)
        
        # Include captured logs if requested
        if include_logs:
            payload['captured_logs'] = self.current_session.get('captured_logs', [])
            payload['metadata']['total_logs'] = len(payload['captured_logs'])
        
        # Calculate payload size
        payload_str = json.dumps(payload, indent=2)
        payload['metadata']['payload_size_chars'] = len(payload_str)
        
        return payload
    
    def save_payload(self, payload: Dict[str, Any], filename: Optional[str] = None) -> Path:
        """Save payload to file."""
        if not filename:
            session_id = payload.get('session_info', {}).get('session_id', 'unknown')
            timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
            filename = f"payload_{session_id}_{timestamp}.json"
        
        payload_file = self.payload_dir / filename
        
        with open(payload_file, 'w', encoding='utf-8') as f:
            json.dump(payload, f, indent=2)
        
        logger.info(f"Saved payload: {payload_file} ({payload['metadata']['payload_size_chars']} chars)")
        return payload_file
    
    def get_session_status(self) -> Dict[str, Any]:
        """Get current session status."""
        if not self.current_session:
            return {'active': False}
        
        return {
            'active': True,
            'session_id': self.current_session['session_id'],
            'session_name': self.current_session['session_name'],
            'description': self.current_session['description'],
            'created_at': self.current_session['created_at'],
            'last_updated': self.current_session['last_updated'],
            'scoped_paths': list(self.scoped_paths),
            'injected_files_count': len(self.current_session.get('injected_files', [])),
            'captured_logs_count': len(self.current_session.get('captured_logs', []))
        }
    
    def list_sessions(self) -> List[Dict[str, Any]]:
        """List all sessions."""
        sessions = []
        
        if self.sessions_dir.exists():
            for session_file in self.sessions_dir.glob('*.json'):
                if session_file.name == 'active_session.json':
                    continue
                
                try:
                    with open(session_file, 'r', encoding='utf-8') as f:
                        session_data = json.load(f)
                        sessions.append({
                            'session_id': session_data['session_id'],
                            'session_name': session_data['session_name'],
                            'description': session_data['description'],
                            'created_at': session_data['created_at'],
                            'status': session_data.get('status', 'unknown'),
                            'scoped_paths_count': len(session_data.get('scoped_paths', [])),
                            'injected_files_count': len(session_data.get('injected_files', [])),
                            'captured_logs_count': len(session_data.get('captured_logs', []))
                        })
                except Exception as e:
                    logger.warning(f"Could not load session {session_file}: {e}")
        
        return sorted(sessions, key=lambda x: x['created_at'], reverse=True)

def main():
    """Main entry point for session script."""
    import argparse
    
    parser = argparse.ArgumentParser(description='Context Engine Session Manager')
    parser.add_argument('--start', metavar='NAME', help='Start a new session')
    parser.add_argument('--stop', action='store_true', help='Stop current session')
    parser.add_argument('--status', action='store_true', help='Show session status')
    parser.add_argument('--list', action='store_true', help='List all sessions')
    parser.add_argument('--generate-payload', action='store_true', help='Generate payload')
    parser.add_argument('--query', help='Query for semantic search in payload')
    
    args = parser.parse_args()
    
    try:
        config = ContextConfig.load_or_create()
        session_manager = SessionManager(config)
        
        if args.start:
            session_id = session_manager.start_session(args.start)
            print(f"Started session: {session_id}")
        
        elif args.stop:
            success = session_manager.stop_session()
            if success:
                print("Session stopped")
            else:
                print("No active session to stop")
        
        elif args.status:
            status = session_manager.get_session_status()
            print("\nSession Status:")
            for key, value in status.items():
                print(f"  {key}: {value}")
        
        elif args.list:
            sessions = session_manager.list_sessions()
            print(f"\nFound {len(sessions)} sessions:")
            for session in sessions:
                print(f"  {session['session_id']}: {session['session_name']} ({session['status']})")
        
        elif args.generate_payload:
            payload = session_manager.generate_payload(query=args.query)
            if payload:
                payload_file = session_manager.save_payload(payload)
                print(f"Generated payload: {payload_file}")
            else:
                print("No active session")
        
        else:
            parser.print_help()
        
        return 0
        
    except Exception as e:
        logger.error(f"Session command failed: {e}")
        return 1

if __name__ == '__main__':
    import sys
    sys.exit(main())