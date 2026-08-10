---
title: "hkask-capability — Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-04
version: "0.1.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [domain, trust]
---

# hkask-capability — Reference

`hkask-capability` defines the in-process capability layer for hKask:
`DelegationToken`, the `ToolPort` trait, and the `capabilities_match`
function. A `DelegationToken` declares "holder X may perform action Y on
resource Z". Tokens are minted and consumed **in-process** (the composition
root hands them to `McpRuntime::invoke`); there is no untrusted transport
boundary, so tokens carry **no signature and no public key**. The enforced
gate is the capability match in `McpRuntime::invoke` (`is_valid_for` /
`verify_capability_domain`), not cryptography — it catches manifest/config
bugs (a caller naming the wrong tool); it is not a security boundary against
a hostile in-process caller.

## Source citations

| Symbol                              | Location                                                                      |
| ----------------------------------- | ----------------------------------------------------------------------------- |
| `DelegationToken` struct            | `kask/crates/hkask-capability/src/token_types.rs`                             |
| `SYSTEM_MAX_RECURSION` const        | `kask/crates/hkask-capability/src/token_types.rs`                             |
| `panel_default_token` (minting)     | `kask/crates/hkask-capability/src/auth.rs`                                    |
| `ToolPort` trait                    | `kask/crates/hkask-capability/src/tool_port.rs`                               |
| `ToolPortError` enum                | `kask/crates/hkask-capability/src/tool_port.rs`                               |
| `ToolInfo` struct                   | `kask/crates/hkask-capability/src/tool_port.rs`                               |
| Capability-match gate (enforcement) | `kask/crates/hkask-mcp/src/runtime.rs` (`invoke`, `verify_capability_domain`) |
| `CapabilitySpec` struct             | `kask/crates/hkask-capability/src/resources.rs`                               |
| `DelegationResource` enum           | `kask/crates/hkask-capability/src/resources.rs`                               |
| `DelegationAction` enum             | `kask/crates/hkask-capability/src/resources.rs`                               |
| `capabilities_match` fn             | `kask/crates/hkask-capability/src/resources.rs`                               |
| `capability_from_server_id` fn      | `kask/crates/hkask-capability/src/resources.rs`                               |

Removed in the 2026-07-31 token-ceremony collapse: `signature`/`public_key`
fields, `verify()`/`verify_cryptographic()`, `TokenSignature`,
`derive_signing_key`, base64 serialization, the `SigningPayload`, the
`AuthContext` struct, `require_read_access`/`require_write_access`,
`NoOpTokenRegistry`, and the Kani tamper-detection harnesses. Removed in the
2026-08-02 config collapse: `OcapConfig` + manifest `ocap:` blocks,
`expires_at`, `attenuation_level`/`max_attenuation`, `context_nonce`,
`caveats`, the `DelegationTokenBuilder`, the `Caveat` struct, the
`CapabilityError` enum, the `TokenRegistry` trait + SQL table, and
`SYSTEM_MAX_ATTENUATION`.

## Token model

The `DelegationToken` is the core capability object. It carries an id
(deterministic content hash), a resource, a resource_id, an action, and a
delegation chain (from/to WebIDs). Construct it with `DelegationToken::new`
(there is no builder and no signing step).

```mermaid
classDiagram
    class DelegationToken {
        +id: String
        +resource: DelegationResource
        +resource_id: String
        +action: DelegationAction
        +delegated_from: WebID
        +delegated_to: WebID
        +is_valid_for(resource, resource_id, action) bool
    }
    class ToolPort {
        <<interface>>
        +invoke(server, tool, args, token) ToolFuture
        +discover_tools() ToolFuture~Vec~String~~
        +get_tool_info(name) ToolFuture~Option~ToolInfo~~
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

    DelegationToken --> DelegationResource
    DelegationToken --> DelegationAction
    ToolPort --> DelegationToken : requires
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-CAP-002
verified_date: 2026-08-04
verified_against: kask/crates/hkask-capability/src/token_types.rs; kask/crates/hkask-capability/src/tool_port.rs:47; kask/crates/hkask-capability/src/resources.rs; kask/crates/hkask-mcp/src/runtime.rs (invoke)
status: VERIFIED
-->

## The enforced gate

`McpRuntime::invoke` (in `hkask-mcp/src/runtime.rs`) applies, in order:

1. **Capability match** — `token.is_valid_for(Tool, tool, Execute)` or
   `verify_capability_domain` (string-form capability comparison via
   `capabilities_match`). Denies with `CapabilityDenied`.
2. **Call cap** — `can_proceed(agent)` then `charge_call(agent)` (one call
   charged against the agent's per-tick `CallCap`). Denies with
   `EnergyBudgetExceeded` when the agent has no cap or it is exhausted.
3. **Span emission** — `reg.tool.*` spans persisted via the wired
   `RegulationSink` (`RegulationArchive` on the curator's pod.db in
   zed-kask).

The gate does NOT verify token signatures. There is no token registry; the
former `TokenRegistry` SQL table and the curator `list_tokens` tool were
removed as consent-audit theater with no enforcement point.

## ToolPort trait

The `ToolPort` trait is the actuator boundary for governed tool dispatch.
The `invoke` method requires a `DelegationToken` as a parameter. The
`discover_tools` and `get_tool_info` methods return public tool metadata and
require no token, because tool schemas are public per the MCP protocol
design.

The trait returns `ToolFuture` (`Pin<Box<dyn Future + Send + '_>>`), a pinned
boxed future, because tool invocation is asynchronous. The trait is
object-safe: `Arc<dyn ToolPort>` works. The `ToolPortError` enum includes
`CapabilityDenied` for capability mismatches, `EnergyBudgetExceeded` for gas
exhaustion, `NotFound` for missing tools, and `InvocationFailed` for runtime
errors.

Implementor: `McpRuntime` itself (`hkask-mcp/src/runtime.rs`) — the
composition root passes `Arc<McpRuntime>` wherever a `ToolPort` is needed.
The former `BridgeToolPort` adapter was deleted as a pure pass-through.

## Resource and action model

The `DelegationResource` enum has four variants: `Tool`, `Template`,
`Registry`, and `Key`. The `DelegationAction` enum has three variants:
`Read`, `Write`, and `Execute`. The action hierarchy is
`Execute >= Write >= Read`: a token with `Execute` action satisfies requests
for `Write` or `Read`; a `Write` token satisfies `Read` requests but not
`Execute`.

The `capabilities_match` function compares a token's declared capability
against a required capability, applying the action hierarchy. The
`capability_from_server_id` function maps an MCP server ID (e.g.
`hkask-mcp-<domain>` or short `<domain>`) to a capability string
`tool:<domain>:execute`, used when constructing tokens for server-scoped
access.

`SYSTEM_MAX_RECURSION` (7) is the shared structural bound for cascade depth
and subgoal nesting, consulted by the manifest executor and the registry
bootstrap.

**Empirical context:** Wang (2026, arXiv:2603.02615v1) demonstrates that RLM
recursion depth=2 already degrades model performance across all tested
models and tasks — format collapse, parametric hallucination, and latency
explosion (3.6s → 344.5s). The cap of 7 is structural headroom for future
native-RLM-aligned models; skill authors should self-limit flowdef nesting
to depth 1–2 in practice. The matryoshka guard is a hard ceiling, not a
recommended operating point.

## See also

- [hkask-capability Explanation](./explanation.md): why the gate is a
  capability match and not a cryptographic check.
- [`kask/docs/architecture/core/PRINCIPLES.md`](../../architecture/core/PRINCIPLES.md):
  P4 (Clear Boundaries).

---

[^miller-ocap]: Miller, M. S. (2006). _Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control._ Johns Hopkins University. <https://www.erights.org/talks/thesis/markm-thesis.pdf>. The Object Capability model as inspiration for capability-based dispatch. Note: zed-kask implements capability _matching_ in-process, not the unforgeable-token model — there is no trust boundary to defend with cryptography.
