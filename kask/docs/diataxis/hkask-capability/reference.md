---
title: "hkask-capability — Reference"
audience: [developers, architects, agents]
last_updated: 2026-07-29
version: "0.2.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [domain, trust]
---

# hkask-capability — Reference

`hkask-capability` implements the Object Capability (OCAP) layer for hKask. It
defines `DelegationToken`, the `ToolPort` trait, and the `capabilities_match`
function that verifies tokens before tool invocation. Every tool call in the
governed dispatch path requires a valid `DelegationToken` proving authorization.
(Enforcement lives in `hkask-mcp/src/runtime.rs`; the former
`CapabilityChecker` struct and `verify_delegation_token*` helpers in this
crate were removed.)

## Source citations

| Symbol | Location |
|--------|----------|
| `DelegationToken` struct | `kask/crates/hkask-capability/src/token_types.rs:58` |
| `DelegationTokenBuilder` | `kask/crates/hkask-capability/src/token_types.rs:90` |
| `TokenSignature` newtype | `kask/crates/hkask-capability/src/token_types.rs:50` |
| `Caveat` struct | `kask/crates/hkask-capability/src/token_types.rs:40` |
| `CapabilityError` enum | `kask/crates/hkask-capability/src/token_types.rs:15` |
| `TokenRegistry` trait | `kask/crates/hkask-capability/src/token_types.rs:574` |
| `NoOpTokenRegistry` | `kask/crates/hkask-capability/src/token_types.rs:613` |
| `ToolPort` trait | `kask/crates/hkask-capability/src/tool_port.rs:47` |
| `ToolPortError` enum | `kask/crates/hkask-capability/src/tool_port.rs:9` |
| `ToolInfo` struct | `kask/crates/hkask-capability/src/tool_port.rs:73` |
| `capabilities_match` fn (enforcement) | `kask/crates/hkask-mcp/src/runtime.rs` |
| `require_write_access` | `kask/crates/hkask-capability/src/verification/verify.rs:114` |
| `require_read_access` | `kask/crates/hkask-capability/src/verification/verify.rs:140` |
| `VerificationOutcome` enum | `kask/crates/hkask-capability/src/verification/types.rs:22` |
| `CapabilitySpec` struct | `kask/crates/hkask-capability/src/resources.rs:8` |
| `DelegationResource` enum | `kask/crates/hkask-capability/src/resources.rs:51` |
| `DelegationAction` enum | `kask/crates/hkask-capability/src/resources.rs:80` |
| `capabilities_match` fn | `kask/crates/hkask-capability/src/resources.rs:131` |
| `capability_from_server_id` fn | `kask/crates/hkask-capability/src/resources.rs:117` |

## Token and verification model

The `DelegationToken` (`token_types.rs:58`) is the core capability object. It
carries a resource, a resource_id, an action, a delegation chain (from/to
WebIDs), an Ed25519 signature, an Ed25519 public key, an optional expiry, an
attenuation level, a max attenuation cap, a context nonce, and a list of
caveats. The `DelegationTokenBuilder` (`token_types.rs:90`) constructs tokens
with the required fields and signs them with an Ed25519 key via `sign()`.

Token verification is performed by `capabilities_match` (in
`hkask-mcp/src/runtime.rs`), which checks signature, expiry, and capability
against the request. The former `CapabilityChecker` struct that held an
optional signing key, trusted root public keys, and an `enforce_roots` flag
was removed; the verification logic now lives in `hkask-mcp`.

```mermaid
classDiagram
    class DelegationToken {
        +id: String
        +resource: DelegationResource
        +resource_id: String
        +action: DelegationAction
        +delegated_from: WebID
        +delegated_to: WebID
        +signature: TokenSignature
        +public_key: Ed25519PublicKey
        +expires_at: Option~i64~
        +attenuation_level: u8
        +max_attenuation: u8
        +context_nonce: String
        +caveats: Vec~Caveat~
    }
    class DelegationTokenBuilder {
        +sign() DelegationToken
    }
    class TokenSignature {
        <<newtype>>
        +[u8; 64]
    }
    class Caveat {
        +caveat_id: String
        +data: String
    }
    class ToolPort {
        <<interface>>
        +invoke(server, tool, args, token) ToolFuture
        +discover_tools() ToolFuture~Vec~String~~
        +get_tool_info(name) ToolFuture~Option~ToolInfo~~
    }
    class VerificationOutcome {
        <<enumeration>>
        Valid
        InvalidSignature
        Expired
        InsufficientAccess
        NoChecker
    }
    class DelegationResource {
        <<enumeration>>
        Tool
        Template
        Registry
        Key
    }
    class DelegationAction {
        <<enumeration>>
        Read
        Write
        Execute
    }

    DelegationToken --> TokenSignature
    DelegationToken --> Caveat
    DelegationToken --> DelegationResource
    DelegationToken --> DelegationAction
    DelegationTokenBuilder ..> DelegationToken : creates
    capabilities_match --> DelegationToken : verifies
    ToolPort --> DelegationToken : requires
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-CAP-001
verified_date: 2026-07-29
verified_against: kask/crates/hkask-capability/src/token_types.rs:58,90,50,40; kask/crates/hkask-capability/src/tool_port.rs:47,73; kask/crates/hkask-capability/src/verification/types.rs:22; kask/crates/hkask-capability/src/resources.rs:51,80; kask/crates/hkask-mcp/src/runtime.rs (capabilities_match)
status: VERIFIED
-->

## ToolPort trait

The `ToolPort` trait (`tool_port.rs:47`) is the actuator boundary for governed
tool dispatch. The `invoke` method requires a `DelegationToken` as a
parameter. The `discover_tools` and `get_tool_info` methods return public
tool metadata and require no token, because tool schemas are public per the
MCP protocol design.

The trait returns `ToolFuture` (`Pin<Box<dyn Future + Send + '_>>`), a pinned
boxed future, because tool invocation is asynchronous. The trait is
object-safe: `Arc<dyn ToolPort>` works. The `ToolPortError` enum
(`tool_port.rs:9`) includes `CapabilityDenied` for insufficient-token
failures, `EnergyBudgetExceeded` for gas exhaustion, `NotFound` for missing
tools, and `InvocationFailed` for runtime errors.

Implementor: `BridgeToolPort` in `kask/crates/kask_bridge/src/tool_port.rs:25`,
which wraps zed's `McpRuntime` and enforces OCAP, gas, and span emission.

## Verification functions

Two public functions in `verification/verify.rs` perform token verification:

- `require_write_access` (`verify.rs:114`) returns an error if the token
  does not grant write-level access on the given store.
- `require_read_access` (`verify.rs:140`) returns an error if the token does
  not grant read-level access on the given store.

Note: the structured `VerificationOutcome` is produced by `capabilities_match`
in `hkask-mcp/src/runtime.rs`. The former `verify_delegation_token` /
`verify_delegation_token_now` free functions and the `CapabilityChecker` struct
in this crate were removed; enforcement now lives in `hkask-mcp`.

The `VerificationOutcome` enum (`verification/types.rs:22`) has five
variants: `Valid`, `InvalidSignature`, `Expired`, `InsufficientAccess {
resource_id, action }`, and `NoChecker`. The `InsufficientAccess` variant
carries the `resource_id` and `action` that were denied. The `NoChecker`
variant indicates that no checker was configured, which denies access by
default.

## Resource and action model

The `DelegationResource` enum (`resources.rs:51`) has four variants: `Tool`,
`Template`, `Registry`, and `Key`. The `DelegationAction` enum
(`resources.rs:80`) has three variants: `Read`, `Write`, and `Execute`. The
action hierarchy is `Execute >= Write >= Read`: a token with `Execute` action
satisfies requests for `Write` or `Read`; a `Write` token satisfies `Read`
requests but not `Execute`.

The `capabilities_match` function (`resources.rs:131`) compares a token's
declared capability against a required capability, applying the action
hierarchy. The `capability_from_server_id` function (`resources.rs:117`) maps
an MCP server ID (e.g. `hkask-mcp-<domain>` or short `<domain>`) to a
capability string `tool:<domain>:execute`, used when constructing tokens for
server-scoped access.

## See also

- [hkask-capability Explanation](./explanation.md): state diagram of token
  verification outcomes and the OCAP rationale.
- [hkask-types Reference](../hkask-types/reference.md): the `ToolPort` trait
  appears in both crates; this crate defines it, `hkask-types` re-exports it.
- [`kask/docs/architecture/core/PRINCIPLES.md`](../../architecture/core/PRINCIPLES.md):
  P4 (Clear Boundaries) and P4.1 (Pod Boundary as OCAP Enforcement Perimeter).
- [`kask/docs/architecture/core/magna-carta.md`](../../architecture/core/magna-carta.md):
  sovereignty principles P1 through P4.

---

[^miller-ocap]: Miller, M. S. (2006). *Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control.* Johns Hopkins University. <https://www.erights.org/talks/thesis/markm-thesis.pdf>. The Object Capability model: access is granted by possession of an unforgeable capability token, not by ambient authority.

[^ed25519]: Bernstein, D. J., Duif, N., Lange, T., Schwabe, P., & Yang, B. Y. (2012). *High-speed high-security signatures.* Journal of Cryptographic Engineering, 2(2), 77-89. <https://ed25519.cr.yp.to/>. The Ed25519 signature scheme used for `TokenSignature`.
