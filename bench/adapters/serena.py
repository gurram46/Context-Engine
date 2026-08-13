"""Placeholder for Serena adapter (future)."""

from pathlib import Path
from .interface import BenchmarkAdapter, IndexingMetrics, SearchResult


class SerenaAdapter(BenchmarkAdapter):
    name = "serena"

    def index(self, repo_path: Path) -> IndexingMetrics:
        raise NotImplementedError("Serena adapter not yet implemented in C0")

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        raise NotImplementedError("Serena adapter not yet implemented")
