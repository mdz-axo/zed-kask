---
dcterms:title: "Design Hypothesis Dossier — H1–H5"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
---

# Design Hypothesis Dossier

Each hypothesis: FINER gate → PICO structure → H₁/H₀ → multiple working hypotheses →
minimal counterfactual → discriminating test(s) → evidential status.
Status vocabulary (falsifiability discipline): **corroborated** (withstood a test that could
have falsified it) / **refuted** / **open** / **survived_by_default** (no test available).
Never "confirmed".

---

## H1 — Prediction-market contracts capture elements of systemic risk for a specific company

**FINER**: Feasible 8 (market data + company data both live in-platform), Interesting 9,
Novel 7 (Wolfers & Zitzewitz established aggregation; firm-level systemic-risk extraction is
less trodden), Ethical 9 (public data), Relevant 8. Weakest: Novelty — refine toward
*firm-specific* systemic decomposition, not market-level.

**PICO**: Population = US-listed equities with ≥3 thematically linked liquid prediction-market
contracts; Intervention = augmenting a company risk model with market-implied event
probabilities composed into a scenario tree; Comparison = same company's risk model using
only historical/market-data factors (FF5/AMF); Outcome = out-of-sample Brier/log-score of
downside-event predictions and factor-model residual variance explained.

**H₁**: For companies with linked liquid contracts, scenario-tree-augmented risk models will
explain more out-of-sample downside variance than factor-only baselines.
**H₀**: No difference in out-of-sample downside prediction between augmented and baseline models.

**Multiple working hypotheses**:
- H1a (aggregation): contract prices aggregate dispersed private info about firm-relevant events (C16, C17). Falsifier: augmented model adds no predictive power even where contracts are liquid and thematically tight.
- H1b (public-signal feedback): prices are endogenous public signals that reintroduce multiplicity/noise (C12) — they move beliefs without adding information. Falsifier: price changes predict *subsequent* fundamentals beyond their correlation with contemporaneous news.
- H1c (liquidity mirage): apparent signal is bid/ask bounce + thin-volume noise. Falsifier: signal survives spread/volume/reliability-tier controls (the platform's `MarketRecord` annotation makes this testable).
- H1d (venue fragmentation): cross-venue price deviations (C18, 2–4%) swamp firm-level signal. Falsifier: single-venue and cross-venue signal strengths are statistically indistinguishable.

**Minimal counterfactual**: do(no linked prediction markets) — would the company's downside
events still be predictable at the same accuracy from factor models + news? Natural experiment:
firms that gain/lose contract coverage (contract listing/delisting events) — difference-in-differences
on forecast accuracy around coverage changes.

**Discriminating tests** (coverage matrix):
| Test | H1a | H1b | H1c | H1d |
|---|---|---|---|---|
| T1: Out-of-sample downside Brier, augmented vs baseline, tier-controlled | corroborates | neutral | falsifies | neutral |
| T2: Lead-lag: price changes vs subsequent fundamentals, news-controlled | corroborates | falsifies | neutral | neutral |
| T3: Single-venue vs cross-venue signal strength | neutral | neutral | neutral | falsifies |
| T4: Diff-in-diff around contract listing/delisting | corroborates | falsifies | falsifies | neutral |

**Numeric falsifier threshold** (from the parallel plan's suite, adopted): out-of-sample
ΔR² < 0.01 vs historical-volatility baseline refutes H1. *Provenance: Hypothesis-tier
design parameter — no extracted source grounds 0.01; re-derive from baseline noise levels
during T8a before the test runs.*

**Status: open.** No test run. T1 is buildable with existing servers (markets + scenarios + companies) — schedule first.

---

## H2 — Equity risk is long-duration; a duration model is required to match risk/return across prediction-market horizons

**FINER**: Feasible 7 (DCF timing exists in companies server; duration metric does not),
Interesting 8, Novel 8 (equity duration literature is thin and model-sensitive; linking it to
contract ladders is new), Ethical 9, Relevant 8. Weakest: Feasibility — implied equity duration
is estimation-sensitive (F2, FRAGILE).

**PICO**: Population = equities with full DCF inputs in `hkask-mcp-companies` + linked contract
ladders; Intervention = computing an equity-duration measure (Dechow–Sloan–Soliman implied
duration and/or Leibowitz franchise-value decomposition) and duration-matching contract
selection; Comparison = naive horizon-matching (contract deadline nearest to forecast horizon);
Outcome = stability of risk/return mappings (variance of implied risk premia across horizon
pairs) and forecast gap decomposition error.

**H₁**: Duration-matched contract selection produces more stable implied risk premia across
horizons than deadline-nearest matching.
**H₀**: No difference in risk-premium stability between duration-matched and deadline-matched selection.

**Multiple working hypotheses**:
- H2a: equity cash flows are back-loaded (terminal value dominates DCF PV), so effective duration ≫ contract durations — maturity transformation is real and must be modeled. Falsifier: computed equity durations cluster near typical contract horizons (<1yr) for most firms.
- H2b: duration is firm-heterogeneous (growth vs value) but stable within firm — a per-firm scalar suffices. Falsifier: within-firm duration estimates vary more across model assumptions than across firms.
- H2c: duration mismatch is second-order vs probability-estimation error — the whole axis is noise relative to the risk axis. Falsifier: duration-matching changes risk-premium estimates by more than probability-estimation error bars.

**Minimal counterfactual**: do(no duration model — use deadline-nearest matching). If risk/return
mappings are statistically indistinguishable, the duration model is cut (essentialist G1).

**Discriminating tests**:
| Test | H2a | H2b | H2c |
|---|---|---|---|
| T1: Compute implied equity duration distribution across coverage universe | falsifies if ≈ contract horizons | neutral | neutral |
| T2: Within-firm vs cross-firm duration variance under model perturbation | neutral | falsifies | neutral |
| T3: Risk-premium stability, duration-matched vs deadline-matched | corroborates | corroborates | falsifies |

**Status: open.** T1 is computable today from `dcf_valuation` outputs (stage years + PV split) — zero new infrastructure.

---

## H3 — Scenario event trees yield APT-relevant factor exposures (the scenario graph is a factor model)

**FINER**: Feasible 6 (requires new composition machinery — the largest build), Interesting 9,
Novel 9 (no located literature maps prediction-market event trees to APT factors), Ethical 9,
Relevant 9. Weakest: Feasibility — this is the research core.

**PICO**: Population = companies × scenario trees built from linked contracts; Intervention =
treating tree nodes as factors and company cash-flow sensitivities to node outcomes as factor
loadings; Comparison = statistical factors (AMF, arXiv:1804.08472) and FF5; Outcome = (i)
cross-sectional pricing: does the scenario factor model price test assets (linear relation
between expected return and scenario-factor covariances, per sr216); (ii) time-invariance of
loadings (per arXiv:2011.04171).

**H₁**: Scenario-graph factor loadings satisfy the APT linear pricing relation cross-sectionally
with pricing errors comparable to statistical factor models.
**H₀**: Scenario-graph loadings show no linear pricing relation beyond statistical-factor benchmarks.

**Multiple working hypotheses**:
- H3a (structural factors): event nodes are *causal* factors — they name the mechanism, unlike statistical factors. Falsifier: scenario factors price no better than a equal-numbered set of principal components.
- H3b (static-portfolio trap): sr216's warrant covers static portfolios only (C2); scenario trees are inherently dynamic (probabilities update), so APT's linear relation need not hold. Falsifier: pricing errors remain small under probability updates without re-estimating loadings.
- H3c (spanning): scenario factors are spanned by traded factors — they add interpretation, not pricing power. Falsifier: scenario factors retain significant pricing error reduction after projecting onto FF5/AMF span.
- H3d (joint-contract bridge): parlay/joint AMMs (C19) already price tree branches; the scenario graph is redundant with market-priced joints. Falsifier: tree-implied joint probabilities deviate from parlay-market prices beyond transaction costs.

**Minimal counterfactual**: do(no scenario graph — regress returns on raw contract price changes
directly). If raw contract factors price as well as tree-composed factors, the composition
algebra is cut.

**Loading construction note** (phase2-review B2): loadings are cash-flow sensitivities
elicited via `branch_return` revaluation, not covariances with branch indicators —
indicators over mutually exclusive branches are collinear and mechanically determined by
branch probabilities.

**Numeric falsifier threshold** (adopted, flagged): stage-2 out-of-sample ΔR² < 0.005 on
FF5/AMF residuals refutes H3. *Hypothesis-tier design parameter; re-derive at T8a.*

**Discriminating tests**:
| Test | H3a | H3b | H3c | H3d |
|---|---|---|---|---|
| T1: Cross-sectional GRS-style pricing test vs FF5/AMF | corroborates | neutral | falsifies | neutral |
| T2: Loading stability under probability updates | neutral | falsifies | neutral | neutral |
| T3: Spanning regression onto traded factors | neutral | neutral | falsifies | neutral |
| T4: Tree joints vs parlay-market prices | neutral | neutral | neutral | falsifies |

**Status: open.** Depends on the composition algebra (Workstream 3) — highest-risk, schedule
fail-fast prototype early.

---

## H4 — Simple time/return math + complex risk math is the right complexity allocation

**FINER**: Feasible 9 (it is a design choice, testable by construction), Interesting 7,
Novel 6, Ethical 9, Relevant 8.

**PICO**: Population = the foundation's model components; Intervention = the constrained
allocation (single duration mapping; expected return implied by prices/probabilities; all
model budget in event-tree probabilities + risk); Comparison = two alternatives: (i) uniform
simplicity (scalar risk too), (ii) rich time model (term structure of equity risk premia);
Outcome = model failure modes: where do forecast errors concentrate under each allocation?

**H₁**: Forecast errors under the constrained allocation concentrate less in risk-relevant
dimensions (downside events) than under uniform simplicity, and no more in time/return
dimensions than under the rich-time alternative.
**H₀**: Error concentration is allocation-invariant.

**Essentialist deletion test per axis** (G1: delete the component; does complexity reappear?):
- **Time axis (single duration mapping)**: delete → callers must each reconcile contract
  horizons with cash-flow timing ad hoc; complexity reappears in every forecast. **Survives G1.**
  But a *term structure* of equity premia: delete → only H2-style mismatch error grows, and only
  if H2c is refuted. **Conditional: keep simple unless H2c fails.**
- **Return axis (price/probability-implied expected return)**: delete → callers re-derive
  implied returns from prices inconsistently. **Survives G1 as a thin, deep module.**
- **Risk axis (event-tree probabilities + volatility + payoff uncertainty)**: delete → the
  entire foundation collapses to factor regression; the differentiating capability vanishes.
  **Survives G1 maximally — this is where complexity belongs.**
- Bookstaber's "coarse approach" (C14) independently supports coarse time/return machinery.

**Discriminating test**: build the minimal model first (uniform simplicity); instrument where
its errors concentrate; add complexity only at the measured concentration. If errors concentrate
in event probabilities/risk → H4 corroborated; if in timing → H4 refuted, escalate time axis.
**Retrospective threshold** (adopted, flagged): a complex-time model (stochastic discount
factor) improving H1's ΔR² by >0.05 refutes H4. *Hypothesis-tier design parameter.*

**Status: open** (test is a plan-phase gate, cheap to run).

---

## H5 — LLM/AI-mediated analysis enables economically new analyses here

**FINER**: Feasible 9, Interesting 7, Novel 5, Ethical 8, Relevant 6.

**Popper admissibility gate**: as stated ("economically new") this is **subjunctive and
borderline-inadmissible** — "new" is not observable. Refined testable form: "LLM mediation
reduces the marginal cost of constructing a citation-pinned, probability-annotated scenario
tree for one company below the cost of the equivalent human-analyst workflow by a measurable
factor, at equal or better calibration."

**H₁ (refined)**: LLM-mediated tree construction achieves equal calibration (Brier) at lower
analyst-hours per tree than the manual baseline.
**H₀**: No difference in calibration-adjusted cost.

**Falsifier threshold** (adopted, flagged): LLM trees ≥30% more events but Brier worse by
>0.05 → refuted in strong form; refinement path: LLM generates, human calibrates.
*Thresholds are Hypothesis-tier design parameters, not sourced values.*

**Discriminating test**: paired construction exercise — N companies, same information set,
LLM-mediated vs manual tree construction; compare Brier on resolved events and hours logged.
**Status: open. Load-bearing only for "why now" — the math stands if H5 is refuted.**

---

## Falsification suite summary

| Hypothesis | First test to run | Infrastructure needed | Refutation would mean |
|---|---|---|---|
| H1 | T1: tier-controlled out-of-sample downside Brier | existing servers + harness | markets carry no firm-level systemic signal |
| H2 | T1: implied equity duration distribution | DCF outputs only | duration axis is decorative |
| H3 | T1: cross-sectional pricing test | composition algebra (WS3) | scenario graph is interpretive, not pricing |
| H4 | error-concentration instrumentation on minimal model | WS4 prototype | complexity budget misallocated |
| H5 | paired construction cost/calibration study | none | "why now" weakens; math unaffected |
