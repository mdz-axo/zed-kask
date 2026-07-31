# hkask-capability

OCAP (Object Capability) delegation token system for hKask. Implements P4 Clear Boundaries through explicit capability tokens that govern tool dispatch and resource access.

## Core Types

- `DelegationToken` — Time-bounded, scope-limited capability token
- `capabilities_match` — Live OCAP enforcement point, invoked by `McpRuntime` in `hkask-mcp/src/runtime.rs` to gate tool dispatch against a token's declared capability

## Design

Live OCAP enforcement is via `capabilities_match` in the MCP runtime (`hkask-mcp/src/runtime.rs`): every tool dispatch compares the token's declared capability against the tool's required capability, applying the action hierarchy (Execute ≥ Write ≥ Read). No ambient authority — every action requires an explicit, verified token. This is the enforcement boundary for P1 (User Sovereignty) and P2 (Affirmative Consent).

## See Also

- [`PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) §P4 — Clear Boundaries
- [`AGENTS.md`](../../AGENTS.md) — Agent Operating Guide
