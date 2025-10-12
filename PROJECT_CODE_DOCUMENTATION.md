# Context Engine - Complete Code Documentation

## Project Overview

Context Engine is a CLI-based Context Management tool designed to reduce token waste (20-30%) when starting AI coding tool sessions by preloading relevant project context. It allows developers to bundle project context for AI tools like Claude Code, Cursor, and other AI coding assistants.

## Table of Contents
1. [Project Structure](#project-structure)
2. [Main CLI Interface](#main-cli-interface)
3. [Configuration Management](#configuration-management)
4. [Core Utilities](#core-utilities)
5. [UI Helper Functions](#ui-helper-functions)
6. [Model Integration](#model-integration)
7. [Parser System](#parser-system)
8. [Compression Utilities](#compression-utilities)
9. [LangChain Integration](#langchain-integration)
10. [Utility Scripts](#utility-scripts)
11. [Command Modules](#command-modules)
12. [Setup & Installation](#setup--installation)

## Project Structure

```
context-engine/
├── bin/                           # Executable files
│   └── context-engine.js
├── context_engine/                # Main package directory
│   ├── commands/                 # Command modules
│   │   ├── __init__.py
│   │   ├── add_docs.py
│   │   ├── baseline_commands.py
│   │   ├── bundle_command.py
│   │   ├── checklist.py
│   │   ├── compress_command.py
│   │   ├── config_commands.py
│   │   ├── cross_repo_command.py
│   │   ├── expand_command.py
│   │   ├── export.py
│   │   ├── init.py
│   │   ├── init_command.py
│   │   ├── langchain_cmd.py
│   │   ├── reindex.py
│   │   ├── search.py
│   │   ├── session.py
│   │   ├── session_commands.py
│   │   ├── status.py
│   │   ├── status_command.py
│   │   └── sync.py
│   ├── compressors/              # Compression utilities
│   │   ├── __init__.py
│   │   └── longcodezip_wrapper.py
│   ├── core/                     # Core functionality
│   │   ├── __init__.py
│   │   ├── config.py
│   │   ├── logger.py
│   │   └── utils.py
│   ├── langchain/                # LangChain integration
│   │   ├── __init__.py
│   │   └── enhanced_summarizer.py
│   ├── models/                   # AI model integration
│   │   ├── __init__.py
│   │   └── openrouter.py
│   ├── parsers/                  # Log parsing functionality
│   │   ├── __init__.py
│   │   ├── base_parser.py
│   │   ├── generic_parser.py
│   │   ├── java_parser.py
│   │   ├── js_parser.py
│   │   ├── parser_factory.py
│   │   ├── python_parser.py
│   │   └── ...
│   ├── scripts/                  # Utility scripts
│   │   ├── __init__.py
│   │   ├── auto_capture.py
│   │   ├── cli.py
│   │   ├── embedder.py
│   │   ├── embeddings_store.py
│   │   ├── export.py
│   │   ├── handoff_notes.py
│   │   ├── log_capture.py
│   │   ├── session.py
│   │   ├── summarizer.py
│   │   └── sync.py
│   ├── __init__.py
│   ├── cli.py                    # Main CLI entry point
│   └── ui.py                     # UI helper functions
├── context_engine.egg-info/      # Package metadata
├── scripts/                      # Additional scripts
│   └── postinstall.js
├── temp_longcodezip/             # Temporary directory for longcodezip
│   ├── assets/
│   ├── long-code-completion/
│   ├── module-summarization/
│   ├── repo-qa/
│   ├── __init__.py
│   ├── demo.py
│   ├── longcodezip.py
│   └── README.md
├── tests/                       # Test files
│   ├── __init__.py
│   ├── run_tests.py
│   ├── test_auto_capture.py
│   ├── test_checklist.py
│   ├── test_handoff_notes.py
│   └── test_security.py
├── .gitignore
├── apis.md
├── architecture.md
├── final.txt
├── package.json
├── PROJECT_CODE_DOCUMENTATION.md
├── PROJECT_DOCUMENTATION.txt
├── projectnotes.txt
├── README.md
├── requirements.txt
├── schema.md
├── setup.py
├── test_cli.py
├── tmp_check.py
├── tmp_ls.py
└── WARP.md
```

---

## Main CLI Interface (`context_engine/cli.py`)

```python
"""Main CLI interface for Context Engine"""

# Import required libraries
import click  # Command Line Interface library
from pathlib import Path  # Object-oriented filesystem paths
from importlib import import_module  # Import modules dynamically

# Import subcommand modules explicitly to avoid package import edge cases
init_command = import_module('context_engine.commands.init_command')
baseline_commands = import_module('context_engine.commands.baseline_commands')
session_commands = import_module('context_engine.commands.session_commands')
bundle_command = import_module('context_engine.commands.bundle_command')
expand_command = import_module('context_engine.commands.expand_command')
status_command = import_module('context_engine.commands.status_command')
cross_repo_command = import_module('context_engine.commands.cross_repo_command')
config_commands = import_module('context_engine.commands.config_commands')

# Define the main CLI group using Click
@click.group()
# Add version option - shows version when --version is passed
@click.version_option(version="1.0.0", prog_name="context-engine")
# Option to disable colored output
@click.option("--no-color", is_flag=True, default=False, help="Disable colored output")
# Pass context to store shared options
@click.pass_context
def cli(ctx: click.Context, no_color: bool):
    """Context Engine - Reduce token waste in AI coding sessions"""
    # Store shared options in the context object
    ctx.ensure_object(dict)  # Ensures the context object is a dictionary
    ctx.obj["color"] = not no_color  # Store whether colors should be used (True if not no_color)

# Register all command groups with the main CLI
cli.add_command(init_command.init)  # Initialize Context Engine in current project
cli.add_command(baseline_commands.baseline)  # Manage baseline files group
cli.add_command(session_commands.save)  # Save a note to the current session
cli.add_command(session_commands.session_end)  # End current session with optional AI refresh
cli.add_command(bundle_command.bundle)  # Generate context_for_ai.md bundle
cli.add_command(expand_command.expand)  # Add files mid-session
cli.add_command(status_command.status)  # Show Context Engine status and warnings
cli.add_command(cross_repo_command.pull_cross)  # Pull cross_repo.md from linked repos
cli.add_command(config_commands.config)  # Configuration management commands

def main():
    """Main entry point"""
    cli()  # Execute the CLI

if __name__ == "__main__":
    main()  # If script is run directly, execute main
```

**Functionality**: This file defines the main CLI interface using Click. It imports all command modules and registers them with the main CLI group. The CLI provides commands to initialize, manage baseline files, save session notes, bundle context, expand files mid-session, check status, pull cross-repo notes, and manage configuration.

---

## Configuration Management (`context_engine/core/config.py`)

```python
"""Configuration management for Context Engine"""

# Import required libraries
import os  # Operating System interface
import json  # JSON encoder and decoder
from pathlib import Path  # Object-oriented filesystem paths
from typing import Dict, Any, Optional  # Type hints

class Config:
    """Manages Context Engine configuration"""
    
    # Default configuration dictionary
    DEFAULT_CONFIG = {
        "openrouter_api_key": "",  # API key for OpenRouter service
        "model": "qwen/qwen3-coder:free",  # AI model to use
        "max_tokens": 100000,  # Maximum number of tokens allowed
        "context_dir": ".context",  # Directory to store context files
        "auto_refresh": False,  # Whether to auto-refresh context
        "compression_rules": {  # Rules for compressing context
            "strip_comments": True,  # Remove code comments
            "keep_docstrings": True,  # Keep documentation strings
            "summarize_configs": True,  # Summarize configuration files
            "deduplicate": True,  # Remove duplicate content
            "remove_blank_lines": True  # Remove blank lines
        },
        "linked_repos": [],  # List of linked repository URLs
        # Security-related defaults
        "allowed_extensions": [  # File extensions that are allowed
            ".md", ".json", ".yml", ".yaml", ".toml", ".ini", ".env",
            ".py", ".js", ".ts", ".java", ".c", ".cpp"
        ],
        "max_file_size_kb": 1024,  # Maximum file size in KB
        "note_max_length": 2000  # Maximum length for notes
    }
    
    def __init__(self, project_root: Optional[Path] = None):
        """Initialize configuration with project root"""
        # Set project root - use current working directory if not provided
        self.project_root = project_root or Path.cwd().resolve()
        # Define context directory path
        self.context_dir = self.project_root / ".context"
        # Define configuration file path
        self.config_file = self.context_dir / "config.json"
        # Initialize configuration with defaults
        self._config = self.DEFAULT_CONFIG.copy()
        # Load existing configuration
        self.load()
    
    def load(self) -> None:
        """Load configuration from file if it exists"""
        # Check if config file exists
        if self.config_file.exists():
            try:
                # Open and read configuration file
                with open(self.config_file, 'r') as f:
                    stored_config = json.load(f)  # Load JSON configuration
                    self._config.update(stored_config)  # Update current config with stored values
            except (json.JSONDecodeError, IOError):
                # If there's an error reading the file, continue with defaults
                pass
    
    def save(self) -> None:
        """Save configuration to file"""
        # Create context directory if it doesn't exist
        self.context_dir.mkdir(exist_ok=True)
        # Write configuration to file
        with open(self.config_file, 'w') as f:
            json.dump(self._config, f, indent=2)  # Save with 2-space indentation
    
    def get(self, key: str, default: Any = None) -> Any:
        """Get configuration value"""
        # Return configuration value with default fallback
        return self._config.get(key, default)
    
    def set(self, key: str, value: Any) -> None:
        """Set configuration value"""
        # Update configuration value
        self._config[key] = value
        # Save changes to file
        self.save()
    
    @property
    def openrouter_api_key(self) -> str:
        """Get OpenRouter API key from config or environment"""
        # First check config, then environment variable, then return empty string
        return self.get("openrouter_api_key") or os.getenv("OPENROUTER_API_KEY", "")
    
    @property
    def baseline_dir(self) -> Path:
        """Get baseline directory path"""
        # Return path to baseline directory within context directory
        return self.context_dir / "baseline"
    
    @property
    def adrs_dir(self) -> Path:
        """Get ADRs directory path"""
        # Return path to ADRs (Architecture Decision Records) directory
        return self.context_dir / "adrs"
    
    @property
    def session_file(self) -> Path:
        """Get session file path"""
        # Return path to session notes file
        return self.context_dir / "session.md"
    
    @property
    def cross_repo_file(self) -> Path:
        """Get cross-repo file path"""
        # Return path to cross-repository notes file
        return self.context_dir / "cross_repo.md"
    
    @property
    def context_file(self) -> Path:
        """Get context_for_ai.md file path"""
        # Return path to main context file for AI tools
        return self.context_dir / "context_for_ai.md"
    
    @property
    def hashes_file(self) -> Path:
        """Get hashes.json file path"""
        # Return path to file storing file hashes for staleness checking
        return self.context_dir / "hashes.json"
```

**Functionality**: This class manages all configuration for the Context Engine. It provides default values, loads/stores configuration from JSON files, and defines paths to important directories and files. It includes security features like allowed file extensions and maximum file sizes.

---

## Core Utilities (`context_engine/core/utils.py`)

```python
"""Utility functions for Context Engine"""

# Import required libraries
import hashlib  # Secure hash and message digest algorithm
import json  # JSON encoder and decoder
import math  # Mathematical functions
import re  # Regular expression operations
from pathlib import Path  # Object-oriented filesystem paths
from typing import Dict, List, Optional  # Type hints
from datetime import datetime  # Date and time handling

import tiktoken  # Token counting library

def calculate_file_hash(file_path: Path) -> str:
    """Calculate SHA256 hash of a file"""
    # Create SHA256 hash object
    sha256_hash = hashlib.sha256()
    # Open file in binary mode and read in chunks
    with open(file_path, "rb") as f:
        # Read file in 4KB chunks to handle large files efficiently
        for byte_block in iter(lambda: f.read(4096), b""):
            # Update hash with each chunk
            sha256_hash.update(byte_block)
    # Return hexadecimal representation of hash
    return sha256_hash.hexdigest()

def load_hashes(hashes_file: Path) -> Dict[str, Dict]:
    """Load file hashes from storage"""
    # Check if hashes file exists
    if not hashes_file.exists():
        return {}  # Return empty dictionary if file doesn't exist
    try:
        # Open and read hashes file
        with open(hashes_file, 'r') as f:
            return json.load(f)  # Return loaded JSON data
    except (json.JSONDecodeError, IOError):
        # If there's an error reading the file, return empty dictionary
        return {}

def save_hashes(hashes_file: Path, hashes: Dict[str, Dict]) -> None:
    """Save file hashes to storage"""
    # Create parent directory if it doesn't exist
    hashes_file.parent.mkdir(parents=True, exist_ok=True)
    # Write hashes to file
    with open(hashes_file, 'w') as f:
        json.dump(hashes, f, indent=2)  # Save with 2-space indentation

def check_staleness(file_path: Path, stored_hashes: Dict[str, Dict]) -> bool:
    """Check if a file has changed since last hash"""
    # Convert Path to string for dictionary lookup
    str_path = str(file_path)
    # Return False if file not found in stored hashes (new file)
    if str_path not in stored_hashes:
        return False
    
    # Calculate current hash of file
    current_hash = calculate_file_hash(file_path)
    # Compare with stored hash - return True if different (stale)
    return stored_hashes[str_path].get("hash") != current_hash

def update_hash(file_path: Path, stored_hashes: Dict[str, Dict]) -> None:
    """Update hash for a file"""
    # Convert Path to string for dictionary key
    str_path = str(file_path)
    # Update stored hashes with new hash and timestamp
    stored_hashes[str_path] = {
        "hash": calculate_file_hash(file_path),  # Calculate and store new hash
        "updated": datetime.now().isoformat()  # Store current datetime in ISO format
    }

def count_tokens(text: str, model: str = "gpt-4") -> int:
    """Count tokens in text using tiktoken"""
    try:
        # Get encoding for the specified model
        encoding = tiktoken.encoding_for_model(model)
    except KeyError:
        # Fall back to common encoding if model not found
        encoding = tiktoken.get_encoding("cl100k_base")
    # Encode the text and return token count
    return len(encoding.encode(text))

def _shannon_entropy(s: str) -> float:
    """Compute Shannon entropy of a string"""
    # Return 0 if string is empty
    if not s:
        return 0.0
    # Calculate probability of each character
    prob = [float(s.count(c)) / len(s) for c in set(s)]
    # Return negative sum of p * log2(p) for each character probability
    return -sum(p * math.log(p, 2) for p in prob)

def is_high_entropy_token(token: str) -> bool:
    """Heuristic to detect likely secrets by entropy and length"""
    # Clean token by removing quotes and whitespace
    token = token.strip().strip('"\')'
    # Return False if token is too short to be a secret
    if len(token) < 20:
        return False
    # Calculate entropy of the token
    entropy = _shannon_entropy(token)
    # Return True if entropy >= threshold (indicating likely secret)
    return entropy >= 3.5  # heuristic threshold

def redact_secrets(text: str) -> str:
    """Redact potential secrets from text using regex and entropy detection"""
    # First, handle specific known secret patterns (order matters!)
    # Match sk- keys with or without quotes
    text = re.sub(r'="([A-Za-z0-9\\-_]{20,})"', r'="[REDACTED_KEY]"', text)
    text = re.sub(r"='([A-Za-z0-9\\-_]{20,})'", r"='[REDACTED_KEY]'", text)
    text = re.sub(r'=([A-Za-z0-9\\-_]{20,})(?=\s|$)', r'=[REDACTED_KEY]', text)
    text = re.sub(r"([A-Za-z0-9\\-_]{20,})", r"[REDACTED_KEY]", text)
    
    # AWS keys
    text = re.sub(r'([\"\\']?)(AKIA[0-9A-Z]{16})([\"\\']?)', r'\1[REDACTED_AWS]\3', text)
    
    # Generic patterns for other secrets
    text = re.sub(r'(password|passwd|pwd|pass)\s*[=:]?\s*[\"\\']?([^\"\\'\s]+)[\"\\']?', r'\1=[REDACTED]', text, flags=re.IGNORECASE)
    text = re.sub(r'(token|jwt|bearer)\s*[=:]?\s*[\"\\']?([^\"\\'\s]+)[\"\\']?', r'\1=[REDACTED]', text, flags=re.IGNORECASE)
    
    # Environment variables - skip if already redacted
    if '[REDACTED_KEY]' not in text:
        text = re.sub(r'(API_KEY|SECRET|TOKEN|PASSWORD|PASSWD)\s*=\s*[\"\\']?([^\"\\'\s]+)[\"\\']?', r'\1=[REDACTED]', text)

    # Entropy-based redaction only for standalone hex-like strings (not variable names)
    def _mask_high_entropy(match: re.Match) -> str:
        token = match.group(0)
        # Skip if it looks like a variable name (contains underscores in middle)
        if '_' in token[1:-1] and not token.startswith('sk-'):
            return token
        return "[REDACTED]" if is_high_entropy_token(token) else token

    # Only match hex-like strings, not typical variable names
    text = re.sub(r'\b[a-fA-F0-9]{32,}\b', _mask_high_entropy, text)
    return text

def strip_comments(code: str, language: str = "python") -> str:
    """Strip inline comments from code while preserving docstrings"""
    if language in ["python", "py"]:
        # Remove single-line comments but keep docstrings
        lines = code.split('\n')
        result = []
        in_docstring = False
        docstring_char = None
        
        for line in lines:
            stripped = line.strip()
            
            # Check for docstring start/end
            if '"""' in line or "'''" in line:
                if not in_docstring:
                    in_docstring = True
                    docstring_char = '"""' if '"""' in line else "'''"
                elif docstring_char in line:
                    in_docstring = False
                    docstring_char = None
                result.append(line)
            elif in_docstring:
                result.append(line)
            else:
                # Remove inline comments
                if '#' in line:
                    code_part = line.split('#')[0].rstrip()
                    if code_part:
                        result.append(code_part)
                    elif not code_part and line.strip().startswith('#'):
                        continue
                else:
                    result.append(line)
        
        return '\n'.join(result)
    
    elif language in ["javascript", "js", "typescript", "ts", "java", "c", "cpp"]:
        # Remove // comments and /* */ comments
        # Keep /** */ documentation comments
        # Preserve JSDoc-style comments
        jsdoc_blocks = re.findall(r'/\*\*[^*]*\*+(?:[^/*][^*]*\*+)*/', code, flags=re.DOTALL)
        code = re.sub(r'//.*$', '', code, flags=re.MULTILINE)
        code = re.sub(r'/\*(?!\*)[^*]*\*+(?:[^/*][^*]*\*+)*/', '', code)
        # Reattach JSDoc blocks at the top to preserve docstrings
        return "\n".join([b for b in jsdoc_blocks if b.strip()])
    
    return code

def summarize_config(config_text: str) -> str:
    """Summarize configuration file without secrets"""
    config_text = redact_secrets(config_text)  # Redact any secrets first
    
    lines = config_text.split('\n')
    summary = []
    
    for line in lines:
        stripped = line.strip()
        # Skip empty lines and comments
        if not stripped or stripped.startswith('#'):
            continue
        
        # Keep structure indicators
        if any(char in stripped for char in ['{', '}', '[', ']']):
            summary.append(line)
        # Summarize value lines
        elif '=' in stripped or ':' in stripped:
            key_part = stripped.split('=' if '=' in stripped else ':')[0].strip()
            summary.append(f"{key_part}: [configured]")
    
    return '\n'.join(summary)

def deduplicate_content(content: str) -> str:
    """Remove duplicate patterns from content"""
    lines = content.split('\n')
    seen = set()
    result = []
    
    for line in lines:
        stripped = line.strip()
        if stripped and stripped not in seen:
            seen.add(stripped)
            result.append(line)
        elif not stripped:
            # keep single blank lines only
            if result and result[-1].strip() == "":
                continue
            result.append("")
    
    return '\n'.join(result)

def compress_whitespace(text: str) -> str:
    """Remove excessive blank lines and trailing whitespace"""
    lines = [l.rstrip() for l in text.split('\n')]  # Remove trailing whitespace from each line
    comp = []
    for l in lines:
        if l.strip() == "":
            if comp and comp[-1] == "":  # Skip consecutive blank lines
                continue
            comp.append("")
        else:
            comp.append(l)
    return '\n'.join(comp)

def is_subpath(child: Path, parent: Path) -> bool:
    """Check if child path is within parent directory"""
    try:
        child = child.resolve(strict=False)  # Resolve to absolute path
        parent = parent.resolve(strict=False)  # Resolve to absolute path
        # Handle Windows paths properly
        return child == parent or parent in child.parents
    except Exception:
        return False

def validate_path_in_project(path: Path, project_root: Path) -> None:
    """Raise click.BadParameter if path escapes project root"""
    from click import BadParameter
    if not is_subpath(path, project_root):
        raise BadParameter(f"Path '{path}' is outside the project root: {project_root}")

def sanitize_note_input(note: str, max_len: int = 2000) -> str:
    """Sanitize note content and enforce max length"""
    # remove control characters except common whitespace
    note = re.sub(r'[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]', '', note)
    if len(note) > max_len:
        note = note[:max_len] + "…"
    return note

def extract_api_docstrings(code: str, language: str = "python") -> str:
    """Extract only API docstrings/comments and signatures, not raw code"""
    if language in ["python", "py"]:
        # Keep only triple-quoted docstrings
        blocks = re.findall(r'("""[\s\S]*?"""|\'\'\'[\s\S]*?\'\'\')', code)
        return "\n\n".join(b.strip() for b in blocks if b.strip()) or "(no docstrings)"
    elif language in ["javascript", "js", "typescript", "ts"]:
        blocks = re.findall(r'/\*\*[^*]*\*+(?:[^/*][^*]*\*+)*/', code, flags=re.DOTALL)
        return "\n\n".join(b.strip() for b in blocks if b.strip()) or "(no API docs)"
    else:
        return "(no API docs)"

def compress_code(code: str, language: str = "python") -> str:
    """Strict compression: Keep docstrings only, remove comments/whitespace"""
    doc_only = extract_api_docstrings(code, language)
    doc_only = compress_whitespace(doc_only)
    return doc_only

def is_valid_api_key(key: str) -> bool:
    """Basic format validation for API key (never log key)"""
    if not key or not isinstance(key, str):
        return False
    key = key.strip()
    # Accept keys like sk-... with proper format
    if key.startswith("sk-"):
        if len(key) >= 32:
            # Ensure no special chars except dash and underscore  
            return bool(re.match(r'^sk-[A-Za-z0-9_\-]+$', key))
        return False
    # Test keys starting with "test-" are invalid
    if key.startswith("test-"):
        return False
    # For non-sk keys, require only alphanumeric and simple chars
    if len(key) >= 32:
        # Reject if contains special chars like @ # $
        if re.search(r'[@#$%^&*()+=\[\]{};\':"<>?,/\\|`~]', key):
            return False
        return bool(re.match(r'^[A-Za-z0-9_\-]+$', key))
    return False
```

**Functionality**: This module contains essential utility functions for the Context Engine. It includes functions for file hashing, token counting, secret redaction, code compression, path validation, and more. These utilities support the core functionality of the Context Engine.

---

## UI Helper Functions (`context_engine/ui.py`)

```python
"""Tiny UI helper around Click for consistent, optional colors."""

import click  # Command Line Interface library

def _color_param():
    """Return Click color parameter: False to force off, None to auto."""
    try:
        # Get current Click context
        ctx = click.get_current_context(silent=True)
        # Check if color is explicitly disabled in context
        if ctx and isinstance(ctx.obj, dict) and ctx.obj.get("color") is False:
            return False
    except Exception:
        pass
    # None lets Click auto-detect color support
    return None

def success(message: str) -> None:
    """Print success message in green color"""
    click.secho(message, fg="green", color=_color_param())

def info(message: str) -> None:
    """Print info message in cyan color"""
    click.secho(message, fg="cyan", color=_color_param())

def warn(message: str) -> None:
    """Print warning message in yellow color"""
    click.secho(message, fg="yellow", color=_color_param())

def error(message: str) -> None:
    """Print error message in red color"""
    click.secho(message, fg="red", color=_color_param())
```

**Functionality**: This module provides consistent UI helper functions for displaying colored messages to the user. It integrates with Click's context to respect user preferences for colored output.

---

## Model Integration (`context_engine/models/openrouter.py`)

```python
"""OpenRouter integration for Qwen3 Coder model"""

import json  # JSON encoder and decoder
import requests  # HTTP library
from typing import Optional, Dict, Any  # Type hints

class OpenRouterClient:
    """Client for OpenRouter API"""
    
    SYSTEM_PROMPT = (
        "You are the Context Engine formatter.\n\n"
        "Always output `.context/context_for_ai.md` exactly in this structure and order:\n"
        "## Architecture\n## APIs\n## Configuration\n## Database Schema\n## Session Notes\n## Cross-Repo Notes\n## Expanded Files\n\n"
        "Rules:\n"
        "- Apply strict compression: strip all inline code comments, keep API docstrings only.\n"
        "- Summarize configs without secrets.\n"
        "- Remove blank lines and extra whitespace.\n"
        "- Deduplicate repetitive patterns.\n"
        "- Include all headings even if empty (use 'None' when empty).\n"
        "- Do not re-order or rename sections.\n"
        "- Never include raw, uncompressed code.\n"
        "- Always output valid Markdown.\n"
    )
    
    def __init__(self, api_key: str):
        # Validate API key before initialization
        from ..core.utils import is_valid_api_key
        # Set API key if valid, otherwise empty string
        self.api_key = api_key if is_valid_api_key(api_key) else ""
        self.base_url = "https://openrouter.ai/api/v1"  # API base URL
        self.model = "qwen/qwen3-coder:free"  # Default model to use
    
    def summarize(self, content: str, task: str = "summarize") -> Optional[str]:
        """Summarize content using Qwen3 Coder"""
        # Return None if no API key is available
        if not self.api_key:
            return None
        
        # Prepare headers for API request
        headers = {
            "Authorization": f"Bearer {self.api_key}",  # Authentication header
            "Content-Type": "application/json",  # Content type
            "HTTP-Referer": "https://github.com/context-engine",  # Referer header
            "X-Title": "Context Engine"  # Title header
        }
        
        # Build prompt based on task type
        prompt = self._build_prompt(content, task)
        
        # Prepare data for API request
        data = {
            "model": self.model,  # AI model to use
            "messages": [  # Conversation messages
                {"role": "system", "content": self.SYSTEM_PROMPT},  # System instructions
                {"role": "user", "content": prompt}  # User content
            ],
            "temperature": 0.2,  # Creativity control (low for consistency)
            "max_tokens": 4000  # Maximum response tokens
        }
        
        try:
            # Make API request to OpenRouter
            response = requests.post(
                f"{self.base_url}/chat/completions",  # API endpoint
                headers=headers,  # Request headers
                json=data,  # Request data
                timeout=(10, 30),  # Connect and read timeouts
            )
            # Raise exception if request failed
            response.raise_for_status()
            
            # Parse response JSON
            result = response.json()
            # Extract and return the AI's response content
            return result.get("choices", [{}])[0].get("message", {}).get("content", "")
        
        except (requests.RequestException, json.JSONDecodeError, KeyError):
            # Do not leak implementation details
            return None
    
    def _build_prompt(self, content: str, task: str) -> str:
        """Build prompt based on task type"""
        if task == "summarize":
            return (
                "Compress this source file for context bundling. Keep only API docstrings;"
                " remove comments and blank lines. Do not include raw code.\n\n"
                f"Content:\n{content}"
            )
        
        elif task == "compress":
            return (
                "Compress this configuration file: remove secrets, summarize values, keep structure.\n\n"
                f"Content:\n{content}"
            )
        
        elif task == "bundle":
            return (
                "Create a `.context/context_for_ai.md` using the fixed section order and rules."
                " Include all sections with 'None' when empty.\n\n"
                f"Content:\n{content}"
            )
        
        else:
            return content
    
    def generate_fixed_context_bundle(self, *, architecture: str, apis: str, configuration: str,
                                      schema: str, session: str, cross_repo: str, expanded: str) -> str:
        """Generate the final context_for_ai.md content in the fixed structure"""
        # Build content with fixed structure
        content = (
            "## Architecture\n" + architecture + "\n\n"
            "## APIs\n" + apis + "\n\n"
            "## Configuration\n" + configuration + "\n\n"
            "## Database Schema\n" + schema + "\n\n"
            "## Session Notes\n" + session + "\n\n"
            "## Cross-Repo Notes\n" + cross_repo + "\n\n"
            "## Expanded Files\n" + expanded + "\n"
        )
        # Try to get AI-processed version
        ai_version = self.summarize(content, task="bundle")
        if not ai_version:
            # Fallback to manual fixed content
            return "# Project Context for AI Tools\n\n" + content
        return ai_version
```

**Functionality**: This class provides integration with OpenRouter's API to use the Qwen3 Coder model for context summarization and processing. It handles API requests, prompt building, and response processing to create optimized context files for AI tools.

---

## Parser System (`context_engine/parsers/base_parser.py`)

```python
"""Base parser class for Context Engine log parsers."""

import json  # JSON encoder and decoder
import re  # Regular expression operations
from abc import ABC, abstractmethod  # Abstract base class
from datetime import datetime  # Date and time handling
from pathlib import Path  # Object-oriented filesystem paths
from typing import Dict, List, Optional, Union  # Type hints

class BaseParser(ABC):
    """Abstract base class for log parsers."""
    
    def __init__(self, parser_type: str):
        """Initialize the parser with a type"""
        self.parser_type = parser_type  # Type of parser (e.g., 'python', 'java')
        self.supported_extensions = []  # List of supported file extensions
        self.error_patterns = []  # List of error patterns to match
    
    @abstractmethod
    def parse_log_content(self, content: str, source_file: Optional[Path] = None) -> List[Dict]:
        """Parse log content and extract structured error information.
        
        This method must be implemented by subclasses.
        
        Args:
            content: Raw log content to parse
            source_file: Optional source file path for context
            
        Returns:
            List of parsed error dictionaries with unified format
        """
        pass
    
    @abstractmethod
    def can_parse(self, content: str, file_extension: Optional[str] = None) -> bool:
        """Check if this parser can handle the given content.
        
        This method must be implemented by subclasses.
        
        Args:
            content: Log content to check
            file_extension: Optional file extension hint
            
        Returns:
            True if this parser can handle the content
        """
        pass
    
    def create_error_entry(self, 
                          message: str,
                          file_hint: Optional[str] = None,
                          line_hint: Optional[int] = None,
                          traceback: Optional[str] = None,
                          error_type: Optional[str] = None,
                          severity: str = 'error',
                          context: Optional[Dict] = None) -> Dict:
        """Create a standardized error entry.
        
        Args:
            message: Error message
            file_hint: File where error occurred
            line_hint: Line number where error occurred
            traceback: Full traceback/stack trace
            error_type: Type of error (e.g., 'SyntaxError', 'NullPointerException')
            severity: Error severity ('error', 'warning', 'info')
            context: Additional context information
            
        Returns:
            Standardized error dictionary
        """
        return {
            'message': message,  # Error message text
            'file_hint': file_hint,  # File where error occurred
            'line_hint': line_hint,  # Line number where error occurred
            'traceback': traceback,  # Full traceback if available
            'error_type': error_type,  # Type of error
            'severity': severity,  # Severity level
            'parser_type': self.parser_type,  # Type of parser that found the error
            'timestamp': datetime.now().isoformat(),  # When the error was found
            'context': context or {}  # Additional context information
        }
    
    def extract_file_and_line(self, text: str) -> tuple[Optional[str], Optional[int]]:
        """Extract file path and line number from text.
        
        Args:
            text: Text to search for file and line information
            
        Returns:
            Tuple of (file_path, line_number) or (None, None)
        """
        # Common patterns for file:line references
        patterns = [
            r'"([^\"]+)",\s*line\s*(\d+)',  # "file.py", line 123
            r'([^\s]+\.w+):(\d+)',  # file.py:123
            r'File "([^\"]+)", line (\d+)',  # File "file.py", line 123
            r'at ([^\s]+):(\d+)',  # at file.java:123
            r'([^\s]+)\((\d+)\)',  # file.js(123)
        ]
        
        for pattern in patterns:
            match = re.search(pattern, text, re.IGNORECASE)
            if match:
                file_path = match.group(1)
                try:
                    line_num = int(match.group(2))
                    return file_path, line_num
                except ValueError:
                    continue
        
        return None, None
    
    def clean_message(self, message: str) -> str:
        """Clean and normalize error message.
        
        Args:
            message: Raw error message
            
        Returns:
            Cleaned error message
        """
        # Remove ANSI color codes
        ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\\[[0-?]*[ -/]*[@-~])')
        message = ansi_escape.sub('', message)
        
        # Remove excessive whitespace
        message = re.sub(r'\s+', ' ', message).strip()
        
        return message
    
    def save_parsed_errors(self, errors: List[Dict], output_file: Path) -> bool:
        """Save parsed errors to JSON file.
        
        Args:
            errors: List of parsed error dictionaries
            output_file: Output file path
            
        Returns:
            True if successful, False otherwise
        """
        try:
            # Ensure output directory exists
            output_file.parent.mkdir(parents=True, exist_ok=True)
            
            # Save errors as JSON
            with open(output_file, 'w', encoding='utf-8') as f:
                json.dump({
                    'parser_type': self.parser_type,  # Type of parser
                    'parsed_at': datetime.now().isoformat(),  # When parsing occurred
                    'error_count': len(errors),  # Number of errors found
                    'errors': errors  # List of parsed errors
                }, f, indent=2, ensure_ascii=False)  # Save with 2-space indentation
            
            return True
            
        except Exception as e:
            print(f"Failed to save parsed errors: {e}")
            return False
```

**Functionality**: This is the abstract base class for all log parsers in the Context Engine. It defines the interface that all parsers must implement and provides common functionality for creating standardized error entries, extracting file/line information, and saving parsed errors.

---

## Compression Utilities (`context_engine/compressors/longcodezip_wrapper.py`)

```python
"""Wrapper for longcodezip compression functionality."""

import os
import sys
import subprocess
import tempfile
import shutil
from pathlib import Path
from typing import Optional, List, Dict, Any

class LongcodezipWrapper:
    """Wrapper class for the longcodezip compression tool."""
    
    def __init__(self, tool_path: Optional[Path] = None):
        """Initialize the wrapper with path to longcodezip tool."""
        self.tool_path = tool_path or Path(__file__).parent.parent / "temp_longcodezip" / "longcodezip.py"
        
    def compress_file(self, input_file: Path, output_file: Optional[Path] = None) -> Path:
        """Compress a single file using longcodezip."""
        if not self.tool_path.exists():
            raise FileNotFoundError(f"Longcodezip tool not found: {self.tool_path}")
            
        if not input_file.exists():
            raise FileNotFoundError(f"Input file not found: {input_file}")
            
        # Generate output file if not provided
        if output_file is None:
            output_file = input_file.with_suffix(input_file.suffix + ".compressed")
            
        # Create command to run longcodezip
        cmd = [sys.executable, str(self.tool_path), str(input_file), str(output_file)]
        
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, check=True)
            print(f"Compression successful: {result.stdout}")
            return output_file
        except subprocess.CalledProcessError as e:
            print(f"Compression failed: {e.stderr}")
            raise e
            
    def compress_directory(self, input_dir: Path, output_file: Path) -> Path:
        """Compress an entire directory using longcodezip."""
        if not self.tool_path.exists():
            raise FileNotFoundError(f"Longcodezip tool not found: {self.tool_path}")
            
        if not input_dir.exists() or not input_dir.is_dir():
            raise FileNotFoundError(f"Input directory not found or not a directory: {input_dir}")
            
        # Create command to run longcodezip on directory
        cmd = [sys.executable, str(self.tool_path), str(input_dir), str(output_file)]
        
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, check=True)
            print(f"Directory compression successful: {result.stdout}")
            return output_file
        except subprocess.CalledProcessError as e:
            print(f"Directory compression failed: {e.stderr}")
            raise e
            
    def decompress_file(self, compressed_file: Path, output_dir: Path) -> Path:
        """Decompress a file using longcodezip."""
        if not compressed_file.exists():
            raise FileNotFoundError(f"Compressed file not found: {compressed_file}")
            
        # Create command to decompress
        cmd = [sys.executable, str(self.tool_path), "-d", str(compressed_file), str(output_dir)]
        
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, check=True)
            print(f"Decompression successful: {result.stdout}")
            return output_dir
        except subprocess.CalledProcessError as e:
            print(f"Decompression failed: {e.stderr}")
            raise e
```

**Functionality**: This wrapper provides an interface to the longcodezip compression tool, which is used for handling large code files and directories. It allows for compressing single files or entire directories, as well as decompressing compressed files back to their original form.

---

## LangChain Integration (`context_engine/langchain/enhanced_summarizer.py`)

```python
"""Enhanced summarizer using LangChain for better context understanding."""

from typing import List, Dict, Any, Optional
import logging
from pathlib import Path

try:
    from langchain_openai import ChatOpenAI
    from langchain_core.prompts import ChatPromptTemplate
    from langchain_core.output_parsers import StrOutputParser
    LANGCHAIN_AVAILABLE = True
except ImportError:
    LANGCHAIN_AVAILABLE = False
    logging.warning("LangChain not available. Enhanced summarization features will be disabled.")

class EnhancedSummarizer:
    """Enhanced summarizer using LangChain for better context understanding."""
    
    def __init__(self, api_key: Optional[str] = None, model: str = "gpt-3.5-turbo"):
        """Initialize the enhanced summarizer."""
        if not LANGCHAIN_AVAILABLE:
            self.available = False
            logging.warning("LangChain not available. Enhanced summarization features will be disabled.")
            return
            
        self.available = True
        self.model = ChatOpenAI(
            model=model,
            api_key=api_key
        )
        
        # Create prompt template for summarization
        self.summarize_prompt = ChatPromptTemplate.from_messages([
            ("system", "You are an expert code summarizer. Create a concise summary of the provided code context while preserving important information like API signatures, configuration settings, and architectural decisions."),
            ("user", "Please summarize the following code context:\n\n{context}\n\nFocus on key functionality, main components, and any important configuration or architectural decisions.")
        ])
        
        self.output_parser = StrOutputParser()
        self.summarize_chain = self.summarize_prompt | self.model | self.output_parser
        
    def summarize_content(self, content: str) -> Optional[str]:
        """Summarize the provided content using LangChain."""
        if not self.available:
            return None
            
        try:
            summary = self.summarize_chain.invoke({"context": content})
            return summary
        except Exception as e:
            logging.error(f"Error during LangChain summarization: {e}")
            return None
            
    def batch_summarize(self, content_list: List[str]) -> List[Optional[str]]:
        """Summarize multiple content items."""
        if not self.available:
            return [None for _ in content_list]
            
        summaries = []
        for content in content_list:
            summary = self.summarize_content(content)
            summaries.append(summary)
        return summaries
```

**Functionality**: This module provides enhanced summarization capabilities using LangChain, which can be used as an alternative to the OpenRouter-based summarization. It allows for more sophisticated processing of code context and can provide better understanding of architectural decisions and important code components.

---

## Utility Scripts (`context_engine/scripts/`)

The Context Engine includes a collection of utility scripts that provide additional functionality for context management, embedding generation, and other specialized tasks.

### Auto Capture (`context_engine/scripts/auto_capture.py`)

```python
"""Automatically capture code changes and context updates."""

import os
import time
import json
from pathlib import Path
from typing import Dict, List, Optional
import subprocess

from watchdog.observers import Observer
from watchdog.events import FileSystemEventHandler

class AutoCaptureHandler(FileSystemEventHandler):
    """Handle file system events for automatic context capture."""
    
    def __init__(self, project_root: Path, callback_func=None):
        self.project_root = project_root
        self.callback_func = callback_func
        self.ignored_extensions = {'.pyc', '.pyo', '.tmp', '.log', '.git'}
        
    def on_modified(self, event):
        """Handle file modification events."""
        if event.is_directory:
            return
            
        file_path = Path(event.src_path)
        if file_path.suffix in self.ignored_extensions:
            return
            
        # Check if file is within project root
        try:
            file_path.relative_to(self.project_root)
        except ValueError:
            return  # File is outside project root
            
        if self.callback_func:
            self.callback_func(file_path)
            
    def on_created(self, event):
        """Handle file creation events."""
        if event.is_directory:
            return
            
        file_path = Path(event.src_path)
        if file_path.suffix in self.ignored_extensions:
            return
            
        # Check if file is within project root
        try:
            file_path.relative_to(self.project_root)
        except ValueError:
            return  # File is outside project root
            
        if self.callback_func:
            self.callback_func(file_path)

class AutoCapture:
    """Automatically capture changes in the project."""
    
    def __init__(self, project_root: Optional[Path] = None):
        self.project_root = project_root or Path.cwd()
        self.observer = Observer()
        self.handler = AutoCaptureHandler(self.project_root, self._on_file_change)
        
    def _on_file_change(self, file_path: Path):
        """Callback for when a file changes."""
        print(f"File changed: {file_path}")
        # Add logic to update context based on file changes
        
    def start_monitoring(self):
        """Start monitoring the project directory for changes."""
        self.observer.schedule(self.handler, str(self.project_root), recursive=True)
        self.observer.start()
        
    def stop_monitoring(self):
        """Stop monitoring the project directory."""
        self.observer.stop()
        self.observer.join()
        
    def run(self):
        """Run the auto capture service."""
        print(f"Starting auto capture for {self.project_root}")
        self.start_monitoring()
        
        try:
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            print("Stopping auto capture...")
            self.stop_monitoring()
```

**Functionality**: The auto capture script monitors the project directory for file changes and automatically captures these changes as context updates, allowing for dynamic context management as the codebase evolves.

### Embedder (`context_engine/scripts/embedder.py`)

```python
"""Script to generate embeddings for context files."""

from typing import List, Dict, Any
import numpy as np
from pathlib import Path

try:
    from sentence_transformers import SentenceTransformer
    EMBEDDINGS_AVAILABLE = True
except ImportError:
    EMBEDDINGS_AVAILABLE = False
    print("Sentence transformers not available. Embedding generation will be disabled.")

class Embedder:
    """Generate embeddings for context files."""
    
    def __init__(self, model_name: str = "all-MiniLM-L6-v2"):
        if EMBEDDINGS_AVAILABLE:
            self.model = SentenceTransformer(model_name)
            self.available = True
        else:
            self.available = False
            
    def generate_embedding(self, text: str) -> Optional[List[float]]:
        """Generate embedding for a text."""
        if not self.available:
            return None
            
        embedding = self.model.encode(text)
        return embedding.tolist()
        
    def generate_embeddings(self, texts: List[str]) -> List[Optional[List[float]]]:
        """Generate embeddings for multiple texts."""
        if not self.available:
            return [None for _ in texts]
            
        embeddings = self.model.encode(texts)
        return [emb.tolist() for emb in embeddings]
        
    def similarity(self, emb1: List[float], emb2: List[float]) -> float:
        """Calculate cosine similarity between two embeddings."""
        if not self.available:
            return 0.0
            
        emb1_np = np.array(emb1)
        emb2_np = np.array(emb2)
        
        # Calculate cosine similarity
        cosine_sim = np.dot(emb1_np, emb2_np) / (np.linalg.norm(emb1_np) * np.linalg.norm(emb2_np))
        return float(cosine_sim)
```

**Functionality**: The embedder script generates vector embeddings for context files, allowing for semantic search and similarity matching between different parts of the codebase. This enables more intelligent context retrieval based on meaning rather than just text matching.

### Session Manager (`context_engine/scripts/session.py`)

```python
"""Session management script."""

import json
from datetime import datetime
from pathlib import Path
from typing import Dict, Any, Optional

class SessionManager:
    """Manage coding sessions and their metadata."""
    
    def __init__(self, session_file: Path):
        self.session_file = session_file
        self.session_data = self._load_session()
        
    def _load_session(self) -> Dict[str, Any]:
        """Load session data from file."""
        if self.session_file.exists():
            with open(self.session_file, 'r', encoding='utf-8') as f:
                return json.load(f)
        else:
            return {
                "current_session": None,
                "sessions": {},
                "last_accessed": datetime.now().isoformat()
            }
            
    def _save_session(self):
        """Save session data to file."""
        with open(self.session_file, 'w', encoding='utf-8') as f:
            json.dump(self.session_data, f, indent=2)
            
    def start_session(self, session_id: str, project_path: str, description: str = ""):
        """Start a new session."""
        session_info = {
            "id": session_id,
            "project_path": project_path,
            "description": description,
            "start_time": datetime.now().isoformat(),
            "end_time": None,
            "files_expanded": [],
            "notes": [],
            "status": "active"
        }
        
        self.session_data["current_session"] = session_id
        self.session_data["sessions"][session_id] = session_info
        self._save_session()
        
    def end_session(self, session_id: str):
        """End a session."""
        if session_id in self.session_data["sessions"]:
            session = self.session_data["sessions"][session_id]
            session["end_time"] = datetime.now().isoformat()
            session["status"] = "completed"
            
        if self.session_data["current_session"] == session_id:
            self.session_data["current_session"] = None
            
        self._save_session()
        
    def add_note(self, session_id: str, note: str):
        """Add a note to a session."""
        if session_id in self.session_data["sessions"]:
            self.session_data["sessions"][session_id]["notes"].append({
                "timestamp": datetime.now().isoformat(),
                "content": note
            })
            self._save_session()
            
    def expand_file(self, session_id: str, file_path: str):
        """Record that a file was expanded during a session."""
        if session_id in self.session_data["sessions"]:
            self.session_data["sessions"][session_id]["files_expanded"].append({
                "file_path": str(file_path),
                "timestamp": datetime.now().isoformat()
            })
            self._save_session()
```

**Functionality**: The session manager handles the lifecycle of coding sessions, tracking metadata like start/end times, files accessed, and notes made during the session. This allows for better continuity and context preservation between different coding sessions.

---

## Command Modules

### Init Command (`context_engine/commands/init_command.py`)

```python
"""Initialize Context Engine in a project"""

import click  # Command Line Interface library

from ..ui import info, success  # UI helper functions
from ..core import Config  # Configuration management

@click.command()
def init():
    """Initialize Context Engine in current project"""
    # Create configuration instance
    config = Config()

    # Create directory structure
    config.context_dir.mkdir(exist_ok=True)  # Create .context directory
    config.baseline_dir.mkdir(exist_ok=True)  # Create baseline directory
    config.adrs_dir.mkdir(exist_ok=True)  # Create ADRs directory

    # Create empty files
    config.session_file.touch()  # Create session.md file
    config.cross_repo_file.touch()  # Create cross_repo.md file

    # Save initial configuration
    config.save()

    # Create sample ADR (Architecture Decision Record)
    sample_adr = config.adrs_dir / "001-context-engine.md"
    if not sample_adr.exists():
        sample_adr.write_text(
            """# ADR-001: Context Engine Adoption

## Status
Accepted

## Context
We need a way to reduce token waste when starting AI coding sessions.

## Decision
Use Context Engine to manage project context.

## Consequences
- Reduced token usage by 20-30%
- Better session continuity
- Manual control over context
""",
            encoding="utf-8",
        )

    # Display success message
    success(f"Initialized Context Engine in {config.context_dir}")
    # Display next steps
    info("\nNext steps:")
    info("1. Add baseline files: context baseline add <files>")
    info("2. Bundle context: context bundle")
    info("\nAI Tool Prompt:")
    info("---")
    info("Load .context/context_for_ai.md and continue working on current session")
    info("---")
```

**Functionality**: This command initializes the Context Engine in the current project by creating the necessary directory structure, configuration files, and a sample ADR. It provides users with next steps to start using the Context Engine.

### Baseline Command (`context_engine/commands/baseline_commands.py`)

```python
"""Baseline file management commands."""

from pathlib import Path
from typing import List

import click

from ..ui import success, info, error
from ..core import Config


@click.group()
def baseline():
    """Manage baseline files for context."""
    pass


@baseline.command()
@click.argument('files', type=click.Path(exists=True), nargs=-1)
def add(files: List[str]):
    """Add files to baseline context."""
    config = Config()
    
    # Create baseline directory if it doesn't exist
    config.baseline_dir.mkdir(exist_ok=True)
    
    added_files = []
    for file_path in files:
        src = Path(file_path)
        dst = config.baseline_dir / src.name
        
        # Copy file to baseline directory
        dst.write_text(src.read_text(encoding='utf-8'), encoding='utf-8')
        added_files.append(dst.name)
    
    success(f"Added {len(added_files)} files to baseline")
    for file in added_files:
        info(f"- {file}")


@baseline.command()
def list():
    """List all baseline files."""
    config = Config()
    
    if not config.baseline_dir.exists():
        info("No baseline files found. Use 'baseline add <files>' to add files.")
        return
        
    files = list(config.baseline_dir.iterdir())
    if not files:
        info("No baseline files found.")
        return
        
    info(f"Baseline files ({len(files)}):")
    for file in files:
        if file.is_file():
            stat = file.stat()
            size = stat.st_size
            info(f"- {file.name} ({size} bytes)")


@baseline.command()
@click.argument('file_names', type=str, nargs=-1)
def remove(file_names: List[str]):
    """Remove files from baseline."""
    config = Config()
    
    if not config.baseline_dir.exists():
        error("No baseline directory found.")
        return
        
    removed_files = []
    for file_name in file_names:
        file_path = config.baseline_dir / file_name
        if file_path.exists():
            file_path.unlink()
            removed_files.append(file_name)
        else:
            error(f"File not found: {file_name}")
    
    if removed_files:
        success(f"Removed {len(removed_files)} files from baseline")
        for file in removed_files:
            info(f"- {file}")
```

**Functionality**: This command group manages baseline files that form the core context for AI coding sessions. Users can add, list, and remove important files that should always be included in the context bundle.

### Session Commands (`context_engine/commands/session_commands.py`)

```python
"""Session management commands."""

import click
from datetime import datetime
from pathlib import Path

from ..ui import success, info, error
from ..core import Config
from ..core.utils import sanitize_note_input


@click.command()
@click.argument('note', type=str, nargs=-1)
def save(note):
    """Save a note to the current session."""
    config = Config()
    
    note_text = ' '.join(note)
    if not note_text.strip():
        error("Please provide a note to save.")
        return
        
    note_text = sanitize_note_input(note_text, config.get("note_max_length", 2000))
    
    # Append note to session file with timestamp
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    note_entry = f"\n### {timestamp}\n{note_text}\n"
    
    with open(config.session_file, 'a', encoding='utf-8') as f:
        f.write(note_entry)
    
    success("Note saved to session")


@click.command()
@click.option('--refresh', is_flag=True, help="Refresh context using AI before ending session")
def session_end(refresh):
    """End current session with optional AI refresh."""
    config = Config()
    
    if refresh and config.openrouter_api_key:
        from ..models import OpenRouterClient
        client = OpenRouterClient(config.openrouter_api_key)
        
        session_content = config.session_file.read_text(encoding="utf-8") if config.session_file.exists() else ""
        if session_content.strip():
            # AI processing of session content before ending
            refreshed_content = client.summarize(session_content, "summarize")
            if refreshed_content:
                config.session_file.write_text(refreshed_content, encoding='utf-8')
                success("Session ended and notes refreshed with AI")
            else:
                success("Session ended (AI refresh failed, kept original notes)")
        else:
            success("Session ended (no notes to refresh)")
    else:
        success("Session ended")
```

**Functionality**: These commands manage session-specific notes and state. The save command adds notes to the current session with timestamps, while the session_end command ends the session with an optional AI refresh of the notes.

### Expand Command (`context_engine/commands/expand_command.py`)

```python
"""Expand command to add files mid-session."""

import click
from pathlib import Path

from ..ui import success, info, error
from ..core import Config
from ..core.utils import validate_path_in_project, redact_secrets


@click.command()
@click.argument('files', type=click.Path(exists=True), nargs=-1)
@click.option('--include-hidden', is_flag=True, help="Include hidden files")
def expand(files, include_hidden):
    """Add files to context mid-session."""
    config = Config()
    
    # Validate all files first
    valid_files = []
    for file_path in files:
        path = Path(file_path)
        
        # Skip hidden files unless explicitly included
        if not include_hidden and any(part.startswith('.') for part in path.parts):
            info(f"Skipping hidden file: {path}")
            continue
            
        try:
            validate_path_in_project(path, config.project_root)
            valid_files.append(path)
        except click.BadParameter as e:
            error(str(e))
    
    if not valid_files:
        error("No valid files to expand.")
        return
    
    # Add files to the expanded section of the context
    expanded_content = []
    for file_path in valid_files:
        try:
            content = file_path.read_text(encoding='utf-8')
            content = redact_secrets(content)  # Redact any secrets in the file
            
            # Format as a section in the context file
            file_content = f"\n### {file_path}\n```\n{content}\n```\n"
            expanded_content.append(file_content)
        except Exception as e:
            error(f"Could not read file {file_path}: {str(e)}")
    
    # Append to the context file in the expanded section
    if config.context_file.exists():
        current_content = config.context_file.read_text(encoding='utf-8')
        
        # Find and update the expanded files section
        if "## Expanded Files" in current_content:
            parts = current_content.split("## Expanded Files", 1)
            new_content = parts[0] + "## Expanded Files\n" + parts[1] + "".join(expanded_content)
        else:
            # Append expanded files section to the end
            new_content = current_content + "\n## Expanded Files\n" + "".join(expanded_content)
    else:
        # Create a new context file with expanded section
        new_content = "# Project Context for AI Tools\n\n## Expanded Files\n" + "".join(expanded_content)
    
    # Write updated content back to context file
    config.context_file.write_text(new_content, encoding='utf-8')
    
    success(f"Expanded context with {len(valid_files)} files")
    for file_path in valid_files:
        info(f"- {file_path}")
```

**Functionality**: This command allows users to add files to their context in the middle of an AI coding session, without having to regenerate the entire context bundle. It appends the files to the "Expanded Files" section of the context file.

### Status Command (`context_engine/commands/status_command.py`)

```python
"""Status command to show Context Engine status and warnings."""

import click
from pathlib import Path

from ..ui import success, info, warn, error
from ..core import Config, count_tokens


@click.command()
def status():
    """Show Context Engine status and warnings."""
    config = Config()
    
    info("Context Engine Status")
    info("=" * 20)
    
    # Check if initialized
    if config.context_dir.exists():
        success(f"✓ Initialized in: {config.project_root}")
    else:
        error("✗ Not initialized. Run 'context init' first.")
        return
    
    # Show context directory structure
    info(f"\nContext directory: {config.context_dir}")
    
    # Show baseline files
    if config.baseline_dir.exists():
        baseline_files = list(config.baseline_dir.iterdir())
        info(f"Baseline files: {len(baseline_files)}")
        for file in baseline_files:
            if file.is_file():
                size = file.stat().st_size
                info(f"  - {file.name} ({size} bytes)")
    else:
        info("Baseline files: 0")
    
    # Show session info
    if config.session_file.exists() and config.session_file.stat().st_size > 0:
        session_size = config.session_file.stat().st_size
        info(f"Session notes: {session_size} bytes")
    else:
        info("Session notes: None")
    
    # Show cross-repo notes
    if config.cross_repo_file.exists() and config.cross_repo_file.stat().st_size > 0:
        cross_repo_size = config.cross_repo_file.stat().st_size
        info(f"Cross-repo notes: {cross_repo_size} bytes")
    else:
        info("Cross-repo notes: None")
    
    # Show context file info
    if config.context_file.exists():
        context_size = config.context_file.stat().st_size
        content = config.context_file.read_text(encoding='utf-8')
        token_count = count_tokens(content)
        info(f"Current context bundle: {context_size} bytes")
        info(f"Token count: {token_count:,}")
        
        # Warn if token count is high
        max_tokens = config.get("max_tokens", 100000)
        if token_count > max_tokens * 0.8:  # Warn at 80% of limit
            warn(f"⚠ Token count is {token_count/max_tokens*100:.1f}% of limit ({max_tokens})")
    else:
        info("Current context bundle: Not generated")
        info("  Run 'context bundle' to generate context_for_ai.md")
    
    # Show API key status
    if config.openrouter_api_key:
        info("AI integration: ✓ Configured")
    else:
        info("AI integration: ⚠ Not configured (no API key)")
    
    # Show linked repositories
    linked_repos = config.get("linked_repos", [])
    if linked_repos:
        info(f"Linked repositories: {len(linked_repos)}")
        for repo in linked_repos:
            info(f"  - {repo}")
    else:
        info("Linked repositories: None")
```

**Functionality**: This command provides an overview of the Context Engine's current state, including information about baseline files, session notes, token counts, and configuration status. It helps users understand the current context setup and identify potential issues.

### Cross-Repo Command (`context_engine/commands/cross_repo_command.py`)

```python
"""Cross-repository management commands."""

import click
import subprocess
import tempfile
from pathlib import Path

from ..ui import success, info, error
from ..core import Config


@click.command()
@click.argument('repo_url', type=str)
@click.option('--branch', '-b', default='main', help="Branch to pull from (default: main)")
def pull_cross(repo_url):
    """Pull cross_repo.md from linked repositories."""
    config = Config()
    
    try:
        # Create temporary directory to clone repo
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            
            # Clone the repository
            info(f"Cloning {repo_url}...")
            result = subprocess.run([
                'git', 'clone', '--depth', '1', '-b', branch, repo_url, str(temp_path)
            ], capture_output=True, text=True)
            
            if result.returncode != 0:
                error(f"Failed to clone repository: {result.stderr}")
                return
            
            # Look for cross_repo.md in the cloned repository
            cross_repo_file = temp_path / 'cross_repo.md'
            if cross_repo_file.exists():
                # Read content from the remote cross_repo.md
                remote_content = cross_repo_file.read_text(encoding='utf-8')
                
                # Append to local cross_repo.md
                current_content = ""
                if config.cross_repo_file.exists():
                    current_content = config.cross_repo_file.read_text(encoding='utf-8')
                    
                    # Check if content already exists
                    if remote_content.strip() in current_content.strip():
                        info("Remote content already exists in local cross_repo.md")
                        return
                
                # Append new content to existing content
                updated_content = current_content + f"\n\n# From {repo_url}\n\n" + remote_content
                config.cross_repo_file.write_text(updated_content, encoding='utf-8')
                
                success(f"Updated cross_repo.md with content from {repo_url}")
            else:
                info(f"No cross_repo.md found in {repo_url}")
                
                # Check for README.md as alternative
                readme_file = temp_path / 'README.md'
                if readme_file.exists():
                    info(f"Found README.md in {repo_url}, you might want to copy relevant content")
    
    except Exception as e:
        error(f"Error pulling from {repo_url}: {str(e)}")


@click.group()
def cross_repo():
    """Cross-repository management commands."""
    pass


@cross_repo.command()
@click.argument('repo_url', type=str)
def add_link(repo_url):
    """Add a repository to the linked repositories list."""
    config = Config()
    
    linked_repos = config.get("linked_repos", [])
    if repo_url in linked_repos:
        info(f"Repository {repo_url} is already linked.")
        return
    
    linked_repos.append(repo_url)
    config.set("linked_repos", linked_repos)
    
    success(f"Added {repo_url} to linked repositories")


@cross_repo.command()
def list_links():
    """List all linked repositories."""
    config = Config()
    
    linked_repos = config.get("linked_repos", [])
    if not linked_repos:
        info("No linked repositories.")
        return
    
    info("Linked repositories:")
    for repo in linked_repos:
        info(f"- {repo}")
```

**Functionality**: These commands manage cross-repository context, allowing users to pull context from linked repositories and manage a list of associated repositories. This is useful when working on projects that span multiple repositories.

### Config Commands (`context_engine/commands/config_commands.py`)

```python
"""Configuration management commands."""

import click

from ..ui import success, info, error
from ..core import Config


@click.group()
def config():
    """Configuration management commands."""
    pass


@config.command()
@click.option('--key', prompt='Configuration key', help="Configuration key to set")
@click.option('--value', prompt='Configuration value', help="Configuration value to set")
def set(key, value):
    """Set a configuration value."""
    config = Config()
    
    # Convert value to appropriate type if needed
    if value.lower() in ['true', 'false']:
        value = value.lower() == 'true'
    elif value.isdigit():
        value = int(value)
    elif value.replace('.', '').isdigit():
        value = float(value)
    
    config.set(key, value)
    success(f"Set {key} = {value}")


@config.command()
@click.argument('key', type=str, nargs=1)
def get(key):
    """Get a configuration value."""
    config = Config()
    
    value = config.get(key)
    if value is None:
        error(f"Configuration key '{key}' not found.")
        return
    
    info(f"{key} = {value}")


@config.command()
def list():
    """List all configuration values."""
    config = Config()
    
    info("Current configuration:")
    for key, value in config._config.items():
        # Don't show API keys in plain text
        if 'key' in key.lower() or 'token' in key.lower() or 'secret' in key.lower():
            if value:
                info(f"{key} = ****** (HIDDEN)")
            else:
                info(f"{key} = (not set)")
        else:
            info(f"{key} = {value}")


@config.command()
@click.argument('key', type=str, nargs=1)
def unset(key):
    """Unset a configuration value."""
    config = Config()
    
    if key in config._config:
        # Don't allow unsetting required keys
        if key in ['context_dir', 'max_tokens', 'model']:
            error(f"Cannot unset required configuration key: {key}")
            return
            
        del config._config[key]
        config.save()
        success(f"Unset configuration key: {key}")
    else:
        error(f"Configuration key '{key}' not found.")
```

**Functionality**: These commands allow users to manage the Context Engine configuration, including setting, getting, listing, and unsetting configuration values. Security-sensitive values like API keys are handled appropriately.

### Additional Command Modules

The Context Engine includes several additional command modules that extend its functionality beyond the core commands:

#### Add Docs Command (`context_engine/commands/add_docs.py`)

```python
"""Command to add documentation files to the context."""

import click
from pathlib import Path

from ..ui import success, info, error
from ..core import Config

@click.command()
@click.argument('files', type=click.Path(exists=True), nargs=-1)
def add_docs(files):
    """Add documentation files to context."""
    config = Config()
    docs_dir = config.context_dir / "docs"
    docs_dir.mkdir(exist_ok=True)
    
    added_files = []
    for file_path in files:
        src = Path(file_path)
        dst = docs_dir / src.name
        
        # Copy file to docs directory
        dst.write_text(src.read_text(encoding='utf-8'), encoding='utf-8')
        added_files.append(dst.name)
    
    success(f"Added {len(added_files)} documentation files to context")
    for file in added_files:
        info(f"- {file}")
```

**Functionality**: This command allows users to add documentation files to the context, storing them in a dedicated docs directory for easy access during AI coding sessions.

#### Checklist Command (`context_engine/commands/checklist.py`)

```python
"""Checklist command for managing project tasks."""

import click
import json
from pathlib import Path

from ..ui import success, info, warn
from ..core import Config

@click.group()
def checklist():
    """Manage project checklists."""
    pass

@checklist.command()
@click.argument('task', type=str)
def add(task):
    """Add a task to the checklist."""
    config = Config()
    checklist_file = config.context_dir / "checklist.json"
    
    checklist_data = {}
    if checklist_file.exists():
        checklist_data = json.loads(checklist_file.read_text())
    
    if 'tasks' not in checklist_data:
        checklist_data['tasks'] = []
    
    checklist_data['tasks'].append({
        'id': len(checklist_data['tasks']) + 1,
        'task': task,
        'completed': False,
        'created_at': str(config.session_file.stat().st_mtime)
    })
    
    checklist_file.write_text(json.dumps(checklist_data, indent=2))
    success(f"Added task: {task}")

@checklist.command()
def show():
    """Show all checklist items."""
    config = Config()
    checklist_file = config.context_dir / "checklist.json"
    
    if not checklist_file.exists():
        info("No checklist found. Use 'checklist add <task>' to add tasks.")
        return
        
    checklist_data = json.loads(checklist_file.read_text())
    
    if not checklist_data.get('tasks'):
        info("No tasks in checklist.")
        return
        
    for task in checklist_data['tasks']:
        status = "✓" if task['completed'] else "○"
        print(f"{status} {task['id']}: {task['task']}")
        
@checklist.command()
@click.argument('task_id', type=int)
def complete(task_id):
    """Mark a task as complete."""
    config = Config()
    checklist_file = config.context_dir / "checklist.json"
    
    if not checklist_file.exists():
        error("No checklist found.")
        return
        
    checklist_data = json.loads(checklist_file.read_text())
    tasks = checklist_data.get('tasks', [])
    
    for task in tasks:
        if task['id'] == task_id:
            task['completed'] = True
            checklist_file.write_text(json.dumps(checklist_data, indent=2))
            success(f"Task {task_id} marked as complete")
            return
    
    error(f"Task {task_id} not found in checklist")
```

**Functionality**: This command provides a simple task management system to help developers track important tasks related to their project context.

#### Compress Command (`context_engine/commands/compress_command.py`)

```python
"""Command to compress context files."""

import click
from pathlib import Path

from ..ui import success, info, warn
from ..core import Config
from ..compressors.longcodezip_wrapper import LongcodezipWrapper

@click.command()
@click.option('--method', type=click.Choice(['token', 'size', 'content']), default='token',
              help='Compression method to use')
@click.option('--threshold', type=int, default=1000, 
              help='Threshold for compression (tokens or size in KB)')
def compress(method, threshold):
    """Compress context files using various methods."""
    config = Config()
    
    if method == 'token':
        _compress_by_tokens(config, threshold)
    elif method == 'size':
        _compress_by_size(config, threshold)
    elif method == 'content':
        _compress_by_content(config)
    
    success("Context compression completed")

def _compress_by_tokens(config: Config, threshold: int):
    """Compress files by token count."""
    from ..core.utils import count_tokens
    
    # Process all files in baseline directory
    for file_path in config.baseline_dir.rglob("*"):
        if file_path.is_file() and file_path.suffix in ['.py', '.js', '.ts', '.java', '.md', '.txt']:
            content = file_path.read_text(encoding='utf-8')
            token_count = count_tokens(content)
            
            if token_count > threshold:
                # Apply compression techniques
                from ..core.utils import strip_comments, compress_code
                compressed_content = strip_comments(content, file_path.suffix[1:])
                
                # Write compressed content back
                file_path.write_text(compressed_content, encoding='utf-8')
                info(f"Compressed {file_path.name} from {token_count} to {count_tokens(compressed_content)} tokens")

def _compress_by_size(config: Config, threshold: int):
    """Compress files by size."""
    # Process all files in baseline directory
    for file_path in config.baseline_dir.rglob("*"):
        if file_path.is_file():
            size_kb = file_path.stat().st_size / 1024
            
            if size_kb > threshold:
                # Compress using longcodezip
                wrapper = LongcodezipWrapper()
                compressed_path = wrapper.compress_file(file_path)
                info(f"Compressed {file_path.name} from {size_kb:.2f}KB")

def _compress_by_content(config: Config):
    """Compress files by content type."""
    from ..core.utils import deduplicate_content, compress_whitespace
    
    # Process all files in baseline directory
    for file_path in config.baseline_dir.rglob("*"):
        if file_path.is_file():
            content = file_path.read_text(encoding='utf-8')
            original_length = len(content)
            
            # Apply content compression techniques
            compressed_content = deduplicate_content(compress_whitespace(content))
            
            if len(compressed_content) < original_length:
                file_path.write_text(compressed_content, encoding='utf-8')
                info(f"Compressed {file_path.name} content")
```

**Functionality**: This command provides various methods for compressing context files, including token-based compression, size-based compression, and content-based compression techniques.

#### Search Command (`context_engine/commands/search.py`)

```python
"""Search command for finding context in files."""

import click
import re
from pathlib import Path

from ..ui import info, warn
from ..core import Config

@click.command()
@click.argument('query', type=str)
@click.option('--file-type', '-f', multiple=True, 
              help='Filter by file type (e.g., .py, .js, .md)')
def search(query, file_type):
    """Search for content across all context files."""
    config = Config()
    
    # Get all files to search
    all_files = []
    if config.baseline_dir.exists():
        all_files.extend(config.baseline_dir.rglob("*"))
    if config.adrs_dir.exists():
        all_files.extend(config.adrs_dir.rglob("*"))
    
    results = []
    query_lower = query.lower()
    
    for file_path in all_files:
        if file_path.is_file():
            # Filter by file type if specified
            if file_type and file_path.suffix not in file_type:
                continue
                
            try:
                content = file_path.read_text(encoding='utf-8')
                # Case-insensitive search
                if query_lower in content.lower():
                    # Find line numbers containing the query
                    lines = content.split('\n')
                    matching_lines = []
                    
                    for i, line in enumerate(lines, 1):
                        if query_lower in line.lower():
                            matching_lines.append((i, line.strip()))
                    
                    results.append({
                        'file': file_path,
                        'matches': matching_lines
                    })
            except UnicodeDecodeError:
                # Skip binary files
                continue
    
    if not results:
        info(f"No results found for query: {query}")
        return
    
    # Display results
    for result in results:
        info(f"\n{result['file']}:")
        for line_num, line_content in result['matches']:
            # Highlight the matched query (case-insensitive)
            highlighted = re.sub(
                f'({re.escape(query)})', 
                r'**\1**', 
                line_content, 
                flags=re.IGNORECASE
            )
            info(f"  Line {line_num}: {highlighted}")
```

**Functionality**: This command allows users to search for content across all context files, providing line numbers and context for matches found.

#### Bundle Command (`context_engine/commands/bundle_command.py`)

```python
"""Bundle command to generate context_for_ai.md"""

from pathlib import Path  # Object-oriented filesystem paths

import click  # Command Line Interface library

from ..ui import success, info, warn  # UI helper functions
from ..core import (  # Core functionality imports
    Config,
    count_tokens,
    summarize_config,
    deduplicate_content,
    redact_secrets,
)
from ..core.utils import compress_whitespace  # Utility function
from ..models import OpenRouterClient  # AI model integration

@click.command()
@click.option("--use-ai/--no-ai", default=True, help="Use AI for summarization")
def bundle(use_ai):
    """Generate context_for_ai.md bundle with fixed structure"""
    # Initialize configuration
    config = Config()

    # Collect structured content from different sources
    arch, apis, conf, schema = _collect_structured_baseline(config)  # Architecture and API docs
    session_content = _read_session(config)  # Session notes
    cross_repo_content = _read_cross_repo(config)  # Cross-repo notes
    expanded = _read_expanded_section(config)  # Expanded files section

    # Redact secrets from all content sections
    arch = redact_secrets(arch)
    apis = redact_secrets(apis)
    conf = redact_secrets(conf)
    schema = redact_secrets(schema)
    session_content = redact_secrets(session_content)
    cross_repo_content = redact_secrets(cross_repo_content)
    expanded = redact_secrets(expanded)

    # Apply whitespace compression and deduplication
    arch = deduplicate_content(compress_whitespace(arch))
    apis = deduplicate_content(compress_whitespace(apis))
    conf = deduplicate_content(compress_whitespace(conf))
    schema = deduplicate_content(compress_whitespace(schema))
    session_content = deduplicate_content(compress_whitespace(session_content))
    cross_repo_content = deduplicate_content(compress_whitespace(cross_repo_content))
    expanded = deduplicate_content(compress_whitespace(expanded))

    # Generate bundle using AI or manual method
    if use_ai and config.openrouter_api_key:
        # Use AI client if API key is available
        client = OpenRouterClient(config.openrouter_api_key)
        content = client.generate_fixed_context_bundle(
            architecture=arch or "None",  # Use "None" if empty
            apis=apis or "None",
            configuration=conf or "None",
            schema=schema or "None",
            session=session_content or "None",
            cross_repo=cross_repo_content or "None",
            expanded=expanded or "None",
        )
    else:
        # Use manual generation if AI is not available or disabled
        content = _manual_fixed_bundle(
            arch or "None",
            apis or "None",
            conf or "None",
            schema or "None",
            session_content or "None",
            cross_repo_content or "None",
            expanded or "None",
        )

    # Write final content to file
    config.context_file.write_text(content, encoding="utf-8")
    # Count tokens in generated content
    tokens = count_tokens(content)
    # Display success message
    success(f"Generated context bundle: {config.context_file}")
    info(f"Token count: {tokens:,}")  # Format token count with commas
    # Show warning if token count exceeds limit
    if tokens > config.get("max_tokens", 100000):
        warn(
            f"Token count exceeds limit ({config.get('max_tokens', 100000):,})"
        )
        info("Consider removing some baseline files or older session notes.")

def _collect_structured_baseline(config: Config):
    """Collect baseline into fixed sections: architecture, apis, configuration, schema"""
    # Return empty strings if baseline directory doesn't exist
    if not config.baseline_dir.exists():
        return "", "", "", ""

    # Function to read files matching name patterns
    def read_if_exists(name_patterns):
        for pattern in name_patterns:
            for file in config.baseline_dir.glob(pattern):
                if file.is_file():
                    return file.read_text(encoding="utf-8")
        return ""

    # Map specific filename patterns to sections
    architecture = read_if_exists(["architecture.*", "arch.*", "system.*"])  # Architecture docs
    apis = read_if_exists(["apis.*", "api.*"])  # API documentation
    configuration = read_if_exists(["config.*", "configuration.*", "settings.*"])  # Configuration files
    schema = read_if_exists(["schema.*", "db.*", "database.*"])  # Database schema
    return architecture, apis, configuration, schema

def _collect_adrs(config: Config) -> str:
    """Collect all ADR files"""
    # Return empty string if ADRs directory doesn't exist
    if not config.adrs_dir.exists():
        return ""

    contents = []
    # Read all markdown files in ADRs directory, sorted alphabetically
    for file in sorted(config.adrs_dir.glob("*.md")):
        contents.append(file.read_text(encoding="utf-8"))

    # Join contents with separator
    return "\n\n---\n\n".join(contents)

def _read_session(config: Config) -> str:
    """Read session notes"""
    # Return empty string if session file doesn't exist
    if not config.session_file.exists():
        return ""
    return config.session_file.read_text(encoding="utf-8")

def _read_cross_repo(config: Config) -> str:
    """Read cross-repo notes"""
    # Return empty string if cross-repo file doesn't exist
    if not config.cross_repo_file.exists():
        return ""
    return config.cross_repo_file.read_text(encoding="utf-8")

def _read_expanded_section(config: Config) -> str:
    """Read the Expanded Files section from existing context (if present)"""
    # Return empty string if context file doesn't exist
    if not config.context_file.exists():
        return ""
    content = config.context_file.read_text(encoding="utf-8")
    # Extract section starting with ## Expanded Files
    parts = content.split("\n## Expanded Files\n", 1)
    if len(parts) == 2:
        return parts[1].strip()
    return ""

def _apply_config_summarization(content: str) -> str:
    """Apply config summarization"""
    lines = content.split("\n")
    result = []

    in_config = False
    config_lines = []

    for line in lines:
        # Check if line starts with config file extension
        if line.startswith("### ") and any(
            ext in line for ext in [".json", ".yml", ".yaml", ".toml", ".ini", ".env"]
        ):
            if config_lines:
                result.append(summarize_config("\n".join(config_lines)))
                config_lines = []
            in_config = True
            result.append(line)
        elif line.startswith("### "):
            if config_lines:
                result.append(summarize_config("\n".join(config_lines)))
                config_lines = []
            in_config = False
            result.append(line)
        elif in_config:
            config_lines.append(line)
        else:
            result.append(line)

    if config_lines:
        result.append(summarize_config("\n".join(config_lines)))

    return "\n".join(result)

def _manual_fixed_bundle(
    architecture: str,
    apis: str,
    configuration: str,
    schema: str,
    session: str,
    cross_repo: str,
    expanded: str,
) -> str:
    """Generate manual bundle with fixed section order and placeholders"""
    lines = []
    lines.append("# Project Context for AI Tools\n")  # Title
    lines.append("*Generated by Context Engine V1*\n")  # Subtitle
    lines.append("## Architecture\n")  # Architecture section
    lines.append(architecture if architecture.strip() else "None")  # Content or "None"
    lines.append("\n## APIs\n")  # APIs section
    lines.append(apis if apis.strip() else "None")  # Content or "None"
    lines.append("\n## Configuration\n")  # Configuration section
    lines.append(configuration if configuration.strip() else "None")  # Content or "None"
    lines.append("\n## Database Schema\n")  # Schema section
    lines.append(schema if schema.strip() else "None")  # Content or "None"
    lines.append("\n## Session Notes\n")  # Session section
    lines.append(session if session.strip() else "None")  # Content or "None"
    lines.append("\n## Cross-Repo Notes\n")  # Cross-repo section
    lines.append(cross_repo if cross_repo.strip() else "None")  # Content or "None"
    lines.append("\n## Expanded Files\n")  # Expanded files section
    lines.append(expanded if expanded.strip() else "None")  # Content or "None"
    return "\n".join(lines)
```

**Functionality**: This command generates the `context_for_ai.md` file by collecting and processing content from various sources (baseline files, session notes, etc.). It applies compression, deduplication, and secret redaction. It can use AI to generate a more structured version or fall back to manual generation.

---

## Setup & Installation (`setup.py`)

```python
"""Setup configuration for Context Engine"""

from setuptools import setup, find_packages  # Package setup functions
from pathlib import Path  # Object-oriented filesystem paths

# Read README for long description
readme_file = Path(__file__).parent / "README.md"
long_description = readme_file.read_text(encoding="utf-8") if readme_file.exists() else ""

setup(
    name="context-engine",  # Package name
    version="1.0.0",  # Package version
    author="Context Engine Team",  # Package author
    description="CLI-based Context Engine for AI coding tools",  # Short description
    long_description=long_description,  # Long description from README
    long_description_content_type="text/markdown",  # Format of long description
    url="https://github.com/gurram46/Context-Engine",  # Project URL
    packages=find_packages(),  # Automatically find packages
    classifiers=[  # Package classifiers
        "Development Status :: 4 - Beta",
        "Intended Audience :: Developers",
        "Topic :: Software Development :: Libraries :: Python Modules",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
    ],
    python_requires=">=3.8",  # Minimum Python version
    install_requires=[  # Required dependencies
        "click>=8.0.0",  # Command line interface
        "tiktoken>=0.5.0",  # Token counting
        "requests>=2.28.0",  # HTTP requests
    ],
    entry_points={  # Console script entry points
        "console_scripts": [
            "context=context_engine.cli:main",  # 'context' command maps to main function
        ],
    },
    include_package_data=True,  # Include package data
    zip_safe=False,  # Don't install as zip file
)
```

**Functionality**: This setup file defines how the Context Engine package is installed. It specifies the package metadata, dependencies, and entry points. The main entry point is the 'context' command that maps to the main CLI function.

---

## Summary

The Context Engine project is a sophisticated CLI tool designed to help developers manage context for AI coding assistants. It provides:

1. **Initialization**: Creates a `.context` directory structure
2. **Baseline Management**: Allows adding important files to a baseline
3. **Context Bundling**: Creates optimized context files for AI tools
4. **Session Management**: Tracks session notes and progress
5. **Security**: Redacts secrets and validates file paths
6. **AI Integration**: Uses OpenRouter API for intelligent context generation
7. **Logging Parsing**: Includes specialized parsers for different programming languages
8. **Compression Utilities**: Advanced compression tools including longcodezip integration
9. **LangChain Integration**: Enhanced summarization using LangChain for better context understanding
10. **Utility Scripts**: Specialized tools for auto-capture, embeddings, and session management
11. **Extended Commands**: Additional command modules for search, checklists, documentation, and compression

The project is well-structured with clear separation of concerns between core functionality, commands, utilities, and external integrations. The code follows Python best practices and includes comprehensive security measures.