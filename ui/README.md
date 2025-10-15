# Context Engine CLI

A powerful hybrid CLI tool for compressing and bundling project context with Node.js frontend and Python backend.

## Features

- 🚀 **Interactive Mode**: Beautiful ASCII art welcome screen with command palette
- 🎯 **Node.js Frontend**: Modern CLI interface with chalk colors and inquirer prompts
- 🐍 **Python Backend**: Robust Context Engine core functionality
- 📦 **Easy Installation**: Install via npm for global usage
- 🎨 **Orange-on-Black Theme**: Sleek terminal UI matching Context Engine branding

## Installation

### Global Installation
```bash
npm install -g @contextengine/cli
```

### Local Installation
```bash
npm install @contextengine/cli
```

## Quick Start

### Interactive Mode (Recommended)
```bash
context-engine
```
This shows the beautiful welcome screen and interactive command palette.

### Direct Commands
```bash
# Initialize project
context-engine init

# Create bundle
context-engine bundle

# Compress source files
context-engine compress

# Generate baseline
context-engine baseline

# Show command palette
context-engine palette
```

## Commands

| Command | Description | Options |
|---------|-------------|---------|
| `init` | Initialize Context Engine in current directory | |
| `baseline` | Generate project baseline | `-o, --output <file>` (default: BASELINE.md) |
| `bundle` | Create context bundle | `-o, --output <file>` (default: bundle.md)<br>`-f, --format <format>` (default: markdown) |
| `compress` | Compress source files | `-o, --output <file>` (default: compressed.md)<br>`-r, --recursive` (recursive compression) |
| `palette` | Show interactive command palette | |
| `welcome` | Show welcome screen and command palette | |
| `help` | Show help information | |

## Interactive Mode

The interactive mode features:

- **ASCII Art Logo**: Beautiful "C✱ NTXT ENGINE" logo with orange gradient
- **Dynamic Project Info**: Shows current working directory
- **Command Palette**: Interactive selection of available commands
- **Quick Tips**: Helpful getting started information
- **Recent Activity**: Session history tracking

## Examples

### Initialize a New Project
```bash
context-engine init
```

### Create a Custom Bundle
```bash
context-engine bundle -o my-bundle.md -f markdown
```

### Compress All Source Files Recursively
```bash
context-engine compress --recursive
```

### Generate Project Baseline
```bash
context-engine baseline -o project-baseline.md
```

## Architecture

The Context Engine CLI uses a hybrid architecture:

```
context-engine/
├── ui/                    # Node.js frontend
│   ├── index.js          # Main CLI entry point
│   ├── lib/
│   │   ├── welcome.js    # Welcome screen and UI
│   │   └── backend-bridge.js  # Python backend bridge
│   └── bin/
│       ├── context-engine.js  # Global CLI command
│       └── ce.js              # Short CLI command
└── backend/              # Python backend
    ├── main.py          # Backend entry point
    ├── context_engine/  # Core Context Engine modules
    └── requirements.txt # Python dependencies
```

## Requirements

- **Node.js**: >= 16.0.0
- **Python**: >= 3.7
- **Terminal**: Supports ANSI colors and Unicode

## Development

### Setup Development Environment
```bash
# Clone repository
git clone https://github.com/contextengine/cli.git
cd cli

# Install Node.js dependencies
cd ui
npm install

# Set up Python backend
cd ../backend
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate
pip install -r requirements.txt
```

### Development Commands
```bash
# Run in development mode
npm run dev

# Run tests
npm test

# Lint code
npm run lint

# Build for distribution
npm run build
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

MIT License - see LICENSE file for details.

## Support

- **Issues**: [GitHub Issues](https://github.com/contextengine/cli/issues)
- **Documentation**: [Context Engine Docs](https://contextengine.dev)
- **Community**: [Context Engine Discord](https://discord.gg/contextengine)

---

**Context Engine — Compress the Chaos.**