---
name: cmp-term-structure
description: "Build and use Constant-Maturity Prediction (CMP) term structures: ladder a base-event series, propose and accept an economic context, build provenance-carrying CMP indices, compose them into an event tree, test contract-price coherence, and match horizons against equity duration. Reifies the CMT-analogous CMP research program over the prediction-markets, scenarios, and companies MCP servers."
---

# CMP Term Structure

Constant-Maturity Prediction (CMP) indices are the prediction-market
analog of constant-maturity Treasury curves: fixed-tenor (1m/3m/6m)
probabilities per (family, orientation), interpolated in log-odds space
across the family's contract ladder. This skill runs the full pipeline:
ladder → context → indices → tree → coherence → duration matching.

## When to Use

- The operator wants a fixed-tenor probability for a registered base
  event (e.g. "the 3m probability of a Fed cut") rather than a decaying
  contract price.
- Building a scenario tree from market-implied priors with full
  provenance (family, tenor, orientation, venue).
- Testing whether a parlay/joint contract price is coherent with the
  tree-implied joint probability (the R5 arbitrage check).
- Matching a prediction-market horizon against a company's cash-flow
  duration (R2 duration matching).

## When NOT to Use

- The series is not registered in HKASK_PREDICTION_MARKETS_BASE_EVENTS —
  the CMP tools refuse unregistered series; register it first or use
  `market_lookup` for raw records instead.
- You only need a single market's current price — `market_lookup` is
  the right tool; CMP is for term structure.

## Instructions

### Phase 1 — Ladder and context

1. Call `market_ladder` (prediction-markets) with the series ticker.
   Read the contract maturities (time_to_maturity in fractional years)
   — this is the raw material the indices interpolate across.
2. Call `market_cmp_context_suggest` with the series. It proposes a
   curated economic context (reference level, volatility, predicted
   level, direction) with reasoning. Present the proposal to the
   operator — the operator accepts, overrides, or rejects it. Do not
   proceed with a rejected context.

### Phase 2 — Build the indices

3. Call `market_cmp_indices` with the series, venue ("both" unless the
   operator wants one), and the accepted context fields
   (reference/volatility/predicted_level/direction_up). It fetches live
   open markets, classifies each contract, solves the maturity-bucketed
   portfolios, and returns `indices` — an array of
   ProvenancedCmpIndex objects with full provenance.
4. Read the per-venue report: `withheld_buckets` are buckets with no
   eligible bracket — they are withheld, never fabricated. Report every
   withheld bucket and its rejection reasons to the operator.

### Phase 3 — Compose and test coherence

5. Pass the `indices` array verbatim to `scenario_from_cmp_indices`
   (scenarios server) with the observation date. Optionally add
   dependency edges (e.g. oil→inflation) with caller-authored
   conditionals. The composed tree is cached for coherence testing.
6. If an observed parlay/joint contract price exists for the tree's
   joint event, call `contract_price_coherence` with the market price
   and a transaction-cost band. Divergence within the band is coherent;
   beyond it is the arbitrage signal. tree_implied defaults to the
   cached tree's joint probability.
7. For the term-structure signal, call `market_cmp_index` with the
   series — the full tenor grid (7d/30d/90d/180d/1y/2y) plus the curve
   slope in log-odds/year. Tenors without cohort coverage return null
   probability — report them as unknown, never extrapolated.

### Phase 4 — Persist and match duration

8. To persist the curve for tracking, call `market_cmp_index_store`
   (curve as a tenor-constituent portfolio) or
   `market_cmp_portfolio_store` (solved maturity-bucketed portfolios).
   Both write transaction-ledger portfolios in the portfolio server.
9. For horizon matching against an equity, call `equity_duration`
   (companies server) with the symbol and read `cmp_tenor_gaps` — the
   R2 maturity-transformation gap against the fixed CMP tenors. Pair
   the equity's duration with the CMP tenor whose gap is smallest.

### Convergence

10. Gate — call `lisp_eval` with:
    - form: `(and (eq (length withheld_unexplained) 0) (eq slope_reconciled 1))`
    - env: `{ "withheld_unexplained": <withheld buckets with no rejection reason>,
              "slope_reconciled": <1 if the curve slope sign matches the accepted context direction, else 0> }`
    Every tenor in the report must have a probability or an explicit
    withheld reason, and the slope sign must reconcile with the
    accepted context direction. If not, re-run Phase 2 with an adjusted
    context (with the operator) or report the discrepancy.

## Constraints

- Per-venue indices are never pooled — do not average Kalshi and
  Polymarket indices for the same (family, tenor, orientation).
- Withheld buckets are the honest outcome. Never fill a withheld
  tenor from an adjacent one.
- The economic context is operator-accepted — never silently use the
  curated default when the operator has rejected it.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "cmp-term-structure", the tool name, and the error;
  continue with the best available information.
