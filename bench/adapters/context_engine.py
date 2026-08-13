"""Context Engine adapter for bench v1.

On bench branch (main) this is a *harness-side* re-implementation that mirrors
production ranking enough to be measurable, without importing production code.
It must NOT be tuned to make benchmark cases pass — record failures honestly.

Later, when Rust contextd is available, this adapter can delegate to the real
binary via `contextd search --json` without changing the harness interface.

Current implementation (C0):
- exact via ripgrep (rg) with generic excludes
- file kind via extension / path heuristics (mirrors crates/context-index)
- tiny authority (testWhenAsked/sourceWhenTestAsked/doc penalty) — generic only
- fuse by final_score (base*20 + authority) + dedup per file
- pack via truncation (no tiktoken; count via whitespace split)

Indexing metrics are measured via a lightweight Python walk (no Tantivy/ONNX).
"""

from __future__ import annotations

import subprocess
import time
from pathlib import Path
from typing import List

from .interface import BenchmarkAdapter, IndexingMetrics, SearchHit, SearchResult


# --- helpers: file kind (generic, not repo-specific) ---

def classify_file(path: str) -> str:
    p = Path(path)
    lower = p.name.lower()
    ext = p.suffix.lower().lstrip(".")
    s = str(path).lower().replace("\\", "/")
    # tests/ dirs
    if "/tests/" in s or "/test/" in s or "/__tests__/" in s:
        if ext in ("py", "ts", "js", "tsx", "jsx", "go", "rs", "java", "kt", "rb", "php"):
            return "test"
        return "test"
    if lower.startswith("test_") or "_test." in lower:
        return "test"
    if lower.endswith(".test.ts") or lower.endswith(".spec.ts") or lower.endswith(".spec.js"):
        return "test"
    if ext in ("md", "rst", "txt"):
        return "doc"
    if ext in ("json", "yaml", "yml", "toml", "ini"):
        return "config"
    if ext in ("py", "ts", "tsx", "js", "jsx", "go", "rs", "java", "kt", "rb", "php", "c", "cpp"):
        return "source"
    return "unknown"


ENGINE_EXCLUDES = [
    ".git",
    ".context",
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


def _rg_available() -> bool:
    try:
        subprocess.run(["rg", "--version"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        return True
    except Exception:
        return False


def _exact_via_rg(repo: Path, term: str, max_results: int = 50) -> List[dict]:
    """Run rg --fixed-strings for term, return [{file, line, text}]."""
    if not term.strip():
        return []
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
    # rg exit 0 = matches, 1 = no matches, 2 = error
    if proc.returncode not in (0, 1):
        return []
    stdout = proc.stdout or ""
    out = []
    for line in stdout.splitlines():
        # file:line:text
        # Use splitn 3 to handle colons in text
        parts = line.split(":", 2)
        if len(parts) < 3:
            continue
        file, lno, text = parts
        # strip leading ./ from rg when run with "."
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


def _authority_score(file: str, query: str, kind: str, text: str = "") -> tuple[int, list[str]]:
    """Tiny generic authority, mirrors authority.rs but minimal and not benchmark-tuned."""
    score = 0
    reasons = []
    ql = query.lower()
    is_test_query = "test" in ql or "spec" in ql or "what tests" in ql or "cover" in ql
    is_def_query = ("where is" in ql and "implemented" in ql) or ("definition" in ql)
    if kind == "test" and is_test_query:
        score += 38
        reasons.append("+38 test when asked")
    if kind == "doc" and is_test_query:
        score -= 20
        reasons.append("-20 doc when test asked")
    if kind == "source" and is_test_query:
        score -= 12
        reasons.append("-12 source when test asked")
    if kind == "test" and not is_test_query:
        score -= 12
        reasons.append("-12 test when impl wanted")
    # Generic definition boost: for definition queries, prefer source files where text looks like definition
    if is_def_query and kind == "source" and text:
        low = text.lower()
        # check if text contains class/def for an identifier in query
        import re

        ids = re.findall(r"[A-Za-z_][A-Za-z0-9_]*", query)
        stops = {"where", "what", "who", "how", "is", "are", "for", "the", "and", "or", "test", "tests", "cover", "covers", "implemented", "implementation"}
        for tok in ids:
            if tok.lower() in stops or len(tok) < 3:
                continue
            if f"class {tok.lower()}" in low or f"def {tok.lower()}" in low or f"func {tok}" in text or f"type {tok}" in text:
                score += 18
                reasons.append(f"+18 true definition ({tok})")
                break
    # No path-specific bonuses (tests/ etc.) — generic only
    return score, reasons


class ContextEngineAdapter(BenchmarkAdapter):
    name = "context_engine"

    def index(self, repo_path: Path) -> IndexingMetrics:
        t0 = time.perf_counter()
        files = []
        for p in repo_path.rglob("*"):
            if p.is_dir():
                continue
            rel = p.relative_to(repo_path).as_posix()
            low = rel.lower()
            # skip engine excludes
            if any(low == pat or low.startswith(pat + "/") or f"/{pat}/" in low for pat in ENGINE_EXCLUDES):
                continue
            # skip generated
            if "dist/" in low or "target/" in low or "node_modules/" in low:
                continue
            files.append(rel)
        wall = int((time.perf_counter() - t0) * 1000)
        # Count by kind
        # symbols/bm25/vectors are unavailable in this harness re-implementation
        return IndexingMetrics(
            initial_wall_ms=wall,
            files_indexed=len(files),
            index_disk_bytes=None,
            symbols=None,
            bm25_docs=None,
            vector_count=None,
            unavailable=["symbols", "bm25_docs", "vector_count", "index_disk_bytes", "cpu_ms", "peak_rss_mb"],
        )

    def search(self, query: str, repo_path: Path, top_n: int = 5) -> SearchResult:
        t0 = time.perf_counter()
        t_exact = time.perf_counter()
        # --- exact file lookup for exact category (query looks like path) ---
        # If query contains a filename with dot, check existence directly (generic, not benchmark-specific)
        import re

        file_like = re.findall(r"[\w./-]+\.\w+", query)
        file_hits: List[dict] = []
        for cand in file_like:
            # strip punctuation
            cand_clean = cand.strip('",.?:;()')
            # try to find file by suffix match
            for p in repo_path.rglob(cand_clean):
                if p.is_file():
                    rel = p.relative_to(repo_path).as_posix()
                    # only if suffix matches expected (for global_settings.py etc.)
                    if rel.lower().endswith(cand_clean.lower()) or cand_clean.lower() in rel.lower():
                        file_hits.append({"file": rel, "line": 1, "text": f"File exists: {rel}", "term": cand_clean})
            # also try basename only
            base = Path(cand_clean).name
            if base != cand_clean:
                for p in repo_path.rglob(base):
                    if p.is_file() and p.name.lower() == base.lower():
                        rel = p.relative_to(repo_path).as_posix()
                        if rel not in [h["file"] for h in file_hits]:
                            file_hits.append({"file": rel, "line": 1, "text": f"File exists: {rel}", "term": base})

        # extract identifiers: take longest alphanumeric token >=3 chars
        tokens = re.findall(r"[A-Za-z_][A-Za-z0-9_]*", query)
        # filter stop like the real engine but minimal
        stops = {"where", "what", "who", "how", "is", "are", "for", "the", "and", "or", "test", "tests", "cover", "covers", "implemented", "implementation", "find", "how", "does"}
        ids = [t for t in tokens if t.lower() not in stops and len(t) >= 3]
        # pick top 2 ids for exact queries (like plan.rs for Test)
        # For C0: if test query, also add test_<id> variant
        exact_terms = []
        ql = query.lower()
        is_test = "test" in ql or "what tests" in ql
        is_def = ("where is" in ql and "implemented" in ql)
        if is_test and ids:
            base = ids[0]
            exact_terms.append(base)
            # test_ variant generic
            snake = re.sub(r"(?<!^)(?=[A-Z])", "_", base).lower()
            exact_terms.append(f"test_{snake.lower()}")
            if len(ids) > 1:
                exact_terms.append(ids[1])
        elif is_def and ids:
            # for definition, search for both raw and class/def variants
            base = ids[0]
            exact_terms.append(base)
            exact_terms.append(f"class {base}")
            exact_terms.append(f"def {base}")
            if len(ids) > 1:
                exact_terms.append(ids[1])
        else:
            # for others, just first id
            if ids:
                exact_terms.append(ids[0])
            # fallback to raw query words if no id
            if not exact_terms:
                for w in query.split():
                    if len(w) >= 3:
                        exact_terms.append(w)
                        break

        # Deduplicate preserve order
        seen = set()
        uniq_terms = []
        for t in exact_terms:
            if t.lower() not in seen:
                seen.add(t.lower())
                uniq_terms.append(t)

        all_hits: List[dict] = []
        # file hits first (higher priority)
        all_hits.extend(file_hits)
        for term in uniq_terms[:4]:
            hits = _exact_via_rg(repo_path, term, max_results=100)
            for h in hits:
                h["term"] = term
                all_hits.append(h)

        exact_ms = int((time.perf_counter() - t_exact) * 1000)

        # Authority + fuse (simplified)
        t_rank = time.perf_counter()
        scored = []
        for h in all_hits:
            kind = classify_file(h["file"])
            auth, reasons = _authority_score(h["file"], query, kind, h["text"])
            base = 1.0  # exact literal
            # file lookup hits get a small boost (generic, not benchmark-specific)
            if h.get("text", "").startswith("File exists:"):
                auth += 10
                reasons.append("+10 file exists")
            final = base * 20 + auth
            scored.append({
                "file": h["file"],
                "line": h["line"],
                "text": h["text"],
                "kind": kind,
                "base": base,
                "auth": auth,
                "final": final,
                "reasons": reasons,
                "prov": "rg:exact",
            })
        # sort by final desc, then file asc for determinism
        scored.sort(key=lambda x: (-x["final"], x["file"]))
        # dedup per file:line
        seen_keys = set()
        deduped = []
        for s in scored:
            key = f"{s['file']}:{s['line']}"
            if key not in seen_keys:
                seen_keys.add(key)
                deduped.append(s)
        # collapse per file keep top 3 per file
        from collections import defaultdict

        by_file = defaultdict(list)
        for s in deduped:
            by_file[s["file"]].append(s)
        collapsed = []
        for f, lst in by_file.items():
            lst.sort(key=lambda x: -x["final"])
            collapsed.extend(lst[:3])

        collapsed.sort(key=lambda x: (-x["final"], x["file"]))
        top = collapsed[:top_n]
        rank_ms = int((time.perf_counter() - t_rank) * 1000)

        # Pack: simple token count via whitespace split, candidate tokens = sum of all scored texts
        t_pack = time.perf_counter()
        def count_tokens(s: str) -> int:
            return len(s.split())

        candidate_tokens = sum(count_tokens(s["text"]) for s in scored) if scored else 0
        packed_tokens = sum(count_tokens(s["text"]) for s in top) if top else 0
        pack_ms = int((time.perf_counter() - t_pack) * 1000)

        elapsed = int((time.perf_counter() - t0) * 1000)

        hits_out = [
            SearchHit(
                file=s["file"],
                score=s["final"],
                line=s["line"],
                text=s["text"],
                provenance=s["prov"],
            )
            for s in top
        ]

        return SearchResult(
            query=query,
            hits=hits_out,
            candidate_count=len(scored),
            evidence_count=len(top),
            files_returned=len(set(h.file for h in hits_out)),
            candidate_tokens=candidate_tokens,
            packed_tokens=packed_tokens,
            retrievers_used=[f"rg:exact:{len(all_hits)}", "authority:generic", "fuse:collapse"],
            elapsed_ms=elapsed,
            exact_ms=exact_ms,
            structural_ms=None,
            bm25_ms=None,
            semantic_ms=None,
            rank_ms=rank_ms,
            pack_ms=pack_ms,
            raw={"scored_sample": scored[:3]},
        )
