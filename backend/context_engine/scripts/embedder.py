"""File indexer and embedder for Context Engine."""

import hashlib
import json
import os
import re
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Any
from datetime import datetime
import fnmatch

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger

logger = setup_logger(__name__)

class FileIndexer:
    """Handles file indexing with SHA256 tracking and chunking."""
    
    def __init__(self, config: ContextConfig, project_root: Optional[Path] = None):
        self.config = config
        self.project_root = project_root or Path.cwd()
        self.embeddings_db_dir = self.project_root / "context_engine" / "embeddings_db"
        self.sync_file = self.project_root / "context_engine" / "sync.json"
        
        # Ensure directories exist
        self.embeddings_db_dir.mkdir(parents=True, exist_ok=True)
    
    def _should_ignore_file(self, file_path: Path) -> bool:
        """Check if file should be ignored based on patterns."""
        relative_path = file_path.relative_to(self.project_root)
        path_str = str(relative_path).replace('\\', '/')
        
        for pattern in self.config.indexing.ignore_patterns:
            if fnmatch.fnmatch(path_str, pattern) or fnmatch.fnmatch(file_path.name, pattern):
                return True
        
        return False
    
    def _is_text_file(self, file_path: Path) -> bool:
        """Check if file is a text file that should be indexed."""
        # Check file size
        try:
            size_mb = file_path.stat().st_size / (1024 * 1024)
            if size_mb > self.config.indexing.max_file_size_mb:
                return False
        except OSError:
            return False
        
        # Check if it's a binary file by reading first few bytes
        try:
            with open(file_path, 'rb') as f:
                chunk = f.read(1024)
                if b'\0' in chunk:
                    return False
        except (OSError, PermissionError):
            return False
        
        # Common text file extensions
        text_extensions = {
            '.py', '.js', '.ts', '.jsx', '.tsx', '.java', '.c', '.cpp', '.h', '.hpp',
            '.cs', '.php', '.rb', '.go', '.rs', '.swift', '.kt', '.scala',
            '.html', '.htm', '.css', '.scss', '.sass', '.less',
            '.xml', '.json', '.yaml', '.yml', '.toml', '.ini', '.cfg',
            '.md', '.txt', '.rst', '.adoc',
            '.sql', '.sh', '.bash', '.zsh', '.fish', '.ps1', '.bat', '.cmd',
            '.dockerfile', '.makefile', '.cmake',
            '.r', '.m', '.pl', '.lua', '.vim', '.el'
        }
        
        if file_path.suffix.lower() in text_extensions:
            return True
        
        # Files without extensions that are commonly text
        if not file_path.suffix and file_path.name.lower() in {
            'makefile', 'dockerfile', 'readme', 'license', 'changelog',
            'authors', 'contributors', 'copying', 'install', 'news'
        }:
            return True
        
        return False
    
    def _compute_file_hash(self, file_path: Path) -> str:
        """Compute SHA256 hash of file content."""
        sha256_hash = hashlib.sha256()
        try:
            with open(file_path, 'rb') as f:
                for chunk in iter(lambda: f.read(4096), b""):
                    sha256_hash.update(chunk)
            return sha256_hash.hexdigest()
        except (OSError, PermissionError) as e:
            logger.warning(f"Could not read file {file_path}: {e}")
            return ""
    
    def _read_file_content(self, file_path: Path) -> str:
        """Read file content with encoding detection."""
        encodings = ['utf-8', 'utf-16', 'latin-1', 'cp1252']
        
        for encoding in encodings:
            try:
                with open(file_path, 'r', encoding=encoding) as f:
                    return f.read()
            except (UnicodeDecodeError, UnicodeError):
                continue
            except (OSError, PermissionError) as e:
                logger.warning(f"Could not read file {file_path}: {e}")
                return ""
        
        logger.warning(f"Could not decode file {file_path} with any encoding")
        return ""
    
    def _redact_secrets(self, content: str) -> str:
        """Redact secrets from content using configured patterns."""
        redacted_content = content
        
        for pattern in self.config.indexing.redact_patterns:
            try:
                redacted_content = re.sub(pattern, '[REDACTED]', redacted_content, flags=re.MULTILINE)
            except re.error as e:
                logger.warning(f"Invalid redaction pattern '{pattern}': {e}")
        
        return redacted_content
    
    def _chunk_content(self, content: str, file_path: Path) -> List[Dict[str, Any]]:
        """Split content into chunks with overlap."""
        chunk_size = self.config.embedding.chunk_size
        overlap = self.config.embedding.chunk_overlap
        
        if len(content) <= chunk_size:
            return [{
                'chunk_index': 0,
                'text': content,
                'start_char': 0,
                'end_char': len(content),
                'file_path': str(file_path.relative_to(self.project_root)),
                'file_hash': self._compute_file_hash(file_path)
            }]
        
        chunks = []
        start = 0
        chunk_index = 0
        
        while start < len(content):
            end = min(start + chunk_size, len(content))
            
            # Try to break at word boundaries
            if end < len(content):
                # Look for good break points (newlines, periods, etc.)
                break_chars = ['\n\n', '\n', '. ', '; ', ', ']
                for break_char in break_chars:
                    last_break = content.rfind(break_char, start, end)
                    if last_break > start + chunk_size // 2:  # Don't break too early
                        end = last_break + len(break_char)
                        break
            
            chunk_text = content[start:end].strip()
            if chunk_text:  # Only add non-empty chunks
                chunks.append({
                    'chunk_index': chunk_index,
                    'text': chunk_text,
                    'start_char': start,
                    'end_char': end,
                    'file_path': str(file_path.relative_to(self.project_root)),
                    'file_hash': self._compute_file_hash(file_path)
                })
                chunk_index += 1
            
            # Move start position with overlap
            start = max(start + 1, end - overlap)
        
        return chunks
    
    def _save_chunks(self, chunks: List[Dict[str, Any]], file_path: Path) -> None:
        """Save chunks to embeddings database."""
        relative_path = file_path.relative_to(self.project_root)
        safe_path = str(relative_path).replace('/', '_').replace('\\', '_').replace(':', '_')
        
        chunk_file = self.embeddings_db_dir / f"{safe_path}.json"
        
        try:
            with open(chunk_file, 'w', encoding='utf-8') as f:
                json.dump({
                    'file_path': str(relative_path),
                    'chunks': chunks,
                    'indexed_at': datetime.now().isoformat(),
                    'total_chunks': len(chunks)
                }, f, indent=2, ensure_ascii=False)
            
            logger.debug(f"Saved {len(chunks)} chunks for {relative_path}")
        except (OSError, PermissionError) as e:
            logger.error(f"Could not save chunks for {file_path}: {e}")
    
    def _load_sync_data(self) -> Dict[str, Any]:
        """Load sync data from sync.json."""
        if not self.sync_file.exists():
            return {'files': {}, 'last_sync': None}
        
        try:
            with open(self.sync_file, 'r', encoding='utf-8') as f:
                return json.load(f)
        except (json.JSONDecodeError, OSError) as e:
            logger.warning(f"Could not load sync data: {e}")
            return {'files': {}, 'last_sync': None}
    
    def _save_sync_data(self, sync_data: Dict[str, Any]) -> None:
        """Save sync data to sync.json."""
        try:
            with open(self.sync_file, 'w', encoding='utf-8') as f:
                json.dump(sync_data, f, indent=2, ensure_ascii=False)
        except (OSError, PermissionError) as e:
            logger.error(f"Could not save sync data: {e}")
    
    def get_all_files(self) -> List[Path]:
        """Get all files in the project that should be indexed."""
        files = []
        
        for root, dirs, filenames in os.walk(self.project_root):
            # Skip ignored directories
            dirs[:] = [d for d in dirs if not self._should_ignore_file(Path(root) / d)]
            
            for filename in filenames:
                file_path = Path(root) / filename
                
                if not self._should_ignore_file(file_path) and self._is_text_file(file_path):
                    files.append(file_path)
        
        return files
    
    def get_changed_files(self) -> List[Path]:
        """Get files that have changed since last sync."""
        sync_data = self._load_sync_data()
        all_files = self.get_all_files()
        changed_files = []
        
        for file_path in all_files:
            relative_path = str(file_path.relative_to(self.project_root))
            current_hash = self._compute_file_hash(file_path)
            
            if not current_hash:  # Skip files we can't read
                continue
            
            stored_info = sync_data['files'].get(relative_path)
            if not stored_info or stored_info.get('hash') != current_hash:
                changed_files.append(file_path)
        
        return changed_files
    
    def index_file(self, file_path: Path) -> bool:
        """Index a single file."""
        try:
            logger.info(f"Indexing {file_path.relative_to(self.project_root)}")
            
            # Read and process content
            content = self._read_file_content(file_path)
            if not content:
                return False
            
            # Redact secrets
            content = self._redact_secrets(content)
            
            # Create chunks
            chunks = self._chunk_content(content, file_path)
            
            # Save chunks
            self._save_chunks(chunks, file_path)
            
            # Update sync data
            sync_data = self._load_sync_data()
            relative_path = str(file_path.relative_to(self.project_root))
            
            sync_data['files'][relative_path] = {
                'hash': self._compute_file_hash(file_path),
                'indexed_at': datetime.now().isoformat(),
                'chunk_count': len(chunks)
            }
            
            self._save_sync_data(sync_data)
            
            return True
            
        except Exception as e:
            logger.error(f"Failed to index {file_path}: {e}")
            return False
    
    def reindex_all(self) -> int:
        """Reindex all files in the project."""
        logger.info("Starting full reindex...")
        
        files = self.get_all_files()
        logger.info(f"Found {len(files)} files to index")
        
        success_count = 0
        for file_path in files:
            if self.index_file(file_path):
                success_count += 1
        
        # Update last sync time
        sync_data = self._load_sync_data()
        sync_data['last_sync'] = datetime.now().isoformat()
        self._save_sync_data(sync_data)
        
        logger.info(f"Reindex complete: {success_count}/{len(files)} files indexed")
        return success_count
    
    def reindex_incremental(self) -> int:
        """Reindex only changed files."""
        logger.info("Starting incremental reindex...")
        
        changed_files = self.get_changed_files()
        logger.info(f"Found {len(changed_files)} changed files")
        
        if not changed_files:
            logger.info("No changes detected")
            return 0
        
        success_count = 0
        for file_path in changed_files:
            if self.index_file(file_path):
                success_count += 1
        
        # Update last sync time
        sync_data = self._load_sync_data()
        sync_data['last_sync'] = datetime.now().isoformat()
        self._save_sync_data(sync_data)
        
        logger.info(f"Incremental reindex complete: {success_count}/{len(changed_files)} files indexed")
        return success_count

def main():
    """Main entry point for embedder script."""
    import argparse
    
    parser = argparse.ArgumentParser(description='Context Engine File Indexer')
    parser.add_argument('--reindex-all', action='store_true', help='Reindex all files')
    parser.add_argument('--incremental', action='store_true', help='Incremental reindex')
    
    args = parser.parse_args()
    
    try:
        config = ContextConfig.load_or_create()
        indexer = FileIndexer(config)
        
        if args.reindex_all:
            indexer.reindex_all()
        elif args.incremental:
            indexer.reindex_incremental()
        else:
            print("Please specify --reindex-all or --incremental")
            return 1
        
        return 0
        
    except Exception as e:
        logger.error(f"Indexer failed: {e}")
        return 1

if __name__ == '__main__':
    import sys
    sys.exit(main())