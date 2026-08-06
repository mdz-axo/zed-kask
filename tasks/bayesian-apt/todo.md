# Todo — Bayesian APT Foundation (v2, CMP-first)

## Phase 0 — CMP foundation (the base layer)
- [x] **C0.3** Weight solver + roll rules — `cmp_portfolio.rs` landed 2026-08-05: weighted-portfolio construction (bracket pair, exact solve, withhold when no bracket), maturity window, materiality level (volatility-derived + override), orientation classification; all thresholds in `CmpConfig` (no magic numbers); 7 tests pass, clippy clean
- [x] **C0.1** Base-event registry — `base_event.rs` landed 2026-08-05: six families (oil, gas, bitcoin, ethereum, inflation, rates) with semantic signatures + per-family materiality settings (type follows volatility units); `classify_base_event` over question/description/series/category; 3 tests pass. Live availability verification via `market_ladder` is the CP-CMP probe (not yet run against live venues)
- [x] **C0.2** Semantic eligibility mapping — `evaluate_record` in `cmp_portfolio.rs`: base-event classify → materiality level → orientation → maturity window → reliability floor, rejections surfaced with reasons; 4 tests pass. FIBO subject mapping is a future refinement (signature matching is the auditable substrate for now)
- [ ] **C0.4** 1m/3m/6m CMP indices per (family, orientation), per-venue; publish probability + weights + maturity error + reliability floor
- [ ] **CP-CMP** rates family produces continuous 1m/3m/6m series on both venues; ≥90% of days within tolerance; eligibility classifications human-reviewed

## Ontology bridge refactor (prerequisite for C0.2 FIBO-anchored mapping)
- [x] **ONT-1** Single shared crate `hkask-bridge-ontology` created 2026-08-05: owns all ontology vocabulary (DC/BIBO/CiTO, PKO, FIBO union, ESO, GOLEM, OMC, ML-Schema) + the dual-axis domain-selection logic (`select_ontology_anchor`). No ontology vocabulary lives inside any MCP server. 31 tests pass.
- [x] **ONT-2** Condenser migrated 2026-08-05: `OntologyNamespace`/`OntologyAxis`/`OntologyAnchor` moved to the shared crate; `derive_ontology_anchor` replaced by `select_ontology_anchor`; 83 tests pass.
- [x] **ONT-3** Corpus, media, companies, training, prediction-markets migrated 2026-08-05: all server-local bridge modules deleted; servers depend on the shared crate; server-specific dispatch helpers kept in servers. Workspace `cargo check` + all crate tests pass.
- [x] **ONT-4** PRINCIPLES.md P5.4/P8.1 updated 2026-08-05: single-crate architecture documented; `prediction-markets` added to the server table.
- [x] **ONT-5** Documentation 2026-08-05: architecture diagram, API reference, how-to guide, and ontology-anchored-embedding explanation updated.
- [ ] **ONT-6** Rewire `economic_object.rs` and `base_event.rs` onto the FIBO-anchored classification through the corpus pipeline (delete the substring synonym-closure loop). This is the next step.

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
