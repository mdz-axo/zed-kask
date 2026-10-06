---
title: "Scenarios MCP Server — From-Scratch Design Review (Findings)"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - scenario-server-redesign-review.md
  - scenario-server-redesign-proposals.md
  - scenario-server-redesign-improvement-plan.md
  - ../../../lean/event_tree_marginalization.lean
---

# Findings — Phases 1–4

All citations are from this session's read/grep receipts over the tree at
review time. Line numbers drift; the cited behavior is what was read.

## Phase 1 — Primitives derivation (the from-scratch model)

Derived from the four verified reference models *before* scoring the
implementation against it. "Equity carriage" answers the review question:
which scenario fields does a company financial forecast explicitly carry?

| # | Primitive | Properties / methods | Reference-model step | Exists? | Equity carriage |
|---|---|---|---|---|---|
| P1 | **FramingDocument** (project charter) | focal question, decision at stake, horizon, action deadline, scope in/out, stakeholders, use case, success criteria, constraints, assumptions, exploration prompts; methods: frame (converse), document (structure) | Chermack Phase 1; Schwartz step 1 | **Yes** — `types.rs:65-106`; produced by `scenario_frame`/`scenario_frame_document` | No — never stored, never read by valuation |
| P2 | **BinomialEvent** | id, name, yes/no question, deadline, horizon, type, subject, probability, basis, Fermi sub-questions, base rate, reference class, Brier, update count; methods: triage, validate, calibrate, update, resolve | MAIA event composition (all 5 parts); Tetlock question discipline | **Yes** — `types.rs:220-257`; validation `types.rs:654-708` | Only via the tree (P4) |
| P3 | **DependencyEdge** (conditional table) | parent ids + bitmap-ordered conditionals (length 2^n); methods: validate, marginalize | MAIA "Order or Path Dependence"; Bayesian-network CPT | **Yes** — `types.rs:273-279`; multi-group noisy-OR extension documented `math.rs:55-60` | Via the tree |
| P4 | **EventTree** (quantified scenario) | nodes (event + marginal + paths + certainty distance), roots, topo order, joint = P(all events occur); methods: quantify, propagate, compose (markets/CMP), score | MAIA template; Tetlock inside view | **Yes** — `types.rs:561-573`; marginalization single-source `hkask_forecast.rs:144-168` | **Yes** — `scenario_impact_valuation` consumes it (`valuation.rs:536-751`) |
| P5 | **Perspective + Synthesis** | source, probability, Fermi, base rate, rationale, historical Brier; method: inverse-Brier weighted aggregate + disagreement | Tetlock dragonfly-eye | **Yes** — `types.rs:301-336`, `assess.rs:429-519` | No |
| P6 | **ForecastRecord + journal** | per-event records keyed `{forecast_id}:{event_id}`; append-only journal + snapshot; methods: record, resolve, Brier | Tetlock record/score | **Yes** — `types.rs:342-363`, `store.rs:23-246` | Separate loop (`forecast_persist`, companies) with no join |
| P7 | **CalibrationObservation + curve** | 10 bins, hit rate vs expected, weighted bias (≥5/bin), isotonic second channel | Tetlock calibration | **Yes** — `assess.rs:524-606`, `hkask_forecast.rs:279-368` | No |
| P8 | **ScenarioProject** (the spine) | id, framing document, tree(s), narratives, strategies, indicators, assessments, journal linkage; method: assess the project *from its own record* | Chermack's unit of assessment (all 5 phases) | **No** — `scenario_assess` takes caller-recalled metrics (`requests.rs:285-306`); the framing document is never stored; the skill mints `forecast_id` agent-side (`scenario-planning/SKILL.md:110-115`) | No |
| P9 | **Narrative / Implications / Indicators** | two-axis quadrant narratives, robust/contingent strategies, observable early-warning indicators | Schwartz steps 5–8 | **No server primitive** — skill templates only (`scenario-planning` Phase 2b, 6 `.j2` files); the server's only trace is the `has_early_warning_indicators` boolean (`requests.rs:305`) | No |
| P10 | **ImpactMapping** (equity bridge) | per-node yes/no DCF assumption deltas; method: enumerate 2^N paths, weight by path probability | MAIA "events drive the financial forecast" | **Yes** — on the companies server (`valuation.rs:536-751`, max 12 nodes) | **Yes** — tree + deltas + fused volatility; **not** framing, narratives, indicators, or journal linkage |
| P11 | **MarketBridge** | market records / CMP indices → root events with provenance; gates: open-status, reliability/match-confidence withholding, deadline refusal, domain-bias correction; method: coherence vs observed price | CMP term-structure program | **Yes** — `bridge.rs:217-306`, `compose.rs:62-356` | Via the tree's `cmp_provenance` |
| P12 | **CertaintyTier** | proximate ≥67% / probable 33–66% / possible <33% | MAIA three-level tier (`hkask_forecast.rs:170-174`) | **Yes** — `hkask_forecast.rs:176-184` | No |

**The from-scratch verdict:** a scenario server built today around these
primitives would keep P2–P7, P10–P12 substantially as built (the MAIA
reification is faithful), and would add the two absent primitives — **P8
(the project record)**, without which Chermack's Phase 5 assesses a
phantom, and **P9's server-side trace** (at minimum stored narratives and
indicators, or an explicit contract that they live in the skill layer
forever). The equity forecast (P10's consumer) should explicitly carry the
tree identity, the impact mappings, and the fused volatility — today the
join is agent-mediated and unrecorded (PR-09).

## Phase 2 — Fidelity review (per tool, with file:line)

### Faithful (no deviation found)

- **`scenario_quantify`** (`hkask_mcp_scenarios.rs:1152-1205` → `math.rs:133-223`):
  validates, topologically sorts (Kahn, `math.rs:226-284`), marginalizes by
  delegation to the single source of truth (`hkask_forecast.rs:144-168`),
  and emits the `graph` viz block. The property layer pins the math against
  two independent oracles (`property_tests.rs:170-273,340-440`).
- **`scenario_propagate`** (`:1219-1258` → `compose.rs:398-453`): forward
  propagation with a per-node before/after journal; writes the tree cache
  post-update (`:1225-1230`).
- **`scenario_calibrate`** (`:1447-1566`): four-stage Tetlock (Fermi
  confidence-weighted → outside-view shrinkage → inside → calibration
  feedback ≥5 resolved) plus the isotonic PAVA second channel with fit
  knots — the strongest reference-model fidelity in the server.
- **`scenario_synthesize`** (`:1578-1618` → `assess.rs:429-519`):
  inverse-Brier weighting (`1/(brier+0.01)`, climatology 0.25 default for
  missing history), normalized-stddev disagreement, dissent surfacing.
- **`scenario_cross_validate`** (`:759-820` → `bridge.rs:28-126`): per-sub-question
  divergence, 0.15 threshold, grill-me routing (`:795-803`). Index-based
  sub-question matching is documented best-effort (`bridge.rs:42-47`).
- **`scenario_from_markets_set`** (`:541-578` → `compose.rs:62-160`,
  `bridge.rs:217-306`): the gates are real — non-open markets rejected
  (`bridge.rs:224-232`), low-reliability/weak-match base rates *withheld*
  (`:244-269`), unparsable deadlines refused rather than fabricated
  (`:271-283`), measured domain-bias correction applied deterministically
  (`:234-242`).
- **`scenario_from_cmp_indices`** (`:608-673` → `compose.rs:183-356`): CMP
  provenance carried in-tree; pinned by
  `scenario_from_cmp_indices_emits_full_cmp_provenance_inside_tree`
  (`superforecast.rs:18-23`).
- **`contract_price_coherence`** (`:697-750` → `hkask_forecast.rs:681-698`):
  rejects inputs outside [0,1]; within-band is coherent, beyond-band is the
  signal.
- **`scenario_status`** (`:298-392`): resolved-without-outcome records are
  excluded from Brier with a warn, not silently counted as false
  (`:306-337`).

### Deviations (findings F1–F24)

**F1 — No project record (the central gap).** `scenario_assess` receives
caller-recalled counts (`requests.rs:285-306`) and pulls the calibration
curve from the **global** store (`hkask_mcp_scenarios.rs:1772-1775`), not
subject-filtered — cross-project contamination of Chermack Phase 5 evidence
despite `subject` being in the request. The framing document
(`scenario_frame_document`, `:873-916`) is returned, never stored.
Chermack's assessment is unanchored from the project's own record.
→ PR-01, PR-03.

**F2 — `scenario_score` scores the prior, not the marginal.** Per-event
Brier uses `event.probability` (`:1347`, journal write `:1405`) — the
caller-supplied prior field. For a dependent event the forecast being
tested is the tree marginal (`EventTreeNode.marginal_probability`,
`types.rs:550`); even passing the resolved tree's nodes scores the stale
prior. Downstream: the calibration curve and domain-bias correction learn
from priors, not from the beliefs actually propagated. → PR-02.

**F3 — `scenario_update` persists nothing.** The handler computes the
posterior and returns it (`:1287-1311`); `forecast_id` is echoed in the
output but never journaled. The superforecasting README's audit-trail claim
("scenario_update writes it", `kask/registry/templates/superforecasting/README.md:142-143`)
is false — active misinformation in a shipped skill doc. → PR-06 (doc),
PR-01 (journal linkage).

**F4 — Triage mislabels specification quality as process regularity.**
`triage_question` scores clarity/data/resolution (sound), then labels a
well-specified question "clocklike" (`assess.rs:634-639`). Tetlock's
clocklike/cloudlike axis is the question's *process regularity* — a
well-specified one-off question is not clocklike. The mislabel propagates
into `superforecasting` stage 0's cross-check. → PR-04.

**F5 — MAIA is a real but unregistered reference model.** `ScenarioType`
(`types.rs:165-174`), `TimeHorizon` (`:142-149`), `basis`
(`:238-242`), and `CertaintyTier` (`hkask_forecast.rs:170-184`) reify the
MAIA "Time Horizons" post verbatim (four types, three horizons, five-part
event composition, technical-feasibility/scaling-distribution basis). The
`scenario_build` framework string names it (`:1117`), and the methodology
block correctly cites Tetlock+Schwartz (`:1131`) — but the
`scenario_planning` anchor ruling and `scenario-planning/SKILL.md:14-19`
cite only Schwartz/Tetlock/Chermack. Not fabrication — an anchor-registry
gap. → PR-05.

**F6 — Stale references to the removed `scenario_research`.** The
brainstorm handler's default research string instructs agents to "Use
scenario_research to gather web search results first"
(`hkask_mcp_scenarios.rs:931`) — a tool that no longer exists on the 19-tool
surface. The crate README still lists `scenario_research` and
`scenario_sensitivity` and omits `scenario_propagate`,
`scenario_from_markets_set`, `scenario_from_cmp_indices`,
`contract_price_coherence`, `scenario_full` (`README.md:17-43`). → PR-06.

**F7 — Description/wire mismatch on `scenario_build`.** The tool
description references `research_text`; the wire field is `context`
(`requests.rs:18`, handler `:1038`). → PR-06.

**F8 — `variance_contribution` is a misnomer.** `|P−0.5|×2`
(`math.rs:186-187`) is a *certainty distance* (1 = most certain), not a
variance contribution (Bernoulli variance is p(1−p)). The sensitivity
ranking inverts it (`1.0 − variance_contribution`, `math.rs:345`) so the
behavior is correct — the emitted field name misleads every consumer.
The proxy itself is honest about its limits (`math.rs:336-339`) and ignores
dependency structure entirely. → PR-14.

**F9 — In-memory tree cache.** `scenario_quantify` caches the tree in a
process-local `Mutex<Option<EventTree>>` (`:1158-1159`); restart loses the
cached joint that `contract_price_coherence`'s `tree_implied` default
documents (`requests.rs:101-104`). → PR-07.

**F10 — Forward-only propagation.** The server updates a prior and
recomputes descendants (`compose.rs:398-453`); the graph widget implements
the richer primitive — backward inference from leaf evidence, soft/hard
evidence, polytree detection, fixpoint convergence
(`crates/hkask-graph-widget/src/propagate.rs:190-370`) — not exposed as a
tool. The UI can do what the MCP surface cannot. → PR-08.

**F11 — `scenario_assess` Phase-1 proxy.** Chermack Phase 1 (Preparation:
scope, stakeholders, resources) is scored by `perspective_count`
(`assess.rs:119-152`) — an exploration-flavored proxy. The FramingDocument
fields the server already types (`types.rs:65-106`) would score
preparation directly. → PR-01.

**F12 — `scenario_assess` integration string conflates Schwartz with event
trees.** "Schwartz (scenario narratives via event trees)"
(`hkask_mcp_scenarios.rs:1852`) — Schwartz narratives are the skill-side
2×2 stories; event trees are the Tetlock/MAIA quantification. → PR-16.

**F13 — Three "Schwartz scenario" semantics coexist.** (a) companies'
`scenario_analysis` Bull/Land-Grab/Cash-Cow/Bear growth×margin 2×2
(`analytics.rs:217-221`); (b) the skill's template-driven quadrant
narratives (`scenario-planning` Phase 2b); (c) `calibrate_forecast`'s
"probabilities across the four Schwartz scenarios". No shared identity or
cross-references between them. → PR-16, PR-09.

**F14 — `scenario_full` is a shallow batch with an incoherent assessment
stage.** It delegates to the same engine functions (`:497-507`) — but its
inline "assessment" consumes caller-supplied optional metrics
(`requests.rs:166-175`) inside a tool whose own description steers to the
staged tools for anything revisited. Deep-module deletion test: the
deterministic core (triage+quantify+sensitivity) earns a batch; the
mini-assessment does not. → PR-13 (operator decision).

**F15 — The sequence advisory is a single chain that warns on legitimate
entries.** `expected_predecessor` (`:137-147`) requires
frame→frame_document→brainstorm→build→quantify→calibrate→synthesize→score→calibration→assess.
`superforecasting` enters at quantify with its own tree; the market bridges
enter at from_markets_set/from_cmp_indices; the skill documents "the
advisory warn is expected noise" (`superforecasting/SKILL.md:89`). A
convention that warns on legitimate paths is a broken feedback signal.
→ PR-12.

**F16 — `basis` field has three unrelated uses.** Doc comment:
"technical_feasibility or scaling_distribution" (`types.rs:238-242`, MAIA);
market bridge: `prediction_market:{source}` (`bridge.rs:294`); CMP bridge:
`cmp_index:{method}` (`compose.rs:212-222`). The bridge uses are provenance
(valuable); the doc enum is MAIA's estimate basis. One free-text field,
three semantics. → PR-05 (document), PR-09 (provenance into the equity
join).

**F17 — Calibration curve's empty bins.** `hit_rate` is `f64::NAN` for
empty bins (`assess.rs:557`) → JSON `null`; `expected_rate` for empty bins
is the bin midpoint (`:562`) — a fabricated-looking expected for a bin
with no data. Cosmetic; surfaced rather than hidden. → noted, no proposal
(below the actionability bar).

**F18 — Unconditional snapshot persist per score.** `scenario_score`
calls `store.persist()` on every invocation (`:1427`) — a full snapshot
write per resolution event, on top of the journal threshold
(`store.rs:17,151-153`). Safe (resolution is rare); noted as a minor
efficiency cost of the durability ordering, not a defect.

**F19 — The math crate's zero-fill leniency is one seam away.**
`hkask_forecast::marginalize` zero-fills missing table entries
(`hkask_forecast.rs:163-165`, pinned deliberately by
`marginalize_short_table_contributes_zero_for_missing_entries`), while the
server's `validate` rejects wrong lengths first (`types.rs:683-695`). The
lenient path is currently shielded; any future caller that skips
validation inherits silent zero-fill. Documented seam, watched, not a
defect today.

**F20 — `compute_marginal_probabilities` defaults a missing parent to 0.0
with only a `tracing::warn`** (`math.rs:94-101`). Unreachable through the
public path (topological sort rejects unknown parents first,
`ScenarioError::UnknownParent`, `types.rs:35`), but it is the
`unwrap_or(0)` shape the `.rules` flag as a broken feedback loop if the
shield ever moves. Watched.

**F21 — Ontology anchoring is coarse.** Protocol tools → `pko:PROCEDURE`,
everything else → `dc_bibo:DATASET` (`:119-122`). Functional; the widget
carries the ontology for a future "explain this scenario" affordance
(`crates/hkask-scenarios-widget/src/block.rs:37-45`). No action.

**F22 — The skill's project identity is agent-minted.** "Mint one
project-wide `forecast_id` at first use and reuse it"
(`scenario-planning/SKILL.md:110-115`) — the spine of P8 lives in agent
convention, not server state. → PR-01, PR-11.

**F23 — MAIA's portfolio link is unimplemented.** The source post envisions
events as "a forward looking basis for thinking about risks in the
portfolio"; no tool aggregates scenario events to portfolio level. →
PR-15 (deferred — no current consumer).

**F24 — README/docs drift.** Crate README lists removed tools and misses
five live ones (F6); the docs reference's pin citation has drifted by line
number (`scenarios.md:17` cites L262,1900-1912; the pin now sits at
`:1964-1969`). The docs reference itself is accurate at 19 tools. → PR-06.

## Phase 3 — Integration review (essentialist Exist / Surface / Contract)

| # | Integration | Exist | Surface | Contract | Verdict |
|---|---|---|---|---|---|
| I1 | **Spreadsheet server** — scenario × delta × valuation grids as workbook templates | The grid data already exists (`scenario_impact_valuation` enumerates 2^N paths); what's missing is an editable presentation | The spreadsheet server has **no template mechanism** (grep: zero `template` hits in its source) — a new registry is new surface | Minimal contract: an opt-in `presentation: "WorkbookWhatIf"` mode on the existing companies tool, mirroring `portfolio_what_if`'s shipped pattern; replaces today's agent-manual transcription of grids into workbooks | **Add (opt-in presentation mode; no template registry)** → PR-10 |
| I2 | **Kata-kanban** — scenario projects as boards; per-scenario goals (Brier-scored via `kanban_goal_create` prediction); per-event tasks; named scenario *types* with templates | The project spine (P8) is missing and kanban already provides durable Brier-scored goal records | Kanban needs **no new mechanism** (boards/tasks/goals all exist); the scaffold is a skill-level pattern | MAIA's four types exist (`types.rs:165-174`); MAIA's own post specifies template inputs (Type, Subject, Events, Order/Path Dependence, Future Considerations); per-type framing defaults belong in the skill + registry templates, not the server | **Add (skill-level scaffold + per-type defaults; server gets only the project record)** → PR-11, PR-01 |
| I3 | **Prediction-markets** | Bidirectional and gated (F-series findings all pass) | — | `market_match` refusals flow into the bridge's match-confidence gates correctly | **Already exists** — fix the cache durability (PR-07) |
| I4 | **Companies** | Tree → DCF exists (`valuation.rs:536-751`); the equity forecast record carries **no** scenario fields — the join is agent-mediated and unrecorded | Additive fields on `forecast_persist`/`calibrate_forecast` records | The forecast should explicitly carry: tree/project identity, node-marginal snapshot, impact-mapping reference, fused volatility | **Add (recorded join)** → PR-09 |
| I5 | **Portfolio server** (MAIA's portfolio link) | No portfolio-level event-exposure view exists | New capability | Essentialist bar: no current consumer; MAIA's post is the demand hypothesis, not demand | **Deferred (operator decision)** → PR-15 |
| I6 | **superforecasting skill** | Healthy: the conformance contract maps stages to functions; `scenario_quantify` owns `combined_probability` | — | The only friction is the sequence-advisory noise (F15) | **Already exists** → PR-12 |
| I7 | **cmp-term-structure skill** | Healthy: D/P labels name the exact tools | — | — | **Already exists** |
| I8 | **company-research-deep/flash** | Consume `scenario_build` / `scenario_impact_valuation` respectively; the flash skill emits the tree + impact mappings in the flat format the tool accepts | — | — | **Already exists** |
| I9 | **metacognition** | References `scenario_calibration` as a *different reference class* — correct discipline, not a consumer | — | — | **No action** |
| I10 | **Graph widget** | The widget's posterior engine (backward inference, `propagate.rs:245-370`) is a richer primitive than any tool exposes | Promote the math into `hkask-forecast` (shared source of truth, matching the marginalize precedent `hkask_forecast.rs:144-147`); expose via a server tool; the widget keeps its viz role | Closes the gap between what the UI can do and what the MCP surface can do | **Add (math promotion + tool)** → PR-08 |

## Phase 4 — Formal and structural checks

### Lean spec pin — `kask/lean/event_tree_marginalization.lean`

`lean_check` result: **exit 0** (warnings only: the unused-instance linter
on four list-only lemmas). Core Lean 4.34.0, no Mathlib. Eight theorems,
each machine-checked; `#print axioms` receipts:

| Theorem | Invariant | Axioms |
|---|---|---|
| `marginalize_length` | CPT completeness — a table that marginalizes has exactly 2^\|parents\| entries (models `types.rs:684-695`) | propext, Quot.sound |
| `marginalize_some` | CPT soundness — a 2^\|parents\|-entry table marginalizes (validation neither over-accepts nor over-rejects) | propext, Classical.choice, Quot.sound |
| `single_parent_bitmap` | conditionals[0] = P(E\|¬parent), conditionals[1] = P(E\|parent) | propext |
| `two_parent_bitmap` | bit j ↔ parent j; the 2-parent marginal is the full joint enumeration | propext |
| `marginal_bounds` | parents and entries in [0,1] → marginal in [0,1] (models `types.rs:655,697-704`) | propext |
| `certainty_preserved` | mass conservation: all-ones table → marginal = 1, whatever the parents (parent independence) | propext, Quot.sound |
| `noisyOr_single` | exact 1−(1−p) = p (the Rust "within 1 ULP" is a float artifact, `math.rs:119-121`) | **none** |
| `noisyOr_bounds` | noisy-OR preserves [0,1] (models `math.rs:122-127`) | propext |

**Stated assumptions** (in the file header): the `OrdField` axioms
(standard ordered-field facts; ℝ/ℚ intended models — the theorems are
parametric, so the class fields are the assumptions, not hidden axioms);
parent independence (the factorized recursion — the same assumption the
Rust makes, whose approximation character under shared ancestry is pinned
by `property_tests.rs:293-325`); exact arithmetic (the f64 clamp at
`math.rs:109` is not modeled); and the model's `none` = the server's
validation rejection (the math crate's zero-fill leniency, F19, is
deliberately out of scope). A proof establishes the invariant under these
assumptions — not fitness to the reference models.

### Finite structural checks — `lisp_eval` (all `true`)

1. Two-parent marginal (A=0.6 bit0, B=0.3 bit1; CPT [0.1, 0.2, 0.3, 0.4])
   = 0.22 — the four assignment terms sum exactly.
2. CPT length = 2² = 4 for two parents.
3. Noisy-OR single channel: 1−(1−0.25) = 0.25 (exact).
4. Joint (all-true) semantics: A=0.6 → C with P(C|A)=0.5 gives joint 0.3;
   independent roots 0.6·0.3 = 0.18 — pinning `joint_probability` =
   P(every event occurs) (`math.rs:168-181`).
5. Coverage reconciliation: 19 enumerated = 19 reviewed; 2 companies
   consumers; 2 prediction-markets producers; 7 skills; name-set
   membership — `true`.

### What the formal layer adds

The property tests (`property_tests.rs`) verify the Rust *implementation*
against two oracles; the Lean pin verifies the *contract* — that the
validation-and-marginalization design is internally correct for every
ordered field, not just for the f64 code path. Together they close the
loop: contract proven, implementation property-tested, and the seam
between them (zero-fill leniency, F19) documented on both sides.
