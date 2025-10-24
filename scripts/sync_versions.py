#!/usr/bin/env python3
"""
Version synchronization script for Context Engine
Keeps setup.py and package.json versions aligned
"""

import json
import sys
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def sync_versions(new_version):
    """Sync versions across setup.py and package.json"""
    new_version = new_version.strip().lstrip('v')

    print(f"Syncing versions to {new_version}")

    # --- Sync setup.py ---
    setup_path = ROOT / "setup.py"
    with setup_path.open("r", encoding="utf-8") as f:
        content = f.read()

    # Use regex to find and replace version line
    pattern = r"version\s*=\s*[\"']([^\"']*)[\"']"
    new_content = re.sub(pattern, f'version="{new_version}"', content)

    if new_content != content:
        with setup_path.open("w", encoding="utf-8") as f:
            f.write(new_content)
        print(f"  Updated setup.py version to {new_version}")
    else:
        print(f"  setup.py version already {new_version}")

    # --- Sync workspace package.json ---
    try:
        workspace_pkg_path = ROOT / "package.json"
        with workspace_pkg_path.open("r+", encoding="utf-8") as f:
            workspace_pkg = json.load(f)
            workspace_old = workspace_pkg.get("version", "")
            workspace_pkg["version"] = new_version

            f.seek(0)
            json.dump(workspace_pkg, f, indent=2)
            f.truncate()

            print(f"  Updated workspace package.json to {new_version} (was {workspace_old})")
    except FileNotFoundError:
        print("  workspace package.json not found")
    except json.JSONDecodeError as exc:
        print(f"  Invalid JSON in package.json: {exc}")
        sys.exit(1)

    # --- Sync ui/package.json ---
    try:
        ui_pkg_path = ROOT / "ui" / "package.json"
        with ui_pkg_path.open("r+", encoding="utf-8") as f:
            pkg = json.load(f)
            old_version = pkg.get("version", "")
            old_name = pkg.get("name")
            name_updated = False
            pkg["version"] = new_version
            if old_name != "context-engine":
                pkg["name"] = "context-engine"
                name_updated = True

            f.seek(0)
            json.dump(pkg, f, indent=2)
            f.truncate()

            print(f"  Updated ui/package.json version to {new_version} (was {old_version})")
            if name_updated:
                previous = old_name if old_name is not None else "<unset>"
                print(f"  Updated package.json name to context-engine (was {previous})")

    except FileNotFoundError:
        print("  ui/package.json not found")
        sys.exit(1)
    except json.JSONDecodeError as exc:
        print(f"  Invalid JSON in ui/package.json: {exc}")
        sys.exit(1)

    # --- Sync package-lock files if present ---
    for lock_rel_path in ["package-lock.json", "ui/package-lock.json"]:
        lock_file = ROOT / lock_rel_path
        if not lock_file.exists():
            continue
        try:
            data = json.loads(lock_file.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            print(f"  Skipping {lock_rel_path} due to invalid JSON: {exc}")
            continue

        original_version = data.get("version")
        data["version"] = new_version

        packages = data.get("packages")
        if isinstance(packages, dict) and "" in packages and isinstance(packages[""], dict):
            packages[""]["version"] = new_version

        lock_file.write_text(json.dumps(data, indent=2), encoding="utf-8")
        print(f"  Updated {lock_rel_path} version to {new_version} (was {original_version})")

    print(f"Version synchronization completed!")

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: python sync_versions.py <version>")
        sys.exit(1)

    sync_versions(sys.argv[1])
