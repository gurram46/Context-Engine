"""Compress command using LongCodeZip for context optimization."""

from pathlib import Path
import click

from ..ui import success, info, warn, error
from ..core import count_tokens, redact_secrets, deduplicate_content
from ..core.config import get_model, set_model, get_api_key
from ..core.utils import compress_whitespace
from ..compressors.longcodezip_wrapper import LongCodeZipWrapper


@click.command()
@click.option("--rate", default=0.5, type=float, help="Compression rate (0.0 to 1.0)")
@click.option("--query", default="Summarize for AI context", help="Query to guide compression")
@click.option("--instruction", default="Focus on essential information for AI understanding.", help="Instruction for compression model")
def compress_cmd(rate, query, instruction):
    """Compress project context using LongCodeZip."""
    if not 0.0 <= rate <= 1.0:
        error("Compression rate must be between 0.0 and 1.0")
        return

    # Get AI model from config.json
    ai_model = get_model()
    info(f"🧠 Using model: {ai_model}")

    # Check if baseline files exist
    context_dir = Path.cwd() / ".context" / "baseline"
    if not context_dir.exists():
        warn("No baseline directory found. Run 'context init' first.")
        return

    baseline_files = list(context_dir.glob("*"))
    if not baseline_files:
        warn("No baseline files found. Add files with: context baseline add <files>")
        return

    info(f"📦 Files scanned: {len([f for f in baseline_files if f.is_file()])}")

    try:
        # Use user's configured model for compression
        # Falls back to default model if not specified
        compressor = LongCodeZipWrapper(model_name=ai_model, rate=rate)
    except ImportError as e:
        error(f"Failed to initialize compressor: {e}")
        return
    
    total_original_tokens = 0
    total_compressed_tokens = 0
    results = []
    
    for file_path in baseline_files:
        if file_path.name.startswith('compressed_'):
            info(f"Skipping already compressed file {file_path.name}")
            continue
        if file_path.is_file():
            try:
                # Read file content
                content = file_path.read_text(encoding="utf-8")
                original_tokens = count_tokens(content)
                total_original_tokens += original_tokens
                
                # Apply compression
                compressed_content, compressed_prompt, compression_ratio = compressor.compress(
                    content, query, instruction
                )
                
                if compressed_content is not None:
                    compressed_tokens = count_tokens(compressed_content)
                    total_compressed_tokens += compressed_tokens
                    
                    # Save compressed version
                    compressed_file = config.baseline_dir / f"compressed_{file_path.name}"
                    compressed_file.write_text(compressed_content, encoding="utf-8")
                    
                    results.append({
                        'file': file_path.name,
                        'original_tokens': original_tokens,
                        'compressed_tokens': compressed_tokens,
                        'compression_ratio': compressed_tokens / original_tokens if original_tokens > 0 else 0
                    })
                    
                    info(f"Compressed {file_path.name}: {original_tokens} -> {compressed_tokens} tokens")
                else:
                    warn(f"Failed to compress {file_path.name}")
                    
            except Exception as e:
                error(f"Error processing {file_path.name}: {e}")
    
    if results:
        # Print summary with compression percentage
        overall_compression = total_compressed_tokens / total_original_tokens if total_original_tokens > 0 else 0
        size_reduction = (1 - overall_compression) * 100

        info(f"📦 Files processed: {len(results)}")
        info(f"🪶 Compression complete: {size_reduction:.0f}% size reduction")

        success("Compression completed. Compressed files saved with 'compressed_' prefix.")
    else:
        error("No files were successfully compressed.")
