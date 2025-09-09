"""Reindex command for Context Engine."""

from pathlib import Path
from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.embedder import FileIndexer

logger = setup_logger(__name__)

def reindex_command(all_files: bool = False, incremental: bool = False) -> int:
    """Reindex project files."""
    try:
        project_root = Path.cwd()
        
        # Check if Context Engine is initialized
        config_path = project_root / "context_engine" / "config" / "context.yml"
        if not config_path.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Load configuration
        config = ContextConfig.load_from_file(config_path)
        
        # Create indexer
        indexer = FileIndexer(config, project_root)
        
        # Determine reindex mode
        if all_files:
            logger.info("Starting full reindex...")
            success_count = indexer.reindex_all()
        elif incremental:
            logger.info("Starting incremental reindex...")
            success_count = indexer.reindex_incremental()
        else:
            # Default to incremental
            logger.info("Starting incremental reindex (default)...")
            success_count = indexer.reindex_incremental()
        
        if success_count > 0:
            logger.info(f"Successfully indexed {success_count} files")
        else:
            logger.info("No files were indexed")
        
        return 0
        
    except Exception as e:
        logger.error(f"Reindex failed: {e}")
        return 1