"""OCI adapter — C0 stub with honest unavailable reporting.

Open Codebase Index is documented as npm package open-codebase-index (Rust+tree-sitter, SQLite+usearch+BM25).
In C0 it is NOT installed as a hard dependency; this stub allows harness to run without it
while still recording competitor metadata in competitors.json. When OCI is installed via npm,
replace this stub with real delegation (index/query) without changing harness runner.
"""

from pathlib import Path
from .interface import BenchmarkAdapter, IndexingMetrics, SearchResult

class OciAdapter(BenchmarkAdapter):
    name = "oci"

    def index(self, repo_path: Path) -> IndexingMetrics:
        return IndexingMetrics(
            initial_wall_ms=None,
            unavailable=["oci_not_installed", "symbols", "bm25_docs", "vector_count", "index_disk_bytes", "cpu_ms", "peak_rss_mb", "no_change_wall_ms", "one_file_wall_ms"],
        )

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        return SearchResult(
            query=query,
            hits=[],
            candidate_count=0,
            evidence_count=0,
            files_returned=0,
            retrievers_used=["oci:not_installed"],
            elapsed_ms=0,
            raw={"error": "OCI not installed — see competitors.json for install instructions"},
        )
