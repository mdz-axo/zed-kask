---
title: "hkask-capability — Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-12
version: "0.2.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [domain, trust]
---

# hkask-capability — Reference

`hkask-capability` defines the tool dispatch port and the FIDES taint labels:
the `ToolPort` trait, its `ToolInfo` metadata and `ToolPortError` taxonomy, the
`ToolTaint` information-flow lattice, and the `SYSTEM_MAX_RECURSION` structural
bound. It contains **no capability tokens and no authorization check**. Tool
authority is enforced outside this crate, at the allowlist boundaries listed
under [Where authority is enforced](#where-authority-is-enforced).

## Source citations

| Symbol                       | Location                                            |
| ---------------------------- | --------------------------------------------------- |
| `ToolPort` trait             | `kask/crates/hkask-capability/src/tool_port.rs`     |
| `ToolPortError` enum         | `kask/crates/hkask-capability/src/tool_port.rs`     |
| `ToolFuture` type alias      | `kask/crates/hkask-capability/src/tool_port.rs`     |
| `ToolInfo` struct            | `kask/crates/hkask-capability/src/tool_port.rs`     |
| `ToolTaint` enum             | `kask/crates/hkask-capability/src/tool_taint.rs`    |
| `SYSTEM_MAX_RECURSION` const | `kask/crates/hkask-capability/src/token_types.rs`   |
| `ToolPort` implementor       | `kask/crates/hkask-mcp/src/runtime.rs` (`McpRuntime`) |

The crate's `src/` directory holds exactly four files: `hkask_capability.rs`
(lib root), `token_types.rs`, `tool_port.rs`, and `tool_taint.rs`.

## What was removed, and why

**2026-08-12 — the per-call capability gate (RR-0056).** `McpRuntime::invoke`
previously checked a `DelegationToken`'s declared `(resource, resource_id,
action)` triple against the invoked tool. The check was vacuous: all three
production mint sites derived the token's `resource_id` from the same tool name
they then passed to `invoke`, so `is_valid_for` — plain equality on each field —
compared a caller-supplied value against itself and returned true
unconditionally. It denied nothing while running on every tool call. Deleted:
`DelegationToken` (with `new` and `is_valid_for`), `panel_default_token`,
`capabilities_match`, `capability_from_server_id`, `CapabilitySpec`,
`CapabilityParseError`, `DelegationResource`, `DelegationAction`,
`ToolPortError::CapabilityDenied`, `McpRuntime::verify_capability_domain`,
`McpRuntime::required_capability_for`, the `ToolInfo::required_capability`
field, the `src/auth.rs` and `src/resources.rs` modules, and the
`test_token_for_tool` / `arb_delegation_token` / `arb_resource` / `arb_action`
test helpers. `ToolPort::invoke` now takes `agent: hkask_types::WebID` in the
token's place.

**2026-08-12 — the fail-closed call cap (RR-0057).** The meter no longer refuses
an agent that has no registered ceiling; see [Call metering](#call-metering).

Earlier collapses removed the surrounding token ceremony: the 2026-07-31 pass
deleted `signature`/`public_key`, `verify()`/`verify_cryptographic()`,
`TokenSignature`, `derive_signing_key`, base64 serialization, `SigningPayload`,
`AuthContext`, `require_read_access`/`require_write_access`, `NoOpTokenRegistry`,
and the Kani tamper-detection harnesses. The 2026-08-02 pass deleted
`OcapConfig` and manifest `ocap:` blocks, `expires_at`,
`attenuation_level`/`max_attenuation`, `context_nonce`, `caveats`,
`DelegationTokenBuilder`, `Caveat`, `CapabilityError`, the `TokenRegistry` trait
and its SQL table, and `SYSTEM_MAX_ATTENUATION`.

## Type surface

```mermaid
classDiagram
    class ToolPort {
        <<interface>>
        +invoke(server, tool, args, agent) ToolFuture
        +discover_tools() ToolFuture~Vec~String~~
        +get_tool_info(name) ToolFuture~Option~ToolInfo~~
    }
    class ToolInfo {
        +name: String
        +description: String
        +input_schema: Value
        +server_id: String
        +taint: ToolTaint
    }
    class ToolPortError {
        <<enumeration>>
        EnergyBudgetExceeded(String)
        NotFound(NotFound)
        Unavailable(String)
        InvocationFailed(String)
        +is_retryable() bool
    }
    class ToolTaint {
        <<enumeration>>
        Source
        Sink
        Pure
        Endorser
        +can_flow_to(target) bool
    }
    class McpRuntime {
        -servers: HashMap
        -tool_registry: HashMap
        -connections: HashMap
        -governance: Option
    }

    ToolPort ..> ToolInfo : returns
    ToolPort ..> ToolPortError : returns
    ToolInfo --> ToolTaint
    McpRuntime ..|> ToolPort : implements
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-CAP-002
verified_date: 2026-08-12
verified_against: kask/crates/hkask-capability/src/tool_port.rs; kask/crates/hkask-capability/src/tool_taint.rs; kask/crates/hkask-mcp/src/runtime.rs (McpRuntime, impl hkask_capability::ToolPort)
status: VERIFIED
-->

## ToolPort trait

`ToolPort` is the dispatch boundary for MCP tool invocation. Every method
returns `ToolFuture` (`Pin<Box<dyn Future + Send + '_>>`), so the trait is
object-safe and `Arc<dyn ToolPort>` works — this is why no adapter layer wraps
`McpRuntime` any more. The former `BridgeToolPort` pass-through was deleted.

| Method            | Signature                                                                        |
| ----------------- | -------------------------------------------------------------------------------- |
| `invoke`          | `(&self, server: &str, tool: &str, args: Value, agent: WebID) -> Result<Value, ToolPortError>` |
| `discover_tools`  | `(&self) -> Vec<String>`                                                         |
| `get_tool_info`   | `(&self, tool_name: &str) -> Option<ToolInfo>`                                   |

`agent` is the accounting identity for the call meter. `discover_tools` and
`get_tool_info` take no identity at all, because tool schemas are public per the
MCP protocol design (`tools/list` is an unauthenticated handshake).

`ToolPortError` has four variants and one predicate, `is_retryable`, which is
true only for `Unavailable` — the call never reached the tool, so a retry cannot
duplicate a side effect. `EnergyBudgetExceeded` needs a new regulation tick, and
`NotFound` / `InvocationFailed` would fail identically.

## What `invoke` does

`McpRuntime::invoke` (in `kask/crates/hkask-mcp/src/runtime.rs`) performs, in
order:

1. **Call metering** — when governance is wired (`with_governance`), charge one
   call against the agent's per-tick ceiling via
   `CyberneticsLoop::charge_call_metered`. Without governance, dispatch
   unmetered.
2. **Dispatch** — `call_tool_inner` checks for a live connection, reconnects once
   if the transport closed, and issues the JSON-RPC call.
3. **Span emission** — persist a `reg.gas.settled` `RegulationRecord` (target
   `reg.mcp`) carrying server, tool, call count, and success/failure status,
   through the wired `RegulationSink` (`RegulationArchive` on the curator's
   pod.db in zed-kask).

There is no authorization step. The only way `invoke` returns an error before
dispatch is `EnergyBudgetExceeded`.

## Call metering

The meter is a runaway-loop breaker and a usage recorder, not a permission
check. `hkask_regulation::CallCapManager::charge_metered` returns
`CallMeterOutcome`:

| Outcome                        | Behavior                                                                                    |
| ------------------------------ | ------------------------------------------------------------------------------------------- |
| `Charged`                      | Headroom remained; the call proceeds.                                                        |
| `AutoRegistered`               | The agent had no registered ceiling: register at `DEFAULT_RUNAWAY_CALL_CEILING` (10 000), charge, proceed, and log the wiring gap. |
| `CeilingReached { ceiling }`   | The per-tick ceiling is exhausted: refuse with `ToolPortError::EnergyBudgetExceeded`. Resets on the next regulation tick. |

Fail-open on an unregistered agent is deliberate (RR-0057): a missing
registration is a composition-root wiring omission, and refusing it fails live
paths without protecting anything.

## Where authority is enforced

A capability check is only a gate when the authority list is not chosen by the
caller being checked. Three boundaries satisfy that:

| Boundary                                                        | Location                                                       | Note                                        |
| --------------------------------------------------------------- | -------------------------------------------------------------- | ------------------------------------------- |
| Per-request delegated-tool `tool_allowlist` (fail-closed on missing/empty) | `kask/crates/kask_bridge/src/inference_ipc_server.rs` `tool_invoke` dispatch | Enforced before dispatch; pinned by `dispatch_tool_invoke_rejects_unallowed_tool` |
| Per-agent declared `mcp_tools` allowlist                        | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs`        | Restricts which tools a swarm agent may call |
| Per-server MCP env / credential allowlists                      | `kask/crates/kask_bridge/src/mcp_servers.rs`                    | Scopes credentials per server (RR-0038)      |

A fourth live gate acts on information flow rather than authority: the FIDES
`Source`→`Sink` runtime policy check in `hkask-templates`'s `invoke_tool`, which
reads `ToolInfo.taint` and can return `Block` / `RequireHuman` / `Log`
(RR-0053).

## Taint model

`ToolTaint` labels every tool by its data-flow character: `Source` (returns
untrusted external data), `Sink` (state-changing), `Pure` (no side effects, no
external data), `Endorser` (trusted extraction from untrusted input). The
default assigned by `McpRuntime::get_tool_info` is `Pure`.

`can_flow_to` encodes the whole policy as one prohibition: `Source → Sink` is
blocked; all fifteen other pairs are allowed. Untrusted data must pass through
an `Endorser` before reaching a state-changing tool. The full 4×4 matrix is
pinned by `can_flow_to_matrix` in `tool_taint.rs`.

Source: Microsoft Research FIDES (arXiv:2505.23643).[^fides-cap]

## Structural bound

`SYSTEM_MAX_RECURSION` (7) is the shared bound for cascade depth and subgoal
nesting, consulted by the manifest executor and the registry bootstrap. It is a
runaway-recursion breaker, not an authorization limit — the same distinction the
call meter draws.

**Empirical context:** Wang (2026, arXiv:2603.02615v1) demonstrates that RLM
recursion depth=2 already degrades model performance across all tested
models and tasks — format collapse, parametric hallucination, and latency
explosion (3.6s → 344.5s). The cap of 7 is structural headroom for future
native-RLM-aligned models; skill authors should self-limit flowdef nesting
to depth 1–2 in practice. The matryoshka guard is a hard ceiling, not a
recommended operating point.

## See also

- [hkask-capability Explanation](./explanation.md): why per-call gating was
  removed and separation kept.
- [`kask/docs/architecture/core/PRINCIPLES.md`](../../architecture/core/PRINCIPLES.md):
  P4 (Clear Boundaries).
- `kask/security/regressions/RR-0056.yaml`, `RR-0057.yaml`.

---

[^fides-cap]: Microsoft Research. (2025). _FIDES: Information flow control for LLM agents_ (arXiv:2505.23643). The Source/Sink/Pure/Endorser lattice and the Source→Sink endorsement rule implemented in `tool_taint.rs`.

[^miller-ocap]: Miller, M. S. (2006). _Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control._ Johns Hopkins University. <https://www.erights.org/talks/thesis/markm-thesis.pdf>. The Object Capability model, retained here as the source of the principle that survived: authority must be *separated* by a list the caller cannot choose. The in-process token *matching* that once cited this reference was removed as vacuous — see [What was removed, and why](#what-was-removed-and-why).
