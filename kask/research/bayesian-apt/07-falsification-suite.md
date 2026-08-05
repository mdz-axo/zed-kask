# Falsification Suite (H1–H5)

For each hypothesis: the discriminating test, the falsifier threshold, the
data required, and the execution plan. **Corroborate survivors; never
confirm.**

---

## H1 — Prediction-market-implied scenario risk predicts company realized
volatility out-of-sample.

**Test:** Panel regression. For each company-quarter *t*:
1. Build the scenario tree from prediction markets referencing the company's
   environment at time *t*.
2. Compute σ_scenario(t) via `scenario_risk_measure`.
3. Regress realized 90d volatility over [t, t+90d] on σ_scenario(t),
   controlling for 60d historical volatility.
4. Out-of-sample: train on first 70% of the panel, test on the last 30%.

**Falsifier:** ΔR² (out-of-sample, vs. historical-volatility-only baseline)
< 0.01 across ≥ 30 company-quarters. If the 95% CI of ΔR² includes 0, H1 is
refuted.

**Data required:** ≥ 30 company-quarters with (a) liquid prediction markets
on the company's environment, (b) realized volatility data. Source:
prediction-markets server (Polymarket/Kalshi) + companies server (price
data via providers).

**Execution:** WS8 Task 8.3. Requires WS4 Tasks 4.1–4.2 complete.

**Refinement if refuted:** Decompose σ_scenario into per-element risk
(regulatory, demand, supply-chain, macro). Test per-element; H1 may survive
for a subset.

---

## H2 — Equity duration >> prediction-market duration (maturity mismatch).

**Test:** Bootstrap median ratio.
1. Compute equity duration for 50 companies (continuous, from RIM cash-flow
   timing).
2. Compute prediction-market-contract duration for 100 active markets
   (deadline-weighted, probability-adjusted).
3. Compute the ratio D_equity / D_market per company-market pair (match by
   subject).
4. Bootstrap the median ratio (10,000 resamples); compute 95% CI.

**Falsifier:** The 95% bootstrap CI of the median ratio includes 2.0. If so,
the "long-duration" claim is refuted in its strong form; the
maturity-transformation framing collapses to "similar durations."

**Data required:** 50 companies with DCF/RIM outputs; 100 active
Polymarket/Kalshi markets. Both available from existing servers.

**Execution:** WS8 Task 8.2. Requires WS2 Tasks 2.1–2.3 complete.

**Refinement if refuted:** The mismatch may be sector-specific (e.g.,
long-duration tech vs. short-duration utilities). Test per-sector.

---

## H3 — Scenario-graph factor loadings have incremental cross-sectional
pricing power.

**Test:** Two-stage regression.
1. Stage 1: Regress company monthly returns on standard APT factors
   (Fama-French 3-factor + momentum + industry). Save residuals.
2. Stage 2: Regress stage-1 residuals on scenario-graph factor loadings
   β(c, b) per branch *b*.
3. Out-of-sample: train on first 70% of months, test on last 30%.

**Falsifier:** Stage-2 ΔR² (out-of-sample) < 0.005 across ≥ 100
company-months. If the scenario-graph loadings are spanned by standard APT
factors (stage-2 coefficients jointly insignificant), H3 is refuted — the
scenario graph is redundant as a factor model.

**Data required:** ≥ 100 company-months with (a) scenario trees built from
prediction markets, (b) standard APT factor returns. Source: prediction-
markets + scenarios servers + a standard factor-data feed (Fama-French
library).

**Execution:** WS8 Task 8.4. Requires WS4 Tasks 4.1, 4.3 complete.

**Refinement if refuted:** The scenario graph may be a factor model *only
when* the belief-hierarchy recursion is satisfied (Task 1.3). Test H3
conditional on the recursion check passing; if it fails unconditionally but
passes conditionally, H3 is refined (not refuted).

---

## H4 — Simple time/return + complex risk is the right complexity allocation.

**Test:** Essentialist deletion test (retrospective, after WS4 is built).
1. *Delete simple duration* → replace with flat 1-year horizon. Does
   σ_scenario's out-of-sample error increase? (Compare ΔR² of H1 test with
   vs. without duration.)
2. *Delete complex risk core* → replace with historical volatility only.
   Does the scenario-tree probability machinery become redundant? (H1 test
   cannot run — its input is gone.)
3. *Promote time to complex* (stochastic discount factor) → does it improve
   H1's ΔR² by > 0.05?

**Falsifier:** Test 3 shows ΔR² improvement > 0.05 from a complex time
model. If so, time deserves more complexity; H4 is refuted.

**Data required:** The H1 test infrastructure + a complex-time model
(stochastic discount factor implementation).

**Execution:** WS8 Task 8.5. Requires WS4 complete.

**Refinement if refuted:** Adopt the complex time model; re-allocate the
complexity budget. The three-axes spec is revised.

---

## H5 — LLM-mediated scenario composition is both more comprehensive *and*
at least as calibrated as human-only.

**Test:** Randomized crossover.
1. Select 20 forecasting questions (Goldilocks-zone, per Tetlock triage).
2. For each, produce two scenario trees: (a) LLM-mediated
   (`scenario_brainstorm` + `scenario_build`), (b) human-only (same data,
   no LLM).
3. Compare: event count, dependency depth, and out-of-sample Brier score
   (after resolution).

**Falsifier:** LLM-mediated trees are more comprehensive (≥ 30% more events)
*but* significantly less calibrated (Brier worse by > 0.05). If LLM adds
volume at the cost of calibration, H5 is refuted in its strong form.

**Data required:** 20 forecasting questions with known resolutions (≥ 3
months out). Human analysts for the control arm.

**Execution:** WS8 (not yet tasked; add as Task 8.6). Requires scenarios
server (already available).

**Refinement if refuted:** LLMs may be useful for *breadth* (event
generation) but not *calibration* (probability assignment). Refine to a
human-in-the-loop design: LLM generates, human calibrates.

---

## Summary

| Hypothesis | Test type | Falsifier | Data scale | Status |
|---|---|---|---|---|
| H1 | Panel regression, out-of-sample | ΔR² < 0.01 | ≥ 30 company-quarters | Ready to build |
| H2 | Bootstrap median ratio | CI includes 2.0 | 50 companies + 100 markets | Ready to build |
| H3 | Two-stage regression, out-of-sample | ΔR² < 0.005 | ≥ 100 company-months | Ready to build |
| H4 | Deletion test (retrospective) | Complex-time ΔR² > 0.05 | H1 infra + complex-time model | After WS4 |
| H5 | Randomized crossover | Brier worse by > 0.05 | 20 questions + human analysts | Ready to build |

**No hypothesis is confirmed by passing its test.** A surviving hypothesis
is *corroborated* until a new falsifier is devised (Popper). The suite is
designed so that each hypothesis has a concrete, executable refutation path.
