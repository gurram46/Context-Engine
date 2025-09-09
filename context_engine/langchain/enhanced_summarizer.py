"""Enhanced summarizer that integrates LangChain methods with existing file summarization.

This module extends the base FileSummarizer with LangChain processing capabilities,
providing more intelligent and structured summarization options.
"""

import json
from typing import Dict, List, Optional, Any, Union
from pathlib import Path
from datetime import datetime

from context_engine.scripts.summarizer import FileSummarizer
from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from .langchain_methods import LangChainProcessor, ProcessingResult

logger = setup_logger(__name__)

class EnhancedSummarizer(FileSummarizer):
    """Enhanced file summarizer with LangChain integration."""
    
    def __init__(self, config: ContextConfig):
        super().__init__(config)
        self.langchain_processor = LangChainProcessor(config)
        self.enhancement_modes = {
            'structured': self._structured_enhancement,
            'compressed': self._compressed_enhancement,
            'isolated': self._isolated_enhancement,
            'comprehensive': self._comprehensive_enhancement
        }
    
    def enhanced_summarize_file(self, file_path: Path, mode: str = 'structured', **kwargs) -> Dict[str, Any]:
        """Summarize file with LangChain enhancements.
        
        Args:
            file_path: Path to file to summarize
            mode: Enhancement mode ('structured', 'compressed', 'isolated', 'comprehensive')
            **kwargs: Additional parameters for specific modes
        
        Returns:
            Enhanced summary with LangChain processing results
        """
        try:
            # Get base summary first
            base_summary = self.summarize_file(file_path)
            
            if mode not in self.enhancement_modes:
                logger.warning(f"Unknown enhancement mode: {mode}. Using 'structured'.")
                mode = 'structured'
            
            # Apply LangChain enhancement
            enhancement_func = self.enhancement_modes[mode]
            enhanced_result = enhancement_func(file_path, base_summary, **kwargs)
            
            # Combine results
            return {
                'file_path': str(file_path),
                'base_summary': base_summary,
                'enhancement_mode': mode,
                'enhanced_content': enhanced_result,
                'timestamp': datetime.now().isoformat(),
                'langchain_metadata': enhanced_result.metadata if hasattr(enhanced_result, 'metadata') else {}
            }
            
        except Exception as e:
            logger.error(f"Enhanced summarization failed for {file_path}: {e}")
            return {
                'file_path': str(file_path),
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }
    
    def _structured_enhancement(self, file_path: Path, base_summary: Dict[str, Any], **kwargs) -> ProcessingResult:
        """Apply structured writing enhancement to base summary."""
        # Prepare structured data for template
        file_content = file_path.read_text(encoding='utf-8', errors='ignore')
        
        structured_data = {
            'title': f"Analysis of {file_path.name}",
            'overview': base_summary.get('summary', 'No summary available'),
            'components': self._format_components(base_summary),
            'dependencies': self._format_dependencies(base_summary),
            'issues': self._identify_issues(file_content, base_summary),
            'recommendations': self._generate_recommendations(base_summary)
        }
        
        return self.langchain_processor.write(
            structured_data,
            template_type='code_summary',
            **kwargs
        )
    
    def _compressed_enhancement(self, file_path: Path, base_summary: Dict[str, Any], **kwargs) -> ProcessingResult:
        """Apply compression to create concise summary."""
        # Combine all summary information into text
        full_summary = self._combine_summary_text(base_summary)
        
        compression_ratio = kwargs.get('compression_ratio', 0.4)
        preserve_structure = kwargs.get('preserve_structure', True)
        
        return self.langchain_processor.compress(
            full_summary,
            compression_ratio=compression_ratio,
            preserve_structure=preserve_structure
        )
    
    def _isolated_enhancement(self, file_path: Path, base_summary: Dict[str, Any], **kwargs) -> ProcessingResult:
        """Apply isolation to extract specific patterns."""
        file_content = file_path.read_text(encoding='utf-8', errors='ignore')
        
        pattern_type = kwargs.get('pattern_type', 'functions')
        
        # Determine pattern type based on file extension if not specified
        if pattern_type == 'auto':
            if file_path.suffix in ['.py']:
                pattern_type = 'functions'
            elif file_path.suffix in ['.js', '.ts']:
                pattern_type = 'functions'
            elif 'error' in file_path.name.lower() or 'log' in file_path.name.lower():
                pattern_type = 'errors'
            else:
                pattern_type = 'functions'
        
        return self.langchain_processor.isolate(
            file_content,
            pattern_type=pattern_type,
            **kwargs
        )
    
    def _comprehensive_enhancement(self, file_path: Path, base_summary: Dict[str, Any], **kwargs) -> Dict[str, ProcessingResult]:
        """Apply multiple LangChain methods for comprehensive analysis."""
        file_content = file_path.read_text(encoding='utf-8', errors='ignore')
        
        # Define processing pipeline
        pipeline_steps = [
            {
                'name': 'structured_write',
                'method': 'write',
                'params': {
                    'template_type': 'code_summary'
                }
            },
            {
                'name': 'pattern_isolation',
                'method': 'isolate',
                'params': {
                    'pattern_type': 'functions'
                }
            },
            {
                'name': 'compressed_summary',
                'method': 'compress',
                'params': {
                    'compression_ratio': 0.3,
                    'preserve_structure': True
                }
            }
        ]
        
        results = {}
        
        for step in pipeline_steps:
            try:
                if step['method'] == 'write':
                    structured_data = {
                        'title': f"Comprehensive Analysis of {file_path.name}",
                        'overview': base_summary.get('summary', 'No summary available'),
                        'components': self._format_components(base_summary),
                        'dependencies': self._format_dependencies(base_summary),
                        'issues': self._identify_issues(file_content, base_summary),
                        'recommendations': self._generate_recommendations(base_summary)
                    }
                    result = self.langchain_processor.write(structured_data, **step['params'])
                
                elif step['method'] == 'isolate':
                    result = self.langchain_processor.isolate(file_content, **step['params'])
                
                elif step['method'] == 'compress':
                    full_summary = self._combine_summary_text(base_summary)
                    result = self.langchain_processor.compress(full_summary, **step['params'])
                
                results[step['name']] = result
                
            except Exception as e:
                logger.error(f"Pipeline step {step['name']} failed: {e}")
                results[step['name']] = ProcessingResult(
                    content=f"Error in {step['name']}: {str(e)}",
                    metadata={'error': str(e)},
                    confidence=0.0,
                    processing_time=0.0,
                    method_used=step['method']
                )
        
        return results
    
    def _format_components(self, base_summary: Dict[str, Any]) -> str:
        """Format components from base summary."""
        components = []
        
        if 'classes' in base_summary:
            classes = base_summary['classes']
            if classes:
                components.append(f"Classes: {', '.join(classes)}")
        
        if 'functions' in base_summary:
            functions = base_summary['functions']
            if functions:
                components.append(f"Functions: {', '.join(functions[:5])}{'...' if len(functions) > 5 else ''}")
        
        if 'methods' in base_summary:
            methods = base_summary['methods']
            if methods:
                components.append(f"Methods: {', '.join(methods[:5])}{'...' if len(methods) > 5 else ''}")
        
        return '; '.join(components) if components else 'No major components detected'
    
    def _format_dependencies(self, base_summary: Dict[str, Any]) -> str:
        """Format dependencies from base summary."""
        deps = []
        
        if 'imports' in base_summary:
            imports = base_summary['imports']
            if imports:
                deps.extend(imports[:10])  # Limit to first 10
        
        if 'external_dependencies' in base_summary:
            external = base_summary['external_dependencies']
            if external:
                deps.extend(external[:5])  # Limit to first 5
        
        return ', '.join(deps) if deps else 'No dependencies detected'
    
    def _identify_issues(self, file_content: str, base_summary: Dict[str, Any]) -> str:
        """Identify potential issues in the code."""
        issues = []
        
        # Check for common issues
        if 'TODO' in file_content:
            todo_count = file_content.count('TODO')
            issues.append(f"{todo_count} TODO items found")
        
        if 'FIXME' in file_content:
            fixme_count = file_content.count('FIXME')
            issues.append(f"{fixme_count} FIXME items found")
        
        if 'XXX' in file_content:
            xxx_count = file_content.count('XXX')
            issues.append(f"{xxx_count} XXX markers found")
        
        # Check for potential security issues
        security_patterns = ['password', 'secret', 'api_key', 'token']
        for pattern in security_patterns:
            if pattern in file_content.lower():
                issues.append(f"Potential security concern: {pattern} found")
        
        # Check complexity indicators
        if 'complexity' in base_summary:
            complexity = base_summary['complexity']
            if isinstance(complexity, dict):
                if complexity.get('cyclomatic', 0) > 10:
                    issues.append("High cyclomatic complexity detected")
        
        return '; '.join(issues) if issues else 'No obvious issues detected'
    
    def _generate_recommendations(self, base_summary: Dict[str, Any]) -> str:
        """Generate recommendations based on analysis."""
        recommendations = []
        
        # Check function count
        if 'functions' in base_summary:
            func_count = len(base_summary['functions'])
            if func_count > 20:
                recommendations.append("Consider splitting into multiple modules")
            elif func_count == 0:
                recommendations.append("Consider adding functions for better organization")
        
        # Check class count
        if 'classes' in base_summary:
            class_count = len(base_summary['classes'])
            if class_count > 5:
                recommendations.append("Consider using composition over inheritance")
        
        # Check documentation
        if 'docstring_coverage' in base_summary:
            coverage = base_summary['docstring_coverage']
            if coverage < 0.5:
                recommendations.append("Improve documentation coverage")
        
        # Default recommendation
        if not recommendations:
            recommendations.append("Code structure appears well-organized")
        
        return '; '.join(recommendations)
    
    def _combine_summary_text(self, base_summary: Dict[str, Any]) -> str:
        """Combine all summary information into a single text."""
        text_parts = []
        
        if 'summary' in base_summary:
            text_parts.append(f"Summary: {base_summary['summary']}")
        
        if 'classes' in base_summary and base_summary['classes']:
            text_parts.append(f"Classes: {', '.join(base_summary['classes'])}")
        
        if 'functions' in base_summary and base_summary['functions']:
            text_parts.append(f"Functions: {', '.join(base_summary['functions'])}")
        
        if 'imports' in base_summary and base_summary['imports']:
            text_parts.append(f"Imports: {', '.join(base_summary['imports'])}")
        
        if 'complexity' in base_summary:
            text_parts.append(f"Complexity: {base_summary['complexity']}")
        
        if 'risks' in base_summary and base_summary['risks']:
            text_parts.append(f"Risks: {', '.join(base_summary['risks'])}")
        
        return '\n\n'.join(text_parts)
    
    def batch_enhanced_summarize(self, file_paths: List[Path], mode: str = 'structured', **kwargs) -> Dict[str, Any]:
        """Perform enhanced summarization on multiple files."""
        results = {}
        
        for file_path in file_paths:
            try:
                result = self.enhanced_summarize_file(file_path, mode, **kwargs)
                results[str(file_path)] = result
            except Exception as e:
                logger.error(f"Batch summarization failed for {file_path}: {e}")
                results[str(file_path)] = {
                    'error': str(e),
                    'timestamp': datetime.now().isoformat()
                }
        
        return {
            'batch_results': results,
            'total_files': len(file_paths),
            'successful': len([r for r in results.values() if 'error' not in r]),
            'failed': len([r for r in results.values() if 'error' in r]),
            'mode': mode,
            'timestamp': datetime.now().isoformat()
        }
    
    def smart_select_files(self, directory: Path, criteria: Dict[str, Any], limit: int = 10) -> List[Path]:
        """Use LangChain Select method to intelligently choose files for summarization."""
        try:
            # Get all files in directory
            all_files = []
            for pattern in ['**/*.py', '**/*.js', '**/*.ts', '**/*.java', '**/*.cpp', '**/*.h']:
                all_files.extend(directory.glob(pattern))
            
            # Use LangChain select method
            select_result = self.langchain_processor.select(
                all_files,
                criteria=criteria,
                limit=limit
            )
            
            # Parse the JSON result
            if select_result.content:
                try:
                    selected_paths = json.loads(select_result.content)
                    return [Path(p) for p in selected_paths if Path(p).exists()]
                except json.JSONDecodeError:
                    logger.warning("Failed to parse select results, falling back to all files")
                    return all_files[:limit]
            
            return all_files[:limit]
            
        except Exception as e:
            logger.error(f"Smart file selection failed: {e}")
            return []
    
    def generate_project_overview(self, project_path: Path, max_files: int = 20) -> Dict[str, Any]:
        """Generate a comprehensive project overview using LangChain methods."""
        try:
            # Smart select important files
            criteria = {
                'keywords': ['main', 'index', 'app', 'server', 'client'],
                'file_type': '.py',
                'recency': 30  # Files modified in last 30 days
            }
            
            selected_files = self.smart_select_files(project_path, criteria, max_files)
            
            if not selected_files:
                return {
                    'error': 'No suitable files found for overview generation',
                    'timestamp': datetime.now().isoformat()
                }
            
            # Batch summarize selected files
            batch_results = self.batch_enhanced_summarize(selected_files, mode='compressed')
            
            # Generate overall project summary
            project_summary_data = {
                'title': f"Project Overview: {project_path.name}",
                'overview': f"Analysis of {len(selected_files)} key files in the project",
                'components': f"Total files analyzed: {len(selected_files)}",
                'dependencies': "See individual file summaries",
                'issues': "Aggregated from file analyses",
                'recommendations': "Based on project structure and file analysis"
            }
            
            project_write_result = self.langchain_processor.write(
                project_summary_data,
                template_type='code_summary'
            )
            
            return {
                'project_path': str(project_path),
                'overview_summary': project_write_result.content,
                'file_summaries': batch_results,
                'metadata': {
                    'files_analyzed': len(selected_files),
                    'selection_criteria': criteria,
                    'processing_time': project_write_result.processing_time
                },
                'timestamp': datetime.now().isoformat()
            }
            
        except Exception as e:
            logger.error(f"Project overview generation failed: {e}")
            return {
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }