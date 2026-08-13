#!/usr/bin/env python3
"""Checkout pinned repos shallow at fixed commits.

Reads bench/manifest.json, clones missing repos shallow and checks out pinned SHA.
Safe to re-run: if repo exists and HEAD matches pinned, does nothing.
Uses --depth 1 + fetch of single commit to avoid giant history.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = REPO_ROOT / "bench" / "manifest.json"
REPOS_DIR = REPO_ROOT / "bench" / "repos"


def run(cmd, cwd=None):
    print(f"$ {' '.join(cmd)}", flush=True)
    subprocess.run(cmd, cwd=cwd, check=True)


def main():
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    REPOS_DIR.mkdir(parents=True, exist_ok=True)
    for repo in manifest["repos"]:
        name = repo["name"]
        url = repo["url"]
        commit = repo["commit"]
        dest = REPOS_DIR / name
        if dest.exists():
            # check current HEAD
            try:
                out = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=dest, text=True).strip()
                if out == commit:
                    print(f"{name}: already at {commit[:8]}")
                    continue
                print(f"{name}: exists but at {out[:8]} != {commit[:8]}, fetching pinned")
            except subprocess.CalledProcessError:
                print(f"{name}: exists but not a git repo, re-cloning")
                import shutil

                shutil.rmtree(dest)
                dest.mkdir(parents=True)
        if not dest.exists() or not (dest / ".git").exists():
            print(f"{name}: cloning --no-checkout {url}")
            # clone --no-checkout to allow detached checkout
            run(["git", "clone", "--no-checkout", "--depth", "1", url, str(dest)])
        # fetch pinned commit shallow
        # Try to fetch directly; if already fetched, checkout
        try:
            # check if commit exists locally
            subprocess.check_output(["git", "cat-file", "-e", commit], cwd=dest)
            print(f"{name}: commit {commit[:8]} already fetched")
        except subprocess.CalledProcessError:
            print(f"{name}: fetching {commit[:8]}")
            run(["git", "fetch", "--depth", "1", "origin", commit], cwd=dest)
        run(["git", "checkout", commit], cwd=dest)
        # verify
        out = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=dest, text=True).strip()
        assert out == commit, f"{name} checkout failed: {out} != {commit}"
        print(f"{name}: checked out {commit[:8]}")
    print("All repos at pinned commits.")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"checkout failed: {e}", file=sys.stderr)
        sys.exit(1)
