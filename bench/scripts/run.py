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
    """hits: list of SearchHit in ranked order (already top_n).

    FILE-LEVEL evaluation: convert evidence into ranked UNIQUE files
    preserving first occurrence before metrics.
    Hit@K = binary (any relevant in first K unique files)
    Recall@K = relevant retrieved / total relevant (fractional)
    MRR = 1/rank of first relevant unique file
    """
    exp_norm = [e.lower().replace("\\", "/") for e in expected_files]
    # unique expected
    exp_unique = list(dict.fromkeys(exp_norm))
    exp_set = set(exp_unique)
    total_relevant = len(exp_unique) if exp_unique else 1

    # dedupe hits into unique files preserving rank
    seen = set()
    unique_files = []
    unique_hits = []  # SearchHit corresponding to first occurrence per file
    for h in hits:
        norm = h.file.lower().replace("\\", "/")
        if norm not in seen:
            seen.add(norm)
            unique_files.append(norm)
            unique_hits.append(h)

    def matches_expected(hit_file, expected):
        lf = hit_file.lower()
        e = expected.lower()
        return lf == e or lf.endswith("/" + e) or e.endswith("/" + lf) or lf.endswith(e)

    def is_hit_file(hit_file):
        for e in exp_unique:
            if matches_expected(hit_file, e):
                return True
        return False

    # find rank of first relevant unique file (1-indexed)
    rank = None
    for i, uf in enumerate(unique_files, start=1):
        if is_hit_file(uf):
            rank = i
            break

    # Hit@K
    def hit_at(K):
        if K <= 0:
            return 0
        for uf in unique_files[:K]:
            if is_hit_file(uf):
                return 1
        return 0

    # Recall@K
    def recall_at(K):
        if total_relevant == 0:
            return 0.0
        retrieved = 0
        # for each expected, check if it appears in first K unique files
        for e in exp_unique:
            for uf in unique_files[:K]:
                if matches_expected(uf, e):
                    retrieved += 1
                    break
        return retrieved / total_relevant

    top1 = hit_at(1)
    h1 = hit_at(1)
    h3 = hit_at(3)
    h5 = hit_at(5)
    r1 = recall_at(1)
    r3 = recall_at(3)
    r5 = recall_at(5)
    mrr = 1.0 / rank if rank else 0.0
    unique_count = len(unique_files)

    return {
        "top1_correct": top1,
        "hit_at_1": h1,
        "hit_at_3": h3,
        "hit_at_5": h5,
        "recall_at_1": r1,
        "recall_at_3": r3,
        "recall_at_5": r5,
        "mrr": mrr,
        "rank": rank,
        "unique_files": unique_count,
        "deduplicated_from": len(hits),
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
    parser.add_argument("--profile", choices=["smoke", "official"], default="official", help="smoke=fast local with bench-created .ignore (never for public claims), official=exact pinned upstream (default)")
    args = parser.parse_args()
    profile = args.profile
    print(f"Profile: {profile} (smoke=with bench .ignore, official=exact upstream)", flush=True)

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
    # Profile handling: smoke vs official
    # SMOKE: bench-created .ignore for fast iteration (NEVER for public claims)
    # OFFICIAL: exact upstream, respect only repo-native ignores
    SMOKE_IGNORES = {
        "django": "# bench smoke profile: ignore large vendor/static for fast local iteration (NEVER for official)\n"
                  "django/contrib/admin/static/**\n"
                  "docs/**\n"
                  "django/contrib/gis/**\n",
        "nestjs": "# bench smoke profile: ignore sample/integration for fast local (NEVER for official)\n"
                  "sample/**\n"
                  "integration/**\n",
    }
    for repo_name in list(by_repo.keys()):
        repo_path = REPO_ROOT / "bench" / "repos" / repo_name
        if not repo_path.exists():
            continue
        ignore_path = repo_path / ".ignore"
        is_smoke_ignore = ignore_path.exists() and "bench smoke profile" in ignore_path.read_text(encoding="utf-8", errors="ignore")[:500]
        if profile == "official":
            if is_smoke_ignore:
                print(f"[{repo_name}] official profile: removing bench-created .ignore", flush=True)
                try:
                    ignore_path.unlink()
                except Exception as e:
                    print(f"  failed to remove .ignore: {e}", file=sys.stderr)
                # also need to clean index that was built with smoke ignores — force rebuild on next search
                # Remove .context/index to ensure official indexing is not polluted by smoke DB
                ctx_index = repo_path / ".context" / "index"
                if ctx_index.exists():
                    print(f"  cleaning .context/index for official (was smoke)", flush=True)
                    import shutil as _shutil
                    try:
                        _shutil.rmtree(ctx_index)
                    except Exception as e:
                        print(f"    rmtree failed: {e}", file=sys.stderr)
        else:  # smoke
            if repo_name in SMOKE_IGNORES:
                if not is_smoke_ignore:
                    print(f"[{repo_name}] smoke profile: creating bench .ignore", flush=True)
                    try:
                        ignore_path.write_text(SMOKE_IGNORES[repo_name], encoding="utf-8")
                    except Exception as e:
                        print(f"  failed to write .ignore: {e}", file=sys.stderr)
                    # clean old official index if exists, so smoke rebuilds with ignores
                    ctx_index = repo_path / ".context" / "index"
                    if ctx_index.exists():
                        import shutil as _shutil
                        try:
                            _shutil.rmtree(ctx_index)
                            print(f"  cleaned .context/index for smoke", flush=True)
                        except Exception as e:
                            print(f"    rmtree failed: {e}", file=sys.stderr)

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
            status_data = getattr(adapter, "_last_status", None) if adapter.name == "context_engine" else None
            rec = {
                "type": "indexing",
                "adapter": adapter.name,
                "repo": repo_name,
                "commit": entry["commit"],
                "profile": profile,
                "wall_ms": wall,
                "indexing": idx.__dict__ if idx else None,
                "status": status_data,
            }
            with out_path.open("a", encoding="utf-8") as f:
                f.write(json.dumps(rec) + "\n")

    # Additional timing measurements for real contextd: cold first-search, warm, one-file-change
    # Skip for quick accuracy run; enable via CONTEXT_BENCH_TIMING=1
    import os as _os
    _do_timing = _os.environ.get("CONTEXT_BENCH_TIMING") == "1"
    for adapter in adapters:
        if not _do_timing or adapter.name != "context_engine":
            continue
        for repo_name, qs in by_repo.items():
            if filter_repos and repo_name not in filter_repos:
                continue
            repo_path = REPO_ROOT / "bench" / "repos" / repo_name
            if not repo_path.exists():
                continue
            # neutral query: first question's query for this repo
            neutral_q = qs[0]["query"] if qs else "Model"
            # Attempt to capture cold first-search by optionally removing index file (disposable checkout only)
            # Remove ONLY the bench checkout's contextd index to force cold rebuild, if it exists
            index_db = repo_path / ".context" / "index" / "structural.db"
            had_index = index_db.exists()
            if had_index:
                try:
                    # remove index files to force cold rebuild on next search; keep backup via rename if needed
                    # For C0, we remove only this checkout's index; it will be rebuilt via reconcile
                    import shutil
                    idx_dir = repo_path / ".context" / "index"
                    # save backup list
                    backups = []
                    for pat in ["structural.db", "structural.db-wal", "structural.db-shm"]:
                        p = idx_dir / pat
                        if p.exists():
                            bak = p.with_suffix(p.suffix + ".bak_timing")
                            try:
                                p.rename(bak)
                                backups.append((p, bak))
                            except Exception:
                                pass
                    # cold measurement
                    t0 = time.perf_counter()
                    # Use adapter.search which will trigger reconcile+build if index missing
                    cold_res = adapter.search(neutral_q, repo_path, top_n=5)
                    cold_wall = cold_res.elapsed_ms
                    # restore backups? Actually keep rebuilt index for subsequent warm measurement; remove backups
                    for orig, bak in backups:
                        try:
                            if bak.exists():
                                bak.unlink()
                        except Exception:
                            pass
                    # warm measurement (no change)
                    t1 = time.perf_counter()
                    warm_res = adapter.search(neutral_q, repo_path, top_n=5)
                    warm_wall = warm_res.elapsed_ms
                    # one-file-change: create disposable file, measure, restore exactly
                    tmp_file = repo_path / "__bench_tmp_one_file_change__.txt"
                    try:
                        tmp_file.write_text("bench timing probe", encoding="utf-8")
                        t2 = time.perf_counter()
                        one_res = adapter.search(neutral_q, repo_path, top_n=5)
                        one_wall = one_res.elapsed_ms
                    finally:
                        try:
                            if tmp_file.exists():
                                tmp_file.unlink()
                        except Exception:
                            pass
                    # reconcile after removal of tmp file to restore state (next search will see deletion)
                    try:
                        adapter.search(neutral_q, repo_path, top_n=5)
                    except Exception:
                        pass
                    timing_rec = {
                        "type": "timing",
                        "adapter": adapter.name,
                        "repo": repo_name,
                        "profile": profile,
                        "neutral_query": neutral_q,
                        "cold_first_search_wall_ms": cold_wall,
                        "warm_no_change_wall_ms": warm_wall,
                        "one_file_change_wall_ms": one_wall,
                        "had_index_before": had_index,
                        "label": "cold first-search wall time (not pure index time)",
                    }
                    with out_path.open("a", encoding="utf-8") as f:
                        f.write(json.dumps(timing_rec) + "\n")
                    print(f"[{adapter.name}] timing {repo_name}: cold {cold_wall}ms warm {warm_wall}ms one-file {one_wall}ms", flush=True)
                except Exception as e:
                    print(f"[{adapter.name}] timing {repo_name} failed: {e}", file=sys.stderr)
            else:
                # no index to delete; just measure cold as first search wall, warm as second
                try:
                    t0 = time.perf_counter()
                    cold_res = adapter.search(neutral_q, repo_path, top_n=5)
                    cold_wall = cold_res.elapsed_ms
                    warm_res = adapter.search(neutral_q, repo_path, top_n=5)
                    warm_wall = warm_res.elapsed_ms
                    tmp_file = repo_path / "__bench_tmp_one_file_change__.txt"
                    try:
                        tmp_file.write_text("bench timing probe", encoding="utf-8")
                        one_res = adapter.search(neutral_q, repo_path, top_n=5)
                        one_wall = one_res.elapsed_ms
                    finally:
                        try:
                            if tmp_file.exists():
                                tmp_file.unlink()
                        except Exception:
                            pass
                    try:
                        adapter.search(neutral_q, repo_path, top_n=5)
                    except Exception:
                        pass
                    timing_rec = {
                        "type": "timing",
                        "adapter": adapter.name,
                        "repo": repo_name,
                        "profile": profile,
                        "neutral_query": neutral_q,
                        "cold_first_search_wall_ms": cold_wall,
                        "warm_no_change_wall_ms": warm_wall,
                        "one_file_change_wall_ms": one_wall,
                        "had_index_before": False,
                        "label": "cold first-search wall time (not pure index time)",
                    }
                    with out_path.open("a", encoding="utf-8") as f:
                        f.write(json.dumps(timing_rec) + "\n")
                    print(f"[{adapter.name}] timing {repo_name}: cold {cold_wall}ms warm {warm_wall}ms one-file {one_wall}ms (no prior index)", flush=True)
                except Exception as e:
                    print(f"[{adapter.name}] timing {repo_name} failed: {e}", file=sys.stderr)

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
                    "profile": profile,
                    "top_n": args.top_n,
                    "hits": [{"file": h.file, "score": h.score, "line": h.line, "provenance": h.provenance} for h in res.hits],
                    "top1_correct": metrics["top1_correct"],
                    "hit_at_1": metrics["hit_at_1"],
                    "hit_at_3": metrics["hit_at_3"],
                    "hit_at_5": metrics["hit_at_5"],
                    "recall_at_1": metrics["recall_at_1"],
                    "recall_at_3": metrics["recall_at_3"],
                    "recall_at_5": metrics["recall_at_5"],
                    "mrr": metrics["mrr"],
                    "rank": metrics["rank"],
                    "unique_files": metrics["unique_files"],
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
