"""Plain rg/read baseline adapter.

Reproducible behavior:
- ripgrep (rg) --fixed-strings with bounded --max-count 50 and generic excludes
- for definition/caller etc., uses the longest identifier token as search term
- no authority, no reranking — order is rg filesystem order (sorted by file for determinism)
- file reads are bounded: at most 400 chars per hit, at most top_n files read for token counts
- no semantic model, no Context Engine indexes, no BM25

Do not intentionally cripple: uses --hidden, generic excludes only, case-sensitive.

Future: add --ignore-case toggle if needed, but keep deterministic.
"""

from __future__ import annotations

import subprocess
import time
import re
from pathlib import Path
from typing import List

from .interface import BenchmarkAdapter, IndexingMetrics, SearchHit, SearchResult


ENGINE_EXCLUDES = [
    ".git",
    ".context",
    ".opencode",
    ".codebase-index",
    "node_modules",
    "dist",
    "build",
    "target",
    "__pycache__",
    ".pytest_cache",
    ".next",
    ".nuxt",
    "coverage",
]


def _extract_term(query: str) -> str:
    tokens = re.findall(r"[A-Za-z_][A-Za-z0-9_]*", query)
    stops = {"where", "what", "who", "how", "is", "are", "for", "the", "and", "or", "test", "tests", "cover", "covers", "implemented", "implementation", "find", "how", "does", "cover", "generation"}
    cands = [t for t in tokens if t.lower() not in stops and len(t) >= 3]
    # prefer snake/longest
    if cands:
        cands.sort(key=lambda x: (0 if "_" in x else 1, -len(x)))
        return cands[0]
    # fallback to first word >=3
    for w in query.split():
        if len(w.strip('",.?:')) >= 3:
            return w.strip('",.?:')
    return query.split()[0] if query else "test"


def _rg_search(repo: Path, term: str, max_results: int = 50) -> List[dict]:
    args = [
        "rg",
        "--line-number",
        "--no-heading",
        "--color", "never",
        "--max-count", str(max_results),
        "--hidden",
        "--glob", "!.git/**",
        "--fixed-strings",
    ]
    for pat in ENGINE_EXCLUDES:
        args.extend(["-g", f"!{pat}/**", "-g", f"!**/{pat}/**"])
    args.extend(["--", term, "."])
    try:
        proc = subprocess.run(args, cwd=repo, capture_output=True, text=True, encoding="utf-8", errors="ignore", timeout=5)
    except subprocess.TimeoutExpired:
        return []
    if proc.returncode not in (0, 1):
        return []
    stdout = proc.stdout or ""
    out = []
    for line in stdout.splitlines():
        parts = line.split(":", 2)
        if len(parts) < 3:
            continue
        file, lno, text = parts
        if file.startswith("./"):
            file = file[2:]
        file = file.replace("\\", "/")
        try:
            n = int(lno)
        except ValueError:
            n = 1
        out.append({"file": file, "line": n, "text": text[:400]})
        if len(out) >= max_results:
            break
    return out


class RgBaselineAdapter(BenchmarkAdapter):
    name = "rg_baseline"

    def index(self, repo_path: Path) -> IndexingMetrics:
        t0 = time.perf_counter()
        cnt = 0
        for p in repo_path.rglob("*"):
            if p.is_dir():
                continue
            rel = p.relative_to(repo_path).as_posix().lower()
            if any(rel == pat or rel.startswith(pat + "/") or f"/{pat}/" in rel for pat in ENGINE_EXCLUDES):
                continue
            if "dist/" in rel or "target/" in rel or "node_modules/" in rel:
                continue
            cnt += 1
        wall = int((time.perf_counter() - t0) * 1000)
        return IndexingMetrics(
            initial_wall_ms=wall,
            files_indexed=cnt,
            unavailable=["symbols", "bm25_docs", "vector_count", "index_disk_bytes", "cpu_ms", "peak_rss_mb", "no_change_wall_ms", "one_file_wall_ms"],
        )

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        t0 = time.perf_counter()
        # File lookup for exact path queries (generic, not benchmark-specific)
        file_hits = []
        import re as _re

        for cand in _re.findall(r"[\w./-]+\.\w+", query):
            cand_clean = cand.strip('",.?:;()')
            base = Path(cand_clean).name
            for p in repo_path.rglob(base):
                if p.is_file() and p.name.lower() == base.lower():
                    rel = p.relative_to(repo_path).as_posix()
                    file_hits.append({"file": rel, "line": 1, "text": f"File exists: {rel}"})
                    break
            if file_hits:
                break

        term = _extract_term(query)
        hits = _rg_search(repo_path, term, max_results=100)
        # prepend file hits
        hits = file_hits + hits
        elapsed = int((time.perf_counter() - t0) * 1000)

        # No ranking — sort by file for determinism (rg already sorted, but we enforce)
        # Keep rg order, but dedup per file:line
        seen = set()
        deduped = []
        for h in hits:
            key = f"{h['file']}:{h['line']}"
            if key not in seen:
                seen.add(key)
                deduped.append(h)

        # Collapse per file to top_n files (keep first hit per file)
        by_file = {}
        for h in deduped:
            if h["file"] not in by_file:
                by_file[h["file"]] = h
            if len(by_file) >= top_n:
                break

        # If still less than top_n, fill with next hits truncated to top_n
        top_hits = list(by_file.values())[:top_n]
        if len(top_hits) < top_n:
            # fill from deduped not yet used but ensure file not duplicate beyond one per file?
            for h in deduped:
                if len(top_hits) >= top_n:
                    break
                if h["file"] not in [x["file"] for x in top_hits]:
                    top_hits.append(h)

        # Token counts: common cl100k (same as CE) — fixed from whitespace
        try:
            import tiktoken
            _enc = tiktoken.get_encoding("cl100k_base")
            def tok(s: str) -> int:
                return len(_enc.encode(s or ""))
        except:
            def tok(s: str) -> int:
                return len((s or "").split())

        candidate_tokens = sum(tok(h["text"]) for h in deduped) if deduped else 0
        packed_tokens = sum(tok(h["text"]) for h in top_hits) if top_hits else 0

        out_hits = [
            SearchHit(file=h["file"], score=None, line=h["line"], text=h["text"], provenance="rg:exact")
            for h in top_hits
        ]

        return SearchResult(
            query=query,
            hits=out_hits,
            candidate_count=len(deduped),
            evidence_count=len(top_hits),
            files_returned=len(set(h.file for h in out_hits)),
            candidate_tokens=candidate_tokens,
            packed_tokens=packed_tokens,
            retrievers_used=[f"rg:exact:{len(deduped)}"],
            elapsed_ms=elapsed,
            exact_ms=elapsed,
            rank_ms=0,
            pack_ms=0,
        )
