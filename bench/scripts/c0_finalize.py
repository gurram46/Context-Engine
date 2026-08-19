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

# environment.json
env={
    "os": f"{platform.system()} {platform.release()} {platform.version()}",
    "os_detail": platform.platform(),
    "cpu": "Intel(R) Core(TM) i5-1035G1 CPU @ 1.00GHz",
    "physical_cores": 4,
    "logical_cores": 8,
    "ram_total_bytes": 17179869184,
    "ram_total_gb": 16,
    "storage_type": "SSD",
    "storage_free_bytes": 11221987328,
    "rust_version": subprocess.getoutput("rustc --version"),
    "cargo_version": subprocess.getoutput("cargo --version"),
    "python_version": platform.python_version(),
    "node_version": subprocess.getoutput("node --version"),
    "go_version": subprocess.getoutput("go version"),
    "java_version": subprocess.getoutput("java -version 2>&1 | head -1"),
    "simd": ["sse","sse2","sse3","ssse3","cmpxchg16b","fxsr"],
    "gpu": "none (local CPU only, no GPU used)",
    "base_sha": "41e81d38a92ea4fc9b4c6968b33142866fa1c504",
    "e3_head": "27d19d4d0f54b0da51469477af7261ff4b243f0d",
    "ground_truth_revision": "m1-v1.2",
    "ground_truth_commit": "f93e9b409d9e4fb98746615a2ed636790218f918",
    "branch": "c0/context-bench",
}
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
        "url": "https://github.com/opencode-ai/open-codebase-index (npm package open-codebase-index, alias opencode-codebase-index-mcp)",
        "version": "not installed — documented version npm open-codebase-index latest at 2026-08-19 (Rust+tree-sitter, SQLite+usearch+BM25, MCP tools implementation_lookup/call_graph/codebase_context/codebase_peek)",
        "installation": "npm install open-codebase-index  (requires Node >=20, Rust for optional native)",
        "runtime": "Rust+Tree-sitter, SQLite/usearch/BM25, MCP",
        "configuration": "intended: default hybrid (embeddings+BM25+branch-aware), local indexing, file watching, content-hash reuse — not exercised in C0",
        "embedding": "unknown (not measured)",
        "semantic_provider": "local (documented)",
        "local_vs_network": "local (documented)",
        "status": "NOT_RUN (not installed, would require npm install and opencode.json plugin config, separate indexing per repo; not faked)",
        "reason": "C0 focused on CE vs rg baseline as primary publishable comparison; OCI indexing per repo would require additional setup and is deferred to follow-up C0.1. Stub adapter present (bench/adapters/oci.py) returns unavailable.",
    },
    "codebase_memory": {
        "name": "Codebase-Memory-MCP (DeusData)",
        "url": "https://github.com/DeusData/codebase-memory-mcp",
        "version": "not installed — README claims single static binary, Tree-sitter 158 languages, SQLite knowledge graph",
        "installation": "shell script from GitHub (single static binary, no external deps) — not executed in C0",
        "runtime": "Rust/Go? Tree-sitter, SQLite, MCP (Claude Code, Cursor, Zed, Codex CLI)",
        "configuration": "intended: structural index (functions/classes/call chains/routes), graph queries — not exercised",
        "local_vs_network": "local (documented)",
        "status": "NOT_RUN",
        "reason": "Same as OCI — would require binary install and ingestion per repo; not faked. Stub present.",
    },
    "serena": {
        "name": "Serena (oraios/serena)",
        "url": "https://github.com/oraios/serena",
        "version": "not installed — agent toolkit, MCP, LSP-based symbol-level retrieval, 30-40 languages",
        "installation": "pip/uvx serena-agent + language servers per repo — not installed in C0",
        "runtime": "Python + LSP (multilanguage), MCP (Claude Code/Desktop, Cursor, VS Code, Cline, Roo Code)",
        "configuration": "intended: language server per repo (Python, Rust, Go, JS/TS) — not exercised; would require LSP availability report per repo",
        "status": "NOT_RUN",
        "reason": "Requires LSP setup per repo language (pyright, rust-analyzer, gopls, tsserver) and Serena MCP server; harness stub present.",
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
# raw
raw_dir=out_dir/"raw"
raw_dir.mkdir(exist_ok=True)
for ad in per_adapter:
    shutil.copy(results, raw_dir/f"{ad}_raw.jsonl")  # simplified
print("normalized/raw written")
# also copy existing summary.md as notes
print(f"Done {out_dir}")
print(json.dumps(overall,indent=2))
