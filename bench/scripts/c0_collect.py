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
try:
    from adapters.codebase_memory import CodebaseMemoryAdapter
except: CodebaseMemoryAdapter=None
try:
    from adapters.serena import SerenaAdapter
except: SerenaAdapter=None
try:
    from adapters.oci import OciAdapter
except: OciAdapter=None

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

def _rss_for_pid(pid):
    try:
        import psutil
        if pid is None:
            return None
        proc=psutil.Process(int(pid))
        rss=proc.memory_info().rss
        # include children
        try:
            for child in proc.children(recursive=True):
                try: rss+=child.memory_info().rss
                except: pass
        except: pass
        return int(rss/1024/1024)
    except: return None

def measure_resource(repo_path, adapter=None):
    idx = repo_path / ".context" / "index"
    disk=None
    if idx.exists():
        total=0
        for p in idx.rglob("*"):
            if p.is_file():
                try: total+=p.stat().st_size
                except: pass
        disk=total
    controller_rss=None
    sut_rss=None
    auxiliary_rss=None
    try:
        import psutil
        controller_rss=int(psutil.Process().memory_info().rss/1024/1024)
    except: pass
    # try to get SUT pid from adapter where available
    try:
        pid=None
        if adapter is not None:
            # generic: adapter may expose resource_processes or _clients
            if hasattr(adapter, "resource_processes"):
                try: procs=adapter.resource_processes(repo_path)
                except: procs=None
                if procs:
                    # procs is dict or list of pids
                    if isinstance(procs, dict):
                        sut_rss=procs.get("sut_rss_mb")
                        auxiliary_rss=procs.get("auxiliary_rss_mb")
                        controller_rss=procs.get("controller_rss_mb", controller_rss)
                        pid=None
                    elif isinstance(procs, list):
                        # first is sut
                        pid=procs[0] if procs else None
            if pid is None and hasattr(adapter, "_clients"):
                try:
                    key=str(repo_path.resolve())
                    c=adapter._clients.get(key)
                    if c is not None:
                        pid=getattr(c, "contextd_pid", None) or getattr(c, "os_pid", None) or getattr(c, "pid", None) or getattr(c, "proc", None) and getattr(c.proc, "pid", None)
                except: pass
            if pid is None and hasattr(adapter, "_client"):
                try:
                    # for serena adapter with _clients
                    key=str(repo_path.resolve())
                    if hasattr(adapter, "_clients"):
                        c=adapter._clients.get(key)
                        if c is not None:
                            pid=getattr(c, "pid", None) or getattr(c, "proc", None) and getattr(c.proc, "pid", None)
                except: pass
            if sut_rss is None and pid is not None:
                sut_rss=_rss_for_pid(pid)
                # auxiliary: try children already included; for serena try to sum LSP children via psutil children already
                # for OCI also try to get Node+Ollama separate if adapter provides
                # we keep auxiliary as None for now, sut includes children
        if sut_rss is None and auxiliary_rss is None:
            # fallback: if no sut, keep controller only
            pass
    except: pass
    return {"disk": disk, "controller_rss_mb": controller_rss, "sut_rss_mb": sut_rss, "auxiliary_rss_mb": auxiliary_rss, "rss": controller_rss}

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

    # build adapter registry with availability
    registry=[]
    for name, Cls in [("context_engine_hot", ContextEngineHotAdapter), ("rg_baseline", RgBaselineAdapter), ("codebase_memory", CodebaseMemoryAdapter), ("serena", SerenaAdapter), ("oci", OciAdapter)]:
        if Cls is None:
            latency_placeholder=None
            continue
        try:
            # quick availability check: try to instantiate, but for serena check binary exists
            inst=Cls()
            # for external adapters, ensure binary/server available else mark BLOCKED
            # we keep instance for later; close after check
            try: inst.close()
            except: pass
            registry.append((name, Cls))
        except Exception as e:
            # mark as blocked
            registry.append((name, None))
    # fallback to at least CE+rg if registry empty
    if not registry:
        registry=[("context_engine_hot", ContextEngineHotAdapter), ("rg_baseline", RgBaselineAdapter)]

    # hot latency
    latency={}
    for name, AdapterCls in registry:
        if AdapterCls is None:
            latency[name]={"status": "BLOCKED", "reason": "adapter not available / binary missing"}
            continue
        try:
            adapter=AdapterCls()
        except Exception as e:
            latency[name]={"status": "BLOCKED", "reason": str(e)[:200]}
            continue
        latency[name]={}
        for repo, qlist in by_repo.items():
            repo_path=REPO_ROOT/"bench/repos"/repo
            query=qlist[0]["query"]
            try:
                adapter.search(query, repo_path, top_n=5)
            except: pass
            time.sleep(0.2)
            try:
                res=hot_latency(adapter, repo_path, query, samples=11)
                latency[name][repo]=res
                print(f"[{name}] {repo} wall p50 {res['wall_p50']} p95 {res['wall_p95']} internal p50 {res['internal_p50']}")
            except Exception as e:
                latency[name][repo]={"status": "BLOCKED", "reason": str(e)[:200]}
        try: adapter.close()
        except: pass
    (out_dir/"latency.json").write_text(json.dumps(latency, indent=2))

    # indexing detailed + resource
    indexing={}
    resources={}
    for name, AdapterCls in registry:
        if AdapterCls is None:
            indexing[name]={"status": "BLOCKED"}
            resources[name]={"status": "BLOCKED"}
            continue
        try:
            adapter=AdapterCls()
        except Exception as e:
            indexing[name]={"status": "BLOCKED", "reason": str(e)[:200]}
            resources[name]={"status": "BLOCKED", "reason": str(e)[:200]}
            continue
        indexing[name]={}
        resources[name]={}
        for repo in by_repo:
            repo_path=REPO_ROOT/"bench/repos"/repo
            t0=time.perf_counter()
            try: idx=adapter.index(repo_path)
            except Exception as e: idx={"error": str(e)}
            wall=int((time.perf_counter()-t0)*1000)
            try:
                res=measure_resource(repo_path, adapter)
            except:
                res=measure_resource(repo_path, None)
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
