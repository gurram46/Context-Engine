"""Base parser class for Context Engine log parsers."""

import json
import re
from abc import ABC, abstractmethod
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Union

class BaseParser(ABC):
    """Abstract base class for log parsers."""
    
    def __init__(self, parser_type: str):
        self.parser_type = parser_type
        self.supported_extensions = []
        self.error_patterns = []
    
    @abstractmethod
    def parse_log_content(self, content: str, source_file: Optional[Path] = None) -> List[Dict]:
        """Parse log content and extract structured error information.
        
        Args:
            content: Raw log content to parse
            source_file: Optional source file path for context
            
        Returns:
            List of parsed error dictionaries with unified format
        """
        pass
    
    @abstractmethod
    def can_parse(self, content: str, file_extension: Optional[str] = None) -> bool:
        """Check if this parser can handle the given content.
        
        Args:
            content: Log content to check
            file_extension: Optional file extension hint
            
        Returns:
            True if this parser can handle the content
        """
        pass
    
    def create_error_entry(self, 
                          message: str,
                          file_hint: Optional[str] = None,
                          line_hint: Optional[int] = None,
                          traceback: Optional[str] = None,
                          error_type: Optional[str] = None,
                          severity: str = 'error',
                          context: Optional[Dict] = None) -> Dict:
        """Create a standardized error entry.
        
        Args:
            message: Error message
            file_hint: File where error occurred
            line_hint: Line number where error occurred
            traceback: Full traceback/stack trace
            error_type: Type of error (e.g., 'SyntaxError', 'NullPointerException')
            severity: Error severity ('error', 'warning', 'info')
            context: Additional context information
            
        Returns:
            Standardized error dictionary
        """
        return {
            'message': message,
            'file_hint': file_hint,
            'line_hint': line_hint,
            'traceback': traceback,
            'error_type': error_type,
            'severity': severity,
            'parser_type': self.parser_type,
            'timestamp': datetime.now().isoformat(),
            'context': context or {}
        }
    
    def extract_file_and_line(self, text: str) -> tuple[Optional[str], Optional[int]]:
        """Extract file path and line number from text.
        
        Args:
            text: Text to search for file and line information
            
        Returns:
            Tuple of (file_path, line_number) or (None, None)
        """
        # Common patterns for file:line references
        patterns = [
            r'"([^"]+)"[,\s]*line[\s]+(\d+)',  # "file.py", line 123
            r'([^\s]+\.\w+):(\d+)',  # file.py:123
            r'File "([^"]+)", line (\d+)',  # File "file.py", line 123
            r'at ([^\s]+):(\d+)',  # at file.java:123
            r'([^\s]+)\((\d+)\)',  # file.js(123)
        ]
        
        for pattern in patterns:
            match = re.search(pattern, text, re.IGNORECASE)
            if match:
                file_path = match.group(1)
                try:
                    line_num = int(match.group(2))
                    return file_path, line_num
                except ValueError:
                    continue
        
        return None, None
    
    def clean_message(self, message: str) -> str:
        """Clean and normalize error message.
        
        Args:
            message: Raw error message
            
        Returns:
            Cleaned error message
        """
        # Remove ANSI color codes
        ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
        message = ansi_escape.sub('', message)
        
        # Remove excessive whitespace
        message = re.sub(r'\s+', ' ', message).strip()
        
        return message
    
    def save_parsed_errors(self, errors: List[Dict], output_file: Path) -> bool:
        """Save parsed errors to JSON file.
        
        Args:
            errors: List of parsed error dictionaries
            output_file: Output file path
            
        Returns:
            True if successful, False otherwise
        """
        try:
            # Ensure output directory exists
            output_file.parent.mkdir(parents=True, exist_ok=True)
            
            # Save errors as JSON
            with open(output_file, 'w', encoding='utf-8') as f:
                json.dump({
                    'parser_type': self.parser_type,
                    'parsed_at': datetime.now().isoformat(),
                    'error_count': len(errors),
                    'errors': errors
                }, f, indent=2, ensure_ascii=False)
            
            return True
            
        except Exception as e:
            print(f"Failed to save parsed errors: {e}")
            return False