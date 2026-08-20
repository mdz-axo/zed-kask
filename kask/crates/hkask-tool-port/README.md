# hkask-tool-port

Tool dispatch port.

## Core Types

- `ToolPort` — dyn-compatible tool dispatch trait (`Arc<dyn ToolPort>`), implemented by `hkask_mcp::McpRuntime`
- `ToolInfo` — tool metadata (name, description, input schema, server id)
- `SYSTEM_MAX_RECURSION` — cascade depth / subgoal nesting bound (runaway-recursion breaker)

## This crate does not authorize

It previously minted `DelegationToken`s that `McpRuntime::invoke` checked against
the invoked tool. **That gate was removed (2026-08-12, RR-0056) because it could
not deny anything.** All three production mint sites built the token's
`resource_id` from the same tool name they then passed to `invoke`, so
`is_valid_for` compared a caller-supplied value against itself and returned true
unconditionally — while `DIVERGENCE.md` D3 advertised it as "the enforced gate."

`ToolPort::invoke` now takes `agent: WebID`, an **accounting identity for call
metering, not a credential**.

A capability check is only a gate when the authority list is not chosen by the
caller being checked. Authority in zed-kask lives at three such boundaries:

| Boundary | Location | Pinned by |
|---|---|---|
| Per-request delegated-tool allowlist (fail-closed on missing/empty) | `kask_bridge::inference_ipc_server` `tool_invoke` dispatch | `dispatch_tool_invoke_rejects_unallowed_tool` |
| Per-agent declared `mcp_tools` allowlist | `hkask-mcp-swarm` `agent_executor` | swarm card tests |
| Per-server MCP env / credential allowlists | `kask_bridge::mcp_servers` | RR-0038, `all_servers_have_credential_allowlist` |

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
  `hkask_regulation::CallCapManager` — see RR-0057.

## See Also

- [`PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) §P4 — Clear Boundaries
- [`AGENTS.md`](../../AGENTS.md) — Agent Operating Guide
- `kask/security/regressions/RR-0056.yaml`, `RR-0057.yaml`
