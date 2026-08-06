---
dcterms:title: "Design Hypothesis Dossier — H1–H5"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
---

# Design Hypothesis Dossier

**v2 amendment (2026-08-05, user corrections):**
1. **CMP prerequisite**: every test below that consumes contract probabilities now runs on
   **constant-maturity prediction (CMP) index** inputs, not raw decaying contracts. The time
   axis is controlled before any test runs (see cmp-foundation.md).
2. **Equity-pricing discipline**: equities are priced on fundamental forecast models
   (DCF/RIM, MAIA). No CAPM, no factor betas, no equity-return regressions anywhere in this
   dossier. The arbitrage-pricing apparatus applies to the **contracts** (decomposition,
   bridging, price coherence), never to modeling stock returns. H3 is reframed accordingly.

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

**PICO**: Population = equities with full DCF inputs in `hkask-mcp-companies` + the CMP
indices over base-event families; Intervention = computing an equity-duration measure
(Dechow–Sloan–Soliman implied duration and/or Leibowitz franchise-value decomposition) and
duration-matching against the **constant** CMP tenors (1m/3m/6m); Comparison = matching
against raw decaying contract maturities (the uncontrolled baseline); Outcome = stability of
risk/return mappings (variance of implied risk premia across horizon pairs) and forecast gap
decomposition error.

**v2 note**: the comparison is now equity duration vs *constant* contract maturity — the
maturity-transformation gap is a controlled quantity only because CMP fixes the tenor. The
v1 comparison (equity duration vs decaying snapshots) was unmeasurable in principle.

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

## H3 — Scenario-tree-implied pricing is coherent with contract prices (contract-price coherence)

**v2 reframe (user correction)**: the v1 framing ("scenario graph is an equity factor model,
tested by equity-return regressions") is **withdrawn** — it priced equities off betas, which
is not how MAIA works and not what the research is for. The arbitrage-pricing apparatus
applies to the **contracts**: decomposing and bridging their prices and analyzing their
coherence. Equities stay on fundamental forecast models.

**FINER**: Feasible 8 (composition machinery exists; coherence is measurable with
`market_ladder` + tree joints, no equity-return data), Interesting 9, Novel 8, Ethical 9,
Relevant 9.

**PICO**: Population = CMP-controlled scenario trees over base-event families + the contract
ladders that price them; Intervention = composing tree-implied joint probabilities from CMP
inputs and comparing them to observed contract prices (including parlay/joint contracts where
listed); Comparison = raw (non-CMP) contract snapshots and single-contract prices;
Outcome = the coherence gap (tree-implied joint vs market joint price) relative to a
transaction-cost band.

**H₁**: Tree-implied joint probabilities from CMP-controlled composition are coherent with
observed contract prices within transaction costs; divergences beyond the band are the
analyzable arbitrage signal.
**H₀**: Tree-implied joints diverge from market joint prices beyond transaction costs
systematically (the composition adds no pricing coherence), OR raw snapshots are as coherent
as CMP-controlled trees (CMP adds nothing).

**Multiple working hypotheses**:
- H3a (coherence holds): the tree is the market's own joint structure made explicit;
  divergences are transient and within costs. Falsifier: systematic divergence beyond the
  cost band.
- H3b (CMP is the active ingredient): coherence holds only on maturity-controlled inputs;
  raw snapshots diverge because the time axis is uncontrolled. Falsifier: raw snapshots are
  as coherent as CMP trees.
- H3c (parlay markets already price joints — C19): joint contracts make the tree redundant
  as a pricing device; the tree's value is interpretive, not pricing. Falsifier: tree-implied
  joints deviate from parlay prices beyond costs in the parlay market's favor.
- H3d (venue fragmentation dominates — C18): cross-venue price deviations swamp any
  tree-level coherence. Falsifier: single-venue coherence is tight while cross-venue
  diverges — then coherence is per-venue only.

**Minimal counterfactual**: do(no composition — read each contract's price as an independent
probability). If independent prices are as coherent with joints as the tree's composed
probabilities, the composition algebra is cut (essentialist G1).

**Discriminating tests**:
| Test | H3a | H3b | H3c | H3d |
|---|---|---|---|---|
| T1: Tree-implied joint vs market joint price, cost-banded | corroborates | neutral | falsifies | neutral |
| T2: Coherence of CMP trees vs raw-snapshot trees | neutral | falsifies | neutral | neutral |
| T3: Tree joints vs parlay-market prices | neutral | neutral | falsifies | neutral |
| T4: Single-venue vs cross-venue coherence | neutral | neutral | neutral | falsifies |

**Status: open.** Blocked on CMP (Phase 0) — the coherence test needs the stable probability
series CMP provides; running it on decaying snapshots would confound time and probability.

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
