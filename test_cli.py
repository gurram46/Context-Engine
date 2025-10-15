#!/usr/bin/env python3
"""Simple test script to verify CLI functionality."""

import subprocess
import sys
from pathlib import Path

def test_cli_help():
    """Test that the CLI help command works."""
    try:
        result = subprocess.run(
            [sys.executable, "-m", "context_engine.scripts.cli", "--help"],
            capture_output=True,
            text=True,
            cwd=Path(__file__).parent
        )
        
        print("CLI Help Output:")
        print(result.stdout)
        
        if result.returncode == 0:
            print("✅ CLI help command works!")
            return True
        else:
            print(f"❌ CLI help failed with return code {result.returncode}")
            print(f"Error: {result.stderr}")
            return False
            
    except Exception as e:
        print(f"❌ Error testing CLI: {e}")
        return False

def test_cli_commands():
    """Test that CLI commands are recognized."""
    commands_to_test = [
        "init",
        "reindex", 
        "sync",
        "search",
        "start-session",
        "stop-session",
        "status"
    ]
    
    for command in commands_to_test:
        try:
            result = subprocess.run(
                [sys.executable, "-m", "context_engine.scripts.cli", command, "--help"],
                capture_output=True,
                text=True,
                cwd=Path(__file__).parent
            )
            
            if result.returncode == 0:
                print(f"✅ Command '{command}' recognized")
            else:
                print(f"❌ Command '{command}' failed: {result.stderr}")
                
        except Exception as e:
            print(f"❌ Error testing command '{command}': {e}")

if __name__ == "__main__":
    print("Testing Context Engine CLI...")
    print("=" * 40)
    
    # Test basic help
    help_works = test_cli_help()
    
    print("\nTesting individual commands:")
    print("-" * 30)
    
    # Test command recognition
    test_cli_commands()
    
    print("\n" + "=" * 40)
    if help_works:
        print("✅ CLI basic functionality verified!")
        print("\nTo get started:")
        print("1. Run: python -m context_engine.scripts.cli init")
        print("2. Run: python -m context_engine.scripts.cli --help")
    else:
        print("❌ CLI has issues that need to be resolved")