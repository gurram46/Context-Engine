# ADR 001 — Choose `rmcp` for MCP transport

- Date: 2026-08-09
- Status: accepted (R0)
- Owner: contextd

## Context

R0 must expose the same 5 MCP tools over stdio and delegate to the existing Node V2 child. We need a Rust MCP implementation that handles `initialize`, `tools/list`, `tools/call`, JSON-RPC framing, and stdio correctly on Windows.

## Decision

Use `rmcp` `3.1.2` (official Rust SDK, `modelcontextprotocol/rust-sdk`) with features `server`, `client`, `transport-io`, `transport-child-process`, `macros`.

- Server: `rmcp::transport::stdio()` + `#[tool_router]` / `#[tool]` macros + `schemars` for schemas.
- Client (bridge): `rmcp::transport::TokioChildProcess` to spawn `node v2/dist/mcp/server.js` as a child and `call_tool`.

## Alternatives

- `mcp-rs` / community crates: less mature, fewer transports, weaker docs.
- Manual JSON-RPC over `tokio::io::stdin/stdout`: ~150 lines, but must reimplement framing, request IDs, notifications, and error codes; easy to corrupt stdio.

## Consequences

- Small dependency count (adds `tokio-util`, `schemars`, `process-wrap`) but avoids owning stdio correctness.
- `rmcp` requires Rust 1.88+, `tokio` full, and `client` feature for `RoleClient`. Our workspace uses `1.97` gnu, fine.
- Server logs must go to `stderr` (rmcp uses `stdout` for JSON-RPC).
- Future R3-R5 still use `rmcp` for the same transport; no migration needed.
