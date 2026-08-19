"""Codebase-Memory-MCP adapter — real CLI delegation (0.10.8)."""

from __future__ import annotations

import json
import subprocess
import time
import re
from pathlib import Path
from typing import List

from .interface import BenchmarkAdapter, IndexingMetrics, SearchHit, SearchResult

BIN = Path(r"C:\Temp\cbm\codebase-memory-mcp.exe")
REPO_ROOT = Path(__file__).resolve().parents[2]

try:
    import tiktoken
    _enc=tiktoken.get_encoding("cl100k_base")
    def _tok(s: str) -> int: return len(_enc.encode(s or ""))
except:
    def _tok(s: str) -> int: return len((s or "").split())

def _run(args: List[str], timeout=120):
    proc=subprocess.run([str(BIN)]+args, capture_output=True, text=True, encoding="utf-8", errors="ignore", timeout=timeout)
    out=(proc.stdout or "") + (proc.stderr or "")
    rc=proc.returncode
    if rc!=0:
        return {"raw": out, "returncode": rc, "error": f"non-zero exit {rc}: {out[:500]}", "stderr": proc.stderr or ""}
    try:
        idx=out.rfind('{"content"')
        if idx!=-1:
            for line in reversed(out.splitlines()):
                line=line.strip()
                if line.startswith("{") and '"content"' in line:
                    j=json.loads(line)
                    j["returncode"]=rc
                    return j
            j=json.loads(out[out.find("{"):])
            j["returncode"]=rc
            return j
        # if no json wrapper, try to parse out as json
        try:
            j=json.loads(out[out.find("{"):])
            j["returncode"]=rc
            return j
        except:
            return {"raw": out, "returncode": rc}
    except Exception as e:
        return {"raw": out, "returncode": rc, "error": str(e)}

def _project_name(repo_path: Path) -> str:
    p=str(repo_path.resolve()).replace("\\","-").replace(":","").replace("/","-").replace(" ","-")
    return p.replace("--","-")

class CodebaseMemoryAdapter(BenchmarkAdapter):
    name="codebase_memory"
    def index(self, repo_path: Path) -> IndexingMetrics:
        t0=time.perf_counter()
        try:
            res=_run(["cli","--json","index_repository","--repo_path",str(repo_path)], timeout=120)
            wall=int((time.perf_counter()-t0)*1000)
            # validate
            if res.get("returncode",0)!=0 or "error" in res and "non-zero" in str(res.get("error","")):
                return IndexingMetrics(initial_wall_ms=wall, unavailable=[f"cbm_index_failed:rc={res.get('returncode')}"[:100], "bm25_docs","vector_count","index_disk_bytes","cpu_ms","peak_rss_mb","no_change_wall_ms","one_file_wall_ms"])
            # also validate content indicates success
            txt=""
            try:
                txt=res.get("content",[{}])[0].get("text","") if "content" in res else ""
            except: txt=""
            if "error" in txt.lower() and "failed" in txt.lower():
                return IndexingMetrics(initial_wall_ms=wall, unavailable=[f"cbm_index_error:{txt[:80]}"[:100], "bm25_docs","vector_count","index_disk_bytes","cpu_ms","peak_rss_mb","no_change_wall_ms","one_file_wall_ms"])
            nodes=None
            try:
                inner=json.loads(txt) if txt.strip().startswith("{") else {}
                nodes=inner.get("nodes")
            except: pass
            # if nodes is None and res has no usable content, treat as unavailable
            unavailable=["bm25_docs","vector_count","index_disk_bytes","cpu_ms","peak_rss_mb","no_change_wall_ms","one_file_wall_ms"]
            if res.get("returncode",0)!=0:
                unavailable.insert(0, f"cbm_index_failed")
            return IndexingMetrics(initial_wall_ms=wall, files_indexed=None, symbols=nodes, bm25_docs=None, vector_count=None, index_disk_bytes=None, unavailable=unavailable)
        except Exception as e:
            wall=int((time.perf_counter()-t0)*1000)
            return IndexingMetrics(initial_wall_ms=wall, unavailable=[f"cbm_error:{e}"[:100]])

    def search(self, query: str, repo_path: Path, top_n: int=5) -> SearchResult:
        t0=time.perf_counter()
        proj=_project_name(repo_path)
        try:
            res=_run(["cli","--json","search_graph","--project",proj,"--query",query], timeout=60)
            wall=int((time.perf_counter()-t0)*1000)
            # res content text contains table: total: 54 ... results: 50 (cols: qn label file lines rank)
            txt=""
            try:
                txt=res["content"][0]["text"] if "content" in res else str(res.get("raw",""))
            except:
                txt=str(res)
            # parse lines like: <symbol> <kind> <file> <lines> <score>
            hits=[]
            for line in txt.splitlines():
                m=re.search(r"(\S+\.\w+)\s+(\d+)-(\d+)\s+(-?[0-9.]+)", line)
                if m:
                    f=m.group(1)
                    f=f.replace("\\","/")
                    try: l=int(m.group(2))
                    except: l=None
                    try: score=float(m.group(4))
                    except: score=None
                    hits.append(SearchHit(file=f, score=score, line=l, text=line[:400], provenance="cbm:search_graph"))
                    if len(hits)>=top_n: break
            # generic fallback: extract file-like paths and verify under repo
            if not hits:
                seen=set()
                cands=[]
                for cand in re.findall(r"[\w./-]+\.\w+", txt):
                    norm=cand.replace("\\","/")
                    if len(norm)>120: continue
                    if norm not in seen:
                        seen.add(norm)
                        cands.append(norm)
                cands=sorted(set(cands))
                for cand in cands:
                    if len(hits)>=top_n: break
                    # verify candidate maps to existing repo-relative file
                    try:
                        # normalize to repo-relative POSIX
                        cand_norm=cand.replace("\\","/").lstrip("./")
                        # check if file exists under repo_path
                        if (repo_path / cand_norm).is_file():
                            rel=cand_norm
                        else:
                            # try basename search
                            base=Path(cand_norm).name
                            found=None
                            for p in repo_path.rglob(base):
                                if p.is_file() and p.name.lower()==base.lower():
                                    rel=p.relative_to(repo_path).as_posix()
                                    found=rel
                                    break
                            if found is None:
                                continue
                            rel=found
                    except: continue
                    hits.append(SearchHit(file=rel, score=None, text=txt[:400], provenance="cbm:fallback"))
            common=_tok(" ".join(h.text or "" for h in hits)) if hits else 0
            return SearchResult(query=query, hits=hits[:top_n], candidate_count=len(hits), evidence_count=len(hits), files_returned=len(set(h.file for h in hits)), candidate_tokens=None, packed_tokens=common, retrievers_used=[f"cbm:search_graph:{len(hits)}"], elapsed_ms=wall, wall_ms=wall, internal_ms=wall, raw={"text":txt[:2000]})
        except Exception as e:
            wall=int((time.perf_counter()-t0)*1000)
            return SearchResult(query=query, hits=[], candidate_count=0, evidence_count=0, files_returned=0, retrievers_used=[f"cbm:error:{e}"], elapsed_ms=wall, raw={"error":str(e)})
