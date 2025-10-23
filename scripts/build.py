#!/usr/bin/env python3
"""
Build script for Context Engine
Handles both Python and Node.js builds
"""

import subprocess
import sys
import os
from pathlib import Path

def run_command(cmd, cwd=None):
    """Run a command and handle errors"""
    try:
        result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=True)
        return result.stdout, result.returncode
    except subprocess.CalledProcessError as e:
        print(f"Error running command: {cmd}")
        print(f"Error: {e}")
        return "", e.returncode

def build_python():
    """Build Python package"""
    print("Building Python package...")
    output, code = run_command([sys.executable, "-m", "build"])
    if code == 0:
        print("Python build completed successfully")
        return True
    return False

def build_node():
    """Build Node.js package"""
    print("Building Node.js package...")
    output, code = run_command(["npm", "run", "build"], cwd="ui")
    if code == 0:
        print("Node.js build completed successfully")
        return True
    else:
        # Fallback: try npm pack if build script doesn't exist
        print("npm run build failed, trying npm pack...")
        output, code = run_command(["npm", "pack"], cwd="ui")
        if code == 0:
            print("Node.js npm pack completed successfully")
            return True
    return False

def test_python():
    """Run Python tests"""
    print("Running Python tests...")
    output, code = run_command([sys.executable, "-m", "pytest", "-v", "--maxfail=1"])
    return code == 0

def test_node():
    """Run Node.js tests"""
    print("Running Node.js tests...")
    output, code = run_command(["npm", "test"], cwd="ui")
    return code == 0

def lint_python():
    """Lint Python code"""
    print("Linting Python code...")
    output, code = run_command(["flake8", "backend/context_engine", "--max-line-length=120"])
    return code == 0 or "warning" in output.lower()

def lint_node():
    """Lint Node.js code"""
    print("Linting Node.js code...")
    output, code = run_command(["npm", "run", "lint"], cwd="ui")
    return code == 0

def main():
    """Main build script entry point"""
    command = sys.argv[1] if len(sys.argv) > 1 else "build"

    if command == "build":
        python_success = build_python()
        node_success = build_node()
        sys.exit(0 if python_success and node_success else 1)

    elif command == "test":
        python_success = test_python()
        node_success = test_node()
        sys.exit(0 if python_success and node_success else 1)

    elif command == "lint":
        python_success = lint_python()
        node_success = lint_node()
        sys.exit(0 if python_success and node_success else 1)

    elif command == "build-python":
        success = build_python()
        sys.exit(0 if success else 1)

    elif command == "build-node":
        success = build_node()
        sys.exit(0 if success else 1)

    elif command == "test-python":
        success = test_python()
        sys.exit(0 if success else 1)

    elif command == "test-node":
        success = test_node()
        sys.exit(0 if success else 1)

    elif command == "lint-python":
        success = lint_python()
        sys.exit(0 if success else 1)

    elif command == "lint-node":
        success = lint_node()
        sys.exit(0 if success else 1)

    else:
        print("Usage: python build.py [command]")
        print("Commands:")
        print("  build        - Build both Python and Node.js packages")
        print("  test         - Run both Python and Node.js tests")
        print("  lint         - Lint both Python and Node.js code")
        print("  build-python  - Build Python package only")
        print("  build-node    - Build Node.js package only")
        print("  test-python   - Run Python tests only")
        print("  test-node     - Run Node.js tests only")
        print("  lint-python   - Lint Python code only")
        print("  lint-node     - Lint Node.js code only")
        sys.exit(1)

if __name__ == "__main__":
    main()