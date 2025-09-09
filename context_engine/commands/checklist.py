"""Checklist command implementation for Context Engine."""

import json
from pathlib import Path
from typing import Dict, List, Tuple

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger

logger = setup_logger(__name__)

def checklist_command(args) -> int:
    """Execute checklist command to check for project documentation."""
    try:
        # Load configuration
        config = ContextConfig.load_or_create()
        
        # Check if initialized
        context_dir = Path.cwd() / "context_engine"
        if not context_dir.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        print("\n📋 Project Documentation Checklist")
        print("=" * 40)
        
        # Define checklist items with their patterns
        checklist_items = [
            {
                'name': 'ADRs (Architectural Decision Records)',
                'patterns': ['**/adr/**/*.md', '**/adrs/**/*.md', '**/decisions/**/*.md', '**/*adr*.md'],
                'description': 'Documents architectural decisions and their rationale'
            },
            {
                'name': 'Architecture Documentation',
                'patterns': ['**/architecture/**/*.md', '**/arch/**/*.md', '**/*architecture*.md', '**/docs/architecture/**/*'],
                'description': 'High-level system architecture and design documents'
            },
            {
                'name': 'User Flows',
                'patterns': ['**/flows/**/*.md', '**/user-flows/**/*.md', '**/*flow*.md', '**/docs/flows/**/*'],
                'description': 'User journey and workflow documentation'
            },
            {
                'name': 'API Specifications',
                'patterns': ['**/api/**/*.md', '**/swagger/**/*', '**/openapi/**/*', '**/*api*.md', '**/*.yaml', '**/*.yml', '**/*.json'],
                'description': 'API documentation and specifications'
            },
            {
                'name': 'Team Roles Documentation',
                'patterns': ['**/team/**/*.md', '**/roles/**/*.md', '**/*team*.md', '**/*roles*.md', '**/TEAM.md', '**/ROLES.md'],
                'description': 'Team structure, roles, and responsibilities'
            }
        ]
        
        project_root = Path.cwd()
        results = []
        
        for item in checklist_items:
            files_found = find_files_by_patterns(project_root, item['patterns'])
            results.append({
                'name': item['name'],
                'description': item['description'],
                'files': files_found,
                'count': len(files_found)
            })
        
        # Display results
        all_good = True
        for result in results:
            status = "✅" if result['count'] > 0 else "❌"
            if result['count'] == 0:
                all_good = False
            
            print(f"\n{status} {result['name']}")
            print(f"   {result['description']}")
            print(f"   Found: {result['count']} file(s)")
            
            if result['count'] > 0:
                for file_path in result['files'][:3]:  # Show first 3 files
                    rel_path = file_path.relative_to(project_root)
                    print(f"     - {rel_path}")
                if result['count'] > 3:
                    print(f"     ... and {result['count'] - 3} more")
            else:
                print(f"     💡 Suggestion: Use 'context-engine add-docs <path>' to include relevant files")
        
        # Summary
        total_items = len(results)
        completed_items = sum(1 for r in results if r['count'] > 0)
        
        print(f"\n📊 Summary: {completed_items}/{total_items} documentation types found")
        
        if all_good:
            print("🎉 Great! Your project has good documentation coverage.")
        else:
            print("\n💡 Recommendations:")
            print("   - Consider adding missing documentation types")
            print("   - Use 'context-engine add-docs <path>' to include existing docs")
            print("   - Run 'context-engine reindex' after adding documentation")
        
        return 0
        
    except Exception as e:
        logger.error(f"Checklist command failed: {e}")
        return 1

def find_files_by_patterns(root_path: Path, patterns: List[str]) -> List[Path]:
    """Find files matching any of the given glob patterns."""
    found_files = set()
    
    for pattern in patterns:
        try:
            # Use glob to find matching files
            matches = list(root_path.glob(pattern))
            for match in matches:
                if match.is_file() and not _should_ignore_file(match):
                    found_files.add(match)
        except Exception as e:
            logger.debug(f"Error matching pattern '{pattern}': {e}")
    
    return sorted(list(found_files))

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