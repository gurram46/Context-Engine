"""Log capture and parsing system for Context Engine."""

import json
import os
import re
import subprocess
import threading
import time
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Any, Callable, Set
from queue import Queue, Empty

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger

logger = setup_logger(__name__)

class LogParser:
    """Parses and categorizes log content."""
    
    def __init__(self):
        # Error patterns
        self.error_patterns = [
            r'(?i)error[:;]?\s*(.+)',
            r'(?i)exception[:;]?\s*(.+)',
            r'(?i)failed[:;]?\s*(.+)',
            r'(?i)fatal[:;]?\s*(.+)',
            r'(?i)panic[:;]?\s*(.+)',
            r'(?i)traceback',
            r'(?i)stack trace',
        ]
        
        # Warning patterns
        self.warning_patterns = [
            r'(?i)warning[:;]?\s*(.+)',
            r'(?i)warn[:;]?\s*(.+)',
            r'(?i)deprecated[:;]?\s*(.+)',
            r'(?i)caution[:;]?\s*(.+)',
        ]
        
        # Success patterns
        self.success_patterns = [
            r'(?i)success[:;]?\s*(.+)',
            r'(?i)completed[:;]?\s*(.+)',
            r'(?i)passed[:;]?\s*(.+)',
            r'(?i)ok[:;]?\s*(.+)',
            r'(?i)✓\s*(.+)',
            r'(?i)✅\s*(.+)',
        ]
        
        # Build-specific patterns
        self.build_patterns = [
            r'(?i)compiling\s+(.+)',
            r'(?i)building\s+(.+)',
            r'(?i)linking\s+(.+)',
            r'(?i)bundling\s+(.+)',
            r'(?i)webpack\s+(.+)',
            r'(?i)rollup\s+(.+)',
        ]
        
        # Test-specific patterns
        self.test_patterns = [
            r'(?i)running\s+tests?\s*(.+)',
            r'(?i)test\s+(.+?)\s+(?:passed|failed|ok|error)',
            r'(?i)\d+\s+(?:passed|failed|skipped)',
            r'(?i)coverage[:;]?\s*(.+)',
        ]
    
    def parse_log_content(self, content: str, log_type: str = 'runtime') -> Dict[str, Any]:
        """Parse log content and extract structured information."""
        lines = content.split('\n')
        
        parsed = {
            'total_lines': len(lines),
            'errors': [],
            'warnings': [],
            'successes': [],
            'build_info': [],
            'test_info': [],
            'timestamps': [],
            'log_type': log_type,
            'parsed_at': datetime.now().isoformat()
        }
        
        # Common timestamp patterns
        timestamp_patterns = [
            r'\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}',
            r'\d{2}:\d{2}:\d{2}',
            r'\[\d{2}:\d{2}:\d{2}\]',
        ]
        
        for line_num, line in enumerate(lines, 1):
            line = line.strip()
            if not line:
                continue
            
            # Extract timestamps
            for ts_pattern in timestamp_patterns:
                matches = re.findall(ts_pattern, line)
                if matches:
                    parsed['timestamps'].extend(matches)
            
            # Check for errors
            for pattern in self.error_patterns:
                match = re.search(pattern, line)
                if match:
                    parsed['errors'].append({
                        'line_number': line_num,
                        'content': line,
                        'extracted': match.group(1) if match.groups() else line
                    })
                    break
            
            # Check for warnings
            for pattern in self.warning_patterns:
                match = re.search(pattern, line)
                if match:
                    parsed['warnings'].append({
                        'line_number': line_num,
                        'content': line,
                        'extracted': match.group(1) if match.groups() else line
                    })
                    break
            
            # Check for successes
            for pattern in self.success_patterns:
                match = re.search(pattern, line)
                if match:
                    parsed['successes'].append({
                        'line_number': line_num,
                        'content': line,
                        'extracted': match.group(1) if match.groups() else line
                    })
                    break
            
            # Check for build info
            if log_type in ['build', 'runtime']:
                for pattern in self.build_patterns:
                    match = re.search(pattern, line)
                    if match:
                        parsed['build_info'].append({
                            'line_number': line_num,
                            'content': line,
                            'extracted': match.group(1) if match.groups() else line
                        })
                        break
            
            # Check for test info
            if log_type in ['test', 'runtime']:
                for pattern in self.test_patterns:
                    match = re.search(pattern, line)
                    if match:
                        parsed['test_info'].append({
                            'line_number': line_num,
                            'content': line,
                            'extracted': match.group(1) if match.groups() else line
                        })
                        break
        
        # Calculate summary stats
        parsed['summary'] = {
            'error_count': len(parsed['errors']),
            'warning_count': len(parsed['warnings']),
            'success_count': len(parsed['successes']),
            'build_events': len(parsed['build_info']),
            'test_events': len(parsed['test_info']),
            'has_timestamps': len(parsed['timestamps']) > 0
        }
        
        return parsed

class LogCapture:
    """Captures logs from various sources."""
    
    def __init__(self, config: ContextConfig, project_root: Optional[Path] = None):
        self.config = config
        self.project_root = project_root or Path.cwd()
        self.logs_dir = self.project_root / "context_engine" / "logs"
        self.errors_dir = self.logs_dir / "errors"
        
        # Ensure directories exist
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        self.errors_dir.mkdir(parents=True, exist_ok=True)
        
        self.parser = LogParser()
        
        # Active captures
        self.active_captures: Dict[str, Dict[str, Any]] = {}
        self.capture_threads: Dict[str, threading.Thread] = {}
    
    def capture_command_output(self, command: List[str], log_type: str = 'runtime',
                             capture_id: Optional[str] = None,
                             callback: Optional[Callable[[str], None]] = None) -> str:
        """Capture output from a command execution."""
        if not capture_id:
            capture_id = f"{log_type}_{int(time.time())}"
        
        logger.info(f"Starting command capture: {' '.join(command)}")
        
        try:
            # Start process
            process = subprocess.Popen(
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
                universal_newlines=True,
                cwd=self.project_root
            )
            
            output_lines = []
            
            # Read output in real-time
            while True:
                output = process.stdout.readline()
                if output == '' and process.poll() is not None:
                    break
                if output:
                    line = output.strip()
                    output_lines.append(line)
                    
                    # Call callback if provided
                    if callback:
                        callback(line)
            
            # Wait for process to complete
            return_code = process.wait()
            
            # Combine all output
            full_output = '\n'.join(output_lines)
            
            # Add return code info
            if return_code != 0:
                full_output += f"\n\nProcess exited with code: {return_code}"
            
            # Save captured log
            log_file = self._save_log(full_output, log_type, capture_id)
            
            logger.info(f"Command capture completed: {log_file}")
            return capture_id
        
        except Exception as e:
            logger.error(f"Command capture failed: {e}")
            return ""
    
    def capture_file_logs(self, file_paths: List[Path], log_type: str = 'runtime',
                         watch: bool = False) -> List[str]:
        """Capture logs from files."""
        capture_ids = []
        
        for file_path in file_paths:
            if not file_path.exists():
                logger.warning(f"Log file not found: {file_path}")
                continue
            
            try:
                # Read file content
                with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                    content = f.read()
                
                # Create capture ID
                capture_id = f"{log_type}_{file_path.stem}_{int(time.time())}"
                
                # Save log
                log_file = self._save_log(content, log_type, capture_id, source_file=file_path)
                capture_ids.append(capture_id)
                
                # Start watching if requested
                if watch:
                    self._start_file_watch(file_path, log_type)
                
                logger.info(f"Captured log from {file_path}: {log_file}")
            
            except Exception as e:
                logger.error(f"Failed to capture log from {file_path}: {e}")
        
        return capture_ids
    
    def _start_file_watch(self, file_path: Path, log_type: str):
        """Start watching a file for changes."""
        watch_id = f"watch_{file_path.stem}_{int(time.time())}"
        
        def watch_file():
            last_size = file_path.stat().st_size if file_path.exists() else 0
            
            while watch_id in self.active_captures:
                try:
                    if file_path.exists():
                        current_size = file_path.stat().st_size
                        
                        if current_size > last_size:
                            # File has grown, read new content
                            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                                f.seek(last_size)
                                new_content = f.read()
                            
                            if new_content.strip():
                                # Save new content
                                capture_id = f"{log_type}_{file_path.stem}_{int(time.time())}"
                                self._save_log(new_content, log_type, capture_id, source_file=file_path)
                            
                            last_size = current_size
                    
                    time.sleep(1)  # Check every second
                
                except Exception as e:
                    logger.error(f"File watch error for {file_path}: {e}")
                    break
        
        # Start watch thread
        self.active_captures[watch_id] = {
            'type': 'file_watch',
            'file_path': str(file_path),
            'log_type': log_type,
            'started_at': datetime.now().isoformat()
        }
        
        thread = threading.Thread(target=watch_file, daemon=True)
        thread.start()
        self.capture_threads[watch_id] = thread
        
        logger.info(f"Started file watch: {file_path}")
        return watch_id
    
    def _save_log(self, content: str, log_type: str, capture_id: str,
                 source_file: Optional[Path] = None) -> Path:
        """Save captured log content."""
        # Parse log content
        parsed = self.parser.parse_log_content(content, log_type)
        
        # Create log entry
        log_entry = {
            'capture_id': capture_id,
            'log_type': log_type,
            'captured_at': datetime.now().isoformat(),
            'source_file': str(source_file) if source_file else None,
            'content': content,
            'parsed': parsed,
            'size': len(content)
        }
        
        # Save to appropriate directory
        if parsed['summary']['error_count'] > 0:
            log_file = self.errors_dir / f"{capture_id}.json"
        else:
            log_file = self.logs_dir / f"{capture_id}.json"
        
        with open(log_file, 'w', encoding='utf-8') as f:
            json.dump(log_entry, f, indent=2)
        
        return log_file
    
    def get_recent_logs(self, log_type: Optional[str] = None, 
                       limit: int = 10) -> List[Dict[str, Any]]:
        """Get recent log entries."""
        logs = []
        
        # Collect logs from both directories
        for log_dir in [self.logs_dir, self.errors_dir]:
            if not log_dir.exists():
                continue
            
            for log_file in log_dir.glob("*.json"):
                try:
                    with open(log_file, 'r', encoding='utf-8') as f:
                        log_entry = json.load(f)
                    
                    # Filter by log type if specified
                    if log_type and log_entry.get('log_type') != log_type:
                        continue
                    
                    logs.append(log_entry)
                
                except Exception as e:
                    logger.warning(f"Could not read log file {log_file}: {e}")
        
        # Sort by capture time and limit
        logs.sort(key=lambda x: x.get('captured_at', ''), reverse=True)
        return logs[:limit]
    
    def get_log_summary(self) -> Dict[str, Any]:
        """Get summary of captured logs."""
        summary = {
            'total_logs': 0,
            'error_logs': 0,
            'by_type': {},
            'recent_errors': [],
            'active_captures': len(self.active_captures)
        }
        
        # Count logs in main directory
        if self.logs_dir.exists():
            summary['total_logs'] = len(list(self.logs_dir.glob("*.json")))
        
        # Count error logs
        if self.errors_dir.exists():
            summary['error_logs'] = len(list(self.errors_dir.glob("*.json")))
        
        # Get recent logs for analysis
        recent_logs = self.get_recent_logs(limit=50)
        
        for log_entry in recent_logs:
            log_type = log_entry.get('log_type', 'unknown')
            summary['by_type'][log_type] = summary['by_type'].get(log_type, 0) + 1
            
            # Collect recent errors
            parsed = log_entry.get('parsed', {})
            if parsed.get('summary', {}).get('error_count', 0) > 0:
                summary['recent_errors'].append({
                    'capture_id': log_entry['capture_id'],
                    'captured_at': log_entry['captured_at'],
                    'log_type': log_type,
                    'error_count': parsed['summary']['error_count'],
                    'first_error': parsed['errors'][0]['content'] if parsed['errors'] else None
                })
        
        # Limit recent errors
        summary['recent_errors'] = summary['recent_errors'][:5]
        
        return summary
    
    def stop_all_captures(self):
        """Stop all active captures."""
        for capture_id in list(self.active_captures.keys()):
            del self.active_captures[capture_id]
        
        for thread in self.capture_threads.values():
            if thread.is_alive():
                thread.join(timeout=1)
        
        self.capture_threads.clear()
        logger.info("Stopped all active captures")

def main():
    """Main entry point for log capture script."""
    import argparse
    
    parser = argparse.ArgumentParser(description='Context Engine Log Capture')
    parser.add_argument('--command', nargs='+', help='Command to capture')
    parser.add_argument('--file', action='append', help='Log file to capture')
    parser.add_argument('--type', default='runtime', help='Log type')
    parser.add_argument('--watch', action='store_true', help='Watch files for changes')
    parser.add_argument('--summary', action='store_true', help='Show log summary')
    parser.add_argument('--recent', type=int, default=10, help='Show recent logs')
    
    args = parser.parse_args()
    
    try:
        config = ContextConfig.load_or_create()
        capture = LogCapture(config)
        
        if args.command:
            capture_id = capture.capture_command_output(args.command, args.type)
            print(f"Captured command output: {capture_id}")
        
        elif args.file:
            file_paths = [Path(f) for f in args.file]
            capture_ids = capture.capture_file_logs(file_paths, args.type, args.watch)
            print(f"Captured {len(capture_ids)} log files")
            
            if args.watch:
                print("Watching files for changes... Press Ctrl+C to stop")
                try:
                    while True:
                        time.sleep(1)
                except KeyboardInterrupt:
                    capture.stop_all_captures()
                    print("\nStopped watching files")
        
        elif args.summary:
            summary = capture.get_log_summary()
            print("\nLog Summary:")
            for key, value in summary.items():
                if key == 'recent_errors' and value:
                    print(f"  {key}:")
                    for error in value:
                        print(f"    - {error['capture_id']}: {error['first_error'][:100]}...")
                else:
                    print(f"  {key}: {value}")
        
        elif args.recent:
            logs = capture.get_recent_logs(limit=args.recent)
            print(f"\nRecent {len(logs)} logs:")
            for log_entry in logs:
                parsed = log_entry.get('parsed', {})
                summary = parsed.get('summary', {})
                print(f"  {log_entry['capture_id']} ({log_entry['log_type']}) - "
                      f"Errors: {summary.get('error_count', 0)}, "
                      f"Warnings: {summary.get('warning_count', 0)}")
        
        else:
            parser.print_help()
        
        return 0
        
    except Exception as e:
        logger.error(f"Log capture failed: {e}")
        return 1

if __name__ == '__main__':
    import sys
    sys.exit(main())