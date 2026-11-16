# Context Engine — Engineering Handoff

## Canonical branch

`main` is the only canonical integrated development branch.

Historical milestone branches are not sources of truth. New work should branch from `main`.

## Product

Context Engine is a local, agent-agnostic repository intelligence layer for coding agents.

Core thesis:

> Index once. Give every coding agent high-precision repository context.

## Current architecture status

- **R1 Resource Hardening** — complete
- **P1 Context Proof** — complete
- **P2 Context Delta** — complete
- **P3 Cross-Agent Handoff** — complete

The continuity model is:

```text
context_search
  -> Context Proof A
  -> repository changes
  -> Context Delta
  -> context_search(query, fromProofId=A)
  -> fresh current context + Proof B
```

## Public MCP surface

Keep the public surface small:

- `context_search`
- `symbol_lookup`
- `dependency_trace`
- `test_lookup`
- `context_status`
- `context_delta`

Cross-agent handoff extends `context_search` through optional `fromProofId`; it is not a seventh MCP tool.

## Important invariants

- One user-level shared global daemon.
- Resource management is process-wide, not duplicated per coding agent.
- Proof storage is independent from the disposable structural index.
- Historical packed markdown/source snippets are not persisted as handoff memory.
- Proof identity is tied to repository/source-state semantics, not chat history.
- Delta prefers `UNVERIFIABLE` over guessing ambiguous evidence.
- Cross-agent continuation must use the canonical search path and create exactly one fresh proof.
- Failed continuity validation must not silently run a normal search.
- Keep generated benchmark working copies, secrets, build output, and local index state out of Git.

## Build and test

From the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p contextd --lib
cargo test --test p3_cross_agent_handoff -- --test-threads=1
cargo test --test p2_context_delta -- --test-threads=1
cargo test --test p2_real_daemon -- --test-threads=1
cargo test --test p1_context_proof -- --test-threads=1
cargo test --test r1_resource -- --test-threads=1
cargo test --test r1_1_global -- --test-threads=1
cargo test --test r1_2_closure -- --test-threads=1
cargo test --workspace -- --test-threads=1
cargo build --release -p contextd
git diff --check
```

## Storage

Repository-local state:

```text
.context/
  index/
    structural.db
  proofs/
    proofs.db
```

Global daemon metadata defaults to the platform-specific user data/runtime directory and can be overridden with `CONTEXTD_GLOBAL_DAEMON_DIR`.

## Known/deferred work

- The `contextd --lib` suite includes tests that construct real OS watchers. Linux `notify` can emit a root-level startup event that races synthetic tracker assertions; CI therefore keeps the full lib suite strict on Windows while P3/P2/P1/R1 integration suites run on both Windows and Linux.
- Cross-file `MOVED` evidence classification is intentionally deferred; P2 v1 is conservative.
- On current GitHub Ubuntu runners, a subset of `contextd --lib` watcher/dirty-state tests report `Unknown` instead of the expected `Dirty/Clean` state. This predates the clean P3 diff and is kept visible as a non-blocking CI diagnostic; do not silently delete or weaken those tests.
- Real controlled agent A/B evaluation remains the next product-validation step after the canonical P3 integration.
- Do not expand the architecture based on speculation; harden only issues demonstrated by tests or measured agent runs.

## Next engineer

Start from `main`, run the Rust gates above, read the P1/P2/P3 integration tests, then continue with controlled agent evaluation and measured hardening.
