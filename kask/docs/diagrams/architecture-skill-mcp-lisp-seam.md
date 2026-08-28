---
title: "Skill ↔ MCP ↔ Lisp Capabilities Seam — Architecture"
audience: [architects, developers, agents]
last_updated: 2026-08-20
version: "0.39.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust, domain]
---

# Skill ↔ MCP ↔ Lisp Capabilities Seam — Architecture

Reference-quadrant architecture diagram of the three coupled surfaces in
zed-kask: the **skill system** (D1, upstream-Zed body injection), the **MCP
server wiring** (D3), and the **Lisp capabilities layer** (the `lisp_eval`
tool's deterministic primitive). Every node traces to a grep-verified symbol;
no symbol is invented.

Skill execution is upstream-Zed body injection: `SkillTool::run` reads the
`SKILL.md` body from disk via `agent_skills::read_skill_body` and injects it
via `render_skill_envelope`. The model reads the body and follows the
instructions. PDCA loops are model-coordinated: the model self-iterates using
the `lisp_eval` tool for deterministic checks and the `render_template` tool
for structured prompt scaffolding.

## The seam

The agent's tool-use loop reaches MCP tools through the `LazyToolRouter`,
which filters MCP candidates but bypasses built-in tools (`lisp_eval`,
`render_template`, `read_file`, etc.). Both paths land in
`McpRuntime::invoke`, which meters (call cap) and dispatches but does **not**
authorize.

```mermaid
architecture-beta
    group skill(cloud)[Skill System — D1]
    group mcp(cloud)[MCP Server Wiring — D3]
    group lisp(cloud)[Lisp Capabilities]
    group agent(cloud)[Agent Tool-Use Loop]

    service skilltool(agent)[SkillTool::run<br/>crates/agent/src/tools/skill_tool.rs]
    service envelope(agent)[render_skill_envelope<br/>crates/agent/src/tools/skill_tool.rs]
    service read_body(skill)[agent_skills::read_skill_body<br/>crates/agent_skills/agent_skills.rs]
    service render_template(skill)[render_template tool<br/>crates/agent/src/tools/render_template_tool.rs]

    service lisp_eval(lisp)[lisp_eval tool<br/>crates/agent/src/tools/lisp_eval_tool.rs]
    service lisp_runtime(lisp)[hkask_lisp::eval_sandboxed_with_budget<br/>hkask-lisp/]

    service lazy_router(agent)[LazyToolRouter<br/>crates/agent/src/tool_router.rs]
    service thread(agent)[Thread::enabled_tools<br/>crates/agent/src/thread.rs]

    service tool_port(mcp)[ToolPort trait<br/>hkask-tool-port/src/tool_port.rs]
    service mcp_runtime(mcp)[McpRuntime<br/>hkask-mcp/src/runtime.rs]
    service call_cap(mcp)[CallCapManager<br/>hkask-regulation/src/energy.rs]
    service servers(mcp)[10 MCP servers<br/>kask/mcp-servers/hkask-mcp-*]

    service unwrap(agent)[unwrap_tool_envelope<br/>hkask-types/src/tool_response.rs]

    skilltool --> read_body: reads SKILL.md body from disk
    skilltool --> envelope: injects body into agent context
    envelope --> agent: model reads body and follows instructions
    agent --> render_template: structured prompt scaffolding (model-coordinated)
    agent --> lisp_eval: deterministic checks (model-coordinated)
    lisp_eval --> lisp_runtime: eval_sandboxed_with_budget

    thread --> lazy_router: apply_router_bypassing_built_ins
    lazy_router --> tool_port: MCP candidates only (built-ins bypassed)
    tool_port --> mcp_runtime: invoke(server, tool, args, agent)
    mcp_runtime --> call_cap: charge_call_metered(agent)
    mcp_runtime --> servers: dispatch over stdio
    mcp_runtime --> unwrap: result is {"content": value}
```

## The two dispatch paths into `ToolPort::invoke`

| Caller | Entry point | Action | Resolves to |
| --- | --- | --- | --- |
| Agent tool-use loop (LLM-decided) | `Thread::enabled_tools` → `apply_router_bypassing_built_ins` | LLM emits a tool_use event | `ToolPort::invoke` under the agent's `WebID` |
| Widget compose-back (D21) | `hkask_tool_invoker::ToolInvoker` impls | UI gesture | `ToolPort::invoke` under the `swarm-panel` persona |

Both share the same metering (`CallCapManager::charge_metered`), the same
`reg.tool.*` span emission, and the same `unwrap_tool_envelope` result seam.
The only pre-dispatch refusal is `ToolPortError::EnergyBudgetExceeded` (the
runaway-loop breaker).

The model decides every tool call. Skills do not dispatch MCP tools
deterministically; the `SKILL.md` body instructs the model, and the model
emits tool_use events through the same agent tool-use loop as any other
request.

## The Lisp capabilities layer

The `lisp_eval` tool (`crates/agent/src/tools/lisp_eval_tool.rs`) wraps
`hkask_lisp::eval_sandboxed_with_budget(form, env, max_steps, max_depth)`.
The model calls it directly when a `SKILL.md` instructs it to perform
deterministic computation (convergence signals, invariant checks, scoring).
This is a deterministic primitive — no LLM round-trip, no MCP dispatch. It is
the canonical scaffold for structural invariants the LLM cannot reliably
self-evaluate (see the `lisp-scaffold-reasoning` skill). It is a built-in
tool registered in `add_default_tools` and bypasses the `LazyToolRouter`.

## The 10 on-disk MCP servers

`McpRuntime` dispatches to child processes over stdio. The 10 servers live
under `kask/mcp-servers/hkask-mcp-*` and are enumerated by
`BUILT_IN_MCP_SERVERS` in `kask/crates/kask_bridge/src/mcp_servers.rs`:
`companies`, `corpus`, `curator`, `kata-kanban`, `portfolio`,
`prediction-markets`, `research`, `scenarios`, `swarm`, `training`.

## Related

- [MCP Tool Call Sequence](./sequence-mcp-tool-call.md) — the `LazyToolRouter` → `McpRuntime::invoke` → `unwrap_tool_envelope` path
- [Credential Resolution ERD](./erd-credential-resolution.md) — the `ctx.credentials` → keychain → `nudge_mcp_servers` chain
- [MCP Runtime Invoke — Metering and Dispatch Flow](./flowchart-mcp-runtime-invoke.md) — the metering detail
- [Skill ↔ MCP Tool Integration](../explanation/skill-mcp-integration.md) — the model-coordinated tool invocation pattern

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ARCH-SKILL-MCP-LISP-001
verified_date: 2026-08-20
verified_against: crates/agent/src/tools/skill_tool.rs (SkillTool::run, render_skill_envelope); crates/agent_skills/agent_skills.rs (read_skill_body); crates/agent/src/tools/lisp_eval_tool.rs (lisp_eval tool); crates/agent/src/tools/render_template_tool.rs (render_template tool); kask/crates/hkask-lisp/ (eval_sandboxed_with_budget); crates/agent/src/tool_router.rs (LazyToolRouter, apply_router_bypassing_built_ins); crates/agent/src/thread.rs (enabled_tools); kask/crates/hkask-tool-port/src/tool_port.rs (ToolPort, ToolPortError::EnergyBudgetExceeded); kask/crates/hkask-mcp/src/runtime.rs (impl ToolPort for McpRuntime, charge_call_metered); kask/crates/hkask-regulation/src/energy.rs (CallCapManager, CallMeterOutcome); kask/crates/hkask-types/src/tool_response.rs (unwrap_tool_envelope); kask/crates/kask_bridge/src/mcp_servers.rs (BUILT_IN_MCP_SERVERS — 10 servers)
status: VERIFIED
-->
