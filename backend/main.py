#!/usr/bin/env python3
"""
Context Engine Backend Module
Handles all core Context Engine functionality for the hybrid CLI architecture
"""

import sys
import json
import subprocess
from pathlib import Path

def main():
    """Main backend entry point"""
    if len(sys.argv) < 2:
        print(json.dumps({"error": "No command provided"}))
        sys.exit(1)

    command = sys.argv[1]

    # Map commands to existing Context Engine functionality
    if command == "init":
        from context_engine.commands.init_command import main as init_main
        init_main()
    elif command == "baseline":
        from context_engine.commands.baseline_command import main as baseline_main
        baseline_main()
    elif command == "bundle":
        from context_engine.commands.bundle_command import main as bundle_main
        bundle_main()
    elif command == "compress":
        from context_engine.compressors.compress_src import main as compress_main
        compress_main()
    elif command == "welcome":
        from welcome_screen import main as welcome_main
        welcome_main()
    else:
        print(json.dumps({"error": f"Unknown command: {command}"}))
        sys.exit(1)

if __name__ == "__main__":
    main()