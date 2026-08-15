# Continuation Prompt: Skill ↔ MCP Co-Evolution — Phase 3 and Follow-ups

You are continuing work on evolving the kask skill system to natively integrate
with the kask MCP servers. Phase 1 (migration) and Phase 2 (feedback loops) are
complete. This prompt covers Phase 3 (co-evolution) and the four follow-up
questions documented in the co-evolution plan.

## What exists

### Co-evolution plan (authoritative)

**File**: `zed-kask/kask/docs/plans/skill-mcp-coevolution.md`

Read this file first. It is the complete research plan covering all three phases.
The status line says "Phase 1 + Phase 2 implemented. Phase 3 (co-evolution)
pending." The "Open questions" section at the end has two subsections:
- "Resolved (Phase 1 + Phase 2 implementation)" — 4 items with resolutions
- "Follow-up questions (post-Phase 2)" — 4 items with priority and effort estimates

The "Prioritized migration list" has a Status column showing ✅ Done / ⏳ Pending
for each skill. The "Infrastructure needed" table has a Status column showing
what was built vs what remains.

### Skill ↔ MCP integration reference

**File**: `zed-kask/kask/docs/explanation/skill-mcp-integration.md`

The complete reference for how flowdef steps invoke MCP tools: the two invocation
patterns (`action: execute` with `mcp:`, and `action: select` with `mcp:`/`tool:`),
the action dispatch table, the code path from flowdef step → `StepMachine::dispatch_action`
→ `execute_tool_invoke` → `ToolPort::invoke` → `McpRuntime`, and guidance on when
to use native `execute` steps vs. agent-mediated tool calls. Read this before
editing any flowdef manifest.

### What was built in Phase 1 + Phase 2

#### Phase 1: 4 high-priority skills migrated to native `execute` steps

| Skill | Execute steps | Total steps | MCP tools called |
|---|---|---|---|
| `superforecasting` | 3 | 21 | `market_match`, `scenario_score`, `scenario_calibration` |
| `scenario-builder` | 3 | 11 | `scenario_calibration`, `market_match`, `scenario_build` |
| `kanban-task-management` | 5 | 19 | `kanban_board_create`, `kanban_task_spawn`, `kanban_task_comment`, `kanban_board_list`, `kanban_task_list` |
| `swarm-intelligence` | 4 | 13 | `swarm_get_swarm` (×2), `swarm_get_local_swarm` (×2) |

All `on_failure` configs use `action: report` (wires `curator_report_skill_use_issue`).

#### Phase 2: 3 curated feedback loops

**Loop 1 (Calibration)**: Closed for superforecasting and scenario-builder.
Forecast → persist via `scenario_score` → market resolves → Brier score →
`scenario_calibration` reads Brier → `overconfidence_score` feeds
`apply_calibration_adjustment` → next forecast is calibrated.

**Loop 2 (Skill-use reporting)**: `curator_report_skill_use_issue` MCP tool built
on `hkask-mcp-curator`. `OnFailureConfig` extended with `action: report` — when
any `execute` step fails, the executor calls `curator_report_skill_use_issue`
with the skill name, tool name, step ordinal, and error, then escalates. Reports
stored as episodic h_mems with entity `skill_use_issue:<skill_name>`.

**Loop 3 (Persistence-grounded learning)**: Each migrated skill reads prior runs
from MCP persistence at the start of each invocation:
- `superforecasting` step 18: `scenario_calibration`
- `scenario-builder` step 1: `scenario_calibration`
- `kanban-task-management` steps 11-12: `kanban_board_list` + `kanban_task_list`
- `swarm-intelligence` steps 1-2: `swarm_get_swarm` / `swarm_get_local_swarm`
- `company-research-flash` step 0: `forecast_list`

#### Pre-existing issues fixed

- 11 manifests had Jinja `{{ }}` syntax in `condition:` fields — converted to
  native condition evaluator syntax (dot paths, `==`, `!=`, `AND`, `OR`, `NOT`).
- `prompt-enhance/enhance-output-render.j2` — registered orphan template, fixed
  `sort_by` filter (unsupported in minijinja).
- `step_actions.rs` — fixed unterminated string literal (`"{\"partial":` →
  `"{\"partial\":"`).
- Templates updated to handle `MatchCandidate` wrapper from `market_match`
  (`{% set market = m.market | default(m) %}`).
- `swarm-intelligence` SENSE and CHECK templates updated to consume
  `fetched_swarm_state` / `fetched_post_act_act_state` from execute steps.
- `scenario-builder` focal-question template updated to consume
  `prior_calibration` from the `scenario_calibration` execute step.

### Key code references (verified)

- **Action dispatch**: `kask/crates/hkask-templates/src/step_machine.rs` lines 346–373 —
  `dispatch_action` matches on `node.action`, routes `execute`/`feedback`/`validate`/
  `retrieve` to `execute_tool_invoke`.
- **Tool invocation**: `kask/crates/hkask-templates/src/step_actions.rs` lines 443–497 —
  `execute_tool_invoke` reads `node.mcp`, resolves `input_mapping`, calls `invoke_tool`.
- **`invoke_tool`**: `kask/crates/hkask-templates/src/step_actions.rs` line 1292 —
  `pub(crate) async fn invoke_tool` (made pub(crate) for `dispatch_with_retry` access).
- **`on_failure` enforcement**: `kask/crates/hkask-templates/src/step_machine.rs` —
  `dispatch_with_retry` checks `on_failure.action` for all step types. `"report"` arm
  calls `curator_report_skill_use_issue` via `invoke_tool` (best-effort), then escalates.
  `"halt"`/`"escalate"` arms log and return `Effect::Exit(Escalated)`.
- **`manifest_id` on `StepMachine`**: `kask/crates/hkask-templates/src/step_machine.rs` —
  `pub(crate) manifest_id: String` field, passed to `StepMachine::new` from
  `executor.rs`, `execute_flowdef`, and `execute_parallel`.
- **`OnFailureConfig`**: `kask/crates/hkask-templates/src/bundle/manifest.rs` lines 146–156 —
  `action: String` (supports `"halt"`, `"escalate"`, `"report"`), `resume: String`.
- **`curator_report_skill_use_issue`**: `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` —
  stores reports as episodic h_mems with entity `skill_use_issue:<skill_name>`.
- **`ReportSkillUseIssueRequest`**: `kask/mcp-servers/hkask-mcp-curator/src/types.rs` —
  `{skill_name, tool_name, step_ordinal, error, tool_input?, failure_type?}`.
- **Tool surface test**: `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` —
  `tool_surface_is_exactly_10_registered_tools` (was 9, now 10).
- **Step struct**: `kask/crates/hkask-templates/src/bundle/manifest.rs` lines 56–141 —
  `BundleManifestStep` has `mcp: Option<String>`, `condition: Option<String>`,
  `on_failure: Option<OnFailureConfig>`.
- **Condition evaluator**: `kask/crates/hkask-templates/src/condition.rs` —
  `evaluate_step_condition` supports truthy, `NOT`, `AND`, `OR`, `==`, `!=`, `<`,
  `<=`, `>`, `>=`. Does NOT render Jinja. Unset variables resolve to literal
  strings (truthy for non-empty), not `None`.
- **Loop execution**: `kask/crates/hkask-templates/src/step_actions.rs` lines 148–195 —
  `execute_loop` sets `self.pc` to the loop target via `Effect::Reenter(step_id)`.
  The loop target determines which steps re-run on each iteration.

### Tests pinning the migration

- `kask/crates/hkask-templates/tests/yaml_schema_validation.rs` — 4 tests (one per
  migrated skill) pinning step counts, execute step ordinals, `mcp:` fields,
  `on_failure.action == "report"`, and `condition:` gates.
- `kask/crates/hkask-templates/tests/company_research_manifest_test.rs` — pins
  step count (26), execute step count (14), and `mcp:` field presence for
  company-research-flash.
- `kask/crates/hkask-templates/src/registry.rs` — 2 swarm-intelligence tests
  pinning compute step ordinals, accumulator threading, and convergence signal
  binding.
- `kask/mcp-servers/hkask-mcp-curator/tests/schema_compliance.rs` — 9 schema
  compliance tests (was 7, now 9 with `CuratorConsultRequest` and
  `ReportSkillUseIssueRequest`).

## The work to continue (Phase 3 + follow-ups)

### Follow-up question #1: `forecast_persist` tool for pre-computed price targets

**Priority**: Medium. **Effort**: Small.

**Problem**: The `calibrate_forecast` tool on `hkask-mcp-companies` persists a
forecast but runs its own Fermi decomposition model — it doesn't accept a
pre-computed PT from the company-research valuation step. The `forecast_record`
tool requires the actual outcome (can't persist a pending PT). There's no tool
to persist a pre-computed price target for later Brier scoring.

**Proposed solution**: Build a new `forecast_persist` tool on
`hkask-mcp-companies` that accepts `{symbol, forecast_date, horizon,
forecast_multiple, forecast_price_change, forecast_id?}` and stores it without
an outcome. The stored forecast can later be resolved by `forecast_record` when
the horizon passes. The storage infrastructure already exists via
`calibrate_forecast`'s persistence path — the new tool would reuse the same
`PersistedForecast` storage but skip the Fermi decomposition.

**Files to modify**:
- `kask/mcp-servers/hkask-mcp-companies/src/tools/valuation.rs` — add `forecast_persist` tool
- `kask/mcp-servers/hkask-mcp-companies/src/types.rs` — add `ForecastPersistRequest`
- `kask/mcp-servers/hkask-mcp-companies/tests/schema_compliance.rs` — add schema test
- `kask/registry/manifests/company-research-flash.yaml` — add `execute` step after valuation (step 16) to persist the PT
- `kask/crates/hkask-templates/tests/company_research_manifest_test.rs` — update step count
- `kask/crates/hkask-templates/tests/manifest_load_validation.rs` — add `forecast_persist` to `KNOWN_MCP_TOOLS`

### Follow-up question #2: `on_failure` resume text not surfaced to operator

**Priority**: Low. **Effort**: Medium.

**Problem**: The `dispatch_with_retry` enforcement logs the `resume` text via
`tracing::warn!` but doesn't store it in the step result or `CascadeOutcome`.
The operator sees `ExitKind::Escalated` but not the resume instruction.

**Proposed solution**: Extend `CascadeOutcome` to carry a
`resume_text: Option<String>` field, populated from the `on_failure.resume`
config when the escalation is triggered by `on_failure`. This requires:
1. Adding `resume_text` to `CascadeOutcome` in `step_machine.rs`
2. Threading it from `dispatch_with_retry` through `run_pass` to `run`
3. Updating `PassResult::Exit` to carry the resume text
4. Updating `CascadeOutcome` consumers in `executor.rs` and callers

**Files to modify**:
- `kask/crates/hkask-templates/src/step_machine.rs` — `CascadeOutcome`, `PassResult`, `dispatch_with_retry`, `run`
- `kask/crates/hkask-templates/src/executor.rs` — `execute_manifest_into` and callers

### Follow-up question #3: Hardcoded `scenario_type` and `time_horizon`

**Priority**: Low. **Effort**: Small.

**Problem**: Superforecasting step 16 constructs a `ScenarioEvent` with
`scenario_type: "emerging_economic"` and `time_horizon: "strategic"` as
hardcoded defaults. All forecasts are categorized as "emerging economic" in the
calibration store, which could skew per-domain calibration.

**Proposed solution**: Have the triage step (step 1) classify the forecasting
question into a scenario type and time horizon, thread the classification
through to the persistence step via `input_mapping`.

**Files to modify**:
- `kask/registry/templates/superforecasting/stage_0_triage.j2` — add `scenario_type` and `time_horizon` to output contract
- `kask/registry/manifests/superforecasting.yaml` — thread `step_1_result.scenario_type` and `step_1_result.time_horizon` through to step 16's `input_mapping`

### Follow-up question #4: Curator directive targeting MCP tool schemas

**Priority**: Medium. **Effort**: TBD.

**Problem**: Can `curator_directive` currently target MCP tool schemas, or only
skill thresholds? If it can only target skills, we need to extend it to target
MCP tool configuration. This is the Phase 3 co-evolution orchestration gap.

**Action**: Verify by reading the curator MCP server code. Check if
`curator_directive` exists as an MCP tool or only as an in-process Curator agent
capability. If it doesn't exist as an MCP tool, design and build it.

**Files to investigate**:
- `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` — check for `curator_directive` tool
- `kask/crates/hkask-bridge/src/curator_agent.rs` (if exists) — check in-process Curator agent

### Phase 3: Co-evolution — skills and MCP tools adapting together

Phase 3 is described in the co-evolution plan (section 3.1–3.3). The three
components:

1. **Skills reveal MCP tool design improvements** (section 3.1): As skills use
   `execute` steps, they reveal schema mismatches, missing inputs, confusing
   output shapes. The skill-use reporting loop (Phase 2.2) captures these signals.
   The Curator analyzes patterns and issues directives.

2. **MCP tools reveal skill design improvements** (section 3.2): As MCP tools
   gain new capabilities, skills should adopt them. The plan lists specific
   examples: `scenario_calibration` → `metacognition`, `portfolio_daily_returns`
   → `company-research`, `evaluate_evidence` → `superforecasting`/`scenario-builder`.

3. **The Curator as co-evolution orchestrator** (section 3.3): The Curator reads
   scores + skill-use reports, issues directives targeting both skill thresholds
   and MCP tool schemas. This requires:
   - The skill-use reporting channel (✅ built in Phase 2)
   - The Curator's analysis of MCP tool usage patterns (⏳ pending)
   - The Curator's directives targeting MCP tool schemas (⏳ pending — follow-up #4)

### Remaining medium-priority skill migrations

From the prioritized migration list, these skills are still pending:

| Skill | MCP server | Migration | Priority |
|---|---|---|---|
| `graph-audit` | codegraph | Convert code-graph queries to `execute` steps | Medium |
| `metacognition` | scenarios + prediction-markets | Add `execute` steps to read prior Brier scores and calibration | Medium |
| `kata-improvement` | kata-kanban | Add `execute` steps to read prior PDCA cycles from the kanban MCP | Medium |

## Key design rules to follow

- **MCP tool calls are native `execute` steps**, not "post-cascade instructions
  for the agent." Every MCP tool call in the flowdef uses `action: execute` with
  `mcp: <tool_name>`.
- **MCP tool failures must not collapse to `None`**. Per `.rules`: "Opt-in
  features that fail must log the failure classification, not collapse to `None`
  via `.ok()?`." The flowdef `on_failure` config should use `action: report`
  (which calls `curator_report_skill_use_issue` before escalating).
- **No `unwrap_or(0)` on regulation signals**. The `scenario_calibration` step
  must not default `overconfidence_bias` to 0.0 on failure — that silently
  disables calibration. Use `on_failure: { action: report, resume: "..." }`.
- **Renumber carefully**. When adding `execute` steps, all downstream
  `input_mapping` references (`step_N_result`) must be updated. A missed
  renumber will silently bind to the wrong step's output.
- **Update manifest compliance tests** in the same PR as the manifest change.
- **Do not change template content unless necessary**. The migration is about
  *how* tools are called (flowdef `execute` steps vs. agent-mediated), not
  *what* the templates reason about.
- **Do not remove other agents' in-process work**. Dead code from another
  agent's uncommitted work is not yours to remove. Use `#[allow(dead_code)]`
  with a comment if clippy fails on it.
- **Stale diagnostics after bulk edits**: the crate's lib root is authoritative,
  not individual-file diagnostics. Don't retry stale diagnostics or fix phantom
  errors.

## Before starting

1. **Read the co-evolution plan** (`zed-kask/kask/docs/plans/skill-mcp-coevolution.md`)
   in full — especially the "Follow-up questions" and "Phase 3" sections.
2. **Read the skill ↔ MCP integration doc**
   (`zed-kask/kask/docs/explanation/skill-mcp-integration.md`) to understand the
   `execute` step pattern.
3. **Verify the current state** by running `cargo test -p hkask-templates -p
   hkask-mcp-curator` — all tests should pass.
4. **Check for other agents' in-process work** by running `git diff --stat HEAD`
   — do not modify files that other agents are actively editing.

Then begin with follow-up question #1 (`forecast_persist` tool), since it closes
the calibration loop for equity research skills and the storage infrastructure
already exists.
