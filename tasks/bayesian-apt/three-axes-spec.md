---
dcterms:title: "Three-Axes Specification — Time, Return, Risk"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
---

# Three-Axes Specification

Design constraint under test (H4): keep time and return mathematically simple; spend the
complexity budget on event-tree probabilities and risk. Each axis carries its essentialist
deletion-test result.

## Axis 1 — Time / duration (SIMPLE)

**Model**: one duration scalar per asset class, one transparent mapping.

- **Equity duration** D_e: cash-flow-weighted average time of the DCF PV stream,
  computed from existing `ProjectedModel` outputs (stage-1/stage-2 PVs + terminal value PV).
  D_e = Σ_t t·PV(CF_t) / Σ_t PV(CF_t). Two estimation variants to be compared (H2/T2):
  (a) Dechow–Sloan–Soliman implied duration (ROIC-persistence-driven; cite JAE 2004 via
  Library Damodaran-ROIC + Franchise_Value_Liebowitz.pdf lineage);
  (b) mechanical DCF-timing duration from the platform's own projections.
- **Contract duration** D_c: time-to-deadline in years, promoted to a first-class
  `time_to_maturity: f64` field on `MarketRecord` (today: RFC3339 string + ad-hoc
  `days_between`, C39). For a contract ladder (e.g. Fed meeting chain), the ladder's
  duration profile is the vector of D_c — no fitting, no term-structure model.
- **The mapping**: duration mismatch Δ = D_e − D_c enters the risk axis as a
  maturity-transformation haircut on the contract's usable signal, and nowhere else.
  Time does not appear in the return axis beyond discounting already inside the DCF.

**Deletion test (G1)**: delete the duration mapping → every forecast reconciles horizons
ad hoc; complexity reappears in callers. **Survives.** Delete a richer term-structure model
→ only H2c-sensitive error grows. **Rich time model cut, conditionally** (reinstated only
if H2c is refuted).

**Explicitly excluded**: stochastic discount-rate term structures, duration convexity,
rate-scenario simulation. (Justification: C14 coarse-approach; H4 test governs reinstatement.)

## Axis 2 — Return (SIMPLE)

**Model**: expected return implied by prices and probabilities, nothing more.

- **Contract-implied**: a binary contract at price p paying 1 implies expected payoff p;
  expected return = (payoff × P(event) − price) / price under the tree's P(event).
  No utility, no risk adjustment at this layer — risk adjustment is the risk axis's job.
- **Equity-implied**: expected return = (intrinsic value under scenario-weighted DCF −
  market price) / market price, using existing `expected_intrinsic` machinery
  (companies/superforecast.rs L182) with scenario weights from the event tree instead of
  the current independence-assuming `distribute_scenario_probabilities` (L158–168).
- **Cross-asset comparability** comes from expressing both as expected return per unit of
  scenario-factor exposure (the APT relation, C1) — the risk axis supplies the exposures.

**Deletion test (G1)**: delete the implied-return module → every caller re-derives returns
from prices inconsistently. **Survives as a thin, deep module** (small interface, one formula
family). Anything richer (utility functions, consumption CAPM, habit formation — cf.
arXiv:2406.02155) is **cut**: it would spend budget the risk axis needs.

## Axis 3 — Risk (COMPLEX — the budget lives here)

Three sub-components, in dependency order:

### 3.1 Event-tree probability machinery (Bayesian core)
- Tree-level Bayesian propagation: updating one node's prior recomputes all descendant
  marginals and the joint — closing the gap identified at C30. Machinery: Koller & Friedman
  PGM inference (Library holding) over the existing CPT representation
  (`EventDependency.conditionals`, bitmap-ordered); must first lift the single-dependency-group
  limitation (C28).
- Challenge gates: each node's probability carries provenance (market-implied / Fermi /
  base rate / research-claim) and must survive a gate before entering the tree — extending
  the existing refusal gates (C29) from bridge-time to tree-time.
- Calibration feedback: resolved events update node-level and bucket-level calibration
  (existing `CalibrationStore`, `scenario_score`) and demote unreliable sources
  (existing reliability-tier negative feedback, C37).

### 3.2 Volatility & payoff uncertainty
- Contract side: extend existing `Volatility` (realized variance + structural flags, C37)
  with price-history series (today snapshot-only, C39) so variance is measured, not flagged.
- Equity side: scenario-weighted variance of intrinsic value across tree paths (replaces
  uniform-sampling Monte Carlo with tree-structured simulation — arXiv:0904.1131,
  arXiv:2004.09042 methods as references).
- Default/payoff uncertainty: resolution-risk on contracts (resolution_source, UMA status
  already in `MarketRecord`) + cash-flow haircut scenarios on equity.

### 3.3 APT factor-risk interface
- Scenario nodes as factors; company cash-flow sensitivity to node outcomes as loadings
  (H3). Pricing test per sr216's static-portfolio warrant (C1); dynamic updating handled
  outside APT's warrant and labeled as such (C2).
- Backward recursion on the tree for time-consistent risk measures
  (arXiv:1508.02367 machinery) — the computational engine for path-dependent risk.

**Deletion test (G1)**: delete 3.1 → no probabilities, foundation collapses. Delete 3.2 →
no uncertainty quantification, probabilities are point estimates. Delete 3.3 → no link to
return, foundation is a forecasting tool not a pricing tool. **All three survive; this is
where the complexity budget is spent — per H4, subject to the error-concentration test.**

## Complexity-allocation rationale (justified, not asserted)

1. Essentialist: time and return each survive G1 only as *thin* modules; risk survives as
   the deep core. The allocation is the deletion test's output, not a preference.
2. Independent support: Bookstaber (C14) — coarse models beat fine-grained ones under
   tight coupling; Tetlock (Library) — calibration gains concentrate in probability
   estimation, not in model elaboration.
3. Falsifiable: H4's error-concentration instrumentation will refute the allocation if
   errors concentrate in time/return.
