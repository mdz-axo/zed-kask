---
dcterms:title: "CMP — Constant-Maturity Prediction Indices (Foundation Layer)"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
pko:procedure-target: "Define and construct constant-maturity prediction indices as the prerequisite for all downstream composition, duration-matching, and risk work"
---

# CMP — Constant-Maturity Prediction Indices (Foundation Layer)

**Status: this is the new base layer of the research plan.** All prior downstream work
(T4a composition, T5 propagation, T7 tree-weighted valuation, T8a risk core) is
**machinery awaiting controlled inputs**. The CMP layer supplies those inputs. Nothing
downstream is informative until the time axis is taken out of the equation by CMP —
that was the foundational skip, now corrected.

## 1. Why CMP comes first (the problem)

A raw prediction-market contract is a decaying instrument: a 90-day contract is an
89-day contract tomorrow. Its price moves for two entangled reasons — the event's
perceived probability changed, *and* the clock ran. Comparing such contracts across
time, across events, or against equity duration (years) is meaningless while the tenor
is uncontrolled. Every downstream quantity (tree probabilities, duration gaps,
σ_scenario) inherits the mismatch.

The fixed-income analog is exact: a constant-maturity Treasury yield ("the 10-year")
is not a bond — it is an interpolated, rolling construct that is *always* 10 years, so
its movements reflect rate expectations, not aging. A CMP index is the same construct
for prediction contracts: **always the target maturity, always the target
orientation/magnitude of change — so the only thing that moves is the probability.**

## 2. Base events (the eligible universe)

A **base event** is a contract family where some semantic form of the contract is
*always available* on Kalshi and Polymarket, and whose subject is a **systematic
factor in the global economy** — not a one-off happening.

Initial base-event set (both venues track these recurrently):

| Family | Subject | Systematic factor |
|---|---|---|
| Energy | WTI/Brent oil price level | commodity / input cost |
| Energy | Natural gas price level | commodity / input cost |
| Crypto | Bitcoin price level | risk-asset / liquidity proxy |
| Crypto | Ethereum price level | risk-asset / liquidity proxy |
| Macro | CPI / inflation prints | price level |
| Rates | Central-bank policy rate (Fed, and analogs) | discount rate |

These are chosen because (a) both venues list them continuously, (b) they map to the
discount-rate and cash-flow drivers of the fundamental forecast models (MAIA DCF/RIM),
and (c) they are the raw material for general risk measures, not company-specific bets.

## 3. The abstract general contract (the key design decision)

We do **not** index a specific contract ("Fed raises 25bp at the March meeting"). We
compose **abstract directional contracts** that abstract away the specific details and
are **never held to maturity**:

- **Rate-increase CMP** vs **rate-decrease CMP** — each a rolling synthetic portfolio
  of the specific rate contracts, normalized to a constant orientation (up/down),
  magnitude band, and maturity.
- Same for oil-up/oil-down, inflation-up/inflation-down, etc.

This is what makes the output behave like a **synthetic swap**: a rate-increase CMP is
to prediction contracts what an interest-rate swap is to bonds — a standardized,
rolling, direction-pure exposure to an underlying factor, tradable as an input to
scenario construction and usable as a general risk measure. The specific contracts are
the *constituents*; the CMP is the *instrument*.

**Consequence for eligibility**: a contract's eligibility is not just "is it about
rates" — it depends on the **semantic mapping of what the contract pertains to** and
the **orientation and magnitude of the change it predicts**. This is where the
`MarketRecord` ontology block (PKO process axis + Dublin Core state axis, already on
every record) and a FIBO subject mapping do the work: eligibility is a *semantic
match*, not a keyword match.

## 4. The three-step index process (normative spec)

### Step 1 — Define the index
- **(a) Prediction**: the abstract directional claim, e.g. "the policy rate increases
  by ≥ 25bp" — specified as (subject factor, orientation ∈ {increase, decrease},
  magnitude band).
- **(b) Constant-maturity target**: the fixed tenor. Initial set: **1-month, 3-month,
  6-month forward** CMP indices per (family, orientation, magnitude band).

### Step 2 — Eligibility rules for specific contracts
- **(a) Prediction match rules** (semantic, not keyword):
  - Subject match: the contract's object maps (via its FIBO/Dublin-Core annotation)
    to the index's subject factor.
  - Orientation match: the contract predicts a change in the index's direction
    (increase vs decrease).
  - Magnitude match: the contract's predicted change size falls within the index's
    magnitude band (e.g. "≥25bp" includes 25bp and 50bp contracts, not 10bp).
- **(b) Maturity limits**: the contract's expiration must lie within
  [target − max_dev, target + max_dev] — bounds may be absolute (e.g. ±30 days) or
  relative (e.g. ±50% of target). Contracts too close to expiry (near-deadline
  volatility regime, already flagged on `MarketRecord`) or too far out are excluded.

### Step 3 — Weight the synthetic portfolio
- Assign non-negative weights to eligible contracts so that the **weighted-average
  maturity of the portfolio matches the target maturity to within an error tolerance
  of 0.5 days (default)**.
- This is a small constrained linear system per index per day: weights w_i ≥ 0,
  Σw_i = 1, |Σw_i·T_i − T_target| ≤ 0.5 days. With two contracts bracketing the
  target it is exact; with more, the solution is the minimum-deviation weighting
  (ties broken toward higher liquidity/reliability tier — the existing
  `MarketRecord` annotations).
- **Two-factor matching** (your up/down generalization): when the index also targets
  a constant magnitude of change, the weights simultaneously match *both* targets —
  weighted-average maturity to T_target ± 0.5d AND weighted-average magnitude to the
  magnitude target within its tolerance. This is a two-constraint weighting; when
  exactly identified it is unique, when over-identified it is least-deviation in both
  dimensions.
- **Rolling**: as time passes and the front contract's maturity decays below the
  eligibility floor, its weight rolls to the next contract in the ladder — the index
  never holds a contract to maturity. The roll rule is part of the spec (weights
  shift smoothly, not cliff-edge, to avoid artificial probability jumps at roll).

## 5. What CMP outputs (the downstream interface)

Each CMP index publishes, per day:
- the **index probability** (weighted average of constituent probabilities, using the
  same weights that match maturity/magnitude),
- the **constituent weights and maturities** (full provenance — which contracts, what
  weights, why eligible),
- the **maturity-matching error** (honesty about the 0.5-day tolerance),
- the **reliability floor** (weakest constituent tier — the index is only as strong
  as its weakest eligible constituent).

These indices — not raw contracts — become the inputs to: scenario event trees (T4a
re-pointed), duration matching against equity duration (T2/T6 re-pointed), and the
risk core (T8a re-pointed). The time axis is controlled *before* any of that runs.

## 6. Open design questions (for you, before implementation)

1. **Magnitude bands**: fixed per family (rates: 25bp steps; oil: $5 or 5% bands;
   inflation: 0.1pp bands) or continuous with a tolerance? Bands are simpler and
   match how contracts are actually written; continuous is smoother but harder to
   keep eligible-contract-rich.
2. **Orientation pairs**: should increase and decrease CMPs be independent indices
   (they can both be "priced" by different constituent sets) or one signed index
   (probability of up vs down on the same constituents)? Independent pairs handle
   asymmetric contract availability; signed is cleaner semantically.
3. **Cross-venue**: constituents from one venue per index (avoids the
   law-of-one-price failure, arXiv:2601.01706) or pooled with a venue adjustment?
   Recommendation: single-venue per index, two parallel indices (Kalshi-CMP and
   Polymarket-CMP), divergence between them is itself a signal.
4. **Sparse ladders**: when fewer than two eligible contracts bracket the target,
   the index cannot match maturity within 0.5d. Publish with a "degraded" flag and
   wider stated error, or withhold the index that day (never-fabricate posture)?
   Recommendation: withhold — a CMP with uncontrolled maturity is the disease, not
   the cure.

## 7. Relationship to existing platform assets

- `MarketRecord.time_to_maturity` (T2) — the per-contract maturity input. ✅ exists.
- `market_ladder` (T2) — the per-series contract chain. ✅ exists; CMP consumes ladders.
- Reliability tiers, volatility structural flags, calibration (T-existing) — the
  eligibility and tie-breaking inputs. ✅ exist.
- Ontology block (PKO + Dublin Core) — the semantic-match substrate for eligibility.
  ✅ exists; a FIBO subject mapping for the six base-event families is the one new
  annotation needed.
- `hkask-forecast` — the natural home for the weight-solving math (pure, no MCP deps).

## 8. What changes in the plan (summary)

- **New Phase 0: CMP foundation** — base-event definition, semantic eligibility
  mapping, weight solver, roll rules, 1m/3m/6m indices for the six families.
- **T4a/T5/T7/T8a**: re-pointed at CMP outputs instead of raw contracts. The
  machinery is unchanged; the inputs become maturity-controlled.
- **H2 (duration)**: now testable properly — equity duration vs *constant* contract
  maturity, not decaying snapshots.
- **H3**: reframed per your correction (contract-price coherence / decomposition,
  not equity-return betas) — and now *possible* because CMP gives the stable
  probability series a coherence test needs.
