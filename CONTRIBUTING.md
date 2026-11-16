# Contributing

## Branch policy

`main` is the canonical integrated branch.

Create short-lived branches from current `main`:

- `feature/<name>`
- `fix/<name>`
- `bench/<name>`
- `docs/<name>`

When work is merged and verified, delete the feature branch. Do not keep completed milestone branches permanently.

## Before opening a PR

Run the relevant Rust gates, including formatting, clippy, affected integration tests, and a release build when core runtime code changes.

## Repository hygiene

Do not commit:

- secrets or credentials
- `.context/` local index/proof runtime state
- generated benchmark working copies
- raw model run output unless intentionally curated as a fixture/result
- `target/`, `node_modules/`, build artifacts, temp files
- presentation/export artifacts at repository root

## Architecture changes

Keep the MCP surface small and agent-neutral. Major changes to proof identity, source-state semantics, daemon/resource architecture, persistence, or retrieval orchestration require tests and an architecture note.

Prefer deterministic/fail-closed behavior over heuristic guessing for proof/delta correctness.
