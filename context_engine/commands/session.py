"""Session command implementations for Context Engine."""

import json
import sys
from pathlib import Path

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.session import SessionManager
from context_engine.scripts.auto_capture import AutoCapture

logger = setup_logger(__name__)

def start_session_command(args) -> int:
    """Execute start-session command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Check if initialized
        context_dir = Path.cwd() / "context_engine"
        if not context_dir.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Initialize session manager
        session_manager = SessionManager(config)
        
        # Start session
        session_name = getattr(args, 'name', None)
        description = getattr(args, 'description', None)
        
        session_id = session_manager.start_session(session_name, description)
        
        # Start auto-capture monitoring
        try:
            auto_capture = AutoCapture(config)
            if auto_capture.start_monitoring():
                logger.info("Auto-capture monitoring started")
            else:
                logger.warning("Failed to start auto-capture monitoring")
        except Exception as e:
            logger.warning(f"Auto-capture initialization failed: {e}")
        
        print(f"\n✅ Started session: {session_name or session_id}")
        print(f"Session ID: {session_id}")
        if description:
            print(f"Description: {description}")
        print("🔍 Auto-capture monitoring started")
        
        print("\nNext steps:")
        print("  - Set scope: context-engine set-scope <paths>")
        print("  - Inject files: context-engine inject <file>")
        print("  - Generate payload: context-engine generate-payload")
        
        return 0
        
    except Exception as e:
        logger.error(f"Start session command failed: {e}")
        return 1

def stop_session_command(args) -> int:
    """Execute stop-session command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Initialize session manager
        session_manager = SessionManager(config)
        
        # Stop auto-capture monitoring first
        try:
            auto_capture = AutoCapture(config)
            if auto_capture.stop_monitoring():
                logger.info("Auto-capture monitoring stopped")
        except Exception as e:
            logger.warning(f"Failed to stop auto-capture monitoring: {e}")
        
        # Stop session
        success = session_manager.stop_session()
        
        if success:
            print("\n✅ Session stopped successfully")
            print("🔍 Auto-capture monitoring stopped")
            return 0
        else:
            print("\n⚠️  No active session to stop")
            return 1
        
    except Exception as e:
        logger.error(f"Stop session command failed: {e}")
        return 1

def set_scope_command(args) -> int:
    """Execute set-scope command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Initialize session manager
        session_manager = SessionManager(config)
        
        # Check for active session
        status = session_manager.get_session_status()
        if not status['active']:
            logger.error("No active session. Start a session first with 'context-engine start-session'.")
            return 1
        
        # Set scope
        paths = getattr(args, 'paths', [])
        if not paths:
            logger.error("No paths provided. Usage: context-engine set-scope <path1> [path2] ...")
            return 1
        
        success = session_manager.set_scope(paths)
        
        if success:
            print(f"\n✅ Set scope to {len(paths)} paths:")
            for path in paths:
                print(f"  - {path}")
            return 0
        else:
            logger.error("Failed to set scope")
            return 1
        
    except Exception as e:
        logger.error(f"Set scope command failed: {e}")
        return 1

def inject_command(args) -> int:
    """Execute inject command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Initialize session manager
        session_manager = SessionManager(config)
        
        # Check for active session
        status = session_manager.get_session_status()
        if not status['active']:
            logger.error("No active session. Start a session first with 'context-engine start-session'.")
            return 1
        
        # Inject file
        file_path = getattr(args, 'file_path', None)
        if not file_path:
            logger.error("No file path provided. Usage: context-engine inject <file_path>")
            return 1
        
        success = session_manager.inject_file(file_path)
        
        if success:
            print(f"\n✅ Injected file: {file_path}")
            return 0
        else:
            logger.error(f"Failed to inject file: {file_path}")
            return 1
        
    except Exception as e:
        logger.error(f"Inject command failed: {e}")
        return 1

def capture_command(args) -> int:
    """Execute capture command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Initialize session manager
        session_manager = SessionManager(config)
        
        # Check for active session
        status = session_manager.get_session_status()
        if not status['active']:
            logger.error("No active session. Start a session first with 'context-engine start-session'.")
            return 1
        
        # Get log content
        log_content = getattr(args, 'content', None)
        log_type = getattr(args, 'type', 'runtime')
        source = getattr(args, 'source', 'manual')
        
        if not log_content:
            # Try to read from stdin
            import sys
            if not sys.stdin.isatty():
                log_content = sys.stdin.read()
            else:
                logger.error("No log content provided. Usage: context-engine capture --content <content> or pipe content")
                return 1
        
        success = session_manager.capture_log(log_content, log_type, source)
        
        if success:
            print(f"\n✅ Captured {log_type} log ({len(log_content)} chars)")
            return 0
        else:
            logger.error("Failed to capture log")
            return 1
        
    except Exception as e:
        logger.error(f"Capture command failed: {e}")
        return 1

def generate_payload_command(args) -> int:
    """Execute generate-payload command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Initialize session manager
        session_manager = SessionManager(config)
        
        # Check for active session
        status = session_manager.get_session_status()
        if not status['active']:
            logger.error("No active session. Start a session first with 'context-engine start-session'.")
            return 1
        
        # Generate payload
        query = getattr(args, 'query', None)
        include_summaries = getattr(args, 'summaries', True)
        include_logs = getattr(args, 'logs', True)
        max_chunks = getattr(args, 'max_chunks', 50)
        output_file = getattr(args, 'output', None)
        
        print("\n🔄 Generating AI tool payload...")
        
        payload = session_manager.generate_payload(
            query=query,
            include_summaries=include_summaries,
            include_logs=include_logs,
            max_chunks=max_chunks
        )
        
        if not payload:
            logger.error("Failed to generate payload")
            return 1
        
        # Save payload
        payload_file = session_manager.save_payload(payload, output_file)
        
        # Show summary
        metadata = payload['metadata']
        print(f"\n✅ Generated payload: {payload_file}")
        print(f"\n📊 Payload Summary:")
        print(f"  Session: {payload['session_info']['session_name']}")
        print(f"  Code chunks: {metadata['total_chunks']}")
        print(f"  File summaries: {metadata['total_summaries']}")
        print(f"  Captured logs: {metadata['total_logs']}")
        print(f"  Injected files: {len(payload['injected_files'])}")
        print(f"  Total size: {metadata['payload_size_chars']:,} characters")
        
        if query:
            print(f"  Query: {query}")
        
        scoped_paths = payload['session_info']['scoped_paths']
        if scoped_paths:
            print(f"  Scoped paths: {len(scoped_paths)}")
        
        print(f"\n📁 Payload saved to: {payload_file}")
        
        return 0
        
    except Exception as e:
        logger.error(f"Generate payload command failed: {e}")
        return 1

def session_status_command(args) -> int:
    """Execute session status command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Initialize session manager
        session_manager = SessionManager(config)
        
        # Get session status
        status = session_manager.get_session_status()
        
        print("\n=== Session Status ===")
        
        if not status['active']:
            print("❌ No active session")
            print("\nStart a session with: context-engine start-session <name>")
        else:
            print(f"✅ Active session: {status['session_name']}")
            print(f"Session ID: {status['session_id']}")
            print(f"Description: {status['description']}")
            print(f"Created: {status['created_at']}")
            print(f"Last updated: {status['last_updated']}")
            
            print(f"\n📊 Session Data:")
            print(f"  Scoped paths: {len(status['scoped_paths'])}")
            print(f"  Injected files: {status['injected_files_count']}")
            print(f"  Captured logs: {status['captured_logs_count']}")
            
            if status['scoped_paths']:
                print(f"\n🎯 Scoped Paths:")
                for path in status['scoped_paths']:
                    print(f"  - {path}")
        
        # Show recent sessions
        sessions = session_manager.list_sessions()
        if sessions:
            print(f"\n📋 Recent Sessions ({len(sessions)}):")
            for session in sessions[:5]:  # Show last 5
                status_icon = "✅" if session['status'] == 'active' else "📁"
                print(f"  {status_icon} {session['session_name']} ({session['session_id']}) - {session['status']}")
        
        return 0
        
    except Exception as e:
        logger.error(f"Session status command failed: {e}")
        return 1