---
dcterms:title: "Falsification Log — H1–H5 (CMP-controlled)"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-07"
rdf:type: bibo:Document
pko:procedure-target: "Record the falsification test results for H1–H5 on CMP-controlled inputs."
---

# Falsification Log — H1–H5 (CMP-controlled)

**Status vocabulary**: corroborated (withstood a test that could have falsified it) / refuted / open / survived_by_default (no test available) / blocked (test exists but needs infrastructure).

## Summary

| Hypothesis | Status | Test | Falsifier |
|---|---|---|---|
| H1 (systemic risk capture) | **blocked** | T1: tier-controlled out-of-sample downside Brier | ΔR² < 0.01 vs baseline |
| H2 (duration) | **open** (test computable) | T1: implied equity duration vs CMP tenors | durations cluster near <1yr |
| H3 (contract-price coherence) | **open** (test computable) | T1: tree-implied joint vs market price, cost-banded | coherence rate < 50% |
| H4 (complexity allocation) | **open** | error-concentration instrumentation | errors concentrate in risk dimensions |
| H5 (LLM leverage) | **blocked** | paired construction cost/calibration study | Brier worse by >0.05 at 30%+ more events |

## H1 — Systemic risk capture

**Status: blocked.**

The test (T1: tier-controlled out-of-sample downside Brier, augmented vs baseline)
needs live company data + CMP-controlled scenario trees running through the full
MCP stack. The CMP foundation (Phase 0) and composition machinery (R1) are
landed; the empirical test requires the companies MCP server + a Brier scoring
loop over resolved events.

**Falsifier**: augmented model adds no predictive power (ΔR² < 0.01) even where
contracts are liquid and thematically tight.

**What's needed to unblock**: a historical backtest over resolved CMP indices,
feeding `compose_cmp_tree` outputs into company risk models, and scoring
out-of-sample downside predictions with Brier.

## H2 — Duration

**Status: open (test computable today).**

The test (T1: implied equity duration distribution vs fixed CMP tenors) is
computable using `h2_duration_test(equity_duration_years)` from
`hkask-forecast::falsification`. The function compares the equity duration
against the 1m/3m/6m CMP tenors and checks the falsifier: do equity durations
cluster near contract horizons (<1yr)?

**Falsifier**: computed equity durations cluster near typical contract horizons
(<1yr) for most firms — the minimum ratio (duration / nearest CMP tenor) is
< 2.0.

**Preliminary assessment**: a typical equity duration of 10 years produces a
minimum ratio of ~20× (10y / 0.493y for the 6m tenor). The falsifier is far
from triggered — H2a (maturity transformation is real) is corroborated for
typical equities. The test needs to be run across the full company universe
to confirm the distribution doesn't have a short-duration tail.

## H3 — Contract-price coherence

**Status: open (test computable today).**

The test (T1: tree-implied joint vs market joint price, cost-banded) is
computable using `h3_coherence_test(pairs, cost_band)` from
`hkask-forecast::falsification`. The function measures the coherence between
CMP-controlled tree-implied joint probabilities and observed market prices,
within a transaction-cost band.

**Falsifier**: systematic divergence beyond the cost band (coherence rate < 50%
across the tested pairs).

**What's needed to run**: (tree_implied, market_price) pairs from CMP-controlled
trees and observed parlay/joint contract prices. The CMP trees are buildable
via `compose_cmp_tree`; the parlay prices need to be fetched from the venues.

## H4 — Complexity allocation

**Status: open.**

The test (error-concentration instrumentation on the minimal model vs the
constrained allocation) requires a running forecast loop with resolved outcomes.
The CMP foundation (Phase 0) supplies the controlled inputs; the test needs a
historical backtest over resolved CMP indices to measure where forecast errors
concentrate.

**Falsifier**: forecast errors concentrate in risk-relevant dimensions (downside
events) under the constrained allocation, or in time/return dimensions under the
rich-time alternative.

## H5 — LLM leverage

**Status: blocked.**

The test (paired construction cost/calibration study) needs a human-in-the-loop
paired study. The LLM-mediated construction path (`compose_cmp_tree`) is landed;
the study requires analyst-hours tracking and a manual baseline.

**Falsifier**: LLM trees ≥30% more events but Brier worse by >0.05 → refuted in
strong form.

## No equity-return beta machinery

Per the user correction (plan.md "Equity pricing discipline"): equities are
priced on fundamental forecast models (DCF/RIM, MAIA). No CAPM, no factor betas,
no equity-return regressions anywhere in this falsification suite. The
arbitrage-pricing apparatus applies to the **contracts** (decomposition,
bridging, price coherence), never to modeling stock returns.

The falsification suite's public API (`h2_duration_test`, `h3_coherence_test`,
`falsification_log`) takes no equity returns, betas, or factor loadings as
inputs. The only equity input is `equity_duration_years` (a DCF output, not a
return). This is pinned by the `no_equity_return_beta_machinery_anywhere` test.
