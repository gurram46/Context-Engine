#!/usr/bin/env python3
"""Main CLI entry point for Context Engine."""

import argparse
import sys
import os
from pathlib import Path

# Add the parent directory to the path so we can import context_engine modules
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger

# Import command functions
from context_engine.commands.init import init_command
from context_engine.commands.reindex import reindex_command
from context_engine.commands.sync import sync_command
from context_engine.commands.search import search_command
from context_engine.commands.session import (
    start_session_command, stop_session_command, set_scope_command,
    inject_command, capture_command, generate_payload_command, session_status_command
)
from context_engine.commands.export import export_command, pull_digest_command
from context_engine.commands.status import status_command
from context_engine.commands.checklist import checklist_command
from context_engine.commands.add_docs import add_docs_command
from context_engine.commands.langchain_cmd import (
    enhanced_summarize_command, project_overview_command, 
    langchain_process_command, smart_select_command
)

logger = setup_logger(__name__)

def create_parser():
    """Create the main argument parser."""
    parser = argparse.ArgumentParser(
        prog='context-engine',
        description='Context Engine - A local project brain for dev teams'
    )
    
    subparsers = parser.add_subparsers(dest='command', help='Available commands')
    
    # Init command
    init_parser = subparsers.add_parser('init', help='Initialize context engine in current directory')
    
    # Reindex commands
    reindex_parser = subparsers.add_parser('reindex', help='Reindex project files')
    reindex_parser.add_argument('--all', action='store_true', help='Reindex all files')
    reindex_parser.add_argument('--incremental', action='store_true', help='Incremental reindex (default)')
    
    # Sync command
    sync_parser = subparsers.add_parser('sync', help='Sync changes and update index')
    
    # Search command
    search_parser = subparsers.add_parser('search', help='Search indexed content')
    search_parser.add_argument('query', help='Search query')
    search_parser.add_argument('--k', type=int, default=8, help='Number of results to return')
    
    # Session commands
    session_parser = subparsers.add_parser('start-session', help='Start a new session')
    session_parser.add_argument('--inject', choices=['trae', 'claude', 'cursor', 'warp'], help='Target AI agent')
    session_parser.add_argument('--pipe', action='store_true', help='Pipe output to stdout')
    
    stop_session_parser = subparsers.add_parser('stop-session', help='Stop current session')
    
    inject_parser = subparsers.add_parser('inject', help='Rebuild session payload')
    
    # Capture command
    capture_parser = subparsers.add_parser('capture', help='Capture command output')
    capture_parser.add_argument('--name', required=True, help='Name for the capture session')
    capture_parser.add_argument('command', nargs=argparse.REMAINDER, help='Command to capture')
    
    # Export command
    export_parser = subparsers.add_parser('export', help='Export shared digest')
    export_parser.add_argument('--shared', action='store_true', help='Export shared team context')
    
    # Pull digest command
    pull_parser = subparsers.add_parser('pull-digest', help='Pull digest from shared repo')
    pull_parser.add_argument('repo_or_path', help='Repository or path to pull from')
    
    # Scope commands
    scope_parser = subparsers.add_parser('set-scope', help='Set active scope')
    scope_parser.add_argument('paths', nargs='+', help='Paths to include in scope')
    
    # Merge command
    merge_parser = subparsers.add_parser('suggest-merge', help='Suggest merge resolution')
    merge_parser.add_argument('file', help='File with merge conflicts')
    
    # Status command
    status_parser = subparsers.add_parser('status', help='Show context engine status')
    
    # Checklist command
    checklist_parser = subparsers.add_parser('checklist', help='Check for project documentation (ADRs, architecture, user flows, API specs, team roles)')
    
    # Add-docs command
    add_docs_parser = subparsers.add_parser('add-docs', help='Add documentation files to context')
    add_docs_parser.add_argument('paths', nargs='+', help='Paths to documentation files or directories')
    
    # LangChain enhanced summarization command
    enhanced_sum_parser = subparsers.add_parser('enhanced-summarize', help='Enhanced file summarization using LangChain methods')
    enhanced_sum_parser.add_argument('files', nargs='+', help='Files or directories to summarize')
    enhanced_sum_parser.add_argument('--mode', choices=['structured', 'compressed', 'isolated', 'comprehensive'], 
                                   default='structured', help='Enhancement mode')
    enhanced_sum_parser.add_argument('--compression-ratio', type=float, default=0.5, 
                                   help='Compression ratio for compressed mode')
    enhanced_sum_parser.add_argument('--pattern-type', choices=['errors', 'functions', 'imports', 'urls', 'security', 'auto'], 
                                   default='auto', help='Pattern type for isolated mode')
    enhanced_sum_parser.add_argument('--output', help='Output file path')
    enhanced_sum_parser.add_argument('--format', choices=['text', 'json'], default='text', help='Output format')
    
    # Project overview command
    overview_parser = subparsers.add_parser('project-overview', help='Generate comprehensive project overview')
    overview_parser.add_argument('--path', help='Project path (default: current directory)')
    overview_parser.add_argument('--max-files', type=int, default=20, help='Maximum files to analyze')
    overview_parser.add_argument('--output', help='Output file path')
    overview_parser.add_argument('--format', choices=['text', 'json'], default='text', help='Output format')
    overview_parser.add_argument('--verbose', action='store_true', help='Include detailed file summaries')
    
    # LangChain processing command
    process_parser = subparsers.add_parser('langchain-process', help='Process content using LangChain methods')
    process_parser.add_argument('method', choices=['write', 'select', 'compress', 'isolate'], 
                              help='LangChain method to use')
    process_parser.add_argument('--input', default='-', help='Input file (default: stdin)')
    process_parser.add_argument('--output', help='Output file path')
    process_parser.add_argument('--format', choices=['text', 'json'], default='text', help='Output format')
    process_parser.add_argument('--template-type', choices=['code_summary', 'error_report', 'session_notes', 'documentation'], 
                              help='Template type for write method')
    process_parser.add_argument('--compression-ratio', type=float, default=0.5, 
                              help='Compression ratio for compress method')
    process_parser.add_argument('--preserve-structure', action='store_true', default=True, 
                              help='Preserve structure in compress method')
    process_parser.add_argument('--pattern-type', choices=['errors', 'functions', 'imports', 'urls', 'security'], 
                              default='functions', help='Pattern type for isolate method')
    process_parser.add_argument('--limit', type=int, default=10, help='Limit for select method')
    process_parser.add_argument('--verbose', action='store_true', help='Show detailed metadata')
    
    # Smart file selection command
    select_parser = subparsers.add_parser('smart-select', help='Smart file selection using LangChain')
    select_parser.add_argument('--directory', help='Directory to search (default: current directory)')
    select_parser.add_argument('--keywords', nargs='+', help='Keywords to match')
    select_parser.add_argument('--file-type', help='File extension to match (e.g., .py)')
    select_parser.add_argument('--recency', type=int, help='Files modified within N days')
    select_parser.add_argument('--size-range', help='Size range in bytes (min,max)')
    select_parser.add_argument('--limit', type=int, default=10, help='Maximum files to select')
    select_parser.add_argument('--output', help='Output file path')
    
    return parser

def main():
    """Main CLI entry point."""
    parser = create_parser()
    args = parser.parse_args()
    
    if not args.command:
        parser.print_help()
        return 1
    
    try:
        # Route to appropriate command handler
        if args.command == 'init':
            return init_command(args)
        elif args.command == 'reindex':
            return reindex_command(args)
        elif args.command == 'sync':
            return sync_command(args)
        elif args.command == 'search':
            return search_command(args)
        elif args.command == 'start-session':
            return start_session_command(args)
        elif args.command == 'stop-session':
            return stop_session_command(args)
        elif args.command == 'inject':
            return inject_command(args)
        elif args.command == 'capture':
            return capture_command(args)
        elif args.command == 'generate-payload':
            return generate_payload_command(args)
        elif args.command == 'export':
            return export_command(args)
        elif args.command == 'pull-digest':
            return pull_digest_command(args)
        elif args.command == 'set-scope':
            return set_scope_command(args)
        elif args.command == 'suggest-merge':
            print("Suggest merge command not yet implemented")
            return 1
        elif args.command == 'status':
            if hasattr(args, 'session') and args.session:
                return session_status_command(args)
            else:
                return status_command(args)
        elif args.command == 'checklist':
            return checklist_command(args)
        elif args.command == 'add-docs':
            return add_docs_command(args)
        elif args.command == 'enhanced-summarize':
            return enhanced_summarize_command(args)
        elif args.command == 'project-overview':
            return project_overview_command(args)
        elif args.command == 'langchain-process':
            return langchain_process_command(args)
        elif args.command == 'smart-select':
            return smart_select_command(args)
        
        else:
            logger.error(f"Unknown command: {args.command}")
            return 1
            
    except Exception as e:
        logger.error(f"Error executing command '{args.command}': {e}")
        return 1

if __name__ == '__main__':
    sys.exit(main())