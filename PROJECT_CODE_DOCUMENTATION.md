# Context Engine v1.0.0 - Complete Architecture Documentation

## Table of Contents
1. [Overview](#overview)
2. [Hybrid Architecture](#hybrid-architecture)
3. [Project Structure](#project-structure)
4. [Backend (Python) Architecture](#backend-python-architecture)
5. [Frontend (Node.js) Architecture](#frontend-nodejs-architecture)
6. [Installation & Setup](#installation--setup)
7. [Command Reference](#command-reference)
8. [Development Workflow](#development-workflow)
9. [Testing & Validation](#testing--validation)
10. [Deployment & Distribution](#deployment--distribution)
11. [Security Considerations](#security-considerations)
12. [Future Enhancements](#future-enhancements)

## Overview

Context Engine v1.0.0 is a **hybrid CLI tool** that combines the power of Python backend processing with a modern Node.js frontend interface. This architecture provides the best of both worlds: robust Python libraries for file processing and compression, and a sleek, interactive Node.js CLI experience.

### Key Features
- 🚀 **Interactive Mode**: Beautiful ASCII art welcome screen with command palette
- 🎯 **Hybrid Architecture**: Node.js frontend + Python backend
- 📦 **NPM Distribution**: Global installation via npm
- 🎨 **Modern Terminal UI**: Orange-on-black theme with gradient effects
- 🔧 **Full Feature Set**: All original Context Engine functionality preserved
- 🛡️ **Enhanced Security**: Improved secret redaction and error handling

### Design Philosophy
The Context Engine reduces token waste (20-30%) when starting AI coding tool sessions by preloading relevant project context. It allows developers to bundle project context for AI tools like Claude Code, Cursor, and other AI coding assistants with a modern, interactive interface.

## Hybrid Architecture

### Design Principles
The hybrid architecture was designed to:
- Leverage Python's mature ecosystem for file processing, compression, and text manipulation
- Utilize Node.js's superior CLI libraries for interactive terminal experiences
- Maintain separation of concerns while enabling seamless communication
- Enable easy distribution via npm package manager

### Architecture Components

```
┌─────────────────────────────────────────────────────────────┐
│                    Context Engine CLI                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐         ┌──────────────────────────┐   │
│  │  Node.js        │         │  Python Backend         │   │
│  │  Frontend       │◄────────┤  (Core Functionality)   │   │
│  │                 │         │                          │   │
│  │ • CLI Interface │         │ • File Processing       │   │
│  │ • Terminal UI   │         │ • Compression           │   │
│  │ • User          │         │ • Secret Redaction      │   │
│  │   Interaction   │         │ • Bundle Generation     │   │
│  │ • Command       │         │ • Baseline Creation     │   │
│  │   Routing       │         │                          │   │
│  └─────────────────┘         └──────────────────────────┘   │
│         │                                   │              │
│         │ Child Process Communication       │              │
│         └───────────────────────────────────┘              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Communication Bridge
- **Process Spawning**: Node.js spawns Python processes as needed
- **Argument Passing**: Command-line arguments and options passed securely
- **Result Handling**: Python outputs captured and displayed by Node.js
- **Error Management**: Comprehensive error handling across the bridge

## Project Structure

### Complete Directory Layout

```
Context-Engine/
├── README.md                              # Main project documentation
├── LICENSE                                # MIT license file
├── PROJECT_CODE_DOCUMENTATION.md          # This file
├── package.json                           # Root NPM package configuration
│
├── ui/                                    # Node.js Frontend
│   ├── package.json                       # Frontend NPM configuration
│   ├── index.js                          # Main CLI entry point
│   ├── README.md                         # Frontend-specific documentation
│   │
│   ├── bin/                              # Global CLI binaries
│   │   ├── context-engine.js             # Main CLI command
│   │   └── ce.js                         # Short CLI command
│   │
│   ├── lib/                              # Frontend library modules
│   │   ├── welcome.js                    # Welcome screen & ASCII art
│   │   └── backend-bridge.js             # Python backend bridge
│   │
│   └── scripts/                          # Build and utility scripts
│       └── build.js                      # Distribution build script
│
├── backend/                              # Python Backend
│   ├── main.py                          # Backend entry point
│   ├── requirements.txt                 # Python dependencies
│   ├── welcome_screen.py                # Original welcome screen (legacy)
│   │
│   └── context_engine/                  # Core Context Engine modules
│       ├── __init__.py                  # Package initialization
│       ├── cli.py                       # CLI interface module
│       ├── ui.py                        # UI components
│       │
│       ├── commands/                    # Command implementations
│       │   ├── __init__.py
│       │   ├── init_command.py          # Project initialization
│       │   ├── baseline_command.py      # Baseline generation
│       │   └── bundle_command.py        # Bundle creation
│       │
│       ├── compressors/                 # File compression modules
│       │   ├── __init__.py
│       │   └── compress_src.py          # Source file compression
│       │
│       ├── core/                        # Core functionality
│       │   ├── __init__.py
│       │   ├── config.py                # Configuration management
│       │   └── utils.py                 # Utility functions
│       │
│       ├── models/                      # Data models
│       │   ├── __init__.py
│       │   └── session.py               # Session management
│       │
│       ├── parsers/                     # Content parsing modules
│       │   ├── __init__.py
│       │   └── file_parser.py           # File parsing logic
│       │
│       ├── scripts/                     # Utility scripts
│       │   ├── __init__.py
│       │   └── setup.py                 # Setup utilities
│       │
│       └── langchain/                   # LangChain integration
│           ├── __init__.py
│           └── llm_interface.py         # LLM integration module
│
└── tests/                                # Test suite
    ├── test_bundle_integration.py       # Bundle integration tests
    ├── test_compression_workflow.py     # Compression workflow tests
    ├── test_end_to_end_workflow.py      # End-to-end tests
    ├── test_secret_redaction.py         # Secret redaction tests
    └── test_session_management.py       # Session management tests
```

## Backend (Python) Architecture

### Core Modules

#### `main.py` - Backend Entry Point
```python
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
```

#### Enhanced Command Implementations

##### `commands/bundle_command.py` - Enhanced Bundle Creation
- **Features**: Robust error handling, Unicode support, content validation
- **Security**: Secret redaction before bundling
- **Output**: Multiple format support (Markdown, JSON, plain text)

##### `compressors/compress_src.py` - Intelligent Compression
- **Features**: Configurable skip patterns, recursive processing
- **Configuration**: JSON-based configuration management
- **Performance**: Optimized for large codebases

##### `core/utils.py` - Enhanced Security & Utilities
- **Secret Redaction**: Comprehensive pattern matching for:
  - AWS Access Keys: `AKIA[0-9A-Z]{16}`
  - Bearer Tokens: `Bearer\s+[A-Za-z0-9\-._~+\/]+=*`
  - Environment Variables: `PASSWORD=`, `API_KEY=`, etc.
  - JWT Tokens: `eyJ[A-Za-z0-9\-._~+\/]+=*`
  - High-entropy strings detection
- **Error Handling**: Graceful fallbacks for encoding issues

### Configuration System

#### `core/config.py` - Centralized Configuration
```python
DEFAULT_CONFIG = {
    "compression": {
        "max_file_size": 1024 * 1024,  # 1MB
        "include_line_numbers": True,
        "output_format": "markdown"
    },
    "skip_patterns": [
        "*.log", "*.tmp", "*.cache",
        "__pycache__/", "node_modules/",
        ".git/", ".vscode/", ".idea/",
        "*.min.js", "*.bundle.js",
        "*.pyc", "*.pyo", "*.pyd"
    ],
    "secret_detection": {
        "enabled": True,
        "min_entropy": 3.0,
        "patterns": {
            "aws_access_key": r'AKIA[0-9A-Z]{16}',
            "bearer_token": r'Bearer\s+[A-Za-z0-9\-._~+\/]+=*',
            "jwt_token": r'eyJ[A-Za-z0-9\-._~+\/]+=*\.eyJ[A-Za-z0-9\-._~+\/]+=*\.[A-Za-z0-9\-._~+\/]+=*'
        }
    }
}
```

### Dependencies

#### `requirements.txt` - Python Dependencies
```
# Context Engine Backend Dependencies
rich>=13.0.0
chardet>=5.0.0
python-dotenv>=1.0.0
pathspec>=0.10.0
click>=8.0.0
```

## Frontend (Node.js) Architecture

### Core Components

#### `index.js` - Main CLI Interface
- **Framework**: Commander.js for command-line parsing
- **Interactive Mode**: Inquirer.js for command palette
- **Styling**: Chalk.js for terminal colors
- **ASCII Art**: Figlet.js for logo generation

#### `lib/welcome.js` - Welcome Screen & UI
```javascript
async showWelcomeScreen() {
  // Clear screen
  console.clear();

  // Create ASCII art logo with figlet
  const logoText = await this.createLogo();

  // Create welcome content
  const welcomeContent = this.createWelcomeContent(logoText);

  // Create main welcome box
  const welcomeBox = boxen(welcomeContent, {
    padding: { top: 1, bottom: 1, left: 3, right: 3 },
    margin: { top: 2, bottom: 1 },
    borderStyle: 'round',
    borderColor: '#FF3B00',
    backgroundColor: '#000000',
    title: ' ',
    titleAlignment: 'center'
  });

  // Display welcome screen
  console.log(welcomeBox);
  this.displayFooter();
}
```

#### `lib/backend-bridge.js` - Python Integration
```javascript
async executeCommand(command, args = []) {
  return new Promise((resolve, reject) => {
    const spinner = ora(`Executing ${command}...`).start();

    const child = spawn(this.getPythonCommand(), ['main.py', command, ...args], {
      cwd: this.backendDir,
      stdio: 'pipe',
      env: {
        ...process.env,
        PYTHONPATH: this.backendDir
      }
    });

    // Handle stdout, stderr, and process completion
    child.stdout.on('data', (data) => {
      const output = data.toString();
      spinner.clear();
      process.stdout.write(output);
    });

    // ... error handling and result processing
  });
}
```

### Package Configuration

#### `ui/package.json` - Frontend Dependencies
```json
{
  "name": "@contextengine/cli",
  "version": "1.0.0",
  "description": "Context Engine CLI - Compress the Chaos",
  "main": "index.js",
  "bin": {
    "context-engine": "./bin/context-engine.js",
    "ce": "./bin/ce.js"
  },
  "scripts": {
    "start": "node index.js",
    "dev": "nodemon index.js",
    "test": "jest",
    "lint": "eslint .",
    "prepare": "npm run build",
    "build": "node scripts/build.js",
    "prepack": "npm run build"
  },
  "dependencies": {
    "chalk": "^4.1.2",
    "figlet": "^1.7.0",
    "inquirer": "^8.2.0",
    "commander": "^9.4.0",
    "ora": "^5.4.1",
    "boxen": "^5.1.0",
    "gradient-string": "^2.0.2",
    "cli-table3": "^0.6.3",
    "cross-spawn": "^7.0.3"
  },
  "engines": {
    "node": ">=16.0.0"
  }
}
```

## Installation & Setup

### Prerequisites
- **Node.js**: >= 16.0.0
- **Python**: >= 3.7
- **Terminal**: ANSI color support

### Installation Methods

#### Method 1: NPM Global Installation (Recommended)
```bash
npm install -g @contextengine/cli
```

#### Method 2: Local Development Installation
```bash
# Clone repository
git clone https://github.com/contextengine/cli.git
cd cli

# Install Node.js dependencies
npm install

# Set up Python backend
cd backend
python -m venv venv

# On Windows
venv\Scripts\activate
pip install -r requirements.txt

# On macOS/Linux
source venv/bin/activate
pip install -r requirements.txt
```

#### Method 3: Development Mode
```bash
# Install dependencies and set up development environment
npm run install:backend
npm run dev
```

## Command Reference

### Interactive Mode (Default)
```bash
context-engine
```
Launches the interactive welcome screen with command palette.

### Direct Commands

#### `init` - Initialize Project
```bash
context-engine init
```
Creates CONTEXT.md with setup instructions and project structure.

#### `baseline` - Generate Project Baseline
```bash
context-engine baseline [options]
```
**Options**:
- `-o, --output <file>`: Output file (default: BASELINE.md)

#### `bundle` - Create Context Bundle
```bash
context-engine bundle [options]
```
**Options**:
- `-o, --output <file>`: Output file (default: bundle.md)
- `-f, --format <format>`: Output format (default: markdown)

#### `compress` - Compress Source Files
```bash
context-engine compress [options]
```
**Options**:
- `-o, --output <file>`: Output file (default: compressed.md)
- `-r, --recursive`: Recursive compression

#### `palette` - Interactive Command Palette
```bash
context-engine palette
```
Shows the interactive command selection menu.

#### `help` - Show Help
```bash
context-engine help
```
Displays comprehensive help information.

### Command Examples
```bash
# Initialize new project
context-engine init

# Create bundle with custom output
context-engine bundle -o my-bundle.md -f markdown

# Compress recursively with custom output
context-engine compress --recursive -o source-compressed.md

# Generate baseline
context-engine baseline -o project-baseline.md

# Show interactive palette
context-engine palette
```

## Development Workflow

### Development Commands
```bash
# Start development mode
npm run dev

# Run tests
npm test

# Lint code
npm run lint

# Build for distribution
npm run build
```

### Code Quality Standards
- **ESLint**: JavaScript/Node.js code quality
- **Black**: Python code formatting (recommended)
- **Jest**: JavaScript testing framework
- **Pytest**: Python testing framework (recommended)

### Git Workflow
```bash
# Create feature branch
git checkout -b feature/new-feature

# Make changes and commit
git add .
git commit -m "feat: add new feature"

# Push and create pull request
git push origin feature/new-feature
```

## Testing & Validation

### Test Coverage
- **Unit Tests**: Individual module testing
- **Integration Tests**: Frontend-backend communication
- **End-to-End Tests**: Complete workflow validation
- **Security Tests**: Secret redaction validation

### Running Tests
```bash
# Node.js tests
cd ui && npm test

# Python tests (if using pytest)
cd backend && pytest

# Integration tests
npm run test:integration
```

### Test Files Structure
```
tests/
├── test_bundle_integration.py       # Bundle creation tests
├── test_compression_workflow.py     # Compression workflow tests
├── test_end_to_end_workflow.py      # Complete workflow tests
├── test_secret_redaction.py         # Security validation tests
└── test_session_management.py       # Session handling tests
```

## Deployment & Distribution

### NPM Package Distribution
```bash
# Build package
npm run build

# Publish to NPM
npm publish --access public
```

### Version Management
- **Semantic Versioning**: MAJOR.MINOR.PATCH
- **Changelog**: Maintain CHANGELOG.md
- **Release Tags**: Git tags for each release

### Package Structure for Distribution
```
package.json (root)
├── ui/bin/                    # CLI binaries
├── ui/lib/                    # Frontend library
├── ui/scripts/                # Build scripts
├── backend/                   # Python backend
├── README.md                  # Documentation
└── LICENSE                    # License file
```

## Security Considerations

### Secret Redaction System
The Context Engine includes comprehensive secret detection and redaction:

#### Supported Secret Types
1. **AWS Access Keys**: `AKIA[0-9A-Z]{16}`
2. **Bearer Tokens**: `Bearer [token]`
3. **JWT Tokens**: `eyJ[token]`
4. **Environment Variables**: `PASSWORD=`, `API_KEY=`, etc.
5. **High-Entropy Strings**: Statistical analysis for potential secrets

#### Redaction Implementation
```python
def redact_secrets(text: str) -> str:
    """
    Redact secrets from text using comprehensive pattern matching
    """
    # AWS keys
    text = re.sub(r'(AKIA[0-9A-Z]{16})', '[AWS_ACCESS_KEY]', text)

    # Bearer tokens
    text = re.sub(r'(Bearer\s+[A-Za-z0-9\-._~+\/]+=*)', '[BEARER_TOKEN]', text)

    # JWT tokens
    text = re.sub(r'(eyJ[A-Za-z0-9\-._~+\/]+=*\.eyJ[A-Za-z0-9\-._~+\/]+=*\.[A-Za-z0-9\-._~+\/]+=*)', '[JWT_TOKEN]', text)

    # Environment variables
    text = re.sub(r'([A-Z_]+_(PASSWORD|SECRET|KEY|TOKEN)\s*=\s*)([^\s\n]+)', r'\1[REDACTED]', text)

    return text
```

### Error Handling
- **Graceful Degradation**: Continue processing even with individual file errors
- **Unicode Support**: Handle various file encodings
- **Input Validation**: Validate file paths and command arguments
- **Memory Management**: Efficient processing of large files

### Secure Communication
- **Process Isolation**: Python processes isolated from Node.js
- **Argument Sanitization**: Clean command-line arguments
- **Output Filtering**: Filter sensitive information from output

## Future Enhancements

### Planned Features
1. **Plugin System**: Extensible architecture for custom processors
2. **Cloud Integration**: Direct upload to cloud storage
3. **Web Interface**: Browser-based management interface
4. **Performance Optimization**: Parallel processing for large files
5. **Advanced Filtering**: More sophisticated file selection criteria

### Architecture Improvements
1. **Microservices**: Separate services for different functions
2. **Caching**: Intelligent caching for repeated operations
3. **Configuration Management**: Remote configuration support
4. **Monitoring**: Performance and usage analytics

### Platform Support
1. **Windows Services**: Native Windows service support
2. **macOS Integration**: macOS-specific features
3. **Linux Packages**: Debian/RPM package support

## Version History

### v1.0.0 (Current)
- ✅ Complete hybrid architecture implementation
- ✅ Node.js frontend with interactive CLI
- ✅ Python backend with all original features
- ✅ Enhanced secret redaction system
- ✅ Robust error handling and validation
- ✅ NPM distribution support
- ✅ Comprehensive documentation

### v0.83 (Legacy)
- Original Python-only Context Engine
- Basic CLI interface
- Core compression and bundling functionality
- Preserved in `v0.83` branch for reference

## Contributing

### Development Setup
1. Fork the repository
2. Create feature branch
3. Install dependencies: `npm run install:backend`
4. Make changes
5. Run tests: `npm test`
6. Submit pull request

### Code Style
- **JavaScript**: ESLint configuration
- **Python**: PEP 8 compliance
- **Documentation**: Comprehensive comments and README files

### Issue Reporting
- **Bug Reports**: Use GitHub Issues with detailed reproduction steps
- **Feature Requests**: Describe use case and proposed implementation
- **Security Issues**: Report privately to maintainers

## License

MIT License - see LICENSE file for complete terms.

---

**Context Engine v1.0.0 — Compress the Chaos.**

*This document represents the complete architectural specification for the Context Engine hybrid CLI tool. For implementation details, refer to the source code in the respective directories.*