---
title: "Skill ↔ MCP Tool Integration"
audience: [developers, skill-authors, agents]
last_updated: 2026-08-20
version: "0.37.0"
status: "Active"
domain: "Skill System"
mds_categories: [composition, domain]
---

# Skill ↔ MCP Tool Integration

> **Execution model (verified 2026-08-20):** Skills execute via upstream Zed body injection.
> `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:266`) reads the `SKILL.md` body and
> injects it via `render_skill_envelope`. The model reads the body and follows the instructions.
> There is no `ManifestExecutor`, no `StepMachine`, no FlowDef `action: execute` step. The prior
> manifest-driven cascade model was deleted (commit `5f4cf5f10d`).
>
> **How skills invoke MCP tools:** A skill body instructs the model to call MCP tools via the
> agent's tool-use loop. The model decides to call the tool, constructs the arguments, and
> receives the result. This is the **agent-mediated** pattern. There is no longer a
> flowdef-native `action: execute` pattern — that was deleted with the `hkask-templates` crate.

## The agent-mediated pattern

A skill body (injected via `render_skill_envelope`) instructs the model to call MCP tools. The model's tool-use loop performs the call:

```
skill body (injected via render_skill_envelope)
  ↓ model reads instructions
  ↓ model decides to call an MCP tool
  ↓
agent's tool-use loop
  ↓ model emits tool_use with tool name + arguments
  ↓
LazyToolRouter (built-in bypass) or McpRuntime::invoke
  ↓ charge one call against agent's CallCap
  ↓ dispatch to MCP server child process
  ↓
result returned to model
  ↓ model reads result and continues
```

### Example: skill body instructing a tool call

A `SKILL.md` body might contain:

```markdown
## Process

1. Call the `dcf_valuation` tool from the companies MCP server with the target ticker symbol.
2. Read the returned valuation.
3. Compare against the current market price.
4. Produce a recommendation.
```

The model reads this, emits a `tool_use` for `dcf_valuation` with the appropriate arguments, and receives the result. The tool call flows through the standard agent tool-use loop — same `CallCap` charging, same `reg.tool.*` span emission, same governance as any agent-initiated tool call.

## Why agent-mediated is the only pattern now

The prior flowdef-native `action: execute` pattern (deleted with `hkask-templates`, commit `5f4cf5f10d`) allowed a manifest step to invoke an MCP tool directly — without the LLM deciding to call it. This was deterministic: the tool call happened at a known step ordinal with bound inputs.

With body injection, there is no step ordinal and no bound inputs — the model reads the instructions and decides how to execute them. The trade-off:

| Property | Agent-mediated (body injection) | Former flowdef-native (`action: execute`) |
|----------|--------------------------------|------------------------------------------|
| Determinism | Model may forget to call the tool, or call it with wrong args | Tool call happened at a known step ordinal with bound inputs |
| Governance | Flows through the agent's tool-use loop | Flowed through `ToolPort::invoke` — same gas, same cap, same spans |
| Testability | Hard to test (depends on LLM behavior) | Testable — manifest parse tests pinned the contract |
| Composability | Tool result is in the agent's context | Tool result was in `step_{ordinal}_result`, available to all downstream steps |
| Cost | LLM round-trip to decide to call the tool | No LLM round-trip — direct call |
| Error handling | LLM may silently skip a failed tool | Step failure surfaced as a `TemplateError`, halted the cascade |

The body-injection model accepts the determinism trade-off in exchange for simplicity: the skill body is prose the model reads, not a machine-executed manifest. The `lisp_eval` tool provides the deterministic scaffold for invariant checks and convergence signals that the prior `compute` step provided.

## When to use `lisp_eval` vs. an MCP tool call

| Need | Tool | Why |
|------|------|-----|
| Deterministic math (Brier score, gap calculation) | `lisp_eval` | No LLM round-trip, no MCP dispatch — pure computation |
| Invariant checks (count, completeness, mutual exclusivity) | `lisp_eval` | Deterministic verification of LLM output |
| Convergence signal | `lisp_eval` | Deterministic gap score the model reads to decide whether to iterate |
| Data retrieval (financial data, code graph, calibration) | MCP tool call | External data source — requires MCP server dispatch |
| Web search, extraction, browsing | MCP tool call | External service — requires MCP server dispatch |
| Structured prompt scaffolding | `render_template` | Jinja2 template rendering with context variables |

## How the tool call flows through the system

```
skill body (injected via render_skill_envelope)
  ↓ model reads instructions and decides to call a tool
  ↓
agent's tool-use loop (crates/agent/src/thread.rs)
  ↓ model emits tool_use with tool name + arguments
  ↓
LazyToolRouter (crates/agent/src/tool_router.rs)
  ↓ built-in tools bypass the router
  ↓ MCP tools route through McpRuntime
  ↓
McpRuntime::invoke (kask/crates/hkask-mcp/src/runtime.rs)
  ↓ charge one call against agent's CallCap (CallCapManager::charge_metered)
  ↓ dispatch to MCP server child process
  ↓ emit reg.tool.* span
  ↓
result returned to model
  ↓ model reads result and continues following the skill body
```

## Verified code references

All claims in this document are grounded in:

- `crates/agent/src/tools/skill_tool.rs:47` — `render_skill_envelope` (wraps SKILL.md body)
- `crates/agent/src/tools/skill_tool.rs:172` — `SkillTool::run` (reads body, calls `render_skill_envelope`)
- `crates/agent/src/tools/skill_tool.rs:266` — `render_skill_envelope(&skill, &body)` call site
- `crates/agent/src/tools/lisp_eval_tool.rs` — `lisp_eval` tool (sandboxed Lisp interpreter)
- `crates/agent/src/tools/render_template_tool.rs` — `render_template` tool (Jinja2 rendering via `minijinja`)
- `crates/zed/src/main.rs:776` — `agent::set_template_base_path` (wires template base path via OnceLock)
- `crates/agent/src/tool_router.rs` — `LazyToolRouter` (built-in bypass, MCP routing)
- `kask/crates/hkask-mcp/src/runtime.rs` — `McpRuntime::invoke` (metering + dispatch)
- `kask/crates/hkask-regulation/src/energy.rs` — `CallCapManager::charge_metered`, `CallMeterOutcome`
- `kask/crates/kask_bridge/src/mcp_servers.rs:330` — `BUILT_IN_MCP_SERVERS_IDS` (10 on-disk servers)

## Co-Evolution Patterns

The agent-mediated pattern enables three feedback loops that co-evolve skills and MCP tools:

### The Calibration Loop (Forecast → Outcome → Brier → Calibrate)

Skills that produce forecasts instruct the model to persist them via the `scenario_score` MCP tool. When outcomes resolve, `forecast_record` scores the forecast (Brier score). The next invocation instructs the model to read prior Brier scores via `scenario_calibration` at the start of the process and adjust its predictions.

Wired in: `superforecasting`, `scenario-builder`, `metacognition`, `company-research-flash`.

### The Skill-Use Reporting Loop (Skill → Curator → MCP Evolution)

When a skill body instructs the model to call an MCP tool and the call fails, the skill body should instruct the model to call `curator_report_skill_use_issue` with the skill name, tool name, and error. The Curator reads these reports via `curator_memory_recall` and issues `EvolveMcpToolSchema` directives to evolve the MCP tool's schema.

### The Persistence-Grounded Learning Loop (Skill → MCP Persistence → Skill)

Each skill that produces forecasts, analyses, or recommendations should instruct the model to read prior runs from MCP persistence at the start of each invocation:

| Skill | MCP tool | What it reads |
|-------|----------|---------------|
| `superforecasting` | `scenario_calibration` | Brier score history, overconfidence bias |
| `scenario-builder` | `scenario_calibration` | Calibration curve |
| `metacognition` | `scenario_calibration` | Overconfidence bias for prediction calibration |
| `kata-improvement` | `kanban_board_list` + `kanban_task_list` | Prior PDCA cycles |
| `company-research-flash` | `forecast_list` | Prior price targets |
| `swarm-intelligence` | `swarm_get_swarm` / `swarm_get_local_swarm` | Prior swarm state |

See [`skills-and-composition.md`](skills-and-composition.md) §Composition Principles
for the five co-evolution principles (determinism frontier, persistence-grounded
learning, failure surfacing, lisp scaffold, co-evolution loop).
