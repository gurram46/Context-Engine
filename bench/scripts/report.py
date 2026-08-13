#!/usr/bin/env python3
"""Generate bench/results/summary.md from results.jsonl.

Produces per SYSTEM / REPO / CATEGORY tables plus macro averages.
Do NOT combine incomparable metrics into one fake score.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from collections import defaultdict
import statistics

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
    by_repo = defaultdict(list)
    by_cat = defaultdict(list)
    by_system_repo = defaultdict(list)
    by_system_cat = defaultdict(list)

    for r in queries:
        by_system[r["adapter"]].append(r)
        by_repo[r["repo"]].append(r)
        by_cat[r["category"]].append(r)
        by_system_repo[(r["adapter"], r["repo"])].append(r)
        by_system_cat[(r["adapter"], r["category"])].append(r)

    def stats(rs):
        n = len(rs)
        top1 = sum(x["top1_correct"] for x in rs) / n if n else 0
        r3 = sum(x["recall_at_3"] for x in rs) / n if n else 0
        r5 = sum(x["recall_at_5"] for x in rs) / n if n else 0
        mrr = sum(x["mrr"] for x in rs) / n if n else 0
        lat = [x["elapsed_ms"] for x in rs if x["elapsed_ms"] is not None]
        p50 = percentile(lat, 50)
        p95 = percentile(lat, 95)
        packed = [x["packed_tokens"] for x in rs if x["packed_tokens"] is not None]
        avg_packed = sum(packed) / len(packed) if packed else None
        files = [x["files_returned"] for x in rs if x["files_returned"] is not None]
        avg_files = sum(files) / len(files) if files else None
        cand_tok = [x["candidate_tokens"] for x in rs if x["candidate_tokens"] is not None]
        avg_cand = sum(cand_tok) / len(cand_tok) if cand_tok else None
        comp = [x["compression_ratio"] for x in rs if x["compression_ratio"] is not None]
        avg_comp = sum(comp) / len(comp) if comp else None
        return {
            "n": n,
            "top1": top1,
            "r3": r3,
            "r5": r5,
            "mrr": mrr,
            "p50": p50,
            "p95": p95,
            "avg_packed": avg_packed,
            "avg_files": avg_files,
            "avg_cand": avg_cand,
            "avg_comp": avg_comp,
        }

    # Build markdown
    lines = []
    lines.append("# Context Bench v1 — Summary")
    lines.append("")
    lines.append(f"Generated from `{inp}` — {len(queries)} queries")
    lines.append("")

    # Overall per system
    lines.append("## SYSTEM")
    lines.append("")
    lines.append("| SYSTEM | QUESTIONS | TOP1 | R@3 | R@5 | MRR | P50 LAT | P95 LAT | AVG PACKED | AVG FILES | COMP |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|---|")
    for sys_name, rs in sorted(by_system.items()):
        s = stats(rs)
        lines.append(
            f"| {sys_name} | {s['n']} | {s['top1']:.3f} | {s['r3']:.3f} | {s['r5']:.3f} | {s['mrr']:.3f} | {s['p50']:.0f} | {s['p95']:.0f} | {s['avg_packed']:.0f} | {s['avg_files']:.1f} | {s['avg_comp']:.3f} |"
            if s["p50"] is not None
            else f"| {sys_name} | {s['n']} | {s['top1']:.3f} | {s['r3']:.3f} | {s['r5']:.3f} | {s['mrr']:.3f} | - | - | {s['avg_packed']} | {s['avg_files']} | - |"
        )
    lines.append("")

    # Per repo
    lines.append("## REPO")
    lines.append("")
    lines.append("| SYSTEM | REPO | QUESTIONS | TOP1 | R@3 | R@5 | MRR | P50 | P95 | PACKED |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|")
    for (sys_name, repo), rs in sorted(by_system_repo.items()):
        s = stats(rs)
        lines.append(
            f"| {sys_name} | {repo} | {s['n']} | {s['top1']:.3f} | {s['r3']:.3f} | {s['r5']:.3f} | {s['mrr']:.3f} | {s['p50']:.0f} | {s['p95']:.0f} | {s['avg_packed']:.0f} |"
        )
    lines.append("")

    # Per category
    lines.append("## CATEGORY")
    lines.append("")
    lines.append("| SYSTEM | CATEGORY | QUESTIONS | TOP1 | R@3 | R@5 | MRR |")
    lines.append("|---|---|---|---|---|---|---|")
    for (sys_name, cat), rs in sorted(by_system_cat.items()):
        s = stats(rs)
        lines.append(f"| {sys_name} | {cat} | {s['n']} | {s['top1']:.3f} | {s['r3']:.3f} | {s['r5']:.3f} | {s['mrr']:.3f} |")
    lines.append("")

    # Overall macro averages (mean of per-system)
    lines.append("## MACRO AVERAGES")
    lines.append("")
    all_stats = [stats(rs) for rs in by_system.values()]
    if all_stats:
        macro_top1 = sum(s["top1"] for s in all_stats) / len(all_stats)
        macro_r3 = sum(s["r3"] for s in all_stats) / len(all_stats)
        macro_r5 = sum(s["r5"] for s in all_stats) / len(all_stats)
        macro_mrr = sum(s["mrr"] for s in all_stats) / len(all_stats)
        lines.append(f"- macro Top1: {macro_top1:.3f}")
        lines.append(f"- macro R@3: {macro_r3:.3f}")
        lines.append(f"- macro R@5: {macro_r5:.3f}")
        lines.append(f"- macro MRR: {macro_mrr:.3f}")
        lines.append("")

    # Indexing section
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

    # Notes
    lines.append("## NOTES")
    lines.append("")
    lines.append("- candidate_tokens / packed_tokens are raw whitespace-split counts (harness-side, not tiktoken).")
    lines.append("- compression_ratio = packed / candidate; not 'tokens saved'. Real savings need controlled agent A/B later.")
    lines.append("- No benchmark-specific production logic was used. Failures are recorded as evidence for roadmap.")
    lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote {out}")


if __name__ == "__main__":
    main()
