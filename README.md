# Context Engine

**Local project brain for AI tools** - Intelligent code context and session management

Context Engine is a powerful tool that creates a "local brain" for your development projects, enabling AI tools to understand your codebase better through intelligent indexing, semantic search, and session management.

## Features

### 🧠 **Intelligent Code Understanding**
- **Incremental File Indexing**: SHA256-based change detection with smart chunking
- **Semantic Search**: Vector embeddings using sentence-transformers for contextual code search
- **Template-based Summaries**: Automated file summaries with AST parsing for Python, regex for other languages
- **Multi-language Support**: Works with Python, JavaScript, TypeScript, Java, and more

### 🔄 **Git Integration**
- **Merge Conflict Detection**: Automatically identifies and reports merge conflicts
- **Change Tracking**: Monitors Git changes and updates index accordingly
- **Pre-push Hooks**: Ensures shared context is exported before pushing
- **Commit History**: Tracks recent changes for better context

### 🎯 **AI Tool Sessions**
- **Session Management**: Start/stop sessions with scoped file tracking
- **Context Injection**: Add specific files to session context
- **Auto-Capture**: Automatically monitor and capture dev server logs
- **Intelligent Log Parsing**: Parse Python, Java, JavaScript errors with structured output
- **Documentation Integration**: Add docs and specifications to session context
- **Agent Handoff Notes**: Seamless context transfer between AI sessions
- **Payload Generation**: Create structured context for AI tools

### 🤝 **Team Collaboration**
- **Shared Digests**: Export and share project context with team members
- **Conflict Resolution**: Smart merging of team context updates
- **Team Context Directory**: Centralized location for shared project knowledge
- **Project Readiness Checklist**: Automated checks for ADRs, docs, and team setup

### 📦 **Cross-Platform Support**
- **NPM Package**: Install globally via npm for easy access
- **Node.js Wrapper**: Cross-platform CLI with automatic Python dependency management
- **Multiple Installation Methods**: pip, npm, or from source

## Installation

### Via NPM (Recommended)

```bash
# Install globally for easy access
npm install -g context-engine

# Or use without installing
npx context-engine --help
```

### Via pip

```bash
# Install from PyPI (when published)
pip install context-engine

# Or install from source
git clone https://github.com/contextengine/context-engine.git
cd context-engine
pip install -e .
```

### System Requirements

- **Python 3.8+** (automatically managed by npm wrapper)
- **Node.js 14+** (for npm installation)
- **Git** (for version control integration)

## Quick Start

### 1. Initialize Context Engine

```bash
# Navigate to your project directory
cd /path/to/your/project

# Initialize Context Engine
context-engine init
```

This creates:
- `context_engine/` - Local context data and configuration
- `team_context/` - Shared team knowledge
- `.context_payload/` - Session data and exports
- `context.yml` - Configuration file

### 2. Index Your Project

```bash
# Full reindex
context-engine reindex --full

# Incremental reindex (only changed files)
context-engine reindex
```

### 3. Search Your Code

```bash
# Semantic search
context-engine search "authentication logic"
context-engine search "database connection setup" --limit 5
```

### 4. Start an AI Session

```bash
# Start a new session with auto-capture
context-engine start-session "bug-fix-auth"

# Set scope to relevant files (supports append mode)
context-engine set-scope src/auth/ tests/auth/
context-engine set-scope docs/ --append

# Add documentation to context
context-engine add-docs README.md docs/api-spec.md

# Inject specific files
context-engine inject src/config/database.py

# Auto-capture will monitor dev servers automatically
# Manual log capture is also available
context-engine capture --content "Error: Connection timeout" --type error

# Generate payload for AI tool
context-engine generate-payload --query "authentication issues" --output payload.json

# Stop session (saves handoff notes automatically)
context-engine stop-session
```

### 5. Project Readiness Check

```bash
# Run comprehensive project checklist
context-engine checklist

# Check specific categories
context-engine checklist --category documentation
context-engine checklist --category architecture
```

### 6. Sync with Git

```bash
# Check for merge conflicts
context-engine sync --check-conflicts

# Full sync (reindex changed files)
context-engine sync

# Create Git pre-push hook
context-engine sync --create-hook
```

### 7. Team Collaboration

```bash
# Export shared digest
context-engine export --shared --format zip --output team-digest.zip

# Pull team digest
context-engine pull-digest team-digest.zip

# List available digests
context-engine export --list
```

## Configuration

The `context.yml` file controls Context Engine behavior:

```yaml
project:
  name: "my-project"
  description: "Project description"
  chunk_size: 1000
  chunk_overlap: 200
  
embedding:
  provider: "sentence-transformers"
  model: "all-MiniLM-L6-v2"
  
indexing:
  ignore_patterns:
    - "*.pyc"
    - "node_modules/"
    - ".git/"
  redact_patterns:
    - "password\s*=\s*['\"][^'\"]+['\"]"  # Remove passwords
    - "api_key\s*=\s*['\"][^'\"]+['\"]"   # Remove API keys
    
shared_context:
  auto_export: true
  include_summaries: true
  max_chunks_per_file: 10
```

## Commands Reference

### Core Commands

- `init` - Initialize Context Engine in current directory
- `reindex` - Reindex project files (supports `--full` and `--files`)
- `search <query>` - Search indexed content with semantic similarity
- `sync` - Sync with Git and update index
- `status` - Show Context Engine status

### Session Management

- `start-session [name]` - Start new AI tool session with auto-capture
- `stop-session` - Stop current session and save handoff notes
- `set-scope <paths...>` - Set session scope to specific paths
  - `--append` - Add to existing scope instead of replacing
- `add-docs <paths...>` - Add documentation files to session context
- `inject <file>` - Inject file into current session
- `capture` - Capture logs for current session
- `generate-payload` - Generate AI tool payload
- `checklist` - Run project readiness checklist
  - `--category <name>` - Check specific category only
- `status --session` - Show session status

### Team Collaboration

- `export --shared` - Export shared digest
- `pull-digest <path>` - Pull and integrate team digest
- `export --list` - List available team digests

## Architecture

### Directory Structure

```
your-project/
├── context_engine/           # Local context data
│   ├── config/              # Configuration files
│   ├── embeddings_db/       # Vector embeddings and FAISS index
│   ├── summaries/           # Generated file summaries
│   ├── logs/               # Context Engine logs
│   ├── auto_capture/       # Auto-captured dev server logs
│   │   ├── logs/           # Raw captured logs
│   │   └── errors/         # Parsed error reports
│   └── handoff_notes/      # Agent handoff notes
├── team_context/            # Shared team knowledge
│   ├── summaries/          # Shared file summaries
│   └── digests/           # Team digest files
├── .context_payload/        # Session data and exports
│   ├── sessions/           # Active and past sessions
│   └── exports/           # Generated payloads
├── package.json            # NPM package configuration
├── bin/                    # CLI wrapper scripts
└── context.yml             # Configuration file
```

### Core Components

1. **File Indexer** (`embedder.py`) - Handles incremental indexing with SHA256 tracking
2. **Embeddings Store** (`embeddings_store.py`) - Vector embeddings and similarity search
3. **File Summarizer** (`summarizer.py`) - Template-based file analysis
4. **Session Manager** (`session.py`) - AI tool session lifecycle with handoff notes
5. **Auto Capture** (`auto_capture.py`) - Automatic dev server monitoring and log capture
6. **Log Parsers** (`parsers/`) - Intelligent parsing for Python, Java, JavaScript errors
7. **Git Sync** (`sync.py`) - Git integration and change tracking
8. **Digest Exporter** (`export.py`) - Team collaboration features
9. **NPM Wrapper** (`bin/context-engine.js`) - Cross-platform CLI interface

## Use Cases

### For Individual Developers
- **Code Understanding**: Quickly find relevant code sections
- **Context for AI**: Provide rich context to AI coding assistants
- **Session Tracking**: Maintain context across development sessions
- **Change Monitoring**: Track what's changed since last AI interaction

### For Teams
- **Knowledge Sharing**: Share project understanding across team members
- **Onboarding**: Help new team members understand codebase structure
- **Code Reviews**: Provide context for code review discussions
- **Documentation**: Maintain living documentation of code purpose and structure

### For AI Tools
- **Rich Context**: Structured project information for better AI responses
- **Semantic Search**: Find relevant code based on natural language queries
- **Session Continuity**: Maintain context across multiple AI interactions
- **Log Integration**: Include runtime information in AI context

## Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup

```bash
# Clone and install in development mode
git clone https://github.com/contextengine/context-engine.git
cd context-engine
pip install -e ".[dev]"

# Run tests
pytest

# Format code
black .
flake8 .
```

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Support

- **Issues**: [GitHub Issues](https://github.com/contextengine/context-engine/issues)
- **Discussions**: [GitHub Discussions](https://github.com/contextengine/context-engine/discussions)
- **Documentation**: [Full Documentation](https://contextengine.dev/docs)

---

**Context Engine** - Making AI tools smarter about your code, one project at a time.