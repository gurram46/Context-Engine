# Context Engine Skill Installation

Canonical skill: `skills/context-engine/SKILL.md` (source of truth)

## Codex
```bash
mkdir -p ~/.codex/skills/context-engine
cp skills/context-engine/SKILL.md ~/.codex/skills/context-engine/SKILL.md
```
Or use install script: `bash skills/context-engine/install.sh --codex`

## OpenCode
OpenCode loads skills from `.opencode/skills/` or `opencode.json` `skills` array:
```bash
mkdir -p .opencode/skills/context-engine
cp skills/context-engine/SKILL.md .opencode/skills/context-engine/SKILL.md
```
Or `bash skills/context-engine/install.sh --opencode`

## Claude Code
```bash
mkdir -p ~/.claude/skills/context-engine
cp skills/context-engine/SKILL.md ~/.claude/skills/context-engine/SKILL.md
```
Or `bash skills/context-engine/install.sh --claude`

All wrappers source the same conceptual instructions — no forked skill bodies.
For platform-specific metadata wrappers, keep them tiny and source `SKILL.md`.

Use `install.sh`/`install.ps1` for automated copy/link.
