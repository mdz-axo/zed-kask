---
dcterms:title: "Research Plan — Bayesian Arbitrage Pricing via Composable Prediction-Market Scenarios"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
pko:procedure: "8-workstream foundation build, ~1 senior-researcher-year"
---

# Research Plan

Scope: ~1 year senior researcher effort. Phases: Foundation (Q1) → Core (Q2–Q3) →
Integration & Validation (Q4). High-risk slices scheduled early (fail fast).
Essentialist enforcement: ≤7 top-level workstream blocks — the 8 user-named workstreams
are preserved but WS6 (MCP examination) is **already executed** (mcp-gap-report.md) and
folds into WS7; the plan therefore runs 7 live blocks.

## Dependency graph (bottom-up)

```
T0 keystone mapping verification (3-outcome gate) ──gates──> T4, T8
T1 citations (R2/R3) ──────────────────────────────┐
T2 time_to_maturity (P2) ──────────────────────────┤
T3 CPT multi-group fix (S1) ──> T4 composition algebra (S4) ──> T5 tree propagation (S3)
T6 equity duration (C3, RIM/EP-based) ──> H2 tests ┤
T7 tree-weighted valuation (C2) <──────────────────┤
T8 factor mapping + pricing harness (S7/C4) <──────┴── (highest risk)
T9 H1–H5 falsification suite (runs against T1–T8 outputs)
T10 refresh/tâtonnement journal (closes stale-anchor loop)
```

## Phase 1 — Foundation (Q1)

**Checkpoint CP0 (entry):** MCP gap report + territory map reviewed by human. ✅ (this package)

### T0 — Keystone: verify belief-hierarchy ↔ `EventDependency` mapping — S (gates T4/T8)
- Slice: theory/mapping-verification (adopted from the parallel plan's Task 1.3; see
  phase2-review.md)
- Using the already-extracted theorems (territory-map C5–C7), produce a written mapping
  between Bhattacharya's belief-hierarchy recursion (infinite, interactive, over others'
  *strategies*) and the CPT algebra (finite, parent-independent, over *states of nature*).
- AC: three-outcome gate — (i) holds exactly → proceed; (ii) holds approximately → derive
  the depth-k truncation bound, document the approximate license, proceed; (iii) fails →
  STOP, no WS3/WS4 build until an independent anchor exists. Expected outcome per C6:
  (ii), with the state-hierarchy vs strategy-hierarchy distinction made precise.
- Verification: mapping document reviewed against ar5iv full text; result written back
  into territory-map C5–C7 (confidence updated).
- Deps: none (extraction already complete). Files: tasks/bayesian-apt/, no code.

### T1 — Citation store in research server (R2/R3) — M
- Slice: research/citation-pinning
- AC: (i) blake3-pinned content store with stable citation IDs; (ii) `web_extract` responses
  carry citation IDs + claim-level spans; (iii) scenarios `scenario_research` accepts citation
  IDs in lieu of pasted text; (iv) `ScenarioEvent.basis` warn-and-label gate: basis must be a
  citation ID or explicitly labeled `hypothesis` (warn, not reject — consistent with the
  platform's withhold-never-reject refusal-gate semantics, phase2-review.md B4).
- Verification: integration test — extract → cite → build event → trace event provenance to hash.
- Deps: none. Files: hkask-mcp-research/{db.rs,types,tools}, hkask-mcp-scenarios bridge.

### T2 — First-class contract maturity (P2) — S
- Slice: prediction-markets/maturity
- AC: (i) `time_to_maturity: f64` on `MarketRecord`; (ii) ladder endpoint returning a series'
  contract chain with duration profile; (iii) volatility flags consume the new field.
- Verification: unit tests + schema test (`find_boolean_schema_positions` per .rules).
- Deps: none. Files: hkask-mcp-prediction-markets/{types.rs,provider_*,tools}.

### T3 — Multi-group CPT fix (S1) — S
- Slice: scenarios/cpt-generalization
- AC: `compute_marginal_probabilities` consumes all `depends_on` groups; validation covers
  multi-group CPTs; existing single-group behavior unchanged (regression test).
- Deps: none. Files: hkask-mcp-scenarios/superforecast.rs, hkask-forecast.

### T6 — Equity duration tool (C3) — S
- Slice: companies/equity-duration
- AC: `equity_duration` tool computing D_e as a Macaulay-style weighted average over the
  RIM/EP stream (`EpPeriod.present_value` per year, `economic_profit.rs` L195–269 —
  primary, per the parallel plan's Task 2.1) **and** over the DCF stream (cross-check);
  sensitivity report across ROIC-persistence and fade assumptions; `FadeHorizon` retained
  as input seeding the schedule.
- Verification: unit test wide-moat (20y fade) duration > no-moat (5y fade); golden-file
  tests on 3 reference firms; H2/T1 dataset emitted.
- Deps: none. Files: hkask-mcp-companies/{economic_profit.rs,valuation.rs,financial_model.rs}.

**Checkpoint CP1:** all tests pass; H2/T1 (duration distribution) computed on coverage
universe; human reviews duration estimates for face validity.

## Phase 2 — Core (Q2–Q3)

### Design constraint: the analyst maturity ladder (user directive, 2026-08-05)

The simple 2×2 scenario mode is a **first-class citizen, permanently retained** — not a
legacy path to be replaced by the tree machinery. The two modes serve different points on
the analyst's research maturity:

1. **Simple mode (entry)**: companies `scenario_analysis` (Schwartz 2×2, growth × margin)
   → `scenario_from_companies` → single-market `scenario_from_markets`. Fast, low-data,
   appropriate when the analyst does not yet know which events condition each other.
2. **Detailed mode (earned)**: `scenario_from_markets_set` → `scenario_propagate` →
   tree-weighted valuation (T7) → factor loadings (T8). Requires the analyst to have
   done the research — company, industry, economy, technology, management, domain
   experts — to know the tree's structure. **You don't start out knowing the full tree
   of events; you work up to it.**

Consequences:
- T7 must **add** a tree-weighted path alongside the 2×2 path, never replace it. The
  2×2 mode's independence-assuming weights stay as the default; tree weights are an
  explicit opt-in upgrade when a validated tree exists.
- The platform's existing pipeline-sequence discipline (`check_sequence`:
  frame → brainstorm → build → quantify → …) already encodes this ladder; the new
  tree tools are documented as the ladder's top rung (maturity note added to
  `scenario_from_markets_set`).
- This is also the epistemically correct order per T0: the tree's conditioning
  structure is caller-authored knowledge, and the 2×2 mode is how an analyst
  accumulates enough understanding to author it.

### T4 — Composition algebra (S4) — L→split into T4a/T4b
- T4a: markets→tree wiring (M): new `scenario_from_markets_set` tool — given N matched
  `MarketRecord`s + dependency spec, construct a validated `EventTree` with CPTs; refusal
  gates at tree-time. Dependency links inferred from market-question overlap using the
  existing matcher.rs Jaccard machinery (parallel plan's Task 3.1 heuristic, adopted).
  Extend `EventTree` with per-branch `branches: Vec<Branch>` (joint_probability + path).
  AC: round-trip test markets→tree→marginals matching `compute_marginal_probabilities`;
  CPT-size cap with independence diagnostics (variety amplifier iv); cycle rejection test;
  branch probabilities sum to 1 within float tolerance on binary trees.
- T4b: challenge gates at tree-time (S): provenance-carrying probabilities; gate policy
  per source class (market/Fermi/base-rate/research-citation). AC: gate audit log; a
  fabricated-probability attempt is refused in test.
- Deps: T1, T2, T3. Files: hkask-forecast (new composition module), hkask-mcp-scenarios.

### T5 — Tree-level Bayesian propagation (S3) — M
- Slice: scenarios/propagation
- AC: updating any node's prior recomputes descendant marginals + joint; `scenario_update`
  gains a tree mode; propagation journal for the tâtonnement record.
- Deps: T4a. Files: hkask-forecast, hkask-mcp-scenarios/superforecast.rs.

### T7 — Tree-weighted valuation (C2) — M
- Slice: companies/tree-weighted-dcf
- **Maturity-ladder constraint (user directive)**: the 2×2 mode stays the default,
  first-class path. T7 ADDS a tree-weighted option alongside it — an explicit opt-in
  upgrade for analysts who have built a validated tree — it does not replace the
  independence-assuming 2×2 weights.
- AC: (i) `scenario_analysis` retains its current 2×2 behavior unchanged (regression
  test); (ii) a new tree-weighted path accepts an `EventTree` (from
  `scenario_from_markets_set`/`scenario_propagate`) and produces a scenario-weighted
  valuation using tree marginals/joints as the weights; (iii) the output labels which
  mode produced it (`weighting_mode: "schwartz_2x2" | "event_tree"`) so downstream
  consumers can tell the maturity level of the analysis; (iv) gap decomposition
  attributes error to scenario vs operating assumptions in both modes.
- Verification: 2×2 regression test unchanged; tree-weighted path on a hand-built tree
  reproduces hand-computed weights; mode label present.
- Deps: T4a. Files: hkask-mcp-companies/{scenarios.rs,superforecast.rs,valuation.rs}.

**Checkpoint CP2:** end-to-end vertical slice demo — markets → tree → propagation →
tree-weighted DCF for one real company; H4 error-concentration instrumentation live.

### T8 — Factor mapping + pricing harness (S7/C4) — L (highest risk, fail-fast prototype first)
- T8a: prototype (S): hand-built tree for 5 companies; loadings = cash-flow sensitivity
  of company value to branch outcomes, elicited via `branch_return` revaluation of the
  DCF/RIM under each branch's assumptions (NOT Cov with branch indicators — indicators
  across mutually exclusive branches are collinear; see phase2-review.md B2). Run H3/T1
  cross-sectional pricing test vs FF5/AMF baseline. AC: pricing errors computed; verdict
  on H3a/H3c recorded; hand-check unit tests pass (binary tree {+20%, −15%} at p=0.6 →
  σ≈0.176; single-branch loading = 1.0). **Kill gate: if prototype refutes H3, T8b is
  re-scoped before building.**
- T8b: platform surface (M): `scenario_factor_exposures` tool + pricing-test harness in
  hkask-forecast; loading-stability tracking (H3/T2).
- Deps: T5, T7. Files: hkask-forecast, hkask-mcp-scenarios, hkask-mcp-companies.

## Phase 3 — Integration & validation (Q4)

### T9 — Falsification suite (H1–H5) — M
- AC: every test in hypothesis-dossier.md executed or explicitly deferred with reason;
  falsification log committed; statuses updated (corroborated/refuted/open).
- Deps: T1–T8. Files: kask/traces/, tasks/bayesian-apt/.

### T10 — Refresh / tâtonnement journal — S
- AC: re-bridge policy (scheduled + price-move trigger); each refresh journaled as a
  tâtonnement step (SDF-analog: scenario-weighted implied returns); equilibrium-drift
  metrics reported.
- Deps: T5, T7. Files: hkask-mcp-scenarios, hkask-forecast.

**Checkpoint CP3:** full falsification suite results; metacognitive close-out re-run;
human go/no-go on productionizing T8b.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| H3 refuted (scenario graph not pricing-relevant) | foundation becomes interpretive tool | T8a kill gate before T8b spend |
| Equity duration model-sensitive (F2) | duration axis decorative | two estimation variants + sensitivity report (T6) |
| Venue fragmentation swamps signal (C18) | H1 fails | single-venue scoping; tier controls (T9/H1c) |
| CPT combinatorics explode | trees unusable | size caps + independence diagnostics (T4a) |
| sr216 dynamic-portfolio limitation (C2) | APT warrant misapplied | label dynamic layer as outside APT warrant; static tests only |

## Open questions

1. Which venue is canonical for the single-venue scope (Polymarket vs Kalshi coverage)?
2. Is parlay/joint-contract data (H3d) obtainable from either provider API?
3. What is the human analyst's current workflow cost baseline for H5's paired study?
4. Should refresh policy be per-tree or global (variety vs simplicity)?

## Refinement history

Iteration 1 (this plan): producer self-evaluation — sizing 0.10 (T4/T8 split pre-emptively),
vertical-slice 0.05, AC specificity 0.10, dependency ordering 0.05, checkpoints 0.0,
red-flags 0.0 → weighted 0.075. Quality gate: pass (≤0.15, no criterion >0.30).
