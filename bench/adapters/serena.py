"""Serena adapter — real MCP delegation (serena-agent 1.7.0, LSP)."""

from __future__ import annotations

import json
import subprocess
import time
import threading
import queue
import re
from pathlib import Path
from typing import Dict, List

from .interface import BenchmarkAdapter, IndexingMetrics, SearchHit, SearchResult

REPO_ROOT = Path(__file__).resolve().parents[2]

try:
    import tiktoken
    _enc=tiktoken.get_encoding("cl100k_base")
    def _tok(s: str) -> int: return len(_enc.encode(s or ""))
except:
    def _tok(s: str) -> int: return len((s or "").split())

def _extract_term(query: str) -> str:
    tokens=re.findall(r"[A-Za-z_][A-Za-z0-9_]*", query)
    stops={"where","what","who","how","is","are","for","the","and","or","test","tests","cover","covers","implemented","implementation","find","does","cover","generation"}
    cands=[t for t in tokens if t.lower() not in stops and len(t)>=3]
    if cands:
        cands.sort(key=lambda x: (0 if "_" in x else 1, -len(x)))
        return cands[0]
    for w in query.split():
        if len(w.strip('",.?:'))>=3:
            return w.strip('",.?:')
    return query.split()[0] if query else "test"

class _SerenaClient:
    def __init__(self, repo_path: Path):
        self.repo_path=repo_path
        self.proc=subprocess.Popen(
            ["serena-agent","start-mcp-server","--project",str(repo_path),"--transport","stdio","--enable-web-dashboard","False","--enable-gui-log-window","False","--open-web-dashboard","False"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", errors="ignore", bufsize=1
        )
        self._next=1
        self._pending: Dict[int, queue.Queue]={}
        self._lock=threading.Lock()
        threading.Thread(target=self._reader, daemon=True).start()
        threading.Thread(target=self._err, daemon=True).start()
        self._initialize()
        # warm: wait for LSP ready (already indexed)
        time.sleep(1)
        self.pid=self.proc.pid

    def _reader(self):
        for line in iter(self.proc.stdout.readline,""):
            if not line: break
            line=line.strip()
            if not line: continue
            try: msg=json.loads(line)
            except: continue
            if "id" in msg:
                with self._lock:
                    q=self._pending.get(msg["id"])
                if q: q.put(msg)

    def _err(self):
        for line in iter(self.proc.stderr.readline,""):
            if not line: break
            pass

    def _send(self, obj):
        self.proc.stdin.write(json.dumps(obj)+"\n"); self.proc.stdin.flush()

    def _req(self, method, params=None, timeout=120):
        mid=self._next; self._next+=1
        q=queue.Queue()
        with self._lock: self._pending[mid]=q
        req={"jsonrpc":"2.0","id":mid,"method":method}
        if params is not None: req["params"]=params
        self._send(req)
        try: resp=q.get(timeout=timeout)
        except queue.Empty: raise TimeoutError(method)
        finally:
            with self._lock: self._pending.pop(mid,None)
        if "error" in resp: raise RuntimeError(resp["error"])
        return resp.get("result",resp)

    def _notify(self, method, params=None):
        obj={"jsonrpc":"2.0","method":method}
        if params is not None: obj["params"]=params
        self._send(obj)

    def _initialize(self):
        self._req("initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench-serena","version":"0.1"}}, timeout=15)
        self._notify("notifications/initialized", {})
        time.sleep(1)
        # ensure project activated (auto via --project)
        time.sleep(2)

    def call(self, name, args, timeout=90):
        r=self._req("tools/call", {"name":name,"arguments":args}, timeout=timeout)
        content=r.get("content") or []
        text="".join(c.get("text","")+"\n" for c in content if isinstance(c,dict)).strip()
        return text, r

    def close(self):
        try:
            if self.proc.stdin: self.proc.stdin.close()
        except: pass
        try: self.proc.terminate(); self.proc.wait(timeout=2)
        except: pass
        try: self.proc.kill()
        except: pass

class SerenaAdapter(BenchmarkAdapter):
    name="serena"
    def __init__(self):
        self._clients: Dict[str, _SerenaClient]={}

    def _client(self, repo_path: Path) -> _SerenaClient:
        key=str(repo_path.resolve())
        if key not in self._clients:
            self._clients[key]=_SerenaClient(repo_path)
        return self._clients[key]

    def index(self, repo_path: Path) -> IndexingMetrics:
        t0=time.perf_counter()
        # project already created and indexed via serena-agent project create --index
        # we just ensure client exists and is ready
        try:
            c=self._client(repo_path)
            wall=int((time.perf_counter()-t0)*1000)
            # count files via rglob for metrics
            cnt=sum(1 for p in repo_path.rglob("*") if p.is_file())
            return IndexingMetrics(initial_wall_ms=wall, files_indexed=cnt, symbols=None, bm25_docs=None, vector_count=None, unavailable=["bm25_docs","vector_count","index_disk_bytes","cpu_ms","peak_rss_mb","no_change_wall_ms","one_file_wall_ms"])
        except Exception as e:
            wall=int((time.perf_counter()-t0)*1000)
            return IndexingMetrics(initial_wall_ms=wall, unavailable=[f"serena_error:{e}"[:100]])

    def search(self, query: str, repo_path: Path, top_n: int=5) -> SearchResult:
        t0=time.perf_counter()
        term=_extract_term(query)
        # category hint: if query contains "test" or "caller" we map differently, but use generic find_symbol for definition/caller/conceptual, and search_for_pattern for exact
        # For exact file queries like "Find global_settings.py", use find_file
        # For test queries like "What tests cover Q objects?", use find_symbol for Q
        try:
            c=self._client(repo_path)
            hits=[]
            # try find_symbol first (good for definition)
            try:
                text,_=c.call("find_symbol", {"name_path":term,"depth":0,"include_body":False}, timeout=60)
                # text is json list of symbols
                # try to parse as json
                try:
                    data=json.loads(text)
                    if isinstance(data, list):
                        for entry in data:
                            if not isinstance(entry, dict): continue
                            rel=entry.get("relative_path","")
                            rel=rel.replace("\\","/")
                            score=None
                            # rank by exact match: if name_path == term, boost
                            name_path=entry.get("name_path","")
                            if name_path==term:
                                score=1.0
                            elif name_path.endswith("/"+term) or term in name_path:
                                score=0.9
                            else:
                                score=0.5
                            hits.append((rel, score, entry.get("body_location",{}).get("start_line"), text[:200]))
                except:
                    # text is not json, maybe plain
                    pass
            except Exception as e:
                pass
            # if no hits or not enough, try search_for_pattern (substring)
            if len(hits)<top_n:
                try:
                    text2,_=c.call("search_for_pattern", {"substring_pattern":term}, timeout=60)
                    # text2 is like "Found 10 occurrences in 5 files:\n- django/db/models/base.py:508: class Model..."
                    for line in text2.splitlines():
                        m=re.search(r"^\s*-\s*(\S+):(\d+):", line)
                        if m:
                            f=m.group(1).replace("\\","/")
                            try: l=int(m.group(2))
                            except: l=None
                            if f not in [h[0] for h in hits]:
                                hits.append((f, 0.5, l, line[:200]))
                            if len(hits)>=top_n*2: break
                except: pass
            # if still no hits, try find_file for exact queries
            if len(hits)<top_n and ("." in query or "Find " in query):
                try:
                    # extract file name pattern
                    for cand in re.findall(r"[\w./-]+\.\w+", query):
                        base=Path(cand.strip('",.?:')).name
                        text3,_=c.call("find_file", {"file_mask":base}, timeout=30)
                        for line in text3.splitlines():
                            if base.lower() in line.lower():
                                # find file path in line
                                m=re.search(r"(\S+\.\w+)", line)
                                if m:
                                    f=m.group(1).replace("\\","/")
                                    hits.append((f, 0.8, 1, line[:200]))
                                    break
                        if hits: break
                except: pass
            # dedup and rank
            # For definition queries, prefer hits where relative_path matches expected definition file (we don't have ground truth here, so rank by score)
            # Sort by score desc, then by hits order
            hits_sorted=sorted(hits, key=lambda x: - (x[1] or 0))
            # dedup per file
            seen=set()
            out=[]
            for f,score,line,txt in hits_sorted:
                if f not in seen:
                    seen.add(f)
                    out.append(SearchHit(file=f, score=score, line=line, text=txt, provenance="serena:find_symbol"))
                if len(out)>=top_n: break
            wall=int((time.perf_counter()-t0)*1000)
            common=_tok(" ".join(h.text or "" for h in out)) if out else 0
            # Determine unsupported: if no hits, mark as not found but still return empty (will be counted as miss)
            return SearchResult(query=query, hits=out[:top_n], candidate_count=len(hits), evidence_count=len(out), files_returned=len(set(h.file for h in out)), candidate_tokens=None, packed_tokens=common, retrievers_used=[f"serena:find_symbol:{len(hits)}"], elapsed_ms=wall, wall_ms=wall, internal_ms=wall, raw={"query":query,"term":term,"hits_raw":hits[:10]})
        except Exception as e:
            wall=int((time.perf_counter()-t0)*1000)
            return SearchResult(query=query, hits=[], candidate_count=0, evidence_count=0, files_returned=0, retrievers_used=[f"serena:error:{e}"], elapsed_ms=wall, raw={"error":str(e)})

    def close(self):
        for c in list(self._clients.values()):
            try: c.close()
            except: pass
        self._clients.clear()
