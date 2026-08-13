#!/usr/bin/env python3
"""Generate bench/results/summary.md from results.jsonl.

Produces per SYSTEM / REPO / CATEGORY tables plus macro averages.
Uses FILE-LEVEL evaluation with deduped unique files (run.py).

Metrics:
- Hit@1/3/5 : binary (any relevant in first K unique files)
- Recall@1/3/5 : fractional (relevant retrieved / total relevant)
- MRR, Top1 (=Hit@1)
Do NOT combine incomparable metrics into one fake score.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from collections import defaultdict

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_INPUT = REPO_ROOT / "bench" / "results" / "results.jsonl"
DEFAULT_OUTPUT = REPO_ROOT / "bench" / "results" / "summary.md"


def percentile(vals, p):
    if not vals:
        return None
    s = sorted(vals)
    k = (len(s) - 1) * p / 100
    f = int(k)
    c = min(f + 1, len(s) - 1)
    if f == c:
        return s[f]
    d0 = k - f
    return s[f] * (1 - d0) + s[c] * d0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--input", default=str(DEFAULT_INPUT))
    ap.add_argument("--output", default=str(DEFAULT_OUTPUT))
    args = ap.parse_args()

    inp = Path(args.input)
    out = Path(args.output)
    if not inp.exists():
        print(f"no results at {inp}", file=sys.stderr)
        return

    records = [json.loads(l) for l in inp.read_text(encoding="utf-8").splitlines() if l.strip()]
    queries = [r for r in records if r.get("type") == "query"]
    if not queries:
        print("no query records")
        return

    # Groupings
    by_system = defaultdict(list)
    by_system_repo = defaultdict(list)
    by_system_cat = defaultdict(list)

    for r in queries:
        by_system[r["adapter"]].append(r)
        by_system_repo[(r["adapter"], r["repo"])].append(r)
        by_system_cat[(r["adapter"], r["category"])].append(r)

    def get_hit(r, k):
        # prefer new hit_at_K, fallback to old binary recall_at_K or top1
        if f"hit_at_{k}" in r:
            return r[f"hit_at_{k}"]
        if k == 1 and "top1_correct" in r:
            return r["top1_correct"]
        # fallback: old recall was binary hit
        return r.get(f"recall_at_{k}", 0)

    def get_recall(r, k):
        return r.get(f"recall_at_{k}", 0)

    def stats(rs):
        n = len(rs)
        def avg(key, fn):
            vals = [fn(x) for x in rs]
            return sum(vals) / n if n else 0
        hit1 = avg("hit1", lambda x: get_hit(x, 1))
        hit3 = avg("hit3", lambda x: get_hit(x, 3))
        hit5 = avg("hit5", lambda x: get_hit(x, 5))
        r1 = avg("r1", lambda x: get_recall(x, 1))
        r3 = avg("r3", lambda x: get_recall(x, 3))
        r5 = avg("r5", lambda x: get_recall(x, 5))
        mrr = sum(x["mrr"] for x in rs) / n if n else 0
        lat = [x["elapsed_ms"] for x in rs if x["elapsed_ms"] is not None]
        p50 = percentile(lat, 50)
        p95 = percentile(lat, 95)
        packed = [x["packed_tokens"] for x in rs if x["packed_tokens"] is not None]
        avg_packed = sum(packed) / len(packed) if packed else None
        files = [x["files_returned"] for x in rs if x["files_returned"] is not None]
        avg_files = sum(files) / len(files) if files else None
        uniq = [x.get("unique_files") for x in rs if x.get("unique_files") is not None]
        avg_uniq = sum(uniq) / len(uniq) if uniq else avg_files
        return {
            "n": n,
            "hit1": hit1,
            "hit3": hit3,
            "hit5": hit5,
            "r1": r1,
            "r3": r3,
            "r5": r5,
            "mrr": mrr,
            "p50": p50,
            "p95": p95,
            "avg_packed": avg_packed,
            "avg_files": avg_files,
            "avg_uniq": avg_uniq,
        }

    lines = []
    lines.append("# Context Bench v1 — Summary")
    lines.append("")
    lines.append(f"Generated from `{inp}` — {len(queries)} queries")
    lines.append("")
    lines.append("Note: FILE-LEVEL evaluation uses ranked UNIQUE files (deduplicated preserving first occurrence). Hit@K is binary, Recall@K is fractional (relevant retrieved / total relevant).")
    lines.append("")

    # Overall per system
    lines.append("## SYSTEM")
    lines.append("")
    lines.append("| SYSTEM | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR | P50 | P95 | AVG PACKED | AVG FILES |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for sys_name, rs in sorted(by_system.items()):
        s = stats(rs)
        p50 = f"{s['p50']:.0f}" if s["p50"] is not None else "-"
        p95 = f"{s['p95']:.0f}" if s["p95"] is not None else "-"
        packed = f"{s['avg_packed']:.0f}" if s["avg_packed"] is not None else "-"
        files = f"{s['avg_files']:.1f}" if s["avg_files"] is not None else "-"
        lines.append(
            f"| {sys_name} | {s['n']} | {s['hit1']:.3f} | {s['hit3']:.3f} | {s['hit5']:.3f} | {s['r1']:.3f} | {s['r3']:.3f} | {s['r5']:.3f} | {s['mrr']:.3f} | {p50} | {p95} | {packed} | {files} |"
        )
    lines.append("")

    # Per repo
    lines.append("## REPO")
    lines.append("")
    lines.append("| SYSTEM | REPO | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR | P50 | P95 | PACKED |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for (sys_name, repo), rs in sorted(by_system_repo.items()):
        s = stats(rs)
        p50 = f"{s['p50']:.0f}" if s["p50"] is not None else "-"
        p95 = f"{s['p95']:.0f}" if s["p95"] is not None else "-"
        packed = f"{s['avg_packed']:.0f}" if s["avg_packed"] is not None else "-"
        lines.append(
            f"| {sys_name} | {repo} | {s['n']} | {s['hit1']:.3f} | {s['hit3']:.3f} | {s['hit5']:.3f} | {s['r1']:.3f} | {s['r3']:.3f} | {s['r5']:.3f} | {s['mrr']:.3f} | {p50} | {p95} | {packed} |"
        )
    lines.append("")

    # Per category
    lines.append("## CATEGORY")
    lines.append("")
    lines.append("| SYSTEM | CATEGORY | QS | Hit@1 | Hit@3 | Hit@5 | R@1 | R@3 | R@5 | MRR |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|")
    for (sys_name, cat), rs in sorted(by_system_cat.items()):
        s = stats(rs)
        lines.append(f"| {sys_name} | {cat} | {s['n']} | {s['hit1']:.3f} | {s['hit3']:.3f} | {s['hit5']:.3f} | {s['r1']:.3f} | {s['r3']:.3f} | {s['r5']:.3f} | {s['mrr']:.3f} |")
    lines.append("")

    # Macro averages
    lines.append("## MACRO AVERAGES")
    lines.append("")
    all_stats = [stats(rs) for rs in by_system.values()]
    if all_stats:
        macro_hit1 = sum(s["hit1"] for s in all_stats) / len(all_stats)
        macro_hit3 = sum(s["hit3"] for s in all_stats) / len(all_stats)
        macro_hit5 = sum(s["hit5"] for s in all_stats) / len(all_stats)
        macro_r1 = sum(s["r1"] for s in all_stats) / len(all_stats)
        macro_r3 = sum(s["r3"] for s in all_stats) / len(all_stats)
        macro_r5 = sum(s["r5"] for s in all_stats) / len(all_stats)
        macro_mrr = sum(s["mrr"] for s in all_stats) / len(all_stats)
        lines.append(f"- macro Hit@1: {macro_hit1:.3f}")
        lines.append(f"- macro Hit@3: {macro_hit3:.3f}")
        lines.append(f"- macro Hit@5: {macro_hit5:.3f}")
        lines.append(f"- macro Recall@1: {macro_r1:.3f}")
        lines.append(f"- macro Recall@3: {macro_r3:.3f}")
        lines.append(f"- macro Recall@5: {macro_r5:.3f}")
        lines.append(f"- macro MRR: {macro_mrr:.3f}")
        lines.append("")

    # Indexing
    idx_recs = [r for r in records if r.get("type") == "indexing"]
    if idx_recs:
        lines.append("## INDEXING")
        lines.append("")
        lines.append("| ADAPTER | REPO | WALL MS | FILES | SYMBOLS | BM25 | VECTORS | DISK | UNAVAILABLE |")
        lines.append("|---|---|---|---|---|---|---|---|---|")
        for r in idx_recs:
            idx = r.get("indexing") or {}
            lines.append(
                f"| {r['adapter']} | {r['repo']} | {r.get('wall_ms')} | {idx.get('files_indexed')} | {idx.get('symbols')} | {idx.get('bm25_docs')} | {idx.get('vector_count')} | {idx.get('index_disk_bytes')} | {','.join(idx.get('unavailable', []))} |"
            )
        lines.append("")
        # also include contextd status details if available
        # try to extract from indexing raw? but we store only indexing metrics; status details are in indexing unavailable but we can show notes

    # Cold/warm timings note
    lines.append("## NOTES")
    lines.append("")
    lines.append("- FILE-LEVEL: hits are deduplicated to unique files preserving first rank before Hit/Recall/MRR.")
    lines.append("- Hit@K = binary (1 if any expected file in first K unique files). Recall@K = |relevant ∩ first K| / |expected|.")
    lines.append("- packed_tokens is real tokenizer count from contextd; candidate_tokens unavailable in production (marked None).")
    lines.append("- No benchmark-specific production logic. Failures are roadmap evidence.")
    lines.append("- Previous Python-proxy metrics are INVALID / DISCARDED.")
    lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote {out}")


if __name__ == "__main__":
    main()
