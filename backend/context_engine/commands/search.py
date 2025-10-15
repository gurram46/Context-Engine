"""Search command for Context Engine."""

from pathlib import Path
from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger
from context_engine.scripts.embeddings_store import EmbeddingsStore

logger = setup_logger(__name__)

def search_command(query: str, k: int = 8) -> int:
    """Search indexed content using vector similarity."""
    try:
        project_root = Path.cwd()
        
        # Check if Context Engine is initialized
        config_path = project_root / "context_engine" / "config" / "context.yml"
        if not config_path.exists():
            logger.error("Context Engine not initialized. Run 'context-engine init' first.")
            return 1
        
        # Load configuration
        config = ContextConfig.load_from_file(config_path)
        
        # Create embeddings store
        try:
            store = EmbeddingsStore(config, project_root)
        except Exception as e:
            logger.error(f"Failed to initialize embeddings store: {e}")
            logger.info("Try running 'context-engine reindex --all' to build the index first.")
            return 1
        
        # Check if index exists
        stats = store.get_stats()
        if stats['total_embeddings'] == 0:
            logger.warning("No embeddings found. Run 'context-engine reindex --all' to build the index.")
            return 1
        
        # Perform search
        logger.info(f"Searching for: '{query}'")
        results = store.similarity_search(query, k)
        
        if not results:
            print("No results found.")
            return 0
        
        # Display results
        print(f"\n=== Search Results for '{query}' ===")
        print(f"Found {len(results)} results:\n")
        
        for i, result in enumerate(results, 1):
            print(f"{i}. {result['file_path']}")
            print(f"   Chunk {result['chunk_index']} (chars {result['start_char']}-{result['end_char']})")
            print(f"   Similarity: {result['similarity_score']:.4f}")
            
            # Show text preview
            text = result['text']
            if len(text) > 300:
                # Try to find a good break point
                break_point = text.find('\n', 250)
                if break_point == -1 or break_point > 350:
                    break_point = 300
                preview = text[:break_point] + '...' if break_point < len(text) else text
            else:
                preview = text
            
            # Highlight query terms (simple approach)
            query_words = query.lower().split()
            highlighted_preview = preview
            for word in query_words:
                if len(word) > 2:  # Only highlight words longer than 2 chars
                    # Simple case-insensitive highlighting
                    import re
                    pattern = re.compile(re.escape(word), re.IGNORECASE)
                    highlighted_preview = pattern.sub(f"**{word.upper()}**", highlighted_preview)
            
            print(f"   Preview: {highlighted_preview}")
            print()
        
        # Show search statistics
        print(f"Search completed. Searched through {stats['total_embeddings']} indexed chunks.")
        
        return 0
        
    except Exception as e:
        logger.error(f"Search failed: {e}")
        return 1