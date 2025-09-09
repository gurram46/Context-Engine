"""Generic log parser for Context Engine."""

import re
from pathlib import Path
from typing import Dict, List, Optional

from .base_parser import BaseParser

class GenericParser(BaseParser):
    """Generic parser that uses regex patterns to match common error indicators."""
    
    def __init__(self):
        super().__init__('generic')
        self.supported_extensions = ['*']  # Supports all file types
        
        # Common error patterns across languages
        self.error_patterns = [
            # Generic error keywords
            r'(?i)\b(error|exception|fail|failed|failure)\b.*',
            r'(?i)\b(warning|warn)\b.*',
            r'(?i)\b(critical|fatal)\b.*',
            
            # Stack trace indicators
            r'(?i)\b(traceback|stack\s*trace)\b.*',
            r'(?i)\bat\s+[^\s]+\([^)]*\)',  # at method(file:line)
            
            # HTTP errors
            r'(?i)\b(4\d{2}|5\d{2})\s+(error|not\s+found|internal\s+server\s+error)',
            
            # Compilation errors
            r'(?i)\b(syntax\s*error|compile\s*error|parse\s*error)\b.*',
            
            # Runtime errors
            r'(?i)\b(runtime\s*error|null\s*pointer|segmentation\s*fault)\b.*',
        ]
        
        # Severity mapping patterns
        self.severity_patterns = {
            'critical': [r'(?i)\b(critical|fatal|severe)\b'],
            'error': [r'(?i)\b(error|exception|fail)\b'],
            'warning': [r'(?i)\b(warning|warn)\b'],
            'info': [r'(?i)\b(info|information|notice)\b']
        }
    
    def can_parse(self, content: str, file_extension: Optional[str] = None) -> bool:
        """Generic parser can handle any content as fallback."""
        return True  # Always available as fallback
    
    def parse_log_content(self, content: str, source_file: Optional[Path] = None) -> List[Dict]:
        """Parse log content using generic patterns."""
        errors = []
        lines = content.split('\n')
        
        for line_num, line in enumerate(lines, 1):
            line = line.strip()
            if not line:
                continue
            
            # Check if line matches any error pattern
            for pattern in self.error_patterns:
                if re.search(pattern, line):
                    error = self._parse_error_line(line, line_num, lines, source_file)
                    if error:
                        errors.append(error)
                    break  # Only match first pattern per line
        
        return errors
    
    def _parse_error_line(self, line: str, line_num: int, all_lines: List[str], 
                         source_file: Optional[Path]) -> Optional[Dict]:
        """Parse a single error line and extract information."""
        try:
            # Clean the message
            message = self.clean_message(line)
            
            # Determine severity
            severity = self._determine_severity(line)
            
            # Extract file and line information
            file_hint, line_hint = self.extract_file_and_line(line)
            
            # Look for additional context in surrounding lines
            context_lines = self._get_context_lines(line_num, all_lines)
            
            # Try to extract error type
            error_type = self._extract_error_type(line)
            
            # Build traceback from context if available
            traceback = self._build_traceback(line_num, all_lines)
            
            return self.create_error_entry(
                message=message,
                file_hint=file_hint or (str(source_file) if source_file else None),
                line_hint=line_hint,
                traceback=traceback,
                error_type=error_type,
                severity=severity,
                context={
                    'source_line_number': line_num,
                    'context_lines': context_lines,
                    'raw_line': line
                }
            )
            
        except Exception as e:
            # If parsing fails, create a basic error entry
            return self.create_error_entry(
                message=f"Parse error: {str(e)} | Original: {line}",
                severity='warning',
                context={'source_line_number': line_num}
            )
    
    def _determine_severity(self, line: str) -> str:
        """Determine error severity from line content."""
        for severity, patterns in self.severity_patterns.items():
            for pattern in patterns:
                if re.search(pattern, line):
                    return severity
        return 'error'  # Default severity
    
    def _extract_error_type(self, line: str) -> Optional[str]:
        """Extract error type from line."""
        # Common error type patterns
        patterns = [
            r'(?i)(\w*error)\b',
            r'(?i)(\w*exception)\b',
            r'(?i)(\w*failure)\b',
        ]
        
        for pattern in patterns:
            match = re.search(pattern, line)
            if match:
                return match.group(1)
        
        return None
    
    def _get_context_lines(self, line_num: int, all_lines: List[str], 
                          context_size: int = 2) -> List[str]:
        """Get context lines around the error line."""
        start = max(0, line_num - context_size - 1)
        end = min(len(all_lines), line_num + context_size)
        
        context = []
        for i in range(start, end):
            if i != line_num - 1:  # Don't include the error line itself
                context.append(all_lines[i].strip())
        
        return [line for line in context if line]  # Remove empty lines
    
    def _build_traceback(self, line_num: int, all_lines: List[str]) -> Optional[str]:
        """Build traceback from surrounding lines if available."""
        # Look for stack trace patterns in surrounding lines
        traceback_lines = []
        
        # Look backwards for traceback start
        start_idx = line_num - 1
        for i in range(max(0, line_num - 10), line_num):
            line = all_lines[i].strip()
            if re.search(r'(?i)\b(traceback|stack\s*trace)\b', line):
                start_idx = i
                break
        
        # Collect traceback lines
        for i in range(start_idx, min(len(all_lines), line_num + 5)):
            line = all_lines[i].strip()
            if line and (re.search(r'(?i)\bat\s+', line) or 
                        re.search(r'File "[^"]+", line \d+', line) or
                        i == line_num - 1):  # Include the error line
                traceback_lines.append(line)
        
        return '\n'.join(traceback_lines) if traceback_lines else None