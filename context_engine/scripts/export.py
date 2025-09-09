"""Export and shared digest functionality for Context Engine."""

import json
import os
import shutil
import tarfile
import zipfile
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Any, Set
import hashlib

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.embedder import FileIndexer

logger = setup_logger(__name__)

class DigestExporter:
    """Handles export and import of shared digests."""
    
    def __init__(self, config: ContextConfig, project_root: Optional[Path] = None):
        self.config = config
        self.project_root = project_root or Path.cwd()
        self.team_context_dir = self.project_root / "team_context"
        self.context_engine_dir = self.project_root / "context_engine"
        
        # Ensure team_context directory exists
        self.team_context_dir.mkdir(exist_ok=True)
        
        self.indexer = FileIndexer(config, project_root)
    
    def _get_project_metadata(self) -> Dict[str, Any]:
        """Get project metadata for the digest."""
        metadata = {
            'project_name': self.project_root.name,
            'export_timestamp': datetime.now().isoformat(),
            'context_engine_version': '1.0.0',
            'project_root': str(self.project_root),
            'config': self.config.to_dict()
        }
        
        # Add Git info if available
        try:
            import git
            if (self.project_root / ".git").exists():
                repo = git.Repo(self.project_root)
                metadata['git_info'] = {
                    'current_branch': repo.active_branch.name,
                    'current_commit': repo.head.commit.hexsha,
                    'commit_message': repo.head.commit.message.strip(),
                    'commit_author': str(repo.head.commit.author),
                    'commit_date': repo.head.commit.committed_datetime.isoformat(),
                    'is_dirty': repo.is_dirty(),
                    'untracked_files': len(repo.untracked_files)
                }
        except Exception as e:
            logger.warning(f"Could not get Git info: {e}")
            metadata['git_info'] = None
        
        return metadata
    
    def _collect_summaries(self) -> List[Dict[str, Any]]:
        """Collect all file summaries."""
        summaries = []
        summaries_dir = self.context_engine_dir / "summaries"
        
        if summaries_dir.exists():
            for summary_file in summaries_dir.glob("*.json"):
                try:
                    with open(summary_file, 'r', encoding='utf-8') as f:
                        summary_data = json.load(f)
                        summaries.append(summary_data)
                except Exception as e:
                    logger.warning(f"Could not load summary {summary_file}: {e}")
        
        return summaries
    
    def _collect_key_chunks(self, max_chunks: int = 100) -> List[Dict[str, Any]]:
        """Collect key code chunks for sharing."""
        chunks = []
        chunks_dir = self.context_engine_dir / "chunks"
        
        if not chunks_dir.exists():
            return chunks
        
        # Prioritize chunks from important files
        important_patterns = [
            'README', 'CHANGELOG', 'LICENSE', 'CONTRIBUTING',
            'package.json', 'requirements.txt', 'Cargo.toml', 'go.mod',
            'main.', 'index.', 'app.', '__init__.py', 'setup.py'
        ]
        
        # Collect chunks with priority scoring
        chunk_candidates = []
        
        for file_chunks_dir in chunks_dir.iterdir():
            if not file_chunks_dir.is_dir():
                continue
            
            file_path = file_chunks_dir.name.replace('_', '/')
            
            # Calculate priority score
            priority_score = 0
            for pattern in important_patterns:
                if pattern.lower() in file_path.lower():
                    priority_score += 10
            
            # Add chunks from this file
            for chunk_file in sorted(file_chunks_dir.glob('chunk_*.json')):
                try:
                    with open(chunk_file, 'r', encoding='utf-8') as f:
                        chunk_data = json.load(f)
                        chunk_candidates.append({
                            'chunk': chunk_data,
                            'priority': priority_score,
                            'file_path': file_path
                        })
                except Exception as e:
                    logger.warning(f"Could not load chunk {chunk_file}: {e}")
        
        # Sort by priority and take top chunks
        chunk_candidates.sort(key=lambda x: x['priority'], reverse=True)
        
        for candidate in chunk_candidates[:max_chunks]:
            chunks.append(candidate['chunk'])
        
        return chunks
    
    def _collect_project_structure(self) -> Dict[str, Any]:
        """Collect high-level project structure."""
        structure = {
            'directories': [],
            'key_files': [],
            'file_types': {},
            'total_files': 0,
            'total_size': 0
        }
        
        # Walk through project directory
        for root, dirs, files in os.walk(self.project_root):
            root_path = Path(root)
            
            # Skip ignored directories
            if self.indexer._should_ignore_file(root_path):
                continue
            
            # Add directory info
            relative_root = root_path.relative_to(self.project_root)
            if str(relative_root) != '.':
                structure['directories'].append({
                    'path': str(relative_root),
                    'file_count': len(files)
                })
            
            # Process files
            for file in files:
                file_path = root_path / file
                
                if self.indexer._should_ignore_file(file_path):
                    continue
                
                try:
                    file_size = file_path.stat().st_size
                    structure['total_files'] += 1
                    structure['total_size'] += file_size
                    
                    # Track file types
                    file_ext = file_path.suffix.lower()
                    if file_ext:
                        structure['file_types'][file_ext] = structure['file_types'].get(file_ext, 0) + 1
                    
                    # Collect key files
                    relative_file = file_path.relative_to(self.project_root)
                    if any(pattern.lower() in file.lower() for pattern in [
                        'readme', 'license', 'changelog', 'contributing',
                        'package.json', 'requirements.txt', 'cargo.toml', 'go.mod'
                    ]):
                        structure['key_files'].append({
                            'path': str(relative_file),
                            'size': file_size
                        })
                
                except (OSError, PermissionError):
                    continue
        
        return structure
    
    def create_shared_digest(self, include_chunks: bool = True, 
                           max_chunks: int = 100) -> Dict[str, Any]:
        """Create a shared digest for team collaboration."""
        logger.info("Creating shared digest...")
        
        digest = {
            'metadata': self._get_project_metadata(),
            'project_structure': self._collect_project_structure(),
            'file_summaries': self._collect_summaries(),
            'key_chunks': [],
            'sync_info': self.indexer._load_sync_data(),
            'digest_stats': {}
        }
        
        # Add key chunks if requested
        if include_chunks:
            digest['key_chunks'] = self._collect_key_chunks(max_chunks)
        
        # Calculate digest stats
        digest['digest_stats'] = {
            'total_summaries': len(digest['file_summaries']),
            'total_chunks': len(digest['key_chunks']),
            'indexed_files': len(digest['sync_info'].get('files', {})),
            'project_files': digest['project_structure']['total_files'],
            'project_size_bytes': digest['project_structure']['total_size']
        }
        
        return digest
    
    def export_shared_digest(self, output_path: Optional[Path] = None, 
                           format: str = 'json') -> Path:
        """Export shared digest to file."""
        digest = self.create_shared_digest()
        
        if not output_path:
            timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
            project_name = self.project_root.name
            
            if format == 'json':
                filename = f"{project_name}_digest_{timestamp}.json"
            elif format == 'zip':
                filename = f"{project_name}_digest_{timestamp}.zip"
            else:
                raise ValueError(f"Unsupported format: {format}")
            
            output_path = self.team_context_dir / filename
        
        if format == 'json':
            with open(output_path, 'w', encoding='utf-8') as f:
                json.dump(digest, f, indent=2)
        
        elif format == 'zip':
            # Create a zip file with the digest and additional files
            with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as zf:
                # Add main digest
                zf.writestr('digest.json', json.dumps(digest, indent=2))
                
                # Add key project files
                key_files = ['README.md', 'README.txt', 'LICENSE', 'package.json', 
                           'requirements.txt', 'Cargo.toml', 'go.mod']
                
                for key_file in key_files:
                    file_path = self.project_root / key_file
                    if file_path.exists() and file_path.is_file():
                        try:
                            zf.write(file_path, key_file)
                        except Exception as e:
                            logger.warning(f"Could not add {key_file} to zip: {e}")
        
        logger.info(f"Exported shared digest: {output_path}")
        return output_path
    
    def pull_digest(self, digest_path: Path) -> bool:
        """Pull and integrate a shared digest."""
        if not digest_path.exists():
            logger.error(f"Digest file not found: {digest_path}")
            return False
        
        try:
            # Load digest
            if digest_path.suffix == '.zip':
                with zipfile.ZipFile(digest_path, 'r') as zf:
                    digest_content = zf.read('digest.json')
                    digest = json.loads(digest_content)
            else:
                with open(digest_path, 'r', encoding='utf-8') as f:
                    digest = json.load(f)
            
            # Validate digest format
            required_keys = ['metadata', 'project_structure', 'file_summaries']
            if not all(key in digest for key in required_keys):
                logger.error("Invalid digest format")
                return False
            
            # Create integration report
            integration_report = {
                'source_digest': {
                    'project_name': digest['metadata'].get('project_name'),
                    'export_timestamp': digest['metadata'].get('export_timestamp'),
                    'git_info': digest['metadata'].get('git_info')
                },
                'integration_timestamp': datetime.now().isoformat(),
                'summaries_imported': 0,
                'chunks_imported': 0,
                'conflicts': []
            }
            
            # Import summaries (with conflict detection)
            summaries_dir = self.context_engine_dir / "summaries"
            summaries_dir.mkdir(exist_ok=True)
            
            for summary in digest.get('file_summaries', []):
                file_path = summary.get('file_path')
                if not file_path:
                    continue
                
                summary_filename = file_path.replace('/', '_').replace('\\', '_') + '.json'
                summary_file = summaries_dir / summary_filename
                
                # Check for conflicts
                if summary_file.exists():
                    with open(summary_file, 'r', encoding='utf-8') as f:
                        existing_summary = json.load(f)
                    
                    if existing_summary.get('file_hash') != summary.get('file_hash'):
                        integration_report['conflicts'].append({
                            'type': 'summary_conflict',
                            'file_path': file_path,
                            'action': 'skipped'
                        })
                        continue
                
                # Import summary
                with open(summary_file, 'w', encoding='utf-8') as f:
                    json.dump(summary, f, indent=2)
                
                integration_report['summaries_imported'] += 1
            
            # Import key chunks (as reference)
            if digest.get('key_chunks'):
                reference_chunks_dir = self.team_context_dir / "reference_chunks"
                reference_chunks_dir.mkdir(exist_ok=True)
                
                timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
                source_project = digest['metadata'].get('project_name', 'unknown')
                chunks_file = reference_chunks_dir / f"{source_project}_{timestamp}.json"
                
                with open(chunks_file, 'w', encoding='utf-8') as f:
                    json.dump(digest['key_chunks'], f, indent=2)
                
                integration_report['chunks_imported'] = len(digest['key_chunks'])
            
            # Save integration report
            reports_dir = self.team_context_dir / "integration_reports"
            reports_dir.mkdir(exist_ok=True)
            
            report_timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
            report_file = reports_dir / f"integration_{report_timestamp}.json"
            
            with open(report_file, 'w', encoding='utf-8') as f:
                json.dump(integration_report, f, indent=2)
            
            # Log results
            logger.info(f"Digest integration completed:")
            logger.info(f"  Summaries imported: {integration_report['summaries_imported']}")
            logger.info(f"  Chunks imported: {integration_report['chunks_imported']}")
            logger.info(f"  Conflicts: {len(integration_report['conflicts'])}")
            logger.info(f"  Integration report: {report_file}")
            
            return True
        
        except Exception as e:
            logger.error(f"Failed to pull digest: {e}")
            return False
    
    def list_team_digests(self) -> List[Dict[str, Any]]:
        """List available team digests."""
        digests = []
        
        if not self.team_context_dir.exists():
            return digests
        
        # Find digest files
        for digest_file in self.team_context_dir.glob("*digest*.json"):
            try:
                with open(digest_file, 'r', encoding='utf-8') as f:
                    digest = json.load(f)
                
                digests.append({
                    'file_path': str(digest_file),
                    'project_name': digest['metadata'].get('project_name'),
                    'export_timestamp': digest['metadata'].get('export_timestamp'),
                    'file_size': digest_file.stat().st_size,
                    'summaries_count': len(digest.get('file_summaries', [])),
                    'chunks_count': len(digest.get('key_chunks', []))
                })
            
            except Exception as e:
                logger.warning(f"Could not read digest {digest_file}: {e}")
        
        # Find zip digests
        for zip_file in self.team_context_dir.glob("*digest*.zip"):
            try:
                with zipfile.ZipFile(zip_file, 'r') as zf:
                    digest_content = zf.read('digest.json')
                    digest = json.loads(digest_content)
                
                digests.append({
                    'file_path': str(zip_file),
                    'project_name': digest['metadata'].get('project_name'),
                    'export_timestamp': digest['metadata'].get('export_timestamp'),
                    'file_size': zip_file.stat().st_size,
                    'summaries_count': len(digest.get('file_summaries', [])),
                    'chunks_count': len(digest.get('key_chunks', [])),
                    'format': 'zip'
                })
            
            except Exception as e:
                logger.warning(f"Could not read zip digest {zip_file}: {e}")
        
        return sorted(digests, key=lambda x: x['export_timestamp'], reverse=True)

def main():
    """Main entry point for export script."""
    import argparse
    
    parser = argparse.ArgumentParser(description='Context Engine Digest Export')
    parser.add_argument('--export', action='store_true', help='Export shared digest')
    parser.add_argument('--pull', metavar='PATH', help='Pull digest from file')
    parser.add_argument('--list', action='store_true', help='List team digests')
    parser.add_argument('--format', choices=['json', 'zip'], default='json', help='Export format')
    parser.add_argument('--output', help='Output file path')
    
    args = parser.parse_args()
    
    try:
        config = ContextConfig.load_or_create()
        exporter = DigestExporter(config)
        
        if args.export:
            output_path = Path(args.output) if args.output else None
            digest_file = exporter.export_shared_digest(output_path, args.format)
            print(f"Exported digest: {digest_file}")
        
        elif args.pull:
            success = exporter.pull_digest(Path(args.pull))
            return 0 if success else 1
        
        elif args.list:
            digests = exporter.list_team_digests()
            print(f"\nFound {len(digests)} team digests:")
            for digest in digests:
                format_info = f" ({digest.get('format', 'json')})" if digest.get('format') else ""
                print(f"  {digest['project_name']} - {digest['export_timestamp']}{format_info}")
                print(f"    File: {digest['file_path']}")
                print(f"    Size: {digest['file_size']:,} bytes")
                print(f"    Content: {digest['summaries_count']} summaries, {digest['chunks_count']} chunks")
                print()
        
        else:
            parser.print_help()
        
        return 0
        
    except Exception as e:
        logger.error(f"Export command failed: {e}")
        return 1

if __name__ == '__main__':
    import sys
    sys.exit(main())