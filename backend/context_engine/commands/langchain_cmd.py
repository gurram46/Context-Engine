"""LangChain command implementations for Context Engine CLI.

Provides commands for enhanced summarization and processing using LangChain methods.
"""

import json
import sys
from pathlib import Path
from typing import List, Dict, Any

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.langchain.enhanced_summarizer import EnhancedSummarizer
from context_engine.langchain.langchain_methods import LangChainProcessor

logger = setup_logger(__name__)

def enhanced_summarize_command(args) -> int:
    """Enhanced summarization command using LangChain methods."""
    try:
        config = ContextConfig.load()
        summarizer = EnhancedSummarizer(config)
        
        # Get file paths
        file_paths = []
        for path_str in args.files:
            path = Path(path_str)
            if path.is_file():
                file_paths.append(path)
            elif path.is_dir():
                # Add supported file types from directory
                for pattern in ['**/*.py', '**/*.js', '**/*.ts', '**/*.java', '**/*.cpp', '**/*.h']:
                    file_paths.extend(path.glob(pattern))
            else:
                logger.warning(f"Path not found: {path_str}")
        
        if not file_paths:
            logger.error("No valid files found to summarize")
            return 1
        
        # Process files
        if len(file_paths) == 1:
            # Single file processing
            result = summarizer.enhanced_summarize_file(
                file_paths[0],
                mode=args.mode,
                compression_ratio=args.compression_ratio,
                pattern_type=args.pattern_type
            )
            
            if args.output:
                output_path = Path(args.output)
                output_path.write_text(json.dumps(result, indent=2))
                print(f"Enhanced summary saved to: {output_path}")
            else:
                if args.format == 'json':
                    print(json.dumps(result, indent=2))
                else:
                    # Pretty print the enhanced content
                    if 'enhanced_content' in result:
                        enhanced = result['enhanced_content']
                        if hasattr(enhanced, 'content'):
                            print(enhanced.content)
                        elif isinstance(enhanced, dict):
                            for key, value in enhanced.items():
                                print(f"\n=== {key.upper()} ===")
                                if hasattr(value, 'content'):
                                    print(value.content)
                                else:
                                    print(str(value))
                        else:
                            print(str(enhanced))
        else:
            # Batch processing
            batch_result = summarizer.batch_enhanced_summarize(
                file_paths,
                mode=args.mode,
                compression_ratio=args.compression_ratio,
                pattern_type=args.pattern_type
            )
            
            if args.output:
                output_path = Path(args.output)
                output_path.write_text(json.dumps(batch_result, indent=2))
                print(f"Batch enhanced summaries saved to: {output_path}")
            else:
                if args.format == 'json':
                    print(json.dumps(batch_result, indent=2))
                else:
                    print(f"\n=== BATCH SUMMARY RESULTS ===")
                    print(f"Total files: {batch_result['total_files']}")
                    print(f"Successful: {batch_result['successful']}")
                    print(f"Failed: {batch_result['failed']}")
                    print(f"Mode: {batch_result['mode']}")
                    
                    for file_path, result in batch_result['batch_results'].items():
                        print(f"\n--- {file_path} ---")
                        if 'error' in result:
                            print(f"ERROR: {result['error']}")
                        elif 'enhanced_content' in result:
                            enhanced = result['enhanced_content']
                            if hasattr(enhanced, 'content'):
                                # Truncate for batch display
                                content = enhanced.content[:200] + "..." if len(enhanced.content) > 200 else enhanced.content
                                print(content)
        
        return 0
        
    except Exception as e:
        logger.error(f"Enhanced summarization failed: {e}")
        return 1

def project_overview_command(args) -> int:
    """Generate project overview using LangChain methods."""
    try:
        config = ContextConfig.load()
        summarizer = EnhancedSummarizer(config)
        
        project_path = Path(args.path) if args.path else Path.cwd()
        
        if not project_path.is_dir():
            logger.error(f"Project path is not a directory: {project_path}")
            return 1
        
        print(f"Generating project overview for: {project_path}")
        
        overview = summarizer.generate_project_overview(
            project_path,
            max_files=args.max_files
        )
        
        if 'error' in overview:
            logger.error(f"Project overview generation failed: {overview['error']}")
            return 1
        
        if args.output:
            output_path = Path(args.output)
            output_path.write_text(json.dumps(overview, indent=2))
            print(f"Project overview saved to: {output_path}")
        else:
            if args.format == 'json':
                print(json.dumps(overview, indent=2))
            else:
                print("\n=== PROJECT OVERVIEW ===")
                print(overview['overview_summary'])
                
                print(f"\n=== METADATA ===")
                metadata = overview['metadata']
                print(f"Files analyzed: {metadata['files_analyzed']}")
                print(f"Processing time: {metadata['processing_time']:.2f}s")
                
                if args.verbose:
                    print("\n=== FILE SUMMARIES ===")
                    batch_results = overview['file_summaries']['batch_results']
                    for file_path, result in batch_results.items():
                        print(f"\n--- {Path(file_path).name} ---")
                        if 'error' in result:
                            print(f"ERROR: {result['error']}")
                        elif 'enhanced_content' in result:
                            enhanced = result['enhanced_content']
                            if hasattr(enhanced, 'content'):
                                content = enhanced.content[:300] + "..." if len(enhanced.content) > 300 else enhanced.content
                                print(content)
        
        return 0
        
    except Exception as e:
        logger.error(f"Project overview generation failed: {e}")
        return 1

def langchain_process_command(args) -> int:
    """Process content using specific LangChain methods."""
    try:
        config = ContextConfig.load()
        processor = LangChainProcessor(config)
        
        # Read input content
        if args.input == '-':
            content = sys.stdin.read()
        else:
            input_path = Path(args.input)
            if not input_path.exists():
                logger.error(f"Input file not found: {input_path}")
                return 1
            content = input_path.read_text(encoding='utf-8', errors='ignore')
        
        # Process based on method
        if args.method == 'write':
            if args.template_type:
                result = processor.write(content, template_type=args.template_type)
            else:
                # Try to parse as structured data
                try:
                    structured_data = json.loads(content)
                    result = processor.write(structured_data)
                except json.JSONDecodeError:
                    result = processor.write(content)
        
        elif args.method == 'compress':
            result = processor.compress(
                content,
                compression_ratio=args.compression_ratio,
                preserve_structure=args.preserve_structure
            )
        
        elif args.method == 'isolate':
            result = processor.isolate(
                content,
                pattern_type=args.pattern_type
            )
        
        elif args.method == 'select':
            # For select, we need a list of items and criteria
            try:
                data = json.loads(content)
                if 'items' in data and 'criteria' in data:
                    result = processor.select(
                        data['items'],
                        data['criteria'],
                        limit=args.limit
                    )
                else:
                    logger.error("Select method requires JSON with 'items' and 'criteria' fields")
                    return 1
            except json.JSONDecodeError:
                logger.error("Select method requires JSON input")
                return 1
        
        else:
            logger.error(f"Unknown method: {args.method}")
            return 1
        
        # Output result
        if args.output:
            output_path = Path(args.output)
            if args.format == 'json':
                output_data = {
                    'content': result.content,
                    'metadata': result.metadata,
                    'confidence': result.confidence,
                    'processing_time': result.processing_time,
                    'method_used': result.method_used
                }
                output_path.write_text(json.dumps(output_data, indent=2))
            else:
                output_path.write_text(result.content)
            print(f"Result saved to: {output_path}")
        else:
            if args.format == 'json':
                output_data = {
                    'content': result.content,
                    'metadata': result.metadata,
                    'confidence': result.confidence,
                    'processing_time': result.processing_time,
                    'method_used': result.method_used
                }
                print(json.dumps(output_data, indent=2))
            else:
                print(result.content)
                
                if args.verbose:
                    print(f"\n--- Metadata ---")
                    print(f"Method: {result.method_used}")
                    print(f"Confidence: {result.confidence:.2f}")
                    print(f"Processing time: {result.processing_time:.2f}s")
                    print(f"Metadata: {json.dumps(result.metadata, indent=2)}")
        
        return 0
        
    except Exception as e:
        logger.error(f"LangChain processing failed: {e}")
        return 1

def smart_select_command(args) -> int:
    """Smart file selection using LangChain Select method."""
    try:
        config = ContextConfig.load()
        summarizer = EnhancedSummarizer(config)
        
        directory = Path(args.directory) if args.directory else Path.cwd()
        
        if not directory.is_dir():
            logger.error(f"Directory not found: {directory}")
            return 1
        
        # Build criteria from arguments
        criteria = {}
        
        if args.keywords:
            criteria['keywords'] = args.keywords
        
        if args.file_type:
            criteria['file_type'] = args.file_type
        
        if args.recency:
            criteria['recency'] = args.recency
        
        if args.size_range:
            min_size, max_size = map(int, args.size_range.split(','))
            criteria['size_range'] = (min_size, max_size)
        
        if not criteria:
            logger.error("No selection criteria provided")
            return 1
        
        print(f"Selecting files from: {directory}")
        print(f"Criteria: {json.dumps(criteria, indent=2)}")
        
        selected_files = summarizer.smart_select_files(
            directory,
            criteria,
            limit=args.limit
        )
        
        if not selected_files:
            print("No files matched the selection criteria")
            return 0
        
        if args.output:
            output_path = Path(args.output)
            file_list = [str(f) for f in selected_files]
            output_path.write_text(json.dumps(file_list, indent=2))
            print(f"Selected files saved to: {output_path}")
        else:
            print(f"\nSelected {len(selected_files)} files:")
            for file_path in selected_files:
                print(f"  {file_path}")
        
        return 0
        
    except Exception as e:
        logger.error(f"Smart file selection failed: {e}")
        return 1