"""Codebase-Memory-MCP adapter — C0 stub."""

from pathlib import Path
from .interface import BenchmarkAdapter, IndexingMetrics, SearchResult


class CodebaseMemoryAdapter(BenchmarkAdapter):
    name = "codebase_memory"

    def index(self, repo_path: Path) -> IndexingMetrics:
        return IndexingMetrics(
            initial_wall_ms=None,
            unavailable=["codebase_memory_not_installed", "symbols", "bm25_docs", "vector_count", "index_disk_bytes", "cpu_ms", "peak_rss_mb", "no_change_wall_ms", "one_file_wall_ms"],
        )

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        return SearchResult(
            query=query,
            hits=[],
            candidate_count=0,
            evidence_count=0,
            files_returned=0,
            retrievers_used=["codebase_memory:not_installed"],
            elapsed_ms=0,
            raw={"error": "codebase-memory-mcp not installed — see competitors.json"},
        )
