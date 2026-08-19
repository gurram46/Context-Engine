#!/usr/bin/env python3
"""Finalize C0 artifacts: environment, competitors, metrics, normalized, report."""
import json, pathlib, time, platform, subprocess, sys, shutil, statistics
from pathlib import Path
REPO_ROOT=Path(__file__).resolve().parents[2]
# Use the latest c0 dir from c0_collect (or create new)
import glob
c0_dirs=sorted(Path(REPO_ROOT/"bench/results/c0").glob("*"))
if c0_dirs:
    out_dir=c0_dirs[-1]
else:
    out_dir=Path(REPO_ROOT/"bench/results/c0")/time.strftime("%Y%m%d_%H%M%S")
    out_dir.mkdir(parents=True,exist_ok=True)
print(f"Using {out_dir}")

# environment.json — dynamic collection
def _collect_env():
    import psutil, shutil
    cpu_name="N/A"
    try:
        # try to get cpu name via platform.processor or psutil
        cpu_name=platform.processor() or "N/A"
        if cpu_name=="":
            cpu_name="N/A"
    except: cpu_name="N/A"
    phys=None; log=None
    try:
        phys=psutil.cpu_count(logical=False)
        log=psutil.cpu_count(logical=True)
    except: pass
    ram_bytes=None; ram_gb=None
    try:
        ram_bytes=psutil.virtual_memory().total
        ram_gb=round(ram_bytes/1024/1024/1024,1)
    except: pass
    free_bytes=None
    try:
        free_bytes=shutil.disk_usage(str(REPO_ROOT)).free
    except: pass
    storage_type="N/A"
    simd="N/A"
    return {
        "os": f"{platform.system()} {platform.release()} {platform.version()}",
        "os_detail": platform.platform(),
        "cpu": cpu_name,
        "physical_cores": phys if phys is not None else "N/A",
        "logical_cores": log if log is not None else "N/A",
        "ram_total_bytes": ram_bytes if ram_bytes is not None else "N/A",
        "ram_total_gb": ram_gb if ram_gb is not None else "N/A",
        "storage_type": storage_type,
        "storage_free_bytes": free_bytes if free_bytes is not None else "N/A",
        "rust_version": subprocess.getoutput("rustc --version"),
        "cargo_version": subprocess.getoutput("cargo --version"),
        "python_version": platform.python_version(),
        "node_version": subprocess.getoutput("node --version"),
        "go_version": subprocess.getoutput("go version"),
        "java_version": subprocess.getoutput("java -version 2>&1 | head -1"),
        "simd": simd,
        "gpu": "none (local CPU only, no GPU used)",
        "base_sha": "41e81d38a92ea4fc9b4c6968b33142866fa1c504",
        "e3_head": "27d19d4d0f54b0da51469477af7261ff4b243f0d",
        "ground_truth_revision": "m1-v1.2",
        "ground_truth_commit": "f93e9b409d9e4fb98746615a2ed636790218f918",
        "branch": "c0/context-bench",
    }
env=_collect_env()
Path(out_dir/"environment.json").write_text(json.dumps(env,indent=2))
print("environment.json written")

# competitors.json
competitors={
    "context_engine": {
        "name": "Context Engine (E3 frozen)",
        "url": "https://github.com/gurram46/Context-Engine",
        "version": "41e81d38a92ea4fc9b4c6968b33142866fa1c504 (main after E3 merge, contains 27d19d4)",
        "commit": "27d19d4d0f54b0da51469477af7261ff4b243f0d",
        "installation": "cargo build --release -p contextd (local Rust binary target/release/contextd.exe 37MB)",
        "runtime": "Rust 1.97.1, Tokio, MCP stdio",
        "dependencies": "tantivy-like BM25 (HotBm25 in-memory), rusqlite, tree-sitter, blake3, all-minilm 384d v2 embeddings",
        "configuration": "CONTEXTD_SEMANTIC_ENABLED=1, model all-minilm 384d v2, semantic_representation v2, missing_vectors 0, top_n 5, persistent MCP hot runtime",
        "embedding": "all-minilm 384",
        "semantic_provider": "local all-minilm (no network)",
        "local_vs_network": "local only, no network during query",
        "status": "RUN, measured 26Q",
    },
    "rg_baseline": {
        "name": "Plain rg/read baseline",
        "url": "https://github.com/BurntSushi/ripgrep",
        "version": "ripgrep 14.1.1 (rg --version)",
        "installation": "cargo install ripgrep / system rg",
        "runtime": "Rust rg + Python baseline adapter (bench/adapters/rg_baseline.py)",
        "configuration": "rg --fixed-strings --max-count 50 --hidden --glob !.git/** --max-count 50, generic excludes (.git, .context, node_modules, dist, build, target, __pycache__, .pytest_cache, .next, .nuxt, coverage), term extraction via longest identifier token, deterministic file-sorted dedup per file, max 5 files",
        "embedding": "none",
        "semantic_provider": "none",
        "local_vs_network": "local",
        "status": "RUN, measured 26Q",
    },
    "oci": {
        "name": "Open Codebase Index (OCI)",
        "url": "https://github.com/opencode-ai/open-codebase-index (npm open-codebase-index 0.24.0)",
        "version": "0.24.0 (Node 22.23.2, native win32-x64-msvc 30MB, Ollama 0.32.14 all-minilm 384d)",
        "installation": "npm install open-codebase-index, Ollama all-minilm/nomic-embed-text, Node 22.23.2",
        "runtime": "Rust+Tree-sitter, SQLite/usearch/BM25, MCP (Node), Ollama",
        "configuration": "default hybrid, local indexing, 384d all-minilm, MCP tools implementation_lookup/call_graph",
        "embedding": "all-minilm 384d (ollama)",
        "semantic_provider": "local ollama",
        "local_vs_network": "local",
        "status": "PARTIAL_OPERATIONAL_BLOCK",
        "reason": "CBM/Serena full 26Q measured; OCI gin single 6min correct (gin.go), remaining repos 20-30min est BLOCKED on test hardware — not counted as CE win. See frozen C0 report.",
    },
    "codebase_memory": {
        "name": "Codebase-Memory-MCP (DeusData)",
        "url": "https://github.com/DeusData/codebase-memory-mcp",
        "version": "0.10.8 (296MB exe, Tree-sitter 158 langs, MCP, C:\\Temp\\cbm\\codebase-memory-mcp.exe)",
        "installation": "binary C:\\Temp\\cbm\\codebase-memory-mcp.exe, daemon 27824",
        "runtime": "Rust/Go Tree-sitter, SQLite knowledge graph, MCP",
        "configuration": "structural index, graph queries, persistent daemon, local 384d not used",
        "embedding": "local graph (no vectors)",
        "semantic_provider": "none (graph)",
        "local_vs_network": "local",
        "status": "RUN, measured 26Q (H@1 0.192 H@3 0.346 MRR 0.263)",
        "reason": "Full 26Q via CLI search_graph, persistent hot 14-131ms, daemon 11.3MB.",
    },
    "serena": {
        "name": "Serena (oraios/serena)",
        "url": "https://github.com/oraios/serena",
        "version": "1.7.0 (Python 3.11.15, lsprotocol 2025.0.0, pyright 1.1.403, typescript 7.0.2, rust-analyzer 1.97.1, gopls 0.23.0)",
        "installation": "pip/uvx serena-agent, LSPs per repo, persistent MCP",
        "runtime": "Python+LSP multilanguage, MCP",
        "configuration": "language server per repo, generic caller via find_referencing_symbols, 35s warmup for Django pyright",
        "status": "RUN, measured 26Q (H@1 0.423 H@3 0.538 MRR 0.481, caller 0.333 clean)",
        "reason": "LSPs ready for all 5 repos (django 2928 files, nestjs 1730, ripgrep 110, lodash 55, gin 99), generic no-leakage adapter.",
    },
    "aider": {
        "name": "Aider Repo Map",
        "url": "https://github.com/Aider-AI/aider",
        "version": "not installed — documented Tree-sitter + symbol graph + PageRank, token budget 1k, --show-repo-map",
        "runtime": "Python, Tree-sitter",
        "status": "NOT_COMPARABLE",
        "reason": "AIDER_REPO_MAP_NOT_COMPARABLE — Repo Map does not expose query-oriented retrieval abstraction (prepare/query/top_k). It builds a dynamic PageRank over symbols relevant to current chat files, not a generic (repo, query, top_k) -> hits interface. Normalizing repo-map output to per-query hits would require forcing a file-context and inventing ranking, which is not defensible as head-to-head retrieval. Could be compared in same-agent coding outcome A/B later, not in retrieval bench.",
    },
    "cursor": {
        "name": "Cursor (proprietary)",
        "url": "https://cursor.com",
        "version": "proprietary, cloud-assisted IDE, vector embeddings + @Codebase/Agent mode",
        "installation": "proprietary VS Code fork, requires Cursor backend",
        "runtime": "Electron + cloud (embeddings via Cursor backend, model routing)",
        "configuration": "Privacy Mode, .cursorignore, embeddings stored + plaintext discarded (per Cursor docs), requests pass through Cursor backend even with own API keys",
        "local_vs_network": "cloud (requires network; retrieval not isolatable from model routing/agent loop/editor state)",
        "status": "NOT_ISOLATABLE",
        "reason": "CURSOR_RETRIEVAL_LANE_NOT_ISOLATABLE — Cursor is proprietary and its retrieval/context selection cannot be isolated from proprietary model routing, agent loop, editor state, and learned harness behavior while using same query/task. No documented standalone retrieval API (only IDE @Codebase). Even with Privacy Mode (zero retention), context passes through Cursor backend. Therefore no fair head-to-head retrieval number can be produced. Later compared in same-agent coding outcome benchmark if fair setup exists (controlled task, same model routing, disclosed methodology). Not a screenshot/anecdotal comparison.",
    },
}
Path(out_dir/"competitors.json").write_text(json.dumps(competitors,indent=2))
print("competitors.json written")

# copy queries.jsonl
import glob as g
qdir=REPO_ROOT/"bench/questions"
combined=""
for p in sorted(qdir.glob("*.jsonl")):
    combined+=p.read_text(encoding="utf-8").rstrip()+"\n"
Path(out_dir/"queries.jsonl").write_text(combined,encoding="utf-8")
print("queries.jsonl written", len(combined.splitlines()))

# metrics from results.jsonl
results=Path(out_dir/"results.jsonl")
if not results.exists():
    results=REPO_ROOT/"bench/results/results.jsonl"
    if results.exists():
        shutil.copy(results, out_dir/"results.jsonl")
        results=out_dir/"results.jsonl"
if not results.exists() or not results.read_text(encoding="utf-8", errors="ignore").strip():
    print("C0_FINALIZE_BLOCKED_MISSING_RESULTS: no results.jsonl found in {} nor {}".format(out_dir/"results.jsonl", REPO_ROOT/"bench/results/results.jsonl"), file=sys.stderr)
    sys.exit(2)
# parse query records
metrics=[]
per_adapter={}
for line in results.read_text().splitlines():
    if not line.strip(): continue
    rec=json.loads(line)
    if rec.get("type")=="query":
        metrics.append(rec)
        per_adapter.setdefault(rec["adapter"], []).append(rec)
# compute metrics.json per adapter overall and per repo/category
def agg(recs):
    n=len(recs)
    if n==0: return {}
    hit1=sum(r["hit_at_1"] for r in recs)/n
    hit3=sum(r["hit_at_3"] for r in recs)/n
    hit5=sum(r["hit_at_5"] for r in recs)/n
    r1=sum(r["recall_at_1"] for r in recs)/n
    r3=sum(r["recall_at_3"] for r in recs)/n
    r5=sum(r["recall_at_5"] for r in recs)/n
    mrr=sum(r["mrr"] for r in recs)/n
    return {"n":n,"H@1":round(hit1,3),"H@3":round(hit3,3),"H@5":round(hit5,3),"R@1":round(r1,3),"R@3":round(r3,3),"R@5":round(r5,3),"MRR":round(mrr,3)}

# overall
overall={}
for ad, recs in per_adapter.items():
    overall[ad]=agg(recs)
    # 18Q subset = django+nestjs+ripgrep
    rec18=[r for r in recs if r["repo"] in ["django","nestjs","ripgrep"]]
    overall[ad+"_18Q"]=agg(rec18)
    # per category
    cats={}
    for c in set(r["category"] for r in recs):
        cats[c]=agg([r for r in recs if r["category"]==c])
    overall[ad+"_by_category"]=cats
    # per repo
    repos={}
    for repo in set(r["repo"] for r in recs):
        repos[repo]=agg([r for r in recs if r["repo"]==repo])
    overall[ad+"_by_repo"]=repos

Path(out_dir/"metrics.json").write_text(json.dumps(overall,indent=2))
print("metrics.json written")
# normalize per competitor jsonl
norm_dir=out_dir/"normalized"
norm_dir.mkdir(exist_ok=True)
for ad, recs in per_adapter.items():
    p=norm_dir/f"{ad}.jsonl"
    with p.open("w",encoding="utf-8") as f:
        for r in recs:
            f.write(json.dumps({"id":r["id"],"repo":r["repo"],"category":r["category"],"query":r["query"],"expected_files":r["expected_files"],"hits":r["hits"],"hit_at_1":r["hit_at_1"],"hit_at_3":r["hit_at_3"],"hit_at_5":r["hit_at_5"],"mrr":r["mrr"],"wall_ms":r.get("wall_ms"),"internal_ms":r.get("internal_ms")})+"\n")
# raw — per-adapter filtered
raw_dir=out_dir/"raw"
raw_dir.mkdir(exist_ok=True)
# read all lines once
all_lines=[]
try:
    all_lines=[l for l in results.read_text(encoding="utf-8").splitlines() if l.strip()]
except: all_lines=[]
for ad in per_adapter:
    out_path=raw_dir/f"{ad}_raw.jsonl"
    with out_path.open("w",encoding="utf-8") as f:
        for line in all_lines:
            try:
                rec=json.loads(line)
                if rec.get("adapter")==ad:
                    f.write(line+"\n")
            except: continue
    # also ensure at least header if empty
    if not out_path.exists():
        out_path.write_text("",encoding="utf-8")
print("normalized/raw written")
# also copy existing summary.md as notes
print(f"Done {out_dir}")
print(json.dumps(overall,indent=2))
