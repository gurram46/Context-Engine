"""Adapter interface for Context Bench v1.

Design goals:
- Lean, stdlib-only types for harness portability.
- Supports sync and async implementations (run via asyncio.to_thread if needed).
- Future adapters (OCI, Codebase-Memory-MCP, Serena) implement the same contract
  without runner restructuring.

Do NOT add benchmark-aware logic here.
"""

from __future__ import annotations

import abc
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional


@dataclass
class SearchHit:
    file: str  # relative POSIX from repo root
    score: Optional[float] = None
    line: Optional[int] = None
    text: Optional[str] = None
    symbol: Optional[str] = None
    provenance: Optional[str] = None  # e.g. "rg:exact", "context_engine:exact"


@dataclass
class SearchResult:
    query: str
    hits: List[SearchHit] = field(default_factory=list)
    # Core retrieval metrics (per query)
    candidate_count: int = 0
    evidence_count: int = 0
    files_returned: int = 0
    candidate_tokens: Optional[int] = None
    packed_tokens: Optional[int] = None
    retrievers_used: List[str] = field(default_factory=list)
    elapsed_ms: int = 0
    # Wall vs internal (hot must report both)
    wall_ms: Optional[int] = None
    internal_ms: Optional[int] = None
    # Per-stage timings when exposed (Context Engine only)
    exact_ms: Optional[int] = None
    structural_ms: Optional[int] = None
    bm25_ms: Optional[int] = None
    semantic_ms: Optional[int] = None
    rank_ms: Optional[int] = None
    pack_ms: Optional[int] = None
    # E1 precise stage telemetry (Option/null when not measurable)
    total_ms: Optional[int] = None
    discovery_ms: Optional[int] = None
    reconcile_ms: Optional[int] = None
    semantic_embed_ms: Optional[int] = None
    semantic_search_ms: Optional[int] = None
    fusion_ms: Optional[int] = None
    authority_ms: Optional[int] = None
    generation: Optional[int] = None
    dirty_file_count: Optional[int] = None
    vector_count_scanned: Optional[int] = None
    cache_hit: Optional[bool] = None
    # Raw for debugging
    raw: Optional[Dict] = None


@dataclass
class IndexingMetrics:
    # wall time in ms
    initial_wall_ms: Optional[int] = None
    no_change_wall_ms: Optional[int] = None
    one_file_wall_ms: Optional[int] = None
    # resource
    cpu_ms: Optional[int] = None
    peak_rss_mb: Optional[int] = None
    index_disk_bytes: Optional[int] = None
    # counts
    files_indexed: Optional[int] = None
    symbols: Optional[int] = None
    bm25_docs: Optional[int] = None
    vector_count: Optional[int] = None
    # delta
    affected_structural_updates: Optional[int] = None
    index_size_delta_bytes: Optional[int] = None
    # markers
    unavailable: List[str] = field(default_factory=list)


class BenchmarkAdapter(abc.ABC):
    """Contract every bench adapter must satisfy.

    Implementations must be:
    - deterministic given repo_path + query + top_n
    - isolated per repo (no cross-repo state)
    - side-effect free for search (index is separate)
    """

    @property
    @abc.abstractmethod
    def name(self) -> str:
        """Stable adapter key, e.g. 'context_engine', 'rg_baseline'."""
        ...

    @abc.abstractmethod
    def index(self, repo_path: Path) -> IndexingMetrics:
        """Build / refresh index for repo_path. Called once per repo run.

        For adapters with no index (rg baseline) return metrics with unavailable
        markers and files_indexed via simple walk.
        """
        ...

    @abc.abstractmethod
    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        """Execute query against repo_path, return top_n hits in ranked order."""
        ...

    # Optional hook for future adapters that need per-repo setup
    def warmup(self, repo_path: Path) -> None:
        pass

    def close(self) -> None:
        pass
