"""Task-based source code compression for Context Engine."""

import os
import zipfile
from pathlib import Path
from typing import Optional

from context_engine.core.task_manager import get_task


def compress_for_task(task: Optional[str] = None) -> None:
    """Compress src/ directory based on current task."""
    # Get the current task if not provided
    if task is None:
        task = get_task()
    
    if not task:
        print("No task set for compression. Run 'context start-session --task \"your task\"' first.")
        return
    
    # Path to src directory in current project root
    project_root = Path.cwd()
    src_path = project_root / "src"
    context_dir = project_root / ".context"
    compressed_src_dir = context_dir / "compressed_src"
    
    # Create compressed src directory if it doesn't exist
    compressed_src_dir.mkdir(parents=True, exist_ok=True)
    
    # If src directory exists, compress it
    if src_path.exists() and src_path.is_dir():
        # Create a zip file with the task name
        output_file = compressed_src_dir / f"src_compressed_{abs(hash(task)) % 10000:04d}.zip"
        
        # Create zip archive of src directory
        with zipfile.ZipFile(output_file, 'w', zipfile.ZIP_DEFLATED) as zipf:
            for root, dirs, files in os.walk(src_path):
                for file in files:
                    file_path = Path(root) / file
                    # Add file to zip with relative path from src
                    zipf.write(file_path, file_path.relative_to(project_root))
        
        print(f"Compressed {src_path} to {output_file}")
    else:
        print("src/ directory not found. Skipping compression.")