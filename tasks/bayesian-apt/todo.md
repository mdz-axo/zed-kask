# Todo — Bayesian APT Foundation

## Phase 1 — Foundation (Q1)
- [x] **T0** Keystone: belief-hierarchy ↔ `EventDependency` mapping — **HOLDS APPROXIMATELY** (t0-keystone-mapping.md); T4/T8 unblocked under the approximate license
- [ ] **T1** Citation store in research server (R2/R3)
  - [ ] blake3-pinned store + stable citation IDs
  - [ ] `web_extract` carries citation IDs + claim spans
  - [ ] `scenario_research` accepts citation IDs
- [x] **T2** First-class contract maturity (P2) — found already landed (`time_to_maturity` + `market_ladder`); verified in source, no work needed
- [x] **T3** Multi-group CPT fix (S1) — found already landed (noisy-OR multi-group combination, superforecast.rs L64–109); verified in source
- [x] **T6** Equity duration tool (C3) — `equity_duration_years` on `EpValuation` (economic_profit.rs); Macaulay over the EP stream; 3 new tests pass (ordering, None-for-destroyer, hand-check)
  - [ ] DCF-stream cross-check variant (deferred to Phase 2 — EP stream is primary per the review)
  - [ ] H2/T1 duration distribution dataset (needs coverage-universe run)
- [ ] **CP1** tests pass; duration face-validity review

## Phase 2 — Core (Q2–Q3)
- [ ] **T4a** Markets→tree composition algebra (`scenario_from_markets_set`)
  - [ ] N markets → validated EventTree w/ per-branch joints; tree-time refusal gates
  - [ ] Dependency inference via matcher.rs question overlap
  - [ ] CPT-size caps + independence diagnostics; cycle rejection
- [ ] **T4b** Tree-time challenge gates w/ provenance classes
- [ ] **T5** Tree-level Bayesian propagation + journal
- [ ] **T7** Tree-weighted valuation path in companies
- [ ] **CP2** vertical slice: markets→tree→propagation→tree-weighted DCF
- [ ] **T8a** Factor-mapping prototype (5 companies) — **kill gate on H3**
  - [ ] Loadings via `branch_return` revaluation (not branch-indicator covariances)
  - [ ] Hand-checks: binary tree σ≈0.176; single-branch loading = 1.0
- [ ] **T8b** `scenario_factor_exposures` platform surface

## Phase 3 — Integration & validation (Q4)
- [ ] **T9** Falsification suite H1–H5 executed; falsification log committed
- [ ] **T10** Refresh/tâtonnement journal + equilibrium-drift metrics
- [ ] **CP3** go/no-go on productionizing T8b
