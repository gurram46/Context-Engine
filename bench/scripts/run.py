#!/usr/bin/env python3
"""Context Bench v1 runner.

Loads bench/manifest.json + bench/questions/*.jsonl, runs each adapter per repo,
collects retrieval + indexing metrics, writes bench/results/results.jsonl and
bench/results/summary.md.

Usage:
  python bench/scripts/run.py --adapters context_engine,rg_baseline --top-n 5
  python bench/scripts/run.py --repos django,nestjs,ripgrep  # filter for M1

Metrics per query (spec):
  top1_correct, recall@1/3/5, mrr, rank, candidate_count, evidence_count,
  files_returned, candidate_tokens, packed_tokens, compression_ratio,
  retrievers_used, elapsed_ms + per-stage timings when exposed.
"""

from __future__ import annotations

import argparse
import json
import time
import sys
from pathlib import Path
from collections import defaultdict
import subprocess

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = REPO_ROOT / "bench" / "manifest.json"
QUESTIONS_DIR = REPO_ROOT / "bench" / "questions"
RESULTS_DIR = REPO_ROOT / "bench" / "results"
RESULTS_JSONL = RESULTS_DIR / "results.jsonl"
SUMMARY_MD = RESULTS_DIR / "summary.md"

# Import adapters (harness-side, no production import)
sys.path.insert(0, str(REPO_ROOT / "bench"))
from adapters.interface import BenchmarkAdapter  # noqa: E402
from adapters.context_engine import ContextEngineAdapter  # noqa: E402
from adapters.rg_baseline import RgBaselineAdapter  # noqa: E402

ADAPTERS = {
    "context_engine": ContextEngineAdapter,
    "rg_baseline": RgBaselineAdapter,
    # future: oci, codebase_memory, serena (not mandatory in C0)
}


def load_questions():
    qs = []
    for p in QUESTIONS_DIR.glob("*.jsonl"):
        for line in p.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            qs.append(json.loads(line))
    return qs


def compute_retrieval_metrics(expected_files, hits, top_n=5):
    """hits: list of SearchHit in ranked order (already top_n)."""
    # Normalize expected to lower posix
    exp_norm = [e.lower().replace("\\", "/") for e in expected_files]
    hit_files = [h.file.lower().replace("\\", "/") for h in hits]

    def is_hit(file):
        # exact match or suffix match (allow expected "django/db/models/base.py" to match "django/db/models/base.py")
        # and also allow expected to be suffix of hit (if hit has no prefix)
        lf = file.lower()
        for e in exp_norm:
            if lf == e or lf.endswith("/" + e) or e.endswith("/" + lf) or lf.endswith(e):
                return True
        return False

    # find rank of first expected (1-indexed)
    rank = None
    for i, hf in enumerate(hit_files, start=1):
        if is_hit(hf):
            rank = i
            break

    top1 = 1 if rank == 1 else 0
    r1 = 1 if rank is not None and rank <= 1 else 0
    r3 = 1 if rank is not None and rank <= 3 else 0
    r5 = 1 if rank is not None and rank <= 5 else 0
    mrr = 1.0 / rank if rank else 0.0

    return {
        "top1_correct": top1,
        "recall_at_1": r1,
        "recall_at_3": r3,
        "recall_at_5": r5,
        "mrr": mrr,
        "rank": rank,
    }


def ensure_repo(repo_name, manifest_entry):
    dest = REPO_ROOT / "bench" / "repos" / repo_name
    commit = manifest_entry["commit"]
    if not dest.exists():
        print(f"{repo_name}: missing, cloning shallow...", flush=True)
        url = manifest_entry["url"]
        subprocess.run(["git", "clone", "--no-checkout", "--depth", "1", url, str(dest)], check=True)
        subprocess.run(["git", "fetch", "--depth", "1", "origin", commit], cwd=dest, check=True)
        subprocess.run(["git", "checkout", commit], cwd=dest, check=True)
    else:
        # verify commit, fetch if needed
        try:
            cur = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=dest, text=True).strip()
            if cur != commit:
                print(f"{repo_name}: at {cur[:8]} != {commit[:8]}, fetching...", flush=True)
                subprocess.run(["git", "fetch", "--depth", "1", "origin", commit], cwd=dest, check=True)
                subprocess.run(["git", "checkout", commit], cwd=dest, check=True)
        except Exception as e:
            print(f"{repo_name}: verify failed {e}", file=sys.stderr)
    return dest


def main():
    parser = argparse.ArgumentParser(description="Context Bench v1 runner")
    parser.add_argument("--adapters", default="context_engine,rg_baseline", help="comma list of adapter names")
    parser.add_argument("--top-n", type=int, default=5, help="top_n for search")
    parser.add_argument("--repos", default=None, help="comma filter for repo names (e.g. django,nestjs,ripgrep)")
    parser.add_argument("--output", default=str(RESULTS_JSONL), help="output JSONL path")
    parser.add_argument("--manifest", default=str(MANIFEST), help="manifest path")
    args = parser.parse_args()

    manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
    repo_map = {r["name"]: r for r in manifest["repos"]}
    filter_repos = set(s.strip() for s in args.repos.split(",")) if args.repos else None
    adapter_names = [a.strip() for a in args.adapters.split(",") if a.strip()]

    questions = load_questions()
    if filter_repos:
        questions = [q for q in questions if q["repo"] in filter_repos]
    print(f"Loaded {len(questions)} questions", flush=True)
    # group by repo for indexing once
    by_repo = defaultdict(list)
    for q in questions:
        by_repo[q["repo"]].append(q)

    adapters: list[BenchmarkAdapter] = []
    for name in adapter_names:
        if name not in ADAPTERS:
            print(f"unknown adapter {name}, available {list(ADAPTERS)}", file=sys.stderr)
            sys.exit(2)
        adapters.append(ADAPTERS[name]())

    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    out_path = Path(args.output)
    # clear previous
    out_path.write_text("", encoding="utf-8")

    # Collect indexing metrics per adapter per repo
    indexing_results = {}

    for adapter in adapters:
        for repo_name in list(by_repo.keys()):
            if filter_repos and repo_name not in filter_repos:
                continue
            if repo_name not in repo_map:
                print(f"skip {repo_name}: not in manifest", file=sys.stderr)
                continue
            entry = repo_map[repo_name]
            repo_path = ensure_repo(repo_name, entry)
            print(f"[{adapter.name}] indexing {repo_name} @ {entry['commit'][:8]}", flush=True)
            t0 = time.perf_counter()
            try:
                idx = adapter.index(repo_path)
            except Exception as e:
                print(f"[{adapter.name}] index {repo_name} failed: {e}", file=sys.stderr)
                idx = None
            wall = int((time.perf_counter() - t0) * 1000)
            # store for summary
            indexing_results[(adapter.name, repo_name)] = idx
            # also write a synthetic indexing record to results for traceability
            rec = {
                "type": "indexing",
                "adapter": adapter.name,
                "repo": repo_name,
                "commit": entry["commit"],
                "wall_ms": wall,
                "indexing": idx.__dict__ if idx else None,
            }
            with out_path.open("a", encoding="utf-8") as f:
                f.write(json.dumps(rec) + "\n")

    # Query runs
    for adapter in adapters:
        for repo_name, qs in by_repo.items():
            if filter_repos and repo_name not in filter_repos:
                continue
            entry = repo_map[repo_name]
            repo_path = REPO_ROOT / "bench" / "repos" / repo_name
            if not repo_path.exists():
                print(f"skip {repo_name}: repo not checked out", file=sys.stderr)
                continue
            for q in qs:
                query = q["query"]
                expected = q["expected_files"]
                print(f"[{adapter.name}] {q['id']} ({repo_name}/{q['category']})", flush=True)
                res = adapter.search(query, repo_path, top_n=args.top_n)
                metrics = compute_retrieval_metrics(expected, res.hits, top_n=args.top_n)
                # compression ratio: packed / candidate (when both available and candidate >0)
                comp = None
                if res.candidate_tokens and res.candidate_tokens > 0 and res.packed_tokens is not None:
                    comp = round(res.packed_tokens / res.candidate_tokens, 3) if res.candidate_tokens else None
                record = {
                    "type": "query",
                    "id": q["id"],
                    "repo": q["repo"],
                    "category": q["category"],
                    "query": query,
                    "expected_files": expected,
                    "expected_symbols": q.get("expected_symbols", []),
                    "ground_truth_source": q.get("ground_truth_source"),
                    "adapter": adapter.name,
                    "commit": entry["commit"],
                    "top_n": args.top_n,
                    "hits": [{"file": h.file, "score": h.score, "line": h.line, "provenance": h.provenance} for h in res.hits],
                    "top1_correct": metrics["top1_correct"],
                    "recall_at_1": metrics["recall_at_1"],
                    "recall_at_3": metrics["recall_at_3"],
                    "recall_at_5": metrics["recall_at_5"],
                    "mrr": metrics["mrr"],
                    "rank": metrics["rank"],
                    "candidate_count": res.candidate_count,
                    "evidence_count": res.evidence_count,
                    "files_returned": res.files_returned,
                    "candidate_tokens": res.candidate_tokens,
                    "packed_tokens": res.packed_tokens,
                    "compression_ratio": comp,
                    "retrievers_used": res.retrievers_used,
                    "elapsed_ms": res.elapsed_ms,
                    "exact_ms": res.exact_ms,
                    "structural_ms": res.structural_ms,
                    "bm25_ms": res.bm25_ms,
                    "semantic_ms": res.semantic_ms,
                    "rank_ms": res.rank_ms,
                    "pack_ms": res.pack_ms,
                }
                with out_path.open("a", encoding="utf-8") as f:
                    f.write(json.dumps(record) + "\n")

    print(f"Wrote {out_path} ({out_path.stat().st_size} bytes)", flush=True)
    # Run report generator
    report_py = REPO_ROOT / "bench" / "scripts" / "report.py"
    if report_py.exists():
        print("Generating summary...", flush=True)
        subprocess.run([sys.executable, str(report_py), "--input", str(out_path)], check=False)


if __name__ == "__main__":
    main()
