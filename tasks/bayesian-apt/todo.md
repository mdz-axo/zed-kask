# Todo — Bayesian APT Foundation (v2, CMP-first)

## Phase 0 — CMP foundation (the base layer)
- [ ] **C0.1** Base-event registry (oil, gas, bitcoin, ethereum, inflation, rates) — verify continuous availability on both venues via `market_ladder`
- [ ] **C0.2** Semantic eligibility mapping (FIBO subject + orientation + magnitude band) on the ontology block
- [ ] **C0.3** Weight solver + roll rules in `hkask-forecast` (maturity ±0.5d + magnitude, least-deviation, withhold when no bracket)
- [ ] **C0.4** 1m/3m/6m CMP indices per (family, orientation), per-venue; publish probability + weights + maturity error + reliability floor
- [ ] **CP-CMP** rates family produces continuous 1m/3m/6m series on both venues; ≥90% of days within tolerance; eligibility classifications human-reviewed

## Phase 1 — Re-point machinery at CMP inputs
- [ ] **R1** Composition over CMP (T4a re-pointed; provenance = index, not decaying contract)
- [ ] **R2** Duration matching vs constant maturity (H2 made testable)
- [ ] **R3** Tree-weighted valuation over CMP (T7 re-pointed)

## Phase 2 — Risk and coherence (CMP-controlled)
- [ ] **R4** σ_scenario over CMP-driven branches (T8a re-pointed)
- [ ] **R5** Contract-price coherence test (H3 reframed — tree-implied joints vs market joint prices; NO equity-return betas)

## Phase 3 — Falsification & validation (all on CMP inputs)
- [ ] **R6** Falsification suite H1–H5 (H3 reframed); falsification log committed

## Completed machinery (v1, retained, awaiting CMP inputs)
- [x] T0 keystone (approximate license)
- [x] T1 citations / T2 maturity+ladder / T3 multi-group CPTs / T6 equity duration (EP+DCF)
- [x] T4a composition / T5 propagation / T7 tree-weighted valuation / T8a risk-core functions

## Removed (v1 drift)
- ~~Equity-return beta regressions, FF stage-1/stage-2 tests, stock factor loadings~~
- ~~T8b gated on equity-return pricing test~~ → replaced by R5 contract-price coherence
