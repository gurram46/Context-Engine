# Context Engine

> **Index once. Give every coding agent high-precision repository context.**

Context Engine is a **local, agent-agnostic repository intelligence layer for coding agents**. It indexes a repository once, retrieves and packs focused evidence, records exactly what context was delivered, and lets a later agent/session determine whether that historical evidence is still valid.

Context Engine retrieves repository context. **It does not run the coding agent.**

## Current status

The canonical implementation is the Rust workspace on `main`.

- **R1 — Resource Hardening:** complete
- **P1 — Context Proof:** complete
- **P2 — Context Delta:** complete
- **P3 — Cross-Agent Handoff:** complete

The current continuity model is:

```text
Agent A
  |
  v
context_search
  |
  +--> fresh packed repository context
  +--> Context Proof A
              |
              | repository changes
              v
         Context Delta
              |
              v
Agent B --> context_search(query, fromProofId=A)
              |
              +--> historical evidence status
              +--> fresh current context
              +--> Context Proof B
```

No previous chat transcript is required for the handoff.

## What it does

Context Engine combines:

- structural indexing and symbol extraction
- exact/lexical retrieval
- BM25 retrieval
- semantic/vector retrieval
- dependency and test lookup
- ranking/fusion
- deterministic evidence packing
- incremental repository freshness tracking
- a shared user-level daemon with process-wide resource management
- Context Proof
- Context Delta
- Cross-Agent Handoff

Proof/Delta correctness is deliberately conservative: when historical evidence cannot be identified safely, Context Engine returns **UNVERIFIABLE** rather than guessing.

## Public MCP surface

The public MCP API intentionally stays small:

1. `context_search`
2. `symbol_lookup`
3. `dependency_trace`
4. `test_lookup`
5. `context_status`
6. `context_delta`

Cross-agent handoff does **not** add another tool. It extends `context_search` with an optional historical proof ID.

## Build from source

Prerequisite: a current Rust toolchain.

```bash
cargo build --release -p contextd
```

Binary:

```text
target/release/contextd
# Windows:
target/release/contextd.exe
```

## Quick start

From a repository you want Context Engine to understand:

```bash
# Build/update the structural index
contextd --root . index

# Search the repository
contextd --root . --json search "Where is retry policy enforced?"

# Inspect engine/index state
contextd --root . --json status

# Look up a symbol
contextd --root . --json symbol retry_policy

# Trace dependencies
contextd --root . --json dependency retry_policy --direction callers

# Find related tests
contextd --root . --json tests retry_policy
```

### Context Proof and Delta

An external `context_search` returns proof metadata.

Later, inspect what happened to that historical evidence:

```bash
contextd --root . --json delta <proof-id>
```

Delta evidence is classified as:

- `UNCHANGED`
- `CHANGED`
- `REMOVED`
- `UNVERIFIABLE`

Cross-file `MOVED` detection is intentionally deferred rather than guessed.

### Cross-agent handoff

Continue the same repository question from a historical proof:

```bash
contextd --root . --json search "Where is retry policy enforced?" --from-proof <proof-id>
```

A successful handoff returns:

- fresh current context
- historical Delta metadata
- a new Context Proof
- a new handoff reference for the next agent/session

## MCP

Run the stdio MCP server:

```bash
contextd --root /path/to/repository mcp
```

Configure your MCP-capable coding agent to launch that command. The protocol is agent-neutral; runtime logic is not tied to Codex, Claude Code, OpenCode, Cursor, or another specific agent.

## Shared daemon

Context Engine uses one user-level global daemon so multiple coding-agent clients do not each duplicate heavyweight repository resources.

The daemon owns:

- repository runtime registry
- process-wide memory budget
- shared semantic-indexing concurrency
- per-repository freshness/runtime state

Normally clients attach automatically. The daemon can also be launched directly for diagnostics:

```bash
contextd daemon
```

## Local storage

Repository-local state lives under:

```text
.context/
  index/
    structural.db
  proofs/
    proofs.db
```

The proof database is intentionally separate from the disposable structural index so proof history can survive an index rebuild.

Context Engine does **not** persist old packed context markdown as conversational memory.

## Rust workspace

```text
crates/
  context-core/    shared types and API contracts
  context-index/   discovery, structural index, BM25, vectors, watcher
  context-rank/    query classification, ranking/fusion, evidence packer
  contextd/        service, daemon, CLI, MCP, Proof, Delta, handoff
```

Additional important directories:

```text
bench/             controlled evaluation/benchmark work
docs/              architecture, specs and implementation plans
skills/            Context Engine agent integration helpers
```

### Legacy/reference code

`backend/`, `ui/`, and `v2/` contain earlier Python/Node/TypeScript implementations or compatibility/reference material. They are **not the canonical production runtime**.

New core development should start in the Rust workspace unless a task explicitly targets legacy packaging.

## Development

Start with:

- [HANDOFF.md](HANDOFF.md) — current architecture, invariants, known limitations and next work
- [CONTRIBUTING.md](CONTRIBUTING.md) — branch and repository hygiene rules

Core checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --test p3_cross_agent_handoff -- --test-threads=1
cargo test --test p2_context_delta -- --test-threads=1
cargo test --test p2_real_daemon -- --test-threads=1
cargo test --test p1_context_proof -- --test-threads=1
cargo test --test r1_resource -- --test-threads=1
cargo test --test r1_1_global -- --test-threads=1
cargo test --test r1_2_closure -- --test-threads=1
cargo build --release -p contextd
```

The full `contextd --lib` suite includes tests that instantiate real OS filesystem watchers. See `HANDOFF.md` for the documented Linux `notify` test-timing caveat and CI policy.

## What is next

The next product milestone is **controlled real-agent evaluation and measured hardening**.

The goal is to measure whether Context Engine actually improves coding-agent work:

- task completion
- unnecessary repository exploration
- tool calls
- context usage
- continuity across independent agent sessions

Further architecture should be driven by those measurements rather than by adding speculative features.

## License

MIT
