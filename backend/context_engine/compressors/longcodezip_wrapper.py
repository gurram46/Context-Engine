"""Wrapper for LongCodeZip compression functionality."""

from concurrent.futures import ThreadPoolExecutor, TimeoutError as FuturesTimeoutError
from pathlib import Path
from typing import Dict


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
                "error": "LongCodeZip unavailable, skipping compression.",
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

                try:
                    with ThreadPoolExecutor(max_workers=1) as executor:
                        future = executor.submit(
                            self._compressor.compress_code_file,
                            content,
                            query=task_query,
                            instruction="Compress code for this task.",
                            rate=self.rate
                        )
                        compressed_code = future.result(timeout=30)

                    if compressed_code is None:
                        raise RuntimeError("Compression returned no data")

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

                    original_tokens = len(content.split())
                    results.append({
                        "file": str(relative_path),
                        "original_tokens": original_tokens,
                        "compressed_tokens": token_count,
                        "compression_ratio": 1 - (token_count / original_tokens) if original_tokens else 0
                    })

                except FuturesTimeoutError:
                    results.append({
                        "file": str(file_path.relative_to(src_path)),
                        "error": "Compression timeout (30s)"
                    })
                    continue

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

    def compress(self, content: str, query: str = "Summarize for AI context", instruction: str = "Focus on essential information for AI understanding.") -> tuple:
        """Backward-compatible compress method for CLI integration.

        This method provides the interface expected by the CLI command,
        which calls compressor.compress(content, query, instruction).

        Args:
            content: Text content to compress
            query: Query to guide compression
            instruction: Instruction for compression model

        Returns:
            Tuple of (compressed_content, compressed_prompt, compression_ratio)
        """
        if not self._compressor:
            return None, "LongCodeZip unavailable, using basic compression", 0.0

        try:
            # Log compression attempt
            print(f"Compressing content ({len(content)} chars)...")

            # Use LongCodeZip to compress the content
            compressed_result = self._compressor.compress_text(
                content,
                query=query,
                instruction=instruction,
                rate=self.rate
            )

            if compressed_result and hasattr(compressed_result, 'text'):
                compressed_text = compressed_result.text
                compression_ratio = 1.0 - (len(compressed_text) / len(content)) if content else 0.0

                print(f"Compression successful: {len(content)} -> {len(compressed_text)} chars")
                print(f"Compression ratio: {compression_ratio:.2%}")

                return compressed_text, query, compression_ratio
            else:
                # Fallback to basic compression if LongCodeZip fails
                lines = content.split('\n')
                if len(lines) > 100:
                    # Take top 100 lines as basic compression
                    compressed_text = '\n'.join(lines[:100])
                    compressed_text += f"\n\n... [{len(lines) - 100} more lines compressed]"
                    compression_ratio = 1.0 - (len(compressed_text) / len(content))

                    print(f"LongCodeZip compression failed, used basic compression")
                    print(f"Compression ratio: {compression_ratio:.2%}")

                    return compressed_text, f"Basic compression: {query}", compression_ratio
                else:
                    print(f"LongCodeZip compression failed, content already small")
                    return content, f"Original content: {query}", 0.0

        except Exception as e:
            print(f"Compression error: {e}")

            # Fallback: return original content with error indication
            return content, f"Compression failed: {str(e)}", 0.0
