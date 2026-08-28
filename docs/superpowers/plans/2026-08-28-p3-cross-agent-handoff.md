# P3 Cross-Agent Handoff v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add process-independent same-repository context continuity by extending `context_search` with `fromProofId`, a fresh canonical search, one P2 delta, and one new proof.

**Architecture:** Keep `ContextService::do_search` and `build_proof` as the only retrieval and proof paths. A thin service orchestration layer loads and validates the historical proof, obtains the current runtime snapshot through `access_runtime`, computes P2 Delta, runs the canonical search once, and decorates the result with continuity and handoff metadata. MCP, daemon, remote client, and CLI only adapt this service contract.

**Tech Stack:** Rust workspace, `context-core` serde/schemars types, `contextd` service/MCP/TCP daemon/CLI, rusqlite proof store, existing P1/P2 integration harnesses.

## Global Constraints

- Keep exactly six public MCP tools and do not add persistence or proof schema fields.
- Do not modify `bench/d0/` or `bench/repos/`.
- Do not store historical packed markdown, source text, snippets, transcripts, or handoff records.
- Failed continuity validation performs zero retrieval passes and creates zero proofs.
- Successful continuity performs exactly one canonical retrieval and creates exactly one new proof.
- Delta performs zero embeddings, semantic searches, or vector loads.
- Preserve existing repository identity, branch, detached-HEAD, non-Git, and Windows `\\?\` semantics.
- `continuity.toSourceStateHash` and `continuity.toGeneration` must match Proof B when present.
- Do not transfer chat history or encode agent-specific behavior.

---

### Task 1: Add Continuity Domain Types and Service Orchestration

**Files:**
- Modify: `crates/context-core/src/lib.rs:10-23`
- Modify: `crates/contextd/src/service.rs:167-186,847-960`
- Modify: `crates/contextd/src/lib.rs:5-15`
- Test: `crates/contextd/tests/p3_cross_agent_handoff.rs`

**Interfaces:**
- `ContextSearchParams` produces optional `fromProofId: Option<String>`.
- `ContextService::search_external_with_continuity(query, opts, from_proof_id)` produces `SearchWithProof` with additive `handoff` and optional `continuity`.
- Continuity metadata serializes camelCase fields and embeds the existing `ContextDelta` without old context text.

- [ ] **Step 1: Write failing service tests** for ordinary handoff metadata, query mismatch, unknown proof, same snapshot, changed evidence, and exactly-one-proof behavior. Use existing temporary repository/service helpers and count proofs with `proof::count_proofs`.
- [ ] **Step 2: Run the focused test** with `cargo test -p contextd --test p3_cross_agent_handoff -- --test-threads=1`; verify compilation/test failures identify the missing parameter and method.
- [ ] **Step 3: Add `fromProofId`** as a serde/schemars-defaulted field without changing existing parameter names or tool count.
- [ ] **Step 4: Add serializable `HandoffReference`, `ContinuityMetadata`, and `ContinuityStatus`** in `service.rs`; derive metadata from the persisted historical proof and the resulting ProofMeta. Include source/packed hashes, proof IDs, query, repo ID, delta, and retrieval-build comparison where available.
- [ ] **Step 5: Implement continuity preflight** that opens the existing proof DB, returns `HANDOFF_PROOF_NOT_FOUND` for absent IDs, compares `query.trim()` values, and returns `HANDOFF_QUERY_MISMATCH` before calling `access_runtime` or `do_search`.
- [ ] **Step 6: Call `access_runtime` once before Delta**, then call `delta_for_proof` in `spawn_blocking`; reject `Unverifiable` as `HANDOFF_UNVERIFIABLE` with sanitized reason. Ensure current runtime access is published before Delta and leaves the canonical `do_search` access clean.
- [ ] **Step 7: Invoke `do_search` exactly once and `build_proof` exactly once**, then assert the produced proof state matches the continuity target state; return a conservative internal error rather than mixed-state metadata if it does not.
- [ ] **Step 8: Keep `search_external` as the ordinary wrapper** delegating to the same internal path with no historical proof; ordinary output gains only `handoff`.
- [ ] **Step 9: Run focused tests** and verify ordinary, successful, and failed service paths pass with proof counts of `+1` and `+0` respectively.
- [ ] **Step 10: Commit** with `git add crates/context-core/src/lib.rs crates/contextd/src/lib.rs crates/contextd/src/service.rs crates/contextd/tests/p3_cross_agent_handoff.rs && git commit -m "feat: add P3 continuity orchestration"`.

### Task 2: Thread Continuity Through MCP, Daemon, Remote Client, and CLI

**Files:**
- Modify: `crates/contextd/src/mcp.rs:248-295`
- Modify: `crates/contextd/src/daemon.rs:133-159,291-305,893-961`
- Modify: `crates/contextd/src/cli.rs:40-180`
- Test: `crates/contextd/tests/p3_cross_agent_handoff.rs`

**Interfaces:**
- MCP `context_search` passes `params.fromProofId` to local service and includes it in remote JSON.
- Daemon accepts `fromProofId` in the existing `context_search` request and serializes continuity unchanged.
- CLI `Search` accepts `--from-proof <ID>` and uses the same service method; JSON includes full additive metadata and human output stays compact.

- [ ] **Step 1: Extend local MCP tests** to assert `fromProofId` reaches the service and ordinary responses contain `handoff` while no-continuity responses omit `continuity`.
- [ ] **Step 2: Pass `fromProofId`** through local MCP, global daemon request parsing, and daemon service invocation without changing any other tool handler.
- [ ] **Step 3: Update remote forwarding** to include `fromProofId` and preserve the daemon result object rather than reconstructing or dropping continuity fields.
- [ ] **Step 4: Add the CLI flag** to the existing `Search` subcommand and use the canonical service method; add JSON `handoff`/`continuity` and concise human lines for from-proof, new proof, delta status/counts, and source/packed changes.
- [ ] **Step 5: Run MCP/daemon/CLI focused tests** and verify the tool list remains exactly the existing six names.
- [ ] **Step 6: Commit** with `git add crates/contextd/src/mcp.rs crates/contextd/src/daemon.rs crates/contextd/src/cli.rs crates/contextd/tests/p3_cross_agent_handoff.rs && git commit -m "feat: expose P3 continuity across transports"`.

### Task 3: Add Full P3 Continuity and Process Independence Tests

**Files:**
- Create: `crates/contextd/tests/p3_cross_agent_handoff.rs`
- Modify only if needed: `crates/contextd/Cargo.toml` for existing test support, never `bench/d0/` or `bench/repos/`

**Interfaces:**
- Tests use only public service/daemon APIs and existing harness utilities; no chat history or in-memory handoff state is passed.

- [ ] **Step 1: Add same-snapshot and unrelated-edit tests** asserting `NO_CHANGE`, `UPDATED`, stale historical proof, unchanged historical evidence, fresh context, and matching Proof B source hash/generation.
- [ ] **Step 2: Add relevant-edit and removal tests** asserting `CHANGED > 0` and `REMOVED > 0` while returning fresh current context.
- [ ] **Step 3: Add fail-closed tests** for branch mismatch, repo mismatch, current unverified state, malformed schema, unknown proof, and query mismatch; assert no search/proof/embedding side effects where instrumentation exists.
- [ ] **Step 4: Add A→B→C chain test** asserting distinct proof IDs, `B.fromProofId == A`, `C.fromProofId == B`, one-hop delta only, and state coherence for both new proofs.
- [ ] **Step 5: Add non-Git, detached-HEAD, and Windows-prefixed-root regression coverage** by reusing existing P1/P2 fixture patterns and asserting the established boundary behavior remains unchanged.
- [ ] **Step 6: Add a real TCP two-client test**: create Client A, call daemon search, drop it, edit the repository, create Client B with a different connection, call with A's proof ID, and assert continuity plus one new proof.
- [ ] **Step 7: Add daemon restart coverage** using the existing bounded lifecycle harness: obtain Proof A, stop/restart the daemon with the same root/proof DB, then continue from A with a new client.
- [ ] **Step 8: Record smoke telemetry** from result stats and proof counts for delta duration, search duration, total wall time, embeddings, vector loads when available, and duplicate pass indicators.
- [ ] **Step 9: Run `cargo test --test p3_cross_agent_handoff -- --test-threads=1`** and fix only P3 code or test harness issues; verify no P1/P2 behavior changes.
- [ ] **Step 10: Commit** with `git add crates/contextd/tests/p3_cross_agent_handoff.rs && git commit -m "test: verify cross-agent handoff"`.

### Task 4: Full Verification and Branch Publication

**Files:**
- Verify all changed files; do not modify `bench/d0/` or `bench/repos/`.

- [ ] **Step 1: Run formatting** `cargo fmt --all -- --check`; expected exit 0.
- [ ] **Step 2: Run lint** `cargo clippy --workspace --all-targets --all-features -- -D warnings`; expected exit 0.
- [ ] **Step 3: Run requested targeted tests** for contextd lib, P3, P2, P2 real daemon, P1, R1 resource, R1.1 global, and R1.2 closure, each with `--test-threads=1` where applicable.
- [ ] **Step 4: Run full workspace tests** `cargo test --workspace -- --test-threads=1`; expected exit 0.
- [ ] **Step 5: Run release build** `cargo build --release -p contextd`; expected exit 0.
- [ ] **Step 6: Run `git diff --check`** and inspect `git status --short`; only the permitted untracked benchmark directories may remain untracked.
- [ ] **Step 7: Verify frozen surfaces** with `git diff -- bench/d0 bench/repos`, public tool-name inspection, and proof schema version inspection; expected no D0 changes, six tools, and schema version 1.
- [ ] **Step 8: Push branch** with `git push -u origin p3/cross-agent-handoff`, then verify `git rev-parse HEAD` equals `git ls-remote origin refs/heads/p3/cross-agent-handoff`.
- [ ] **Step 9: Produce the requested closure report** with exact gate outcomes, proof IDs/hashes from tests, commits, remote verification, blockers, and final verdict. Do not merge or start D0.

## Self-Review

- Spec coverage is mapped across service orchestration, all transport adapters, failure semantics, coherence, process independence, persistence invariants, and the full required gate matrix.
- No proof schema migration, handoff database, old markdown storage, retrieval backend, or seventh tool is planned.
- The service method signatures are consistent: transport callers provide `Option<String>`, ordinary callers pass `None`, and all successful calls return `SearchWithProof` with additive metadata.
