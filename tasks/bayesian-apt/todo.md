# Todo — Bayesian APT Foundation

## Phase 1 — Foundation (Q1)
- [ ] **T1** Citation store in research server (R2/R3)
  - [ ] blake3-pinned store + stable citation IDs
  - [ ] `web_extract` carries citation IDs + claim spans
  - [ ] `scenario_research` accepts citation IDs
- [ ] **T2** First-class contract maturity (P2)
  - [ ] `time_to_maturity` on `MarketRecord`
  - [ ] Contract-ladder endpoint with duration profile
- [ ] **T3** Multi-group CPT fix (S1)
  - [ ] All `depends_on` groups consumed; regression test
- [ ] **T6** Equity duration tool (C3)
  - [ ] D_e (mechanical + DSS variants) from DCF outputs
  - [ ] H2/T1 duration distribution dataset
- [ ] **CP1** tests pass; duration face-validity review

## Phase 2 — Core (Q2–Q3)
- [ ] **T4a** Markets→tree composition algebra
  - [ ] N markets → validated EventTree; tree-time refusal gates
  - [ ] CPT-size caps + independence diagnostics
- [ ] **T4b** Tree-time challenge gates w/ provenance classes
- [ ] **T5** Tree-level Bayesian propagation + journal
- [ ] **T7** Tree-weighted valuation path in companies
- [ ] **CP2** vertical slice: markets→tree→propagation→tree-weighted DCF
- [ ] **T8a** Factor-mapping prototype (5 companies) — **kill gate on H3**
- [ ] **T8b** `scenario_factor_exposures` platform surface

## Phase 3 — Integration & validation (Q4)
- [ ] **T9** Falsification suite H1–H5 executed; falsification log committed
- [ ] **T10** Refresh/tâtonnement journal + equilibrium-drift metrics
- [ ] **CP3** go/no-go on productionizing T8b
