"""Git integration and sync functionality for Context Engine."""

import json
import os
import re
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Set
from datetime import datetime

try:
    import git
except ImportError:
    git = None

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.embedder import FileIndexer
from context_engine.scripts.summarizer import FileSummarizer
from context_engine.scripts.embeddings_store import EmbeddingsStore

logger = setup_logger(__name__)

class GitSync:
    """Handles Git integration and synchronization."""
    
    def __init__(self, config: ContextConfig, project_root: Optional[Path] = None):
        self.config = config
        self.project_root = project_root or Path.cwd()
        self.git_dir = self.project_root / ".git"
        
        # Initialize components
        self.indexer = FileIndexer(config, project_root)
        self.summarizer = FileSummarizer(config, project_root)
        
        # Try to initialize Git repo
        self.repo = None
        if git is not None and self.git_dir.exists():
            try:
                self.repo = git.Repo(self.project_root)
            except Exception as e:
                logger.warning(f"Could not initialize Git repo: {e}")
    
    def _check_merge_conflicts(self) -> List[str]:
        """Check for active merge conflicts."""
        conflict_files = []
        
        # Check for MERGE_MSG file
        merge_msg_path = self.git_dir / "MERGE_MSG"
        if merge_msg_path.exists():
            logger.warning("Active merge detected (MERGE_MSG exists)")
        
        # Check for conflict markers in files
        if self.repo is not None:
            try:
                # Get files with conflicts from Git status
                status = self.repo.git.status("--porcelain")
                for line in status.split('\n'):
                    if line.startswith('UU ') or line.startswith('AA ') or line.startswith('DD '):
                        file_path = line[3:].strip()
                        conflict_files.append(file_path)
            except Exception as e:
                logger.warning(f"Could not check Git status: {e}")
        
        # Also check for conflict markers in files manually
        conflict_markers = ["<<<<<<< ", "=======", ">>>>>>> "]
        
        for file_path in self.indexer.get_all_files():
            try:
                with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                    content = f.read()
                    
                for marker in conflict_markers:
                    if marker in content:
                        relative_path = str(file_path.relative_to(self.project_root))
                        if relative_path not in conflict_files:
                            conflict_files.append(relative_path)
                        break
                        
            except (OSError, PermissionError):
                continue
        
        return conflict_files
    
    def _get_git_changed_files(self) -> Set[str]:
        """Get files changed according to Git."""
        changed_files = set()
        
        if self.repo is None:
            return changed_files
        
        try:
            # Get unstaged changes
            unstaged = self.repo.git.diff("--name-only")
            if unstaged:
                changed_files.update(unstaged.split('\n'))
            
            # Get staged changes
            staged = self.repo.git.diff("--cached", "--name-only")
            if staged:
                changed_files.update(staged.split('\n'))
            
            # Get untracked files
            untracked = self.repo.untracked_files
            changed_files.update(untracked)
            
        except Exception as e:
            logger.warning(f"Could not get Git changes: {e}")
        
        return {f for f in changed_files if f.strip()}
    
    def _get_recent_commits_for_file(self, file_path: Path, limit: int = 5) -> List[Dict[str, str]]:
        """Get recent commits that modified a file."""
        if self.repo is None:
            return []
        
        try:
            relative_path = str(file_path.relative_to(self.project_root))
            commits = list(self.repo.iter_commits(paths=relative_path, max_count=limit))
            
            commit_info = []
            for commit in commits:
                commit_info.append({
                    'hash': commit.hexsha[:8],
                    'message': commit.message.strip(),
                    'author': str(commit.author),
                    'date': commit.committed_datetime.isoformat()
                })
            
            return commit_info
            
        except Exception as e:
            logger.warning(f"Could not get commit history for {file_path}: {e}")
            return []
    
    def sync(self) -> bool:
        """Perform synchronization with conflict checking."""
        logger.info("Starting sync process...")
        
        # Check for merge conflicts first
        conflict_files = self._check_merge_conflicts()
        if conflict_files:
            logger.error("Merge conflicts detected in the following files:")
            for file_path in conflict_files:
                logger.error(f"  - {file_path}")
            logger.error("Please resolve conflicts before syncing.")
            return False
        
        # Get changed files from multiple sources
        git_changed = self._get_git_changed_files()
        hash_changed = set(str(f.relative_to(self.project_root)) for f in self.indexer.get_changed_files())
        
        # Combine all changed files
        all_changed = git_changed.union(hash_changed)
        
        # Filter to only include files that should be indexed
        changed_files = []
        for file_path_str in all_changed:
            file_path = self.project_root / file_path_str
            if (file_path.exists() and 
                not self.indexer._should_ignore_file(file_path) and 
                self.indexer._is_text_file(file_path)):
                changed_files.append(file_path)
        
        if not changed_files:
            logger.info("No changes detected")
            return True
        
        logger.info(f"Found {len(changed_files)} changed files to process")
        
        # Process each changed file
        success_count = 0
        for file_path in changed_files:
            try:
                logger.info(f"Processing {file_path.relative_to(self.project_root)}")
                
                # Read file content
                content = self.indexer._read_file_content(file_path)
                if not content:
                    continue
                
                # Get recent changes info
                recent_commits = self._get_recent_commits_for_file(file_path)
                recent_changes = ""
                if recent_commits:
                    recent_changes = "Recent commits:\n"
                    for commit in recent_commits[:3]:  # Show last 3 commits
                        recent_changes += f"- {commit['hash']}: {commit['message']} ({commit['author']})\n"
                else:
                    recent_changes = "No recent commit history available"
                
                # Index the file
                if self.indexer.index_file(file_path):
                    # Generate summary
                    self.summarizer.summarize_file(file_path, content, recent_changes)
                    success_count += 1
                
            except Exception as e:
                logger.error(f"Failed to process {file_path}: {e}")
        
        # Rebuild embeddings index if we have an embeddings store
        try:
            store = EmbeddingsStore(self.config, self.project_root)
            logger.info("Rebuilding embeddings index...")
            store.rebuild_index_from_chunks()
        except Exception as e:
            logger.warning(f"Could not rebuild embeddings index: {e}")
        
        # Update sync timestamp
        sync_data = self.indexer._load_sync_data()
        sync_data['last_sync'] = datetime.now().isoformat()
        sync_data['last_sync_files'] = [str(f.relative_to(self.project_root)) for f in changed_files]
        self.indexer._save_sync_data(sync_data)
        
        logger.info(f"Sync complete: {success_count}/{len(changed_files)} files processed")
        return True
    
    def get_sync_status(self) -> Dict[str, any]:
        """Get current sync status."""
        sync_data = self.indexer._load_sync_data()
        git_changed = self._get_git_changed_files()
        hash_changed = set(str(f.relative_to(self.project_root)) for f in self.indexer.get_changed_files())
        conflict_files = self._check_merge_conflicts()
        
        return {
            'last_sync': sync_data.get('last_sync'),
            'indexed_files': len(sync_data.get('files', {})),
            'git_changed_files': len(git_changed),
            'hash_changed_files': len(hash_changed),
            'conflict_files': conflict_files,
            'has_conflicts': len(conflict_files) > 0,
            'git_available': self.repo is not None
        }
    
    def create_pre_push_hook(self) -> bool:
        """Create a pre-push Git hook."""
        if not self.git_dir.exists():
            logger.error("Not a Git repository")
            return False
        
        hooks_dir = self.git_dir / "hooks"
        hooks_dir.mkdir(exist_ok=True)
        
        pre_push_hook = hooks_dir / "pre-push"
        
        hook_content = '''#!/bin/sh
# Context Engine pre-push hook
# This hook runs context-engine export --shared before pushing

echo "Running Context Engine export..."

# Run context-engine export --shared
if command -v context-engine >/dev/null 2>&1; then
    context-engine export --shared
    if [ $? -eq 0 ]; then
        echo "Context Engine export completed successfully"
        # Stage the team_context files
        git add team_context/
        if [ -n "$(git diff --cached --name-only)" ]; then
            echo "Staging team_context updates"
        fi
    else
        echo "Context Engine export failed"
        exit 1
    fi
else
    echo "Warning: context-engine not found in PATH"
fi

exit 0
'''
        
        try:
            with open(pre_push_hook, 'w', encoding='utf-8') as f:
                f.write(hook_content)
            
            # Make executable (on Unix-like systems)
            if os.name != 'nt':  # Not Windows
                os.chmod(pre_push_hook, 0o755)
            
            logger.info(f"Created pre-push hook: {pre_push_hook}")
            return True
            
        except Exception as e:
            logger.error(f"Failed to create pre-push hook: {e}")
            return False

def main():
    """Main entry point for sync script."""
    import argparse
    
    parser = argparse.ArgumentParser(description='Context Engine Git Sync')
    parser.add_argument('--status', action='store_true', help='Show sync status')
    parser.add_argument('--create-hook', action='store_true', help='Create pre-push hook')
    
    args = parser.parse_args()
    
    try:
        config = ContextConfig.load_or_create()
        sync = GitSync(config)
        
        if args.status:
            status = sync.get_sync_status()
            print("\nSync Status:")
            for key, value in status.items():
                print(f"  {key}: {value}")
        
        elif args.create_hook:
            sync.create_pre_push_hook()
        
        else:
            success = sync.sync()
            return 0 if success else 1
        
        return 0
        
    except Exception as e:
        logger.error(f"Sync failed: {e}")
        return 1

if __name__ == '__main__':
    import sys
    sys.exit(main())