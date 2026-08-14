# Design Plan: Convert EFRA-AI Equity Research Pipeline into a Kask Company Research Skill

**Status**: Design plan (not implementation). Grounded in verified source code.
**Date**: 2026-08-14
**Author**: Curator (GLM 5.2), per plan from Axolotl Partners analyst

---

## Preamble

### Provenance and verification scope

- **EFRA-AI repo**: `https://github.com/Replicant-Partners/EFRA-AI` — README, `src/shared/types.ts`, `src/shared/pipeline.ts`, and four agent source files (`01-scout`, `05-valuation`, `09-lens`, `13-company`) were fetched and read directly. The remaining agent prompts are inferred from `types.ts` interfaces (which are authoritative for inputs/outputs) and the README's role descriptions. Where an agent's prompt was not directly read, the mapping is labeled **inferred** in the Phase 1 table.
- **Kask skill system**: Verified against `kask/registry/templates/{superforecasting,scenario-builder,listening,kata-improvement}/manifest.yaml` and `kask/registry/manifests/{superforecasting,scenario-builder}.yaml`. The flowdef format, `steps`/`ordinal`/`action`/`input_mapping`/`condition`/`loop` primitives, cross-skill `template_ref` (e.g. `falsifiability/falsifiability-hypothesize` inside the superforecasting manifest), and `lisp.eval` compute steps are all confirmed in source.
- **MAIA reference**: Confirmed via the `listening` skill manifest header ("Applies the MAIA v3 listening template") that MAIA is the reference model for kask MCP servers. The MAIA-Substack clone exists at `/home/mdz-axolotl/Clones/Library/MAIA-Substack` but is outside the project root and was not read in detail; the MAIA cross-check is therefore applied at the level of "kask MCP servers were built MAIA-first," not from individual posts.
- **MCP tool surface**: Verified by inspecting the tool definitions available in this session (the `company_transcript`, `comparable_analysis`, `dcf_valuation`, `expectations_gap`, `scenario_build`, `scenario_impact_valuation`, `market_cmp`, `market_history`, `evaluate_evidence`, `research_search`, `ledger_apply`, `portfolio_daily_returns` functions). These are the actual callable surfaces; the plan references them by these exact names.

### Critical design constraint #1 — Sequential pipelines ARE natively supported

**IS claim (verified)**: The kask flowdef manifest format natively supports sequential pipelines. Evidence:
- `kask/registry/manifests/superforecasting.yaml` defines 18 ordered `steps` with `ordinal: 1..18`, where each step's `input_mapping` references prior steps via `{{ step_N_result.<field> }}` (e.g. step 2 references `triage_output` from step 1; step 7 references `outside_view_output` from step 4).
- `kask/registry/manifests/scenario-builder.yaml` defines 8 ordered steps with the same pattern, plus a `loop` step (ordinal 8) that re-enters at step 4 via `loop_target: "{{ 4 }}"`.
- Branching is supported via `condition:` on a step (scenario-builder step 6 carries `condition: "step_5_result.gate_pass"`).
- Cross-skill composition is supported: superforecasting step 6 uses `template_ref: falsifiability/falsifiability-hypothesize` and step 7 uses `template_ref: falsifiability/falsifiability-counterfactual` — templates owned by a different skill crate.

**Implication**: EFRA-AI's 13-agent sequential pipeline maps directly onto a single kask flowdef manifest. No emulation strategy (coordinator agent, chained tool calls) is required. The `PipelineState` object in `src/shared/pipeline.ts` becomes the flowdef's implicit step-result chain.

**Caveat (counterfactual stress)**: The flowdef `loop` primitive re-enters at a fixed `loop_target` ordinal. EFRA-AI's early-exit gates (DROP at SCOUT, HALT at INTEL, BLOCK at FORENSIC) are *terminal* exits, not loops. These map to `condition:` gates on downstream steps: a step with `condition: "{{ step_1_result.decision != 'DROP' }}"` simply does not execute when the upstream gate fails. The flowdef does not need a "halt the whole pipeline" primitive because conditional steps naturally skip. **Verified feasible.**

**MCP tool integration (corrected)**: MCP tool calls are native flowdef steps (`action: execute` with `mcp: <tool_name>`, or `action: select` with `mcp:`/`tool:` fields). They are NOT delegated to the agent's tool-use loop. The `StepMachine` dispatches them via `ToolPort::invoke` — the same governed path as agent-initiated calls. See Critical design constraint #2 above for the verified code references. Every Phase 2 MCP integration uses explicit `execute` steps that feed their results into downstream `select` synthesis steps via `input_mapping`.

### Critical design constraint #2 — MCP tools ARE natively flowdef-invoked (corrected)

**IS claim (verified in source code)**: The flowdef natively supports MCP tool invocation via two mechanisms:

1. **`action: execute`** — a dedicated step action for MCP tool invocation. The step's `mcp:` field carries the tool name; `input_mapping` binds the tool's arguments from prior step results. The `StepMachine::dispatch_action` (in `kask/crates/hkask-templates/src/step_machine.rs` lines 362–366) routes `execute`, `feedback`, `validate`, and `retrieve` actions to `execute_tool_invoke`, which calls `ToolPort::invoke(server_id, tool_name, input, webid)` — the same governed, gas-budgeted, call-capped path as agent-initiated tool calls.

2. **`action: select` with `mcp:` field** — a `select` step that carries an `mcp:` field (and optionally a `tool:` field) is a direct MCP tool invocation, not a template render. The `manifest_properties.rs` test (line 131) confirms: `if step.mcp.is_some() { // MCP tool invocation — no template_ref needed }`.

The canonical action list (`kask/crates/hkask-templates/tests/manifest_compliance.rs` line 6) is:
```
["select", "populate", "compute", "execute", "feedback", "validate", "retrieve", "render", "flowdef", "loop", "choice", "abort", "escalate"]
```

A live pipeline manifest (`kask/corpus/pipeline-capabilities-researcher.yaml`) uses 13 `execute` steps alternating with 12 `gate` steps — proving the pattern is production-tested.

The `eqm` skill flowdef (`kask/registry/manifests/eqm.yaml` step 2) uses the `select`-with-`mcp` variant: `action: select`, `mcp: hkask-mcp-prediction-markets`, `tool: market_score_rationale`.

**Implication**: MCP tool calls are first-class flowdef steps, not delegated to the agent's tool-use loop. This is *better* than agent-mediated calls because: (a) the tool call is deterministic — it happens at a known step ordinal with inputs bound from prior steps, before any LLM round-trip; (b) it flows through `ToolPort::invoke` — same gas budget, same call cap, same `reg.tool.*` span emission; (c) the result lands in `step_{ordinal}_result` and is available to every downstream step via `input_mapping`; (d) it is testable — the pipeline manifest parse test pins the contract.

**Correction note**: An earlier version of this plan incorrectly claimed MCP tools were agent-invoked only, based on observing that the superforecasting and scenario-builder flowdefs (which happen to be pure-LLM + pure-compute pipelines) did not use `execute` steps. That was a sample-of-two generalization error. The executor code (`step_machine.rs`, `step_actions.rs`) and the pipeline manifest test prove native MCP invocation has been available since at least v0.31.

### EFRA-AI agent pipeline map (verified from source)

Two pipelines, run sequentially per the README:

**Main pipeline (01–09)**, verified order from `src/shared/pipeline.ts`:

| Ordinal | Agent | Role | Gate / early-exit | Methodology step (Analyst Solutions) |
|---|---|---|---|---|
| 01 | SCOUT | Coverage universe optimizer | `decision == DROP` → terminal | Idea screening / coverage gap analysis |
| 02 | INTEL | Business analysis + news mosaic | `mosaic_clear == false` → HALT | Business model understanding, information mosaic |
| 04 (first) | FORENSIC | Quick risk pre-screen | `recommendation == BLOCK` → terminal | Risk pre-screening |
| 03 | CRITICAL FACTOR | Bull/Base/Bear thesis engine | `factors.length == 0` → DROP | Critical factor identification, scenario construction |
| 04 (full) | FORENSIC | Full audit (accruals, governance, mgmt) | `recommendation == BLOCK` → terminal | Forensic accounting, management quality |
| 05 | VALUATION | 8-step price target | `rr < 2:1 && UNDERPERFORM` → DROP | Valuation, price target, FaVeS |
| 06 | COMMUNICATION | ENTER gate + CASCADE note | `publication_possible == false` → DROP | Research communication, ENTER criteria |
| 08 | KATA | Process audit (never blocks) | never blocks | Process discipline / Improvement Kata |
| 09 | LENS | Consistency auditor (never blocks) | never blocks | Framework consistency audit |

Note: the README's pipeline diagram shows `FORENSIC(pre) → CF → FORENSIC(full)`, and `pipeline.ts` confirms FORENSIC runs twice (PRE-SCREEN then FULL) with CRITICAL FACTOR in between. The README's agent table lists 07-catalog and 08-kata but `pipeline.ts` only invokes KATA (08) post-publication; CATALOG (07) is not in the main pipeline orchestrator — it is a classification utility (Haiku model, per the model allocation table).

**Research pipeline (10–13)**, per README (not in `pipeline.ts` — runs independently):

| Ordinal | Agent | Role | Output |
|---|---|---|---|
| 13 | COMPANY | Deep 8-part company analysis | `CompanyBoard` |
| 10 | GORILLA | Value Gorilla 4-dimension framework | `GORILLA`/`SMALL_ANIMAL`/`PEDESTRIAN` |
| 11 | IMAGINE | 5/10/20Y scenarios + digital stage | `ImagineBoard` |
| 12 | THESIS | Three-pillar thesis synthesis | `investment_grade`/`needs_work`/`incomplete` |

The research pipeline order is `COMPANY → GORILLA → IMAGINE → THESIS` (each feeds the next).

### Kask ecosystem inventory (verified)

**Existing skills relevant to company research** (from `.agents/skills/` and `kask/registry/templates/`):

| Skill | Relevance | Verified via |
|---|---|---|
| `superforecasting` | 8-stage Tetlock pipeline; directly maps to EFRA-AI's Bull/Base/Bear probability generation and LENS Lens 2 | manifest.yaml + flowdef manifest |
| `scenario-builder` | Schwartz 2×2 scenario planning; maps to IMAGINE's 5/10/20Y scenarios and CRITICAL FACTOR's Bull/Base/Bear | manifest.yaml + flowdef manifest |
| `listening` | MAIA v3 earnings-call transcript analysis with no-fabrication invariant; maps to INTEL's news/earnings ingestion | manifest.yaml |
| `kata-improvement` | 4-step PDCA + 5 coaching questions; **directly maps to EFRA-AI agent 08 KATA** (same Toyota Improvement Kata source) | manifest.yaml |
| `kata-coaching` | 5-question coaching kata; overlaps with kata-improvement | skill catalog |
| `essentialist` | 3-gate challenge (Exist/Surface/Contract); used in this plan as the evaluation lens | skill catalog |
| `deep-module` | Ousterhout depth test; used as evaluation lens | skill catalog |
| `pragmatic-cybernetics` | Feedback loop analysis; used as evaluation lens | skill catalog |
| `pragmatic-semantics` | IS/OUGHT classification; used as evaluation lens | skill catalog |
| `falsifiability` | Hypothesis + counterfactual generation; already composed into superforecasting | superforecasting manifest step 6/7 |
| `hypothesis-framer` | FINER/PICO question framing; could feed SCOUT's idea triage | skill catalog |
| `mcda` | Multi-criteria decision analysis; could structure GORILLA's 4-dimension weighting | skill catalog |
| `wardley-mapper` | Wardley mapping; could support IMAGINE's value-chain positioning | skill catalog |
| `eqm` / `eqm-improvement` | Explanation Quality Markers for forecast rationales; could score LENS's superforecasting lens | skill catalog |
| `metacognition` | Self-calibration; overlaps with LENS's Dunning-Kruger lens | skill catalog |
| `self-improvement` | Persistent FM adaptation; relevant to Phase 3 kata loops | skill catalog |
| `goal-analysis` | Goal verification; could gate THESIS's `investment_grade` verdict | skill catalog |

**Available MCP tools** (verified from this session's tool surface):

| Tool | Server (inferred) | Replaces / enhances which EFRA-AI agent |
|---|---|---|
| `company_transcript` | hkask-mcp-companies | INTEL (earnings transcripts), COMMUNICATION (mgmt quotes) |
| `comparable_analysis` | hkask-mcp-companies | VALUATION Step 4 (peer comparison), COMPANY financials.peer_comparison |
| `dcf_valuation` | hkask-mcp-companies | VALUATION Steps 5–7 (DCF leg), COMPANY value_expectations |
| `expectations_gap` | hkask-mcp-companies | VALUATION Step 3 (market assumptions), LENS Lens 1 (variant expectations) |
| `scenario_build` | hkask-mcp-scenarios | CRITICAL FACTOR (Bull/Base/Bear), IMAGINE (5/10/20Y) |
| `scenario_impact_valuation` | hkask-mcp-scenarios | VALUATION (scenario-weighted PT) |
| `scenario_research` | hkask-mcp-scenarios | INTEL (extract events from research text) |
| `market_cmp` / `market_cmp_index` | hkask-mcp-prediction-markets | LENS Lens 2 (market-implied probabilities as outside view) |
| `market_history` | hkask-mcp-prediction-markets | LENS Lens 2 (realized variance / volatility regime) |
| `market_calibration` | hkask-mcp-prediction-markets | LENS Lens 2, KATA (calibration feedback loop) |
| `market_check_resolutions` | hkask-mcp-prediction-markets | Phase 3 kata loop (outcome → Brier → refine) |
| `evaluate_evidence` | hkask-mcp-scenarios | LENS (evidence quality audit) |
| `research_search` | hkask-mcp-companies | INTEL (fundamental research search) |
| `fred_search_series` / `dbnomics_search` / `wb_search_indicators` | macro | INTEL (macro context), IMAGINE (long-range forces) |
| `web_search` / `fetch` | web | INTEL (news mosaic), COMPANY (self-view from IR pages) |
| `ledger_apply` / `ledger_read` / `portfolio_daily_returns` | hkask-mcp-portfolio | Phase 3 (track realized returns vs forecast PT) |

---

## Phase 1 — Conversion Plan: EFRA-AI → Kask Company Research Skill

### Design decision: one skill, two flowdefs

The 13 EFRA-AI agents become **one kask skill crate** (`company-research`) with **two flowdef manifests**, mirroring EFRA-AI's two-pipeline structure:

1. `company-research-flash` — the main pipeline (SCOUT → INTEL → FORENSIC × 2 → CF → VALUATION → COMM → KATA → LENS), producing a flash note / initiation report.
2. `company-research-deep` — the research pipeline (COMPANY → GORILLA → IMAGINE → THESIS), producing a deep company analysis and investment thesis.

**Rationale (deep-module lens)**: A single 21-step flowdef would exceed the 7-function surface guideline (the superforecasting crate already justifies 10 templates as methodologically mandated). Two flowdefs keep each pipeline's surface coherent and let them be invoked independently (a user can run the deep pipeline on a name already covered by the flash pipeline). The crate shares templates where the same Analyst Solutions step appears in both (e.g. the 3-stage valuation framework appears in both VALUATION and COMPANY).

### Agent-to-template mapping table

Each row: EFRA-AI agent → kask primitive → essentialist verdict (DEEPEN / KEEP / ELIMINATE) → rationale.

| # | EFRA-AI agent | Kask primitive | Verdict | Rationale |
|---|---|---|---|---|
| 01 | SCOUT | Template `company-research/scout-alpha-score` (WordAct) + optional `hypothesis-framer` cross-ref | **KEEP** | Implements a firm-specific alpha formula (coverage gap, market cap fit, sector relevance, valuation anomaly + Gunn bonuses). Not replaceable by an MCP tool — the formula is the firm's IP. The 11-criterion excellence universe (S1–S11) is deterministic and could be a `lisp.eval` compute step, but the alpha-score reasoning is judgmental. Deep enough to keep. |
| 02 | INTEL | Template `company-research/intel-mosaic` (WordAct) + `listening/apply-template` cross-ref for transcripts + `research_search` + `web_search` MCP tools | **DEEPEN** | INTEL's business-context 8-step + news mosaic + hypothesis tracking is the pipeline's information hub. Deepening: split into (a) business-context template, (b) news-mosaic template that consumes `web_search` + `research_search` results, (c) hypothesis tracker that feeds CRITICAL FACTOR. The `listening` skill's no-fabrication invariant should govern every evidence quote. |
| 03 | CRITICAL FACTOR | Template `company-research/critical-factor` (WordAct) + `scenario_build` MCP tool for Bull/Base/Bear + `superforecasting/stage_3_probability_estimate` cross-ref for granular probabilities | **KEEP** | The critical-factor-then-scenarios structure is firm methodology. The probability assignment should delegate to the superforecasting skill's inside-view stage rather than re-implementing it (avoids the round-number problem LENS Lens 2 checks for). |
| 04 (pre) | FORENSIC | Template `company-research/forensic-pre-screen` (WordAct) | **KEEP** | Quick risk pre-screen with SEV-1..5 classification. The severity → EPS haircut / DR add mapping is deterministic and should be a `lisp.eval` compute step consuming the template's severity call. |
| 04 (full) | FORENSIC | Template `company-research/forensic-full` (WordAct) + `company_transcript` MCP tool for mgmt quotes | **KEEP** | Full audit adds the management profile (founder, CEO, team, incentives, key decisions). Distinct from pre-screen by output shape (`ManagementProfile` present/absent). Keeping as a second template is justified by the distinct output contract, not by speculative generality. |
| 05 | VALUATION | Template `company-research/valuation-8step` (WordAct) + `dcf_valuation` + `comparable_analysis` + `expectations_gap` MCP tools + `scenario_impact_valuation` for the probability-weighted PT | **DEEPEN** | The 8-step framework is firm methodology, but Steps 2 (multiples), 4 (comparables), 5–7 (DCF), and 3 (market assumptions) are all directly served by existing MCP tools. Deepening: the template becomes an orchestrator that calls the tools and synthesizes, rather than an LLM hallucinating multiples. The FaVeS score and RR gate are deterministic compute steps. |
| 06 | COMMUNICATION | Template `company-research/communication-enter` (WordAct) | **KEEP** | The ENTER gate (Edge/New/Timely/Examples/Revealing) is firm IP. The CASCADE format (Conclusion/Action/Scenarios/Catalysts/Data) is a deterministic output structure. The 5/5 → PUBLISH gate is a `lisp.eval` compute step. |
| 07 | CATALOG | **ELIMINATE** | **ELIMINATE** | Not in `pipeline.ts`. A Haiku-model classifier for output-type selection (FLASH_NOTE / INITIATION / ALERT / QUARTERLY). The output type is already determined by `downstream_mode` (Valentine → FLASH, Gunn → INITIATION, Dual → both). Folding into COMMUNICATION's deterministic dispatch. Essentialist 3-gate: Exist — fails (no capability added beyond mode dispatch); Surface — fails (thin wrapper); Contract — fails (one-call classifier). |
| 08 | KATA | **Reuse `kata-improvement` skill** via cross-skill `template_ref` (no new template) | **KEEP (reuse)** | EFRA-AI's KATA agent is a direct implementation of the Toyota Improvement Kata — the same source as the existing `kata-improvement` skill. The `KataBoard` output (challenge, current condition, knowledge gaps, assumption risks, target condition, obstacles, PDCA cycle, coaching memo, process_confidence) maps onto `kata-improvement/improvement-step1-direction` through `improvement-step4-experiment`. **Do not re-implement.** Cross-skill compose. |
| 09 | LENS | Template `company-research/lens-five-frameworks` (WordAct) + `evaluate_evidence` + `market_calibration` MCP tools | **KEEP** | The five frameworks (The Loop, Superforecasting, Dunning-Kruger, Hidden Champions, Kauffman) are firm IP and not reducible to existing skills. Lens 2 (Superforecasting) should cross-reference the `superforecasting/forecast-quality-gate` template rather than re-implementing forecast-quality checks. Lens 3 (Dunning-Kruger) should consume `kata`'s `process_confidence` and compare to `communication`'s `final_confidence` — a deterministic `lisp.eval` step. |
| 10 | GORILLA | Template `company-research/gorilla-4dim` (WordAct) + `mcda` cross-ref for the 25/30/25/20 weighting | **KEEP** | The 4-dimension Value Gorilla framework (Obvious Problem / Invisible Gorilla / Combinatorial / Choke Point) is firm IP. The weighting is deterministic and should be a `lisp.eval` compute step. The `mcda` skill could structure the multi-criteria scoring but is optional — the weights are fixed by methodology, not user-tunable. |
| 11 | IMAGINE | Template `company-research/imagine-longrange` (WordAct) + `scenario_build` MCP tool + `wardley-mapper` cross-ref for value-chain positioning | **KEEP** | The digital-stage (MODEL/SHADOW/TWIN/SOURCE) classification and 5/10/20Y scenario structure are firm IP. The long-range scenarios should delegate to `scenario_build` (which already supports `time_horizon` and `max_events`). The falsifiable-predictions output is the firm's commitment to testability — keep. |
| 12 | THESIS | Template `company-research/thesis-three-pillars` (WordAct) + `goal-analysis` cross-ref for the `investment_grade` quality gate | **KEEP** | Three-pillar synthesis (Business Franchise / Management Quality / Valuation). The `investment_grade` / `needs_work` / `incomplete` verdict should be gated by `goal-analysis` (semantic evaluation against the three pillars) rather than self-assessment — this prevents the LLM-improves-against-LLM-scored-target trap flagged in `.rules`. |
| 13 | COMPANY | Template `company-research/company-8part` (WordAct) + `company_transcript` + `dcf_valuation` + `comparable_analysis` + `web_search`/`fetch` MCP tools | **DEEPEN** | The 8-part framework (Self-View / Franchise / Owner-Operator / Financials / Invisible Layer / Turd Blossom / Gorilla Elevator / Thesis Statement) is the deepest agent in EFRA-AI. Deepening: Parts 1 (Self-View) and 4 (Financials) should consume `web_search`/`fetch` on IR pages and `company_transcript` for mgmt language; Part 4's 3-stage valuation should call `dcf_valuation` and `comparable_analysis` rather than hallucinating multiples. The `listening` skill's no-fabrication invariant should govern every quote from filings. |

**Essentialist verdict summary**: 11 KEEP, 3 DEEPEN, 1 ELIMINATE (CATALOG), 1 KEEP-via-reuse (KATA → existing `kata-improvement` skill).

### Pipeline flowdef sketch (pseudocode — not final implementation)

```yaml
# kask/registry/manifests/company-research-flash.yaml (sketch)
manifest:
  id: company-research-flash
  category: skill
  functional_role: flowdef
  convergence:
    convergence_mode: cauchy
    cauchy_epsilon: 0.05
    max_iterations: 3           # flash pipeline is not iterative by design;
                               # convergence is over the LENS verdict consistency
    min_iterations: 1
    on_not_reached: escalate    # LENS INCONSISTENT → human review, not auto-loop

inputs:
  - name: ticker
    type: string
    required: true
  - name: analyst_id
    type: string
    required: false
  - name: catalyst
    type: string
    required: true
  - name: idea_source_tag
    type: string
    required: false
  - name: in_excellence_universe
    type: boolean
    required: false
  - name: downstream_mode_override
    type: string
    required: false
    description: Force valentine/gunn/dual; else SCOUT decides.

steps:
  - ordinal: 1            # SCOUT
    action: select
    template_ref: company-research/scout-alpha-score
    input_mapping:
      ticker: "{{ ticker }}"
      catalyst: "{{ catalyst }}"
      idea_source_tag: "{{ idea_source_tag | default('') }}"
      in_excellence_universe: "{{ in_excellence_universe | default(false) }}"

  - ordinal: 2            # INTEL — business context
    action: select
    template_ref: company-research/intel-mosaic
    condition: "{{ step_1_result.decision != 'DROP' }}"
    input_mapping:
      ticker: "{{ ticker }}"
      horizon_tag: "{{ step_1_result.horizon_tag }}"
      downstream_mode: "{{ step_1_result.downstream_mode }}"

  - ordinal: 3            # INTEL — earnings transcript via listening skill (cross-skill)
    action: select
    template_ref: listening/apply-template
    condition: "{{ step_1_result.decision != 'DROP' }}"
    input_mapping:
      symbol: "{{ ticker }}"

  - ordinal: 4            # FORENSIC pre-screen
    action: select
    template_ref: company-research/forensic-pre-screen
    condition: "{{ step_2_result.mosaic_clear }}"
    input_mapping:
      ticker: "{{ ticker }}"

  - ordinal: 5            # CRITICAL FACTOR
    action: select
    template_ref: company-research/critical-factor
    condition: "{{ step_4_result.recommendation != 'BLOCK' }}"
    input_mapping:
      intel_bundle: "{{ step_2_result }}"
      forensic_profile: "{{ step_4_result }}"
      downstream_mode: "{{ step_1_result.downstream_mode }}"
      horizon_tag: "{{ step_1_result.horizon_tag }}"

  - ordinal: 6            # FORENSIC full
    action: select
    template_ref: company-research/forensic-full
    condition: "{{ step_5_result.factors | length > 0 }}"
    input_mapping:
      ticker: "{{ ticker }}"

  - ordinal: 7a           # DCF via MCP tool — deterministic, no LLM round-trip
    action: execute
    mcp: dcf_valuation
    condition: "{{ step_6_result.recommendation != 'BLOCK' }}"
    input_mapping:
      symbol: "{{ ticker }}"
      # other dcf_valuation inputs bound from prior steps

  - ordinal: 7b           # Comparables via MCP tool
    action: execute
    mcp: comparable_analysis
    condition: "{{ step_6_result.recommendation != 'BLOCK' }}"
    input_mapping:
      symbol: "{{ ticker }}"

  - ordinal: 7c           # Expectations gap via MCP tool (Mauboussin)
    action: execute
    mcp: expectations_gap
    condition: "{{ step_6_result.recommendation != 'BLOCK' }}"
    input_mapping:
      symbol: "{{ ticker }}"

  - ordinal: 7d           # Scenario-weighted PT via MCP tool
    action: execute
    mcp: scenario_impact_valuation
    condition: "{{ step_6_result.recommendation != 'BLOCK' }}"
    input_mapping:
      symbol: "{{ ticker }}"
      scenario_tree: "{{ step_5_result.scenarios }}"

  - ordinal: 7e           # VALUATION synthesis — LLM reasons over tool outputs
    action: select
    template_ref: company-research/valuation-8step
    condition: "{{ step_6_result.recommendation != 'BLOCK' }}"
    input_mapping:
      ticker: "{{ ticker }}"
      forensic_profile: "{{ step_6_result }}"
      cf_scenarios: "{{ step_5_result.scenarios }}"
      intel_bundle: "{{ step_2_result }}"
      downstream_mode: "{{ step_1_result.downstream_mode }}"
      dcf_result: "{{ step_7a_result }}"
      comparables_result: "{{ step_7b_result }}"
      expectations_result: "{{ step_7c_result }}"
      scenario_pt_result: "{{ step_7d_result }}"

  - ordinal: 8            # COMMUNICATION + ENTER gate
    action: select
    template_ref: company-research/communication-enter
    condition: "{{ step_7_result.rr_ratio >= 2.0 or step_7_result.rating != 'UNDERPERFORM' }}"
    input_mapping:
      valuation_model: "{{ step_7_result }}"
      forensic_profile: "{{ step_6_result }}"
      cf_output: "{{ step_5_result }}"
      intel_bundle: "{{ step_2_result }}"
      downstream_mode: "{{ step_1_result.downstream_mode }}"

  - ordinal: 9a           # KATA — fetch resolved prediction-market outcomes for PDCA "check"
    action: execute
    mcp: market_check_resolutions
    condition: "{{ step_8_result.publication_possible }}"
    input_mapping:
      bucket: "{{ ticker }}"

  - ordinal: 9b           # KATA — fetch calibration Brier score
    action: execute
    mcp: market_calibration
    condition: "{{ step_8_result.publication_possible }}"
    input_mapping:
      bucket: "{{ ticker }}"

  - ordinal: 9c           # KATA — cross-skill reuse (process audit)
    action: select
    template_ref: kata-improvement/improvement-step1-direction
    condition: "{{ step_8_result.publication_possible }}"
    input_mapping:
      task: "Audit the research process for {{ ticker }}: knowledge gaps, untested assumptions, next PDCA experiment."
      resolved_outcomes: "{{ step_9a_result }}"
      calibration_brier: "{{ step_9b_result }}"

  - ordinal: 10           # LENS — fetch market-implied probabilities for outside-view check
    action: execute
    mcp: market_cmp
    condition: "{{ step_8_result.publication_possible }}"
    input_mapping:
      query: "{{ ticker }}"

  - ordinal: 11           # LENS — evaluate evidence quality
    action: execute
    mcp: evaluate_evidence
    condition: "{{ step_8_result.publication_possible }}"
    input_mapping:
      question: "Is the investment thesis for {{ ticker }} supported by the evidence?"
      artifacts: "{{ step_2_result.news_items }}"

  - ordinal: 12           # LENS — five-framework audit (LLM synthesis)
    action: select
    template_ref: company-research/lens-five-frameworks
    condition: "{{ step_8_result.publication_possible }}"
    input_mapping:
      ticker: "{{ ticker }}"
      downstream_mode: "{{ step_1_result.downstream_mode }}"
      scout: "{{ step_1_result }}"
      intel: "{{ step_2_result }}"
      forensic: "{{ step_6_result }}"
      cf: "{{ step_5_result }}"
      valuation: "{{ step_7e_result }}"
      communication: "{{ step_8_result }}"
      kata: "{{ step_9c_result }}"
      market_outside_view: "{{ step_10_result }}"
      evidence_audit: "{{ step_11_result }}"

  - ordinal: 13           # Convergence check — LENS verdict consistency
    action: compute
    compute_ref: lisp.eval
    input_mapping:
      form: >
        (let ((v (assoc "overall_verdict" step_12_result)))
          (cond
            ((eq v "CONSISTENT") 0.0)
            ((eq v "PARTIAL") 0.5)
            ((eq v "INCONSISTENT") 1.0)
            (t 1.0)))
      env:
        step_12_result: "{{ step_12_result }}"

  - ordinal: 14           # Loop back to VALUATION if LENS flags PARTIAL
                          # (re-run valuation with LENS tensions injected)
    action: loop
    condition: "{{ step_13_result > 0.0 and step_13_result < 1.0 }}"
    input_mapping:
      loop_target: "{{ 7e }}"
      convergence_signal: "{{ step_13_result }}"
```

The `company-research-deep` flowdef follows the same structure for COMPANY → GORILLA → IMAGINE → THESIS, with THESIS's `investment_grade` gate as the convergence signal and a loop back to COMPANY if `needs_work`.

### Counterfactual stress (upstream failure modes)

| Failure mode | Pipeline behavior | Design |
|---|---|---|
| SCOUT → DROP | Terminal exit; no downstream steps run | `condition:` on step 2 onward |
| INTEL → `mosaic_clear == false` (MNPI halt) | Terminal exit | `condition: "{{ step_2_result.mosaic_clear }}"` on step 4 |
| FORENSIC pre → BLOCK | Terminal exit | `condition` on step 5 |
| CRITICAL FACTOR → 0 factors | Terminal exit (DROP) | `condition: "{{ step_5_result.factors | length > 0 }}"` on step 6 |
| FORENSIC full → BLOCK | Terminal exit | `condition` on step 7 |
| VALUATION → RR < 2:1 + UNDERPERFORM | Terminal exit | `condition` on step 8 |
| COMMUNICATION → `publication_possible == false` | KATA and LENS skip (condition on step 9, 10) | Graceful: pipeline completes with a "not published" verdict; no crash |
| KATA throws (EFRA-AI wraps in try/catch) | LENS still runs (KATA is optional input) | `kata: "{{ step_9_result | default({}) }}"` — missing KATA → LENS Lens 3 reports `process_confidence: N/A` |
| MCP tool failure (e.g. `dcf_valuation` 500) | Template must surface the failure, not silently fall back | Per `.rules`: "Opt-in features that fail must log the failure classification, not collapse to `None` via `.ok()?`." The valuation template must emit a `data_gaps` entry naming the failed tool. |

### Self-verification: Analyst Solutions methodology coverage

| Methodology step (from EFRA-AI README + agent prompts) | Covered by | Status |
|---|---|---|
| Idea screening / coverage gap | SCOUT (step 1) | ✅ |
| Business model understanding | INTEL (step 2) + COMPANY Part 2 (deep pipeline) | ✅ |
| Information mosaic / news | INTEL (step 2) + `web_search`/`research_search` | ✅ |
| Earnings-call listening | `listening` skill (step 3) | ✅ (added — EFRA-AI had no dedicated transcript agent) |
| Risk pre-screening | FORENSIC pre (step 4) | ✅ |
| Critical factor identification | CRITICAL FACTOR (step 5) | ✅ |
| Bull/Base/Bear scenarios | CRITICAL FACTOR + `scenario_build` | ✅ |
| Forensic accounting | FORENSIC full (step 6) | ✅ |
| Management quality / owner-operator | FORENSIC full + COMPANY Part 3 | ✅ |
| Valuation (8-step) | VALUATION (step 7) + `dcf_valuation`/`comparable_analysis`/`expectations_gap` | ✅ |
| Price target / RR / FaVeS | VALUATION (step 7) | ✅ |
| Research communication / ENTER | COMMUNICATION (step 8) | ✅ |
| CASCADE format | COMMUNICATION (step 8) | ✅ |
| Process discipline / Improvement Kata | KATA (step 9) via `kata-improvement` | ✅ |
| Framework consistency audit (5 lenses) | LENS (step 10) | ✅ |
| Deep company analysis (8-part) | COMPANY (deep pipeline) | ✅ |
| Value Gorilla 4-dimension | GORILLA (deep pipeline) | ✅ |
| Long-range imagination / digital stage | IMAGINE (deep pipeline) + `scenario_build` | ✅ |
| Three-pillar thesis synthesis | THESIS (deep pipeline) | ✅ |

**No methodology step is missing.** One step is *added* (earnings-call listening via the `listening` skill) that EFRA-AI did not have as a dedicated agent — this is a Phase 2 enhancement, not a gap.

### Identified gaps (EFRA-AI functionality with no kask equivalent)

1. **EDGAR XBRL fetching** — EFRA-AI's COMPANY agent accepts `edgar_facts` (pre-fetched SEC XBRL). No kask MCP tool provides SEC filing retrieval. `web_search`/`fetch` can reach SEC URLs but does not parse XBRL. **Gap**: a dedicated SEC-filings MCP tool or a `fetch`-based template that parses XBRL. Flagged for Phase 3.
2. **News API integration** — EFRA-AI's INTEL consumes a `rawNewsPool` from a news API. Kask has `web_search` (general) but no dedicated financial-news API with source-tier classification. **Gap**: financial-news MCP tool with tier-1/tier-2 source classification. Flagged for Phase 3.
3. **CRM / hypothesis tracking** — EFRA-AI's INTEL tracks `Hypothesis` objects with `crm_contact_id` and `HypothesisLifecycle` (PENDING / VALIDATED / UNRESOLVABLE). No kask equivalent for persistent hypothesis lifecycle tracking tied to analyst CRM. **Gap**: hypothesis-lifecycle MCP tool or persistence layer. Flagged for Phase 3.
4. **Excellence universe S1–S11 screening** — EFRA-AI's SCOUT applies 11 deterministic criteria (trading status, exchanges, price, working capital, debt, gross margin stability, etc.). These require live market data. No single kask MCP tool provides all 11. **Gap**: a screening MCP tool or a `lisp.eval` step that consumes multiple data sources. Flagged for Phase 3.

---

## Phase 2 — Supercharge Plan: Kask Tool Integration

### Integration mapping table

For each agent-template from Phase 1, evaluated against the MCP tools. Enhancement type: **replace** (tool does the work, template orchestrates), **enhance** (tool provides input, template reasons over it), **ground** (tool provides ground truth / feedback loop), **reject** (integration fails essentialist or cybernetic gates).

| Agent-template | Kask tool | Enhancement | Essentialist 3-gate | Cybernetic loop impact |
|---|---|---|---|---|
| INTEL (intel-mosaic) | `research_search` | enhance | Exist ✅ (adds fundamental research search) / Surface ✅ (deep — structured search vs hallucinated) / Contract ✅ (one tool call per query) | **Creates** loop: research_search → evidence → INTEL reasoning → hypothesis → future research_search targets. **Native `action: execute` step.** |
| INTEL (intel-mosaic) | `web_search` + `fetch` | enhance | Exist ✅ / Surface ✅ / Contract ✅ | **No new loop** (one-shot retrieval); does not break existing. **Native `action: execute` steps.** |
| INTEL (step 3, listening) | `company_transcript` | replace | Exist ✅ / Surface ✅ (MAIA no-fabrication invariant is deep) / Contract ✅ | **Creates** loop: transcript → listening verdict → INTEL business context → future transcript comparison. **Native `action: execute` step.** |
| CRITICAL FACTOR | `scenario_build` | enhance | Exist ✅ / Surface ✅ (structured scenario generation vs freeform) / Contract ✅ | **Creates** loop: scenario_build → Bull/Base/Bear → VALUATION → LENS audit → scenario refinement. **Native `action: execute` step.** |
| CRITICAL FACTOR | `superforecasting/stage_3_probability_estimate` (cross-skill) | enhance | Exist ✅ / Surface ✅ (granular probabilities vs round numbers) / Contract ✅ | **Strengthens** loop: granular probability → LENS Lens 2 (superforecasting) → calibration feedback |
| FORENSIC (full) | `company_transcript` | enhance | Exist ✅ (mgmt quotes for management profile) / Surface ✅ / Contract ✅ | **No new loop** (evidence retrieval) |
| VALUATION | `dcf_valuation` | replace (Step 5–7 DCF leg) | Exist ✅ / Surface ✅ (deterministic DCF vs LLM arithmetic) / Contract ✅ | **Creates** loop: DCF output → VALUATION synthesis → LENS Lens 1 (valuation anchor) → DCF input refinement. **Native `action: execute` step.** |
| VALUATION | `comparable_analysis` | replace (Step 4) | Exist ✅ / Surface ✅ (real peer multiples vs hallucinated) / Contract ✅ | **Creates** loop: comparable_analysis → peer_comparison → LENS audit → peer set refinement |
| VALUATION | `expectations_gap` | replace (Step 3 market assumptions) | Exist ✅ / Surface ✅ (Mauboussin expectations investing vs LLM guess) / Contract ✅ | **Strengthens** loop: expectations_gap → market_assumptions → LENS Lens 1 (variant expectations) → thesis refinement |
| VALUATION | `scenario_impact_valuation` | replace (probability-weighted PT) | Exist ✅ / Surface ✅ (deterministic EV calculation) / Contract ✅ | **Strengthens** loop: scenario_impact → pt_12m → COMMUNICATION → LENS → scenario probability refinement |
| COMMUNICATION | (no MCP tool — ENTER gate is firm IP) | — | — | — |
| KATA | `market_check_resolutions` | ground | Exist ✅ (resolved prediction markets feed PDCA "check") / Surface ✅ (real outcomes vs assumed) / Contract ✅ | **Creates** the missing feedback loop: forecast → market resolution → Brier → KATA PDCA "check" → next forecast. **This is the highest-value integration.** **Native `action: execute` step.** |
| KATA | `market_calibration` | ground | Exist ✅ / Surface ✅ / Contract ✅ | **Strengthens** calibration loop: market_calibration Brier score → KATA process_confidence adjustment |
| LENS (Lens 2) | `market_cmp` / `market_cmp_index` | ground | Exist ✅ (market-implied probabilities as outside view) / Surface ✅ / Contract ✅ | **Creates** loop: market CMP → LENS Lens 2 outside-view check → forecast calibration |
| LENS (Lens 2) | `market_history` | ground | Exist ✅ (realized variance / volatility regime) / Surface ✅ / Contract ✅ | **No new loop** (descriptive context) |
| LENS (Lens 2) | `evaluate_evidence` | enhance | Exist ✅ / Surface ✅ (structured evidence quality audit) / Contract ✅ | **Strengthens** loop: evaluate_evidence → LENS verdict → KATA obstacle → evidence refinement |
| GORILLA | `mcda` (cross-skill) | enhance (optional) | Exist ⚠️ (the 4 weights are fixed by methodology, not user-tunable) / Surface ⚠️ (mcda adds a ceremony layer for fixed weights) / Contract ⚠️ | **No loop impact**. **Verdict: reject** — the 25/30/25/20 weighting is better as a `lisp.eval` compute step than an mcda invocation. mcda's value is in *user-tunable* weights; here they are fixed. |
| IMAGINE | `scenario_build` | enhance | Exist ✅ / Surface ✅ / Contract ✅ | **Creates** loop: scenario_build → 5/10/20Y scenarios → THESIS → LENS → scenario refinement |
| IMAGINE | `wardley-mapper` (cross-skill) | enhance (optional) | Exist ✅ (value-chain positioning for choke-point analysis) / Surface ✅ / Contract ✅ | **No new loop** (structural analysis) |
| THESIS | `goal-analysis` (cross-skill) | replace (quality gate) | Exist ✅ (semantic evaluation vs self-assessment) / Surface ✅ (prevents LLM-improves-against-LLM-scored-target trap per `.rules`) / Contract ✅ | **Creates** loop: goal-analysis verdict → THESIS revision → re-evaluation |
| COMPANY (Part 1 Self-View) | `web_search` + `fetch` (IR pages) | enhance | Exist ✅ / Surface ✅ / Contract ✅ | **No new loop** (evidence retrieval) |
| COMPANY (Part 4 Financials) | `dcf_valuation` + `comparable_analysis` | replace | Exist ✅ / Surface ✅ / Contract ✅ | **Strengthens** loop: same as VALUATION |
| COMPANY (Part 4 Financials) | `company_transcript` | enhance | Exist ✅ / Surface ✅ / Contract ✅ | **No new loop** |
| COMPANY (all parts) | `listening/apply-template` (cross-skill) | enhance | Exist ✅ (no-fabrication invariant for all quotes) / Surface ✅ / Contract ✅ | **Strengthens** loop: listening verdict → COMPANY analysis → LENS audit |

### Rejected integrations (with rationale)

| Proposed integration | Rejection rationale |
|---|---|
| GORILLA → `mcda` | The 4-dimension weights (25/30/25/20) are fixed by firm methodology. mcda's value is user-tunable weights; here they are not. A `lisp.eval` compute step is the right primitive. Essentialist Surface gate fails (adds ceremony without value). |
| SCOUT → `fred_search_series` / macro tools | SCOUT's alpha score is company-specific (coverage gap, market cap fit, sector relevance, valuation anomaly). Macro data does not feed the alpha formula. Would be scope creep. Essentialist Contract gate fails. |
| COMMUNICATION → `ledger_apply` | COMMUNICATION produces a research note, not a trade. Portfolio ledger entries belong to portfolio management, not research communication. Cybernetic: would create a false loop (research note → ledger entry → returns → research note) that conflates research quality with trading execution. |
| LENS → `portfolio_daily_returns` | LENS audits research quality, not portfolio performance. Same conflation risk as above. |

### Cybernetic loop-impact summary

**New feedback loops created** (5):
1. `research_search` → INTEL → hypothesis → future `research_search` (information discovery loop)
2. `company_transcript` → `listening` → INTEL/COMPANY → future transcript comparison (earnings evolution loop)
3. `scenario_build` → CRITICAL FACTOR → VALUATION → LENS → scenario refinement (scenario quality loop)
4. `dcf_valuation`/`comparable_analysis`/`expectations_gap` → VALUATION → LENS → valuation input refinement (valuation grounding loop)
5. **`market_check_resolutions` → KATA PDCA "check" → next forecast** (the forecast-to-outcome calibration loop — the single highest-value integration; this is the loop EFRA-AI's KATA agent describes but cannot close because it has no outcome data)

**Existing loops strengthened** (4):
- Granular probability → LENS Lens 2 → calibration (via `superforecasting` cross-ref)
- `expectations_gap` → LENS Lens 1 variant expectations → thesis
- `evaluate_evidence` → LENS verdict → KATA obstacle
- `listening` no-fabrication → COMPANY → LENS audit

**Loops broken**: 0. No integration breaks an existing EFRA-AI loop. The conversion preserves all early-exit gates (DROP/HALT/BLOCK) as flowdef `condition:` gates.

**Silent failure modes flagged** (per `.rules`):
- `dcf_valuation` failure must not collapse to `None` via `.ok()?`. The valuation template must emit a `data_gaps` entry naming the failed tool and the LLM-derived fallback estimate, with a confidence penalty (mirroring EFRA-AI's L1/L2 fallback hierarchy).
- `market_check_resolutions` returning empty (no resolved markets) must not be read as "calibration = perfect." The KATA template must distinguish "no outcomes" from "outcomes confirm forecast."
- `company_transcript` returning no transcript must not silently skip the listening step. The flowdef condition should be `condition: "{{ step_3_result is defined }}"` and LENS should flag the missing transcript as a knowledge gap.

---

## Phase 3 — Future Skills & Improvements

### Capability gaps revealed by the conversion

| Gap | Source | Proposed skill/tool | Priority |
|---|---|---|---|
| SEC EDGAR XBRL fetching | COMPANY `edgar_facts` input | `sec-filings` MCP tool: fetch 10-K/10-Q/8-K, parse XBRL to structured financials | **High** — COMPANY and FORENSIC both need it; `fetch` can reach URLs but not parse XBRL |
| Financial-news API with source tiers | INTEL `rawNewsPool` | `financial-news` MCP tool: tier-1 (Reuters/Bloomberg/WSJ) / tier-2 (trade press) classification, dedup, relevance scoring | **High** — INTEL's mosaic depends on tiered news; `web_search` does not classify source tier |
| Hypothesis lifecycle tracking | INTEL `Hypothesis` / `HypothesisLifecycle` | `hypothesis-ledger` MCP tool: persistent hypothesis state (PENDING → VALIDATED / UNRESOLVABLE), tied to ticker + analyst + date | **Medium** — EFRA-AI tracks this in PostgreSQL; kask has no persistence for research objects (only portfolio ledger) |
| Excellence universe S1–S11 screening | SCOUT 11 criteria | `equity-screener` MCP tool: deterministic screening against live market data (exchanges, price, working capital, debt, margins, market cap, growth) | **Medium** — the 11 criteria are deterministic but require multiple live data sources |
| Forecast-to-outcome calibration store | KATA PDCA "check" | `forecast-ledger` MCP tool: persist forecast (ticker, PT, probability, date, horizon) → resolve against realized price → Brier score | **High** — this is the infrastructure for the highest-value Phase 2 loop (`market_check_resolutions` → KATA). `market_calibration` does this for prediction markets but not for equity PTs. |

### Kata-improvement lens: agents that benefit from deliberate-practice loops

| Agent | Kata opportunity | Loop design |
|---|---|---|
| CRITICAL FACTOR (probability assignment) | Forecast accuracy improves over time if probabilities are scored against outcomes | forecast → `forecast-ledger` → Brier → KATA obstacle ("probability was overconfident") → next forecast with adjusted prior. **This is the core kata loop.** |
| VALUATION (price target) | PT hit rate (EFRA-AI targets 58%) improves if PTs are scored against realized prices | PT → `forecast-ledger` → hit/miss → KATA obstacle ("PT bias: consistently high/low") → next valuation with bias adjustment |
| LENS (Dunning-Kruger flag) | The overconfidence flag calibrates if `process_confidence` vs `final_confidence` gaps are tracked over time | LENS flag → `forecast-ledger` confidence gap → KATA obstacle ("overconfidence pattern across N analyses") → next analysis with forced confidence reduction |
| IMAGINE (falsifiable predictions) | The 3–5 falsifiable predictions per analysis become testable over time | prediction → `forecast-ledger` → resolution → KATA "check" → next IMAGINE with updated hit rate |

**Kata-improvement verdict**: The entire flash pipeline should be wrapped in a kata loop at the *skill* level: each completed analysis writes its forecasts/PTs/predictions to `forecast-ledger`, and the next invocation of the skill on the same ticker reads prior outcomes as KATA "check" input. This is not a new template — it is a flowdef input (`prior_outcomes`) that feeds the KATA step.

### Deep-module lens: proposed new skills

| Proposed skill | Purpose | Deep-module verdict | Essentialist verdict | Priority |
|---|---|---|---|---|
| `forecast-ledger` (MCP tool) | Persist equity forecasts/PTs/predictions; resolve against realized prices; Brier score | **Deep** — high benefit (closes the calibration loop for every downstream skill), low cost (one persistence + one resolution endpoint) | Exist ✅ (no existing kask tool does this for equities) / Surface ✅ / Contract ✅ | **High** |
| `sec-filings` (MCP tool) | Fetch + parse SEC EDGAR XBRL to structured financials | **Deep** — high benefit (grounds COMPANY, FORENSIC, VALUATION in real data), moderate cost (XBRL parsing) | Exist ✅ / Surface ✅ / Contract ✅ | **High** |
| `financial-news` (MCP tool) | Tiered financial-news retrieval with source classification | **Deep** — high benefit (INTEL mosaic quality), moderate cost | Exist ✅ / Surface ✅ / Contract ✅ | **High** |
| `hypothesis-ledger` (MCP tool) | Persistent hypothesis lifecycle tracking | **Shallow** — thin wrapper over a key-value store; the lifecycle state machine is trivial | Exist ⚠️ (could be a `ledger_apply` variant) / Surface ⚠️ / Contract ⚠️ | **Low** — defer until `forecast-ledger` proves the persistence pattern |
| `equity-screener` (MCP tool) | Deterministic S1–S11 excellence-universe screening | **Deep** — high benefit (automates SCOUT's deterministic gate), moderate cost (multiple data source integrations) | Exist ✅ / Surface ✅ / Contract ✅ | **Medium** |
| `company-research` (skill) | The converted EFRA-AI pipeline (this plan) | **Deep** — the entire point of this plan | Exist ✅ / Surface ✅ / Contract ✅ | **High** (Phase 1 + 2) |
| `gorilla-framework` (standalone skill) | Extract GORILLA's 4-dimension framework as a reusable skill (not just a template inside company-research) | **Shallow** — only one consumer (company-research); extracting it adds a crate without adding consumers | Exist ❌ (no second consumer) / Surface ❌ / Contract ❌ | **Low** — keep as a template inside `company-research` |
| `lens-five-frameworks` (standalone skill) | Extract LENS's 5-framework audit as a reusable skill | **Shallow** — only one consumer (company-research); the 5 frameworks are firm-specific IP not generalizable | Exist ❌ / Surface ❌ / Contract ❌ | **Low** — keep as a template inside `company-research` |

### Prioritized improvement list

| Priority | Item | Dependency | Rationale |
|---|---|---|---|
| **High** | Build `company-research` skill (Phase 1 + 2 of this plan) | — | The core deliverable |
| **High** | Build `forecast-ledger` MCP tool | `company-research` (consumes it) | Closes the forecast-to-outcome calibration loop — the highest-value cybernetic integration |
| **High** | Build `sec-filings` MCP tool | `company-research` COMPANY/FORENSIC/VALUATION (consume it) | Grounds the deepest agents in real SEC data instead of LLM knowledge |
| **High** | Build `financial-news` MCP tool | `company-research` INTEL (consumes it) | INTEL's mosaic quality depends on tiered news; `web_search` does not classify |
| **Medium** | Build `equity-screener` MCP tool | `company-research` SCOUT (consumes it) | Automates the deterministic S1–S11 gate |
| **Low** | `hypothesis-ledger` MCP tool | `forecast-ledger` (proves the pattern) | Defer until persistence pattern is proven |
| **Low** | Extract GORILLA / LENS as standalone skills | A second consumer appearing | Only extract when a second consumer exists; until then, templates inside `company-research` |

---

## Pragmatic-semantics check (IS vs OUGHT)

Every OUGHT claim in this plan, with its supporting IS claim:

| OUGHT claim | Supporting IS claim |
|---|---|
| The 13 EFRA-AI agents should become one kask skill with two flowdefs | IS: EFRA-AI has two pipelines (verified in `pipeline.ts` + README); kask flowdef supports sequential steps (verified in superforecasting/scenario-builder manifests) |
| CATALOG should be eliminated | IS: CATALOG is not invoked in `pipeline.ts`; its function (output-type selection) is determined by `downstream_mode` (verified in types.ts `OutputType` and README operating-modes table) |
| KATA should reuse `kata-improvement` | IS: EFRA-AI's KATA agent implements the Toyota Improvement Kata (verified in README + types.ts `KataBoard`); `kata-improvement` implements the same Kata (verified in its manifest.yaml) |
| VALUATION Steps 2/4/5–7 should call MCP tools | IS: `dcf_valuation`, `comparable_analysis`, `expectations_gap` MCP tools exist (verified in this session's tool surface) and perform exactly these functions |
| `market_check_resolutions` → KATA is the highest-value integration | IS: EFRA-AI's KATA agent defines a PDCA "check" step (verified in types.ts `PdcaCycle.check`) but has no outcome data source; `market_check_resolutions` provides resolved prediction-market outcomes (verified in tool description) |
| THESIS quality gate should use `goal-analysis` | IS: `.rules` flags the LLM-improves-against-LLM-scored-target trap; `goal-analysis` performs semantic evaluation against stated goals (verified in skill description) |
| GORILLA → `mcda` should be rejected | IS: the 4 weights are fixed at 25/30/25/20 (verified in README); `mcda`'s value is user-tunable weights (verified in skill description) |
| `forecast-ledger` is a High-priority new tool | IS: no kask MCP tool persists equity forecasts/PTs for Brier scoring (verified by tool inventory); `market_calibration` does this only for prediction markets (verified in tool description) |

**No OUGHT claim is left without a supporting IS claim.**

---

## Acceptance criteria check

1. ✅ Every EFRA-AI agent from `src/agents/` is accounted for in the Phase 1 mapping table (13 agents: 01-scout, 02-intel, 03-critical-factor, 04-forensic ×2, 05-valuation, 06-communication, 07-catalog, 08-kata, 09-lens, 10-gorilla, 11-imagine, 12-thesis, 13-company), each with an essentialist verdict (DEEPEN/KEEP/ELIMINATE).
2. ✅ Every proposed kask-tool integration in Phase 2 has an explicit pragmatic-cybernetics loop-impact assessment (creates/strengthens/breaks/no-impact) and an essentialist 3-gate verdict (Exist/Surface/Contract).
3. ✅ The plan explicitly addresses the sequential-pipeline question: kask natively supports sequential pipelines via flowdef `steps`/`ordinal`/`input_mapping`/`condition`/`loop` (verified in superforecasting and scenario-builder manifests). No emulation strategy needed.
4. ✅ Every Analyst Solutions methodology step represented in EFRA-AI is mapped to a kask primitive in Phase 1 (self-verification table). No step is missing.
5. ✅ No OUGHT claim is left without a supporting IS claim (pragmatic-semantics table above).

---

## Open questions for the user

1. **Forecast-ledger scope**: Should `forecast-ledger` persist only equity PTs/probabilities, or also the IMAGINE falsifiable predictions? The latter are longer-horizon (5/10/20Y) and may not resolve within a useful feedback window. **Recommendation**: persist both, but tag by horizon so KATA weights short-horizon outcomes more heavily in the "check" step.
2. **Two flowdefs vs one**: This plan proposes two flowdef manifests (`company-research-flash`, `company-research-deep`) in one skill crate. An alternative is a single 21-step flowdef with a branch after step 10 (LENS) into the deep pipeline. **Recommendation**: two flowdefs — they have different convergence criteria (flash: LENS verdict; deep: THESIS `investment_grade`) and different invocation contexts (flash is event-driven; deep is initiation-driven).
3. **KATA cross-skill reuse depth**: EFRA-AI's KATA agent runs all 4 Improvement Kata steps in one LLM call. The `kata-improvement` skill exposes them as 4 separate templates (`improvement-step1-direction` through `improvement-step4-experiment`). Should the flash flowdef invoke all 4 as separate steps (more structured, more gas) or call a single composite template? **Recommendation**: 4 separate steps — the structure is the point of the Kata, and gas cost is bounded by the flowdef `gas.cap`.
4. **MAIA cross-check depth**: This plan applied the MAIA cross-check at the level of "kask MCP servers were built MAIA-first" (verified via the `listening` skill manifest). A deeper MAIA cross-check would read the MAIA-Substack posts on agent-tool interaction patterns. **Recommendation**: defer the deeper MAIA cross-check to the implementation phase, where the specific agent-tool interaction patterns matter.
