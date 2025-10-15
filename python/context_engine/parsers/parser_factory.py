"""Parser factory for Context Engine log parsers."""

from pathlib import Path
from typing import Dict, List, Optional, Type

from .base_parser import BaseParser
from .generic_parser import GenericParser
from .python_parser import PythonParser
from .java_parser import JavaParser
from .js_parser import JSParser

class ParserFactory:
    """Factory class for creating appropriate log parsers."""
    
    def __init__(self):
        # Register available parsers in priority order
        # More specific parsers should come first
        self.parsers: List[Type[BaseParser]] = [
            PythonParser,
            JavaParser,
            JSParser,
            GenericParser,  # Always last as fallback
        ]
        
        # Cache parser instances
        self._parser_cache: Dict[str, BaseParser] = {}
    
    def get_parser(self, content: str, file_path: Optional[Path] = None) -> BaseParser:
        """Get the most appropriate parser for the given content.
        
        Args:
            content: Log content to parse
            file_path: Optional file path for extension hint
            
        Returns:
            Most appropriate parser instance
        """
        # Get file extension if available
        file_extension = None
        if file_path:
            file_extension = file_path.suffix.lower()
        
        # Try each parser in priority order
        for parser_class in self.parsers:
            parser_type = parser_class.__name__.lower().replace('parser', '')
            
            # Get cached instance or create new one
            if parser_type not in self._parser_cache:
                self._parser_cache[parser_type] = parser_class()
            
            parser = self._parser_cache[parser_type]
            
            # Check if this parser can handle the content
            if parser.can_parse(content, file_extension):
                return parser
        
        # Fallback to generic parser (should never reach here)
        if 'generic' not in self._parser_cache:
            self._parser_cache['generic'] = GenericParser()
        return self._parser_cache['generic']
    
    def get_parser_by_type(self, parser_type: str) -> Optional[BaseParser]:
        """Get a specific parser by type.
        
        Args:
            parser_type: Type of parser ('python', 'java', 'javascript', 'generic')
            
        Returns:
            Parser instance or None if not found
        """
        parser_type = parser_type.lower()
        
        # Map parser types to classes
        parser_map = {
            'python': PythonParser,
            'java': JavaParser,
            'javascript': JSParser,
            'js': JSParser,
            'generic': GenericParser,
        }
        
        if parser_type not in parser_map:
            return None
        
        # Get cached instance or create new one
        if parser_type not in self._parser_cache:
            self._parser_cache[parser_type] = parser_map[parser_type]()
        
        return self._parser_cache[parser_type]
    
    def parse_log_file(self, file_path: Path, parser_type: Optional[str] = None) -> List[Dict]:
        """Parse a log file using the appropriate parser.
        
        Args:
            file_path: Path to the log file
            parser_type: Optional specific parser type to use
            
        Returns:
            List of parsed error dictionaries
        """
        try:
            # Read file content
            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                content = f.read()
            
            # Get appropriate parser
            if parser_type:
                parser = self.get_parser_by_type(parser_type)
                if not parser:
                    raise ValueError(f"Unknown parser type: {parser_type}")
            else:
                parser = self.get_parser(content, file_path)
            
            # Parse content
            return parser.parse_log_content(content, file_path)
            
        except Exception as e:
            # Return error as parsed result
            return [{
                'message': f"Failed to parse log file {file_path}: {str(e)}",
                'file_hint': str(file_path),
                'error_type': 'ParseError',
                'severity': 'error',
                'parser_type': 'factory',
                'context': {'parse_error': True}
            }]
    
    def parse_log_content(self, content: str, 
                         source_file: Optional[Path] = None,
                         parser_type: Optional[str] = None) -> List[Dict]:
        """Parse log content using the appropriate parser.
        
        Args:
            content: Log content to parse
            source_file: Optional source file for context
            parser_type: Optional specific parser type to use
            
        Returns:
            List of parsed error dictionaries
        """
        try:
            # Get appropriate parser
            if parser_type:
                parser = self.get_parser_by_type(parser_type)
                if not parser:
                    raise ValueError(f"Unknown parser type: {parser_type}")
            else:
                parser = self.get_parser(content, source_file)
            
            # Parse content
            return parser.parse_log_content(content, source_file)
            
        except Exception as e:
            # Return error as parsed result
            return [{
                'message': f"Failed to parse log content: {str(e)}",
                'file_hint': str(source_file) if source_file else None,
                'error_type': 'ParseError',
                'severity': 'error',
                'parser_type': 'factory',
                'context': {'parse_error': True}
            }]
    
    def get_available_parsers(self) -> List[str]:
        """Get list of available parser types.
        
        Returns:
            List of parser type names
        """
        return ['python', 'java', 'javascript', 'generic']
    
    def get_parser_info(self, parser_type: str) -> Optional[Dict]:
        """Get information about a specific parser.
        
        Args:
            parser_type: Type of parser
            
        Returns:
            Dictionary with parser information or None
        """
        parser = self.get_parser_by_type(parser_type)
        if not parser:
            return None
        
        return {
            'type': parser.parser_type,
            'supported_extensions': parser.supported_extensions,
            'class_name': parser.__class__.__name__
        }
    
    def clear_cache(self):
        """Clear the parser instance cache."""
        self._parser_cache.clear()

# Global factory instance
parser_factory = ParserFactory()