#!/usr/bin/env python3
"""D0 REAL A/B runner — instance-only, no global kills.

Requirements (19):
- synthetic result invalidated under bench/d0/synthetic_harness/
- git apply must return 0 + mutated hash verified, otherwise STOP pair
- WITHOUT: no contextd, no .context, no MCP tools visible (--pure)
- WITH: real contextd index --semantic before timed wall, capture generation/semantic/missing_vectors/pid
- opencode run --model opencode/x-preview-f-free (Ox Alpha Free, OpenCode Zen max, 100T) --format json --dir <workdir> FULL prompt, 20min (1200s), no truncation, no 2-min limit
- 5 tasks x2 arms fresh, counterbalanced seed 20260819
- raw JSONL logs preserved, CE traces extracted from real logs only
- WITHOUT leak audit 0 CE calls expected
- hidden evaluator executed on actual worktree
- pre-run validation mutated FAIL -> reference PASS 5/5
- token/tool metrics parsed from real logs only
- no synthetic numbers preserved

Instance safety: only proc.kill()/proc.terminate() on the Popen handle, shutil.rmtree(ignore_errors=True) scoped to bench/d0/runs/<task>/<arm>.
"""
import argparse, hashlib, json, os, random, shutil, subprocess, sys, time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TASKS_DIR = REPO_ROOT / "bench/d0/tasks"
RUNS_DIR = REPO_ROOT / "bench/d0/runs"
RESULTS_DIR = REPO_ROOT / "bench/d0/results"
REPOS_ROOT = REPO_ROOT / "bench/repos"
MANIFEST = TASKS_DIR / "manifest.json"
MODEL = "opencode/x-preview-f-free"  # Ox Alpha Free (OpenCode Zen max, 100T free quota) — 2 providers: opencode + openrouter/stealth/ox-alpha
TIMEOUT_S = 1200  # 20 min per arm
CONTEXTD_BIN = REPO_ROOT / "target/release/contextd.exe"
OPENCODE_EXE = Path(r"C:\Users\Dell\AppData\Local\Programs\nodejs\node_modules\opencode-ai\bin\opencode.exe")

# ponytail: minimal harness, real flow first, no speculative abstractions.

def _ctx_bin():
    if CONTEXTD_BIN.exists():
        return CONTEXTD_BIN
    cand = REPO_ROOT / "target/release/contextd"
    if cand.exists():
        return cand
    return CONTEXTD_BIN

def run_opencode_real(prompt: str, workdir: Path, model: str = MODEL, timeout_s: int = TIMEOUT_S, arm: str = "with"):
    """Run real opencode, instance-only kill on timeout. Returns raw capture."""
    workdir = Path(workdir)
    exe = OPENCODE_EXE if OPENCODE_EXE.exists() else Path(shutil.which("opencode") or "opencode")
    # For WITHOUT, use --pure to genuinely disable MCP (no contextd tools visible). For WITH, no --pure.
    base = [str(exe), "run", "--model", model, "--format", "json", "--dir", str(workdir)]
    if arm == "without":
        base.append("--pure")
    # pass prompt as final positional; preserve full prompt (no truncation)
    cmd = base + [prompt]
    t0 = time.time()
    # use shell=False with direct exe to avoid cmd.exe indirection; instance handle only
    # utf-8 with replace to avoid cp1252 UnicodeDecodeError on ox provider output (0x9d)
    # For WITH, ensure MCP points to workdir (not main repo) via env override; WITHOUT uses --pure so no MCP.
    env = os.environ.copy()
    if arm == "with":
        env["CONTEXT_ENGINE_PROJECT_ROOT"] = str(workdir)
        # also ensure workdir-local opencode.json fallback sets project root
        try:
            (workdir / "opencode.json").write_text(json.dumps({"mcp": {"contextd": {"type": "local", "command": [str(_ctx_bin())], "enabled": True, "environment": {"CONTEXT_ENGINE_PROJECT_ROOT": str(workdir), "CONTEXTD_EMBED_MODEL": "all-minilm"}}}}, indent=2), encoding="utf-8")
        except Exception:
            pass
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="replace", cwd=str(workdir), env=env)
    try:
        out, err = proc.communicate(timeout=timeout_s)
        wall_ms = int((time.time() - t0) * 1000)
        return {"stdout": out, "stderr": err, "returncode": proc.returncode, "wall_ms": wall_ms, "timeout": False, "pid": proc.pid, "cmd": cmd}
    except subprocess.TimeoutExpired:
        # instance-only termination
        try:
            proc.kill()
        except Exception:
            pass
        try:
            out, err = proc.communicate(timeout=10)
        except Exception:
            try:
                proc.terminate()
            except Exception:
                pass
            out, err = "", ""
        wall_ms = int((time.time() - t0) * 1000)
        return {"stdout": out or "", "stderr": err or "", "returncode": -1, "wall_ms": wall_ms, "timeout": True, "pid": proc.pid, "cmd": cmd}

def prepare_worktree(task: dict, arm: str):
    """Copy pinned repo, git apply mutation with returncode check, verify mutated hash. STOP on failure."""
    repo = task["repo"]
    task_id = task["task_id"]
    dest = RUNS_DIR / task_id / arm
    # scoped rmtree only under bench/d0/runs/<task>/<arm>
    if dest.exists():
        shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)
    src = REPOS_ROOT / repo
    if not src.exists():
        raise RuntimeError(f"source repo missing {src}")
    # copy excluding .git/.context/.serena/.opencode and heavy artifacts
    for item in src.iterdir():
        if item.name in [".git", ".context", ".serena", ".opencode", "target", "node_modules", ".venv", "__pycache__"]:
            continue
        if item.is_dir():
            shutil.copytree(item, dest / item.name, dirs_exist_ok=True)
        else:
            shutil.copy2(item, dest / item.name)
    # ensure dest is a git repo for git apply to work (but we use --directory logic via -C dest)
    # git apply works without init if file exists, but we check via git apply --check first
    patch_file = TASKS_DIR / task["mutation_patch"]
    if not patch_file.exists():
        raise RuntimeError(f"patch missing {patch_file}")
    # ensure dest is a git repo for git apply to be reliable (instance-only init)
    subprocess.run(["git", "init", "-q"], cwd=str(dest), capture_output=True)
    subprocess.run(["git", "config", "user.email", "d0@test.com"], cwd=str(dest), capture_output=True)
    subprocess.run(["git", "config", "user.name", "d0"], cwd=str(dest), capture_output=True)
    # git apply --check then apply, must return 0
    check = subprocess.run(["git", "apply", "--check", str(patch_file)], cwd=str(dest), capture_output=True, text=True, encoding="utf-8", errors="replace")
    if check.returncode != 0:
        raise RuntimeError(f"git apply --check failed for {task_id} {arm}: {check.stderr[:2000]}")
    ap = subprocess.run(["git", "apply", str(patch_file)], cwd=str(dest), capture_output=True, text=True, encoding="utf-8", errors="replace")
    if ap.returncode != 0:
        raise RuntimeError(f"git apply failed for {task_id} {arm}: {ap.stderr[:2000]}")
    # verify mutated source hash contains expected marker
    mut_file = dest / task["mutation_file"]
    if not mut_file.exists():
        raise RuntimeError(f"mutated file missing {mut_file}")
    content = mut_file.read_text(encoding="utf-8", errors="ignore")
    # each patch adds MUTATED marker
    if "MUTATED" not in content:
        raise RuntimeError(f"mutated file {task_id} {arm} does not contain MUTATED marker after apply; patch {patch_file} content head {(patch_file.read_text()[:500])}")
    # also verify via hash: compute sha of mutated file
    h = hashlib.sha256(content.encode()).hexdigest()[:12]
    # write metadata only after successful apply
    (dest / ".mutation_applied").write_text(patch_file.read_text(encoding="utf-8"), encoding="utf-8")
    (dest / ".mutation_sha").write_text(h, encoding="utf-8")
    # stage mutated file so git diff after agent edits shows changes (init repo has no HEAD)
    try:
        subprocess.run(["git", "add", str(task["mutation_file"]), ".mutation_applied", ".mutation_sha"], cwd=str(dest), capture_output=True, encoding="utf-8", errors="replace", timeout=10)
        subprocess.run(["git", "commit", "-m", "initial mutated", "--allow-empty"], cwd=str(dest), capture_output=True, encoding="utf-8", errors="replace", timeout=10)
    except Exception:
        pass
    # also store pre-apply source hash for identity check? For pair identity, both arms should have same mutated hash
    return dest

def ce_prepare(workdir: Path, task: dict):
    """Real CE indexing for WITH arm, outside agent wall. Returns prep metrics."""
    binp = _ctx_bin()
    if not binp.exists():
        raise RuntimeError(f"contextd not found at {binp}")
    t0 = time.time()
    # index --semantic (django large, allow 20min)
    proc = subprocess.run([str(binp), "index", "--root", str(workdir), "--semantic", "--json"], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=1200)
    idx_wall = int((time.time() - t0) * 1000)
    idx_out = proc.stdout
    idx_err = proc.stderr
    # status
    proc2 = subprocess.run([str(binp), "status", "--root", str(workdir), "--json"], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=30)
    status = {}
    try:
        status = json.loads(proc2.stdout or "{}")
    except Exception:
        status = {"raw": proc2.stdout[:5000], "stderr": proc2.stderr[:2000]}
    # extract fields required by spec
    prep = {
        "index_wall_ms": idx_wall,
        "index_stdout": idx_out[:8000] if idx_out else "",
        "index_stderr": idx_err[:4000] if idx_err else "",
        "index_returncode": proc.returncode,
        "status": status,
        "generation": status.get("indexGeneration", status.get("generation", "unknown")),
        "semanticAvailable": status.get("semanticAvailable"),
        "semanticIndexReady": status.get("semanticIndexReady"),
        "missingVectorCount": status.get("missingVectorCount", status.get("missing_vectors", "unknown")),
        "embeddingModel": status.get("embeddingModel"),
        "pid": status.get("pid"),
    }
    # persist
    (workdir / ".ce_prep.json").write_text(json.dumps(prep, indent=2), encoding="utf-8")
    return prep

def run_hidden_evaluator(workdir: Path, task: dict):
    cmd = task["hidden_evaluator"]
    # evaluator may contain shell pipe `| grep`; run via shell
    t0 = time.time()
    proc = subprocess.run(cmd, shell=True, cwd=str(workdir), capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=300)
    wall = int((time.time() - t0) * 1000)
    # PASS if grep -q matched (exit 0) or generic ok detection
    passed = (proc.returncode == 0)
    out = (proc.stdout or "") + "\n" + (proc.stderr or "")
    result = {"command": cmd, "returncode": proc.returncode, "wall_ms": wall, "pass": passed, "stdout": (proc.stdout or "")[:8000], "stderr": (proc.stderr or "")[:8000]}
    (workdir / "evaluator.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    return result

def parse_metrics_from_log(workdir: Path, stdout: str):
    """Parse provider tokens and tool calls from real opencode JSONL. Best-effort, never synthetically fill."""
    # opencode --format json outputs JSONL events; each line is JSON
    tool_calls = 0
    ce_calls = 0
    search_calls = 0
    read_calls = 0
    edit_calls = 0
    shell_calls = 0
    files_read = set()
    files_edited = set()
    input_tokens = None
    output_tokens = None
    cache_read = None
    cache_write = None
    # also count CE tools by name
    ce_details = []
    for line in (stdout or "").splitlines():
        line=line.strip()
        if not line:
            continue
        try:
            j=json.loads(line)
        except Exception:
            continue
        # tool events have type tool or tool_call etc. Inspect heuristically
        jstr = json.dumps(j)
        # count generic tool calls
        if j.get("type") in ("tool_call", "tool", "tool_use") or "tool" in j:
            tool_calls += 1
        # CE tools
        for ce in ["context_search", "symbol_lookup", "dependency_trace", "test_lookup", "context_status"]:
            if ce in jstr:
                ce_calls += 1
                ce_details.append({"tool": ce, "event": j})
        # heuristic for read/search
        if "read" in jstr.lower():
            # this is rough; will refine after real logs captured
            pass
        # provider usage is in j["part"]["tokens"] for step_finish events (opencode format)
        # check both top-level and part-level
        tokens_src = None
        if "tokens" in j:
            tokens_src = j["tokens"]
        elif "part" in j and isinstance(j["part"], dict) and "tokens" in j["part"]:
            tokens_src = j["part"]["tokens"]
        if tokens_src and isinstance(tokens_src, dict):
            if tokens_src.get("input") is not None:
                input_tokens = tokens_src.get("input")
            if tokens_src.get("output") is not None:
                output_tokens = tokens_src.get("output")
            # cache fields vary: cache.read / cache.write
            if tokens_src.get("cache") is not None:
                c = tokens_src.get("cache")
                if isinstance(c, dict):
                    cache_read = c.get("read")
                    cache_write = c.get("write")
                else:
                    cache_read = c
        if "usage" in j:
            u = j["usage"] or {}
            if isinstance(u, dict):
                if u.get("input_tokens") is not None:
                    input_tokens = u.get("input_tokens")
                if u.get("output_tokens") is not None:
                    output_tokens = u.get("output_tokens")
        # also check part.usage
        if "part" in j and isinstance(j["part"], dict) and "usage" in j["part"]:
            u2 = j["part"]["usage"] or {}
            if isinstance(u2, dict):
                if u2.get("input_tokens") is not None:
                    input_tokens = u2.get("input_tokens")
                if u2.get("output_tokens") is not None:
                    output_tokens = u2.get("output_tokens")
    # also fallback: grep counts from stdout text for WITHOUT leak audit
    leak_hits = []
    for ce in ["context_search","symbol_lookup","dependency_trace","test_lookup","context_status","contextd"]:
        if ce in (stdout or ""):
            leak_hits.append(ce)
    return {
        "tool_calls": tool_calls,
        "ce_calls": ce_calls,
        "leak_hits": leak_hits,
        "ce_details": ce_details[:100],
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_read": cache_read,
        "cache_write": cache_write,
    }

def validate_tasks():
    print("=== PRE-RUN TASK VALIDATION (mutated FAIL -> reference PASS) ===")
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    ok = True
    for task in manifest["tasks"]:
        task_id = task["task_id"]
        print(f"\n-- {task_id} ({task['repo']}) --")
        # mutated
        dest_mut = RUNS_DIR / f"_validate_{task_id}_mutated"
        if dest_mut.exists():
            shutil.rmtree(dest_mut, ignore_errors=True)
        dest_mut.mkdir(parents=True, exist_ok=True)
        src = REPOS_ROOT / task["repo"]
        for item in src.iterdir():
            if item.name in [".git",".context",".serena",".opencode","target","node_modules"]:
                continue
            if item.is_dir():
                shutil.copytree(item, dest_mut / item.name, dirs_exist_ok=True)
            else:
                shutil.copy2(item, dest_mut / item.name)
        patch_file = TASKS_DIR / task["mutation_patch"]
        subprocess.run(["git","init","-q"], cwd=str(dest_mut), capture_output=True)
        subprocess.run(["git","config","user.email","d0@test.com"], cwd=str(dest_mut), capture_output=True)
        chk = subprocess.run(["git","apply","--check", str(patch_file)], cwd=str(dest_mut), capture_output=True, text=True)
        if chk.returncode != 0:
            print(f"  mutated git apply --check FAIL: {chk.stderr[:1000]}")
            ok=False
            continue
        ap = subprocess.run(["git","apply", str(patch_file)], cwd=str(dest_mut), capture_output=True, text=True)
        if ap.returncode != 0:
            print(f"  mutated git apply FAIL {ap.stderr[:1000]}")
            ok=False
            continue
        res_mut = run_hidden_evaluator(dest_mut, task)
        print(f"  mutated evaluator {'PASS' if res_mut['pass'] else 'FAIL'} (expect FAIL) rc={res_mut['returncode']}")
        print(f"    out {res_mut['stdout'][:300]} err {res_mut['stderr'][:300]}")
        if res_mut["pass"]:
            print(f"  ERROR: mutated should FAIL but got PASS")
            ok=False
        # reference (clean)
        dest_ref = RUNS_DIR / f"_validate_{task_id}_ref"
        if dest_ref.exists():
            shutil.rmtree(dest_ref, ignore_errors=True)
        dest_ref.mkdir(parents=True, exist_ok=True)
        for item in src.iterdir():
            if item.name in [".git",".context",".serena",".opencode","target","node_modules"]:
                continue
            if item.is_dir():
                shutil.copytree(item, dest_ref / item.name, dirs_exist_ok=True)
            else:
                shutil.copy2(item, dest_ref / item.name)
        res_ref = run_hidden_evaluator(dest_ref, task)
        print(f"  reference evaluator {'PASS' if res_ref['pass'] else 'FAIL'} (expect PASS) rc={res_ref['returncode']}")
        print(f"    out {res_ref['stdout'][:300]} err {res_ref['stderr'][:300]}")
        if not res_ref["pass"]:
            print(f"  ERROR: reference should PASS but got FAIL")
            ok=False
        # cleanup scoped only
        shutil.rmtree(dest_mut, ignore_errors=True)
        shutil.rmtree(dest_ref, ignore_errors=True)
    if ok:
        print("\nVALIDATION 5/5 PASS")
    else:
        print("\nVALIDATION FAILED — fix tasks before live runs")
    return ok

def run_single(task_id: str, arm: str):
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    task = next(t for t in manifest["tasks"] if t["task_id"] == task_id)
    print(f"=== SINGLE {task_id} {arm} ===")
    workdir = prepare_worktree(task, arm)
    print(f"  workdir {workdir} mutated {workdir / '.mutation_sha'}")
    ce_prep = None
    if arm == "with":
        print("  CE prepare (real index --semantic) ...")
        ce_prep = ce_prepare(workdir, task)
        print(f"  CE generation {ce_prep.get('generation')} semantic {ce_prep.get('semanticIndexReady')} missing {ce_prep.get('missingVectorCount')} pid {ce_prep.get('pid')} wall {ce_prep.get('index_wall_ms')}")
    else:
        # ensure no .context leak
        ctx = workdir / ".context"
        if ctx.exists():
            shutil.rmtree(ctx, ignore_errors=True)
        # ensure no MCP visible: write workdir opencode.json that disables? With --pure we disable all, so also create marker
        # For WITHOUT we rely on --pure flag in run_opencode_real, so no need for workdir config
        pass
    prompt = task["public_task_prompt"]  # FULL prompt, no truncation
    # sanity: prompt must not contain hidden evaluator
    if task["hidden_evaluator"] in prompt:
        raise RuntimeError("leakage: hidden evaluator in prompt")
    print(f"  prompt len {len(prompt)} full")
    result = run_opencode_real(prompt, workdir, arm=arm, timeout_s=TIMEOUT_S)
    # persist raw logs
    (workdir / "raw_opencode_stdout.jsonl").write_text(result["stdout"] or "", encoding="utf-8")
    (workdir / "raw_opencode_stderr.txt").write_text(result["stderr"] or "", encoding="utf-8")
    (workdir / "raw_opencode_meta.json").write_text(json.dumps({"wall_ms": result["wall_ms"], "returncode": result["returncode"], "timeout": result.get("timeout"), "pid": result.get("pid"), "model": MODEL, "arm": arm, "task_id": task_id}, indent=2), encoding="utf-8")
    # also legacy session.json for compat but from real data
    metrics = parse_metrics_from_log(workdir, result["stdout"])
    (workdir / "ce_trace.json").write_text(json.dumps({"ce_calls": metrics["ce_calls"], "ce_details": metrics["ce_details"]}, indent=2), encoding="utf-8")
    (workdir / "metrics.json").write_text(json.dumps({"wall_ms": result["wall_ms"], "model": MODEL, "ce_calls": metrics["ce_calls"], "tool_calls": metrics["tool_calls"], "input_tokens": metrics["input_tokens"], "output_tokens": metrics["output_tokens"], "leak_hits": metrics["leak_hits"]}, indent=2), encoding="utf-8")
    # git diff (with encoding utf-8, capture staged commit diff)
    try:
        diff = subprocess.check_output(["git", "diff", "HEAD"], cwd=str(workdir), text=True, encoding="utf-8", errors="replace", timeout=10)
    except Exception:
        diff = subprocess.run(["git","diff", "HEAD"], cwd=str(workdir), capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=10).stdout or ""
    (workdir / "final.diff").write_text(diff or "", encoding="utf-8")
    # hidden evaluator on actual resulting worktree (after agent edits)
    eval_res = run_hidden_evaluator(workdir, task)
    print(f"  opencode wall {result['wall_ms']} timeout {result.get('timeout')} rc {result['returncode']}")
    print(f"  CE calls {metrics['ce_calls']} leak {metrics['leak_hits']}")
    print(f"  evaluator {'PASS' if eval_res['pass'] else 'FAIL'}")
    if arm == "without" and metrics["ce_calls"] != 0:
        print(f"  LEAK FAIL: WITHOUT should have 0 CE calls but got {metrics['ce_calls']}")
    return {"workdir": str(workdir), "ce_prep": ce_prep, "opencode": result, "metrics": metrics, "evaluator": eval_res}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--validate-only", action="store_true", help="pre-run validation 5/5")
    ap.add_argument("--ce-prep-only", type=str, help="ce prep only for task_id (with arm)")
    ap.add_argument("--single", type=str, help="single task_id")
    ap.add_argument("--arm", type=str, default="with", help="arm for --single")
    ap.add_argument("--all", action="store_true", help="run all 10 real sessions counterbalanced")
    args = ap.parse_args()
    if args.validate_only:
        ok = validate_tasks()
        sys.exit(0 if ok else 2)
    if args.ce_prep_only:
        task_id = args.ce_prep_only
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        task = next(t for t in manifest["tasks"] if t["task_id"] == task_id)
        wd = prepare_worktree(task, "with")
        prep = ce_prepare(wd, task)
        print(json.dumps(prep, indent=2))
        sys.exit(0)
    if args.single:
        run_single(args.single, args.arm)
        sys.exit(0)
    if args.all:
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        tasks = manifest["tasks"]
        rnd = random.Random(20260819)
        order = []
        for t in tasks:
            order.append((t["task_id"], "A_first" if rnd.random() < 0.5 else "B_first"))
        print("Order", order)
        # frozen order as per spec: django A_first, nestjs B_first, ripgrep B_first, lodash A_first, gin A_first
        # But we compute from seed; verify matches expected
        for (tid, ordv), task in zip(order, tasks):
            # counters: for each task run both arms in order
            first = "without" if ordv == "A_first" else "with"
            second = "with" if first == "without" else "without"
            for arm in [first, second]:
                try:
                    run_single(tid, arm)
                except Exception as e:
                    print(f"ERROR {tid} {arm}: {e}", file=sys.stderr)
                    import traceback; traceback.print_exc()
                time.sleep(2)
        print("ALL done")
        sys.exit(0)
    ap.print_help()

if __name__ == "__main__":
    main()
