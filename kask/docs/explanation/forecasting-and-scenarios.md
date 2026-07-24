# Forecasting and Scenarios

**Diataxis type:** Explanation
**Status:** Current (v0.31.0)
**Related:** `registry/templates/superforecasting/README.md` (skill pipeline), `crates/hkask-forecast/README.md` (library), `mcp-servers/hkask-mcp-scenarios/README.md` (tool reference)

## Why this document exists

Forecasting in hKask appears in four places — a natural-language skill, a pure-math Rust library, and two domain MCP servers — all describing the same Tetlock methodology at different resolutions. The scenarios MCP server additionally integrates Schwartz's scenario planning and Chermack's assessment framework. This document explains how these surfaces combine, which layer owns what, and why, so reviewers can evaluate whether the implementation matches the methodology.

## Three methodologies, one pipeline

The scenarios MCP server implements three forecasting methodologies as an integrated pipeline:

### Tetlock — Forecast accuracy

The superforecasting methodology (Tetlock & Gardner, 2015) provides the calibration engine:
- **Triage** — classify questions as clocklike, Goldilocks, or cloudlike
- **Fermi decomposition** — break forecasts into sub-questions with confidence-weighted estimates
- **Outside view** — blend with base rates using a shrinkage estimator
- **Bayesian updating** — revise probabilities as evidence arrives
- **Dragonfly-eye synthesis** — aggregate multiple perspectives with inverse-Brier weighting
- **Brier scoring** — measure forecast accuracy against outcomes
- **Calibration tracking** — detect systematic over/underconfidence

### Schwartz — Scenario imagination

The Art of the Long View (Schwartz, 1991) provides the scenario construction approach:
- **Focal question** — what decision does this inform?
- **Driving forces** — STEEP analysis (Social, Technological, Economic, Environmental, Political)
- **2×2 axis matrix** — two key uncertainties define four scenarios (implemented in the companies server for financial modeling)
- **Implications** — what strategies work across scenarios?

In the scenarios server, Schwartz provides the framing and brainstorming tools (`scenario_frame`, `scenario_frame_document`, `scenario_brainstorm`).

### Chermack — Project assessment

Chermack's Performance-Based Scenario System (2011) provides the evaluation framework:
- **Phase 1: Preparation** — stakeholder engagement, scope clarity
- **Phase 2: Exploration** — driving forces, diversity of views
- **Phase 3: Development** — causal structure, internal consistency
- **Phase 4: Implementation** — strategies applied, early warning indicators
- **Phase 5: Project Assessment** — learning outcomes, calibration evidence

The `scenario_assess` tool evaluates a project across all five phases.

### How they connect

```
Schwartz (framing)     → Tetlock (calibration)    → Chermack (assessment)
scenario_frame         scenario_calibrate         scenario_assess
scenario_brainstorm    scenario_quantify
scenario_build         scenario_update
                       scenario_synthesize
                       scenario_score
                       scenario_calibration
```

The pipeline flows from imagination (Schwartz) through computation (Tetlock) to evaluation (Chermack). The `scenario_full` tool compresses the Tetlock stages into a single call.

## Event-tree model (MAIA)

The scenarios server uses a binomial event-tree model (MAIA methodology):
- Each event is a yes/no question with a deadline
- Events can depend on other events via conditional probability tables
- Marginal probabilities are computed via full joint-table marginalization under parent independence
- The "all events occur" path probability is the product of all-node-occur conditionals

## The three-layer architecture

```
┌──────────────────────────────────────────────────────────────┐
│  Skill layer  — registry/templates/superforecasting/*.j2     │
│  Natural-language Tetlock pipeline (8 stages + gate +        │
│  convergence). LLM reasoning: triage judgment, hypothesis     │
│  generation, counterfactual analysis, dragonfly synthesis,   │
│  calibration, record, quality gate. PDCA loop + fusion panel.  │
└──────────────────────────────────────────────────────────────┘
                          │  documents the formulas
                          │  it relies on (conformance contract)
                          ▼
┌──────────────────────────────────────────────────────────────┐
│  Canonical-math layer  — crates/hkask-forecast                │
│  Pure-math Tetlock primitives only. No domain types, no NLP,  │
│  no I/O. calibrate_from_fermi, outside_view_adjustment,        │
│  bayesian_update, brier_score, brier_score_multi,             │
│  brier_interpretation. The single source of truth for the     │
│  deterministic core.                                           │
└──────────────────────────────────────────────────────────────┘
                          ▲  consumed via hkask_forecast::*
                          │  (adapters convert domain types)
              ┌───────────┴────────────────────┐
              ▼                                ▼
┌────────────────────────────┐  ┌──────────────────────────────┐
│ hkask-mcp-scenarios         │  │ hkask-mcp-companies           │
│ Event-tree forecasting,     │  │ FIBO-anchored financial       │
│ ForecastStore journal,      │  │ forecasting, WeightedScenario │
│ calibration curve, triage   │  │ intrinsic-value distribution, │
│ heuristic, certainty tiers. │  │ FermiDefaults env loading.    │
└────────────────────────────┘  └──────────────────────────────┘
```

### What each layer owns

**Skill layer** (`registry/templates/superforecasting/`) — owns the full Tetlock pipeline as LLM prompts. This is where the methodology lives as natural-language reasoning: triage into the Goldilocks zone, Fermi decomposition, outside-view base-rate anchoring, inside-view hypothesis generation + counterfactual analysis (delegated to `falsifiability`), Bayesian evidence update, dragonfly-eye MCDA synthesis, forward-looking calibration, structured record, independent quality gate, and convergence check. These stages are not reducible to pure math — "steelman the strongest opposing argument" is LLM judgment, not a formula.

**Canonical-math layer** (`crates/hkask-forecast/`) — owns the deterministic primitives: confidence-weighted averaging (Fermi), shrinkage estimation (outside view), Bayes' theorem (evidence update), and Brier scoring (calibration tracking). Pure math, no domain types, no NLP, no I/O. Both MCP servers consume it via `hkask_forecast::*`.

**Domain MCP servers** (`hkask-mcp-scenarios`, `hkask-mcp-companies`) — own the domain applications that compose the canonical primitives with domain-specific types and I/O. Domain logic stays here, not in `hkask-forecast`, because it is entangled with domain types and I/O — moving it up would violate the deep-module discipline.

### Why `SubQuestion` survives in scenarios but not in companies

Both servers once defined a local `SubQuestion { question, estimate, confidence }` byte-identical to `hkask_forecast::FermiQuestion`. The essentialist deletion test treats them differently:

- **Companies** used `SubQuestion` as a standalone type with no embedding. Deleting it and consuming `hkask_forecast::FermiQuestion` directly removed the duplicate type and the conversion adapter in one move. **Eliminated.**
- **Scenarios** embeds `SubQuestion` inside domain aggregates (`ScenarioEvent.sub_questions`, `Perspective.fermi_sub_questions`). Replacing it would be a wide type migration across many struct definitions for a 3-line saving. **Retained** — the adapter is the cheaper seam.

## The conformance contract

The contract lives in `registry/templates/superforecasting/README.md` as the "Deterministic Primitives" table. It maps each skill stage to the `hkask-forecast` function that implements its deterministic core, or marks the stage "natural-language only". The contract is mechanically verified by `scripts/check-forecast-conformance.sh` (run in CI), which asserts:

1. Every `hkask-forecast` public function is referenced in the contract table (no orphan primitives).
2. Every primitive the contract table names actually exists in `hkask-forecast` (no dangling references).

## The closed feedback loop (operational)

The Brier learning loop — Tetlock's record → score → recalibrate cycle — is operational across the layers:

1. **Record**: `scenario_score` writes `StoredForecastRecord` entries into the `ForecastStore` journal.
2. **Score**: `hkask_forecast::brier_score` / `brier_score_multi` compute the Brier score for resolved forecasts.
3. **Calibration curve**: `compute_calibration_curve` (scenarios) bins resolved forecasts into 10 probability bands and derives an `overconfidence_score`.
4. **Recalibrate**: `hkask_forecast::apply_calibration_adjustment` consumes the overconfidence bias and regresses the next forecast's prior. `scenario_calibrate` applies this automatically when ≥5 resolved forecasts exist.

## The `compute` action

The FlowDef executor supports a `compute` step action alongside `select` (LLM), `populate`, `execute` (MCP tool), `choice`, and `loop`. A `compute` step invokes a canonical `hkask_forecast::*` primitive deterministically — no LLM round-trip, no MCP call, no inference cost.

The superforecasting manifest uses `compute` for three deterministic stages within the 16-step pipeline:

| Step | Action | compute_ref | Role |
|------|--------|------------|------|
| 3 | compute | `calibrate_from_fermi` | Fermi weighted-average of LLM-produced sub-questions → inside estimate |
| 5 | compute | `outside_view_adjustment` | Shrinkage blend of LLM-produced base rate with Fermi estimate → calibrated anchor |
| 10 | compute | `bayesian_update` | Bayes' theorem: LLM produces P(E|H) + P(E), Rust computes the posterior |
| 16 | compute | `apply_calibration_adjustment` | Calibration feedback in loop re-entry → adjusted prior |

## Common drift and how this model prevents it

| Drift | How the model catches it |
|-------|---------------------------|
| A MCP server reimplements a canonical primitive instead of delegating. | The conformance test surfaces un-delegated math; the canonical layer is the only place the formulas live. |
| The skill describes a formula the Rust lib no longer implements. | The contract table's named functions are checked to exist; a removed function fails CI. |
| `hkask-forecast` grows a primitive the skill's pipeline doesn't use. | The conformance test flags orphan primitives. |
| Stage names diverge between the skill and the servers. | The contract table is the authoritative stage↔primitive map. |

## Non-goals

- This model does not require `hkask-forecast` to implement every Tetlock stage. Stages that are inherently LLM judgment (triage, inside-view hypothesis generation, synthesis, forward calibration, record, quality gate, convergence) have no pure-math core and correctly have no Rust counterpart.
- This model does not make the skill call Rust. The skill remains a natural-language pipeline; the contract is about consistency of formulas, not runtime invocation.
- This model does not merge the two MCP servers. They serve different domains (event trees vs financial valuation) and share only the canonical-math layer.

## Cross-links

- [Scenarios MCP server reference](../reference/mcp-servers/scenarios.md) — tool flow diagram
- [Scenarios ↔ Companies Bridge](../architecture/core/scenarios-companies-bridge.md) — FIBO to Dublin Core translation
