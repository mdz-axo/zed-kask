# hkask-tool-port

Tool dispatch port.

## Core Types

- `ToolPort` — dyn-compatible tool dispatch trait (`Arc<dyn ToolPort>`), implemented by `hkask_mcp::McpRuntime`
- `ToolInfo` — tool metadata (name, description, input schema, server id)
- `SYSTEM_MAX_RECURSION` — cascade depth / subgoal nesting bound (runaway-recursion breaker)

## This crate does not authorize

It previously minted `DelegationToken`s that `McpRuntime::invoke` checked against
the invoked tool. **That gate was removed (2026-08-12) because it could
not deny anything.** All three production mint sites built the token's
`resource_id` from the same tool name they then passed to `invoke`, so
`is_valid_for` compared a caller-supplied value against itself and returned true
unconditionally — while `DIVERGENCE.md` D3 advertised it as "the enforced gate."

`ToolPort::invoke` now takes `agent: WebID`, an **accounting identity for call
metering, not a credential**.

A capability check is only a gate when the authority list is not chosen by the
caller being checked. Authority in zed-kask lives at three such boundaries
(PRINCIPLES P4.2):

| Boundary | Location | Pinned by |
|---|---|---|
| The child's requested narrowing — the per-request `tool_allowlist` on the inference IPC `create_worktree_thread` dispatch (fail-closed on missing) | `kask_bridge::inference_ipc_server` | the `dispatch_create_worktree_thread_*` tests |
| The parent grant the narrowing is measured against (`worktree_tools` intersection; requires `host/create_worktree_thread`) | `kask_bridge::delegation_grants` | `granted_worktree_request` and the worktree authority tests |
| Per-server MCP env / credential allowlists | `kask_bridge::mcp_servers` | `all_servers_have_credential_allowlist` |

Capability **separation** (which tools a given agent may reach at all) is
enforced; per-call capability **gating** is deliberately not.

## Related

- The FIDES `ToolTaint` labels used to live here. Both the labels and the
  runtime policy check were removed (2026-08-12) because the check was inert:
  every `ToolInfo` was labelled `Pure` at its only construction site
  (`McpRuntime::get_tool_info`), and the untrusted-input flag read taint markers
  the context write side had stopped emitting — so the block could never fire.
  Reinstating information-flow control means first giving tools real labels and
  propagating taint on write.
- The call meter (runaway-loop breaker, fail-open on an unseeded agent) lives in
  `hkask_regulation::CallCapManager` — it fails open for an unseeded agent.

## See Also

- [`PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) §P4 — Clear Boundaries
- [`AGENTS.md`](../../AGENTS.md) — Agent Operating Guide

> `RR-NNNN` ids in this document refer to the retired `kali-audit` security
> regression library (removed 2026-08-20). They are historical rationale
> markers only; there is no longer a file to open.
