"""Add-docs command implementation for Context Engine."""

import json
from pathlib import Path
from typing import List

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.session import SessionManager

logger = setup_logger(__name__)

def add_docs_command(args) -> int:
    """Execute add-docs command to include documentation files in the context."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Check if initialized
        context_dir = Path.cwd() / "context_engine"
        if not context_dir.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Get paths from arguments
        paths = getattr(args, 'paths', [])
        if not paths:
            logger.error("No paths provided. Usage: context-engine add-docs <path1> [path2] ...")
            return 1
        
        project_root = Path.cwd()
        valid_paths = []
        
        print("\n📚 Adding documentation files to context...")
        
        # Validate and collect paths
        for path_str in paths:
            path = Path(path_str)
            
            # Make path absolute if it's relative
            if not path.is_absolute():
                path = project_root / path
            
            if path.exists():
                if path.is_file():
                    valid_paths.append(path)
                    print(f"✅ Added file: {path.relative_to(project_root)}")
                elif path.is_dir():
                    # Add all markdown and text files in directory
                    doc_files = []
                    for pattern in ['**/*.md', '**/*.txt', '**/*.rst', '**/*.yaml', '**/*.yml', '**/*.json']:
                        doc_files.extend(path.glob(pattern))
                    
                    for doc_file in doc_files:
                        if not _should_ignore_file(doc_file):
                            valid_paths.append(doc_file)
                    
                    print(f"✅ Added directory: {path.relative_to(project_root)} ({len([f for f in doc_files if not _should_ignore_file(f)])} files)")
                else:
                    logger.warning(f"Path is neither file nor directory: {path}")
            else:
                logger.warning(f"Path does not exist: {path}")
        
        if not valid_paths:
            logger.error("No valid paths found to add.")
            return 1
        
        # Initialize session manager to add files to scope
        session_manager = SessionManager(config)
        
        # Check if there's an active session
        status = session_manager.get_session_status()
        if status['active']:
            # Add to current session scope
            relative_paths = [str(p.relative_to(project_root)) for p in valid_paths]
            session_manager.set_scope(relative_paths, append=True)
            print(f"\n🎯 Added {len(valid_paths)} files to active session scope")
        else:
            print(f"\n💡 No active session. Files will be available for future sessions.")
            print("   Start a session with: context-engine start-session")
        
        # Trigger reindexing for the added files
        print("\n🔄 Reindexing added documentation...")
        from context_engine.commands.reindex import reindex_command
        
        # Create a mock args object for reindex
        class MockArgs:
            def __init__(self):
                self.all = False
                self.incremental = True
        
        reindex_result = reindex_command(MockArgs())
        
        if reindex_result == 0:
            print("✅ Documentation successfully added and indexed!")
            print("\n💡 Next steps:")
            print("   - Run 'context-engine checklist' to see updated status")
            print("   - Use 'context-engine search <query>' to find relevant docs")
            if not status['active']:
                print("   - Start a session to include docs in AI context")
        else:
            print("⚠️  Documentation added but reindexing had issues")
        
        return 0
        
    except Exception as e:
        logger.error(f"Add-docs command failed: {e}")
        return 1

def _should_ignore_file(file_path: Path) -> bool:
    """Check if file should be ignored based on common ignore patterns."""
    ignore_patterns = [
        '.git', 'node_modules', '__pycache__', '.pytest_cache',
        'venv', '.venv', 'env', '.env', 'dist', 'build',
        '.context_payload', 'context_engine'
    ]
    
    # Check if any part of the path contains ignore patterns
    path_parts = file_path.parts
    for part in path_parts:
        if any(ignore in part for ignore in ignore_patterns):
            return True
    
    return False