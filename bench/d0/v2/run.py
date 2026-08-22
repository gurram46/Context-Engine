#!/usr/bin/env python3
"""D0-v2 runner — benchmark integrity hardened.

Supports: --validate-only, --smoke-ce, --run-task, --run-all
Pre-run phase only executes validate-only + smoke-ce.

Fixes vs v1:
- behavioral hidden evaluators (no source-string)
- no MUTATED markers (plausible regressions)
- pair source_tree_hash identity
- WITH semantic precondition (missing 0, ready true) else BLOCKED
- tiktoken cl100k_base for tool-output tokens (fail if unavailable)
- exact OpenCode JSON event parse for tool classification
- leakage audit
"""
import argparse, hashlib, json, os, random, shutil, subprocess, sys, time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
TASKS_DIR = REPO_ROOT / "bench/d0/v2/tasks"
PRIVATE_DIR = REPO_ROOT / "bench/d0/v2/private"
RUNS_DIR = REPO_ROOT / "bench/d0/v2/runs"
RESULTS_DIR = REPO_ROOT / "bench/d0/v2/results"
VALIDATION_DIR = REPO_ROOT / "bench/d0/v2/validation"
SMOKE_DIR = REPO_ROOT / "bench/d0/v2/smoke"
REPOS_ROOT = REPO_ROOT / "bench/repos"
MANIFEST = TASKS_DIR / "manifest.json"
MODEL = "opencode/x-preview-f-free"
TIMEOUT_S = 1200
CONTEXTD_BIN = REPO_ROOT / "target/release/contextd.exe"
OPENCODE_EXE = Path(r"C:\Users\Dell\AppData\Local\Programs\nodejs\node_modules\opencode-ai\bin\opencode.exe")

def _ctx_bin():
    if CONTEXTD_BIN.exists():
        return CONTEXTD_BIN
    cand = REPO_ROOT / "target/release/contextd"
    if cand.exists():
        return cand
    return CONTEXTD_BIN

def _tiktoken_cl100k(text: str):
    try:
        import tiktoken
        enc = tiktoken.get_encoding("cl100k_base")
        return len(enc.encode(text))
    except Exception as e:
        raise RuntimeError(f"tiktoken cl100k_base unavailable: {e}. Install via pip install tiktoken")

def _hash_source_tree(workdir: Path):
    # deterministic hash of all files excluding .git/.context/.opencode/target/node_modules
    h = hashlib.sha256()
    for p in sorted(workdir.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(workdir).as_posix()
        if rel.startswith(".git/") or rel.startswith(".context/") or ".opencode" in rel or "target/" in rel or "node_modules/" in rel:
            continue
        h.update(rel.encode())
        h.update(b"\0")
        h.update(p.read_bytes())
        h.update(b"\0")
    return h.hexdigest()[:16]

def _fast_copy(src: Path, dest: Path):
    # instance-only, scoped to dest, no global kills
    if dest.exists():
        shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)
    if os.name == "nt":
        xd = [".git", ".context", ".serena", ".opencode", "target", "node_modules", ".venv", "__pycache__"]
        cmd = ["robocopy", str(src), str(dest), "/E", "/XD"] + xd + ["/NFL", "/NDL", "/NJH", "/NJS", "/R:0", "/W:0"]
        subprocess.run(cmd, capture_output=True)
    else:
        for item in src.iterdir():
            if item.name in [".git",".context",".serena",".opencode","target","node_modules",".venv","__pycache__"]:
                continue
            if item.is_dir():
                shutil.copytree(item, dest/item.name, dirs_exist_ok=True)
            else:
                shutil.copy2(item, dest/item.name)

def _leakage_scan(text: str):
    forbidden = ["MUTATED", "BENCHMARK", "reference patch", "hidden evaluator", "private", "D0", "BUG HERE", "FIX ME"]
    # also check for exact reference patch content leaked? For now forbid MUTATED etc.
    hits = []
    for term in ["MUTATED", "BENCHMARK", "reference patch", "hidden_evaluator", "private test"]:
        if term.lower() in text.lower():
            hits.append(term)
    # also exact marker leak
    if "MUTATED" in text:
        hits.append("MUTATED")
    return hits

def run_opencode_real(prompt: str, workdir: Path, model: str = MODEL, timeout_s: int = TIMEOUT_S, arm: str = "with"):
    workdir = Path(workdir)
    exe = OPENCODE_EXE if OPENCODE_EXE.exists() else Path(shutil.which("opencode") or "opencode")
    base = [str(exe), "run", "--model", model, "--format", "json", "--dir", str(workdir)]
    if arm == "without":
        base.append("--pure")
    cmd = base + [prompt]
    t0 = time.time()
    env = os.environ.copy()
    if arm == "with":
        env["CONTEXT_ENGINE_PROJECT_ROOT"] = str(workdir)
        try:
            (workdir / "opencode.json").write_text(json.dumps({"mcp": {"contextd": {"type": "local", "command": [str(_ctx_bin())], "enabled": True, "environment": {"CONTEXT_ENGINE_PROJECT_ROOT": str(workdir), "CONTEXTD_EMBED_MODEL": "all-minilm"}}}}, indent=2), encoding="utf-8")
        except: pass
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="replace", cwd=str(workdir), env=env)
    try:
        out, err = proc.communicate(timeout=timeout_s)
        wall_ms = int((time.time() - t0) * 1000)
        return {"stdout": out, "stderr": err, "returncode": proc.returncode, "wall_ms": wall_ms, "timeout": False, "pid": proc.pid, "cmd": cmd}
    except subprocess.TimeoutExpired:
        try: proc.kill()
        except: pass
        try: out, err = proc.communicate(timeout=10)
        except:
            try: proc.terminate()
            except: pass
            out, err = "", ""
        wall_ms = int((time.time() - t0) * 1000)
        return {"stdout": out or "", "stderr": err or "", "returncode": -1, "wall_ms": wall_ms, "timeout": True, "pid": proc.pid, "cmd": cmd}

def prepare_worktree(task: dict, arm: str, run_id: str = ""):
    repo = task["repo"]
    task_id = task["task_id"]
    dest = RUNS_DIR / task_id / arm
    if dest.exists():
        shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)
    src = REPOS_ROOT / repo
    if not src.exists():
        raise RuntimeError(f"source repo missing {src}")
    for item in src.iterdir():
        if item.name in [".git", ".context", ".serena", ".opencode", "target", "node_modules", ".venv", "__pycache__"]:
            continue
        if item.is_dir():
            shutil.copytree(item, dest / item.name, dirs_exist_ok=True)
        else:
            shutil.copy2(item, dest / item.name)
    # git init for patch
    subprocess.run(["git", "init", "-q"], cwd=str(dest), capture_output=True)
    subprocess.run(["git", "config", "user.email", "d0@test.com"], cwd=str(dest), capture_output=True)
    subprocess.run(["git", "config", "user.name", "d0"], cwd=str(dest), capture_output=True)
    patch_file = TASKS_DIR / task["mutation_patch"]
    if not patch_file.exists():
        raise RuntimeError(f"patch missing {patch_file}")
    chk = subprocess.run(["git", "apply", "--check", str(patch_file)], cwd=str(dest), capture_output=True, text=True, encoding="utf-8", errors="replace")
    if chk.returncode != 0:
        raise RuntimeError(f"git apply --check failed {task_id} {arm}: {chk.stderr[:2000]}")
    ap = subprocess.run(["git", "apply", str(patch_file)], cwd=str(dest), capture_output=True, text=True, encoding="utf-8", errors="replace")
    if ap.returncode != 0:
        raise RuntimeError(f"git apply failed {task_id} {arm}: {ap.stderr[:2000]}")
    mut_file = dest / task["mutation_file"] if "mutation_file" in task else None
    # verify file exists (no MUTATED marker check, since v2 has no markers - verify via hash)
    # compute source hash before .context
    source_hash = _hash_source_tree(dest)
    (dest / ".mutation_applied").write_text(patch_file.read_text(encoding="utf-8"), encoding="utf-8")
    (dest / ".source_tree_hash").write_text(source_hash, encoding="utf-8")
    # stage for diff
    try:
        subprocess.run(["git", "add", str(task["mutation_file"]) if "mutation_file" in task else "."], cwd=str(dest), capture_output=True, encoding="utf-8", errors="replace", timeout=10)
        subprocess.run(["git", "commit", "-m", "initial mutated", "--allow-empty"], cwd=str(dest), capture_output=True, encoding="utf-8", errors="replace", timeout=10)
    except: pass
    return dest, source_hash

def ce_prepare(workdir: Path, task: dict):
    binp = _ctx_bin()
    if not binp.exists():
        raise RuntimeError(f"contextd not found {binp}")
    t0 = time.time()
    proc = subprocess.run([str(binp), "index", "--root", str(workdir), "--semantic", "--json"], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=1200)
    idx_wall = int((time.time() - t0) * 1000)
    idx_out = proc.stdout
    proc2 = subprocess.run([str(binp), "status", "--root", str(workdir), "--json"], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
    status={}
    try:
        status=json.loads(proc2.stdout or "{}")
    except:
        status={"raw": proc2.stdout[:2000], "stderr": proc2.stderr[:2000]}
    # WITH precondition
    ready = status.get("semanticIndexReady")
    missing = status.get("missingVectorCount")
    if ready is not True or missing != 0:
        # do not silently continue lexical-only
        status["_precondition"] = "BLOCKED"
    else:
        status["_precondition"] = "READY"
    prep={
        "index_wall_ms": idx_wall,
        "index_stdout": idx_out[:8000] if idx_out else "",
        "index_stderr": proc.stderr[:4000] if proc.stderr else "",
        "index_returncode": proc.returncode,
        "status": status,
        "generation": status.get("indexGeneration"),
        "semanticAvailable": status.get("semanticAvailable"),
        "semanticIndexReady": ready,
        "missingVectorCount": missing,
        "embeddingModel": status.get("embeddingModel"),
        "pid": status.get("pid"),
    }
    (workdir / ".ce_prep.json").write_text(json.dumps(prep, indent=2), encoding="utf-8")
    return prep

def parse_metrics_exact(workdir: Path, stdout: str):
    # exact parse of OpenCode JSON events
    tool_counts = {"read":0,"grep":0,"glob":0,"bash":0,"edit":0,"context_search":0,"symbol_lookup":0,"dependency_trace":0,"test_lookup":0,"context_status":0}
    ce_details=[]
    input_tokens=None
    output_tokens=None
    cache_read=None
    cache_write=None
    # for tool-output tokens via tiktoken
    tool_output_text=""
    for line in (stdout or "").splitlines():
        line=line.strip()
        if not line:
            continue
        try:
            j=json.loads(line)
        except: continue
        # exact tool name from j["part"]["tool"] or j["type"]=="tool_use"
        tool_name=None
        part = j.get("part") if isinstance(j.get("part"), dict) else {}
        if isinstance(part, dict) and "tool" in part:
            tool_name = part.get("tool")
        elif j.get("type")=="tool_use" and "tool" in j:
            tool_name=j.get("tool")
        if tool_name in tool_counts:
            tool_counts[tool_name]+=1
            if tool_name in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status"]:
                ce_details.append({"tool": tool_name, "event": j})
        # tokens in part.tokens
        toks=None
        if isinstance(part, dict) and "tokens" in part and isinstance(part["tokens"], dict):
            toks=part["tokens"]
        elif "tokens" in j and isinstance(j["tokens"], dict):
            toks=j["tokens"]
        if toks:
            if toks.get("input") is not None:
                # sum across steps? Keep last for now, but also sum for total
                # we store last, but aggregate will sum
                input_tokens = toks.get("input") if input_tokens is None else input_tokens
                # actually sum
                if input_tokens is not None:
                    input_tokens = (input_tokens or 0) + toks.get("input",0) if isinstance(input_tokens,int) and toks.get("input") else toks.get("input")
                # fallback: keep last
            if toks.get("output") is not None:
                output_tokens = toks.get("output")
            if isinstance(toks.get("cache"), dict):
                cache_read = toks["cache"].get("read")
                cache_write = toks["cache"].get("write")
        # collect tool output text for tiktoken
        if isinstance(part, dict) and "state" in part and isinstance(part["state"], dict):
            out = part["state"].get("output") or part["state"].get("stdout") or ""
            if isinstance(out, str):
                tool_output_text += out + "\n"
        if "output" in j and isinstance(j["output"], str):
            tool_output_text += j["output"] + "\n"
    # tiktoken for tool-output
    try:
        tool_output_tokens = _tiktoken_cl100k(tool_output_text) if tool_output_text else 0
    except Exception as e:
        raise
    leak_hits=[]
    for ce in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status","contextd"]:
        if ce in (stdout or ""):
            leak_hits.append(ce)
    # define native_repository_lookup = read+grep+glob
    native_lookup = tool_counts["read"] + tool_counts["grep"] + tool_counts["glob"]
    return {
        "tool_counts": tool_counts,
        "native_lookup": native_lookup,
        "ce_calls": sum(tool_counts[k] for k in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status"]),
        "ce_details": ce_details[:100],
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_read": cache_read,
        "cache_write": cache_write,
        "tool_output_tokens_cl100k": tool_output_tokens,
        "leak_hits": leak_hits,
    }

def run_hidden_evaluator(workdir: Path, task: dict):
    cmd = task["hidden_evaluator"]
    t0=time.time()
    proc = subprocess.run(cmd, shell=True, cwd=str(workdir), capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=300)
    wall=int((time.time()-t0)*1000)
    passed=(proc.returncode==0)
    result={"command": cmd, "returncode": proc.returncode, "wall_ms": wall, "pass": passed, "stdout": (proc.stdout or "")[:8000], "stderr": (proc.stderr or "")[:8000]}
    return result

def validate_tasks():
    print("=== D0-v2 PRE-RUN VALIDATION (original PASS -> mutated FAIL -> reference PASS) ===")
    manifest=json.loads(MANIFEST.read_text(encoding="utf-8"))
    ok=True
    VALIDATION_DIR.mkdir(parents=True, exist_ok=True)
    for task in manifest["tasks"]:
        task_id=task["task_id"]
        print(f"\n-- {task_id} ({task['repo']}) --")
        # original (pinned, no mutation)
        # copy without mutation
        src = REPOS_ROOT / task["repo"]
        # helper to copy original to temp and run evaluator
        def copy_original(dest):
            _fast_copy(src, dest)
            return dest
        dest_orig = RUNS_DIR / f"_validate_{task_id}_orig"
        print(f"  copying original {task_id} via {'robocopy' if os.name=='nt' else 'shutil'}...", flush=True)
        dest_orig = copy_original(dest_orig)
        print(f"  copying original done", flush=True)
        print(f"  copying original done", flush=True)
        print(f"  running original evaluator...", flush=True)
        res_orig = run_hidden_evaluator(dest_orig, task)
        print(f"  original evaluator {'PASS' if res_orig['pass'] else 'FAIL'} (expect PASS) rc={res_orig['returncode']}", flush=True)
        if not res_orig["pass"]:
            print(f"    out {res_orig['stdout'][:300]} err {res_orig['stderr'][:500]}")
            ok=False
        # mutated
        print(f"  copying mutated {task_id}...", flush=True)
        dest_mut = RUNS_DIR / f"_validate_{task_id}_mutated"
        _fast_copy(src, dest_mut)
        print(f"  copying mutated done", flush=True)
        patch_file = TASKS_DIR / task["mutation_patch"]
        subprocess.run(["git","init","-q"], cwd=str(dest_mut), capture_output=True)
        subprocess.run(["git","config","user.email","d0@test.com"], cwd=str(dest_mut), capture_output=True)
        chk = subprocess.run(["git","apply","--check", str(patch_file)], cwd=str(dest_mut), capture_output=True, text=True, encoding="utf-8", errors="replace")
        if chk.returncode!=0:
            print(f"  mutated git apply --check FAIL {chk.stderr[:500]}")
            ok=False
            continue
        ap = subprocess.run(["git","apply", str(patch_file)], cwd=str(dest_mut), capture_output=True, text=True, encoding="utf-8", errors="replace")
        if ap.returncode!=0:
            print(f"  mutated apply FAIL {ap.stderr[:500]}")
            ok=False
            continue
        res_mut = run_hidden_evaluator(dest_mut, task)
        print(f"  mutated evaluator {'PASS' if res_mut['pass'] else 'FAIL'} (expect FAIL) rc={res_mut['returncode']}")
        if res_mut["pass"]:
            print(f"    ERROR mutated should FAIL but PASS")
            ok=False
        # reference fix applied to mutated
        dest_ref = RUNS_DIR / f"_validate_{task_id}_ref"
        if dest_ref.exists():
            shutil.rmtree(dest_ref, ignore_errors=True)
        # copy mutated then apply reference patch (fast)
        if os.name == "nt":
            subprocess.run(["robocopy", str(dest_mut), str(dest_ref), "/E", "/NFL", "/NDL", "/NJH", "/NJS", "/R:0", "/W:0"], capture_output=True)
        else:
            shutil.copytree(dest_mut, dest_ref, dirs_exist_ok=True)
        # remove .git from dest_ref and re-init? Use same dest_mut's .git? Simpler: apply reference patch via git apply
        ref_patch = PRIVATE_DIR / f"reference_{task_id}.patch"
        if not ref_patch.exists():
            # try generic
            ref_patch = PRIVATE_DIR / f"reference_{task_id.split('_')[0]}_02.patch"
        if ref_patch.exists():
            ap2 = subprocess.run(["git","apply", str(ref_patch)], cwd=str(dest_ref), capture_output=True, text=True, encoding="utf-8", errors="replace")
            if ap2.returncode!=0:
                print(f"  reference apply FAIL {ap2.stderr[:500]}")
                ok=False
                continue
        else:
            # fallback: revert mutation by checkout original file
            # copy original file over
            pass
        res_ref = run_hidden_evaluator(dest_ref, task)
        print(f"  reference evaluator {'PASS' if res_ref['pass'] else 'FAIL'} (expect PASS) rc={res_ref['returncode']}")
        if not res_ref["pass"]:
            print(f"    out {res_ref['stdout'][:500]} err {res_ref['stderr'][:500]}")
            ok=False
        # store validation evidence
        (VALIDATION_DIR / f"{task_id}_validation.json").write_text(json.dumps({"task_id": task_id, "original": res_orig, "mutated": res_mut, "reference": res_ref}, indent=2), encoding="utf-8")
        # cleanup
        for p in [dest_orig, dest_mut, dest_ref]:
            shutil.rmtree(p, ignore_errors=True)
    if ok:
        print("\nVALIDATION 5/5 PASS (original PASS, mutated FAIL, reference PASS)")
    else:
        print("\nVALIDATION FAILED")
    return ok

def leakage_audit():
    print("=== LEAKAGE AUDIT (agent-visible material) ===")
    manifest=json.loads(MANIFEST.read_text(encoding="utf-8"))
    ok=True
    for task in manifest["tasks"]:
        tid=task["task_id"]
        prompt=task["public_task_prompt"]
        # check prompt for forbidden leaks
        hits=_leakage_scan(prompt)
        # also check mutation patch not leaked? patch is not in prompt, but ensure prompt doesn't contain file/function exact
        # For now, check for exact mutation file name in prompt (should not)
        mut_file = task.get("mutation_file","")
        if mut_file and mut_file.split("/")[-1] in prompt:
            # allow repo name but not exact file
            pass
        # check for forbidden terms
        if "MUTATED" in prompt or "BENCHMARK" in prompt:
            hits.append("MUTATED/BENCHMARK in prompt")
        # check for hidden evaluator leakage
        hidden = task.get("hidden_evaluator","")
        if hidden.split("/")[-1] in prompt:
            hits.append("hidden evaluator leaked")
        status="VALID" if not hits else "INVALID"
        if hits:
            ok=False
        print(f"  {tid}: {status} hits={hits}")
    # also scan tasks directory files for markers
    for p in TASKS_DIR.glob("*.patch"):
        content=p.read_text(encoding="utf-8", errors="ignore")
        if "MUTATED" in content:
            print(f"  LEAK: {p.name} contains MUTATED marker (must be removed)")
            ok=False
    print("LEAKAGE", "PASS 0" if ok else "FAIL")
    return ok

def smoke_ce():
    print("=== CE NON-SCORED SMOKE (disposable repo) ===")
    # use lodash as disposable (small)
    manifest=json.loads(MANIFEST.read_text(encoding="utf-8"))
    task = manifest["tasks"][3]  # lodash
    repo = task["repo"]
    src = REPOS_ROOT / repo
    dest = SMOKE_DIR / "disposable"
    if dest.exists():
        shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)
    for item in src.iterdir():
        if item.name in [".git",".context",".serena",".opencode","target","node_modules"]:
            continue
        if item.is_dir():
            shutil.copytree(item, dest/item.name, dirs_exist_ok=True)
        else:
            shutil.copy2(item, dest/item.name)
    # init git and prepare CE
    subprocess.run(["git","init","-q"], cwd=str(dest), capture_output=True)
    binp=_ctx_bin()
    # index
    proc = subprocess.run([str(binp), "index", "--root", str(dest), "--semantic", "--json"], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=600)
    proc2 = subprocess.run([str(binp), "status", "--root", str(dest), "--json"], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=30)
    try:
        status=json.loads(proc2.stdout)
        print(f"  CE status generation {status.get('indexGeneration')} ready {status.get('semanticIndexReady')} missing {status.get('missingVectorCount')}")
    except:
        print(f"  status fail {proc2.stdout[:500]}")
        return False
    # run opencode smoke that should invoke each tool
    prompt = "Verify Context Engine tools are available. Use each of these tools once with a trivial query: context_search with query 'slice helper', symbol_lookup for 'baseSlice', dependency_trace for 'baseSlice', test_lookup for 'slice', and context_status. Report what you found. Do not edit files."
    res = run_opencode_real(prompt, dest, timeout_s=600, arm="with")
    (SMOKE_DIR / "raw_opencode_stdout.jsonl").write_text(res["stdout"] or "", encoding="utf-8")
    (SMOKE_DIR / "raw_opencode_stderr.txt").write_text(res["stderr"] or "", encoding="utf-8")
    # check each tool seen
    tools = ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status"]
    hits={}
    for t in tools:
        hits[t] = t in (res["stdout"] or "")
        print(f"  {t}: {'PASS' if hits[t] else 'FAIL'}")
    ok = all(hits.values())
    print(f"SMOKE {'PASS' if ok else 'FAIL'} 5/5" if ok else f"SMOKE FAIL {hits}")
    (SMOKE_DIR / "hits.json").write_text(json.dumps(hits, indent=2), encoding="utf-8")
    # cleanup disposable .context after? Keep for evidence
    return ok

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--validate-only", action="store_true")
    ap.add_argument("--smoke-ce", action="store_true")
    ap.add_argument("--run-task", nargs=2, metavar=("task_id","arm"))
    ap.add_argument("--run-all", action="store_true")
    args=ap.parse_args()
    if args.validate_only:
        ok=validate_tasks()
        sys.exit(0 if ok else 2)
    if args.smoke_ce:
        ok=smoke_ce()
        sys.exit(0 if ok else 2)
    if args.run_task:
        print("run-task not in pre-run phase")
        sys.exit(1)
    if args.run_all:
        print("run-all not in pre-run phase")
        sys.exit(1)
    ap.print_help()

if __name__=="__main__":
    main()
