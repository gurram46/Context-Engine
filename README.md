# Context Engine V1

A CLI-based Context Engine that reduces token waste (20-30%) when starting AI coding tool sessions by preloading relevant project context.

## Installation

```bash
pip install context-engine
```

## Quick Start

1. Initialize in your project:
```bash
context init
```

2. Add baseline files:
```bash
context baseline add src/architecture.md docs/apis.md
```

3. Bundle context for AI tools:
```bash
context bundle
```

4. In your AI tool (Claude Code, Cursor, etc.):
```
Load .context/context_for_ai.md and continue working on current session
```

## Commands

### Initialization
- `context init` - Create .context/ directory structure

### Baseline Management
- `context baseline add <files>` - Add files to baseline
- `context baseline list` - List baseline files
- `context baseline review` - Review baseline with staleness warnings

### Session Management
- `context save "<note>"` - Append note to session
- `context session-end` - End session with optional AI refresh

### Context Bundling
- `context bundle` - Generate context_for_ai.md
- `context expand <files>` - Add files mid-session
- `context status` - Show token count and warnings

### Cross-Repo
- `context pull-cross` - Pull cross_repo.md from linked repos

## Workflow Example

**Start of session:**
```bash
context bundle
```

**Mid-session expansion:**
```bash
context expand src/auth.js config/auth.yml
```

**End of session:**
```bash
context save "Finished login validation, next: token refresh"
context session-end
```

## Model Integration

Uses Qwen3 Coder 480B A35B via OpenRouter for automated summaries.

## License

MIT
