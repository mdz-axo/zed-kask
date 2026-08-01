---
title: "hkask-capability — Tutorial: Your First Capability Token"
audience: [developers]
last_updated: 2026-07-31
version: "0.3.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [lifecycle]
---

# hkask-capability — Tutorial: Your First Capability Token

This tutorial walks through creating a `DelegationToken` and invoking a
tool through the governed membrane. You will learn how capability matching
gates tool calls, how the action hierarchy (`Execute >= Write >= Read`)
works, and how attenuation limits delegation depth.

## Learning path

```mermaid
flowchart TD
    A[Step 1: Build a token] --> B[Step 2: Invoke through the membrane]
    B --> C[Step 3: Attempt a mismatched call]
    C --> D[Step 4: Check expiry and attenuation]
```

## Steps 1-2: Build a token and invoke

Use `DelegationToken::new` (or `DelegationTokenBuilder`) with a
`DelegationResource::Tool`, a `DelegationAction::Execute`, and a resource ID
matching the target tool:

```rust
let token = DelegationToken::new(
    DelegationResource::Tool,
    "web_search".into(),
    DelegationAction::Execute,
    WebID::from_persona(b"issuer"),
    WebID::from_persona(b"holder"),
);
```

Pass it to `McpRuntime::invoke` (the `ToolPort` impl). The membrane checks
`token.is_valid_for(Tool, "web_search", Execute)` — an exact triple match —
then reserves gas, dispatches, settles, and emits the `reg.tool.*` span.
There is no signing step: tokens are in-process declarations, and the gate
is the capability match, not cryptography.

## Step 3: Attempt a mismatched call

Build a token naming a *different* tool (`"fs_read"`) and pass it to an
invoke of `"web_search"`. The capability match fails and the call is denied
with `ToolPortError::CapabilityDenied`. This is the gate's purpose: it
catches wiring bugs — a cascade step or panel view that names the wrong
tool — at the membrane rather than deep inside a server.

## Step 4: Expiry and attenuation

`builder.expires_at(ts)` sets an expiry; `token.is_expired(now)` reports it.
`builder.attenuation_level(n)` marks delegation depth;
`SYSTEM_MAX_ATTENUATION` (7) caps the chain so delegation graphs stay
auditable.

## See also

- [hkask-capability Reference](./reference.md): the token model and the
  invoke pipeline.
- [hkask-capability Explanation](./explanation.md): why the gate is a
  capability match and not a cryptographic check.
- [hkask-types Reference](../hkask-types/reference.md): the `ToolPort`
  trait that requires tokens.
