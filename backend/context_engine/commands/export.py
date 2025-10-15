"""Export command implementation for Context Engine."""

import sys
from pathlib import Path

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.export import DigestExporter

logger = setup_logger(__name__)

def export_command(args) -> int:
    """Execute export command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Check if initialized
        context_dir = Path.cwd() / "context_engine"
        if not context_dir.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Initialize exporter
        exporter = DigestExporter(config)
        
        # Handle different export operations
        if hasattr(args, 'shared') and args.shared:
            # Export shared digest
            output_path = None
            if hasattr(args, 'output') and args.output:
                output_path = Path(args.output)
            
            export_format = getattr(args, 'format', 'json')
            
            print("\n🔄 Creating shared digest...")
            digest_file = exporter.export_shared_digest(output_path, export_format)
            
            # Show summary
            digest = exporter.create_shared_digest()
            stats = digest['digest_stats']
            
            print(f"\n✅ Shared digest exported: {digest_file}")
            print(f"\n📊 Digest Summary:")
            print(f"  Project: {digest['metadata']['project_name']}")
            print(f"  File summaries: {stats['total_summaries']}")
            print(f"  Key chunks: {stats['total_chunks']}")
            print(f"  Indexed files: {stats['indexed_files']}")
            print(f"  Project files: {stats['project_files']}")
            print(f"  Project size: {stats['project_size_bytes']:,} bytes")
            
            git_info = digest['metadata'].get('git_info')
            if git_info:
                print(f"\n🔗 Git Info:")
                print(f"  Branch: {git_info['current_branch']}")
                print(f"  Commit: {git_info['current_commit'][:8]}")
                print(f"  Message: {git_info['commit_message']}")
                if git_info['is_dirty']:
                    print(f"  ⚠️  Working directory has uncommitted changes")
            
            return 0
        
        elif hasattr(args, 'list') and args.list:
            # List team digests
            digests = exporter.list_team_digests()
            
            if not digests:
                print("\n📁 No team digests found in team_context/")
                print("\nCreate a digest with: context-engine export --shared")
                return 0
            
            print(f"\n📋 Found {len(digests)} team digests:")
            print()
            
            for i, digest in enumerate(digests, 1):
                format_info = f" ({digest.get('format', 'json')})" if digest.get('format') else ""
                print(f"{i}. {digest['project_name']}{format_info}")
                print(f"   Exported: {digest['export_timestamp']}")
                print(f"   File: {Path(digest['file_path']).name}")
                print(f"   Size: {digest['file_size']:,} bytes")
                print(f"   Content: {digest['summaries_count']} summaries, {digest['chunks_count']} chunks")
                print()
            
            return 0
        
        else:
            # Default to shared export
            print("\n🔄 Creating shared digest...")
            digest_file = exporter.export_shared_digest()
            print(f"\n✅ Shared digest exported: {digest_file}")
            return 0
        
    except Exception as e:
        logger.error(f"Export command failed: {e}")
        return 1

def pull_digest_command(args) -> int:
    """Execute pull-digest command."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Check if initialized
        context_dir = Path.cwd() / "context_engine"
        if not context_dir.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Get digest path
        digest_path = getattr(args, 'digest_path', None)
        if not digest_path:
            logger.error("No digest path provided. Usage: context-engine pull-digest <path>")
            return 1
        
        digest_file = Path(digest_path)
        if not digest_file.exists():
            logger.error(f"Digest file not found: {digest_file}")
            return 1
        
        # Initialize exporter
        exporter = DigestExporter(config)
        
        print(f"\n🔄 Pulling digest from: {digest_file}")
        
        # Pull digest
        success = exporter.pull_digest(digest_file)
        
        if success:
            print("\n✅ Digest integration completed successfully")
            print("\nCheck team_context/integration_reports/ for detailed results")
            return 0
        else:
            print("\n❌ Digest integration failed")
            return 1
        
    except Exception as e:
        logger.error(f"Pull digest command failed: {e}")
        return 1