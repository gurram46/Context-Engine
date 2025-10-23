#!/usr/bin/env python3
"""
Context Engine Backend Module
Handles all core Context Engine functionality for the hybrid CLI architecture
"""

import sys
import json
import os
from pathlib import Path

try:
    from context_engine.core.session_tracker import log_cli_command
    TRACKING_AVAILABLE = True
except ImportError:
    TRACKING_AVAILABLE = False

# Add backend to Python path for imports
back_end_dir = Path(__file__).resolve().parent
project_root = back_end_dir.parent
sys.path.insert(0, str(back_end_dir))

# Ensure working directory is always the project root
if Path.cwd() != project_root:
    os.chdir(project_root)

def main():
    """Main backend entry point"""
    if len(sys.argv) < 2:
        print(json.dumps({
            "success": False,
            "error": "No command provided",
            "usage": "python main.py <command> [args...]"
        }))
        sys.exit(1)

    command = sys.argv[1]
    args = sys.argv[2:]

    try:
        # Special handling for summary command
        if command == "summary":
            import asyncio
            from context_engine.core.summary import ProjectSummarizer

            # Parse summary-specific args
            import argparse
            parser = argparse.ArgumentParser()
            parser.add_argument("--model", choices=["claude", "glm", "qwen", "deepseek", "kimi", "langchain", "static"], default="static")
            parser.add_argument("--project-root", type=str)

            # Parse only the relevant args
            summary_args = parser.parse_args(args)

            # Create summarizer and run
            summarizer = ProjectSummarizer(model_choice=summary_args.model)

            # Get project root
            if summary_args.project_root:
                project_root = Path(summary_args.project_root)
            else:
                project_root = Path.cwd()

            # Run summary generation
            summary, summary_path = asyncio.run(summarizer.generate_summary(project_root))

            if TRACKING_AVAILABLE:
                cmd_string = " ".join(["summary"] + args)
                log_cli_command(cmd_string, "")

            print(json.dumps({
                "success": True,
                "summary": summary,
                "summary_path": summary_path,
                "model_used": summary_args.model
            }))
            return

        # Import the Context Engine CLI
        from context_engine.cli import cli

        # Set up click context for command execution
        import click
        from click.testing import CliRunner

        runner = CliRunner()

        # Execute the command with args
        result = runner.invoke(cli, [command] + args)
        command_string = " ".join([command] + args)

        if TRACKING_AVAILABLE:
            if result.exit_code == 0:
                log_cli_command(command_string, "")
            else:
                log_cli_command(command_string, result.output or "Command failed")

        # Output the result
        if result.exit_code == 0:
            print(json.dumps({
                "success": True,
                "output": result.output,
                "exit_code": result.exit_code
            }))
        else:
            print(json.dumps({
                "success": False,
                "error": result.output or "Command failed",
                "exit_code": result.exit_code
            }))
            sys.exit(result.exit_code)

    except ImportError as e:
        print(json.dumps({
            "success": False,
            "error": f"Failed to import Context Engine: {str(e)}"
        }))
        sys.exit(1)
    except Exception as e:
        print(json.dumps({
            "success": False,
            "error": f"Command execution failed: {str(e)}"
        }))
        sys.exit(1)

if __name__ == "__main__":
    main()
