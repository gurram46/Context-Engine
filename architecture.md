# Context Engine Architecture

This project is a Python CLI that prepares a compact, privacy‑aware project context for AI coding tools. It avoids token waste by bundling a small set of baseline documents plus on‑demand “expanded” file summaries into a single Markdown file: `.context/context_for_ai.md`.

## Goals
- Reduce token usage (20–30%) at session start.
- Keep sensitive data out of prompts via redaction.
- Provide deterministic, fixed section output for AI tools.

## Components
- CLI (`context`): Click‑based entrypoint in `context_engine/cli.py`.
- Commands (`context_engine/commands/…`):
  - `init`: create `.context/` layout and sample ADR.
  - `baseline`: add/list/review baseline files.
  - `bundle`: generate `.context/context_for_ai.md` from sources.
  - `expand`: append compressed file summaries mid‑session.
  - `save` and `session-end`: session note capture.
  - `status`: show token counts and warnings.
  - `pull-cross`: import cross‑repo notes.
  - `config`: `show/set/unset/path` for `.context/config.json`.
- Core (`context_engine/core`):
  - `config.py`: config defaults, paths, getters/setters.
  - `utils.py`: hashing, staleness, token counting (tiktoken),
    secret redaction (regex + entropy), compression helpers.
- Models (`context_engine/models/openrouter.py`): optional AI summarization via OpenRouter (Qwen3 Coder); graceful fallback to manual bundling.
- UI helper (`context_engine/ui.py`): consistent colored output with global `--no-color`.

## Data Model & Files
- `.context/` (generated per project):
  - `config.json`: settings (allowed extensions, token limits, model, linked repos).
  - `baseline/`: copies of selected project files (architecture, apis, config, schema).
  - `adrs/`: architecture decision records (markdown).
  - `session.md`: timestamped notes across sessions.
  - `cross_repo.md`: imported notes from linked repos.
  - `hashes.json`: file hash map for staleness checks.
  - `context_for_ai.md`: final bundle with fixed sections.

## Typical Flow
1) `context init` → create `.context/` and defaults.
2) `context baseline add <files>` → copy allowed files into `.context/baseline/` and record hashes.
3) `context bundle [--no-ai]` → read baseline + session + cross‑repo + expanded; redact secrets; compress/deduplicate; write `.context/context_for_ai.md`.
4) `context expand <files>` → append compressed previews to the “Expanded Files” section.
5) `context status` → token count, staleness, session activity, config summary.

## Security & Safety
- Path traversal protection: validates that added files are within the project root.
- Intake guardrails: allowed extensions + size limits (configurable).
- Secret hygiene: regex patterns (API keys, AWS, passwords/tokens) + entropy‑based masking for high‑entropy tokens.
- API key validation: format checks before calling OpenRouter.

## Bundling Rules (deterministic)
- Fixed section order: Architecture, APIs, Configuration, Database Schema, Session Notes, Cross‑Repo Notes, Expanded Files.
- Empty sections rendered as `None`.
- Optional AI summarization applies strict compression (keep docstrings, drop inline comments and raw code). Manual path preserves structure without AI.

