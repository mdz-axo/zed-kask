---
title: "hkask-capability — Tutorial: Your First Capability Token"
audience: [developers new to OCAP]
last_updated: 2026-07-29
version: "0.2.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [lifecycle]
---

# hkask-capability — Tutorial: Your First Capability Token

This tutorial walks through creating and verifying a `DelegationToken`. You
will learn how the OCAP model grants access, how attenuation limits
delegation depth, and how `capabilities_match` verifies tokens at the
actuator boundary.

## Learning path

```mermaid
flowchart TD
    A[Step 1: Build a token] --> B[Step 2: Verify the token]
    B --> C[Step 3: Attempt insufficient access]
    C --> D[Step 4: Attenuate and re-delegate]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-CAP-003
verified_date: 2026-07-29
verified_against: kask/crates/hkask-capability/src/token_types.rs:58,90; kask/crates/hkask-mcp/src/runtime.rs (capabilities_match)
status: VERIFIED
-->

## Steps 1-2: Build and verify a token

Use `DelegationTokenBuilder` (`token_types.rs:90`) to construct a token
with a `DelegationResource::Tool`, a `DelegationAction::Execute`, and a
resource ID matching the target MCP server. Sign it with an Ed25519 key via
`builder.sign()`.

Pass the token to `capabilities_match` (in `hkask-mcp/src/runtime.rs`) along with
the holder WebID, resource, resource_id, and action. The function returns
`VerificationOutcome::Valid` if the signature is correct, the token has not
expired, and the resource and action match the request. (The former
`verify_delegation_token_now` / `CapabilityChecker::verify` helpers in
`hkask-capability` were removed; enforcement now lives in `hkask-mcp`.)

## Steps 3-4: Attempt insufficient access and attenuate

Construct a token with `DelegationAction::Read` and attempt to invoke a
tool that requires `Execute`. The verifier returns
`VerificationOutcome::InsufficientAccess { resource_id, action }`. This is
the fail-closed behavior: the token does not grant the requested access.

Now attenuate the token: increase the `attenuation_level` field
(`token_types.rs:58`) and re-delegate to a sub-task. The sub-task's token is
strictly less powerful. When `attenuation_level` reaches `max_attenuation`,
the token cannot be further delegated.

## See also

- [hkask-capability Reference](./reference.md): class diagram of tokens
  and checkers.
- [hkask-capability How-to](./how-to.md): attenuating a token for a
  sub-task.
- [hkask-types Reference](../hkask-types/reference.md): the `ToolPort`
  trait that requires tokens.

---

[^miller-ocap]: Miller, M. S. (2006). *Robust Composition.* <https://www.erights.org/talks/thesis/markm-thesis.pdf>. The Object Capability model that this tutorial demonstrates.
