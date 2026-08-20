---
title: "Skill ↔ MCP Tool Integration"
audience: [developers, skill-authors, agents]
last_updated: 2026-08-15
version: "0.36.0"
status: "Active"
domain: "Skill System"
mds_categories: [composition, integration]
---

# Skill ↔ MCP Tool Integration

> **Why this document exists:** Multiple agents and developers have incorrectly
> assumed that MCP tool calls can only happen through the agent's tool-use loop
> (the LLM deciding to call a tool). This is wrong. The flowdef natively supports
> MCP tool invocation as first-class steps. This document exists to prevent that
> error from recurring.

## The capability you may be missing

A kask flowdef manifest can invoke MCP tools **directly from a step**, without
going through the LLM's tool-use loop. The tool call is deterministic: it
happens at a known step ordinal, with inputs bound from prior step results,
before any LLM round-trip. The result is stored as `step_{ordinal}_result` and
is available to every downstream step.

This is not a future feature. It has been in the executor since at least v0.31
and is production-tested by the `pipeline-capabilities-researcher.yaml` manifest
(13 `execute` steps + 12 `gate` steps).

## Two ways to invoke an MCP tool from a flowdef step

### Pattern 1: `action: execute` (dedicated MCP tool step)

```yaml
steps:
  - ordinal: 3
    action: execute
    description: "Fetch DCF valuation from hkask-mcp-companies"
    mcp: dcf_valuation          # tool name (resolved via ToolPort::get_tool_info)
    gas_cap: 5000
    timeout_seconds: 120
    input_mapping:
      symbol: "{{ ticker }}"
      growth_rate: "{{ step_2_result.estimated_growth }}"

  - ordinal: 4
    action: select              # LLM reasons over the tool output
    template_ref: my-skill/synthesis
    input_mapping:
      dcf_result: "{{ step_3_result }}"
      ticker: "{{ ticker }}"
```

The `mcp:` field carries the tool name. `input_mapping` binds the tool's
arguments from prior step results (same Jinja2 `{{ step_N_result.field }}`
mechanism as `select` steps). The result is stored as `step_3_result`.

### Pattern 2: `action: select` with `mcp:` field

```yaml
steps:
  - ordinal: 2
    action: select
    description: "Score rationales via market_score_rationale MCP tool"
    mcp: hkask-mcp-prediction-markets
    tool: market_score_rationale
    gas_cap: 8192
    timeout_seconds: 120
    input_mapping:
      rationale: "{{ step_1_result.rationale }}"
```

When a `select` step carries an `mcp:` field, it is a direct MCP tool
invocation — no `template_ref` is needed. The `eqm` skill uses this pattern
(`kask/registry/manifests/eqm.yaml` step 2).

## Which action names route to MCP tool invocation?

The `StepMachine::dispatch_action` in
`kask/crates/hkask-templates/src/step_machine.rs` (lines 378–389) routes these
actions to `execute_tool_invoke`:

```rust
"execute" | "feedback" | "validate" | "retrieve" => {
    self.execute_tool_invoke(node, infra).await
}
```

The full canonical action list
(`kask/crates/hkask-templates/tests/manifest_compliance.rs`):

| Action | Routes to | Purpose |
|--------|-----------|---------|
| `select` | `execute_select` (LLM inference) | Render a Jinja2 template and send to LLM |
| `select` + `mcp:` | `execute_tool_invoke` | Direct MCP tool call (no template) |
| `populate` | `execute_select` | Alias for `select` (legacy) |
| `compute` | `execute_compute` | Deterministic math (`lisp.eval`, `bayesian_update`, etc.) |
| `execute` | `execute_tool_invoke` | MCP tool call |
| `feedback` | `execute_tool_invoke` | MCP tool call (feedback semantics) |
| `validate` | `execute_tool_invoke` | MCP tool call (validation semantics) |
| `retrieve` | `execute_tool_invoke` | MCP tool call (retrieval semantics) |
| `render` | `execute_render` | Render a Jinja2 template without LLM (RenderAct) |
| `flowdef` | `execute_flowdef` | Recursively execute a sub-manifest |
| `loop` | (loop control) | Re-enter at a target ordinal |
| `choice` | (branching) | Route based on step output |
| `gate` | (shell gate) | Run a shell command, check for GATE_PASS/GATE_FAIL |
| `abort` | (terminal) | Stop the cascade |
| `escalate` | (terminal) | Stop and escalate to human/Curator |

## How the tool call flows through the system

```
flowdef step (action: execute, mcp: <tool_name>)
  ↓
StepMachine::dispatch_action
  ↓
execute_tool_invoke (step_actions.rs)
  ↓ resolves mcp: field via TemplateRenderer::render_inline (for ${variable} refs)
  ↓ resolves input_mapping via resolve_mapping_value (Jinja2 context binding)
  ↓
invoke_tool (step_actions.rs line 1266)
  ↓ tools.get_tool_info(tool_name) → { server_id, tool_name }
  ↓
ToolPort::invoke(server_id, tool_name, input, webid)
  ↓ webid = WebID::from_persona(b"manifest-executor") — accounting identity, not credential
  ↓ same gas budget, same call cap, same reg.tool.* span emission as agent-initiated calls
  ↓
McpRuntime (implements ToolPort)
  ↓ dispatches to the MCP server child process
  ↓
result stored as step_{ordinal}_result
  ↓ available to all downstream steps via input_mapping
```

## Why this is better than agent-mediated tool calls

| Property | Agent-mediated (LLM decides to call) | Flowdef-native (`action: execute`) |
|----------|--------------------------------------|-----------------------------------|
| Determinism | LLM may forget to call the tool, or call it with wrong args | Tool call happens at a known step ordinal with bound inputs |
| Governance | Flows through the agent's tool-use loop | Flows through `ToolPort::invoke` — same gas, same cap, same spans |
| Testability | Hard to test (depends on LLM behavior) | Testable — `pipeline_manifest_parse_test.rs` pins the contract |
| Composability | Tool result is in the agent's context, not in the step chain | Tool result is in `step_{ordinal}_result`, available to all downstream steps |
| Cost | LLM round-trip to decide to call the tool | No LLM round-trip — direct call |
| Error handling | LLM may silently skip a failed tool | Step failure surfaces as a `TemplateError`, halts the cascade |

## When to use each pattern

### Use `action: execute` when:

- The tool call is **required** (the pipeline cannot proceed without it)
- The tool's inputs are **fully determined** by prior step results (no LLM judgment needed to construct the call)
- The tool's output is **consumed by a downstream step** (not just by the LLM's reasoning)
- You want **deterministic, testable** tool invocation

Example: `dcf_valuation(symbol, growth_rate)` where `growth_rate` comes from a
prior `select` step's output.

### Use agent-mediated tool calls when:

- The LLM needs to **decide whether** to call the tool (conditional on its reasoning)
- The tool's inputs require **LLM judgment** to construct (e.g., crafting a search query from context)
- The tool is **optional** (the pipeline can proceed without it)
- The tool call is **exploratory** (the LLM is probing, not following a fixed pipeline)

Example: an agent deciding whether to call `web_search` based on whether its
current knowledge is sufficient.

### Use `action: select` with `mcp:` when:

- You want the tool call to be deterministic but the step is semantically a "select" (choosing data)
- The `eqm` skill uses this pattern for `market_score_rationale` — it's always called, always with the same input shape

## Common error: "instruct the agent to call the tool"

Some existing skills (notably `kanban-task-management` and `swarm-intelligence`)
use a pattern where the Jinja2 template includes text like:

> "Includes post-cascade instructions for the agent to call kanban_board_create
> and kanban_task_create."

This is the **agent-mediated** pattern — the template tells the LLM to call the
tool, and the LLM's tool-use loop performs the call. This works, but it is
weaker than `action: execute` for the reasons listed above. Skills that use this
pattern should be evaluated for migration to native `execute` steps where the
tool call is required and its inputs are deterministic.

## Verified code references

All claims in this document are grounded in:

- `kask/crates/hkask-templates/src/step_machine.rs` lines 378–389 — action dispatch (`execute` / `feedback` / `validate` / `retrieve` → `execute_tool_invoke`)
- `kask/crates/hkask-templates/src/step_actions.rs` lines 447–451 — `execute_tool_invoke`
- `kask/crates/hkask-templates/src/step_actions.rs` lines 1298–1308 — `invoke_tool` helper
- `kask/crates/hkask-templates/src/bundle/manifest.rs` — `BundleManifestStep` struct (the `mcp:` field)
- `kask/crates/hkask-templates/src/executor.rs` — `ManifestExecutor::execute_manifest_into` (returns typed `CascadeOutcome`; callers extract via `extract_final_step_result`)
- `kask/crates/hkask-templates/src/hkask_templates.rs:36` — `pub use executor::extract_final_step_result` (crate-root re-export; downstream crates must match on `hkask_templates::extract_final_step_result`, not the submodule path)
- `kask/crates/hkask-templates/tests/manifest_compliance.rs` line 6 — canonical action list
- `kask/crates/hkask-templates/tests/manifest_properties.rs` line 131 — `select` with `mcp:` validation
- `kask/crates/hkask-templates/tests/pipeline_manifest_parse_test.rs` lines 74–82 — `execute` steps require `mcp:`
- `kask/registry/manifests/eqm.yaml` step 2 — live `select`-with-`mcp` usage
- `kask/corpus/pipeline-capabilities-researcher.yaml` — live `execute`-step pipeline (13 execute + 12 gate steps)
- `kask/crates/hkask-capability/src/tool_port.rs` — `ToolPort` trait and `ToolPortError`
- `crates/zed/src/main.rs` lines 1210–1214 — `tool_port` construction and wiring
- `kask/crates/hkask-templates/src/step_machine.rs` — `dispatch_with_retry` checks
  `on_failure` for all step types (not just gates); `"report"` arm calls
  `curator_report_skill_use_issue` via `invoke_tool` (best-effort), then escalates.
  `resume_text` field on `CascadeOutcome` surfaces the resume instruction.
- `kask/crates/hkask-templates/src/bundle/manifest.rs` — `OnFailureConfig` with
  `action: "report"` and `resume` text; `validate()` accepts ordinal 0 as a
  valid starting ordinal (pre-processing step pattern).
- `kask/mcp-servers/hkask-mcp-companies/src/tools/valuation.rs` — `forecast_persist`
  tool for pre-computed price targets; `forecast_record` gracefully falls back
  to Brier-only scoring when the snapshot doesn't contain a full `StoredForecast`.
- `crates/agent/src/tools/curator_tools.rs` — `CuratorDirectiveRequest::EvolveMcpToolSchema`
  variant for co-evolution directives targeting MCP tool schemas.
- `kask/crates/hkask-regulation/src/cybernetics_loop.rs` — `apply_evolve_mcp_tool_schema`
  handler persists the evolution request to the regulation ledger.

## Co-Evolution Patterns

The `execute` step pattern enables three feedback loops that co-evolve skills
and MCP tools:

### The Calibration Loop (Forecast → Outcome → Brier → Calibrate)

Skills that produce forecasts persist them via `scenario_score` (execute step).
When outcomes resolve, `forecast_record` scores the forecast (Brier score).
The next invocation reads prior Brier scores via `scenario_calibration`
(execute step at ordinal 0) and adjusts its predictions.

Wired in: `superforecasting` (steps 18, 16), `scenario-builder` (step 1),
`metacognition` (step 0), `company-research-flash` (steps 0, 26).

### The Skill-Use Reporting Loop (Skill → Curator → MCP Evolution)

When an `execute` step fails, its `on_failure: { action: report }` config calls
`curator_report_skill_use_issue` with the skill name, tool name, step ordinal,
and error. The Curator reads these reports via `curator_memory_recall` and
issues `EvolveMcpToolSchema` directives to evolve the MCP tool's schema.

The `resume_text` field on `CascadeOutcome` surfaces the author's resume
instruction to the operator — they see not just `ExitKind::Escalated` but
also what was lost and how to proceed.

### The Persistence-Grounded Learning Loop (Skill → MCP Persistence → Skill)

Each migrated skill reads prior runs from MCP persistence at the start of
each invocation:

| Skill | MCP tool | What it reads |
|-------|----------|---------------|
| `superforecasting` | `scenario_calibration` | Brier score history, overconfidence bias |
| `scenario-builder` | `scenario_calibration` | Calibration curve |
| `metacognition` | `scenario_calibration` | Overconfidence bias for prediction calibration |
| `kata-improvement` | `kanban_board_list` + `kanban_task_list` | Prior PDCA cycles |
| `company-research-flash` | `forecast_list` | Prior price targets |
| `swarm-intelligence` | `swarm_get_swarm` / `swarm_get_local_swarm` | Prior swarm state |
| `graph-audit` | `codegraph_stats` + `codegraph_structure` | Index statistics, top symbols |
| `bug-hunt` | `codegraph_analysis` | Pre-computed quality findings |
| `diagnose` | `codegraph_impact` | Blast radius for the bug's location |
| `capabilities-reasoner` | `curator_memory_recall` | Prior capability evaluations |

See [`skills-and-composition.md`](skills-and-composition.md) §Composition Principles
for the five co-evolution principles (determinism frontier, persistence-grounded
learning, failure surfacing, lisp scaffold, co-evolution loop) and the
[`skill-mcp-integration.md`](skill-mcp-integration.md) §Co-Evolution Patterns
for the three feedback loops (calibration, skill-use reporting, persistence-grounded learning).
