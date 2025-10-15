"""Wrapper for LongCodeZip compression functionality."""

import os
import signal
from pathlib import Path
from typing import Dict, List, Optional
import time


class LongCodeZipWrapper:
    """Wrapper class for the LongCodeZip compression tool."""
    
    def __init__(self, model_name: str = "Qwen/Qwen2.5-Coder-7B-Instruct", rate: float = 0.5):
        """Initialize the wrapper with model and compression rate.
        
        Args:
            model_name: Name of the model to use for compression
            rate: Compression rate (0.0-1.0)
        """
        self.model_name = model_name
        self.rate = rate
        self._compressor = None
        self._initialize_compressor()
    
    def _initialize_compressor(self) -> None:
        """Initialize the LongCodeZip compressor."""
        try:
            # Try to import CodeCompressor from the LongCodeZip repo
            from longcodezip import CodeCompressor
            self._compressor = CodeCompressor(model_name=self.model_name)
        except ImportError:
            # LongCodeZip not available
            self._compressor = None
    
    def compress_src(self, src_dir: str, task_query: str) -> Dict:
        """Compress source code in the given directory.
        
        Args:
            src_dir: Source directory path
            task_query: Task description for compression
            
        Returns:
            Dictionary with compression stats
        """
        if not self._compressor:
            return {
                "error": "⚠ LongCodeZip unavailable, skipping compression.",
                "ratio": 0,
                "token_count": 0,
                "task": task_query
            }
        
        src_path = Path(src_dir)
        if not src_path.exists():
            return {
                "error": f"Source directory {src_dir} does not exist",
                "ratio": 0,
                "token_count": 0,
                "task": task_query
            }
        
        # Create output directory
        output_dir = Path(".context/compressed_src")
        output_dir.mkdir(parents=True, exist_ok=True)
        
        # Collect all source files
        source_files = []
        for ext in [".py", ".js", ".ts"]:
            source_files.extend(src_path.rglob(f"*{ext}"))
        
        if not source_files:
            return {
                "error": f"No source files found in {src_dir}",
                "ratio": 0,
                "token_count": 0,
                "task": task_query
            }
        
        # Process each file with timeout
        results = []
        total_tokens = 0
        
        for file_path in source_files:
            try:
                # Read file content
                content = file_path.read_text(encoding="utf-8")
                
                # Set up timeout handler
                def timeout_handler(signum, frame):
                    raise TimeoutError("Compression timeout")
                
                signal.signal(signal.SIGALRM, timeout_handler)
                signal.alarm(30)  # 30 second timeout
                
                try:
                    # Compress the code
                    compressed_code = self._compressor.compress_code_file(
                        content, 
                        query=task_query, 
                        instruction="Compress code for this task.",
                        rate=self.rate
                    )
                    
                    # Count tokens (rough estimate)
                    token_count = len(compressed_code.split())
                    total_tokens += token_count
                    
                    # Save compressed code
                    relative_path = file_path.relative_to(src_path)
                    output_file = output_dir / f"{relative_path}.md"
                    output_file.parent.mkdir(parents=True, exist_ok=True)
                    
                    # Create markdown content
                    markdown_content = f"# {relative_path}\n\n```\n{compressed_code}\n```"
                    output_file.write_text(markdown_content, encoding="utf-8")
                    
                    results.append({
                        "file": str(relative_path),
                        "original_tokens": len(content.split()),
                        "compressed_tokens": token_count,
                        "compression_ratio": 1 - (token_count / len(content.split()))
                    })
                    
                finally:
                    signal.alarm(0)  # Cancel timeout
                    
            except TimeoutError:
                results.append({
                    "file": str(file_path.relative_to(src_path)),
                    "error": "Compression timeout (30s)"
                })
            except Exception as e:
                results.append({
                    "file": str(file_path.relative_to(src_path)),
                    "error": str(e)
                })
        
        # Calculate overall compression ratio
        successful_results = [r for r in results if "error" not in r]
        if successful_results:
            total_original = sum(r["original_tokens"] for r in successful_results)
            avg_ratio = sum(r["compression_ratio"] for r in successful_results) / len(successful_results)
        else:
            avg_ratio = 0
        
        return {
            "results": results,
            "total_files": len(source_files),
            "successful": len(successful_results),
            "ratio": avg_ratio,
            "token_count": total_tokens,
            "task": task_query
        }