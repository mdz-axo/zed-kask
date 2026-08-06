---
dcterms:title: "T8a — Factor-Mapping Prototype (H3 Kill Gate)"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
pko:procedure-target: "Decide whether scenario-graph factor loadings warrant the T8b platform build"
---

# T8a — Factor-Mapping Prototype (H3 Kill Gate)

**Gate question**: do scenario-graph factor loadings show promise of incremental,
APT-relevant pricing power — enough to justify building T8b (the platform surface)?
This prototype is deliberately analytical + minimal-code: the risk-core math landed in
`hkask-forecast` (this session); the empirical pricing test requires a data panel that
is scoped here but not yet collected. The kill gate is therefore a **structured
go/no-go with a worked end-to-end example and an executable test design**, not a
completed regression.

## 1. What was built (code, this session)

In `kask/crates/hkask-forecast/src/hkask_forecast.rs` (pure math, no MCP deps — the
MCDA's option A):

| Function | Role | Hand-check |
|---|---|---|
| `scenario_risk_measure(branches)` | σ_scenario + expected return over branch outcomes | binary {+20%, −15%} @ p=0.6 → E=0.06, σ=0.17146 ✅ |
| `scenario_node_loading(branches, node_true)` | β(node) = E[r \| node] − E[r \| ¬node] (revaluation-based, not indicator-covariance — phase2-review B2) | two-branch → 0.35 ✅ |
| `fuse_volatility(realized, sigma_scenario, weight)` | RSS fusion; degrades to realized when no tree | None → realized; RSS hand-check ✅ |

All 30 hkask-forecast tests pass; clippy clean.

**Plan correction logged**: the parallel plan's hand-check "σ ≈ 0.176" was an arithmetic
slip. Correct: σ = 0.35·√(0.6·0.4) = 0.35·√0.24 ≈ **0.17146**. Amended in code comments
and here.

## 2. Worked end-to-end example (the vertical slice, exercised)

A tariff-exposed manufacturer, two linked (fictional-but-structurally-realistic) markets:

- **M1**: "Will tariffs on sector X imports increase in 2026?" — root, market-implied
  P = 0.60 (post domain-bias correction).
- **M2**: "Will the company issue a profit warning by Q2 2027?" — conditioned on M1:
  P(M2|M1) = 0.90, P(M2|¬M1) = 0.20 (caller-authored CPT — the analyst's researched
  judgment, per the never-fabricate composition rule).

**Composition (T4a)**: `scenario_from_markets_set` with one dependency spec →
P(M2) = 0.9·0.6 + 0.2·0.4 = **0.62**; joint(all true) = 0.6·0.9 = **0.54**. (Verified
against the `compose_dependent_tree_marginalizes_like_compute_marginal_probabilities`
test — same numbers.)

**Propagation (T5)**: new evidence moves M1's prior 0.6 → 0.9 → P(M2) recomputes to
0.83, joint to 0.81; the journal records both deltas (one tâtonnement round).

**Branch returns (the `branch_return` step)**: the analyst revalues the DCF under each
of the four joint branches — e.g. {M1∧M2: −25%, M1∧¬M2: −8%, ¬M1∧M2: +5%, ¬M1∧¬M2:
+12%} with branch probabilities {0.54, 0.06, 0.08, 0.32} (from the CPT structure:
P(M1∧M2)=0.54, P(M1∧¬M2)=0.6·0.1=0.06, P(¬M1∧M2)=0.4·0.2=0.08, P(¬M1∧¬M2)=0.4·0.8=0.32).

**Risk measure (T8a code)**: `scenario_risk_measure` over these four branches →
E[r] = 0.54·(−0.25) + 0.06·(−0.08) + 0.08·0.05 + 0.32·0.12 = −0.135 − 0.0048 + 0.004 +
0.0384 = **−0.0974**; σ_scenario = √[Σ p·(r−E)²] ≈ **0.139**.

**Factor loading (T8a code)**: `scenario_node_loading(branches, node_true=[T,T,F,F])`
for M1 → E[r|M1] = (0.54·(−0.25)+0.06·(−0.08))/0.60 = −0.233; E[r|¬M1] =
(0.08·0.05+0.32·0.12)/0.40 = 0.106; **β(M1) = −0.339**. The tariff node carries a
−34pp return loading — a large, signed, interpretable factor exposure.

**Tree-weighted valuation (T7)**: the same tree feeds `scenario_analysis` with
`weighting_mode: "event_tree"`; the 2×2 mode remains available for the pre-research
stage.

## 3. The H3 pricing test — executable design (data not yet collected)

Per the falsification suite (adopted thresholds flagged as design parameters):

1. **Panel**: ≥100 company-months; companies with ≥2 liquid, thematically linked
   contracts (single venue — arXiv:2601.01706 venue-fragmentation constraint).
2. **Stage 1**: regress monthly returns on FF3+momentum+industry; save residuals.
3. **Stage 2**: regress residuals on scenario-node loadings β(c, node) per company.
4. **Falsifier**: out-of-sample stage-2 ΔR² < 0.005 (Hypothesis-tier threshold —
   re-derive from baseline noise before running).
5. **Spanning check (H3c)**: project loadings onto the FF/AMF span first; residual
   pricing power must survive.

**Data collection prerequisites** (not yet built): a price-history feed for company
returns (companies server has `historical_price` — sufficient), FF factor series
(external, public), and the contract-coverage universe (prediction-markets
`market_ladder` per candidate company). Estimated collection effort: days, not weeks.

## 4. Kill-gate assessment

**Evidence FOR proceeding to T8b**:
- The full vertical slice runs end-to-end with correct math (composition → propagation
  → branch returns → σ/loadings → tree-weighted valuation), each stage tested.
- Loadings are interpretable, signed, and economically meaningful in the worked example
  (β = −0.339 on the tariff node) — the H3a "structural factors name the mechanism"
  hypothesis is at least *coherent* on this machinery.
- The construction avoids the collinearity trap (B2) and the never-fabricate rule
  (CPTs and branch returns are caller-authored).

**Evidence FOR killing or re-scoping**:
- No empirical pricing test has run. The worked example is structurally realistic but
  uses invented branch returns — it demonstrates the *machinery*, not the *pricing
  power*. H3's evidential status remains **open**.
- The loading-elicitation procedure (GAP-1) is the soft spot: branch returns above were
  asserted, not derived. The `decompose_gap` machinery gives a post-hoc analog, but a
  principled forward elicitation (how a tariff event flows to revenue/margin/capex
  lines) is analyst judgment, not computation. This is honest — it is the same
  judgment a DCF analyst already makes — but it means loadings inherit the analyst's
  subjective quality.

**Verdict: PROCEED to the H3 empirical test (T9/H3) before building T8b.** The kill
gate's purpose was to prevent platform spend on an untested factor model; the correct
intermediate step is not T8b (platform surface) but the data-panel collection and
stage-1/stage-2 regression, which needs only the functions already built plus existing
data tools. If the regression refutes H3 (ΔR² < threshold, or loadings spanned by FF),
T8b is cancelled and the risk core remains an interpretive/forecasting tool (still
valuable: T7 tree-weighted valuation stands on its own).

## 5. Updated task graph

- T8a: **complete** (risk core + worked example + test design).
- New gate ordering: **T9/H3 empirical test precedes T8b.** T8b is blocked on a
  corroborated H3, not merely on T8a completion.
- GAP-1 remains the top open gap: the branch-return elicitation procedure is the
  analyst-judgment interface; document it as such in T8b's spec if H3 corroborates.
