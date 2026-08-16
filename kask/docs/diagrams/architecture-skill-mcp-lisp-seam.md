---
title: "Skill ↔ MCP ↔ Lisp Capabilities Seam — Architecture"
audience: [architects, developers, agents]
last_updated: 2026-08-15
version: "0.35.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust, domain]
---

# Skill ↔ MCP ↔ Lisp Capabilities Seam — Architecture

Reference-quadrant architecture diagram of the three coupled surfaces that
evolved together in zed-kask: the **skill system** (D1), the **MCP server
wiring** (D3), and the **Lisp capabilities layer** (the `compute` action's
deterministic primitive). Every node traces to a grep-verified symbol; no
symbol is invented.

## The seam

The skill cascade drives MCP tool calls (via `action: execute` steps) and
Lisp evaluations (via `action: compute` steps) through the same `ToolPort`
dispatch seam. The agent's tool-use loop reaches MCP tools through the
`LazyToolRouter`, which filters MCP candidates but bypasses built-in tools.
Both paths land in `McpRuntime::invoke`, which meters (call cap) and
dispatches but does **not** authorize — the per-call capability gate was
removed 2026-08-12 (RR-0056).

```mermaid
architecture-beta
    group skill(cloud)[Skill System — D1]
    group mcp(cloud)[MCP Server Wiring — D3]
    group lisp(cloud)[Lisp Capabilities]
    group agent(cloud)[Agent Tool-Use Loop]

    service skilltool(agent)[SkillTool<br/>crates/agent/src/tools/skill_tool.rs]
    service manifest_executor(skill)[ManifestExecutor<br/>hkask-templates/src/executor.rs]
    service bridge_executor(skill)[BridgeManifestExecutor<br/>kask_bridge/src/skill_executor.rs]
    service step_machine(skill)[StepMachine<br/>hkask-templates/src/step_machine.rs]
    service compute(skill)[dispatch_compute<br/>hkask-templates/src/compute.rs]

    service lisp_eval(lisp)[hkask_lisp::eval_sandboxed_with_budget<br/>hkask-lisp/]

    service lazy_router(agent)[LazyToolRouter<br/>crates/agent/src/tool_router.rs]
    service thread(agent)[Thread::enabled_tools<br/>crates/agent/src/thread.rs]

    service tool_port(mcp)[ToolPort trait<br/>hkask-capability/src/tool_port.rs]
    service mcp_runtime(mcp)[McpRuntime<br/>hkask-mcp/src/runtime.rs]
    service call_cap(mcp)[CallCapManager<br/>hkask-regulation/src/energy.rs]
    service servers(mcp)[13 MCP servers<br/>kask/mcp-servers/hkask-mcp-*]

    service envelope(agent)[unwrap_tool_envelope<br/>hkask-types/src/tool_response.rs]

    skilltool --> bridge_executor: resolves SkillManifestExecutor
    bridge_executor --> manifest_executor: drives cascade
    manifest_executor --> step_machine: execute_flowdef / execute_parallel
    step_machine --> compute: action: compute
    step_machine --> tool_port: action: execute / feedback / validate / retrieve
    compute --> lisp_eval: lisp.eval primitive

    thread --> lazy_router: apply_router_bypassing_built_ins
    lazy_router --> tool_port: MCP candidates only (built-ins bypassed)
    tool_port --> mcp_runtime: invoke(server, tool, args, agent)
    mcp_runtime --> call_cap: charge_call_metered(agent)
    mcp_runtime --> servers: dispatch over stdio
    mcp_runtime --> envelope: result is {"content": value}
```

## The three dispatch paths into `ToolPort::invoke`

| Caller | Entry point | Action | Resolves to |
| --- | --- | --- | --- |
| Skill cascade (deterministic) | `StepMachine::dispatch_action` → `execute_tool_invoke` | `execute` / `feedback` / `validate` / `retrieve` / `select`+`mcp:` | `ToolPort::invoke` under `WebID::from_persona(b"manifest-executor")` |
| Agent tool-use loop (LLM-decided) | `Thread::enabled_tools` → `apply_router_bypassing_built_ins` | LLM emits a tool_use event | `ToolPort::invoke` under the agent's `WebID` |
| Widget compose-back (D21) | `hkask_tool_invoker::ToolInvoker` impls | UI gesture | `ToolPort::invoke` under the `swarm-panel` persona |

All three share the same metering (`CallCapManager::charge_metered`), the same
`reg.tool.*` span emission, and the same `unwrap_tool_envelope` result seam.
The only pre-dispatch refusal is `ToolPortError::EnergyBudgetExceeded` (the
runaway-loop breaker).

## The Lisp capabilities layer

`action: compute` steps invoke `dispatch_compute`
(`hkask-templates/src/compute.rs`), which routes the `lisp.eval` primitive to
`hkask_lisp::eval_sandboxed_with_budget(form, env, max_steps, max_depth)`.
This is a deterministic primitive — no LLM round-trip, no MCP dispatch. It is
the canonical scaffold for structural invariants the LLM cannot reliably
self-evaluate (see the `lisp-scaffold-reasoning` skill). Other `compute_ref`
values (`bayesian_update`, Kata convergence primitives) also route through
`dispatch_compute`.

## The 13 on-disk MCP servers

`McpRuntime` dispatches to child processes over stdio. The 13 servers live
under `kask/mcp-servers/hkask-mcp-*` and are enumerated by
`BUILT_IN_MCP_SERVERS` in `kask/crates/kask_bridge/src/mcp_servers.rs`:
`codegraph`, `companies`, `condenser`, `corpus`, `curator`, `kata-kanban`,
`media`, `portfolio`, `prediction-markets`, `research`, `scenarios`,
`swarm`, `training`.

## Related

- [Skill Invocation Flowchart](./flowchart-skill-invocation.md) — the cascade path in detail
- [MCP Tool Call Sequence](./sequence-mcp-tool-call.md) — the `LazyToolRouter` → `McpRuntime::invoke` → `unwrap_tool_envelope` path
- [Credential Resolution ERD](./erd-credential-resolution.md) — the `ctx.credentials` → keychain → `nudge_mcp_servers` chain
- [MCP Runtime Invoke — Metering and Dispatch Flow](./flowchart-mcp-runtime-invoke.md) — the metering detail
- [Skill ↔ MCP Tool Integration](../explanation/skill-mcp-integration.md) — flowdef-native MCP invocation patterns

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ARCH-SKILL-MCP-LISP-001
verified_date: 2026-08-15
verified_against: crates/agent/src/tools/skill_tool.rs (SkillTool, SkillManifestExecutor); kask/crates/hkask-templates/src/executor.rs (ManifestExecutor, extract_final_step_result); kask/crates/kask_bridge/src/skill_executor.rs (BridgeManifestExecutor); kask/crates/hkask-templates/src/step_machine.rs (StepMachine, last_result_step); kask/crates/hkask-templates/src/compute.rs (dispatch_compute, lisp.eval); kask/crates/hkask-lisp/ (eval_sandboxed_with_budget); crates/agent/src/tool_router.rs (LazyToolRouter, apply_router_bypassing_built_ins); crates/agent/src/thread.rs (enabled_tools); kask/crates/hkask-capability/src/tool_port.rs (ToolPort, ToolPortError::EnergyBudgetExceeded); kask/crates/hkask-mcp/src/runtime.rs (impl ToolPort for McpRuntime, charge_call_metered); kask/crates/hkask-regulation/src/energy.rs (CallCapManager, CallMeterOutcome); kask/crates/hkask-types/src/tool_response.rs (unwrap_tool_envelope); kask/crates/kask_bridge/src/mcp_servers.rs (BUILT_IN_MCP_SERVERS)
status: VERIFIED
-->
