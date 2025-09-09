"""Python-specific log parser for Context Engine."""

import re
from pathlib import Path
from typing import Dict, List, Optional

from .base_parser import BaseParser

class PythonParser(BaseParser):
    """Parser specialized for Python error logs and tracebacks."""
    
    def __init__(self):
        super().__init__('python')
        self.supported_extensions = ['.py', '.pyw', '.pyi']
        
        # Python-specific error patterns
        self.error_patterns = [
            # Python exceptions
            r'(?i)\b\w*Error\b.*',
            r'(?i)\b\w*Exception\b.*',
            r'(?i)\bTraceback \(most recent call last\):',
            
            # Django specific
            r'(?i)\bDjango.*Error\b.*',
            r'(?i)\bImproperlyConfigured\b.*',
            
            # Flask specific
            r'(?i)\bFlask.*Error\b.*',
            r'(?i)\bWerkzeug.*Error\b.*',
            
            # FastAPI/Uvicorn specific
            r'(?i)\bUvicorn.*Error\b.*',
            r'(?i)\bFastAPI.*Error\b.*',
        ]
        
        # Python exception types
        self.python_exceptions = [
            'SyntaxError', 'IndentationError', 'TabError',
            'NameError', 'UnboundLocalError',
            'TypeError', 'ValueError', 'AttributeError',
            'KeyError', 'IndexError', 'ZeroDivisionError',
            'FileNotFoundError', 'PermissionError', 'IOError',
            'ImportError', 'ModuleNotFoundError',
            'RuntimeError', 'RecursionError', 'MemoryError',
            'AssertionError', 'StopIteration', 'GeneratorExit',
            'SystemExit', 'KeyboardInterrupt',
            'ConnectionError', 'TimeoutError',
        ]
    
    def can_parse(self, content: str, file_extension: Optional[str] = None) -> bool:
        """Check if content appears to be Python-related."""
        # Check file extension
        if file_extension and file_extension.lower() in self.supported_extensions:
            return True
        
        # Check for Python-specific patterns
        python_indicators = [
            r'Traceback \(most recent call last\):',
            r'File "[^"]+\.py", line \d+',
            r'\b(' + '|'.join(self.python_exceptions) + r')\b',
            r'\bdjango\b.*\berror\b',
            r'\bflask\b.*\berror\b',
            r'\buvicorn\b.*\berror\b',
        ]
        
        for pattern in python_indicators:
            if re.search(pattern, content, re.IGNORECASE):
                return True
        
        return False
    
    def parse_log_content(self, content: str, source_file: Optional[Path] = None) -> List[Dict]:
        """Parse Python log content and extract structured error information."""
        errors = []
        lines = content.split('\n')
        
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            
            # Look for traceback start
            if re.match(r'Traceback \(most recent call last\):', line):
                error = self._parse_traceback(i, lines)
                if error:
                    errors.append(error)
                    # Skip to end of traceback
                    i = error['context'].get('end_line', i + 1)
                else:
                    i += 1
            
            # Look for direct exception lines
            elif self._is_exception_line(line):
                error = self._parse_exception_line(line, i, lines, source_file)
                if error:
                    errors.append(error)
                i += 1
            
            # Look for Django/Flask/FastAPI specific errors
            elif self._is_framework_error(line):
                error = self._parse_framework_error(line, i, lines, source_file)
                if error:
                    errors.append(error)
                i += 1
            
            else:
                i += 1
        
        return errors
    
    def _parse_traceback(self, start_line: int, lines: List[str]) -> Optional[Dict]:
        """Parse a complete Python traceback."""
        try:
            traceback_lines = []
            current_line = start_line
            
            # Add the "Traceback" line
            traceback_lines.append(lines[current_line])
            current_line += 1
            
            # Parse stack frames
            stack_frames = []
            while current_line < len(lines):
                line = lines[current_line].strip()
                
                # Check for file reference line
                file_match = re.match(r'File "([^"]+)", line (\d+), in (.+)', line)
                if file_match:
                    file_path = file_match.group(1)
                    line_num = int(file_match.group(2))
                    function_name = file_match.group(3)
                    
                    # Get the code line if available
                    code_line = None
                    if current_line + 1 < len(lines):
                        next_line = lines[current_line + 1].strip()
                        if next_line and not re.match(r'File "', next_line) and not self._is_exception_line(next_line):
                            code_line = next_line
                            current_line += 1
                    
                    stack_frames.append({
                        'file': file_path,
                        'line': line_num,
                        'function': function_name,
                        'code': code_line
                    })
                    
                    traceback_lines.append(line)
                    if code_line:
                        traceback_lines.append(f"    {code_line}")
                
                # Check for final exception line
                elif self._is_exception_line(line):
                    traceback_lines.append(line)
                    
                    # Parse the exception
                    error_type, message = self._parse_exception_line_content(line)
                    
                    # Get the most relevant file/line (usually the last frame)
                    file_hint = None
                    line_hint = None
                    if stack_frames:
                        last_frame = stack_frames[-1]
                        file_hint = last_frame['file']
                        line_hint = last_frame['line']
                    
                    return self.create_error_entry(
                        message=message,
                        file_hint=file_hint,
                        line_hint=line_hint,
                        traceback='\n'.join(traceback_lines),
                        error_type=error_type,
                        severity='error',
                        context={
                            'stack_frames': stack_frames,
                            'start_line': start_line,
                            'end_line': current_line,
                            'traceback_type': 'full'
                        }
                    )
                
                # If we hit an empty line or unrelated content, stop
                elif not line or not self._is_traceback_related(line):
                    break
                
                current_line += 1
            
            return None
            
        except Exception as e:
            return self.create_error_entry(
                message=f"Failed to parse traceback: {str(e)}",
                severity='warning',
                context={'start_line': start_line}
            )
    
    def _parse_exception_line(self, line: str, line_num: int, all_lines: List[str], 
                             source_file: Optional[Path]) -> Optional[Dict]:
        """Parse a standalone exception line."""
        error_type, message = self._parse_exception_line_content(line)
        
        if not error_type:
            return None
        
        # Look for file/line information in the message or surrounding lines
        file_hint, line_hint = self.extract_file_and_line(line)
        
        # If no file info found, check surrounding lines
        if not file_hint:
            for i in range(max(0, line_num - 3), min(len(all_lines), line_num + 3)):
                if i != line_num:
                    f, l = self.extract_file_and_line(all_lines[i])
                    if f:
                        file_hint, line_hint = f, l
                        break
        
        return self.create_error_entry(
            message=message,
            file_hint=file_hint or (str(source_file) if source_file else None),
            line_hint=line_hint,
            error_type=error_type,
            severity='error',
            context={
                'source_line_number': line_num,
                'traceback_type': 'single_line'
            }
        )
    
    def _parse_framework_error(self, line: str, line_num: int, all_lines: List[str], 
                              source_file: Optional[Path]) -> Optional[Dict]:
        """Parse framework-specific error lines."""
        message = self.clean_message(line)
        
        # Determine framework
        framework = None
        if re.search(r'\bdjango\b', line, re.IGNORECASE):
            framework = 'Django'
        elif re.search(r'\bflask\b', line, re.IGNORECASE):
            framework = 'Flask'
        elif re.search(r'\buvicorn\b|\bfastapi\b', line, re.IGNORECASE):
            framework = 'FastAPI/Uvicorn'
        
        # Extract error type
        error_type = self._extract_error_type(line) or f"{framework}Error"
        
        # Look for file/line information
        file_hint, line_hint = self.extract_file_and_line(line)
        
        return self.create_error_entry(
            message=message,
            file_hint=file_hint or (str(source_file) if source_file else None),
            line_hint=line_hint,
            error_type=error_type,
            severity='error',
            context={
                'framework': framework,
                'source_line_number': line_num,
                'traceback_type': 'framework_error'
            }
        )
    
    def _is_exception_line(self, line: str) -> bool:
        """Check if line contains a Python exception."""
        for exception in self.python_exceptions:
            if re.search(rf'\b{exception}\b', line):
                return True
        return False
    
    def _is_framework_error(self, line: str) -> bool:
        """Check if line contains a framework-specific error."""
        framework_patterns = [
            r'\bdjango\b.*\berror\b',
            r'\bflask\b.*\berror\b',
            r'\buvicorn\b.*\berror\b',
            r'\bfastapi\b.*\berror\b',
            r'\bImproperlyConfigured\b',
            r'\bWerkzeug.*Error\b',
        ]
        
        for pattern in framework_patterns:
            if re.search(pattern, line, re.IGNORECASE):
                return True
        return False
    
    def _is_traceback_related(self, line: str) -> bool:
        """Check if line is part of a traceback."""
        return (re.match(r'\s*File "', line) or 
                re.match(r'\s+\w+', line) or  # Indented code line
                self._is_exception_line(line))
    
    def _parse_exception_line_content(self, line: str) -> tuple[Optional[str], str]:
        """Parse exception type and message from line."""
        # Pattern: ExceptionType: message
        match = re.match(r'(\w+(?:Error|Exception)):\s*(.*)', line)
        if match:
            return match.group(1), match.group(2) or line
        
        # Check for known exception types without colon
        for exception in self.python_exceptions:
            if exception in line:
                return exception, line
        
        return None, line