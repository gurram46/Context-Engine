"""Placeholder for Open Codebase Index adapter (future).

Do not make OCI a mandatory dependency in C0. When implemented, this adapter
will delegate to the OCI service without changing the harness runner.
"""

from pathlib import Path
from .interface import BenchmarkAdapter, IndexingMetrics, SearchResult


class OciAdapter(BenchmarkAdapter):
    name = "oci"

    def index(self, repo_path: Path) -> IndexingMetrics:
        raise NotImplementedError("OCI adapter not yet implemented in C0 — see bench/README.md")

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        raise NotImplementedError("OCI adapter not yet implemented")
