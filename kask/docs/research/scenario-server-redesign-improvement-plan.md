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

Slices 1–5 are landed, plus PR-09 of Slice 6 — committed in
`8692a23520` (review documents + Slices 1–3), `64b2f4f6d5` (Slices 4–6:
shared posterior engine, marginal scoring, scenario provenance),
`c03cf4aa89` (the PR-10 second pass: workbook presentation wiring and
pins), and `b252d88b3d` (post-restart follow-ups). Slice 1 (PR-03/04/06/12/14/16), Slice 2 (PR-05/11), Slice 3
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
pending the operator's next server restart). **PR-10 landed (2026-10-06,
second pass):** `scenario_impact_valuation` gained the `presentation` field
(`DataOnly` default | `WorkbookWhatIf`); the workbook mode publishes the
FULL path grid — the JSON output caps at 50 paths, the workbook carries
every one — as an editable workbook revision via the `hkask-spreadsheet`
engine wired into the companies server (per-instance `WorkbookService`,
mirroring the portfolio server), with a ```spreadsheet display hint. The
default output is byte-identical (pinned by
`scenario_impact_valuation_default_has_no_display_hint`; the workbook mode
pinned by
`scenario_impact_valuation_workbook_whatif_publishes_the_path_grid`).
188 companies tests green; the crate compiles clean — the workspace clippy
gate is currently blocked by a PARALLEL STREAM's in-flight `hkask-memory`
edits (`redundant clone` in `federated_recall.rs`, files this stream never
touched), reported, not fixed here. A cleanup sweep over the scenarios
server confirmed: every redesign deletion verified gone, zero
TODO/deprecated/dead-code markers, `paths` is consumed
(`paths_from_root` in the quantify output), `CertaintyTier::from_probability`
delegates to the shared thresholds — the one find was a stale
module-header doc (Tools (19) + the renamed pin + the missing new tool),
fixed in the same pass. PR-13 and PR-15 stay deferred as operator
decisions.

**Follow-up pass (2026-10-06, post-restart):** the servers were rebuilt
and restarted, closing both pending-restart receipts live — `onto_anchor`
resolves `maia_event_based_scenario_template` at the derived tier, and
`scenario_recompute_posteriors` is registered on the live surface. The
skill enrichments landed: `scenario-planning` step 14 names
`scenario_recompute_posteriors` as the both-directions evidence tool and
step 17 notes that scoring is on tree marginals; `company-research-flash`'s
valuation step surfaces the `WorkbookWhatIf` presentation. The
`companies.md` pre-existing staleness is fixed: the deleted
`scenario_from_companies` bridge's Ontology Translation table and its
probability/deadline heuristics are removed (design decisions renumbered),
and both DIAGRAM_ALIGNMENT records (DIAG-RF-004A, DIAG-RF-005) are
current. The workspace clippy gate is green over all touched crates
(the parallel `hkask-memory` stream landed its fix).

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

- **PR-13 (`scenario_full`):** decided — remove (operator, 2026-10-06).
  Executed the same day: handler, `FullPipelineRequest`, its test, and
  every live-surface doc row deleted; the pin moved 20 → 19 and stayed
  green. The staged pipeline is the only path.
- **PR-15 (portfolio event-exposure report):** decided — stay deferred
  (operator, 2026-10-06). The PR-09 join fields keep it an M effort
  when demand materializes.

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
versioned); the tool-surface pin (19 → 20 with PR-08; 20 → 19 with
PR-13's removal, executed 2026-10-06); the assess wire contract
(metrics become overrides); the skill's Phase 1/5 steps (project-backed).

**Sequencing rationale:** Slice 1 is risk-free and removes active
misinformation first; Slice 2 registers the anchor before any code
builds on MAIA-derived fields; Slice 3 creates the spine everything
durability-related hangs on; Slice 4 changes learned-from semantics
before the calibration history grows further; Slice 5 is the largest
single change and lands on a stable spine; Slice 6 closes the
cross-server loop last, when the ids it needs to record exist.

## Loose-end closeout (2026-10-06, third pass)

Terminal state: every loose end is closed with a receipt below or
parked as an explicit operator decision. Nothing is silently open.

### Closed receipts

| Item | Receipt |
|---|---|
| Commits | `8692a23520` (review + Slices 1–3), `64b2f4f6d5` (Slices 4–6), `c03cf4aa89` (PR-10 second pass), `b252d88b3d` (follow-ups); the working tree carries no scenarios files |
| Goal loop | `e82bf99d` scored achieved and acknowledged out of the retained list; `c2a23bc8`, `818afff3`, `5e1db9ad` judged done (scoring awaits operator confirmation) |
| Live surface | 20 tools registered post-restart; descriptions current (marginal scoring on `scenario_score`, project record on `scenario_assess`, the posterior tool) |
| Posterior engine | live probe on a two-node tree: LR 3.0 on a 0.5 prior → root posterior 0.75, child marginal 0.65 (hand-checked); method `polytree_backward_inference`, engine `hkask_forecast::posterior` shared with the graph widget |
| Cleanup | zero TODO/FIXME/deprecated/dead-code markers in the scenarios + forecast crates; `variance_contribution` and `scenario_research` absent from skills, reference docs, and registry (the surviving `clocklike` mentions are the superforecasting skill's own Tetlock regularity axis, deliberately distinct from the renamed triage labels) |
| Skill currency | 7 skills reference scenario terms. The three touching changed semantics carry their enrichments (`scenario-planning` steps 14/17, `superforecasting` audit trail, `company-research-flash` valuation step). The other four reference unchanged contracts: `company-research-deep` uses `scenario_build` (SKILL.md:207), `cmp-term-structure` uses `scenario_from_cmp_indices`/`contract_price_coherence` (SKILL.md:24,86), `metacognition` and `mcp-tool-review` cite tools incidentally (SKILL.md:149, :41). No gaps |

### Operator decision PR-13 — `scenario_full`: narrow vs. remove

Current state: the one-call Tetlock batch — triage, inline Fermi/outside
view, quantification, synthesis, and an inline Chermack assessment — at
`hkask_mcp_scenarios.rs:432-545`, request `FullPipelineRequest`
(`requests.rs:184-208`). F14 holds at current lines: the "assessment"
stage is a pass-through of five optional caller metrics
(`perspective_count`, `strategies_generated`, `strategies_implemented`,
`learning_events`, `has_early_warning_indicators` — omitted ones reported
as unreported, never zero; pinned by
`scenario_full_reports_unreported_metrics_not_measured_ones`,
`tests/tool_behavior.rs:1562-1589`). Blast radius (grep, 27 hits): the
handler, its request type, one test, the crate README row,
`scenarios.md` (tool list, diagram, delegation note),
`reference/mcp-servers/README.md:107`, and an aside in
`registry/templates/superforecasting/README.md:54`. **No skill instructs
calling it.**

- **(a) Narrow to the deterministic core:** the tool becomes a pure
  deterministic batch — triage + quantify + sensitivity ranking over
  caller-supplied events. Deletes the inline calibrate/synthesize/assess
  stages (each has a staged tool that persists and is revisitable), the
  caller-metrics request fields, and the metrics test. The user keeps a
  one-call raw-events → quantified-tree entry; the surface stays 20.
- **(b) Remove:** deletes the handler (~113 lines),
  `FullPipelineRequest`, the test, the README row, the `scenarios.md`
  entries, the reference-README sentence, the superforecasting README
  aside, and the pin entry (surface 20 → 19). The staged pipeline
  (`scenario_triage` → `scenario_calibrate` → `scenario_quantify` →
  `scenario_synthesize`) becomes the only path.

Recommendation: **(b) remove.** Zero skill callers, a live description
that already steers to the staged tools, and a deterministic core that
is one `scenario_triage` plus one `scenario_quantify` call. The
functional loss is a convenience no current workflow uses. Effort: S
either way. Depends: none.

**Decided (operator, 2026-10-06): remove — executed the same day.**
Deleted: the handler (`hkask_mcp_scenarios.rs`), `FullPipelineRequest`
(`requests.rs`), the metrics test (`tool_behavior.rs`), the crate
README row, the `scenarios.md` entries (tool list, diagram node and
edges, table row, key-paths bullet, DIAG-RF-005 record), the
reference-README sentence, the superforecasting README aside, and the
pin entry (renamed `tool_surface_is_exactly_19_registered_tools`).
Receipts: 39 scenarios tests green (the 19-pin passing against the
regenerated `TOOL_NAMES`), scoped clippy + machete clean, and the
full-repo sweep showing `scenario_full`/`FullPipelineRequest` only in
these research docs. The live 19-tool surface is pending the
operator's rebuild+restart.

### Operator decision PR-15 — portfolio event-exposure report

Unchanged verdict, changed facts: still no consumer (F23 — the MAIA
post's portfolio line is the demand hypothesis, not demand), but the
join keys now exist — PR-01's project record and PR-09's recorded
scenario fields on equity forecasts (`scenario_project_id`,
`scenario_tree`, `fused_volatility`) make the bridge a join, not a new
data model. What it would build: a portfolio-level event-exposure
report — which holdings' scenario trees share which events, per-event
marginals, event-risk concentration across the book. Seam: portfolio
ledger × companies forecast records × scenarios store. Effort drops
M-L → M. Depends: PR-01 ✓, PR-09 ✓, operator demand.

Decision: stay deferred (recommended — no consumer has asked) or
commission it (name a portfolio and a scenario project; it becomes a
slice). Building it now would add surface with no caller — the
essentialist bar that deferred it still holds.

**Decided (operator, 2026-10-06): stay deferred.**

### Remaining queue (updated 2026-10-06, decision pass)

- PR-13 decided — remove; executed the same day (pin 20 → 19, green).
- PR-15 decided — stay deferred.
- `c2a23bc8`, `818afff3`, `5e1db9ad` scored achieved on the operator's
  confirmation (Brier 0.16 / 0.20 / 0.023); curator-memory ingestion
  verification and acknowledgment are the post-turn receipt.
- The live 19-tool surface is pending the operator's rebuild+restart.
- The spreadsheet capability plan executes in a parallel stream (goal
  `11616a31`); its in-flight tree edits are that stream's.
- The PR-13 execution edits (source, tests, docs) are uncommitted and
  ride the next operator commit.
