#!/usr/bin/env python3
"""C0.5 failure diagnostics: capture per-query classification, plan, retriever counts, top10."""
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "bench"))
from adapters.context_engine import ContextEngineAdapter
from adapters.rg_baseline import RgBaselineAdapter

MANIFEST = REPO_ROOT / "bench" / "manifest.json"
QUESTIONS_DIR = REPO_ROOT / "bench" / "questions"

def load_questions():
    qs=[]
    for p in QUESTIONS_DIR.glob("*.jsonl"):
        for line in p.read_text(encoding="utf-8").splitlines():
            line=line.strip()
            if not line or line.startswith("#"):
                continue
            qs.append(json.loads(line))
    return qs

def ensure_repo(repo_name):
    # assume already checked out via run.py
    return REPO_ROOT / "bench" / "repos" / repo_name

def main():
    import argparse
    ap=argparse.ArgumentParser()
    ap.add_argument("--repos", default="django,nestjs,ripgrep")
    ap.add_argument("--profile", choices=["smoke","official"], default="smoke")
    args=ap.parse_args()
    filter_repos=set(args.repos.split(",")) if args.repos else None
    qs=load_questions()
    if filter_repos:
        qs=[q for q in qs if q["repo"] in filter_repos]
    print(f"Profile: {args.profile}, {len(qs)} questions", flush=True)
    # handle profile ignores like run.py (reuse same logic)
    import subprocess, time
    # We rely on run.py's profile handling having been done externally; for diagnostics we just ensure status is official vs smoke by checking .ignore
    # For now just report what profile we think we are in by checking .ignore existence
    for repo in filter_repos or []:
        rp=ensure_repo(repo)
        ignore=rp/".ignore"
        is_smoke=ignore.exists() and "bench smoke profile" in ignore.read_text(encoding="utf-8", errors="ignore")[:500]
        print(f"{repo}: .ignore smoke={is_smoke}", flush=True)

    adapter=ContextEngineAdapter()
    # Also need rg for comparison
    rg=RgBaselineAdapter()
    results=[]
    for q in qs:
        repo_path=ensure_repo(q["repo"])
        query=q["query"]
        print(f"\n=== {q['id']} ({q['repo']}/{q['category']}) ===", flush=True)
        print(f"query: {query}", flush=True)
        print(f"expected: {q['expected_files']} symbols={q.get('expected_symbols')}", flush=True)
        # context_engine
        ce_res=adapter.search(query, repo_path, top_n=5)
        raw=ce_res.raw or {}
        dbg=raw.get("debug") or {}
        print(f"classification: {dbg.get('classification')} hints={dbg.get('hints')}", flush=True)
        print(f"identifiers: {dbg.get('identifiers')}", flush=True)
        print(f"exact_queries: {dbg.get('exact_queries')}", flush=True)
        print(f"symbol_queries: {dbg.get('symbol_queries')}", flush=True)
        print(f"graph_queries: {dbg.get('graph_queries')}", flush=True)
        print(f"test_queries: {dbg.get('test_queries')}", flush=True)
        print(f"semantic_queries: {dbg.get('semantic_queries')}", flush=True)
        print(f"retrievers: {ce_res.retrievers_used}", flush=True)
        print(f"stats: cand={ce_res.candidate_count} ev={ce_res.evidence_count} files={ce_res.files_returned} packed={ce_res.packed_tokens} elapsed={ce_res.elapsed_ms} exact={ce_res.exact_ms} struct={ce_res.structural_ms} bm25={ce_res.bm25_ms} sem={ce_res.semantic_ms} rank={ce_res.rank_ms} pack={ce_res.pack_ms}", flush=True)
        print(f"top10 unique files (dedup):", flush=True)
        seen=set()
        uniq=[]
        for h in ce_res.hits:
            f=h.file.lower()
            if f not in seen:
                seen.add(f)
                uniq.append(h)
        # if raw evidence has more than hits (hits are only top_n), we should show raw evidence list for top10
        raw_ev=raw.get("evidence") or []
        # Show up to 10 raw evidence (ranked)
        for i,ev in enumerate(raw_ev[:10], start=1):
            print(f"  {i}. {ev.get('file')} [{ev.get('source')} {ev.get('relation')}] {ev.get('symbol')} score={ev.get('score')} auth={ev.get('authorityScore')} final={ev.get('finalScore')}", flush=True)
        # Also show hits
        print(f"hits ({len(ce_res.hits)}):", flush=True)
        for i,h in enumerate(ce_res.hits, start=1):
            print(f"  {i}. {h.file} prov={h.provenance} score={h.score}", flush=True)
        # Check if expected found
        exp_norm=[e.lower().replace("\\","/") for e in q["expected_files"]]
        def matches(hit_file, exp):
            lf=hit_file.lower()
            e=exp.lower()
            return lf==e or lf.endswith("/"+e) or e.endswith("/"+lf) or lf.endswith(e)
        # deduped unique files list
        uniq_files=[h.file.lower().replace("\\","/") for h in uniq]
        # find rank
        rank=None
        for i,uf in enumerate(uniq_files, start=1):
            for e in exp_norm:
                if matches(uf, e):
                    rank=i
                    break
            if rank:
                break
        print(f"expected found rank: {rank} (Hit@1={1 if rank==1 else 0} Hit@5={1 if rank and rank<=5 else 0})", flush=True)
        # Now check structural definitions availability via direct query for symbol queries
        # For each symbol query, check find_definitions count via contextd status? We can call find_definitions via adapter? Not directly, but we can check via raw debug if symbol_queries empty
        # Also check rg baseline for same query
        rg_res=rg.search(query, repo_path, top_n=5)
        print(f"rg hits: {[h.file for h in rg_res.hits]}", flush=True)
        rg_rank=None
        rg_uniq=[]
        seen=set()
        for h in rg_res.hits:
            f=h.file.lower()
            if f not in seen:
                seen.add(f)
                rg_uniq.append(h.file.lower().replace("\\","/"))
        for i,uf in enumerate(rg_uniq, start=1):
            for e in exp_norm:
                if matches(uf, e):
                    rg_rank=i
                    break
            if rg_rank:
                break
        print(f"rg expected rank: {rg_rank}", flush=True)
        results.append({"id":q["id"],"rank":rank,"rg_rank":rg_rank,"dbg":dbg,"ce":ce_res,"rg":rg_res})
    # Summary counts
    print("\n=== SUMMARY ===", flush=True)
    for r in results:
        print(f"{r['id']}: ce_rank={r['rank']} rg_rank={r['rg_rank']} class={r['dbg'].get('classification')}")

if __name__=="__main__":
    main()
