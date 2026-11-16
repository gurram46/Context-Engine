# P3 Cross-Agent Handoff v1

## Goal

Allow an independent agent session to continue the same repository question from a persisted Context Proof without transferring conversation history or historical packed context.

## Public Contract

`context_search` gains optional `fromProofId`. The six-tool MCP surface remains unchanged. Every successful external context search adds a compact `handoff` object containing version, proof ID, query, repository instance ID, source-state hash, and packed-context hash. A continuation additionally returns `continuity` containing the one-hop P2 delta, source/generation comparisons, packed-context comparison, and old/new proof IDs.

## Architecture

`ContextService` remains the canonical owner of retrieval. Ordinary search continues through `do_search` followed by `build_proof`. Continuation orchestration validates and loads the historical proof before retrieval, checks conservative query equality, obtains the current snapshot through `access_runtime`, computes `delta_for_proof` against that published snapshot, rejects non-ready delta states, then invokes the same `do_search` path and creates exactly one new proof. The clean runtime snapshot prevents a second reconcile in the canonical search path. No proof schema or persistence changes are made.

The service compares the historical and new proof source-state and packed-context hashes independently. Continuity status is `no_change` only when P2 reports no change and source hashes match; otherwise a successful delta is `updated`. Retrieval-build differences are observable but do not invalidate source continuity.

Local MCP, global daemon, `RemoteClient`, and CLI search all pass `fromProofId` without session state. Daemon responses forward the same additive JSON fields. Failure validation occurs before retrieval and proof creation, with sanitized `HANDOFF_PROOF_NOT_FOUND`, `HANDOFF_QUERY_MISMATCH`, or `HANDOFF_UNVERIFIABLE` errors.

## Error Handling

Unknown proofs, query mismatches, repository identity mismatches, branch/detached-boundary mismatches, unverified current snapshots, and unsupported/corrupt proofs fail closed. P2 remains unchanged and continues to expose its existing `context_delta` operation. A final identity check rejects mixed-state results if the proof produced by the canonical search does not match the continuity target state.

## Verification

Tests cover ordinary additive handoff metadata, same-snapshot and changed/removed evidence, all fail-closed paths, proof-count and retrieval-count invariants, chain continuity, non-Git and Windows path behavior, multi-repository isolation, independent daemon clients, daemon restart, and unchanged P1/P2/R1/D0 surfaces. Full formatting, lint, unit, integration, workspace, release-build, and diff checks are required.
