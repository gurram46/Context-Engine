#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="$ROOT/skills/context-engine/SKILL.md"

install_codex() { mkdir -p "$HOME/.codex/skills/context-engine"; cp "$SRC" "$HOME/.codex/skills/context-engine/SKILL.md"; echo "installed Codex skill"; }
install_opencode() { mkdir -p "$ROOT/.opencode/skills/context-engine"; cp "$SRC" "$ROOT/.opencode/skills/context-engine/SKILL.md"; echo "installed OpenCode skill to .opencode/skills/context-engine"; }
install_claude() { mkdir -p "$HOME/.claude/skills/context-engine"; cp "$SRC" "$HOME/.claude/skills/context-engine/SKILL.md"; echo "installed Claude skill"; }

case "${1:-all}" in
  --codex) install_codex ;;
  --opencode) install_opencode ;;
  --claude) install_claude ;;
  --all|all) install_codex; install_opencode; install_claude ;;
  *) echo "Usage: $0 [--codex|--opencode|--claude|--all]"; exit 1 ;;
esac
