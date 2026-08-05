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
T1 citations (R2/R3) ──────────────────────────────┐
T2 time_to_maturity (P2) ──────────────────────────┤
T3 CPT multi-group fix (S1) ──> T4 composition algebra (S4) ──> T5 tree propagation (S3)
T6 equity duration (C3) ──> H2 tests ──────────────┤
T7 tree-weighted valuation (C2) <──────────────────┤
T8 factor mapping + pricing harness (S7/C4) <──────┴── (highest risk)
T9 H1–H5 falsification suite (runs against T1–T8 outputs)
T10 refresh/tâtonnement journal (closes stale-anchor loop)
```

## Phase 1 — Foundation (Q1)

**Checkpoint CP0 (entry):** MCP gap report + territory map reviewed by human. ✅ (this package)

### T1 — Citation store in research server (R2/R3) — M
- Slice: research/citation-pinning
- AC: (i) blake3-pinned content store with stable citation IDs; (ii) `web_extract` responses
  carry citation IDs + claim-level spans; (iii) scenarios `scenario_research` accepts citation
  IDs in lieu of pasted text.
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
- AC: `equity_duration` tool computing D_e (both variants: mechanical DCF-timing + DSS-implied
  scaffold) from existing DCF outputs; sensitivity report across ROIC-persistence assumptions.
- Verification: golden-file tests on 3 reference firms; H2/T1 dataset emitted.
- Deps: none. Files: hkask-mcp-companies/{valuation.rs,financial_model.rs}.

**Checkpoint CP1:** all tests pass; H2/T1 (duration distribution) computed on coverage
universe; human reviews duration estimates for face validity.

## Phase 2 — Core (Q2–Q3)

### T4 — Composition algebra (S4) — L→split into T4a/T4b
- T4a: markets→tree wiring (M): given N matched `MarketRecord`s + dependency spec, construct
  a validated `EventTree` with CPTs; refusal gates at tree-time. AC: round-trip test
  markets→tree→marginals; CPT-size cap with independence diagnostics (variety amplifier iv).
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
- AC: `calibrate_forecast`/`scenario_analysis` accept tree joint probabilities in place of
  independence-assuming 2x2 weights; gap decomposition attributes error to scenario vs
  operating assumptions.
- Deps: T4a. Files: hkask-mcp-companies/{scenarios.rs,superforecast.rs,valuation.rs}.

**Checkpoint CP2:** end-to-end vertical slice demo — markets → tree → propagation →
tree-weighted DCF for one real company; H4 error-concentration instrumentation live.

### T8 — Factor mapping + pricing harness (S7/C4) — L (highest risk, fail-fast prototype first)
- T8a: prototype (S): hand-built tree for 5 companies; loadings = cash-flow sensitivity to
  node outcomes; run H3/T1 cross-sectional pricing test vs FF5/AMF baseline. AC: pricing
  errors computed; verdict on H3a/H3c recorded. **Kill gate: if prototype refutes H3, T8b
  is re-scoped before building.**
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
