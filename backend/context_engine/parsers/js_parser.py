"""JavaScript/Node.js-specific log parser for Context Engine."""

import re
from pathlib import Path
from typing import Dict, List, Optional

from .base_parser import BaseParser

class JSParser(BaseParser):
    """Parser specialized for JavaScript/Node.js error logs and stack traces."""
    
    def __init__(self):
        super().__init__('javascript')
        self.supported_extensions = ['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs']
        
        # JavaScript-specific error patterns
        self.error_patterns = [
            # JavaScript errors
            r'(?i)\b\w*Error\b.*',
            r'(?i)\bUncaught.*',
            r'(?i)\bUnhandled.*',
            r'(?i)\bReferenceError\b.*',
            r'(?i)\bTypeError\b.*',
            r'(?i)\bSyntaxError\b.*',
            
            # Node.js specific
            r'(?i)\bnode:.*error\b.*',
            r'(?i)\bmodule not found\b.*',
            r'(?i)\bcannot find module\b.*',
            
            # React specific
            r'(?i)\breact.*error\b.*',
            r'(?i)\bwarning:.*react.*',
            
            # Build tool errors
            r'(?i)\bwebpack.*error\b.*',
            r'(?i)\bvite.*error\b.*',
            r'(?i)\bbuild failed\b.*',
        ]
        
        # Common JavaScript errors
        self.js_errors = [
            'Error', 'TypeError', 'ReferenceError', 'SyntaxError',
            'RangeError', 'EvalError', 'URIError',
            'AggregateError', 'InternalError',
            # Node.js specific
            'AssertionError', 'SystemError',
            # Browser specific
            'DOMException', 'SecurityError',
        ]
    
    def can_parse(self, content: str, file_extension: Optional[str] = None) -> bool:
        """Check if content appears to be JavaScript-related."""
        # Check file extension
        if file_extension and file_extension.lower() in self.supported_extensions:
            return True
        
        # Check for JavaScript-specific patterns
        js_indicators = [
            r'\bat [^\s]+\([^)]*\)',  # JavaScript stack trace format
            r'\b(' + '|'.join(self.js_errors) + r')\b',
            r'\bnode:',
            r'\bmodule not found\b',
            r'\breact\b.*\berror\b',
            r'\bwebpack\b.*\berror\b',
            r'\bvite\b.*\berror\b',
            r'\bconsole\.(error|warn)\b',
        ]
        
        for pattern in js_indicators:
            if re.search(pattern, content, re.IGNORECASE):
                return True
        
        return False
    
    def parse_log_content(self, content: str, source_file: Optional[Path] = None) -> List[Dict]:
        """Parse JavaScript log content and extract structured error information."""
        errors = []
        lines = content.split('\n')
        
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            
            # Look for error start
            if self._is_error_start(line):
                error = self._parse_error_stack(i, lines)
                if error:
                    errors.append(error)
                    # Skip to end of stack trace
                    i = error['context'].get('end_line', i + 1)
                else:
                    i += 1
            
            # Look for console errors
            elif self._is_console_error(line):
                error = self._parse_console_error(line, i, lines, source_file)
                if error:
                    errors.append(error)
                i += 1
            
            # Look for build/compilation errors
            elif self._is_build_error(line):
                error = self._parse_build_error(line, i, lines, source_file)
                if error:
                    errors.append(error)
                i += 1
            
            # Look for React warnings/errors
            elif self._is_react_error(line):
                error = self._parse_react_error(line, i, lines, source_file)
                if error:
                    errors.append(error)
                i += 1
            
            else:
                i += 1
        
        return errors
    
    def _parse_error_stack(self, start_line: int, lines: List[str]) -> Optional[Dict]:
        """Parse a JavaScript error with stack trace."""
        try:
            stack_lines = []
            current_line = start_line
            
            # Parse the main error line
            error_line = lines[current_line].strip()
            stack_lines.append(error_line)
            
            # Extract error type and message
            error_type, message = self._parse_error_line_content(error_line)
            
            current_line += 1
            stack_frames = []
            
            # Parse stack trace lines
            while current_line < len(lines):
                line = lines[current_line].strip()
                
                # Check for stack trace line (at ...)
                at_match = re.match(r'at\s+([^\s]+)\s*\(([^)]+)\)', line)
                if at_match:
                    function_name = at_match.group(1)
                    location = at_match.group(2)
                    
                    # Parse location (file:line:column)
                    file_hint = None
                    line_hint = None
                    column_hint = None
                    
                    # Handle different location formats
                    if ':' in location:
                        parts = location.split(':')
                        if len(parts) >= 2:
                            file_hint = parts[0]
                            try:
                                line_hint = int(parts[1])
                                if len(parts) >= 3:
                                    column_hint = int(parts[2])
                            except ValueError:
                                pass
                    
                    stack_frames.append({
                        'function': function_name,
                        'file': file_hint,
                        'line': line_hint,
                        'column': column_hint,
                        'location': location
                    })
                    
                    stack_lines.append(line)
                
                # Check for alternative stack trace format (without parentheses)
                elif re.match(r'at\s+([^\s:]+:[^\s:]+:[^\s:]+)', line):
                    location_match = re.match(r'at\s+([^\s:]+):([^\s:]+):([^\s:]+)', line)
                    if location_match:
                        file_hint = location_match.group(1)
                        try:
                            line_hint = int(location_match.group(2))
                            column_hint = int(location_match.group(3))
                        except ValueError:
                            line_hint = None
                            column_hint = None
                        
                        stack_frames.append({
                            'function': '<anonymous>',
                            'file': file_hint,
                            'line': line_hint,
                            'column': column_hint,
                            'location': f"{file_hint}:{line_hint}:{column_hint}"
                        })
                        
                        stack_lines.append(line)
                
                # If we hit an empty line or unrelated content, stop
                elif not line or not self._is_stack_trace_related(line):
                    break
                
                current_line += 1
            
            # Get the most relevant file/line (first frame in user code)
            file_hint = None
            line_hint = None
            for frame in stack_frames:
                if frame['file'] and not self._is_system_file(frame['file']):
                    file_hint = frame['file']
                    line_hint = frame['line']
                    break
            
            # If no user code found, use the first frame
            if not file_hint and stack_frames:
                first_frame = stack_frames[0]
                file_hint = first_frame['file']
                line_hint = first_frame['line']
            
            return self.create_error_entry(
                message=message,
                file_hint=file_hint,
                line_hint=line_hint,
                traceback='\n'.join(stack_lines),
                error_type=error_type,
                severity='error',
                context={
                    'stack_frames': stack_frames,
                    'start_line': start_line,
                    'end_line': current_line - 1,
                    'traceback_type': 'js_error'
                }
            )
            
        except Exception as e:
            return self.create_error_entry(
                message=f"Failed to parse JavaScript error: {str(e)}",
                severity='warning',
                context={'start_line': start_line}
            )
    
    def _parse_console_error(self, line: str, line_num: int, all_lines: List[str], 
                            source_file: Optional[Path]) -> Optional[Dict]:
        """Parse console.error or console.warn output."""
        message = self.clean_message(line)
        
        # Determine severity
        severity = 'warning' if re.search(r'\bconsole\.warn\b', line, re.IGNORECASE) else 'error'
        
        # Extract file and line information if available
        file_hint, line_hint = self.extract_file_and_line(line)
        
        return self.create_error_entry(
            message=message,
            file_hint=file_hint or (str(source_file) if source_file else None),
            line_hint=line_hint,
            error_type='ConsoleError',
            severity=severity,
            context={
                'source_line_number': line_num,
                'traceback_type': 'console_error'
            }
        )
    
    def _parse_build_error(self, line: str, line_num: int, all_lines: List[str], 
                          source_file: Optional[Path]) -> Optional[Dict]:
        """Parse build tool errors (Webpack, Vite, etc.)."""
        message = self.clean_message(line)
        
        # Determine build tool
        build_tool = None
        if re.search(r'\bwebpack\b', line, re.IGNORECASE):
            build_tool = 'Webpack'
        elif re.search(r'\bvite\b', line, re.IGNORECASE):
            build_tool = 'Vite'
        elif re.search(r'\brollup\b', line, re.IGNORECASE):
            build_tool = 'Rollup'
        elif re.search(r'\bparcel\b', line, re.IGNORECASE):
            build_tool = 'Parcel'
        
        # Extract file and line information
        file_hint, line_hint = self.extract_file_and_line(line)
        
        return self.create_error_entry(
            message=message,
            file_hint=file_hint or (str(source_file) if source_file else None),
            line_hint=line_hint,
            error_type=f'{build_tool}Error' if build_tool else 'BuildError',
            severity='error',
            context={
                'build_tool': build_tool,
                'source_line_number': line_num,
                'traceback_type': 'build_error'
            }
        )
    
    def _parse_react_error(self, line: str, line_num: int, all_lines: List[str], 
                          source_file: Optional[Path]) -> Optional[Dict]:
        """Parse React-specific errors and warnings."""
        message = self.clean_message(line)
        
        # Determine severity
        severity = 'warning' if re.search(r'\bwarning\b', line, re.IGNORECASE) else 'error'
        
        # Extract file and line information
        file_hint, line_hint = self.extract_file_and_line(line)
        
        return self.create_error_entry(
            message=message,
            file_hint=file_hint or (str(source_file) if source_file else None),
            line_hint=line_hint,
            error_type='ReactError',
            severity=severity,
            context={
                'framework': 'React',
                'source_line_number': line_num,
                'traceback_type': 'react_error'
            }
        )
    
    def _is_error_start(self, line: str) -> bool:
        """Check if line starts a JavaScript error."""
        # Look for error class names
        for error in self.js_errors:
            if re.search(rf'\b{error}\b', line):
                return True
        
        # Look for uncaught/unhandled errors
        return bool(re.search(r'\b(uncaught|unhandled)\b.*\berror\b', line, re.IGNORECASE))
    
    def _is_console_error(self, line: str) -> bool:
        """Check if line contains console error/warning."""
        return bool(re.search(r'\bconsole\.(error|warn)\b', line, re.IGNORECASE))
    
    def _is_build_error(self, line: str) -> bool:
        """Check if line contains a build error."""
        build_patterns = [
            r'\bwebpack\b.*\berror\b',
            r'\bvite\b.*\berror\b',
            r'\bbuild failed\b',
            r'\bcompilation failed\b',
            r'\bmodule not found\b',
            r'\bcannot resolve\b',
        ]
        
        for pattern in build_patterns:
            if re.search(pattern, line, re.IGNORECASE):
                return True
        return False
    
    def _is_react_error(self, line: str) -> bool:
        """Check if line contains a React error/warning."""
        react_patterns = [
            r'\breact\b.*\b(error|warning)\b',
            r'\bwarning:.*\breact\b',
            r'\binvalid hook call\b',
            r'\bhooks can only be called\b',
        ]
        
        for pattern in react_patterns:
            if re.search(pattern, line, re.IGNORECASE):
                return True
        return False
    
    def _is_stack_trace_related(self, line: str) -> bool:
        """Check if line is part of a stack trace."""
        return line.startswith('at ')
    
    def _is_system_file(self, file_path: str) -> bool:
        """Check if file belongs to system/library files."""
        system_indicators = [
            'node_modules/', '/node_modules/',
            'node:', 'internal/',
            '<anonymous>', '<eval>',
            'webpack://', 'vite://',
        ]
        
        for indicator in system_indicators:
            if indicator in file_path:
                return True
        return False
    
    def _parse_error_line_content(self, line: str) -> tuple[Optional[str], str]:
        """Parse error type and message from line."""
        # Pattern: ErrorType: message
        match = re.match(r'(\w+(?:Error|Exception)):\s*(.*)', line)
        if match:
            return match.group(1), match.group(2) or line
        
        # Pattern: Uncaught ErrorType: message
        match = re.match(r'Uncaught\s+(\w+(?:Error|Exception)):\s*(.*)', line)
        if match:
            return match.group(1), match.group(2) or line
        
        # Check for known error types without colon
        for error in self.js_errors:
            if error in line:
                return error, line
        
        return None, line