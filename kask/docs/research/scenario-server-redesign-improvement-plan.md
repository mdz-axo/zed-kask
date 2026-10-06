---
title: "Scenarios MCP Server — From-Scratch Design Review (Improvement Plan)"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - scenario-server-redesign-review.md
  - scenario-server-redesign-findings.md
  - scenario-server-redesign-proposals.md
---

# Improvement Plan — Phase 6

Sequenced by dependency, each slice independently verifiable. Effort
classes: XS (< half a day), S (days), M (a week+). Nothing here is
implemented — this is the execution sequence for the proposal set.

## Execution status (2026-10-06)

Slices 1–5 are landed (uncommitted in the working tree), plus PR-09 of
Slice 6. Slice 1 (PR-03/04/06/12/14/16), Slice 2 (PR-05/11), Slice 3
(PR-01/07 — plus a wiring fix found during PR-07: `run()` passed the data
DIR as the forecast store's snapshot FILE path, so production compaction
failed past the journal threshold and the journal landed outside the D28
dir), Slice 4 (PR-02 — marginal scoring, journal schema v3 with the
`scored_from_marginal` marker), Slice 5 (PR-08 — the posterior engine
promoted to `hkask_forecast::posterior`, the `scenario_recompute_posteriors`
tool exposed with the surface pin moved 19→20, the graph widget converted
to a delegating adapter with its local engine deleted; the promotion also
fixed a real divergence — the widget's multi-group PRODUCT combination
silently disagreed with `scenario_quantify`'s documented noisy-OR rule, now
unified on `hkask_forecast::combine_independent_channels` and pinned), and
PR-09 (the scenario join recorded on equity forecasts: `forecast_persist`
carries `scenario_project_id`, `scenario_tree`, `impact_mappings_ref`,
`fused_volatility`, echoed into the durable snapshot and read back;
the flash skill's persist step cites the recorded fields). Verification
receipts: 186 companies tests, 40 scenarios tests, 36 widget tests, 65
forecast-crate tests green; scoped clippy clean over every touched crate;
the MAIA anchor is in the derived registry (live `onto_anchor` resolution
pending the operator's next server restart). **Remaining: PR-10** — the
opt-in `WorkbookWhatIf` presentation for `scenario_impact_valuation`
(wiring the `hkask-spreadsheet` engine into the companies server; the
pattern to mirror is `portfolio_what_if`'s workbook branch,
`hkask-mcp-portfolio/src/server.rs:759-784`: `ArtifactOrigin` +
`PublishOptions { access: SpreadsheetAccess::WorkbookWhatIf }` +
`spreadsheet_hint`, with the default output byte-identical and pinned).
PR-13 and PR-15 stay deferred as operator decisions.

## Slice 1 — Fidelity quick wins (no dependencies)

**Proposals:** PR-03, PR-04, PR-06, PR-12, PR-14, PR-16.

**Acceptance criteria:**
- `scenario_assess` on a multi-subject store returns a Phase-5 curve
  computed only from `req.subject`'s records (test: two subjects, distinct
  biases; the assessment reflects the filtered one).
- Triage outputs specification-quality labels; a grep over the tree finds
  no consumer still reading `clocklike` from the tool (skill stage-0 and
  Phase-1 steps updated in the same change).
- The brainstorm default string no longer names `scenario_research`; the
  README tool table matches the live 19-tool surface exactly (a
  count-and-name check, not eyeball); the `scenario_build` description names
  the `context` field; the superforecasting README's audit-trail section
  states that `scenario_update` is stateless (or PR-01 lands first and the
  claim becomes true — either way the false claim is gone).
- The sequence advisory accepts market-bridge and superforecasting entries
  into `scenario_quantify` without warning; the "expected noise" note is
  deleted from the superforecasting skill.
- The tree-node field is `certainty_distance`; the `graph` viz block
  contract and the widget parser agree on the new name in the same change.
- The assess integration string no longer attributes event trees to
  Schwartz; the three Schwartz surfaces are named with distinct contracts
  in the docs.

**Verification:** `./script/clippy` scoped to the touched crates; the
server's tool-surface pin stays at 19; the property layer green.

## Slice 2 — Anchors and skill scaffolding

**Proposals:** PR-05, PR-11. **Depends:** none (PR-11 soft-depends on
PR-01; lands with agent-minted ids if Slice 3 hasn't run).

**Acceptance criteria:**
- `onto_anchor` returns a derived entry for the MAIA event-based scenario
  template naming the "Time Horizons" post; `scenario-planning/SKILL.md`
  lists MAIA among the reference models; the `basis` doc comment names both
  semantics.
- The skill's Phase 1 scaffolds a board with a per-scenario goal carrying
  an intake prediction and per-event tasks with deadlines; per-type framing
  defaults exist as registry templates for MAIA's four types; the
  regression case renders one template per type.

**Verification:** the skill's regression case; `render_template` contract
checks (`all_registry_templates_conform` walks the new templates).

## Slice 3 — The project record

**Proposals:** PR-01, PR-07. **Depends:** none (PR-07 depends on PR-01).

**Acceptance criteria:**
- `scenario_frame_document` persists the framing document under a project
  id; `scenario_assess` with an unknown `project_id` fails `not_found`
  naming it; with a known one it derives event/dependency counts from the
  stored tree and scores Phase 1 from the framing document's fields.
- The last-quantified tree persists with the project;
  `contract_price_coherence`'s `tree_implied` default survives a server
  restart (test: quantify → restart → coherence reads the same joint).
- The skill's assess step reads the project instead of recalling metrics.

**Verification:** the store's durability regression pattern
(`tests/tool_behavior.rs` — snapshot/journal ordering tests) extended to
the project store; the pin count unchanged at 19.

## Slice 4 — Scoring semantics

**Proposals:** PR-02. **Depends:** none (lands cleanlyest after Slice 3 so
the journal keys reference the project, but independent).

**Acceptance criteria:**
- Scoring a dependent event Brier-scores the recomputed marginal, not the
  prior (test: a two-node tree where prior ≠ marginal; the journal record
  carries the marginal and a marker).
- `StoredForecastRecord` schema bumps; the calibration curve and
  domain-bias docs state they learn from marginals.

**Verification:** a property test pinning score-of-marginal against a
hand-computed tree; the existing calibration tests updated to the new
semantics (read the test's intent — the setup pins priors; fix the setup,
keep the purpose).

## Slice 5 — The posterior engine

**Proposals:** PR-08. **Depends:** none (coordinate the widget crate; after
Slice 3 the tool can reference the project's tree).

**Acceptance criteria:**
- The posterior math (backward inference, soft/hard evidence, polytree
  detection, fixpoint convergence) lives in `hkask-forecast`; the scenarios
  tool exposes it; the widget delegates (its local engine deleted).
- The tool's output matches the widget's engine on a shared test tree
  (same tree, same evidence, same posteriors — a parity test, not two
  implementations agreeing with themselves).
- The tool-surface pin moves 19 → 20 as an intentional change; DIAG-RF-005
  and the docs reference update in the same change.

**Verification:** parity test; `./script/clippy` over `hkask-forecast`,
the scenarios server, and the widget crate; the widget's existing
posterior tests (`propagate.rs:649-775`) green against the delegated
engine.

## Slice 6 — The equity bridge

**Proposals:** PR-09, PR-10. **Depends:** PR-01 (project id) — PR-10 is
independent.

**Acceptance criteria:**
- Equity forecast records carry the scenario fields (tree/project identity,
  node-marginal snapshot, impact-mapping reference, fused volatility); the
  flash skill cites the recorded fields instead of re-deriving the join.
- `scenario_impact_valuation` with `presentation: "WorkbookWhatIf"`
  publishes the path grid as an editable workbook; without the flag the
  output is byte-identical to today's.

**Verification:** the companies server's forecast-record round-trip tests
extended; a workbook what-if render of a two-node tree inspected; the
default-output byte-identity pinned.

## Deferred / operator decisions

- **PR-13 (`scenario_full` narrowing):** functional choice between
  narrowing to the deterministic core and removing the tool — the
  operator's call; both paths enumerated in the proposal.
- **PR-15 (portfolio event-exposure report):** deferred until an operator
  demands it; MAIA's post is the demand hypothesis, not demand.

## Net summary

**Built:** one durable project record (framing document + tree snapshots +
assessment history + journal linkage) — the server's first project
primitive; marginal-based scoring; a posterior-recomputation tool with the
math promoted to the shared crate; recorded scenario fields on equity
forecasts; an opt-in workbook presentation for impact grids; a kanban
project scaffold and per-type framing templates at the skill layer; the
MAIA anchor registration; six fidelity fixes (labels, names, strings,
advisory, scoping, docs).

**Deleted:** the caller-recalled assessment metrics (derived from the
project instead); the agent-minted `forecast_id` convention; the
process-local tree cache; the prior-scoring semantics; the widget's local
posterior engine (delegated); the linear sequence chain; the stale
`scenario_research` references and the drifted README table; the false
`scenario_update` journaling claim; the `clocklike` mislabel; the
`variance_contribution` misnomer; the conflated Schwartz attribution.
Every deletion is enumerated in its proposal with the replacement path.

**Migrates:** `StoredForecastRecord` schema (prior → marginal,
version bump); the equity forecast record (additive scenario fields,
versioned); the tool-surface pin (19 → 20 with PR-08; 19 → 18 if the
operator chooses PR-13's removal option); the assess wire contract
(metrics become overrides); the skill's Phase 1/5 steps (project-backed).

**Sequencing rationale:** Slice 1 is risk-free and removes active
misinformation first; Slice 2 registers the anchor before any code
builds on MAIA-derived fields; Slice 3 creates the spine everything
durability-related hangs on; Slice 4 changes learned-from semantics
before the calibration history grows further; Slice 5 is the largest
single change and lands on a stable spine; Slice 6 closes the
cross-server loop last, when the ids it needs to record exist.
