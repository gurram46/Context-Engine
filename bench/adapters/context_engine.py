"""Thin real-contextd subprocess adapter for Context Bench v1.

This adapter does NOT reimplement retrieval, ranking, packing, or planning.
It is a subprocess wrapper over the actual Rust release binary `contextd`:

  cargo build --release -p contextd
  contextd --root <repo> --json --max-results 5 search "<query>"

It parses real JSON fields (evidence, stats, context) and maps them into
SearchResult. No fake scores, no whitespace token estimates when real tokens
exist.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import List

from .interface import BenchmarkAdapter, IndexingMetrics, SearchHit, SearchResult

REPO_ROOT = Path(__file__).resolve().parents[2]


def _resolve_bin() -> Path:
    env = os.environ.get("CONTEXTD_BIN")
    if env:
        p = Path(env)
        if p.exists():
            return p
    # platform-specific
    if sys.platform == "win32":
        cand = REPO_ROOT / "target" / "release" / "contextd.exe"
        if cand.exists():
            return cand
        # also check without exe? fallback
        cand2 = REPO_ROOT / "target" / "release" / "contextd"
        if cand2.exists():
            return cand2
        return cand
    else:
        cand = REPO_ROOT / "target" / "release" / "contextd"
        return cand


def _ensure_built() -> Path:
    bin_path = _resolve_bin()
    if bin_path.exists():
        return bin_path
    # try to build once (outside timing)
    print(f"[context_engine] binary not found at {bin_path}, building cargo --release -p contextd ...", flush=True)
    # cargo build --release -p contextd
    proc = subprocess.run(
        ["cargo", "build", "--release", "-p", "contextd"],
        cwd=REPO_ROOT,
        capture_output=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"cargo build --release -p contextd failed with {proc.returncode}")
    # re-resolve
    bin_path = _resolve_bin()
    if not bin_path.exists():
        raise RuntimeError(f"contextd binary still not found at {bin_path} after build")
    return bin_path


def _run_json(args: List[str], cwd: Path | None = None, timeout: int = 120) -> dict:
    # args[0] is bin, rest are args
    proc = subprocess.run(
        args,
        cwd=cwd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
        timeout=timeout,
    )
    stdout = (proc.stdout or "").strip()
    stderr = (proc.stderr or "").strip()
    if proc.returncode != 0 and not stdout:
        raise RuntimeError(f"contextd failed rc={proc.returncode} stderr={stderr[:500]}")
    # stdout should be JSON
    # contextd prints pretty JSON; strip any leading non-json?
    # Find first { or [
    if not stdout:
        raise RuntimeError(f"empty stdout rc={proc.returncode} stderr={stderr[:500]}")
    # Some tracing may leak to stdout? but cli.rs writes tracing to stderr, so stdout is pure json.
    try:
        return json.loads(stdout)
    except json.JSONDecodeError as e:
        # try to find json substring
        s = stdout.find("{")
        if s != -1:
            try:
                return json.loads(stdout[s:])
            except Exception:
                pass
        raise RuntimeError(f"failed to parse contextd JSON: {e} stdout={stdout[:1000]} stderr={stderr[:500]}")


class ContextEngineAdapter(BenchmarkAdapter):
    """Thin real-contextd subprocess adapter."""

    name = "context_engine"

    def __init__(self) -> None:
        self._bin = _ensure_built()

    def index(self, repo_path: Path) -> IndexingMetrics:
        """Capture real contextd status for this repo.
        Indexing is done lazily via reconcile on first search; here we just
        query status to record filesIndexed/symbols/bm25/vector/semantic.
        Wall times for cold/warm/one-file are measured separately via searches;
        here we record initial_wall as status call time and mark others unavailable
        with honest labelling (cold first-search wall time measured in separate phase).
        """
        t0 = time.perf_counter()
        try:
            data = _run_json(
                [str(self._bin), "--root", str(repo_path), "--json", "status"],
                timeout=30,
            )
        except Exception as e:
            print(f"[context_engine] status failed for {repo_path}: {e}", file=sys.stderr)
            wall = int((time.perf_counter() - t0) * 1000)
            return IndexingMetrics(
                initial_wall_ms=wall,
                files_indexed=None,
                symbols=None,
                bm25_docs=None,
                vector_count=None,
                index_disk_bytes=None,
                unavailable=[
                    "symbols",
                    "bm25_docs",
                    "vector_count",
                    "index_disk_bytes",
                    "cpu_ms",
                    "peak_rss_mb",
                    "no_change_wall_ms",
                    "one_file_wall_ms",
                    "status_error",
                ],
            )
        wall = int((time.perf_counter() - t0) * 1000)
        # status fields: camelCase
        files_indexed = data.get("filesIndexed") if "filesIndexed" in data else data.get("files_indexed")
        symbols = data.get("symbols")
        bm25_docs = data.get("bm25Documents") if "bm25Documents" in data else data.get("bm25_documents")
        vector_count = data.get("vectorCount") if "vectorCount" in data else data.get("vector_count")
        # index disk not exposed via status; leave None
        # record what is available directly via data
        unavailable = []
        if files_indexed is None:
            unavailable.append("files_indexed")
        if symbols is None:
            unavailable.append("symbols")
        if bm25_docs is None:
            unavailable.append("bm25_docs")
        if vector_count is None:
            unavailable.append("vector_count")
        unavailable.extend(["index_disk_bytes", "cpu_ms", "peak_rss_mb", "no_change_wall_ms", "one_file_wall_ms"])
        # store extra status for debugging
        self._last_status = data
        return IndexingMetrics(
            initial_wall_ms=wall,
            files_indexed=int(files_indexed) if files_indexed is not None else None,
            symbols=int(symbols) if symbols is not None else None,
            bm25_docs=int(bm25_docs) if bm25_docs is not None else None,
            vector_count=int(vector_count) if vector_count is not None else None,
            index_disk_bytes=None,
            unavailable=unavailable,
        )

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        t0 = time.perf_counter()
        try:
            data = _run_json(
                [
                    str(self._bin),
                    "--root",
                    str(repo_path),
                    "--json",
                    "--max-results",
                    str(top_n),
                    "search",
                    query,
                ],
                timeout=300,
            )
        except Exception as e:
            # surface error but return empty hits so runner records failure honestly
            print(f"[context_engine] search failed query={query!r} repo={repo_path}: {e}", file=sys.stderr)
            elapsed = int((time.perf_counter() - t0) * 1000)
            return SearchResult(
                query=query,
                hits=[],
                candidate_count=0,
                evidence_count=0,
                files_returned=0,
                candidate_tokens=None,
                packed_tokens=None,
                retrievers_used=[f"error:{e}"],
                elapsed_ms=elapsed,
            )

        # data keys: query, type, evidence[], context, stats{ candidate_count, evidence_count, files_returned, packed_tokens, retrievers, elapsed_ms, exact_ms, structural_ms, bm25_ms, semantic_ms, rank_ms, pack_ms }
        evidence = data.get("evidence") or []
        stats = data.get("stats") or {}
        context_text = data.get("context") or ""

        # Map evidence to SearchHit preserving rank order
        hits: List[SearchHit] = []
        for ev in evidence:
            # file is primary
            f = ev.get("file") or ev.get("path") or ""
            f = f.replace("\\", "/")
            # lines field is "start-end" string or ""
            line = None
            lines_str = ev.get("lines") or ""
            if lines_str and "-" in lines_str:
                try:
                    line = int(lines_str.split("-")[0].split(":")[-1])
                except Exception:
                    line = None
            elif lines_str:
                try:
                    line = int(lines_str.strip().split(":")[-1])
                except Exception:
                    line = None
            # fallback: ev may have start_line if we change cli to expose it
            if line is None and ev.get("start_line") is not None:
                try:
                    line = int(ev["start_line"])
                except Exception:
                    line = None
            # text snippet: not directly in this custom json (only for other commands uses ev.text), but search custom json omitted text; we can leave None or use symbol
            text = ev.get("text")
            # score mapping: use finalScore if available else score
            score = ev.get("finalScore")
            if score is None:
                score = ev.get("score")
            # provenance
            prov = ev.get("source") or ev.get("provenance") or ""
            # attach relation/symbol for debugging but file is ranking key
            symbol = ev.get("symbol")
            # combine provenance with relation for visibility
            rel = ev.get("relation")
            if rel and prov:
                prov = f"{prov}:{rel}"
            elif rel:
                prov = str(rel)
            hits.append(
                SearchHit(
                    file=f,
                    score=float(score) if score is not None else None,
                    line=line,
                    text=text,
                    symbol=symbol,
                    provenance=prov,
                )
            )
            if len(hits) >= top_n:
                break

        # Stats mapping: handle both camel and snake
        def g(k_snake, k_camel, default=None):
            if k_snake in stats:
                return stats[k_snake]
            if k_camel in stats:
                return stats[k_camel]
            return default

        candidate_count = g("candidate_count", "candidateCount", 0) or 0
        evidence_count = g("evidence_count", "evidenceCount", len(hits)) or len(hits)
        files_returned = g("files_returned", "filesReturned", len(set(h.file for h in hits))) or len(set(h.file for h in hits))
        packed_tokens = g("packed_tokens", "packedTokens", None)
        # retrievers: list of strings
        retrievers = stats.get("retrievers") or stats.get("retrievers_used") or []
        wall_ms = int((time.perf_counter() - t0) * 1000)
        elapsed_ms = g("elapsed_ms", "elapsedMs", None)
        # internal is stats elapsed if present, else wall
        internal_ms = int(elapsed_ms) if elapsed_ms is not None else wall_ms
        # but keep wall vs internal distinct
        exact_ms = g("exact_ms", "exactMs", None)
        structural_ms = g("structural_ms", "structuralMs", None)
        bm25_ms = g("bm25_ms", "bm25Ms", None)
        semantic_ms = g("semantic_ms", "semanticMs", None)
        semantic_embed_ms = g("semantic_embed_ms", "semanticEmbedMs", None)
        semantic_search_ms = g("semantic_search_ms", "semanticSearchMs", None)
        rank_ms = g("rank_ms", "rankMs", None)
        authority_ms = g("authority_ms", "authorityMs", None)
        fusion_ms = g("fusion_ms", "fusionMs", None)
        pack_ms = g("pack_ms", "packMs", None)
        discovery_ms = g("discovery_ms", "discoveryMs", None)
        reconcile_ms = g("reconcile_ms", "reconcileMs", None)
        total_ms = g("total_ms", "totalMs", None)
        generation = g("generation", "generation", None)
        dirty = g("dirty_file_count", "dirtyFileCount", None)
        vector_scanned = g("vector_count_scanned", "vectorCountScanned", None)
        cache_hit = g("cache_hit", "cacheHit", None)

        # candidate_tokens: not exposed by production; leave None honestly
        candidate_tokens = None
        # If contextd later exposes it, parse here:
        if "candidate_tokens" in stats:
            candidate_tokens = stats["candidate_tokens"]

        # retrievers: append wall vs internal for visibility
        retrievers = list(retrievers) if isinstance(retrievers, list) else []
        retrievers.append(f"wall:{wall_ms}")
        retrievers.append(f"internal:{internal_ms}")
        if discovery_ms is not None:
            retrievers.append(f"discovery:{discovery_ms}")
        if reconcile_ms is not None:
            retrievers.append(f"reconcile:{reconcile_ms}")

        return SearchResult(
            query=query,
            hits=hits,
            candidate_count=int(candidate_count) if candidate_count is not None else 0,
            evidence_count=int(evidence_count) if evidence_count is not None else len(hits),
            files_returned=int(files_returned) if files_returned is not None else len(set(h.file for h in hits)),
            candidate_tokens=candidate_tokens,
            packed_tokens=int(packed_tokens) if packed_tokens is not None else None,
            retrievers_used=retrievers,
            elapsed_ms=int(internal_ms),
            wall_ms=wall_ms,
            internal_ms=int(internal_ms),
            exact_ms=int(exact_ms) if exact_ms is not None else None,
            structural_ms=int(structural_ms) if structural_ms is not None else None,
            bm25_ms=int(bm25_ms) if bm25_ms is not None else None,
            semantic_ms=int(semantic_ms) if semantic_ms is not None else None,
            rank_ms=int(rank_ms) if rank_ms is not None else (int(authority_ms) if authority_ms is not None else None),
            pack_ms=int(pack_ms) if pack_ms is not None else None,
            total_ms=int(total_ms) if total_ms is not None else None,
            discovery_ms=int(discovery_ms) if discovery_ms is not None else None,
            reconcile_ms=int(reconcile_ms) if reconcile_ms is not None else None,
            semantic_embed_ms=int(semantic_embed_ms) if semantic_embed_ms is not None else None,
            semantic_search_ms=int(semantic_search_ms) if semantic_search_ms is not None else None,
            fusion_ms=int(fusion_ms) if fusion_ms is not None else None,
            authority_ms=int(authority_ms) if authority_ms is not None else (int(rank_ms) if rank_ms is not None else None),
            generation=int(generation) if generation is not None else None,
            dirty_file_count=int(dirty) if dirty is not None else None,
            vector_count_scanned=int(vector_scanned) if vector_scanned is not None else None,
            cache_hit=bool(cache_hit) if cache_hit is not None else None,
            raw=data,
        )
