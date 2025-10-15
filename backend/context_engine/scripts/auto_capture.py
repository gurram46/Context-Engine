"""Auto-capture logs functionality for Context Engine."""

import json
import os
import psutil
import subprocess
import threading
import time
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Set

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.parsers.parser_factory import parser_factory

logger = setup_logger(__name__)

class AutoCapture:
    """Automatically capture logs from development servers and processes."""
    
    def __init__(self, config: ContextConfig, project_root: Optional[Path] = None):
        self.config = config
        self.project_root = project_root or Path.cwd()
        self.logs_dir = self.project_root / "context_engine" / "logs"
        self.errors_dir = self.logs_dir / "errors"
        
        # Ensure directories exist
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        self.errors_dir.mkdir(parents=True, exist_ok=True)
        
        # Tracking state
        self.active_captures: Dict[int, Dict] = {}  # pid -> capture info
        self.capture_threads: Dict[int, threading.Thread] = {}
        self.stop_event = threading.Event()
        
        # Development server patterns to watch for
        self.dev_server_patterns = [
            # Node.js/npm
            {'pattern': ['npm', 'run', 'dev'], 'name': 'npm_dev', 'type': 'nodejs'},
            {'pattern': ['npm', 'start'], 'name': 'npm_start', 'type': 'nodejs'},
            {'pattern': ['yarn', 'dev'], 'name': 'yarn_dev', 'type': 'nodejs'},
            {'pattern': ['yarn', 'start'], 'name': 'yarn_start', 'type': 'nodejs'},
            {'pattern': ['next', 'dev'], 'name': 'next_dev', 'type': 'nodejs'},
            {'pattern': ['vite'], 'name': 'vite_dev', 'type': 'nodejs'},
            
            # Python
            {'pattern': ['python', 'manage.py', 'runserver'], 'name': 'django_dev', 'type': 'python'},
            {'pattern': ['python', '-m', 'flask', 'run'], 'name': 'flask_dev', 'type': 'python'},
            {'pattern': ['uvicorn'], 'name': 'uvicorn_dev', 'type': 'python'},
            {'pattern': ['gunicorn'], 'name': 'gunicorn_dev', 'type': 'python'},
            
            # Java/Maven/Gradle
            {'pattern': ['mvn', 'spring-boot:run'], 'name': 'spring_boot_dev', 'type': 'java'},
            {'pattern': ['gradle', 'bootRun'], 'name': 'gradle_boot_dev', 'type': 'java'},
            {'pattern': ['./gradlew', 'bootRun'], 'name': 'gradlew_boot_dev', 'type': 'java'},
            
            # Other common dev servers
            {'pattern': ['php', 'artisan', 'serve'], 'name': 'laravel_dev', 'type': 'php'},
            {'pattern': ['rails', 'server'], 'name': 'rails_dev', 'type': 'ruby'},
            {'pattern': ['hugo', 'server'], 'name': 'hugo_dev', 'type': 'static'},
        ]
    
    def start_monitoring(self) -> bool:
        """Start monitoring for development server processes."""
        try:
            logger.info("Starting auto-capture monitoring...")
            
            # Start monitoring thread
            monitor_thread = threading.Thread(target=self._monitor_processes, daemon=True)
            monitor_thread.start()
            
            return True
            
        except Exception as e:
            logger.error(f"Failed to start auto-capture monitoring: {e}")
            return False
    
    def stop_monitoring(self) -> bool:
        """Stop monitoring and all active captures."""
        try:
            logger.info("Stopping auto-capture monitoring...")
            
            # Signal all threads to stop
            self.stop_event.set()
            
            # Wait for capture threads to finish
            for pid, thread in self.capture_threads.items():
                if thread.is_alive():
                    thread.join(timeout=5)
            
            # Clear state
            self.active_captures.clear()
            self.capture_threads.clear()
            self.stop_event.clear()
            
            logger.info("Auto-capture monitoring stopped")
            return True
            
        except Exception as e:
            logger.error(f"Failed to stop auto-capture monitoring: {e}")
            return False
    
    def _monitor_processes(self):
        """Monitor system processes for development servers."""
        seen_pids: Set[int] = set()
        
        while not self.stop_event.is_set():
            try:
                current_pids = set()
                
                # Check all running processes
                for proc in psutil.process_iter(['pid', 'name', 'cmdline', 'cwd']):
                    try:
                        proc_info = proc.info
                        pid = proc_info['pid']
                        cmdline = proc_info.get('cmdline', [])
                        cwd = proc_info.get('cwd', '')
                        
                        current_pids.add(pid)
                        
                        # Skip if already monitoring this process
                        if pid in self.active_captures:
                            continue
                        
                        # Check if process matches dev server patterns
                        server_info = self._match_dev_server(cmdline, cwd)
                        if server_info and self._is_in_project_directory(cwd):
                            logger.info(f"Detected {server_info['name']} process (PID: {pid})")
                            self._start_capture(pid, server_info, proc)
                        
                    except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess):
                        continue
                
                # Clean up captures for processes that no longer exist
                dead_pids = set(self.active_captures.keys()) - current_pids
                for pid in dead_pids:
                    self._stop_capture(pid)
                
                seen_pids = current_pids
                
            except Exception as e:
                logger.error(f"Error in process monitoring: {e}")
            
            # Wait before next check
            time.sleep(2)
    
    def _match_dev_server(self, cmdline: List[str], cwd: str) -> Optional[Dict]:
        """Check if command line matches a development server pattern."""
        if not cmdline:
            return None
        
        cmdline_str = ' '.join(cmdline).lower()
        
        for pattern_info in self.dev_server_patterns:
            pattern = pattern_info['pattern']
            
            # Check if all pattern elements are present in order
            if self._matches_pattern(cmdline, pattern):
                return pattern_info
        
        return None
    
    def _matches_pattern(self, cmdline: List[str], pattern: List[str]) -> bool:
        """Check if cmdline matches the given pattern."""
        if len(pattern) > len(cmdline):
            return False
        
        # Convert to lowercase for comparison
        cmdline_lower = [arg.lower() for arg in cmdline]
        pattern_lower = [p.lower() for p in pattern]
        
        # Check for exact sequence match
        for i in range(len(cmdline_lower) - len(pattern_lower) + 1):
            if cmdline_lower[i:i+len(pattern_lower)] == pattern_lower:
                return True
        
        return False
    
    def _is_in_project_directory(self, cwd: str) -> bool:
        """Check if the process is running in or under the project directory."""
        if not cwd:
            return False
        
        try:
            cwd_path = Path(cwd).resolve()
            project_path = self.project_root.resolve()
            
            # Check if cwd is the project root or a subdirectory
            return cwd_path == project_path or project_path in cwd_path.parents
        except Exception:
            return False
    
    def _start_capture(self, pid: int, server_info: Dict, process: psutil.Process):
        """Start capturing logs for a process."""
        try:
            capture_info = {
                'pid': pid,
                'name': server_info['name'],
                'type': server_info['type'],
                'started_at': datetime.now().isoformat(),
                'cmdline': process.cmdline(),
                'cwd': process.cwd()
            }
            
            self.active_captures[pid] = capture_info
            
            # Start capture thread
            capture_thread = threading.Thread(
                target=self._capture_process_output,
                args=(pid, capture_info),
                daemon=True
            )
            capture_thread.start()
            self.capture_threads[pid] = capture_thread
            
            logger.info(f"Started capturing {server_info['name']} (PID: {pid})")
            
        except Exception as e:
            logger.error(f"Failed to start capture for PID {pid}: {e}")
    
    def _stop_capture(self, pid: int):
        """Stop capturing logs for a process."""
        if pid in self.active_captures:
            capture_info = self.active_captures[pid]
            logger.info(f"Stopped capturing {capture_info['name']} (PID: {pid})")
            
            # Remove from tracking
            del self.active_captures[pid]
            
            if pid in self.capture_threads:
                del self.capture_threads[pid]
    
    def _capture_process_output(self, pid: int, capture_info: Dict):
        """Capture output from a specific process."""
        log_file = self.logs_dir / f"{capture_info['name']}_{pid}_{datetime.now().strftime('%Y%m%d_%H%M%S')}.log"
        parsed_file = self.errors_dir / f"{capture_info['name']}_{pid}_{datetime.now().strftime('%Y%m%d_%H%M%S')}_parsed.json"
        
        try:
            captured_content = []
            
            with open(log_file, 'w', encoding='utf-8') as f:
                f.write(f"# Auto-captured logs for {capture_info['name']} (PID: {pid})\n")
                f.write(f"# Started: {capture_info['started_at']}\n")
                f.write(f"# Command: {' '.join(capture_info['cmdline'])}\n")
                f.write(f"# Working Directory: {capture_info['cwd']}\n")
                f.write("# " + "="*60 + "\n\n")
                
                # Monitor process output
                while not self.stop_event.is_set() and pid in self.active_captures:
                    try:
                        # Check if process still exists
                        if not psutil.pid_exists(pid):
                            break
                        
                        # Try to capture process output using psutil
                        try:
                            proc = psutil.Process(pid)
                            # Get process status and basic info
                            status_info = f"[{datetime.now().isoformat()}] Process status: {proc.status()}\n"
                            f.write(status_info)
                            captured_content.append(status_info)
                            
                            # Note: Capturing stdout/stderr from running processes is complex
                            # This is a basic implementation that monitors process status
                            # For full output capture, you'd need to start processes with
                            # redirected output or use more advanced techniques
                            
                        except (psutil.NoSuchProcess, psutil.AccessDenied):
                            break
                        
                        time.sleep(5)  # Check every 5 seconds
                        
                    except Exception as e:
                        error_msg = f"[{datetime.now().isoformat()}] Error monitoring process {pid}: {e}\n"
                        f.write(error_msg)
                        captured_content.append(error_msg)
                        logger.debug(f"Error monitoring process {pid}: {e}")
                        break
                
                f.write(f"\n# Capture ended: {datetime.now().isoformat()}\n")
            
            # Parse captured content for errors
            if captured_content:
                self._parse_and_save_errors('\n'.join(captured_content), parsed_file, capture_info)
                
        except Exception as e:
            logger.error(f"Failed to write log file for PID {pid}: {e}")
    
    def get_active_captures(self) -> List[Dict]:
        """Get list of currently active captures."""
        return list(self.active_captures.values())
    
    def save_capture_state(self) -> bool:
        """Save current capture state to file."""
        try:
            state_file = self.logs_dir / "capture_state.json"
            state = {
                'active_captures': self.active_captures,
                'last_updated': datetime.now().isoformat()
            }
            
            with open(state_file, 'w', encoding='utf-8') as f:
                json.dump(state, f, indent=2)
            
            return True
            
        except Exception as e:
            logger.error(f"Failed to save capture state: {e}")
            return False
    
    def _parse_and_save_errors(self, content: str, output_file: Path, capture_info: Dict):
        """Parse captured content for errors and save results."""
        try:
            # Determine parser type based on server type
            parser_type = None
            if capture_info['type'] == 'python':
                parser_type = 'python'
            elif capture_info['type'] == 'java':
                parser_type = 'java'
            elif capture_info['type'] == 'nodejs':
                parser_type = 'javascript'
            
            # Parse content for errors
            errors = parser_factory.parse_log_content(content, parser_type=parser_type)
            
            if errors:
                # Save parsed errors
                with open(output_file, 'w', encoding='utf-8') as f:
                    json.dump({
                        'capture_info': capture_info,
                        'parsed_at': datetime.now().isoformat(),
                        'error_count': len(errors),
                        'errors': errors
                    }, f, indent=2, ensure_ascii=False)
                
                logger.info(f"Parsed {len(errors)} errors from {capture_info['name']} logs")
            
        except Exception as e:
            logger.error(f"Failed to parse captured content: {e}")
    
    def get_parsed_errors(self) -> List[Dict]:
        """Get all parsed errors from captured logs."""
        parsed_errors = []
        
        try:
            for error_file in self.errors_dir.glob('*_parsed.json'):
                with open(error_file, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                    parsed_errors.extend(data.get('errors', []))
        
        except Exception as e:
            logger.error(f"Failed to load parsed errors: {e}")
        
        return parsed_errors