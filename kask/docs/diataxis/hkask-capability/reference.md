---
title: "hkask-capability — Reference"
audience: [developers, architects, agents]
last_updated: 2026-07-27
version: "0.1.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [domain, trust]
---

# hkask-capability — Reference

`hkask-capability` implements the Object Capability (OCAP) layer for hKask. It
defines `DelegationToken`, the `ToolPort` trait, and the `CapabilityChecker`
that verifies tokens before tool invocation. Every tool call in the governed
dispatch path requires a valid `DelegationToken` proving authorization.

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
| `CapabilityChecker` struct | `kask/crates/hkask-capability/src/verification/checker.rs:20` |
| `verify_delegation_token_now` | `kask/crates/hkask-capability/src/verification/verify.rs:22` |
| `verify_delegation_token` | `kask/crates/hkask-capability/src/verification/verify.rs:63` |
| `require_write_access` | `kask/crates/hkask-capability/src/verification/verify.rs:114` |
| `require_read_access` | `kask/crates/hkask-capability/src/verification/verify.rs:140` |
| `VerificationOutcome` enum | `kask/crates/hkask-capability/src/verification/types.rs:22` |
| `CapabilitySpec` struct | `kask/crates/hkask-capability/src/resources.rs:8` |
| `DelegationResource` enum | `kask/crates/hkask-capability/src/resources.rs:51` |
| `DelegationAction` enum | `kask/crates/hkask-capability/src/resources.rs:80` |
| `capabilities_match` fn | `kask/crates/hkask-capability/src/resources.rs:131` |

## Token and verification model

The `DelegationToken` (`token_types.rs:58`) is the core capability object. It
carries a resource, an action, a delegation chain (from/to WebIDs), an Ed25519
signature, an attenuation level, and a list of caveats. The
`DelegationTokenBuilder` (`token_types.rs:90`) constructs tokens with the
required fields and signs them with an Ed25519 key.

The `CapabilityChecker` (`verification/checker.rs:20`) verifies tokens. It
holds a set of trusted root public keys and an `enforce_roots` flag. When
`enforce_roots` is true, a token is accepted only if its embedded public key
is in the trusted set. When false, the checker verifies only the
self-signature, which is the mode used by pod-internal checkers where tokens
are constructed locally.

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
        +caveats: Vec~Caveat~
    }
    class DelegationTokenBuilder {
        +build() DelegationToken
    }
    class TokenSignature {
        <<newtype>>
        +[u8; 64]
    }
    class Caveat {
        +constraint
    }
    class CapabilityChecker {
        -signing_key: Option~SigningKey~
        -trusted_roots: Vec~Ed25519PublicKey~
        -enforce_roots: bool
        +verify(token) VerificationOutcome
        +grant_tool(...) DelegationToken
    }
    class ToolPort {
        <<interface>>
        +invoke(server, tool, args, token) Result
        +discover_tools() Vec~String~
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
    CapabilityChecker --> DelegationToken : verifies
    CapabilityChecker ..> VerificationOutcome : returns
    ToolPort --> DelegationToken : requires
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-CAP-001
verified_date: 2026-07-27
verified_against: kask/crates/hkask-capability/src/token_types.rs:58,90,50,40; kask/crates/hkask-capability/src/tool_port.rs:47; kask/crates/hkask-capability/src/verification/checker.rs:20; kask/crates/hkask-capability/src/verification/types.rs:22; kask/crates/hkask-capability/src/resources.rs:51,80
status: VERIFIED
-->

## ToolPort trait

The `ToolPort` trait (`tool_port.rs:47`) is the actuator boundary for governed
tool dispatch. The `invoke` method requires a `DelegationToken` as a parameter.
The `discover_tools` method returns public tool metadata and requires no
token, because tool schemas are public per the MCP protocol design.

The trait returns `ToolFuture`, a pinned boxed future, because tool invocation
is asynchronous. The `ToolPortError` enum (`tool_port.rs:9`) includes
`CapabilityDenied` for insufficient-token failures.

Implementor: `BridgeToolPort` in `kask/crates/kask_bridge/src/tool_port.rs:25`,
which wraps zed's `McpRuntime` and enforces OCAP, gas, and span emission.

## Verification functions

Four public functions in `verification/verify.rs` perform token verification:

- `verify_delegation_token_now` (`verify.rs:22`) verifies a token against the
  current time, checking signature, expiry, and capability.
- `verify_delegation_token` (`verify.rs:63`) verifies a token against a
  caller-provided timestamp, used in tests and replay scenarios.
- `require_write_access` (`verify.rs:114`) returns an error string if the
  token does not grant `DelegationAction::Write` on the given resource.
- `require_read_access` (`verify.rs:140`) returns an error string if the
  token does not grant `DelegationAction::Read` on the given resource.

The `VerificationOutcome` enum (`verification/types.rs:22`) has five variants:
`Valid`, `InvalidSignature`, `Expired`, `InsufficientAccess`, and `NoChecker`.
The `NoChecker` variant indicates that no `CapabilityChecker` was provided,
which denies access by default.

## Resource and action model

The `DelegationResource` enum (`resources.rs:51`) has four variants: `Tool`,
`Template`, `Registry`, and `Key`. The `DelegationAction` enum
(`resources.rs:80`) has three variants: `Read`, `Write`, and `Execute`.

The `capabilities_match` function (`resources.rs:131`) compares a token's
declared capability against a required capability. The
`capability_from_server_id` function (`resources.rs:117`) maps an MCP server
ID to a capability string, used when constructing tokens for server-scoped
access.

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
