# hkask-capability

OCAP (Object Capability) delegation token system for hKask. Implements P4 Clear Boundaries through explicit capability tokens that govern tool dispatch and resource access.

## Core Types

- `DelegationToken` — Time-bounded, scope-limited capability token
- `CapabilityChecker` — Verifies tokens before tool dispatch
- `CapabilityStore` — Persists and retrieves capability grants

## Design

Every MCP tool dispatch and resource access passes through `CapabilityChecker`. No ambient authority — every action requires an explicit, verified token. This is the enforcement boundary for P1 (User Sovereignty) and P2 (Affirmative Consent).

## See Also

- [`PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) §P4 — Clear Boundaries
- [`AGENTS.md`](../../AGENTS.md) — Agent Operating Guide
