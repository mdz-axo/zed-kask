# Research Plan: Skill ↔ MCP Co-Evolution

**Status**: Phase 1 + Phase 2 + Phase 3 implemented. All follow-ups (#1–#4) implemented. All medium and low priority skill migrations complete.
**Date**: 2026-08-14 (plan), 2026-08-15 (implementation)
**Author**: Curator (GLM 5.2)

---

## Problem statement

The kask skill system and the kask MCP servers were built to work together, but
most skills do not use the MCP tools that were designed for them. The skills
either (a) mention MCP tools in template descriptions but never invoke them, or
(b) use the "post-cascade instructions for the agent to call..." pattern, which
delegates tool invocation to the LLM's tool-use loop — weaker than native
`action: execute` steps.

Meanwhile, the MCP servers provide persistence (forecast ledgers, calibration
curves, portfolio returns, scenario storage) that could close feedback loops for
every skill — but the skills don't write to or read from these persistence
layers. The calibration data accumulates in the MCP servers with no consumer;
the skills run without the grounding that data would provide.

This plan sketches how to recombine skills with their MCP counterparts and build
the curated feedback loops that let both sides co-evolve.

## Verified inventory: current state

### Skills that already use native MCP invocation (`action: execute` or `mcp:`)

| Skill | MCP tool used | How | Verified in |
|-------|---------------|-----|-------------|
| `eqm` | `market_score_rationale` (hkask-mcp-prediction-markets) | `action: select` with `mcp:` field (step 2) | `kask/registry/manifests/eqm.yaml` |
| `eqm-improvement` | (uses `mcp:` field) | `action: select` with `mcp:` field | `kask/registry/manifests/eqm-improvement.yaml` |

### Skills that mention MCP tools but don't invoke them natively

| Skill | MCP tool mentioned | Current pattern | Migration candidate |
|-------|--------------------|-----------------|---------------------|
| `superforecasting` | `scenario_calibration`, `market_check_resolutions`, `market_cmp` | None — flowdef uses only `select` and `compute`. The `overconfidence_bias` input to step 17 (`apply_calibration_adjustment`) defaults to 0.0 "until an external calibration curve supplies it" — but no step fetches that curve. | **High priority** — the scenarios MCP was built for this skill |
| `scenario-builder` | `market_context` (prediction-market records) | The `market_context` input is declared in the flowdef inputs, with a description saying it's "fetched by the invoking agent from hkask-mcp-prediction-markets before cascade invocation" — but no flowdef step fetches it. | **High priority** — the input exists but the fetch is delegated to the agent |
| `kanban-task-management` | `kanban_board_create`, `kanban_task_create`, `kanban_task_move`, `kanban_task_verify`, `kanban_task_comment`, `kanban_task_spawn`, `kanban_board_list`, `kanban_task_list` | "Post-cascade instructions for the agent to call..." — every template tells the LLM to call the kanban MCP tools | **High priority** — every tool call is required and deterministic |
| `swarm-intelligence` | `swarm_get_swarm`, `swarm_list_local_agents`, `swarm_search_knowledge`, `swarm_fire`, `swarm_delegate`, `swarm_delegate_local`, `swarm_create_local_agent`, `swarm_clone_to_local`, `swarm_request_consent`, `swarm_generate_prompt`, `swarm_reconfigure_local_agent` | Templates describe fetching state from MCP tools and delegating via MCP tools, but the fetch is done by the LLM's tool-use loop, not by flowdef steps | **Medium priority** — the SENSE and CHECK phases should use `execute` steps for state fetching; the ACT phase delegation is correctly agent-mediated (consent-gated) |
| `graph-audit` | `codegraph` tools (query, traverse, analyze) | Templates describe querying the code graph, but the queries are done by the LLM | **Medium priority** — code graph queries are deterministic and should be `execute` steps |
| `bug-hunt` | (mentions MCP tools in description) | Not verified in detail | **Low priority** — investigate |
| `diagnose` | (mentions MCP tools in description) | Not verified in detail | **Low priority** — investigate |

### MCP servers with persistence that skills don't consume

| MCP server | Persistence surface | Skills that should consume it | Current consumption |
|------------|---------------------|-------------------------------|---------------------|
| hkask-mcp-prediction-markets | `market_calibration` (Brier scores, calibration curves), `market_check_resolutions` (resolved outcomes), CMP portfolio storage | `superforecasting`, `scenario-builder`, `eqm`, `metacognition`, `kata-improvement` | Only `eqm` writes (via `market_score_rationale`); none read calibration back |
| hkask-mcp-scenarios | `scenario_build`, `scenario_research`, `scenario_score` (Brier scoring of scenario forecasts), `scenario_calibration` | `superforecasting`, `scenario-builder`, `company-research` (planned) | None — skills generate scenarios via LLM templates, not via the scenarios MCP |
| hkask-mcp-portfolio | `ledger_apply`, `ledger_read`, `portfolio_daily_returns` (TWR/MWR) | `company-research` (planned), any skill that makes buy/sell/hold recommendations | None — no skill tracks realized returns against its recommendations |
| hkask-mcp-companies | `company_transcript`, `comparable_analysis`, `dcf_valuation`, `expectations_gap`, `research_search` | `company-research` (planned), `listening` | `listening` could use `company_transcript` but currently doesn't |
| hkask-mcp-kata-kanban | `kanban_board_*`, `kanban_task_*` | `kanban-task-management`, `kata-improvement`, `kata-coaching` | `kanban-task-management` uses agent-mediated calls; `kata-improvement` doesn't use the kanban MCP at all |
| hkask-mcp-swarm | `swarm_get_swarm`, `swarm_delegate`, etc. | `swarm-intelligence`, `swarm-steering`, `swarm-compose-guide` | `swarm-intelligence` uses agent-mediated calls |
| hkask-mcp-curator | Curator directives, status, analysis | `metacognition`, `self-improvement`, all skills (via Curator feedback) | Not verified — investigate whether skills report to the Curator |

## Phase 1: Migrate high-priority skills to native MCP invocation

### 1.1 `superforecasting` → hkask-mcp-scenarios + hkask-mcp-prediction-markets

**Current gap**: The superforecasting flowdef has 18 steps, all `select` or
`compute`. It generates forecasts via LLM templates and deterministic math, but:
- Step 17 (`apply_calibration_adjustment`) takes an `overconfidence_bias` input
  that defaults to 0.0 because no step fetches the calibration curve.
- Step 14 (`stage_7_record`) creates a "structured forecast record for later
  scoring/post-mortem" — but the record stays in the step chain; it's not
  persisted to the scenarios MCP for later Brier scoring.
- The `market_context` input (prediction-market probabilities as outside view)
  is declared but never fetched.

**Migration**:

Add `execute` steps:
1. Before step 4 (outside view): `action: execute`, `mcp: market_cmp`, to fetch
   market-implied probabilities as an outside-view anchor.
2. Before step 17 (calibration adjustment): `action: execute`,
   `mcp: scenario_calibration`, to fetch the forecaster's Brier score and
   overconfidence bias from prior resolved forecasts.
3. After step 14 (record): `action: execute`, `mcp: scenario_score` (or a new
   `forecast_record` tool), to persist the forecast for later resolution and
   Brier scoring.

**Co-evolution loop created**:
```
forecast → persist to scenarios MCP → market resolves → Brier score →
  scenario_calibration reads Brier → overconfidence_bias feeds step 17 →
    next forecast is calibrated
```

This is the loop the superforecasting skill describes in its manifest comments
("until an external calibration curve supplies it") but never closes.

### 1.2 `scenario-builder` → hkask-mcp-prediction-markets + hkask-mcp-scenarios

**Current gap**: The `market_context` input is declared in the flowdef with a
description saying it's "fetched by the invoking agent from
hkask-mcp-prediction-markets before cascade invocation." This delegates the
fetch to the agent — the skill cannot guarantee the data is present.

**Migration**:

Add an `execute` step before step 2 (key forces):
1. `action: execute`, `mcp: market_cmp`, to fetch prediction-market records
   relevant to the focal question. The result feeds `market_context` in step 2.

Add an `execute` step after step 6 (implications):
2. `action: execute`, `mcp: scenario_build` (the scenarios MCP tool), to persist
   the generated scenarios for later comparison against actual outcomes.

**Co-evolution loop created**:
```
scenarios generated → persist to scenarios MCP → time passes →
  actual outcomes compared → scenario_score (Brier) →
    scenario_calibration feeds next scenario-builder invocation
```

### 1.3 `kanban-task-management` → hkask-mcp-kata-kanban

**Current gap**: Every template in the skill uses the "post-cascade instructions
for the agent to call..." pattern. The tool calls are required and
deterministic — there's no reason for the LLM to mediate them.

**Migration**:

Convert each "post-cascade instruction" to an `execute` step:
1. After `populate-board` (decompose phase): `action: execute`,
   `mcp: kanban_board_create` + `action: execute`, `mcp: kanban_task_create`.
2. After `configure-spawn` (delegate phase): `action: execute`,
   `mcp: kanban_task_spawn`.
3. After `execute-task` (delegate phase): `action: execute`,
   `mcp: kanban_task_comment` + `action: execute`,
   `mcp: kanban_task_add_deliverable`.
4. Before `monitor-board` (operate phase): `action: execute`,
   `mcp: kanban_board_list` + `action: execute`, `mcp: kanban_task_list`.
5. After `move-tasks` (operate phase): `action: execute`,
   `mcp: kanban_task_move`.
6. After `verify-completion` (operate phase): `action: execute`,
   `mcp: kanban_task_verify` + `action: execute`, `mcp: kanban_task_move`.

**Co-evolution loop created**:
```
tasks decomposed → persisted to kanban MCP → tasks executed →
  status updates persisted → monitor reads persisted state →
    coordinate-agents reads comment threads → next iteration
```

The kanban MCP becomes the single source of truth for task state, not the
LLM's context window.

### 1.4 `swarm-intelligence` → hkask-mcp-swarm (partial migration)

**Current gap**: The SENSE and CHECK phases describe fetching swarm state from
MCP tools, but the fetch is agent-mediated. The ACT phase delegation is
correctly agent-mediated (consent-gated, requires human approval).

**Migration**:

Convert SENSE and CHECK state-fetching to `execute` steps:
1. SENSE phase: `action: execute`, `mcp: swarm_get_swarm` (ABW mode) or
   `mcp: swarm_list_local_agents` (local mode), to fetch the roster.
2. CHECK phase: same `execute` steps to re-fetch state post-Act.

Leave the ACT phase (`swarm_delegate`, `swarm_delegate_local`) as
agent-mediated — these are consent-gated and require human approval, which
the flowdef cannot provide.

**Co-evolution loop created**:
```
swarm state fetched → SENSE measures it → ORIENT classifies gap →
  DECIDE proposes composition change → ACT delegates (agent-mediated) →
    CHECK re-fetches state → convergence metric computed →
      next iteration's SENSE has the updated state
```

## Phase 2: Build the curated feedback loops

### 2.1 The calibration loop (forecast → outcome → Brier → calibrate)

This is the highest-value loop. It connects:
- **Producers**: `superforecasting`, `scenario-builder`, `company-research`
  (planned), `metacognition` — all produce forecasts/scenarios/predictions.
- **Persistors**: `hkask-mcp-scenarios` (scenario_score, scenario_calibration),
  `hkask-mcp-prediction-markets` (market_check_resolutions,
  market_calibration).
- **Consumers**: the same producers, on their next invocation, reading back
  their calibration data.

**Loop design**:

```
┌─────────────────────────────────────────────────────────┐
│  Skill invocation (e.g. superforecasting)                │
│                                                          │
│  1. EXECUTE: scenario_calibration → read prior Brier     │
│     score + overconfidence_bias for this forecaster      │
│                                                          │
│  2. SELECT: run the forecasting pipeline with the        │
│     calibration bias applied to the prior probability    │
│                                                          │
│  3. EXECUTE: scenario_score → persist the forecast       │
│     (question, probability, resolution criteria,         │
│      expiration date) to the scenarios MCP               │
│                                                          │
│  4. (time passes; the forecast resolves)                 │
│                                                          │
│  5. EXECUTE: market_check_resolutions → fetch resolved   │
│     outcomes for this forecaster's prior forecasts       │
│                                                          │
│  6. COMPUTE: brier_score → score the forecast against    │
│     the outcome                                         │
│                                                          │
│  7. The Brier score is stored in the scenarios MCP       │
│     calibration store                                    │
│                                                          │
│  8. Next invocation reads it in step 1                   │
└─────────────────────────────────────────────────────────┘
```

**Infrastructure needed**: The scenarios MCP already has `scenario_score` and
`scenario_calibration`. The gap is that no skill flowdef calls them. The
migration in Phase 1.1 closes this gap for `superforecasting`; the same pattern
applies to `scenario-builder` and `company-research`.

### 2.2 The skill-use reporting loop (skill → Curator → MCP evolution)

**Current gap**: When a skill fails to use an MCP tool correctly (wrong inputs,
missing fields, confusing schema), there is no feedback mechanism to tell the
MCP server maintainers. The skill just fails or produces degraded output.

**Loop design**:

```
┌─────────────────────────────────────────────────────────┐
│  Skill invocation                                        │
│                                                          │
│  1. EXECUTE: call MCP tool (e.g. dcf_valuation)          │
│                                                          │
│  2. If the tool call fails or produces unexpected        │
│     output:                                              │
│     - The step's on_failure config emits a report        │
│     - The report includes: tool name, input that was     │
│       sent, error or unexpected output, skill name,      │
│       step ordinal                                        │
│     - EXECUTE: curator_report_skill_use_issue            │
│       (a new MCP tool on hkask-mcp-curator)              │
│                                                          │
│  3. The Curator collects reports across skills and       │
│     invocations                                          │
│                                                          │
│  4. The Curator analyzes patterns:                       │
│     - "dcf_valuation fails 40% of the time when          │
│        growth_rate > 0.3" → schema should validate       │
│        growth_rate range                                 │
│     - "market_cmp returns empty for 60% of ticker        │
│        queries" → the tool needs a fallback or better    │
│        matching                                          │
│     - "kanban_task_create input schema is                │
│        misunderstood by 3 different skills" → schema     │
│        documentation needs improvement                   │
│                                                          │
│  5. The Curator issues CuratorDirectives to evolve the   │
│     MCP tool: add validation, improve error messages,    │
│     add fallbacks, improve schema documentation          │
│                                                          │
│  6. The MCP server is updated; the next skill            │
│     invocation benefits                                  │
└─────────────────────────────────────────────────────────┘
```

**Infrastructure needed**: A new MCP tool on `hkask-mcp-curator`:
`curator_report_skill_use_issue`. The tool accepts a structured report (tool
name, input, error, skill name, step ordinal) and stores it for Curator
analysis. The Curator's analysis surface already exists (the `curator_status`
and `curator_directive` tools); this adds a *skill-reported* input channel to
complement the existing runtime telemetry.

### 2.3 The persistence-grounded learning loop (skill → MCP persistence → skill)

**Current gap**: Skills are stateless across invocations. Each invocation starts
from scratch — no memory of prior runs, prior forecasts, prior outcomes. The
MCP servers provide persistence (forecast ledgers, calibration curves, scenario
stores, portfolio returns), but skills don't read from them on invocation.

**Loop design**:

```
┌─────────────────────────────────────────────────────────┐
│  Skill invocation                                        │
│                                                          │
│  1. EXECUTE: read prior runs from the relevant MCP       │
│     persistence surface:                                 │
│     - superforecasting → scenario_calibration            │
│     - scenario-builder → scenario_score (prior scenarios)│
│     - company-research → portfolio_daily_returns         │
│       (realized returns vs prior PTs)                    │
│     - kata-improvement → kanban_task_list (prior PDCA    │
│       cycles)                                            │
│                                                          │
│  2. SELECT: run the skill with prior context injected    │
│     into the template (e.g. "Your prior 5 forecasts      │
│     had a Brier score of 0.23; you were overconfident    │
│     on tech-sector questions")                           │
│                                                          │
│  3. EXECUTE: persist the current run's outputs to the    │
│     MCP persistence surface for the next invocation      │
│                                                          │
│  4. Over time, the skill's behavior adapts based on      │
│     accumulated history — not via prompt editing or       │
│     fine-tuning, but via the persistence layer feeding   │
│     context into each invocation                         │
└─────────────────────────────────────────────────────────┘
```

This is the "kata improvement" pattern applied at the skill level: each
invocation is a PDCA cycle, and the persistence layer is the "check" that
informs the next "plan."

## Phase 3: Co-evolution — skills and MCP tools adapting together

### 3.1 Skills reveal MCP tool design improvements

As skills migrate to native `execute` steps (Phase 1), they reveal MCP tool
design issues that weren't visible when tools were agent-mediated:

| Issue type | Example | How the skill reveals it |
|------------|---------|--------------------------|
| Missing inputs | `dcf_valuation` doesn't accept `wacc_override` | The valuation flowdef needs to pass the forensic-adjusted WACC, but the tool schema doesn't have the field |
| Confusing schemas | `scenario_build` input schema uses `subject` but skills expect `ticker` | The flowdef `input_mapping` reveals the mismatch when binding fails |
| Missing fallbacks | `market_cmp` returns empty for non-US tickers | The flowdef step produces `None`, and the downstream `select` step has no default handling |
| Output shape mismatch | `comparable_analysis` returns an array but the template expects a single object | The `input_mapping` binding fails or produces wrong results |
| Rate limiting | `research_search` times out after 30s for broad queries | The flowdef step's `timeout_seconds` is exceeded |

Each of these is a signal for MCP tool evolution. The skill-use reporting loop
(Phase 2.2) captures these signals and routes them to the Curator.

### 3.2 MCP tools reveal skill design improvements

Conversely, as MCP tools gain new capabilities, skills should adopt them:

| New MCP capability | Skills that should adopt it | How |
|---------------------|----------------------------|-----|
| `scenario_calibration` (Brier score history) | `superforecasting`, `scenario-builder`, `metacognition` | Add an `execute` step to read calibration before the pipeline runs |
| `portfolio_daily_returns` (realized TWR/MWR) | `company-research` (planned) | Add an `execute` step to compare prior PTs against realized prices |
| `evaluate_evidence` (structured evidence quality audit) | `superforecasting` (stage 4), `scenario-builder` (quality gate) | Add an `execute` step to audit evidence quality before synthesis |
| `expectations_gap` (Mauboussin expectations investing) | `superforecasting` (outside view), `company-research` (valuation) | Add an `execute` step to fetch market-implied expectations as an outside-view anchor |

### 3.3 The Curator as the co-evolution orchestrator

The Curator is the cybernetic regulator that closes the co-evolution loop:

```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  Skills produce forecasts/analyses/recommendations          │
│          ↓                                                   │
│  MCP tools persist them                                     │
│          ↓                                                   │
│  Outcomes resolve (markets move, projects complete,         │
│  scenarios unfold)                                          │
│          ↓                                                   │
│  MCP tools score them (Brier, returns, completion)          │
│          ↓                                                   │
│  Curator reads scores + skill-use reports                   │
│          ↓                                                   │
│  Curator issues directives:                                 │
│    - "superforecasting is overconfident on tech →            │
│       increase overconfidence_bias adjustment"              │
│    - "dcf_valuation schema missing wacc_override →           │
│       add field"                                            │
│    - "kanban-task-management should read prior PDCA          │
│       cycles → add execute step"                            │
│          ↓                                                   │
│  Skills and MCP tools are updated                            │
│          ↓                                                   │
│  Next invocation benefits                                    │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

The Curator already has the `curator_status` and `curator_directive` tools.
What's missing is:
1. The skill-use reporting channel (Phase 2.2).
2. The Curator's analysis of MCP tool usage patterns (reading `reg.tool.*`
   spans + skill-use reports).
3. The Curator's directives targeting MCP tool schemas (not just skill
   thresholds).

## Prioritized migration list

| Priority | Skill | MCP server | Migration | Co-evolution loop | Status |
|----------|-------|------------|-----------|-------------------|--------|
| **High** | `superforecasting` | scenarios + prediction-markets | Add `execute` steps for `market_match`, `scenario_calibration`, `scenario_score` | Calibration loop (2.1) | ✅ Done (3 execute steps, 21 total) |
| **High** | `scenario-builder` | prediction-markets + scenarios | Add `execute` steps for `market_match`, `scenario_build`, `scenario_calibration` | Calibration loop (2.1) | ✅ Done (3 execute steps, 11 total) |
| **High** | `kanban-task-management` | kata-kanban | Convert deterministic single-call "post-cascade instructions" to `execute` steps | Persistence loop (2.3) | ✅ Done (5 execute steps, 19 total) |
| **High** | `company-research-flash` | companies + scenarios + prediction-markets | Native `execute` steps from the start + `forecast_list` step 0 + `forecast_persist` step 26 | All loops | ✅ Done (15 execute steps, 27 total) |
| **High** | `company-research-deep` | companies + scenarios | Native `execute` steps from the start | All loops | ✅ Already had 6 execute steps |
| **Medium** | `swarm-intelligence` | swarm | Convert SENSE/CHECK state-fetching to `execute` steps | Persistence loop (2.3) | ✅ Done (4 execute steps, 13 total) |
| **Medium** | `graph-audit` | codegraph | Convert code-graph queries to `execute` steps | Persistence loop (2.3) | ✅ Done (6 execute steps, 25 total) |
| **Medium** | `metacognition` | scenarios + prediction-markets | Add `execute` steps to read prior Brier scores and calibration | Calibration loop (2.1) | ✅ Done (1 execute step, 10 total) |
| **Medium** | `kata-improvement` | kata-kanban | Add `execute` steps to read prior PDCA cycles from the kanban MCP | Persistence loop (2.3) | ✅ Done (2 execute steps, 14 total) |
| **Low** | `bug-hunt` | codegraph | Pre-compute quality analysis via `codegraph_analysis` execute step | Persistence loop (2.3) | ✅ Done (1 execute step, 8 total) |
| **Low** | `diagnose` | codegraph | Pre-compute blast radius via `codegraph_impact` execute step | Persistence loop (2.3) | ✅ Done (1 execute step, 8 total) |

## Infrastructure needed

| Item | Purpose | Priority | Status |
|------|---------|----------|--------|
| `curator_report_skill_use_issue` (MCP tool on hkask-mcp-curator) | Skill-use reporting channel (Phase 2.2) | High | ✅ Built. Wired into `on_failure: report` on all 4 migrated skills. |
| `forecast-ledger` (MCP tool on hkask-mcp-companies) | Persist equity forecasts/PTs for Brier scoring against realized prices | High | ✅ `forecast_list`, `forecast_get`, `forecast_record`, `calibrate_forecast` exist. `forecast_list` wired into `company-research-flash` step 0. `forecast_persist` (follow-up #1) added — accepts a pre-computed PT (price + current price or price change) and stores it without an outcome or decomposition model. Wired into `company-research-flash` step 26. |
| Migration tests for each skill that moves to `execute` steps | Pin the new flowdef contract | High | ✅ All 4 migrated skills + company-research-flash have tests pinning step counts, execute step ordinals, `mcp:` fields, `on_failure` configs, and `condition:` gates. |
| `on_failure: report` enforcement in `dispatch_with_retry` | Wire skill-use reporting into the executor | High | ✅ Built. `OnFailureConfig` extended with `action: report`. `manifest_id` added to `StepMachine`. `invoke_tool` made `pub(crate)`. Follow-up #2: `resume_text` added to `CascadeOutcome` — the `on_failure.resume` text is now surfaced to the operator via the regulation span payload, not just logged. |
| Curator analysis surface for MCP tool usage patterns | Read `reg.tool.*` spans + skill-use reports; identify patterns | Medium | ✅ The skill-use reporting loop (Phase 2.2) captures the signals. The `EvolveMcpToolSchema` directive (follow-up #4) provides the output channel. The Curator model reads skill-use reports via `curator_memory_recall` and issues directives via `curator_directive`. |
| Curator directive targeting MCP tool schemas | Evolve MCP tool schemas based on skill feedback | Medium | ✅ Built (follow-up #4). `EvolveMcpToolSchema` variant added to `CuratorDirectiveRequest` and `CuratorDirective`. `CyberneticsLoop::apply_directive` persists the evolution request to the regulation ledger. |

## Open questions

### Resolved (Phase 1 + Phase 2 implementation)

1. **`scenario_calibration` scope**: Resolved — the tool computes calibration
   from all resolved forecasts in the store, optionally filtered by `subject`.
   The `overconfidence_score` is a store-level signal (not per-forecaster),
   which is sufficient for the current single-forecaster design. Per-forecaster
   calibration would require adding a `forecaster_id` filter to
   `CalibrationRequest`.

2. **`market_check_resolutions` for equities**: Partially resolved — the
   `forecast_list`, `forecast_get`, `forecast_record`, and `calibrate_forecast`
   tools on `hkask-mcp-companies` provide the forecast-ledger infrastructure.
   `company-research-flash` now calls `forecast_list` (step 0) to read prior
   forecasts. **Remaining gap**: no tool persists a *pre-computed* price target
   from the valuation step (see follow-up question #1 below).

3. **Curator directive scope**: Not yet verified — deferred to Phase 3.

4. **Skill-use report storage**: Resolved — reports are stored as episodic
   h_mems in the curator's memory store with entity `skill_use_issue:<skill_name>`,
   queryable via `curator_memory_recall` and `curator_semantic_search`.

### Follow-up questions (post-Phase 2)

1. **`forecast_persist` tool for pre-computed price targets** ✅ Resolved.
   Built `forecast_persist` on `hkask-mcp-companies` (tool surface 44 → 45).
   Accepts `{symbol, forecast_date, horizon, forecast_price_change?,
   forecast_multiple?, forecast_price?, current_price?, revision_of?,
   forecast_id?}`. When `forecast_price_change` is omitted, computes it from
   `forecast_price` and `current_price` (with explicit zero-guard — no silent
   NaN). Stores a minimal snapshot (`kind: "precomputed_price_target"`) with
   no decomposition model. `forecast_record` was updated to gracefully fall
   back to Brier-only scoring (no decomposition) when the snapshot doesn't
   contain a full `StoredForecast` — logs a `tracing::warn!` so the operator can
   distinguish "no model" from "broken." Wired into `company-research-flash`
   step 26 (after the convergence loop, conditioned on publication). Files:
   `hkask-mcp-companies/src/tools/valuation.rs`, `types.rs`, `fibo.rs`,
   `hkask_mcp_companies.rs`; `company-research-flash.yaml`; manifest test
   updated (26 → 27 steps, 14 → 15 execute steps).

2. **`on_failure` resume text not surfaced to operator** ✅ Resolved. Added
   `resume_text: Option<String>` to `StepMachine` and `CascadeOutcome`.
   `dispatch_with_retry` sets it from `on_failure.resume` when an `on_failure`
   action (halt/escalate/report) triggers `Effect::Exit(Escalated)`. `run`
   threads it into `CascadeOutcome`. The bridge's `execute_skill` includes it
   in the `reg.skill.<id>.outcome` span payload so the operator sees the
   resume instruction alongside `exit_kind: Escalated`. Files:
   `step_machine.rs`, `executor.rs`, `kask_bridge/src/skill_executor.rs`.

3. **Hardcoded `scenario_type` and `time_horizon` in `scenario_score`
   persistence** ✅ Resolved. The triage template (`stage_0_triage.j2`) now
   classifies the forecasting question into `scenario_type` (one of
   `company_update`, `company_analysis`, `emerging_economic`,
   `economic_potential`) and `time_horizon` (one of `tactical`, `strategic`,
   `long_term`). Step 16's `input_mapping` threads `step_1_result.scenario_type`
   and `step_1_result.time_horizon` through with fallbacks to the existing
   defaults. Files: `stage_0_triage.j2`, `superforecasting.yaml`.

4. **Curator directive targeting MCP tool schemas** ✅ Implemented.
   `curator_directive` was an in-process agent tool (`CuratorDirectiveTool` in
   `crates/agent/src/tools/curator_tools.rs`), not an MCP tool. Its
   `CuratorDirectiveRequest` enum had 7 variants, none targeting MCP tool
   schemas. Added a new `EvolveMcpToolSchema` variant to both
   `CuratorDirectiveRequest` (agent crate) and `CuratorDirective`
   (`hkask-types` crate), plus a `SchemaEvolutionType` enum
   (`add_field`/`remove_field`/`rename_field`/`change_type`). The bridge
   (`directive_bridge.rs`) converts the agent-name-based request to the
   `hkask-types` directive. The `CyberneticsLoop::apply_directive` handler
   logs the directive and persists the full evolution request (server,
   tool, evolution type, field, new type, rationale, evidence) to the
   regulation ledger as a `CurationDirectiveAcknowledged` span — so a
   developer or automated migration agent can read the ledger and act on
   the request. The `CURATOR_STATIC_CONTEXT` prompt now advertises the
   `evolve_mcp_tool_schema` variant. Tests: bridge round-trip test,
   regulation-sink persistence test, no-sink-no-panic test. Files:
   `hkask-types/src/curator.rs`, `crates/agent/src/tools/curator_tools.rs`,
   `kask_bridge/src/directive_bridge.rs`,
   `hkask-regulation/src/cybernetics_loop.rs`,
   `crates/agent/src/curator_agent_server.rs`.
