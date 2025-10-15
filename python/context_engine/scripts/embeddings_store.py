"""Embeddings and vector store for Context Engine."""

import json
import pickle
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Any
import numpy as np

try:
    import faiss
except ImportError:
    faiss = None

try:
    from sentence_transformers import SentenceTransformer
except ImportError:
    SentenceTransformer = None

from context_engine.core.config import ContextConfig
from context_engine.core.logger import setup_logger

logger = setup_logger(__name__)

class EmbeddingsStore:
    """Handles embeddings generation and vector similarity search."""
    
    def __init__(self, config: ContextConfig, project_root: Optional[Path] = None):
        self.config = config
        self.project_root = project_root or Path.cwd()
        self.embeddings_db_dir = self.project_root / "context_engine" / "embeddings_db"
        self.vector_store_path = self.embeddings_db_dir / "vector_store.faiss"
        self.metadata_path = self.embeddings_db_dir / "metadata.json"
        self.model_cache_path = self.embeddings_db_dir / "model_cache"
        
        # Ensure directories exist
        self.embeddings_db_dir.mkdir(parents=True, exist_ok=True)
        self.model_cache_path.mkdir(parents=True, exist_ok=True)
        
        self.model = None
        self.index = None
        self.metadata = []
        
        # Initialize model and index
        self._initialize_model()
        self._load_or_create_index()
    
    def _initialize_model(self) -> None:
        """Initialize the embedding model."""
        if self.config.embedding.provider == "local":
            if SentenceTransformer is None:
                raise ImportError("sentence-transformers not installed. Run: pip install sentence-transformers")
            
            try:
                # Try to load from cache first
                cached_model_path = self.model_cache_path / self.config.embedding.model
                if cached_model_path.exists():
                    logger.info(f"Loading cached model from {cached_model_path}")
                    self.model = SentenceTransformer(str(cached_model_path))
                else:
                    logger.info(f"Downloading model {self.config.embedding.model}")
                    self.model = SentenceTransformer(self.config.embedding.model)
                    # Cache the model
                    self.model.save(str(cached_model_path))
                    logger.info(f"Model cached to {cached_model_path}")
                
            except Exception as e:
                logger.error(f"Failed to load embedding model: {e}")
                raise
        
        else:
            # For API-based providers, we'll implement later
            raise NotImplementedError(f"Provider {self.config.embedding.provider} not yet implemented")
    
    def _load_or_create_index(self) -> None:
        """Load existing FAISS index or create a new one."""
        if faiss is None:
            logger.warning("FAISS not installed. Vector search will be limited. Run: pip install faiss-cpu")
            return
        
        if self.vector_store_path.exists() and self.metadata_path.exists():
            try:
                # Load existing index
                self.index = faiss.read_index(str(self.vector_store_path))
                
                with open(self.metadata_path, 'r', encoding='utf-8') as f:
                    self.metadata = json.load(f)
                
                logger.info(f"Loaded vector store with {len(self.metadata)} embeddings")
            except Exception as e:
                logger.warning(f"Failed to load existing vector store: {e}. Creating new one.")
                self._create_new_index()
        else:
            self._create_new_index()
    
    def _create_new_index(self) -> None:
        """Create a new FAISS index."""
        if faiss is None:
            return
        
        # Get embedding dimension from model
        if self.model is None:
            return
        
        # Test embedding to get dimension
        test_embedding = self.model.encode(["test"])
        dimension = test_embedding.shape[1]
        
        # Create FAISS index (using IndexFlatIP for cosine similarity)
        self.index = faiss.IndexFlatIP(dimension)
        self.metadata = []
        
        logger.info(f"Created new vector store with dimension {dimension}")
    
    def _save_index(self) -> None:
        """Save the FAISS index and metadata."""
        if faiss is None or self.index is None:
            return
        
        try:
            faiss.write_index(self.index, str(self.vector_store_path))
            
            with open(self.metadata_path, 'w', encoding='utf-8') as f:
                json.dump(self.metadata, f, indent=2, ensure_ascii=False)
            
            logger.debug(f"Saved vector store with {len(self.metadata)} embeddings")
        except Exception as e:
            logger.error(f"Failed to save vector store: {e}")
    
    def generate_embeddings(self, texts: List[str]) -> np.ndarray:
        """Generate embeddings for a list of texts."""
        if self.model is None:
            raise RuntimeError("Embedding model not initialized")
        
        try:
            embeddings = self.model.encode(texts, convert_to_numpy=True, normalize_embeddings=True)
            return embeddings
        except Exception as e:
            logger.error(f"Failed to generate embeddings: {e}")
            raise
    
    def add_embeddings(self, texts: List[str], metadata_list: List[Dict[str, Any]]) -> None:
        """Add embeddings to the vector store."""
        if len(texts) != len(metadata_list):
            raise ValueError("Number of texts must match number of metadata entries")
        
        if not texts:
            return
        
        # Generate embeddings
        embeddings = self.generate_embeddings(texts)
        
        # Add to FAISS index
        if self.index is not None:
            self.index.add(embeddings.astype(np.float32))
        
        # Add metadata
        self.metadata.extend(metadata_list)
        
        # Save index
        self._save_index()
        
        logger.info(f"Added {len(texts)} embeddings to vector store")
    
    def similarity_search(self, query: str, k: int = 5) -> List[Dict[str, Any]]:
        """Search for similar chunks using vector similarity."""
        if self.model is None:
            logger.error("Embedding model not initialized")
            return []
        
        if self.index is None or len(self.metadata) == 0:
            logger.warning("Vector store is empty")
            return []
        
        try:
            # Generate query embedding
            query_embedding = self.generate_embeddings([query])
            
            # Search in FAISS index
            scores, indices = self.index.search(query_embedding.astype(np.float32), min(k, len(self.metadata)))
            
            # Prepare results
            results = []
            for i, (score, idx) in enumerate(zip(scores[0], indices[0])):
                if idx >= 0 and idx < len(self.metadata):  # Valid index
                    result = self.metadata[idx].copy()
                    result['similarity_score'] = float(score)
                    result['rank'] = i + 1
                    results.append(result)
            
            return results
            
        except Exception as e:
            logger.error(f"Similarity search failed: {e}")
            return []
    
    def rebuild_index_from_chunks(self) -> None:
        """Rebuild the vector index from all chunk files."""
        logger.info("Rebuilding vector index from chunk files...")
        
        # Clear existing index
        self._create_new_index()
        
        # Find all chunk files
        chunk_files = list(self.embeddings_db_dir.glob("*.json"))
        chunk_files = [f for f in chunk_files if f.name not in ['metadata.json']]
        
        all_texts = []
        all_metadata = []
        
        for chunk_file in chunk_files:
            try:
                with open(chunk_file, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                
                chunks = data.get('chunks', [])
                for chunk in chunks:
                    all_texts.append(chunk['text'])
                    
                    # Prepare metadata
                    metadata = {
                        'file_path': chunk['file_path'],
                        'chunk_index': chunk['chunk_index'],
                        'start_char': chunk['start_char'],
                        'end_char': chunk['end_char'],
                        'file_hash': chunk['file_hash'],
                        'text': chunk['text']  # Store text in metadata for retrieval
                    }
                    all_metadata.append(metadata)
                    
            except Exception as e:
                logger.warning(f"Failed to process chunk file {chunk_file}: {e}")
        
        if all_texts:
            # Add all embeddings in batches to avoid memory issues
            batch_size = 100
            for i in range(0, len(all_texts), batch_size):
                batch_texts = all_texts[i:i + batch_size]
                batch_metadata = all_metadata[i:i + batch_size]
                
                logger.info(f"Processing batch {i // batch_size + 1}/{(len(all_texts) + batch_size - 1) // batch_size}")
                self.add_embeddings(batch_texts, batch_metadata)
            
            logger.info(f"Rebuilt vector index with {len(all_texts)} embeddings")
        else:
            logger.warning("No chunks found to rebuild index")
    
    def get_stats(self) -> Dict[str, Any]:
        """Get statistics about the vector store."""
        stats = {
            'total_embeddings': len(self.metadata),
            'model_name': self.config.embedding.model,
            'provider': self.config.embedding.provider,
            'index_exists': self.index is not None,
            'faiss_available': faiss is not None,
            'model_available': self.model is not None
        }
        
        if self.index is not None:
            stats['index_dimension'] = self.index.d
            stats['index_size'] = self.index.ntotal
        
        return stats

def main():
    """Main entry point for embeddings store script."""
    import argparse
    
    parser = argparse.ArgumentParser(description='Context Engine Embeddings Store')
    parser.add_argument('--rebuild', action='store_true', help='Rebuild vector index from chunks')
    parser.add_argument('--search', help='Search query')
    parser.add_argument('--k', type=int, default=5, help='Number of results to return')
    parser.add_argument('--stats', action='store_true', help='Show vector store statistics')
    
    args = parser.parse_args()
    
    try:
        config = ContextConfig.load_or_create()
        store = EmbeddingsStore(config)
        
        if args.rebuild:
            store.rebuild_index_from_chunks()
        
        elif args.search:
            results = store.similarity_search(args.search, args.k)
            print(f"\nSearch results for: '{args.search}'\n")
            
            for result in results:
                print(f"File: {result['file_path']}")
                print(f"Chunk: {result['chunk_index']} (chars {result['start_char']}-{result['end_char']})")
                print(f"Score: {result['similarity_score']:.4f}")
                print(f"Text: {result['text'][:200]}{'...' if len(result['text']) > 200 else ''}")
                print("-" * 80)
        
        elif args.stats:
            stats = store.get_stats()
            print("\nVector Store Statistics:")
            for key, value in stats.items():
                print(f"  {key}: {value}")
        
        else:
            print("Please specify --rebuild, --search, or --stats")
            return 1
        
        return 0
        
    except Exception as e:
        logger.error(f"Embeddings store failed: {e}")
        return 1

if __name__ == '__main__':
    import sys
    sys.exit(main())