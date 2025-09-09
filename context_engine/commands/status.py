"""Status command for Context Engine."""

import json
import os
from pathlib import Path
from datetime import datetime
from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger

logger = setup_logger(__name__)

def status_command() -> int:
    """Show Context Engine status."""
    try:
        project_root = Path.cwd()
        
        # Check if Context Engine is initialized
        config_path = project_root / "context_engine" / "config" / "context.yml"
        if not config_path.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Load configuration
        config = ContextConfig.load_from_file(config_path)
        
        print("\n=== Context Engine Status ===")
        print(f"Project: {config.project.name}")
        print(f"Root Directory: {project_root}")
        print(f"Initialized: Yes")
        
        # Check directory structure
        print("\n=== Directory Structure ===")
        directories = [
            "context_engine/",
            "context_engine/config/",
            "context_engine/embeddings_db/",
            "context_engine/summaries/",
            "context_engine/logs/",
            "team_context/",
            ".context_payload/"
        ]
        
        for dir_name in directories:
            dir_path = project_root / dir_name
            status = "✓" if dir_path.exists() else "✗"
            print(f"  {status} {dir_name}")
        
        # Check sync status
        print("\n=== Sync Status ===")
        sync_path = project_root / "context_engine" / "sync.json"
        if sync_path.exists():
            try:
                with open(sync_path, 'r', encoding='utf-8') as f:
                    sync_data = json.load(f)
                
                file_count = len(sync_data.get('files', {}))
                last_sync = sync_data.get('last_sync')
                
                print(f"  Indexed files: {file_count}")
                if last_sync:
                    print(f"  Last sync: {last_sync}")
                else:
                    print(f"  Last sync: Never")
            except Exception as e:
                print(f"  Error reading sync data: {e}")
        else:
            print("  Sync file: Not found")
        
        # Check embeddings
        print("\n=== Embeddings ===")
        embeddings_dir = project_root / "context_engine" / "embeddings_db"
        if embeddings_dir.exists():
            embedding_files = list(embeddings_dir.glob("*.json"))
            print(f"  Embedding files: {len(embedding_files)}")
            print(f"  Provider: {config.embedding.provider}")
            print(f"  Model: {config.embedding.model}")
        else:
            print("  Embeddings: Not initialized")
        
        # Check summaries
        print("\n=== Summaries ===")
        summaries_dir = project_root / "context_engine" / "summaries"
        if summaries_dir.exists():
            summary_files = list(summaries_dir.glob("**/*.md"))
            print(f"  Summary files: {len(summary_files)}")
        else:
            print("  Summaries: Not initialized")
        
        # Check logs
        print("\n=== Logs ===")
        logs_dir = project_root / "context_engine" / "logs"
        if logs_dir.exists():
            log_files = list(logs_dir.glob("*.log"))
            error_files = list((logs_dir / "errors").glob("*.json")) if (logs_dir / "errors").exists() else []
            print(f"  Log files: {len(log_files)}")
            print(f"  Error files: {len(error_files)}")
        else:
            print("  Logs: Not initialized")
        
        # Check Git status
        print("\n=== Git Integration ===")
        git_dir = project_root / ".git"
        if git_dir.exists():
            print("  Git repository: ✓")
            
            # Check for merge conflicts
            merge_msg = project_root / ".git" / "MERGE_MSG"
            if merge_msg.exists():
                print("  Merge conflicts: ⚠️  Active")
            else:
                print("  Merge conflicts: None")
        else:
            print("  Git repository: ✗")
        
        # Check configuration
        print("\n=== Configuration ===")
        print(f"  Chunk size: {config.embedding.chunk_size}")
        print(f"  Chunk overlap: {config.embedding.chunk_overlap}")
        print(f"  Ignore patterns: {len(config.indexing.ignore_patterns)}")
        print(f"  Redact patterns: {len(config.indexing.redact_patterns)}")
        print(f"  Shared context: {'Enabled' if config.shared_context.enabled else 'Disabled'}")
        
        # Check active scope
        print("\n=== Active Scope ===")
        scope_path = project_root / "context_engine" / "active_scope.json"
        if scope_path.exists():
            try:
                with open(scope_path, 'r', encoding='utf-8') as f:
                    scope_data = json.load(f)
                paths = scope_data.get('paths', [])
                print(f"  Scoped paths: {len(paths)}")
                for path in paths[:5]:  # Show first 5
                    print(f"    - {path}")
                if len(paths) > 5:
                    print(f"    ... and {len(paths) - 5} more")
            except Exception as e:
                print(f"  Error reading scope: {e}")
        else:
            print("  Scope: Not set (full project)")
        
        print("\n=== Recommendations ===")
        
        # Provide recommendations
        if file_count == 0:
            print("  • Run 'context-engine reindex --all' to build initial index")
        
        if not last_sync:
            print("  • Run 'context-engine sync' to update index with recent changes")
        
        if len(embedding_files) == 0:
            print("  • Embeddings not generated. Reindexing will create them.")
        
        if not git_dir.exists():
            print("  • Initialize Git repository for better change tracking")
        
        print("")
        return 0
        
    except Exception as e:
        logger.error(f"Failed to get status: {e}")
        return 1