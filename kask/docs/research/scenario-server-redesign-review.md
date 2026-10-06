---
title: "Scenarios MCP Server — From-Scratch Design Review (Plan and Coverage)"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - scenario-server-redesign-findings.md
  - scenario-server-redesign-proposals.md
  - scenario-server-redesign-improvement-plan.md
  - ../../../lean/event_tree_marginalization.lean
---

# Scenarios MCP Server — From-Scratch Design Review: Plan and Coverage

**Scope:** `kask/mcp-servers/hkask-mcp-scenarios` (19 registered tools), its
data model, the two companies-server consumers, the two prediction-markets
producers, and every dependent skill (discovered empirically, below).
**License:** breaking changes permitted; every proposal enumerates the path
it replaces and what it deletes. **Deliverables are documents only** — no
implementation, no commits.

## Method

Each phase ran as one PDCA improvement-kata loop (hypothesis FINER-gated →
current condition grounded at file:line → target condition → experiment →
verification). Three goal tags apply throughout: **fidelity** (to the
reference models), **graph-efficiency** (the computation graph), and
**integration** (with the other built-in servers and skills).

### Reference models (verified, not presumed)

| Model | Verification | Role in the review |
|---|---|---|
| MAIA, "Time Horizons, Expected Events and…" (operator corpus, `zk-reference/maia-corpus/137682783.time-horizons-expected-events-and.html`, "[DRAFT] Scenario Process & Tools" section) | Read in full during Phase 0/2 | The **data model's source**: event composition (name; yes/no question with deadline; path dependence; probability; basis in "technical feasibility or scaling/distribution"), four scenario types, three time horizons, the certainty tiers (`hkask_forecast.rs:170-174` names MAIA), and a portfolio-level use no tool implements |
| Schwartz, *The Art of the Long View* (1991) | `onto_anchor` → derived `scenario_planning` (ruling 2026-09-25) | Focal question, driving forces, 2×2 narratives, implications, indicators |
| Tetlock & Gardner, *Superforecasting* (2015) | `onto_anchor` → derived `superforecasting` (ruling 2026-09-25) | Triage, Fermi, outside view, Bayesian updating, dragonfly-eye, Brier |
| Chermack, *Scenario Planning in Organizations* (2011) | `onto_anchor` → derived `scenario_planning` (ruling 2026-09-25); phase/chapter mapping asserted at `assess.rs:119-199,1845-1851` | Five-phase project assessment |
| Brier (1950) | derived `brier_score` (ruling 2026-09-18) | Scoring |

**MAIA evidence note:** the MAIA corpus *does* outline a scenario process
(the "Time Horizons" post), so no evidence gap is recorded for it — but it
is an **unregistered** reference model: the `scenario_planning` anchor
ruling cites only Schwartz + Chermack, and `scenario-planning/SKILL.md:14-19`
omits it (finding F22, proposal PR-05).

### Phase hypotheses (FINER-gated, one PDCA cycle each)

| Phase | Hypothesis | F | I | N | E | R | Verdict |
|---|---|---|---|---|---|---|---|
| 0 Inventory | The tool surface and dependent skills are exhaustively enumerable from source + live registration + grep | 10 | 10 | 8 | 10 | 10 | **Held** — 19 tools reconciled by `lisp_eval`; 7 skills found by grep (2 more than the 3 expected) |
| 1 Primitives | A primitive model derived from the four reference models predicts the server's shape with named gaps | 9 | 10 | 8 | 10 | 10 | **Held** — 12 primitives; 2 absent (project record; narratives/indicators) |
| 2 Fidelity | Each tool's deviation from the primitive model is evidenced at file:line | 9 | 10 | 7 | 10 | 10 | **Held** — 24 findings, all cited |
| 3 Integration | Each candidate integration passes or fails the essentialist Exist/Surface/Contract gates | 8 | 10 | 7 | 10 | 10 | **Held** — 10 verdicts (3 add, 4 already exists, 1 fix, 2 deferred) |
| 4 Formal | The event-tree invariants are provable in core Lean under stated assumptions, and the finite checks pass | 9 | 9 | 8 | 10 | 10 | **Held** — 8 theorems, `lean_check` exit 0, no `sorryAx`; 5 `lisp_eval` checks true |
| 5 Proposals | The proposal set covers every finding with required fields | 9 | 10 | 7 | 10 | 10 | **Held** — 16 proposals, each with deletions enumerated |
| 6 Plan | The proposals sequence into dependency-ordered verifiable slices | 9 | 10 | 7 | 10 | 10 | **Held** — 6 slices + 2 deferred items |

## Phase 0 — Coverage table

Live surface via `list_mcp_tools` (server id `scenarios`): **19 tools**,
pinned by `tool_surface_is_exactly_19_registered_tools`
(`hkask_mcp_scenarios.rs:1964-1969`). Count reconciliation executed in
`lisp_eval`: enumerated 19 = reviewed 19, name-set membership verified →
`true`.

### Server tools (19) → phase of scope

| Tool | Handler (`hkask_mcp_scenarios.rs`) | Phase |
|---|---|---|
| `scenario_status` | L298-392 | 2 |
| `scenario_full` | L407-517 | 2 |
| `scenario_from_markets_set` | L541-578 | 2, 3 |
| `scenario_from_cmp_indices` | L608-673 | 2, 3 |
| `contract_price_coherence` | L697-750 | 2, 3 |
| `scenario_cross_validate` | L759-820 | 2 |
| `scenario_frame` | L832-861 | 1, 2 |
| `scenario_frame_document` | L873-916 | 1, 2 |
| `scenario_brainstorm` | L928-1018 | 1, 2 |
| `scenario_build` | L1032-1139 | 1, 2 |
| `scenario_quantify` | L1152-1205 | 1, 2, 4 |
| `scenario_propagate` | L1219-1258 | 1, 2, 4 |
| `scenario_update` | L1275-1317 | 1, 2 |
| `scenario_score` | L1329-1435 | 1, 2, 4 |
| `scenario_calibrate` | L1447-1566 | 1, 2 |
| `scenario_synthesize` | L1578-1618 | 1, 2 |
| `scenario_calibration` | L1630-1708 | 1, 2 |
| `scenario_triage` | L1720-1751 | 1, 2 |
| `scenario_assess` | L1763-1861 | 1, 2 |

### Cross-server surface

| Tool | Server | Location | Phase |
|---|---|---|---|
| `scenario_analysis` (Schwartz 2×2 valuation) | companies | `hkask-mcp-companies/src/tools/analytics.rs:217-499` | 1, 2, 3 |
| `scenario_impact_valuation` (tree → DCF deltas) | companies | `hkask-mcp-companies/src/tools/valuation.rs:536-751` | 1, 2, 3 |
| `calibrate_forecast` (four-scenario distribution) | companies | companies server | 1, 3 |
| `market_match` / `market_lookup` (records in) | prediction-markets | — | 3 |
| `market_cmp_indices` (CMP indices in) | prediction-markets | — | 3 |
| `hkask-graph-widget` (`viz: event_tree` re-propagation) | zed-side viz | `crates/hkask-graph-widget/src/propagate.rs` | 2, 3 |
| `hkask-scenarios-widget` (`viz: scenarios` dashboard) | zed-side viz | `crates/hkask-scenarios-widget/src/block.rs:19-22` | 2 |

### Dependent skills (grep-verified over `.agents/skills/**` and `kask/registry/templates/**`)

| Skill | Relationship | Evidence |
|---|---|---|
| `scenario-planning` | Primary consumer — 14 of 19 tools; 6 registry templates carry the Schwartz narrative phases | `SKILL.md:48-141`; templates at `kask/registry/templates/scenario-planning/` |
| `superforecasting` | Consumer — `scenario_quantify` (stage 3), `scenario_update` (stage 4), `scenario_calibrate` (stage 6), `scenario_score` (resolution), `scenario_triage` (stage 0 cross-check), `scenario_calibration` (prior curve) | `SKILL.md:17-26,54-138`; conformance contract `kask/registry/templates/superforecasting/README.md:50-76` |
| `cmp-term-structure` | Consumer — `scenario_from_cmp_indices`, `contract_price_coherence` | `SKILL.md:22-29,86-90` |
| `company-research-deep` | Consumer — `scenario_build` (IMAGINE stage scaffold) | `SKILL.md:8-9,207-208` |
| `company-research-flash` | Consumer — `scenario_impact_valuation` (critical-factor tree + impact mappings) | `SKILL.md:8-9,106-107,117-118` |
| `metacognition` | Boundary reference — names `scenario_calibration` as a *different reference class*, explicitly not its calibration | `SKILL.md:64-65,149-150` |
| `mcp-tool-review` | Meta-reference — the anchor registry names the scenario tools' anchors | `SKILL.md:39-42` |

**Coverage claim:** every one of the 19 server tools, both companies
consumers, both prediction-markets producers, both viz widgets, and all 7
skills appears in at least one phase's scope in
`scenario-server-redesign-findings.md`. The review is complete when read
with the findings, proposals, and plan documents.

## Execution report — ran vs. proposed

**Ran (oracle-verified receipts in this session):**

- Live surface enumeration (`list_mcp_tools`) + source registration walk
  (grep) + count reconciliation (`lisp_eval` → `true`).
- Full source read of the server crate (all 8 source files), the math crate
  (`hkask_forecast.rs` outline + `marginalize`/`certainty_tier`/Brier
  implementations), both companies consumers' handlers, the widget parsers,
  and all 7 dependent skills' SKILL.md files.
- Reference-model verification: `onto_anchor` on `scenario planning` and
  `superforecasting` (both derived tier); MAIA corpus read (the "Time
  Horizons" post's scenario-process section, in full).
- Phase 4 formal checks: `lean_check` on
  `kask/lean/event_tree_marginalization.lean` — **exit 0**, 8 theorems, no
  `sorryAx` (axiom receipts in the findings doc); 5 `lisp_eval` finite
  checks — all `true`.
- Coverage reconciliation `lisp_eval` → `true`.

**Proposed, not run (by design — the deliverable is documents):**

- Implementation of any proposal (PR-01…PR-16). Nothing was implemented;
  nothing was committed.
- Live probes of the running server (the review is static + formal; the
  property layer `property_tests.rs:293-440` already pins the
  marginalization against two independent oracles).
- The MAIA anchor registration (PR-05) requires an operator ruling —
  proposed, not executed.
- `scenario_full` narrowing (PR-13) and the portfolio event-exposure report
  (PR-15) are operator decisions — proposed with the essentialist bar
  stated, not decided.

**Skill battery:** loaded and followed — `program-manager`,
`refactor-architecture`, `mcp-tool-review`, `kata-improvement`,
`hypothesis-framer` (method), plus `lean-prover` discipline for Phase 4.
Applied from principle without a load (context budget): `essentialist`
(Exist/Surface/Contract applied in Phase 3), `pragmatic-semantics` (D/P
routing labels), `deep-module` (deletion test), `coding-guidelines`
(surgical-scope discipline on the proposals), `code-review` (IS/OUGHT
adjudication on each proposal), `skill-maintenance` (dependent-skill audit
lens), `grill-me`/`pragmatic-cybernetics` (reference-model stress and
feedback-loop checks). No skill in the battery failed to load.
