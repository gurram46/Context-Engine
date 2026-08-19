"""Persistent MCP adapter for hot benchmark (steady-state).

Keeps one contextd MCP process per repo alive across N queries.
Measures both wall clock (adapter) and internal engine elapsed (from stats)
so MCP transport overhead is distinguishable.

Protocol is minimal JSON-RPC 2.0 over stdio (line-delimited) matching rmcp's stdio:
  -> initialize
  <- result
  -> notifications/initialized
  -> tools/call for context_search
  <- result

No benchmark-specific production logic; uses existing MCP service.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import threading
import queue
from pathlib import Path
from typing import Dict, Optional

from .interface import BenchmarkAdapter, IndexingMetrics, SearchHit, SearchResult

REPO_ROOT = Path(__file__).resolve().parents[2]


def _resolve_bin() -> Path:
    env = os.environ.get("CONTEXTD_BIN")
    if env:
        p = Path(env)
        if p.exists():
            return p
    if sys.platform == "win32":
        cand = REPO_ROOT / "target" / "release" / "contextd.exe"
        if cand.exists():
            return cand
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
    print(f"[hot] building contextd --release ...", flush=True)
    proc = subprocess.run(["cargo", "build", "--release", "-p", "contextd"], cwd=REPO_ROOT)
    if proc.returncode != 0:
        raise RuntimeError("cargo build failed")
    bin_path = _resolve_bin()
    if not bin_path.exists():
        raise RuntimeError(f"bin not found {bin_path}")
    return bin_path


class _McpClient:
    """Minimal JSON-RPC 2.0 client over stdio, line-delimited.

    pty-safe: runs reader thread, queue for responses keyed by id.
    """

    def __init__(self, repo_path: Path):
        self.repo_path = repo_path
        self.bin = _ensure_built()
        t_start = time.perf_counter()
        self.proc = subprocess.Popen(
            [str(self.bin), "--root", str(repo_path), "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="ignore",
            bufsize=1,  # line buffered
        )
        self._next_id = 1
        self._pending: Dict[int, queue.Queue] = {}
        self._pending_lock = threading.Lock()
        self._reader_thread = threading.Thread(target=self._reader, daemon=True)
        self._reader_thread.start()
        self._initialize()
        self.startup_ms = int((time.perf_counter() - t_start) * 1000)
        self.os_pid = self.proc.pid
        self.contextd_pid = self.os_pid

    def _send(self, obj: dict):
        line = json.dumps(obj, ensure_ascii=False)
        # rmcp expects one JSON per line
        assert self.proc.stdin is not None
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()

    def _reader(self):
        assert self.proc.stdout is not None
        for line in iter(self.proc.stdout.readline, ""):
            if not line:
                break
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except Exception:
                # ignore non-json stderr? but stdout should be pure json
                continue
            # response has id
            if "id" in msg:
                mid = msg["id"]
                with self._pending_lock:
                    q = self._pending.get(mid)
                if q is not None:
                    q.put(msg)
                # also handle notifications without id? ignore
            else:
                # notification from server (e.g., progress) - ignore
                pass

    def _request(self, method: str, params=None, timeout: int = 30) -> dict:
        mid = self._next_id
        self._next_id += 1
        q: queue.Queue = queue.Queue()
        with self._pending_lock:
            self._pending[mid] = q
        req = {"jsonrpc": "2.0", "id": mid, "method": method}
        if params is not None:
            req["params"] = params
        self._send(req)
        try:
            resp = q.get(timeout=timeout)
        except queue.Empty:
            raise TimeoutError(f"mcp request {method} timeout")
        finally:
            with self._pending_lock:
                self._pending.pop(mid, None)
        if "error" in resp:
            raise RuntimeError(f"mcp error {resp['error']}")
        return resp.get("result", resp)

    def _notify(self, method: str, params=None):
        obj = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            obj["params"] = params
        self._send(obj)

    def _initialize(self):
        # rmcp expects protocolVersion 2024-11-05
        try:
            self._request(
                "initialize",
                {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "bench-hot", "version": "0.1.0"},
                },
                timeout=60,
            )
        except Exception as e:
            # try fallback with 2025 version?
            print(f"[hot] initialize failed: {e}", file=sys.stderr)
            raise
        # send initialized notification (no id)
        self._notify("notifications/initialized", {})
        # small pause to let server settle
        time.sleep(0.15)

    def call_tool(self, name: str, arguments: dict, timeout: int = 60) -> dict:
        params = {"name": name, "arguments": arguments}
        result = self._request("tools/call", params, timeout=timeout)
        # result is CallToolResult: {content: [{type:"text", text:"{...}"}], isError?}
        content = result.get("content") or []
        text = ""
        for c in content:
            if isinstance(c, dict):
                if c.get("type") == "text" and "text" in c:
                    text += c["text"] + "\n"
                elif "text" in c:
                    text += c["text"] + "\n"
        text = text.strip()
        if not text:
            # some servers return content as string?
            return result
        try:
            return json.loads(text)
        except Exception:
            # if text is not json, return raw
            return {"raw_text": text, "result": result}

    def status(self, timeout: int = 15) -> dict:
        return self.call_tool("context_status", {}, timeout=timeout)

    def search(self, query: str, max_results: int = 5, budget_tokens: int = 10000, timeout: int = 180) -> dict:
        return self.call_tool(
            "context_search", {"query": query, "maxResults": max_results, "budgetTokens": budget_tokens}, timeout=timeout
        )

    def close(self):
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
        except Exception:
            pass
        try:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        except Exception:
            pass


class ContextEngineHotAdapter(BenchmarkAdapter):
    """Hot persistent adapter via MCP."""

    name = "context_engine_hot"

    def __init__(self) -> None:
        self._bin = _ensure_built()
        self._clients: Dict[str, _McpClient] = {}

    def _client_for(self, repo_path: Path) -> _McpClient:
        key = str(repo_path.resolve())
        if key not in self._clients:
            self._clients[key] = _McpClient(repo_path)
        return self._clients[key]

    def index(self, repo_path: Path) -> IndexingMetrics:
        t0 = time.perf_counter()
        try:
            client = self._client_for(repo_path)
            data = client.status()
            # data is parsed json from tool result: contains fields like pid, project_root, files_indexed etc
            # Map similar to context_engine.py
            # status fields are camelCase per service.rs StatusReport
            wall = int((time.perf_counter() - t0) * 1000)
            # data may contain stats directly
            files_indexed = data.get("filesIndexed") if "filesIndexed" in data else data.get("files_indexed")
            symbols = data.get("symbols")
            bm25_docs = data.get("bm25Documents") if "bm25Documents" in data else data.get("bm25_documents")
            vector_count = data.get("vectorCount") if "vectorCount" in data else data.get("vector_count")
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
        except Exception as e:
            print(f"[hot] status failed for {repo_path}: {e}", file=sys.stderr)
            wall = int((time.perf_counter() - t0) * 1000)
            return IndexingMetrics(
                initial_wall_ms=wall,
                files_indexed=None,
                symbols=None,
                bm25_docs=None,
                vector_count=None,
                index_disk_bytes=None,
                unavailable=["symbols", "bm25_docs", "vector_count", "index_disk_bytes", "cpu_ms", "peak_rss_mb", "no_change_wall_ms", "one_file_wall_ms", "status_error"],
            )

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        # wall clock for adapter
        t_wall0 = time.perf_counter()
        try:
            client = self._client_for(repo_path)
            data = client.search(query, max_results=top_n, timeout=180)
        except Exception as e:
            print(f"[hot] search failed query={query!r} repo={repo_path}: {e}", file=sys.stderr)
            wall_ms = int((time.perf_counter() - t_wall0) * 1000)
            return SearchResult(
                query=query,
                hits=[],
                candidate_count=0,
                evidence_count=0,
                files_returned=0,
                candidate_tokens=None,
                packed_tokens=None,
                retrievers_used=[f"error:{e}"],
                elapsed_ms=wall_ms,
            )
        wall_ms = int((time.perf_counter() - t_wall0) * 1000)
        # data is parsed from MCP: keys query, type, evidence[], context, stats{}
        evidence = data.get("evidence") or []
        # MCP hot now returns stats as full object via serde (includes total_ms etc)
        # It may be under "stats" or directly; data has "stats"
        stats = data.get("stats") or {}
        # If stats is not dict (maybe raw), try to find
        if not isinstance(stats, dict):
            stats = {}
        hits = []
        for ev in evidence:
            f = ev.get("file") or ev.get("path") or ""
            f = f.replace("\\", "/")
            # lines field "start-end" or ""
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
            if line is None and ev.get("start_line") is not None:
                try:
                    line = int(ev["start_line"])
                except Exception:
                    line = None
            text = ev.get("text")
            score = ev.get("finalScore")
            if score is None:
                score = ev.get("score")
            prov = ev.get("source") or ev.get("provenance") or ""
            symbol = ev.get("symbol")
            rel = ev.get("relation")
            if rel and prov:
                prov = f"{prov}:{rel}"
            elif rel:
                prov = str(rel)
            from .interface import SearchHit

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
        retrievers = stats.get("retrievers") or stats.get("retrievers_used") or []
        # internal elapsed is stats elapsed_ms / total_ms
        internal_ms = g("elapsed_ms", "elapsedMs", None)
        if internal_ms is None:
            internal_ms = g("total_ms", "totalMs", None)
        # if still None, fallback to wall
        if internal_ms is None:
            internal_ms = wall_ms
        else:
            internal_ms = int(internal_ms)

        # stage timings
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
        reconcile_skipped = g("reconcile_skipped", "reconcileSkipped", None)
        discovery_calls = g("discovery_calls", "discoveryCalls", None)
        reconcile_calls = g("reconcile_calls", "reconcileCalls", None)
        runtime_state = g("runtime_state", "runtimeState", None)

        # hot must report both wall and internal
        # wall is adapter-measured per query (excludes startup), internal is engine pipeline
        # transport = wall - total_ms (total includes discovery+reconcile+pipeline)
        total_for_transport = total_ms if total_ms is not None else internal_ms
        transport = wall_ms - total_for_transport if total_for_transport is not None else None
        retrievers = list(retrievers) if isinstance(retrievers, list) else []
        # append wall vs internal for debugging
        retrievers.append(f"wall:{wall_ms}")
        retrievers.append(f"internal:{internal_ms}")
        if total_ms is not None:
            retrievers.append(f"total:{total_ms}")
        if transport is not None:
            retrievers.append(f"transport:{transport}")
        if discovery_ms is not None:
            retrievers.append(f"discovery:{discovery_ms}")
        if reconcile_ms is not None:
            retrievers.append(f"reconcile:{reconcile_ms}")
        # PID diagnostic
        pid_for_query = client.contextd_pid
        startup_for_query = client.startup_ms

        # SearchResult currently has fields for stage timings, we map them
        # It does not have discovery/reconcile fields yet; we will extend it via raw and also via new fields if present
        from .interface import SearchResult as SR

        # determine cache hit: if semantic ran and embed 0, likely cache hit
        cache_hit_val = None
        if semantic_embed_ms is not None and semantic_search_ms is not None:
            # if embed 0 but search >0, we had cache hit (or semantic skipped)
            if semantic_embed_ms == 0 and vector_scanned is not None and vector_scanned > 0:
                cache_hit_val = True
            elif semantic_embed_ms is not None and semantic_embed_ms > 0:
                cache_hit_val = False
        res = SR(
            query=query,
            hits=hits,
            candidate_count=int(candidate_count) if candidate_count is not None else 0,
            evidence_count=int(evidence_count) if evidence_count is not None else len(hits),
            files_returned=int(files_returned) if files_returned is not None else len(set(h.file for h in hits)),
            candidate_tokens=None,
            packed_tokens=int(packed_tokens) if packed_tokens is not None else None,
            retrievers_used=retrievers,
            elapsed_ms=int(internal_ms) if internal_ms is not None else wall_ms,
            wall_ms=wall_ms,
            internal_ms=int(internal_ms) if internal_ms is not None else wall_ms,
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
            cache_hit=cache_hit_val,
            reconcile_skipped=bool(reconcile_skipped) if reconcile_skipped is not None else None,
            discovery_calls=int(discovery_calls) if discovery_calls is not None else None,
            reconcile_calls=int(reconcile_calls) if reconcile_calls is not None else None,
            runtime_state=str(runtime_state) if runtime_state is not None else None,
            process_pid=pid_for_query,
            startup_ms=startup_for_query,
            raw={
                "wall_ms": wall_ms,
                "internal_ms": internal_ms,
                "total_ms": total_ms,
                "transport_ms": transport,
                "startup_ms": startup_for_query,
                "process_pid": pid_for_query,
                "stats": stats,
                "data": data,
                "discovery_ms": discovery_ms,
                "reconcile_ms": reconcile_ms,
                "total_ms": total_ms,
                "semantic_embed_ms": semantic_embed_ms,
                "semantic_search_ms": semantic_search_ms,
                "fusion_ms": fusion_ms,
                "authority_ms": authority_ms,
                "generation": generation,
                "dirty_file_count": dirty,
                "vector_count_scanned": vector_scanned,
                "cache_hit": cache_hit_val,
            },
        )
        return res

    def close(self):
        for c in list(self._clients.values()):
            try:
                c.close()
            except Exception:
                pass
        self._clients.clear()
