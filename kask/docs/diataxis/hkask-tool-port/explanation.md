---
title: "hkask-tool-port — Explanation"
audience: [developers, architects, agents]
last_updated: 2026-08-13
version: "1.0.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [trust, curation]
---

# hkask-tool-port — Explanation

The capability layer exists to keep tool authority *separated* — to make it
structurally impossible for an agent to reach a tool nobody granted it. It does
**not** try to re-check that grant on every call. `McpRuntime::invoke` meters
the call, dispatches it, and emits the outcome span; the decision about what an
agent may reach is made earlier, by a list the agent does not write.

That is a correction, not a design preference. The crate once minted a
`DelegationToken` per call and checked it at invoke time, and the check was
worthless.

## Source citations

| Concept                              | Location                                                       |
| ------------------------------------ | -------------------------------------------------------------- |
| Per-call gate removal rationale      | `kask/crates/hkask-tool-port/src/hkask_tool_port.rs:5-21`    |
| `invoke` does not authorize          | `kask/crates/hkask-tool-port/src/tool_port.rs:64-83`          |
| `invoke` metering + dispatch         | `kask/crates/hkask-mcp/src/runtime.rs:969-1057`                |
| `CallMeterOutcome` branches          | `kask/crates/hkask-regulation/src/energy.rs:35-45`             |
| Per-request allowlist gate           | `kask/crates/kask_bridge/src/inference_ipc_server.rs:724-747` |
| Per-agent `mcp_tools` allowlist       | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:236-346` |
| Per-server credential allowlist      | `kask/crates/kask_bridge/src/mcp_servers.rs:26-43`             |
| Taint gate removal rationale         | `kask/crates/hkask-tool-port/src/hkask_tool_port.rs:19-21`   |

## Where authority lives

Capability separation is enforced at boundaries that hold a list the caller
cannot set:

- **The per-request `tool_allowlist`** on the inference IPC `tool_invoke`
  dispatch (`kask/crates/kask_bridge/src/inference_ipc_server.rs`). The child
  MCP server declares what it may dispatch; the zed side enforces it before
  dispatch, so the gate does not depend on the child's own matching being
  correct. Fail-closed: a missing or empty allowlist is a protocol violation,
  never an implicit grant-all. Pinned by `dispatch_tool_invoke_rejects_unallowed_tool`.
- **The per-agent `mcp_tools` allowlist** on each swarm agent card
  (`kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs`). A tool call
  outside the declared set is refused with "not in declared mcp_tools
  allowlist" and never dispatched.
- **The per-server MCP env/credential allowlists**
  (`kask/crates/kask_bridge/src/mcp_servers.rs`). A server's process receives
  only the credentials scoped to it.

Each of these is a list written by a different actor than the one it
constrains.

## The invoke pipeline

```mermaid
stateDiagram-v2
    [*] --> Governance: invoke(server, tool, args, agent)
    Governance --> Meter: governance wired
    Governance --> Dispatch: no governance (unmetered)
    Meter --> Dispatch: Charged or AutoRegistered
    Meter --> Breaker: CeilingReached
    Dispatch --> SpanEmit: tool result (success or failure)
    SpanEmit --> [*]
    Breaker --> [*]: EnergyBudgetExceeded
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CAP-004
verified_date: 2026-08-13
verified_against: kask/crates/hkask-mcp/src/runtime.rs:969-1057 (impl ToolPort for McpRuntime, charge_call_metered branch + no-governance branch); kask/crates/hkask-regulation/src/energy.rs:35-45 (CallMeterOutcome); kask/crates/hkask-mcp/tests/invoke_gate.rs
status: VERIFIED
-->

One mechanism remains on the dispatch path, and it does not authorize:

**The runaway-loop breaker.** One call is charged against the agent's per-tick
ceiling. Only an exhausted ceiling refuses (`EnergyBudgetExceeded`), and the
cap resets each regulation tick. Its purpose is to end a non-terminating tool
loop and to meter usage so cost can be optimized over time — not to limit
precisely or to authorize. It is deliberately **fail-open** on an agent with no
registered ceiling: such an agent is auto-registered at
`DEFAULT_RUNAWAY_CALL_CEILING` and the wiring gap is logged. The prior
fail-closed behavior demonstrated why: `main.rs` seeded a ceiling only for the
`swarm-panel` persona while the IPC dispatch used `kask-panel` and the MCP
runtime used a different persona, so every delegated tool call was refused for a wiring
omission that had nothing to do with authority.

## Information flow control (Layer 5) is absent by decision

Defense **Layer 5 (information flow control) is absent by decision** — the
same disposition Layer 3 (instruction hierarchy) has. That is the honest state
and the safer one: an inert gate invites reliance on a protection that does
not exist. RR-0053 is an absence check, and it states what a real IFC gate
would have to prove. Rationale in full:
`kask/security/regressions/RR-0053.yaml` (guard layer + taint gate removal).

## If a real trust boundary appears

Nothing here defends against a hostile caller already executing inside the
process; such a caller can call `invoke` directly. That is acceptable only
because there is no trust boundary at this seam — the allowlist gates sit at
the IPC and card boundaries, where the untrusted party actually is.

```mermaid
sequenceDiagram
    participant Caller as In-process caller
    participant Port as ToolPort::invoke
    participant Boundaries as Allowlist boundaries
    participant Runtime as McpRuntime

    Caller->>Port: invoke(server, tool, args, agent)
    Note over Port: No authorization check here.<br/>Authority is enforced at the<br/>IPC and card boundaries, not in invoke.
    Port->>Runtime: meter + dispatch
    Runtime-->>Port: result or ToolPortError
    Port-->>Caller: result

    Note over Boundaries: Per-request tool_allowlist (IPC dispatch)<br/>Per-agent mcp_tools (swarm card)<br/>Per-server credentials (mcp_servers.rs)
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CAP-005
verified_date: 2026-08-13
verified_against: kask/crates/hkask-tool-port/src/tool_port.rs:64-83 (invoke does not authorize); kask/crates/kask_bridge/src/inference_ipc_server.rs:724-747 (tool_allowlist gate); kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:236-346 (mcp_tools gate); kask/crates/kask_bridge/src/mcp_servers.rs:26-43 (credential allowlist)
status: VERIFIED
-->

If a genuine boundary is ever introduced (tokens crossing a process or network
edge to an untrusted verifier), verification must be reintroduced with a
**trusted root key set**, and the change must ship with a test proving that a
mismatched request produced on a path production can actually reach is
refused. Do not re-add a per-call authorization argument to `ToolPort::invoke`
without that proof — see `kask/security/regressions/RR-0056.yaml`.

## See also

- [hkask-tool-port Reference](./reference.md): the current type surfaces and
  the invoke pipeline.
- [hkask-tool-port Tutorial](./tutorial.md): dispatching through the seam.
- `kask/security/regressions/RR-0053.yaml`: full rationale for the guard-layer
  and taint-gate deletions.
- `kask/security/regressions/RR-0053.yaml`, `RR-0056.yaml`, `RR-0057.yaml`.

---

[^fides-cap]: Microsoft Research. (2025). _FIDES: Information flow control for LLM agents_ (arXiv:2505.23643). The Source/Sink/Pure/Endorser lattice and the Source→Sink endorsement rule. Retained as the academic source for a design this crate no longer implements — the lattice was deleted (RR-0053). Citing it is not a claim that information flow control is deployed.

[^miller-ocap]: Miller, M. S. (2006). _Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control._ Johns Hopkins University. <https://www.erights.org/talks/thesis/markm-thesis.pdf>. The Object Capability model, retained here as the source of the principle that authority must be *separated* by a list the caller cannot choose.
