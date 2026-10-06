---
title: "Scenarios MCP Server — From-Scratch Design Review (Proposal Set)"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - scenario-server-redesign-review.md
  - scenario-server-redesign-findings.md
  - scenario-server-redesign-improvement-plan.md
---

# Proposal Set — Phase 5

Fields: **id** · **component** · **goal tags** (fidelity / graph-efficiency /
integration) · **evidence** (file:line, from the findings doc) · **breaking
changes** (enumerated: the path each replaces and what it deletes — a
proposal that only adds is incomplete) · **effort** (XS/S/M/L) ·
**depends**. Every proposal that specifies code was run through code-review
IS/OUGHT adjudication (each finding states what IS, cited, and what OUGHT
to be, anchored to a reference model) and coding-guidelines (surgical
scope; no speculative generality). Nothing here is implemented.

---

## PR-01 — Scenario Project record

- **Component:** `hkask-mcp-scenarios` — new project store (alongside
  `ForecastStore`, `store.rs` pattern) + `scenario_frame_document` /
  `scenario_assess` handlers.
- **Tags:** fidelity (Chermack's unit of assessment), integration (kanban,
  companies join).
- **Evidence:** F1, F11, F22 — `requests.rs:285-306` (caller-recalled
  metrics), `hkask_mcp_scenarios.rs:1772-1775` (global curve),
  `:873-916` (framing doc returned, never stored),
  `scenario-planning/SKILL.md:110-115` (agent-minted identity).
- **What it builds:** a durable project entity: id, subject, framing
  document, tree snapshots, assessment history, forecast-journal linkage —
  the missing primitive P8. `scenario_assess` reads the project and derives
  event/dependency counts from the stored tree; the framing document's
  fields score Chermack Phase 1 directly (replacing the `perspective_count`
  proxy, F11).
- **Breaking changes:** `scenario_assess`'s `project_id` becomes a real
  key — assessing an unknown project is `not_found` naming it (today any
  string is accepted). The caller-recalled metrics in `AssessRequest`
  (`requests.rs:290-305`) become *overrides* (derived-from-project is the
  default path); the recall burden — the thing being replaced — is deleted
  from the skill (`scenario-planning/SKILL.md:138-141` re-written to read
  the project). `scenario_frame_document` gains storage: its output is no
  longer ephemeral.
- **Effort:** M. **Depends:** none (PR-03, PR-07, PR-09, PR-11 build on it).

## PR-02 — Score the marginal, not the prior

- **Component:** `hkask-mcp-scenarios` — `scenario_score` handler +
  `score_forecast` (`math.rs:355-406`).
- **Tags:** graph-efficiency, fidelity (Tetlock: score the forecast you
  actually made).
- **Evidence:** F2 — `hkask_mcp_scenarios.rs:1347,1405` score
  `event.probability`; the marginal lives at `types.rs:550` and is what
  propagation produces (`compose.rs:398-453`).
- **What it builds:** `scenario_score` recomputes the tree server-side
  (`build_event_tree` already exists, `math.rs:133-223`) and scores each
  node's `marginal_probability`; the journal stores the marginal with a
  field marking it as such.
- **Breaking changes:** `StoredForecastRecord.probability` semantics change
  (prior → marginal) — schema_version bump (`types.rs:347`); the
  calibration curve and domain-bias correction consequently learn from
  marginals (the old prior-based learning is deleted, deliberately).
  Agents that manually copied marginals into `event.probability` before
  scoring (the workaround being replaced) stop needing to.
- **Effort:** S. **Depends:** none.

## PR-03 — Subject-scoped assessment calibration

- **Component:** `hkask-mcp-scenarios` — `scenario_assess` handler.
- **Tags:** fidelity (Chermack Phase 5 evidence must be the project's own).
- **Evidence:** F1 — `hkask_mcp_scenarios.rs:1772-1775` reads the global
  store; `filtered_by_subject` already exists (`store.rs:234-246`) and
  `scenario_calibration` already offers the filter (`:1630-1640`).
- **What it builds:** assess uses `store.filtered_by_subject(&req.subject)`.
- **Breaking changes:** replaces the global-curve read; deletes
  cross-project contamination of the Phase-5 score. No wire change.
- **Effort:** XS. **Depends:** none (superseded by full project scoping
  under PR-01, but valuable standalone).

## PR-04 — Triage label fix

- **Component:** `hkask-mcp-scenarios` — `triage_question`
  (`assess.rs:612-663`).
- **Tags:** fidelity (Tetlock's clocklike/cloudlike is process regularity).
- **Evidence:** F4 — the "clocklike" label at `assess.rs:634-639` is
  applied to well-specified questions.
- **What it builds:** specification-quality labels
  (`well_specified` / `goldilocks` / `needs_refinement`) with the
  regularity axis either dropped or added as a separate dimension.
- **Breaking changes:** replaces the `difficulty` strings; consumers read
  them — `superforecasting/SKILL.md:54-55` (stage-0 cross-check) and the
  skill's Phase 1 (`scenario-planning/SKILL.md:50-53`) are updated in the
  same change; the old labels are deleted, not deprecated.
- **Effort:** XS. **Depends:** none.

## PR-05 — Register the MAIA anchor

- **Component:** ontology registry (operator ruling) + `scenario-planning`
  SKILL.md + server docs.
- **Tags:** fidelity (a real reference model must be a registered one).
- **Evidence:** F5, F16 — the MAIA "Time Horizons" post
  (`zk-reference/maia-corpus/137682783.time-horizons-expected-events-and.html`)
  is the verbatim source of `ScenarioType` (`types.rs:165-174`),
  `TimeHorizon` (`:142-149`), `basis` (`:238-242`), `CertaintyTier`
  (`hkask_forecast.rs:170-184`); the anchor ruling cites only
  Schwartz+Chermack; `scenario-planning/SKILL.md:14-19` omits MAIA.
- **What it builds:** an `onto_anchor` derived entry for the MAIA
  event-based scenario template (operator ruling required — ask-first);
  MAIA added to the skill's reference models; the `basis` doc comment
  (`types.rs:239-241`) rewritten to name both semantics (MAIA estimate
  basis; bridge provenance strings).
- **Breaking changes:** replaces the incomplete anchor ruling (amended, not
  deleted); deletes the implicit attribution — `scenario_build`'s framework
  string (`:1117`) becomes citation-accurate. No code behavior change.
- **Effort:** XS (operator decision + docs). **Depends:** none.

## PR-06 — Stale-reference cleanup

- **Component:** `hkask-mcp-scenarios` (brainstorm default string, README)
  + `scenario_build` description + superforecasting README.
- **Tags:** fidelity (stale comments are active misinformation — `.rules`).
- **Evidence:** F3, F6, F7, F24 — `hkask_mcp_scenarios.rs:931`
  (`scenario_research` in a live tool response), `README.md:17-43` (two
  removed tools listed, five live tools missing), the `research_text`
  description vs `context` wire field (`requests.rs:18`),
  `superforecasting/README.md:142-143` (false `scenario_update` journaling
  claim).
- **What it builds:** corrected strings and tables.
- **Breaking changes:** deletes the stale references outright (no
  compatibility shim); the README tool table is regenerated from the live
  19-tool surface so it cannot drift silently again.
- **Effort:** XS. **Depends:** none.

## PR-07 — Durable tree cache via the project record

- **Component:** `hkask-mcp-scenarios` — tree cache + project store.
- **Tags:** graph-efficiency (the coherence default survives restart).
- **Evidence:** F9 — `hkask_mcp_scenarios.rs:1158-1159` (in-memory
  `Mutex<Option<EventTree>>`); `requests.rs:101-104` documents the cached
  default; D28 already gives the server a durable data dir
  (`mcp/scenarios/`).
- **What it builds:** the last-quantified tree persists under the project
  record; `contract_price_coherence`'s `tree_implied` default reads it.
- **Breaking changes:** replaces the process-local cache (deleted); the
  default's failure mode changes from "silently absent after restart" to
  "present or `not_found` naming the project" — an honest-error change.
- **Effort:** S. **Depends:** PR-01.

## PR-08 — Evidence-posterior recomputation as a server primitive

- **Component:** `hkask-forecast` (math promotion) + `hkask-mcp-scenarios`
  (tool) + `crates/hkask-graph-widget` (delegation).
- **Tags:** graph-efficiency, integration (the UI's capability becomes the
  surface's).
- **Evidence:** F10 — the widget's `recompute_posteriors`
  (`crates/hkask-graph-widget/src/propagate.rs:245-370`: backward
  inference, soft/hard evidence, polytree detection, fixpoint sweeps)
  vs the server's forward-only `propagate_prior_update`
  (`compose.rs:398-453`); the shared-source-of-truth precedent is
  `hkask_forecast.rs:144-147`.
- **What it builds:** the posterior math moves to `hkask-forecast`; a
  scenarios tool exposes evidence-posterior recomputation (soft/hard
  evidence on any node, both directions); the widget delegates.
- **Breaking changes:** the tool count pin moves 19 → 20
  (`hkask_mcp_scenarios.rs:1964-1969`) — an intentional surface change;
  the widget's local forward-marginal duplication is deleted (it already
  delegates `marginalize`; the posterior engine follows); the docs
  reference tool count and DIAG-RF-005 update in the same change.
- **Effort:** M. **Depends:** none (coordinate the widget crate).

## PR-09 — Equity forecast scenario fields

- **Component:** `hkask-mcp-companies` — forecast records
  (`forecast_persist`, `calibrate_forecast`) + docs.
- **Tags:** integration (the recorded join), fidelity (one named Schwartz
  surface set).
- **Evidence:** F13, F16, P10 — `scenario_impact_valuation`
  (`valuation.rs:536-751`) consumes the tree ad hoc per call; the equity
  forecast record carries no tree identity, impact-mapping reference, or
  fused volatility; three "Schwartz scenario" semantics coexist (F13).
- **What it builds:** explicit scenario fields on the equity forecast
  record: project/tree identity, node-marginal snapshot, impact-mapping
  reference, fused volatility — the join becomes recorded instead of
  agent-mediated. A docs section names the three Schwartz surfaces and
  their distinct contracts.
- **Breaking changes:** the forecast record schema gains fields
  (additive, versioned); the unrecorded join (the manual convention being
  replaced) is deleted from the flash skill's instructions
  (`company-research-flash/SKILL.md:106-107` re-written to cite the
  recorded fields).
- **Effort:** M. **Depends:** PR-01 for the project id (standalone with a
  tree hash if landed first).

## PR-10 — Workbook presentation for impact valuation

- **Component:** `hkask-mcp-companies` — `scenario_impact_valuation`.
- **Tags:** integration (editable scenario × delta × valuation grids).
- **Evidence:** I1 — the path grid already exists in the tool's output
  (`valuation.rs:536-751` enumerates 2^N paths with probabilities and DCF
  results); the spreadsheet server has no template mechanism (grep: zero
  hits); `portfolio_what_if`'s `WorkbookWhatIf` presentation is the shipped
  pattern to mirror.
- **What it builds:** an opt-in `presentation: "WorkbookWhatIf"` mode
  publishing the path grid as an editable workbook through the existing
  spreadsheet machinery.
- **Breaking changes:** replaces the agent-manual transcription of grids
  into spreadsheets (the current path — deleted as a documented workaround);
  opt-in, so no default output changes.
- **Effort:** S-M. **Depends:** none.

## PR-11 — Scenario-type templates + kanban project scaffold

- **Component:** `scenario-planning` skill + `kask/registry/templates/` (no
  server change).
- **Tags:** integration (kanban), fidelity (MAIA's own template inputs).
- **Evidence:** I2 — MAIA's four types (`types.rs:165-174`) and its
  template inputs (Type, Subject, Events, Order/Path Dependence, Future
  Considerations — the "Time Horizons" post); kanban goals are Brier-scored
  (`kanban_goal_create` prediction → `kanban_goal_score`); the skill's
  project identity is agent-minted (F22).
- **What it builds:** Phase 1 scaffolds a board (per-scenario goal with an
  intake prediction; per-event tasks with deadlines); per-type framing
  defaults as registry templates (company update / company analysis /
  emerging economic / economic potential — MAIA's four, with MAIA's own
  note that types 3–4 subjects may be "a country or an industry or the path
  of a specific technology").
- **Breaking changes:** replaces the agent-minted `forecast_id` convention
  (`scenario-planning/SKILL.md:110-115` — deleted, the durable project id
  from PR-01 takes over); the skill's Phase 1/5 steps are re-written in the
  same change.
- **Effort:** S (skill work). **Depends:** PR-01 (soft — can land with
  agent-minted ids first).

## PR-12 — Sequence-advisory DAG fix

- **Component:** `hkask-mcp-scenarios` — `expected_predecessor`
  (`hkask_mcp_scenarios.rs:137-147`).
- **Tags:** graph-efficiency (a convention that warns on legitimate paths is
  a broken feedback signal).
- **Evidence:** F15 — the single chain; `superforecasting/SKILL.md:89`
  documents the warn as "expected noise".
- **What it builds:** the predecessor relation becomes the set of legitimate
  entries: `scenario_quantify` accepts `scenario_build` OR
  `scenario_from_markets_set` OR `scenario_from_cmp_indices` OR a
  superforecasting-supplied tree; `scenario_triage` and `scenario_status`
  remain free.
- **Breaking changes:** replaces the linear chain (deleted); the
  "expected noise" documentation in the superforecasting skill is deleted
  with the noise it described.
- **Effort:** XS. **Depends:** none.

## PR-13 — `scenario_full` narrowing (operator decision)

- **Component:** `hkask-mcp-scenarios` — `scenario_full` (`:407-517`).
- **Tags:** graph-efficiency (deep-module deletion test).
- **Evidence:** F14 — the inline assessment consumes caller metrics
  (`requests.rs:166-175`) inside a batch whose description steers to the
  staged tools; the deterministic core (triage+quantify+sensitivity) is the
  part that earns a batch.
- **Options (functional, for the operator):** (a) narrow to the
  deterministic core — the user keeps one-call triage+quantify and loses
  the mini-assessment (which needs caller metrics anyway); (b) remove the
  tool — the user loses the one-call entry and the staged pipeline remains
  (the pin moves 19 → 18).
- **Breaking changes:** either option deletes surface; option (b) deletes
  the tool, its handler, and its pin entry; option (a) deletes the
  assessment stage from the output shape.
- **Effort:** S. **Depends:** none. **Status: operator decision.**

## PR-14 — `variance_contribution` rename

- **Component:** `hkask-mcp-scenarios` — `EventTreeNode`
  (`types.rs:547-557`), `math.rs:186-187,345`.
- **Tags:** fidelity (names are contracts).
- **Evidence:** F8 — the field is a certainty distance; the ranking
  inverts it; consumers (the `graph` viz block, docs) read the name.
- **What it builds:** rename to `certainty_distance` (or emit both during
  one transition); document the sensitivity proxy's dependency-blind
  limits at the emission site.
- **Breaking changes:** replaces the emitted field name; the viz block
  contract (`scenario_quantify`'s render instructions, `:1164-1205`)
  updates in the same change; the old name is deleted, not aliased.
- **Effort:** XS. **Depends:** none.

## PR-15 — Portfolio event-exposure report (deferred)

- **Component:** `hkask-mcp-portfolio` + scenarios bridge.
- **Tags:** integration (MAIA's portfolio link).
- **Evidence:** F23 — the MAIA post's portfolio use ("events could function
  as a forward looking basis for thinking about risks in the portfolio");
  no tool aggregates events to portfolio level; no current consumer.
- **What it would build:** a portfolio-level view of which holdings'
  scenario trees share which events (concentration of event risk).
- **Breaking changes:** n/a until built — the essentialist bar (no
  consumer) is why it is deferred.
- **Effort:** M-L. **Depends:** PR-01 + operator demand. **Status:
  deferred, operator decision.**

## PR-16 — Schwartz-surface naming unification

- **Component:** docs (`scenarios.md`, `companies.md`, skill frontmatter).
- **Tags:** fidelity (three distinct contracts, three distinct names).
- **Evidence:** F12, F13 — `hkask_mcp_scenarios.rs:1852` (integration
  string conflates Schwartz narratives with event trees); the three
  Schwartz semantics (companies 2×2, skill narratives, calibrate_forecast's
  four-scenario distribution).
- **What it builds:** a docs section naming the three surfaces and their
  distinct contracts; the assess integration string corrected.
- **Breaking changes:** replaces the conflated string (deleted); no code
  behavior change.
- **Effort:** XS. **Depends:** none.

---

## Coverage check

Every Phase-2 finding maps to a proposal or an explicit no-action record:
F1→PR-01/03, F2→PR-02, F3→PR-06/01, F4→PR-04, F5→PR-05, F6→PR-06, F7→PR-06,
F8→PR-14, F9→PR-07, F10→PR-08, F11→PR-01, F12→PR-16, F13→PR-16/09,
F14→PR-13, F15→PR-12, F16→PR-05/09, F17→noted (below the actionability
bar), F18→noted (durability cost, accepted), F19→watched (documented seam),
F20→watched (unreachable through the public path), F21→no action,
F22→PR-01/11, F23→PR-15, F24→PR-06. Every Phase-3 verdict maps: I1→PR-10,
I2→PR-11/01, I3→PR-07, I4→PR-09, I5→PR-15, I6→PR-12, I7/I8/I9→no action,
I10→PR-08.
