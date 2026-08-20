---
title: "Forecasting and Scenarios"
audience: [architects, developers, operators]
last_updated: 2026-08-20
version: "0.37.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition]
---

# Forecasting and Scenarios

The scenarios and companies MCP servers run as builtin context servers (child
processes over stdio) inside zed-kask (D1–D3).

**Diataxis type:** Explanation
**Status:** Active (v0.36.0)
**Related:** `registry/templates/superforecasting/README.md` (skill pipeline), `crates/hkask-forecast/README.md` (library), `mcp-servers/hkask-mcp-scenarios/README.md` (tool reference)

## Why this document exists

Forecasting in hKask appears in four places — a natural-language skill, a pure-math Rust library, and two domain MCP servers — all describing the same Tetlock methodology at different resolutions. The scenarios MCP server additionally integrates Schwartz's scenario planning and Chermack's assessment framework. This document explains how these surfaces combine, which layer owns what, and why, so reviewers can evaluate whether the implementation matches the methodology.[^tetlock][^schwartz][^chermack]

## Three methodologies, one pipeline

The scenarios MCP server implements three forecasting methodologies as an integrated pipeline:[^tetlock-pipe][^schwartz-pipe][^chermack-pipe]

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

The scenarios server uses a binomial event-tree model (MAIA methodology):[^bayesian-forecasting]
- Each event is a yes/no question with a deadline
- Events can depend on other events via conditional probability tables
- Marginal probabilities are computed via full joint-table marginalization under parent independence
- The "all events occur" path probability is the product of all-node-occur conditionals

## The three-layer architecture

The separation of skill, canonical-math, and domain-server layers follows the deep-module discipline: each module has a narrow interface and deep implementation, and domain logic stays where it is entangled with domain types and I/O.[^ousterhout-deep]

```
┌──────────────────────────────────────────────────────────────┐
│  Skill layer  — registry/templates/superforecasting/*.j2     │
│  Natural-language Tetlock pipeline (8 stages + gate +        │
│  convergence). LLM reasoning: triage judgment, hypothesis     │
│  generation, counterfactual analysis, dragonfly synthesis,   │
│  calibration, record, quality gate. PDCA loop + quality gate.  │
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

Both servers once defined a local `SubQuestion { question, estimate, confidence }` byte-identical to `hkask_forecast::FermiQuestion`. The essentialist deletion test treats them differently:[^ousterhout-deletion]

- **Companies** used `SubQuestion` as a standalone type with no embedding. Deleting it and consuming `hkask_forecast::FermiQuestion` directly removed the duplicate type and the conversion adapter in one move. **Eliminated.**
- **Scenarios** embeds `SubQuestion` inside domain aggregates (`ScenarioEvent.sub_questions`, `Perspective.fermi_sub_questions`). Replacing it would be a wide type migration across many struct definitions for a 3-line saving. **Retained** — the adapter is the cheaper seam.

## The conformance contract

The contract lives in `registry/templates/superforecasting/README.md` as the "Deterministic Primitives" table. It maps each skill stage to the `hkask-forecast` function that implements its deterministic core, or marks the stage "natural-language only". The contract is mechanically verified by `scripts/check-forecast-conformance.sh` (run in CI), which asserts:[^fagan-inspection]

1. Every `hkask-forecast` public function is referenced in the contract table (no orphan primitives).
2. Every primitive the contract table names actually exists in `hkask-forecast` (no dangling references).

## The closed feedback loop (operational)

The Brier learning loop — Tetlock's record → score → recalibrate cycle — is operational across the layers:[^brier-1950][^tetlock-record]

1. **Record**: `scenario_score` writes `StoredForecastRecord` entries into the `ForecastStore` journal.
2. **Score**: `hkask_forecast::brier_score` / `brier_score_multi` compute the Brier score for resolved forecasts.
3. **Calibration curve**: `compute_calibration_curve` (scenarios) bins resolved forecasts into 10 probability bands and derives an `overconfidence_score`.
4. **Recalibrate**: `hkask_forecast::apply_calibration_adjustment` consumes the overconfidence bias and regresses the next forecast's prior. `scenario_calibrate` applies this automatically when ≥5 resolved forecasts exist.

## The `compute` action

> **Note (2026-08-20):** The FlowDef executor and its `compute` step action
> were removed with the `hkask-templates` crate (commit `5f4cf5f10d`).
> Skill execution is now upstream-Zed body injection: `SkillTool::run` reads
> the `SKILL.md` body and injects it via `render_skill_envelope`. The
> deterministic `hkask_forecast::*` primitives are now invoked by the model
> directly via the `lisp_eval` agent tool (wrapping
> `hkask_lisp::eval_sandboxed_with_budget`) when a SKILL.md instructs it to.
> The table below documents the **historical** FlowDef-based pipeline; the
> same primitives survive in `hkask-forecast` and are reached via `lisp_eval`
> in the current model.

The former FlowDef executor supported a `compute` step action alongside
`select` (LLM), `populate`, `execute` (MCP tool), `choice`, and `loop`. A
`compute` step invoked a canonical `hkask_forecast::*` primitive
deterministically — no LLM round-trip, no MCP call, no inference
 cost.[^deming-pdca-compute]

The superforecasting skill (formerly manifest) used `compute` for three
deterministic stages within the 16-step pipeline:

| Step | Action | compute_ref | Role |
|------|--------|------------|------|
| 3 | compute | `calibrate_from_fermi` | Fermi weighted-average of LLM-produced sub-questions → inside estimate |
| 5 | compute | `outside_view_adjustment` | Shrinkage blend of LLM-produced base rate with Fermi estimate → calibrated anchor |
| 10 | compute | `bayesian_update` | Bayes' theorem: LLM produces P(E|H) + P(E), Rust computes the posterior |
| 16 | compute | `apply_calibration_adjustment` | Calibration feedback in loop re-entry → adjusted prior |

## Common drift and how this model prevents it

| Drift | How the model catches it |[^ousterhout-drift]
|-------|---------------------------|
| A MCP server reimplements a canonical primitive instead of delegating. | The conformance test surfaces un-delegated math; the canonical layer is the only place the formulas live. |
| The skill describes a formula the Rust lib no longer implements. | The contract table's named functions are checked to exist; a removed function fails CI. |
| `hkask-forecast` grows a primitive the skill's pipeline doesn't use. | The conformance test flags orphan primitives. |
| Stage names diverge between the skill and the servers. | The contract table is the authoritative stage↔primitive map. |

## Non-goals

- This model does not require `hkask-forecast` to implement every Tetlock stage. Stages that are inherently LLM judgment (triage, inside-view hypothesis generation, synthesis, forward calibration, record, quality gate, convergence) have no pure-math core and correctly have no Rust counterpart.[^tetlock-nongoal]
- This model does not make the skill call Rust. The skill remains a natural-language pipeline; the contract is about consistency of formulas, not runtime invocation.
- This model does not merge the two MCP servers. They serve different domains (event trees vs financial valuation) and share only the canonical-math layer.

## Cross-links

- [Scenarios MCP server reference](../reference/mcp-servers/scenarios.md) — tool flow diagram
- [Scenarios ↔ Companies Bridge](../architecture/core/scenarios-companies-bridge.md) — FIBO to Dublin Core translation

## Footnotes

[^tetlock]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited as the primary methodology this document maps onto the three-layer implementation.

[^schwartz]: Schwartz, P. (1991). *The Art of the Long View*. Doubleday.
    Cited for the scenario-construction methodology (focal question, driving forces, 2×2 axis matrix) integrated into the pipeline.

[^chermack]: Chermack, T. J. (2011). *Scenario Planning in Organizations: Breakthroughs in Decision Making*. Berrett-Koehler Publishers.
    Cited for the five-phase project-assessment framework the `scenario_assess` tool implements.

[^tetlock-pipe]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited for the calibration engine (triage, Fermi decomposition, Bayesian update, dragonfly-eye synthesis, Brier scoring).

[^schwartz-pipe]: Schwartz, P. (1991). *The Art of the Long View*. Doubleday.
    Cited for the framing and brainstorming tools (`scenario_frame`, `scenario_brainstorm`).

[^chermack-pipe]: Chermack, T. J. (2011). *Scenario Planning in Organizations: Breakthroughs in Decision Making*. Berrett-Koehler Publishers.
    Cited for the evaluation framework the `scenario_assess` tool implements.

[^bayesian-forecasting]: Howson, C., & Urbach, P. (2006). *Scientific Reasoning: The Bayesian Approach* (3rd ed.). Open Court Publishing.
    Cited for the conditional-probability marginalization the event-tree model uses.

[^ousterhout-deep]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the deep-module discipline that keeps domain logic in the server layer, not in the canonical-math library.

[^ousterhout-deletion]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the deletion-test heuristic that governs whether a duplicate type is eliminated or retained as an adapter seam.

[^fagan-inspection]: Fagan, M. E. (1976). Design and code inspections to reduce errors in program development. *IBM Systems Journal*, 15(3), 182–211. https://doi.org/10.1147/sj.153.0182
    Cited for the mechanical-conformance-inspection principle the conformance contract applies to skill–library consistency.

[^brier-1950]: Brier, G. W. (1950). Verification of forecasts expressed in terms of probability. *Monthly Weather Review*, 78(1), 1–3. https://doi.org/10.1175/1520-0493(1950)078<0001:VOFERT>2.0.CO;2
    Cited for the Brier scoring formula the closed feedback loop uses to measure forecast accuracy.

[^tetlock-record]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited for the record → score → recalibrate cycle that operationalizes Brier's calibration feedback.

[^deming-pdca-compute]: Deming, W. E. (1986). *Out of the Crisis*. MIT Center for Advanced Engineering Study.
    Cited for the PDCA cycle the `compute` action embeds as a deterministic step within the LLM-driven cascade.

[^ousterhout-drift]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the module-boundary rationale that prevents a server from reimplementing a canonical primitive.

[^tetlock-nongoal]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited for the distinction between LLM-judgment stages and deterministic-math stages that justifies the non-goal boundary.
