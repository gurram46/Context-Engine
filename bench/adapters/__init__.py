"""Benchmark adapters package.

Exports the adapter interface and concrete implementations.
Future adapters (OCI, Codebase-Memory-MCP, Serena) should subclass
BenchmarkAdapter without changing the harness runner.
"""

from .interface import BenchmarkAdapter, IndexingMetrics, SearchResult, SearchHit

__all__ = ["BenchmarkAdapter", "IndexingMetrics", "SearchResult", "SearchHit"]
