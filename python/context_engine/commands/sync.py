"""Sync command implementation for Context Engine."""

import sys
from pathlib import Path

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.sync import GitSync

logger = setup_logger(__name__)

def sync_command(args) -> int:
    """Execute sync command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Check if initialized
        context_dir = Path.cwd() / "context_engine"
        if not context_dir.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Initialize GitSync
        git_sync = GitSync(config)
        
        # Check for conflicts first if requested
        if hasattr(args, 'check_conflicts') and args.check_conflicts:
            status = git_sync.get_sync_status()
            if status['has_conflicts']:
                logger.error("Merge conflicts detected:")
                for conflict_file in status['conflict_files']:
                    logger.error(f"  - {conflict_file}")
                logger.error("Please resolve conflicts before syncing.")
                return 1
            else:
                logger.info("No merge conflicts detected.")
                return 0
        
        # Show status if requested
        if hasattr(args, 'status') and args.status:
            status = git_sync.get_sync_status()
            
            print("\n=== Sync Status ===")
            print(f"Last sync: {status['last_sync'] or 'Never'}")
            print(f"Indexed files: {status['indexed_files']}")
            print(f"Git changed files: {status['git_changed_files']}")
            print(f"Hash changed files: {status['hash_changed_files']}")
            print(f"Git available: {status['git_available']}")
            
            if status['has_conflicts']:
                print(f"\n⚠️  CONFLICTS DETECTED ({len(status['conflict_files'])} files):")
                for conflict_file in status['conflict_files']:
                    print(f"  - {conflict_file}")
                print("\nResolve conflicts before syncing.")
            else:
                print("\n✅ No conflicts detected")
            
            total_changes = status['git_changed_files'] + status['hash_changed_files']
            if total_changes > 0:
                print(f"\n📝 {total_changes} files need processing")
                print("Run 'context-engine sync' to process changes.")
            else:
                print("\n✅ All files are up to date")
            
            return 0
        
        # Create Git hook if requested
        if hasattr(args, 'create_hook') and args.create_hook:
            success = git_sync.create_pre_push_hook()
            if success:
                logger.info("Pre-push hook created successfully")
                return 0
            else:
                logger.error("Failed to create pre-push hook")
                return 1
        
        # Perform sync
        logger.info("Starting synchronization...")
        success = git_sync.sync()
        
        if success:
            logger.info("Synchronization completed successfully")
            return 0
        else:
            logger.error("Synchronization failed")
            return 1
    
    except KeyboardInterrupt:
        logger.info("Sync cancelled by user")
        return 1
    except Exception as e:
        logger.error(f"Sync command failed: {e}")
        return 1