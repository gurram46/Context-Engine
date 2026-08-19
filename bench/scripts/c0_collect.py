#!/usr/bin/env python3
"""C0 collection: latency (hot 11 samples), indexing detailed, resource.

Produces JSON artifacts for C0 report.
Reuse existing adapters; do not modify production.
"""
import json, time, subprocess, shutil, statistics, os, sys, platform
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = REPO_ROOT / "bench/manifest.json"
QUESTIONS_DIR = REPO_ROOT / "bench/questions"
sys.path.insert(0, str(REPO_ROOT / "bench"))

from adapters.context_engine_hot import ContextEngineHotAdapter
from adapters.rg_baseline import RgBaselineAdapter

def hot_latency(adapter, repo_path, query, samples=11):
    # warm already done outside, do samples
    walls=[]
    internals=[]
    for _ in range(samples):
        t0=time.perf_counter()
        res=adapter.search(query, repo_path, top_n=5)
        wall=int((time.perf_counter()-t0)*1000)
        # adapter already reports wall_ms/internal_ms, but we take wall from adapter's wall_ms if available else our wall
        w = res.wall_ms if res.wall_ms is not None else wall
        i = res.internal_ms if res.internal_ms is not None else res.elapsed_ms
        walls.append(w)
        internals.append(i)
        time.sleep(0.05)
    walls_sorted=sorted(walls)
    internals_sorted=sorted(internals)
    def p50(a): return statistics.median(a)
    def p95(a):
        k=int(0.95*len(a))
        return sorted(a)[min(k,len(a)-1)]
    return {
        "walls": walls,
        "internals": internals,
        "wall_p50": p50(walls_sorted),
        "wall_p95": p95(walls_sorted),
        "wall_p95_sorted": walls_sorted,
        "internal_p50": p50(internals_sorted),
        "internal_p95": p95(internals_sorted),
    }

def measure_resource(repo_path):
    # disk: size of .context/index if exists else None
    idx = repo_path / ".context" / "index"
    disk=None
    if idx.exists():
        total=0
        for p in idx.rglob("*"):
            if p.is_file():
                try: total+=p.stat().st_size
                except: pass
        disk=total
    # RSS: try psutil, else None
    rss=None
    try:
        import psutil
        rss=int(psutil.Process().memory_info().rss/1024/1024)
    except: pass
    return {"disk": disk, "rss": rss}

def main():
    out_dir = REPO_ROOT / "bench/results/c0" / time.strftime("%Y%m%d_%H%M%S")
    out_dir.mkdir(parents=True, exist_ok=True)
    # also raw/normalized etc will be under out_dir
    # For now run collection for both adapters
    manifest=json.loads(MANIFEST.read_text())
    qs=[]
    for p in QUESTIONS_DIR.glob("*.jsonl"):
        for line in p.read_text().splitlines():
            if line.strip() and not line.strip().startswith("#"):
                qs.append(json.loads(line))
    by_repo={}
    for q in qs:
        by_repo.setdefault(q["repo"], []).append(q)

    # hot latency
    latency={}
    for name, AdapterCls in [("context_engine_hot", ContextEngineHotAdapter), ("rg_baseline", RgBaselineAdapter)]:
        adapter=AdapterCls()
        latency[name]={}
        for repo, qlist in by_repo.items():
            repo_path=REPO_ROOT/"bench/repos"/repo
            query=qlist[0]["query"]
            # ensure hot built: for context_engine_hot, do one warm query
            try:
                adapter.search(query, repo_path, top_n=5)
            except: pass
            time.sleep(0.2)
            res=hot_latency(adapter, repo_path, query, samples=11)
            latency[name][repo]=res
            print(f"[{name}] {repo} wall p50 {res['wall_p50']} p95 {res['wall_p95']} internal p50 {res['internal_p50']}")
        try: adapter.close()
        except: pass
    (out_dir/"latency.json").write_text(json.dumps(latency, indent=2))

    # indexing detailed + resource
    indexing={}
    resources={}
    for name, AdapterCls in [("context_engine_hot", ContextEngineHotAdapter), ("rg_baseline", RgBaselineAdapter)]:
        adapter=AdapterCls()
        indexing[name]={}
        resources[name]={}
        for repo in by_repo:
            repo_path=REPO_ROOT/"bench/repos"/repo
            # initial via adapter.index (already hot, but we measure)
            t0=time.perf_counter()
            try: idx=adapter.index(repo_path)
            except Exception as e: idx={"error": str(e)}
            wall=int((time.perf_counter()-t0)*1000)
            # also get resource
            res=measure_resource(repo_path)
            indexing[name][repo]={"wall_ms": wall, "metrics": idx.__dict__ if hasattr(idx,"__dict__") else idx}
            resources[name][repo]=res
        try: adapter.close()
        except: pass
    (out_dir/"indexing.json").write_text(json.dumps(indexing, indent=2))
    (out_dir/"resources.json").write_text(json.dumps(resources, indent=2))
    # also copy current results.jsonl as normalized
    src=REPO_ROOT/"bench/results/results.jsonl"
    if src.exists():
        shutil.copy(src, out_dir/"results.jsonl")
    print(f"Written to {out_dir}")

if __name__=="__main__":
    main()
