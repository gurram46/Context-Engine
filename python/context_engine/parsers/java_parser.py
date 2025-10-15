"""Java-specific log parser for Context Engine."""

import re
from pathlib import Path
from typing import Dict, List, Optional

from .base_parser import BaseParser

class JavaParser(BaseParser):
    """Parser specialized for Java error logs and stack traces."""
    
    def __init__(self):
        super().__init__('java')
        self.supported_extensions = ['.java', '.class', '.jar']
        
        # Java-specific error patterns
        self.error_patterns = [
            # Java exceptions
            r'(?i)\b\w*Exception\b.*',
            r'(?i)\b\w*Error\b.*',
            r'(?i)\bCaused by:.*',
            r'(?i)\bat [\w.$]+\([^)]*\)',
            
            # Compilation errors
            r'(?i)\berror:\s*.*',
            r'(?i)\bwarning:\s*.*',
            r'(?i)\bcompilation failed\b.*',
            
            # Spring Boot specific
            r'(?i)\bspring\b.*\berror\b.*',
            r'(?i)\bapplication failed to start\b.*',
            
            # Maven/Gradle build errors
            r'(?i)\bbuild failed\b.*',
            r'(?i)\bmaven\b.*\berror\b.*',
            r'(?i)\bgradle\b.*\berror\b.*',
        ]
        
        # Common Java exceptions
        self.java_exceptions = [
            'NullPointerException', 'ArrayIndexOutOfBoundsException',
            'ClassNotFoundException', 'NoSuchMethodException',
            'IllegalArgumentException', 'IllegalStateException',
            'NumberFormatException', 'ClassCastException',
            'ConcurrentModificationException', 'UnsupportedOperationException',
            'RuntimeException', 'Exception', 'Error',
            'OutOfMemoryError', 'StackOverflowError',
            'NoClassDefFoundError', 'ExceptionInInitializerError',
            'IOException', 'FileNotFoundException',
            'SQLException', 'ConnectException', 'SocketTimeoutException',
        ]
    
    def can_parse(self, content: str, file_extension: Optional[str] = None) -> bool:
        """Check if content appears to be Java-related."""
        # Check file extension
        if file_extension and file_extension.lower() in self.supported_extensions:
            return True
        
        # Check for Java-specific patterns
        java_indicators = [
            r'\bat [\w.$]+\([^)]*\)',  # Java stack trace format
            r'\b(' + '|'.join(self.java_exceptions) + r')\b',
            r'\bCaused by:',
            r'\bjava\.[\w.]+',
            r'\bspring\b.*\berror\b',
            r'\bmaven\b.*\berror\b',
            r'\bgradle\b.*\berror\b',
        ]
        
        for pattern in java_indicators:
            if re.search(pattern, content, re.IGNORECASE):
                return True
        
        return False
    
    def parse_log_content(self, content: str, source_file: Optional[Path] = None) -> List[Dict]:
        """Parse Java log content and extract structured error information."""
        errors = []
        lines = content.split('\n')
        
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            
            # Look for exception start
            if self._is_exception_start(line):
                error = self._parse_exception_stack(i, lines)
                if error:
                    errors.append(error)
                    # Skip to end of stack trace
                    i = error['context'].get('end_line', i + 1)
                else:
                    i += 1
            
            # Look for compilation errors
            elif self._is_compilation_error(line):
                error = self._parse_compilation_error(line, i, lines, source_file)
                if error:
                    errors.append(error)
                i += 1
            
            # Look for build errors
            elif self._is_build_error(line):
                error = self._parse_build_error(line, i, lines, source_file)
                if error:
                    errors.append(error)
                i += 1
            
            else:
                i += 1
        
        return errors
    
    def _parse_exception_stack(self, start_line: int, lines: List[str]) -> Optional[Dict]:
        """Parse a Java exception with stack trace."""
        try:
            stack_lines = []
            current_line = start_line
            
            # Parse the main exception line
            exception_line = lines[current_line].strip()
            stack_lines.append(exception_line)
            
            # Extract exception type and message
            error_type, message = self._parse_exception_line_content(exception_line)
            
            current_line += 1
            stack_frames = []
            
            # Parse stack trace lines
            while current_line < len(lines):
                line = lines[current_line].strip()
                
                # Check for stack trace line (at ...)
                at_match = re.match(r'at ([\w.$]+)\(([^)]+)\)', line)
                if at_match:
                    method_name = at_match.group(1)
                    location = at_match.group(2)
                    
                    # Parse location (FileName.java:123 or Native Method)
                    file_hint = None
                    line_hint = None
                    if ':' in location and not location == 'Native Method':
                        parts = location.split(':') 
                        if len(parts) == 2:
                            file_hint = parts[0]
                            try:
                                line_hint = int(parts[1])
                            except ValueError:
                                pass
                    
                    stack_frames.append({
                        'method': method_name,
                        'file': file_hint,
                        'line': line_hint,
                        'location': location
                    })
                    
                    stack_lines.append(line)
                
                # Check for "Caused by:" line
                elif line.startswith('Caused by:'):
                    stack_lines.append(line)
                    # This starts a new exception chain, but we'll include it in the same error
                    caused_by_type, caused_by_message = self._parse_exception_line_content(line[10:].strip())
                    if caused_by_message:
                        message += f" | Caused by: {caused_by_message}"
                
                # Check for "... X more" line
                elif re.match(r'\s*\.\.\. \d+ more', line):
                    stack_lines.append(line)
                
                # If we hit an empty line or unrelated content, stop
                elif not line or not self._is_stack_trace_related(line):
                    break
                
                current_line += 1
            
            # Get the most relevant file/line (usually the first frame in our code)
            file_hint = None
            line_hint = None
            for frame in stack_frames:
                if frame['file'] and not self._is_system_class(frame['method']):
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
                    'traceback_type': 'java_exception'
                }
            )
            
        except Exception as e:
            return self.create_error_entry(
                message=f"Failed to parse Java exception: {str(e)}",
                severity='warning',
                context={'start_line': start_line}
            )
    
    def _parse_compilation_error(self, line: str, line_num: int, all_lines: List[str], 
                                source_file: Optional[Path]) -> Optional[Dict]:
        """Parse Java compilation error."""
        message = self.clean_message(line)
        
        # Extract file and line information
        file_hint, line_hint = self.extract_file_and_line(line)
        
        # Look for additional context in surrounding lines
        context_lines = []
        for i in range(max(0, line_num - 2), min(len(all_lines), line_num + 3)):
            if i != line_num:
                context_line = all_lines[i].strip()
                if context_line:
                    context_lines.append(context_line)
        
        # Determine error type
        error_type = 'CompilationError'
        if re.search(r'\bwarning\b', line, re.IGNORECASE):
            severity = 'warning'
            error_type = 'CompilationWarning'
        else:
            severity = 'error'
        
        return self.create_error_entry(
            message=message,
            file_hint=file_hint or (str(source_file) if source_file else None),
            line_hint=line_hint,
            error_type=error_type,
            severity=severity,
            context={
                'source_line_number': line_num,
                'context_lines': context_lines,
                'traceback_type': 'compilation_error'
            }
        )
    
    def _parse_build_error(self, line: str, line_num: int, all_lines: List[str], 
                          source_file: Optional[Path]) -> Optional[Dict]:
        """Parse Maven/Gradle build error."""
        message = self.clean_message(line)
        
        # Determine build tool
        build_tool = None
        if re.search(r'\bmaven\b', line, re.IGNORECASE):
            build_tool = 'Maven'
        elif re.search(r'\bgradle\b', line, re.IGNORECASE):
            build_tool = 'Gradle'
        
        return self.create_error_entry(
            message=message,
            file_hint=str(source_file) if source_file else None,
            error_type=f'{build_tool}BuildError' if build_tool else 'BuildError',
            severity='error',
            context={
                'build_tool': build_tool,
                'source_line_number': line_num,
                'traceback_type': 'build_error'
            }
        )
    
    def _is_exception_start(self, line: str) -> bool:
        """Check if line starts a Java exception."""
        # Look for exception class names
        for exception in self.java_exceptions:
            if re.search(rf'\b{exception}\b', line):
                return True
        
        # Look for general exception pattern
        return bool(re.match(r'\w+(\.\w+)*Exception:', line) or 
                   re.match(r'\w+(\.\w+)*Error:', line))
    
    def _is_compilation_error(self, line: str) -> bool:
        """Check if line contains a compilation error."""
        compilation_patterns = [
            r'\berror:\s*',
            r'\bwarning:\s*',
            r'\bcompilation failed\b',
            r'\bcannot find symbol\b',
            r'\bpackage .* does not exist\b',
        ]
        
        for pattern in compilation_patterns:
            if re.search(pattern, line, re.IGNORECASE):
                return True
        return False
    
    def _is_build_error(self, line: str) -> bool:
        """Check if line contains a build error."""
        build_patterns = [
            r'\bbuild failed\b',
            r'\bmaven\b.*\berror\b',
            r'\bgradle\b.*\berror\b',
            r'\bfailed to execute goal\b',
        ]
        
        for pattern in build_patterns:
            if re.search(pattern, line, re.IGNORECASE):
                return True
        return False
    
    def _is_stack_trace_related(self, line: str) -> bool:
        """Check if line is part of a stack trace."""
        return (line.startswith('at ') or 
                line.startswith('Caused by:') or
                re.match(r'\s*\.\.\. \d+ more', line))
    
    def _is_system_class(self, method_name: str) -> bool:
        """Check if method belongs to system/library classes."""
        system_prefixes = [
            'java.', 'javax.', 'sun.', 'com.sun.',
            'org.springframework.', 'org.apache.',
            'org.eclipse.', 'org.junit.'
        ]
        
        for prefix in system_prefixes:
            if method_name.startswith(prefix):
                return True
        return False
    
    def _parse_exception_line_content(self, line: str) -> tuple[Optional[str], str]:
        """Parse exception type and message from line."""
        # Pattern: com.example.ExceptionType: message
        match = re.match(r'([\w.$]+(?:Exception|Error)):\s*(.*)', line)
        if match:
            full_type = match.group(1)
            # Get just the class name, not the full package
            error_type = full_type.split('.')[-1] if '.' in full_type else full_type
            message = match.group(2) or line
            return error_type, message
        
        # Check for known exception types without colon
        for exception in self.java_exceptions:
            if exception in line:
                return exception, line
        
        return None, line