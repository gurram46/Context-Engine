# Project APIs

This document summarizes the primary user‑facing commands provided by the `context` CLI, with brief guidance on inputs/outputs.

## CLI Commands

- `context init`: Initialize `.context/` in the current project and create default files.
- `context baseline add <files>`: Copy selected files into `.context/baseline/` for bundling.
- `context baseline list`: Show files currently in the baseline.
- `context baseline review`: Check staleness vs. stored hashes.
- `context bundle [--no-ai]`: Generate `.context/context_for_ai.md` (fixed, deterministic sections).
- `context expand <files>`: Append compressed summaries to the “Expanded Files” section.
- `context save "<note>"`: Append a timestamped note to `.context/session.md`.
- `context session-end [--refresh]`: Mark session end; optionally refresh the bundle.
- `context status`: Display bundle token count, staleness, and config info.
- `context pull-cross`: Pull `.context/cross_repo.md` from linked repos.
- `context config [show|set|unset|path]`: View or modify `.context/config.json`.

## Notes

- Allowed file types for baseline/expand are configured via `allowed_extensions` in `.context/config.json`.
- Secret redaction is applied to bundled/expanded content (regex + entropy heuristics).
