"""Placeholder for Codebase-Memory-MCP adapter (future)."""

from pathlib import Path
from .interface import BenchmarkAdapter, IndexingMetrics, SearchResult


class CodebaseMemoryAdapter(BenchmarkAdapter):
    name = "codebase_memory"

    def index(self, repo_path: Path) -> IndexingMetrics:
        raise NotImplementedError("Codebase-Memory-MCP adapter not yet implemented in C0")

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        raise NotImplementedError("Codebase-Memory-MCP adapter not yet implemented")
