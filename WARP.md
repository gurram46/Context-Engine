# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Project Overview

Context Engine is a CLI-based Python tool that reduces token waste (20-30%) in AI coding sessions by preloading relevant project context. It creates compact, privacy-aware context bundles from baseline documents and on-demand file summaries.

## Core Architecture

### Command Structure
- **CLI Entry Point**: `context_engine/cli.py` - Click-based CLI with subcommands
- **Commands Directory**: `context_engine/commands/` - Individual command implementations:
  - `init_command.py` - Initialize `.context/` directory structure
  - `baseline_commands.py` - Manage baseline files (`add`, `list`, `review`)
  - `bundle_command.py` - Generate `context_for_ai.md` from all sources
  - `expand_command.py` - Add compressed file summaries mid-session
  - `session_commands.py` - Session management (`save`, `session-end`)
  - `status_command.py` - Show token counts and warnings
  - `cross_repo_command.py` - Import cross-repo notes
  - `config_commands.py` - Configuration management

### Core Components
- **Configuration**: `context_engine/core/config.py` - Config management with JSON persistence
- **Utilities**: `context_engine/core/utils.py` - Security, compression, token counting, secret redaction
- **AI Integration**: `context_engine/models/openrouter.py` - Optional AI summarization via OpenRouter (Qwen3 Coder)
- **UI Helper**: `context_engine/ui.py` - Consistent colored terminal output

### Data Model
The tool creates a `.context/` directory in projects with:
- `config.json` - Settings (extensions, limits, API keys, linked repos)
- `baseline/` - Copies of selected project files
- `adrs/` - Architecture decision records
- `session.md` - Timestamped session notes
- `cross_repo.md` - Notes from linked repositories
- `hashes.json` - File change detection
- `context_for_ai.md` - Final AI-ready bundle (deterministic sections)

## Development Commands

### Installation & Setup
```bash
# Install in development mode
pip install -e .

# Install dependencies
pip install -r requirements.txt
```

### Testing
```bash
# Run all tests
pytest

# Run specific test file
pytest tests/test_security.py

# Run with verbose output
pytest -v
```

### Package Management
```bash
# Build package
python setup.py sdist bdist_wheel

# Install package
pip install context-engine
```

### Common Context Engine Operations
```bash
# Initialize in project
context init

# Add baseline files
context baseline add src/architecture.md docs/apis.md

# Generate AI context bundle
context bundle

# Add files mid-session
context expand src/auth.js config/auth.yml

# Save session note
context save "Finished login validation, next: token refresh"

# End session
context session-end

# Check status and token counts
context status

# Configuration management
context config show
context config set max_tokens 150000
```

## Security Architecture

### Path Traversal Protection
- All file operations validate paths are within project root using `is_subpath()` and `validate_path_in_project()`
- Prevents `../` attacks and absolute path escapes

### Secret Redaction System
- **Regex patterns**: Detects API keys (sk-, AKIA), passwords, tokens
- **Entropy detection**: Uses Shannon entropy to identify high-entropy strings (>= 3.5 entropy, >= 20 chars)
- **Safe replacement**: Replaces secrets with `[REDACTED]`, `[REDACTED_KEY]`, `[REDACTED_AWS]`

### Input Validation
- **File types**: Configurable allowed extensions (`.py`, `.js`, `.md`, etc.)
- **File size**: Configurable limits (default 1MB)
- **Note sanitization**: Removes control characters, enforces max length
- **API key validation**: Format validation for OpenRouter keys

## Bundle Generation Process

The bundling system creates deterministic output with fixed sections:

1. **Architecture** - From baseline architecture files
2. **APIs** - API documentation and schemas  
3. **Configuration** - Config files with secret redaction
4. **Database Schema** - Database structure docs
5. **Session Notes** - Timestamped development notes
6. **Cross-Repo Notes** - Notes from linked repositories
7. **Expanded Files** - On-demand compressed file summaries

### Compression Rules
- **Strip comments**: Remove inline comments, preserve docstrings
- **Deduplicate**: Remove duplicate content patterns
- **Compress whitespace**: Eliminate excessive blank lines
- **Summarize configs**: Keep structure, replace values with `[configured]`
- **Extract API docs**: Keep only docstrings/JSDoc, remove implementation

## AI Integration

### OpenRouter Configuration
- Uses Qwen3 Coder 480B A35B model via OpenRouter
- API key from config or `OPENROUTER_API_KEY` environment variable
- Graceful fallback to manual bundling if AI unavailable
- Strict compression mode when using AI summarization

### Token Management
- Uses `tiktoken` for accurate token counting
- Configurable token limits (default 100,000)
- Staleness warnings for changed baseline files
- Status command shows current token usage

## Configuration System

### Default Settings
```json
{
  "openrouter_api_key": "",
  "model": "qwen/qwen3-coder:free",
  "max_tokens": 100000,
  "context_dir": ".context",
  "auto_refresh": false,
  "allowed_extensions": [".md", ".json", ".yml", ".py", ".js", ".ts"],
  "max_file_size_kb": 1024,
  "note_max_length": 2000
}
```

### Cross-Repository Linking
- Configure linked repos in `config.json`
- Use `context pull-cross` to import notes from linked repos
- Supports distributed development workflows

## Testing Strategy

- **Security tests**: Path traversal, secret redaction, input sanitization
- **Integration tests**: CLI command testing with temporary directories
- **Validation tests**: API key formats, file type restrictions
- **Compression tests**: Code compression and deduplication

When working with this codebase, prioritize security validation and ensure all file operations respect the project boundary restrictions.
