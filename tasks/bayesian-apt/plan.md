---
dcterms:title: "Research Plan — Bayesian Arbitrage Pricing via Composable Prediction-Market Scenarios (v2, CMP-first)"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
pko:procedure: "CMP foundation → composition → risk core → integration, ~1 senior-researcher-year"
---

# Research Plan (v2 — CMP-first)

**What changed and why**: v1 skipped the foundation. It built composition, propagation,
tree-weighted valuation, and a risk core directly on raw prediction-market contracts —
instruments with decaying, mismatched maturities. That made the outputs uninformative
(square peg, round hole). v2 inserts the missing base layer: **Constant-Maturity
Prediction (CMP) indices** — rolling synthetic portfolios of contracts, normalized to
fixed maturity and fixed orientation/magnitude of change, so the time axis is taken
out of the equation before anything downstream consumes a probability.

**Equity pricing discipline (user correction)**: equities are priced on **fundamental
forecast models** (DCF/RIM, MAIA). No CAPM, no factor betas, no equity-return
regressions. The arbitrage-pricing apparatus applies to the **contracts** — decomposing
and bridging their prices and analyzing their coherence — never to modeling stock
returns.

## Dependency graph (bottom-up)

```
C0 CMP foundation (base events, eligibility, weight solver, roll rules)
   ├─> C1 semantic eligibility mapping (FIBO/DC subject + orientation + magnitude)
   └─> C2 1m/3m/6m indices for the six base families
T2 time_to_maturity + market_ladder (done) ──feeds──> C0
T6 equity duration (done) ──compares against──> C2 (constant maturity, not snapshots)
T4a composition (done, machinery) ──re-pointed at──> C2 outputs
T5 propagation (done, machinery) ──re-pointed at──> C2 outputs
T7 tree-weighted valuation (done, machinery) ──re-pointed at──> C2 outputs
T8a risk core (done, machinery) ──re-pointed at──> C2 outputs
H-tests run only on CMP-controlled inputs
```

## Phase 0 — CMP foundation (the new base layer; see cmp-foundation.md)

### C0.1 — Base-event registry — S
- Define the six initial base-event families (oil, gas, bitcoin, ethereum, inflation,
  interest rates) with their systematic-factor rationale.
- AC: a typed registry (family → subject factor, venue availability, typical ladder
  shape); each family verified to have continuously-available contracts on both
  Kalshi and Polymarket (checked via `market_ladder`, not assumed).
- Deps: T2 (done).

### C0.2 — Semantic eligibility mapping — M
- FIBO subject mapping for the six families + orientation (increase/decrease) +
  magnitude-band extraction from contract text, layered on the existing
  PKO/Dublin-Core ontology block.
- AC: given a `MarketRecord`, a deterministic classifier returns
  (family, orientation, magnitude) or "not a base-event contract"; precision checked
  on a hand-labeled sample of real contracts per family.
- Deps: C0.1.

### C0.3 — Weight solver + roll rules — M
- The two-constraint weighting: weights w_i ≥ 0, Σw_i = 1, matching weighted-average
  maturity to target ± 0.5 days (default) AND weighted-average magnitude to target
  within tolerance; least-deviation when over-identified; ties broken by
  liquidity/reliability tier. Smooth roll rule (no cliff-edge probability jumps).
- AC: pure functions in `hkask-forecast`; hand-check on a 2-contract bracket (exact
  solution); property tests (weights in [0,1], maturity error ≤ tolerance when a
  bracket exists); withhold (never fabricate) when no bracket spans the target.
- Deps: C0.2.

### C0.4 — CMP index construction — M
- 1-month, 3-month, 6-month forward CMP indices per (family, orientation) for the six
  families; per-venue (Kalshi-CMP, Polymarket-CMP) to respect the law-of-one-price
  failure (arXiv:2601.01706).
- AC: each index publishes daily index probability, constituent weights/maturities,
  maturity-matching error, reliability floor; degraded/sparse ladders are withheld,
  not fabricated.
- Deps: C0.3.

**Checkpoint CP-CMP**: at least one family (interest rates) produces a continuous
1m/3m/6m CMP series on both venues for a trailing window; maturity error within
tolerance on ≥ 90% of days; human reviews the eligibility classifications.

## Phase 1 — Re-point the machinery at CMP inputs

### R1 — Composition over CMP (T4a re-pointed) — S
- `scenario_from_markets_set` accepts CMP index probabilities as event priors in
  place of raw contract probabilities. AC: same tree, CMP inputs; provenance records
  the index (family, orientation, target maturity), not a decaying contract.

### R2 — Duration matching vs constant maturity (H2 made testable) — S
- Compare equity duration (T6) against the *fixed* CMP tenors (1m/3m/6m), not
  decaying snapshots. AC: H2/T1 dataset recomputed on CMP tenors; the maturity
  transformation gap is now a controlled quantity.

### R3 — Tree-weighted valuation over CMP (T7 re-pointed) — S
- The tree-weighted `scenario_analysis` path consumes CMP-driven trees. AC:
  `weighting_mode: "event_tree"` outputs cite CMP provenance.

## Phase 2 — Risk and coherence (CMP-controlled)

### R4 — σ_scenario over CMP-driven branches (T8a re-pointed) — S
- `scenario_risk_measure` / `fuse_volatility` consume CMP-controlled branch
  probabilities. AC: risk measures carry CMP provenance; no raw-contract inputs.

### R5 — Contract-price coherence (H3, reframed per user correction) — M
- The arbitrage analysis on the contracts: are the tree-implied joint probabilities
  coherent with observed contract prices (incl. parlay/joint contracts where listed)?
  Divergence = the analyzable signal. **No equity-return regressions, no betas.**
- AC: a coherence measure (tree-implied joint vs market joint price) with a
  transaction-cost band; tested on CMP-controlled trees; falsifier defined.
- Deps: R1, C0.4.

## Phase 3 — Falsification & validation (all on CMP inputs)

### R6 — Falsification suite (H1–H5, reframed) — M
- H1 (systemic risk capture), H2 (duration), H3 (contract-price coherence, reframed),
  H4 (complexity allocation), H5 (LLM leverage) — all run on CMP-controlled inputs.
- AC: falsification log; statuses updated; no equity-return beta machinery anywhere.

## What is preserved from v1 (machinery, re-pointed)

- T0 keystone (approximate license) — unchanged, still valid.
- T1 citations, T2 maturity field + ladder, T3 multi-group CPTs, T6 equity duration
  (both variants), T4a composition, T5 propagation, T7 tree-weighted valuation,
  T8a risk-core functions — all retained as **machinery awaiting CMP inputs**.
- The analyst maturity ladder (2×2 simple mode first, tree mode earned) — unchanged.

## What is removed from v1 (the drift)

- Equity-return beta regressions, Fama-French stage-1/stage-2 ΔR² tests, "factor
  loading of a stock" framing. Equities are priced on fundamentals; APT lives on the
  contracts.
- The T8a kill-gate verdict that gated T8b on an equity-return pricing test — replaced
  by R5's contract-price coherence test.

## Open questions (from cmp-foundation.md §6)

1. Magnitude bands: fixed per family vs continuous with tolerance?
2. Orientation: independent increase/decrease index pairs vs one signed index?
3. Cross-venue: per-venue indices (recommended) vs pooled with adjustment?
4. Sparse ladders: withhold (recommended) vs publish degraded with wide error?
