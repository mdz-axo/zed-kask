---
title: "hkask-capability — Explanation"
audience: [developers, architects, agents]
last_updated: 2026-07-31
version: "0.3.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [trust, curation]
---

# hkask-capability — Explanation

The capability layer exists to make tool authority *explicit* at the
dispatch membrane. Every governed tool invocation carries a
`DelegationToken` declaring which resource and action the caller claims.
`McpRuntime::invoke` checks that declaration against the actual call
(capability match) and enforces gas budgets. This replaces ambient authority
(any code can call any tool with no record of what it was allowed to call)
with declared authority (every call names its claimed scope, and mismatches
are denied and logged).

## What the gate is — and is not

Tokens are minted and consumed **in-process** (`panel_default_token` at the
composition root). There is no untrusted transport boundary — the caller and
the gate are in the same address space. Two consequences follow:

1. **No cryptography.** Earlier versions signed tokens with Ed25519 and
   verified the signature at invoke time. The verification was
   self-referential (checked against the public key embedded in the token
   itself, not a trusted authority), so it denied nothing a hostile caller
   couldn't bypass — security theater per the "advertised invariants need
   enforcement points" rule. The signature, public key, `verify()`,
   `derive_signing_key`, and the minting-key threading were removed on
   2026-07-31.
2. **The gate is a consistency check, not a security boundary.** It catches
   manifest/config bugs — a cascade step or panel view naming the wrong
   tool, or a capability string that drifted from the tool's declared
   requirement. It does not (and cannot) defend against a hostile caller
   already executing inside the process; such a caller can mint any token.

If a genuine trust boundary is ever introduced (e.g. tokens crossing a
network or process boundary to an untrusted verifier), cryptographic
verification must be reintroduced *with a trusted root key set* — not the
self-referential check that was removed.

## The invoke pipeline

```mermaid
stateDiagram-v2
    [*] --> CapabilityMatch: invoke(server, tool, args, token)
    CapabilityMatch --> Denied: token does not name this tool+action
    CapabilityMatch --> GasReserve: match
    GasReserve --> BudgetExceeded: insufficient gas
    GasReserve --> Dispatch: reserved
    Dispatch --> Settle: tool result (success or error)
    Settle --> SpanEmit: persist reg.tool.* span
    SpanEmit --> [*]
    Denied --> [*]: CapabilityDenied
    BudgetExceeded --> [*]: EnergyBudgetExceeded
```

The `TokenRegistry` SQL table (`hkask-storage`) records token issuance for
consent audit — the curator MCP server's `list_tokens` tool reads it for
transparency reporting and anomaly detection. The invoke gate does **not**
consult it; a revocation recorded in the registry is an audit fact, not an
authorization decision.

## Attenuation

The `DelegationToken` carries an `attenuation_level` and a `max_attenuation`
field. When a token is delegated from one principal to another, the
attenuation level increases; `SYSTEM_MAX_ATTENUATION` caps the chain depth.
This is a structural bound (cascade depth, subgoal nesting) rather than a
cryptographic one — it limits how deep delegation chains can grow so audit
stays tractable.
