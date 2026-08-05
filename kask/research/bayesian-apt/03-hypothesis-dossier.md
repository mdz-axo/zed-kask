# Design Hypothesis Dossier (H1–H5)

Each hypothesis is taken through FINER + PICO, given a null hypothesis, a
discriminating test, and a current evidential status. **Corroborated ≠
confirmed** (Popper). Statuses: `Open`, `Corroborated (weak)`, `Refuted`,
`Refined`.

---

## H1 — Prediction-market contracts can capture elements of *systemic risk*
for a specific company being analyzed.

**FINER**
- **F**easible: Yes — prediction-markets server already ingests Polymarket/Kalshi
  markets with probability, volume, deadline, calibration. The data exists.
- **I**nterest: High — if true, this is the commercial core of the foundation.
- **N**ovelty: High — linking prediction-market microstructure to *company-
  specific* systemic risk (not just macro) is not standard practice.
- **E**thics: Low risk — uses public market data; no manipulation.
- **R**elevance: High — directly serves the equity-forecast end-state.

**PICO**
- **P**opulation: Public companies with liquid prediction markets referencing
  their operating environment (regulatory, technological, macro).
- **I**ntervention: Compose relevant prediction-market contracts into a
  scenario event tree; extract factor exposures.
- **C**omparison: Same companies' historical realized volatility and default
  events vs. the scenario-tree-implied risk measure.
- **O**utcome: Statistical association between scenario-implied risk and
  realized risk, out-of-sample.

**Null hypothesis (H0):** Prediction-market-implied scenario risk has *no*
out-of-sample predictive power for company realized volatility / default
frequency beyond what a naive historical-volatility baseline provides.

**Discriminating test:** For a panel of companies, build the scenario tree
from prediction markets at time *t*, compute the scenario-implied risk
measure σ_scenario(t), and regress realized volatility over [t, t+90d] on
σ_scenario(t) controlling for historical 60d volatility. Reject H0 if the
coefficient on σ_scenario is significant (p < 0.05, Bonferroni-adjusted)
and improves out-of-sample R² by a meaningful margin (ΔR² > 0.03).

**Falsifier:** If σ_scenario adds < 0.01 incremental R² out-of-sample across
≥ 30 company-quarters, H1 is refuted.

**Current status:** `Open`. The infrastructure exists (prediction-markets +
scenarios servers) but the composition algebra and the risk measure are not
yet built. No data collected.

**Refinement path:** "Elements of systemic risk" may decompose into
(regulatory risk, demand risk, supply-chain risk, macro risk). H1 may
survive for some elements and fail for others — test per-element, not
aggregate-only.

---

## H2 — Equity risk is long-duration relative to most other assets, so a
duration model is required to match risk/return across prediction-market
horizons.

**FINER**
- **F**easible: Yes — the companies server already has `FadeHorizon` (5/10/20y)
  and a Gordon terminal value; a continuous duration is a refinement.
- **I**nterest: High — the maturity-transformation thesis depends on it.
- **N**ovelty: Medium — equity duration is known (Damodaran discusses it);
  novelty is in *matching* it to prediction-market durations.
- **E**thics: Low.
- **R**elevance: High — load-bearing for the three-axes collapse.

**PICO**
- **P**opulation: Equity cash flows of public companies; prediction-market
  contracts with known deadlines.
- **I**ntervention: Compute Macaulay-style duration of equity cash flows
  (DCF-weighted time to cash flow) and of prediction-market contracts
  (probability-weighted time to resolution).
- **C**omparison: Distribution of equity durations vs. distribution of
  prediction-market durations.
- **O**utcome: Median equity duration >> median prediction-market duration
  (e.g., > 5×).

**Null hypothesis (H0):** Median equity duration ≤ 2× median prediction-
market-contract duration (i.e., no meaningful maturity mismatch).

**Discriminating test:** Compute equity duration for a sample of 50
companies across sectors using the existing DCF (terminal-value share is the
dominant duration driver). Compute prediction-market-contract duration for
100 active markets (deadline-weighted). Test the ratio of medians via
bootstrap CI.

**Falsifier:** If the 95% bootstrap CI of the median ratio includes 2.0, H2
is refuted in its strong form; the maturity-transformation framing collapses
to "similar durations, no transformation needed."

**Current status:** `Open`, leaning `Corroborated (weak)` from theory.
Equity terminal value typically > 70% of DCF (standard finance result,
Damodaran), implying long duration. Prediction-market contracts typically
resolve in days–months (Polymarket/Kalshi norm). The qualitative mismatch is
well-grounded; the *quantitative* ratio is not yet measured.

**Refinement:** "Long-duration" should be operationalized as a continuous
duration number, not a category. The existing `FadeHorizon` categorical
(5/10/20y) is a placeholder — H2's test forces its promotion to a continuous
measure.

---

## H3 — Composing prediction events into scenario event trees yields
arbitrage-pricing-relevant factor exposures (the scenario graph *is* a
factor model).

**FINER**
- **F**easible: Yes — the scenarios server already has `EventTree`,
  `EventDependency` conditional tables, `variance_contribution`.
- **I**nterest: High — this is the theoretical bridge from scenarios to APT.
- **N**ovelty: High — treating a scenario graph as an APT factor structure is
  novel (Bhattacharya bridges beliefs→arbitrage; we extend beliefs→scenarios
  →factors).
- **E**thics: Low.
- **R**elevance: High — without this, the scenario tree is a forecasting
  artifact, not a pricing model.

**PICO**
- **P**opulation: Scenario event trees built from prediction markets for a
  panel of companies.
- **I**ntervention: Extract factor loadings from the event tree (each
  company's exposure to each scenario branch, weighted by branch probability
  and variance contribution).
- **C**omparison:** Standard APT factor portfolios (Fama-French, momentum,
  industry) fit on the same company panel.
- **O**utcome:** The scenario-graph factor model explains cross-sectional
  expected returns at least as well as standard APT factors (R² comparison),
  *and* adds incremental explanatory power (orthogonal components).

**Null hypothesis (H0):** Scenario-graph factor loadings have no incremental
cross-sectional pricing power beyond standard APT factors (ΔR² = 0).

**Discriminating test:** Two-stage regression: (1) regress company returns
on standard APT factors, save residuals; (2) regress residuals on
scenario-graph factor loadings. Test whether stage-2 coefficients are
jointly significant and ΔR² > 0.02 out-of-sample.

**Falsifier:** If scenario-graph loadings are spanned by standard APT factors
(stage-2 ΔR² < 0.005 out-of-sample, n ≥ 100 company-months), H3 is refuted —
the scenario graph is redundant as a factor model.

**Current status:** `Open`. The algebra exists (`variance_contribution` is a
sensitivity proxy) but the extraction of *factor loadings* from the tree is
not implemented. Bhattacharya (2211.03244) provides the theoretical license
(arbitrage ↔ belief hierarchies) but does *not* assert the scenario-graph-
as-factor-model claim — that is our extension.

**Refinement:** The scenario graph may be a factor model *only when*
conditioned on the belief-hierarchy recursion being satisfied (H1 of
Bhattacharya). Test H3 conditional on the recursion check passing.

---

## H4 — Keeping time and return mathematically simple while concentrating
complexity in event-tree probabilities + risk is the right complexity
allocation.

**FINER**
- **F**easible: Yes — it's a design choice, testable via the essentialist
  deletion test.
- **I**nterest: High — misallocation wastes the complexity budget.
- **N**ovelty: Medium — the principle is Ousterhout-style; the application
  is novel.
- **E**thics: Low.
- **R**elevance: High — governs the whole architecture.

**PICO**
- **P**opulation: The three axes (time/duration, return, risk) of the target
  foundation.
- **I**ntervention:** Apply the essentialist deletion test to each axis:
  delete the component; does complexity reappear in the model's failures?
- **C**omparison:** Alternative allocations (e.g., complex time models,
  stochastic discount factors).
- **O**utcome:** The chosen allocation (simple time/return, complex risk)
  survives the deletion test; alternatives fail it.

**Null hypothesis (H0):** A complex time model (e.g., stochastic discount
factor, term-structure modeling) adds predictive power that a simple
duration mapping cannot capture.

**Discriminating test (deletion test):**
1. *Delete the simple duration model* → replace with a flat 1-year horizon
   for all assets. Does the risk calculation's error increase
   meaningfully? If yes, duration earns its place (even simple).
2. *Delete the complex risk core* → replace with historical volatility only.
   Does the scenario-tree probability machinery become redundant? If yes,
   the complexity was misplaced *toward risk* (risk earns its complexity).
3. *Promote time to complex* (stochastic discount factor) → does it improve
   out-of-sample risk pricing enough to justify the complexity? If no, time
   stays simple.

**Falsifier:** If test 3 shows ΔR² > 0.05 from a complex time model, H4 is
refuted — time deserves more complexity than the simple duration mapping.

**Current status:** `Open`, leaning `Corroborated (weak)` from essentialist
prior. The deletion test is not yet run because the risk core is not yet
built. H4 is really a *meta-hypothesis* that governs the architecture; it
will be corroborated or refuted *retrospectively* once the other axes are
built.

---

## H5 — LLM/AI-mediated analysis enables economically new analyses here
(motivational framing — "why now").

**FINER**
- **F**easible: Yes — the scenarios server is already LLM-mediated
  (`scenario_brainstorm`, `scenario_build` emit prompts).
- **I**nterest: Medium — motivational, not load-bearing for the math.
- **N**ovelty: Medium — "LLMs can compose belief hierarchies" is plausible
  but unproven.
- **E**thics: Medium — LLM-mediated financial analysis has hallucination
  risk; the citation gate mitigates.
- **R**elevance: Medium — justifies timing, not substance.

**PICO**
- **P**opulation: Analysts using the target foundation.
- **I**ntervention: LLM-mediated scenario composition + Bayesian update.
- **C**omparison: Human-only scenario composition (same data, no LLM).
- **O**utcome: LLM-mediated trees are more comprehensive (more events,
  more dependencies) *and* at least as calibrated (Brier scores not worse).

**Null hypothesis (H0):** LLM-mediated scenario trees are no more
comprehensive than human-only trees *and/or* are less calibrated (higher
Brier).

**Discriminating test:** Randomized crossover: same forecasting questions,
LLM-mediated vs. human-only. Compare event count, dependency depth, and
out-of-sample Brier. LLM wins only if it is *both* more comprehensive
(≥ 30% more events) *and* not less calibrated (Brier difference within
0.02).

**Falsifier:** If LLM-mediated trees are more comprehensive but
significantly less calibrated (Brier worse by > 0.05), H5 is refuted in its
strong form — LLMs add noise, not insight.

**Current status:** `Open`. The scenarios server's `scenario_brainstorm`
already produces LLM-mediated trees; no controlled comparison vs. human-only
has been run. H5 is *not load-bearing* for the math foundation — even if
refuted, the math (H1–H4) stands; only the "why now" framing changes.

---

## Summary table

| Hypothesis | Status | Load-bearing? | Falsifier threshold |
|---|---|---|---|
| H1 (PM → systemic risk) | Open | Yes | ΔR² < 0.01 out-of-sample → refuted |
| H2 (equity long-duration) | Open, weakly corroborated | Yes | Median ratio CI includes 2.0 → refuted |
| H3 (scenario graph = factor model) | Open | Yes | Stage-2 ΔR² < 0.005 → refuted |
| H4 (complexity allocation) | Open, weakly corroborated (meta) | Yes (architectural) | Complex-time ΔR² > 0.05 → refuted |
| H5 (LLM enables new analyses) | Open | No (motivational) | LLM less calibrated by > 0.05 Brier → refuted |
