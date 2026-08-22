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
        res = subprocess.run(cmd, capture_output=True)
        # robocopy 0-7 success, >=8 failure
        if res.returncode >= 8:
            raise RuntimeError(f"robocopy failed rc={res.returncode} src={src} dest={dest}")
        # verify mutation target exists after copy (if src had it)
        # caller will verify specific file
    else:
        for item in src.iterdir():
            if item.name in [".git",".context",".serena",".opencode","target","node_modules",".venv","__pycache__"]:
                continue
            if item.is_dir():
                shutil.copytree(item, dest/item.name, dirs_exist_ok=True)
            else:
                shutil.copy2(item, dest/item.name)
    # verify at least one file copied
    if not any(dest.iterdir()):
        raise RuntimeError(f"fast_copy produced empty dest {dest} from {src}")

def _normalize_tool(name: str) -> str:
    # deterministic normalization for contextd MCP prefix
    # e.g., contextd_context_search -> context_search, contextd_symbol_lookup -> symbol_lookup
    if name.startswith("contextd_"):
        base = name[len("contextd_"):]
        if base in ["context_search", "symbol_lookup", "dependency_trace", "test_lookup", "context_status"]:
            return base
    return name

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
    # WITH precondition - hard-fail
    ready = status.get("semanticIndexReady")
    missing = status.get("missingVectorCount")
    if proc.returncode != 0:
        status["_precondition"] = "BLOCKED"
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
        raise RuntimeError(f"CE index failed rc={proc.returncode} for {workdir}")
    if ready is not True or missing != 0:
        status["_precondition"] = "BLOCKED"
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
        raise RuntimeError(f"CE semantic precondition BLOCKED: ready={ready} missing={missing} for {workdir}")
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
    # exact parse of OpenCode JSON events - uses _normalize_tool and sums tokens
    tool_counts = {"read":0,"grep":0,"glob":0,"bash":0,"edit":0,"context_search":0,"symbol_lookup":0,"dependency_trace":0,"test_lookup":0,"context_status":0}
    ce_details=[]
    input_tokens=0
    output_tokens=0
    cache_read=0
    cache_write=0
    has_tokens=False
    # for tool-output tokens via tiktoken
    tool_output_text=""
    for line in (stdout or "").splitlines():
        line=line.strip()
        if not line:
            continue
        try:
            j=json.loads(line)
        except: continue
        # exact tool name from j["part"]["tool"] when type=="tool_use"
        tool_name=None
        part = j.get("part") if isinstance(j.get("part"), dict) else {}
        if j.get("type")=="tool_use" and isinstance(part, dict) and "tool" in part:
            raw = part.get("tool")
            tool_name = _normalize_tool(raw) if isinstance(raw, str) else None
        if tool_name in tool_counts:
            tool_counts[tool_name]+=1
            if tool_name in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status"]:
                ce_details.append({"tool": tool_name, "event": j})
        # tokens only from step_finish (provider usage)
        toks=None
        if j.get("type")=="step_finish":
            if isinstance(part, dict) and "tokens" in part and isinstance(part["tokens"], dict):
                toks=part["tokens"]
            elif "tokens" in j and isinstance(j["tokens"], dict):
                toks=j["tokens"]
        if toks:
            has_tokens=True
            if isinstance(toks.get("input"), int):
                input_tokens += toks.get("input",0)
            if isinstance(toks.get("output"), int):
                output_tokens += toks.get("output",0)
            if isinstance(toks.get("cache"), dict):
                if isinstance(toks["cache"].get("read"), int):
                    cache_read += toks["cache"].get("read",0)
                if isinstance(toks["cache"].get("write"), int):
                    cache_write += toks["cache"].get("write",0)
        # collect tool output text for tiktoken - only from tool_use completed state
        if j.get("type")=="tool_use" and isinstance(part, dict) and "state" in part and isinstance(part["state"], dict):
            st = part["state"]
            if st.get("status")=="completed":
                out = st.get("output") or st.get("stdout") or ""
                if isinstance(out, str) and out:
                    tool_output_text += out + "\n"
    # tiktoken for tool-output
    try:
        tool_output_tokens = _tiktoken_cl100k(tool_output_text) if tool_output_text else 0
    except Exception as e:
        raise
    # leak hits via exact normalized tool names, not substring
    leak_hits=[]
    for ce in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status"]:
        if tool_counts.get(ce,0) > 0:
            leak_hits.append(ce)
    # define native_repository_lookup = read+grep+glob
    native_lookup = tool_counts["read"] + tool_counts["grep"] + tool_counts["glob"]
    return {
        "tool_counts": tool_counts,
        "native_lookup": native_lookup,
        "ce_calls": sum(tool_counts[k] for k in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status"]),
        "ce_details": ce_details[:100],
        "input_tokens": input_tokens if has_tokens else None,
        "output_tokens": output_tokens if has_tokens else None,
        "cache_read": cache_read if has_tokens and cache_read!=0 else (cache_read if has_tokens else None),
        "cache_write": cache_write if has_tokens and cache_write!=0 else (cache_write if has_tokens else None),
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
        hits=[]
        # forbidden markers in prompt
        for term in ["MUTATED", "BENCHMARK", "D0", "BUG HERE", "FIX ME"]:
            if term in prompt:
                hits.append(f"marker {term} in prompt")
        # mutation file path/basename must not be in prompt
        mut_file = task.get("mutation_file","")
        if mut_file:
            base = mut_file.split("/")[-1]
            if base in prompt or mut_file in prompt:
                hits.append(f"mutation_file {mut_file} leaked in prompt")
            # also check directory part
            if mut_file.split("/")[0] in prompt and mut_file in prompt:
                hits.append("mutation path leaked")
        # private evaluator names must not be in prompt
        hidden = task.get("hidden_evaluator","")
        # extract evaluator script name if any
        for part in hidden.split():
            if "evaluator" in part.lower() or "private" in part.lower():
                if part in prompt:
                    hits.append("private evaluator leaked")
        # reference patch content must not be in prompt (check for exact replacement expression)
        # For now, ensure prompt does not contain exact condition from patch
        # e.g., for django, check for "path.replace" exact
        # We do a manual review: check for leaked phrases that were removed
        leaked_phrases = [
            "deconstruct through the public import path",
            "first defined regardless of truthiness",
            "The fast line-by-line path ignores the flag",
            "The bug is in the shared helper",
            "looking for the wrong tag name",
        ]
        for phrase in leaked_phrases:
            if phrase in prompt:
                hits.append(f"leaked phrase: {phrase[:30]}")
        # also run generic scan
        hits.extend(_leakage_scan(prompt))
        # deduplicate
        hits = list(set(hits))
        status="VALID" if not hits else "INVALID"
        if hits:
            ok=False
        else:
            # manual review - mark VALID (would be QUESTIONABLE if borderline)
            pass
        print(f"  {tid}: {status} hits={hits}")
    # also scan tasks directory files for markers
    for p in TASKS_DIR.glob("*.patch"):
        content=p.read_text(encoding="utf-8", errors="ignore")
        if "MUTATED" in content:
            print(f"  LEAK: {p.name} contains MUTATED marker (must be removed)")
            ok=False
    # scan private reference patch content not leaked in prompts (ensure no prompt contains reference patch diff)
    for task in manifest["tasks"]:
        ref = PRIVATE_DIR / f"reference_{task['task_id']}.patch"
        if ref.exists():
            ref_content = ref.read_text(encoding="utf-8", errors="ignore")
            # check if any large chunk of reference patch is in prompt (should not)
            prompt = task["public_task_prompt"]
            # take first added line from reference patch
            for line in ref_content.splitlines():
                if line.startswith("+") and len(line) > 10 and not line.startswith("+++"):
                    snippet = line[1:30].strip()
                    if snippet and snippet in prompt:
                        print(f"  LEAK: reference patch snippet leaked in {task['task_id']}: {snippet[:30]}")
                        ok=False
    print("LEAKAGE", "PASS 5/5 VALID" if ok else "FAIL")
    return ok

def _verify_patch_sha(task: dict):
    patch_file = TASKS_DIR / task["mutation_patch"]
    expected = task.get("mutation_patch_sha256","")
    if not expected:
        return True
    actual = hashlib.sha256(patch_file.read_bytes()).hexdigest().upper()
    # manifest stores without 0x, may be upper/lower
    if actual != expected.upper():
        # also try lower
        if actual.lower() != expected.lower():
            raise RuntimeError(f"patch SHA mismatch for {task['task_id']}: expected {expected} got {actual}")
    return True

def _assert_pair_identity(task: dict):
    # prepare both arms from same pinned repo + mutation, before .context
    # use deterministic hash excluding harness files
    task_id = task["task_id"]
    # prepare without and with source trees (without actually creating .context yet)
    # we use prepare_worktree but capture hash before ce_prepare
    # For check, we prepare both and compare
    dest_without, hash_without = prepare_worktree(task, "without")
    dest_with, hash_with = prepare_worktree(task, "with")
    # hashes already computed before .context (since .context not yet created)
    match = (hash_without == hash_with)
    # write pair_identity.json
    pair_path = RUNS_DIR / task_id / "pair_identity.json"
    pair_path.parent.mkdir(parents=True, exist_ok=True)
    pair_path.write_text(json.dumps({
        "task_id": task_id,
        "without_hash": hash_without,
        "with_hash": hash_with,
        "match": match,
        "mutation_patch_sha256": task.get("mutation_patch_sha256",""),
        "pinned_sha": task.get("pinned_sha","")
    }, indent=2), encoding="utf-8")
    if not match:
        raise RuntimeError(f"pair identity mismatch for {task_id}: {hash_without} != {hash_with}")
    # verify patch SHA
    _verify_patch_sha(task)
    # cleanup the temp pair (will be recreated for real run)
    # keep them for now, real run will overwrite
    return match

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
    # check each tool seen via exact normalized parser, not substring
    parsed = parse_metrics_exact(dest, res["stdout"] or "")
    hits={}
    for t in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status"]:
        hits[t] = parsed["tool_counts"].get(t,0) >= 1
        print(f"  {t}: {'PASS' if hits[t] else 'FAIL'} (count {parsed['tool_counts'].get(t,0)})")
    # also prove via normalization: show raw tool names seen
    raw_tools = set()
    for line in (res["stdout"] or "").splitlines():
        try:
            j=json.loads(line)
            if j.get("type")=="tool_use":
                raw = j.get("part",{}).get("tool","")
                raw_tools.add(raw)
        except: pass
    print(f"  raw tools seen: {sorted(raw_tools)}")
    ok = all(hits.values())
    print(f"SMOKE {'PASS' if ok else 'FAIL'} 5/5" if ok else f"SMOKE FAIL {hits}")
    (SMOKE_DIR / "hits.json").write_text(json.dumps(hits, indent=2), encoding="utf-8")
    # also write parsed metrics for audit
    (SMOKE_DIR / "parsed_metrics.json").write_text(json.dumps(parsed, indent=2), encoding="utf-8")
    # cleanup disposable .context after? Keep for evidence
    return ok

def _run_single_scored(task_id: str, arm: str):
    manifest=json.loads(MANIFEST.read_text(encoding="utf-8"))
    task=next(t for t in manifest["tasks"] if t["task_id"]==task_id)
    # verify patch SHA before any copy
    _verify_patch_sha(task)
    # prepare worktree and verify mutation
    dest, source_hash = prepare_worktree(task, arm)
    # for WITH, verify pair identity (both arms from same mutation)
    # we need to ensure without and with hashes match - prepare both and compare
    # For single arm, we check against the other arm's hash if exists, else just store
    # For now, we ensure the current arm's hash is stored and will be compared in run-all
    # CE prep for WITH with hard-fail
    ce_prep=None
    if arm=="with":
        try:
            ce_prep=ce_prepare(dest, task)
        except RuntimeError as e:
            # capture prep evidence then abort
            (dest / "infra_blocked.json").write_text(json.dumps({"error": str(e), "task_id": task_id, "arm": arm}, indent=2), encoding="utf-8")
            print(f"  INFRA_BLOCKED {task_id} {arm}: {e}")
            raise
    # run real opencode
    prompt=task["public_task_prompt"]
    if task["hidden_evaluator"] in prompt:
        raise RuntimeError("leakage: hidden evaluator in prompt")
    print(f"  prompt len {len(prompt)}")
    res=run_opencode_real(prompt, dest, arm=arm, timeout_s=TIMEOUT_S)
    (dest / "raw_opencode_stdout.jsonl").write_text(res["stdout"] or "", encoding="utf-8")
    (dest / "raw_opencode_stderr.txt").write_text(res["stderr"] or "", encoding="utf-8")
    (dest / "raw_opencode_meta.json").write_text(json.dumps({"wall_ms": res["wall_ms"], "returncode": res["returncode"], "timeout": res.get("timeout"), "pid": res.get("pid"), "model": MODEL, "arm": arm, "task_id": task_id}, indent=2), encoding="utf-8")
    parsed=parse_metrics_exact(dest, res["stdout"])
    (dest / "ce_trace.json").write_text(json.dumps({"ce_calls": parsed["ce_calls"], "ce_details": parsed["ce_details"]}, indent=2), encoding="utf-8")
    (dest / "metrics.json").write_text(json.dumps({"wall_ms": res["wall_ms"], "model": MODEL, "tool_counts": parsed["tool_counts"], "native_lookup": parsed["native_lookup"], "ce_calls": parsed["ce_calls"], "input_tokens": parsed["input_tokens"], "output_tokens": parsed["output_tokens"], "tool_output_tokens_cl100k": parsed["tool_output_tokens_cl100k"]}, indent=2), encoding="utf-8")
    try:
        diff=subprocess.check_output(["git","diff","HEAD"], cwd=str(dest), text=True, encoding="utf-8", errors="replace", timeout=10)
    except:
        diff=subprocess.run(["git","diff","HEAD"], cwd=str(dest), capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=10).stdout or ""
    (dest / "final.diff").write_text(diff or "", encoding="utf-8")
    eval_res=run_hidden_evaluator(dest, task)
    (dest / "evaluator.json").write_text(json.dumps(eval_res, indent=2), encoding="utf-8")
    print(f"  wall {res['wall_ms']} timeout {res.get('timeout')} rc {res['returncode']} ce {parsed['ce_calls']} eval {'PASS' if eval_res['pass'] else 'FAIL'}")
    return {"dest": str(dest), "ce_prep": ce_prep, "opencode": res, "parsed": parsed, "evaluator": eval_res}

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
        task_id, arm = args.run_task
        if arm not in ["with","without"]:
            print(f"arm must be with/without, got {arm}")
            sys.exit(2)
        # verify patch SHA and pair identity before run
        manifest=json.loads(MANIFEST.read_text(encoding="utf-8"))
        task=next((t for t in manifest["tasks"] if t["task_id"]==task_id), None)
        if not task:
            print(f"unknown task {task_id}")
            sys.exit(2)
        _verify_patch_sha(task)
        # pair identity: prepare both and compare, but for single run we just ensure current hash matches expected
        # For full check, we prepare both hashes and compare
        try:
            _assert_pair_identity(task)
            print(f"pair identity PASS for {task_id}")
        except Exception as e:
            print(f"pair identity FAIL {e}")
            sys.exit(2)
        res=_run_single_scored(task_id, arm)
        print(f"run-task {task_id} {arm} done eval {res['evaluator']['pass']}")
        sys.exit(0 if res['evaluator']['pass'] else 1)
    if args.run_all:
        manifest=json.loads(MANIFEST.read_text(encoding="utf-8"))
        # frozen order
        order = [("django_02","A_first"),("nestjs_02","B_first"),("ripgrep_02","B_first"),("lodash_02","A_first"),("gin_02","A_first")]
        # verify order matches manifest order and seed
        print(f"frozen order {order}")
        for (tid, side), task in zip(order, manifest["tasks"]):
            assert tid==task["task_id"], f"order mismatch {tid} vs {task['task_id']}"
            _verify_patch_sha(task)
            _assert_pair_identity(task)
        # now run 10 sessions one at a time
        for tid, side in order:
            first = "without" if side=="A_first" else "with"
            second = "with" if first=="without" else "without"
            for arm in [first, second]:
                print(f"\n=== {tid} {arm} ===")
                try:
                    _run_single_scored(tid, arm)
                except RuntimeError as e:
                    print(f"INFRA_BLOCKED {tid} {arm}: {e}")
                    # mark pair blocked, continue
                time.sleep(1)
        print("run-all done")
        sys.exit(0)
    ap.print_help()

if __name__=="__main__":
    main()
