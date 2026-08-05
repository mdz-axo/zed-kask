# Research Plan — 8 Workstreams Decomposed into Verifiable Tasks

**Method:** task-breakdown — vertically sliced tasks with acceptance
criteria, checkpoints, and dependency DAG. Scope: ~1 year of senior
researcher effort. Each task is small enough to verify independently.

---

## Dependency DAG (topological order)

```
WS1 (Theory) ─┬─> WS2 (Equity duration)
               ├─> WS3 (Scenario composition algebra)
               └─> WS5 (Equilibrium framing)
WS2 ──┐
WS3 ──┼──> WS4 (Risk calculation core) ──> WS7 (Integration) ──> WS8 (Falsification)
WS6 (MCP deep module) ──┘
WS5 ────────────────────────────────────────────> WS7
```

WS6 (MCP examination) feeds WS7 (integration) with the gap report (already
produced as deliverable 5). WS8 (falsification) consumes all prior
workstreams.

---

## WS1 — Theory foundations

### Task 1.1 — Extract arXiv:2211.03244 full text
- **Acceptance:** Full text of Bhattacharya extracted via corpus OCR or
  web; key theorems (belief-hierarchy equivalence, arbitrage-from-update
  theorem) transcribed with page references.
- **Checkpoint:** Theorem statements verifiable against the arXiv PDF.
- **Depends on:** nothing.

### Task 1.2 — Extract NY Fed sr216 full text
- **Acceptance:** Huberman & Wang APT paper extracted; the one-period /
  dynamic-portfolio distinction and the factor-identification requirement
  documented.
- **Checkpoint:** The "APT does not preclude arbitrage over dynamic
  portfolios" claim is verifiable in the extracted text.
- **Depends on:** nothing.

### Task 1.3 — Map Bhattacharya's belief hierarchy to `EventDependency`
- **Acceptance:** A written mapping showing that the bitmap-indexed
  `conditionals` vector satisfies (or fails) the recursion in
  Bhattacharya's Definition (belief hierarchy). If it fails, document the
  extension required.
- **Checkpoint:** The mapping is reviewed and either corroborates or
  refutes Inference/0.7 in the territory map.
- **Depends on:** 1.1.

### Task 1.4 — Retrieve and extract Morris (MIT) higher-order-belief material
- **Acceptance:** At least one Morris paper or lecture note on higher-order
  beliefs / global games extracted; the equilibrium-selection question
  documented.
- **Checkpoint:** The global-games uniqueness result is available for WS5.
- **Depends on:** nothing.

### Task 1.5 — Retrieve Bookstaber (book or detailed summary)
- **Acceptance:** Bookstaber's endogenous-risk thesis (tight coupling +
  complexity → normal accidents) documented with chapter references; the
  "intricate risk-management makes the system worse" claim grounded.
- **Checkpoint:** Wikipedia entry extracted (done); book retrieval flagged.
- **Depends on:** nothing. (Wikipedia extraction already complete.)

### Task 1.6 — Walras tâtonnement reference extraction
- **Acceptance:** hetwebsite.net Walras page fetched; tâtonnement stability
  conditions documented.
- **Checkpoint:** Stability conditions available for WS5.
- **Depends on:** nothing.

---

## WS2 — Equity duration & maturity transformation

### Task 2.1 — Promote `FadeHorizon` to continuous duration
- **Acceptance:** `economic_profit.rs` exposes a `duration: f64` field on
  the valuation output, computed as the Macaulay-style weighted average of
  `EP_t / (1+r)^t` over *t*. Existing categorical `FadeHorizon` retained as
  input that seeds the cash-flow schedule.
- **Checkpoint:** Unit test: a wide-moat company (20y fade) has duration
  > a no-moat company (5y fade).
- **Depends on:** WS6 (companies server reading, done).

### Task 2.2 — Add `duration` to `MarketRecord`
- **Acceptance:** `prediction-markets/types.rs` `assemble` computes
  `duration = deadline_days · (1 − |2p − 1|)` and populates a new
  `duration: f64` field. Both providers inherit via `assemble`.
- **Checkpoint:** Unit test: a 30-day market at p=0.5 has duration ≈ 30;
  at p=0.99 has duration ≈ 0.3.
- **Depends on:** WS6 (prediction-markets reading, done).

### Task 2.3 — `match_durations` operator
- **Acceptance:** A pure function `match_durations(D_equity, D_market) ->
  DurationGap { ratio, transformation_weight }` in a shared crate.
- **Checkpoint:** Property test: ratio is always > 0;
  transformation_weight ∈ [0, 1].
- **Depends on:** 2.1, 2.2.

### Task 2.4 — H2 empirical test (duration ratio)
- **Acceptance:** For 50 companies + 100 markets, compute the median
  duration ratio and its 95% bootstrap CI. Document whether H2 is
  corroborated or refuted.
- **Checkpoint:** The CI either includes or excludes 2.0.
- **Depends on:** 2.3, data collection.

---

## WS3 — Scenario composition algebra

### Task 3.1 — `scenario_from_markets_set` tool
- **Acceptance:** A new scenarios-server tool that takes a *set* of
  `MarketRecord`s and produces an `EventTree` with `EventDependency`
  links inferred from market question overlap (two markets on the same
  subject → conditional dependency).
- **Checkpoint:** Given 3 markets on a company, produces a tree with ≥ 1
  dependency edge.
- **Depends on:** WS6.

### Task 3.2 — Per-branch joint probability
- **Acceptance:** `EventTree` carries `branches: Vec<Branch>` where each
  `Branch` has `joint_probability: f64` and `path: Vec<EventId>`. The
  existing `joint_probability` (all-events proxy) is retained as a
  summary.
- **Checkpoint:** Sum of branch probabilities ≈ 1.0 (within float
  tolerance) for a binary tree.
- **Depends on:** 3.1.

### Task 3.3 — Composition algebra correctness test
- **Acceptance:** A test suite verifying that the composition algebra
  satisfies: (a) parent-independence marginalization matches
  `compute_marginal_probabilities`; (b) conditional-table lengths are
  2^num_parents; (c) cycle detection rejects cyclic dependencies.
- **Checkpoint:** All three properties hold on random trees.
- **Depends on:** 3.2.

### Task 3.4 — Belief-hierarchy recursion check (Task 1.3 output)
- **Acceptance:** If Task 1.3 found the algebra satisfies the recursion,
  document it. If not, implement the extension and re-test.
- **Checkpoint:** The recursion check passes on a 3-level hierarchy.
- **Depends on:** 1.3, 3.3.

---

## WS4 — Risk calculation core

### Task 4.1 — `branch_return(company, branch) -> f64`
- **Acceptance:** A function that re-evaluates the DCF/RIM under a
  scenario branch's assumptions (e.g., regulatory approval → revenue
  growth +X%; denial → revenue growth −Y%). Returns the implied annualized
  return.
- **Checkpoint:** On a test company, the bull-branch return > bear-branch
  return.
- **Depends on:** 3.2, WS6.

### Task 4.2 — `scenario_risk_measure(tree, company) -> RiskMeasure`
- **Acceptance:** Computes σ_scenario per the three-axes spec: the
  probability-weighted standard deviation of branch returns. Returns
  `RiskMeasure { sigma_scenario, expected_return, branch_count }`.
- **Checkpoint:** On a binary tree with returns {+20%, −15%} at p=0.6,
  σ_scenario ≈ 0.176 (hand-checkable).
- **Depends on:** 4.1.

### Task 4.3 — `scenario_factor_loadings(tree, company) -> Vec<(BranchId, f64)>`
- **Acceptance:** Computes β(c, b) = Cov(r(c), 1_b) / Var(1_b) per branch.
- **Checkpoint:** Loadings sum to a meaningful exposure profile; on a
  single-branch tree, the loading is 1.0.
- **Depends on:** 4.1.

### Task 4.4 — `fuse_volatility(realized, structural, scenario_implied) -> f64`
- **Acceptance:** A fusion operator combining realized market volatility,
  the structural flag, and σ_scenario into a single risk number. The
  fusion weights are documented and tunable.
- **Checkpoint:** When σ_scenario is None, fusion reduces to realized
  volatility (graceful degradation).
- **Depends on:** 4.2.

### Task 4.5 — H1 empirical test (scenario-implied risk → realized volatility)
- **Acceptance:** Panel regression of realized 90d volatility on
  σ_scenario controlling for 60d historical volatility. Document ΔR².
- **Checkpoint:** ΔR² > 0.01 (corroborate) or < 0.01 (refute H1).
- **Depends on:** 4.2, data collection.

### Task 4.6 — H3 empirical test (scenario-graph factor model)
- **Acceptance:** Two-stage regression: standard APT factors, then
  scenario-graph loadings on residuals. Document ΔR².
- **Checkpoint:** ΔR² > 0.005 (corroborate) or < 0.005 (refute H3).
- **Depends on:** 4.3, data collection.

---

## WS5 — Equilibrium discovery framing

### Task 5.1 — Tâtonnement feedback-loop design
- **Acceptance:** A written design for the feedback loop: prediction-market
  prices → scenario probabilities → company forecasts → (observed
  mispricing) → price adjustment. The loop's variety, closure, delay, and
  fidelity assessed per pragmatic-cybernetics.
- **Checkpoint:** The loop is closed (no broken feedback) and has
  identifiable stability conditions.
- **Depends on:** 1.6, WS4.

### Task 5.2 — Stability-condition analysis
- **Acceptance:** Document the conditions under which the tâtonnement loop
  converges (analogous to Walrasian stability: excess demand → price
  adjustment direction). Identify failure modes (oscillation, divergence).
- **Checkpoint:** At least one convergence condition and one divergence
  condition are formally stated.
- **Depends on:** 5.1.

### Task 5.3 — Global-games equilibrium selection (if Task 1.4 available)
- **Acceptance:** If multiple scenario-tree equilibria exist, document
  whether Morris-style global-games uniqueness applies.
- **Checkpoint:** The selection device is either identified or documented
  as absent.
- **Depends on:** 1.4, 5.1.

---

## WS6 — MCP server deep module examination (COMPLETE)

This workstream is already complete — its output is deliverable 5
(`05-mcp-capability-gap.md`). All four servers read from source; the
deep-module deletion test applied to each; the gap report produced.

---

## WS7 — Integration architecture

### Task 7.1 — Reverse bridge: scenarios → companies
- **Acceptance:** A companies-server tool `apply_scenario_tree(company,
  tree) -> ForecastEnvelope` that produces a probabilistic forecast
  envelope (expected return, σ_scenario, factor loadings) from a scenario
  tree.
- **Checkpoint:** On a test company + tree, the envelope is non-degenerate
  (σ > 0).
- **Depends on:** WS4.

### Task 7.2 — Citation-gate enforcement
- **Acceptance:** `ScenarioEvent.basis` is validated: if present, it must
  point to a research-server-extracted source (URL or citation). If
  absent, the event is labeled `hypothesis` and flagged.
- **Checkpoint:** A tree with an ungrounded event is rejected or warned.
- **Depends on:** WS6.

### Task 7.3 — MCDA-ranked integration surfaces
- **Acceptance:** The new surfaces (duration, composition, risk core,
  equilibrium framing) are ranked by MCDA on (leverage, effort, risk,
  falsifiability). The ranking informs build order.
- **Checkpoint:** The ranking is documented and stable under sensitivity
  analysis.
- **Depends on:** WS4, WS5. (Output: deliverable 6.)

---

## WS8 — Falsification & validation

### Task 8.1 — Falsification suite (deliverable 7)
- **Acceptance:** For each H1–H5, a discriminating test with falsifier
  threshold, data requirements, and execution plan.
- **Checkpoint:** Each test is executable given the WS4 outputs.
- **Depends on:** WS4. (Output: deliverable 7.)

### Task 8.2 — Execute H2 test (duration ratio)
- **Acceptance:** H2 test run; result documented.
- **Depends on:** 2.4.

### Task 8.3 — Execute H1 test (scenario-implied risk)
- **Acceptance:** H1 test run; result documented.
- **Depends on:** 4.5.

### Task 8.4 — Execute H3 test (factor model)
- **Acceptance:** H3 test run; result documented.
- **Depends on:** 4.6.

### Task 8.5 — H4 deletion-test retrospective
- **Acceptance:** After WS4 is built, re-run the deletion tests on each
  axis. Document whether the complexity allocation survives.
- **Depends on:** WS4.

---

## Suggested build order (critical path)

1. **WS1** (theory extraction) — in parallel with WS6 (done).
2. **WS2** (duration) — small, high-leverage; unblocks H2.
3. **WS3** (composition algebra) — unblocks WS4.
4. **WS4** (risk core) — the large effort; the heart of the foundation.
5. **WS5** (equilibrium framing) — in parallel with WS4's later tasks.
6. **WS7** (integration) — after WS4.
7. **WS8** (falsification) — after WS7.

**Estimated effort distribution** (of ~1 year):
- WS1: 15% (extraction + mapping)
- WS2: 10% (small, but empirical test needs data)
- WS3: 15% (algebra + tests)
- WS4: 30% (the risk core — largest)
- WS5: 10% (design + stability analysis)
- WS7: 10% (integration)
- WS8: 10% (empirical tests + retrospective)
