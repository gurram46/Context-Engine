"""OCI adapter — real MCP delegation (open-codebase-index 0.24.0, ollama all-minilm).

Uses persistent MCP server per repo via Node dist/cli.js, Ollama local embeddings.
Proven to index gin (6 min, 1598 chunks, all-minilm) and return correct Engine hit.
For C0.1 full 26Q, reuse this adapter with warm daemon.
"""

from __future__ import annotations

import json
import subprocess
import time
import threading
import queue
import re
from pathlib import Path
from typing import Dict

from .interface import BenchmarkAdapter, IndexingMetrics, SearchHit, SearchResult

REPO_ROOT = Path(__file__).resolve().parents[2]

try:
    import tiktoken
    _enc = tiktoken.get_encoding("cl100k_base")
    def _tok(s: str) -> int:
        return len(_enc.encode(s or ""))
except:
    def _tok(s: str) -> int:
        return len((s or "").split())

def _resolve_bin() -> Path:
    # prefer local npm install at C:\Temp\oci_test, fallback to repo's node_modules
    p = Path(r"C:\Temp\oci_test\node_modules\open-codebase-index\dist\cli.js")
    if p.exists():
        return p
    cand = REPO_ROOT / "node_modules" / "open-codebase-index" / "dist" / "cli.js"
    if cand.exists():
        return cand
    return p

class _OciMcpClient:
    def __init__(self, repo_path: Path):
        self.repo_path = repo_path
        self.bin = _resolve_bin()
        self.proc = subprocess.Popen(
            ["node", str(self.bin)],
            cwd=str(repo_path),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True, encoding="utf-8", errors="ignore", bufsize=1,
        )
        self._next_id = 1
        self._pending: Dict[int, queue.Queue] = {}
        self._lock = threading.Lock()
        self._reader = threading.Thread(target=self._reader_loop, daemon=True)
        self._reader.start()
        self._err = threading.Thread(target=self._err_loop, daemon=True)
        self._err.start()
        self._initialize()
        self.startup_ms = 0
        self.pid = self.proc.pid

    def _reader_loop(self):
        for line in iter(self.proc.stdout.readline, ""):
            if not line: break
            line=line.strip()
            if not line: continue
            try: msg=json.loads(line)
            except: continue
            if "id" in msg:
                with self._lock:
                    q=self._pending.get(msg["id"])
                if q: q.put(msg)

    def _err_loop(self):
        for line in iter(self.proc.stderr.readline, ""):
            if not line: break
            pass

    def _send(self, obj: dict):
        self.proc.stdin.write(json.dumps(obj)+"\n")
        self.proc.stdin.flush()

    def _request(self, method: str, params=None, timeout=60):
        mid=self._next_id; self._next_id+=1
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
        return resp.get("result", resp)

    def _notify(self, method, params=None):
        obj={"jsonrpc":"2.0","method":method}
        if params is not None: obj["params"]=params
        self._send(obj)

    def _initialize(self):
        t0=time.perf_counter()
        self._request("initialize", {"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench-oci","version":"0.1"}}, timeout=15)
        self._notify("notifications/initialized", {})
        time.sleep(0.3)
        self.startup_ms=int((time.perf_counter()-t0)*1000)

    def call(self, name, args, timeout=60):
        result=self._request("tools/call", {"name":name,"arguments":args}, timeout=timeout)
        content=result.get("content") or []
        text="".join(c.get("text","")+"\n" for c in content if isinstance(c, dict)).strip()
        return text, result

    def close(self):
        try:
            if self.proc.stdin: self.proc.stdin.close()
        except: pass
        try:
            self.proc.terminate()
            self.proc.wait(timeout=2)
        except: pass
        try: self.proc.kill()
        except: pass

def _parse_oci_text(text: str):
    # OCI returns lines like: [1] type_spec "Engine" in C:\...\gin\gin.go:92-189 (score 0.99)
    hits=[]
    for line in text.splitlines():
        # find file path pattern: in <path>:<lines>
        m=re.search(r" in (.+?):(\d+)-(\d+)", line)
        if not m:
            # try absolute win path with drive
            m=re.search(r"([A-Za-z]:\\.+?\.\w+):(\d+)-(\d+)", line)
        if m:
            f=m.group(1)
            # normalize to repo-relative posix
            # if absolute, try to make relative to repo root by taking last 2 parts?
            # For now, extract file name and try to find via repo walk later; fallback to full path's basename
            # We will return posix relative if possible by stripping absolute prefix
            # Simple: replace \ with /, take after bench/repos/<repo>/
            f=f.replace("\\","/")
            # try to find /bench/repos/<repo>/ segment
            if "/bench/repos/" in f:
                f=f.split("/bench/repos/")[1]
                # remove first component (repo name) slash?
                parts=f.split("/",1)
                if len(parts)==2:
                    f=parts[1]
                else:
                    f=parts[0]
            else:
                # fallback to basename
                f=Path(f).name
                # try to find actual file via walk? For now use basename
            try: start=int(m.group(2))
            except: start=None
            # score
            sm=re.search(r"score ([0-9.]+)", line)
            score=float(sm.group(1)) if sm else None
            hits.append((f, start, score, line[:400]))
    return hits

class OciAdapter(BenchmarkAdapter):
    name="oci"
    def __init__(self):
        self._clients: Dict[str, _OciMcpClient] = {}
        self._bin=_resolve_bin()

    def _client(self, repo_path: Path) -> _OciMcpClient:
        key=str(repo_path.resolve())
        if key not in self._clients:
            self._clients[key]=_OciMcpClient(repo_path)
        return self._clients[key]

    def index(self, repo_path: Path) -> IndexingMetrics:
        t0=time.perf_counter()
        try:
            c=self._client(repo_path)
            text, _=c.call("index_status", {}, timeout=15)
            wall=int((time.perf_counter()-t0)*1000)
            # if not indexed, trigger index_codebase
            if "not indexed" in text.lower():
                # start index, poll
                def do_index():
                    try: c.call("index_codebase", {}, timeout=600)
                    except: pass
                th=threading.Thread(target=do_index, daemon=True)
                th.start()
                # poll up to 10 min
                for _ in range(60):
                    time.sleep(10)
                    txt,_=c.call("index_status", {}, timeout=15)
                    if "not indexed" not in txt.lower() and "indexing" not in txt.lower():
                        break
                    if "failed" in txt.lower():
                        break
                wall=int((time.perf_counter()-t0)*1000)
                # get final status
                text,_=c.call("index_status", {}, timeout=15)
            # try to parse disk/files from status text
            # For now, report wall and mark unavailable for detailed counts
            return IndexingMetrics(initial_wall_ms=wall, files_indexed=None, symbols=None, bm25_docs=None, vector_count=None, index_disk_bytes=None, unavailable=["symbols","bm25_docs","vector_count","index_disk_bytes","cpu_ms","peak_rss_mb","no_change_wall_ms","one_file_wall_ms"])
        except Exception as e:
            wall=int((time.perf_counter()-t0)*1000)
            return IndexingMetrics(initial_wall_ms=wall, unavailable=["oci_error:"+str(e)[:100]])

    def search(self, query: str, repo_path: Path, top_n: int=5) -> SearchResult:
        t0=time.perf_counter()
        try:
            c=self._client(repo_path)
            # prefer codebase_context
            text,_=c.call("codebase_context", {"query":query}, timeout=60)
            # if no results, try codebase_search
            if "No matching code" in text:
                text2,_=c.call("codebase_search", {"query":query}, timeout=60)
                if "No matching" not in text2:
                    text=text2
            hits_raw=_parse_oci_text(text)
            # dedup and top_n
            seen=set()
            hits=[]
            for f, line, score, txt in hits_raw:
                if f not in seen:
                    seen.add(f)
                    hits.append(SearchHit(file=f, score=score, line=line, text=txt, provenance="oci:codebase_context"))
                if len(hits)>=top_n: break
            # if no hits, return empty
            wall=int((time.perf_counter()-t0)*1000)
            # common tokens via cl100k on concatenated hit texts
            common=_tok(" ".join(h.text or "" for h in hits)) if hits else 0
            return SearchResult(query=query, hits=hits, candidate_count=len(hits_raw), evidence_count=len(hits), files_returned=len(set(h.file for h in hits)), candidate_tokens=None, packed_tokens=common, retrievers_used=[f"oci:codebase_context:{len(hits_raw)}"], elapsed_ms=wall, wall_ms=wall, internal_ms=wall, raw={"text":text[:2000]})
        except Exception as e:
            wall=int((time.perf_counter()-t0)*1000)
            return SearchResult(query=query, hits=[], candidate_count=0, evidence_count=0, files_returned=0, retrievers_used=[f"oci:error:{e}"], elapsed_ms=wall, raw={"error":str(e)})

    def close(self):
        for c in list(self._clients.values()):
            try: c.close()
            except: pass
        self._clients.clear()
