---
dcterms:title: "CMP — Constant-Maturity Prediction Indices (Foundation Layer)"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
pko:procedure-target: "Define and construct constant-maturity prediction indices as the prerequisite for all downstream composition, duration-matching, and risk work"
---

# CMP — Constant-Maturity Prediction Indices (Foundation Layer)

**Status: the base layer of the research plan.** All downstream work (composition,
propagation, tree-weighted valuation, risk core) is machinery awaiting controlled
inputs. CMP supplies them. Nothing downstream is informative until the time axis is
taken out of the equation.

**Design rule (user directive): the composition procedures take their settings as
variables. No magic numbers are embedded anywhere.** Every threshold below is a passed
variable with a default value; adjusting the variable adjusts the procedure.

## 1. Why CMP comes first

A raw prediction contract is a decaying instrument: a 90-day contract is an 89-day
contract tomorrow. Its price moves because the probability changed *and* because the
clock ran. Comparing such contracts across time, across events, or against equity
duration is meaningless while the tenor is uncontrolled.

A CMP index is the fixed-income constant-maturity construct applied to prediction
contracts: **always the target maturity, always the target orientation — so the only
thing that moves is the probability.**

## 2. Base events

A **base event** is a contract family where some semantic form of the contract is
always available on Kalshi and Polymarket, and whose subject is a systematic factor in
the global economy. Initial set: **oil, natural gas, bitcoin, ethereum, inflation,
interest rates** — chosen because both venues list them continuously, they map to the
drivers of the fundamental forecast models, and they have liquid futures (needed for
the volatility-based materiality thresholds, §4).

## 3. The abstract general contract

We do not index a specific contract ("Fed raises 25bp in March"). We compose **abstract
directional indices** that abstract away the specific details and are never held to
maturity. Per base event and per constant-maturity target (**1 month, 3 month,
6 month forward**), three index types:

| Index | Meaning | Composition |
|---|---|---|
| **Increase** | the factor moves up materially | contracts predicting the factor ends above the reference |
| **Decline** | the factor moves down materially | contracts predicting the factor ends below the reference |
| **Stable** | the factor stays near the reference | no-change contracts directly, or increase and decline contracts balanced against each other |

The **reference** is the current level of the factor (today's oil price, the current
policy rate, the last inflation print). All orientation is relative to it. When the
reference moves, the mapping of contracts to orientations re-derives and the index
rolls.

This makes the output behave like a **synthetic swap**: a rate-increase CMP is to
prediction contracts what an interest-rate swap is to bonds — a standardized, rolling,
direction-pure exposure to an underlying factor, usable for scenario construction and
as a general risk measure.

## 4. Materiality (type, level) — volatility-based thresholds

Materiality decides whether a contract's predicted change is big enough to count as
Increase or Decline, or small enough to count as Stable. It has two components:

- **Type**: **relative** (percent change from the reference) or **absolute** (absolute
  value of change of the base). Absolute **inherits the underlying units of the base
  contract** — basis points for interest rates, percentage points for inflation,
  dollars for oil.
- **Level**: the numeric threshold of that type.

**The level is volatility-based.** Because the initial base events have liquid futures,
the level is derived from the underlying's own volatility, so "material" means the same
thing economically across families:

```
level = k × volatility_of_the_underlying × scaling_for_the_target_maturity
```

- `volatility_of_the_underlying` — measured from the liquid futures series over a
  trailing window (a passed variable).
- `k` — how many units of volatility count as material (a passed variable).
- `scaling_for_the_target_maturity` — how the level grows with the index tenor (a
  passed variable; the 6-month index has a wider materiality level than the 1-month,
  because the factor has more time to move).

**The type follows how volatility is measured for that family**: for interest rates and
inflation, volatility in absolute units (bp, pp) → absolute type; for oil, gas, crypto,
volatility in return terms → relative type.

**Each base contract's materiality setting is reviewed individually** (user directive).
The volatility rule supplies the default; each family carries a reviewed setting
recording its type, level, how the level was derived, and any override with rationale.

A contract is material for Increase when its predicted level is at least `level` above
the reference; for Decline, at least `level` below; otherwise it belongs to Stable.

## 5. The three-step index process

### Step 1 — Define the index
- **(a) Prediction**: the abstract directional claim — (base event, orientation ∈
  {increase, decline, stable}).
- **(b) Constant-maturity target**: 1 month, 3 month, or 6 month forward.

### Step 2 — Contract eligibility
- **(a) Prediction match rules** (semantic, not keyword): the contract's object maps
  (via its FIBO/Dublin-Core annotation) to the base event; its orientation matches the
  index; its predicted change clears the materiality level (§4).
- **(b) Maturity limits**: the contract's expiration must lie within the eligibility
  window around the target maturity. The window has a minimum absolute width and a
  relative width proportional to the target (both passed variables). Contracts flagged
  near-deadline (existing `MarketRecord` structural flag) are excluded.

### Step 3 — Weight the index
- Assign non-negative weights (summing to one) to the eligible contracts so that the
  **weighted average maturity of the portfolio matches the target maturity to within
  an error tolerance** (a passed variable, default 0.5 days).
- **Stable by balancing**: when Stable is built from increase and decline contracts
  (rather than direct no-change contracts), the weights additionally balance the two
  sides so the portfolio's net orientation is zero within a tolerance (a passed
  variable). Direct no-change contracts are preferred when available; balancing is the
  fallback.
- **Rolling**: as the front contract's maturity decays below the eligibility window,
  its weight shifts smoothly to the next contract over a hand-off period (a passed
  variable) — the index never holds a contract to maturity, and the index probability
  does not jump for non-information reasons.
- **Withhold when no eligible bracket spans the target** — a CMP with uncontrolled
  maturity is the disease, not the cure (never-fabricate posture).

## 6. What each index publishes (daily)

- the **index probability** (weighted average of constituent probabilities at the index
  weights),
- the **constituent contracts, weights, and maturities** (full provenance),
- the **maturity-matching error** (against the tolerance),
- for Stable: whether direct or balanced, and the net-orientation residual if balanced,
- the **reliability floor** (weakest constituent tier).

## 7. Passed variables (the complete list — nothing else is a number)

| Variable | Role | Default |
|---|---|---|
| materiality type | relative or absolute, per base event | follows how volatility is measured for that family |
| materiality level derivation | k × volatility × tenor scaling | k = 1.0 |
| volatility window | trailing window for the underlying's volatility | 90 days |
| tenor scaling | how the level grows with target maturity | square root of tenor |
| maturity error tolerance | index maturity matching bound | 0.5 days |
| maturity window (absolute) | minimum eligibility window width | 7 days |
| maturity window (relative) | eligibility window as a fraction of target | 25% |
| stable net-orientation tolerance | balance bound for synthetic Stable | 0.05 |
| stable preference | direct no-change first, or always balance | direct first |
| roll hand-off period | days over which weight shifts at roll | 3 days |
| minimum reliability tier | weakest eligible constituent | Medium |

Every one of these is a passed variable. The composition procedures are functions of
these variables; adjusting them fine-tunes what works without touching the logic.

## 8. Relationship to existing platform assets

- `MarketRecord.time_to_maturity` and `market_ladder` — the per-contract maturity
  inputs. ✅ exist.
- Reliability tiers, volatility structural flags, calibration — the eligibility and
  tie-breaking inputs. ✅ exist.
- Ontology block (PKO + Dublin Core) — the semantic-match substrate for eligibility.
  ✅ exists; a FIBO subject mapping for the six base events is the one new annotation.
- `hkask-forecast` — the home for the weight-solving math (pure, no MCP deps).
- **New external input**: the underlying futures series for volatility measurement
  (the one data dependency CMP adds — justified because it makes materiality
  economically comparable across families).

## 9. What changes downstream (summary)

- **Phase 0 (CMP)** precedes everything: base events, materiality settings, weight
  solving, roll rules, 1m/3m/6m indices.
- Composition, propagation, tree-weighted valuation, risk core — re-pointed at CMP
  outputs instead of raw contracts.
- H2 (duration) — equity duration vs *constant* CMP maturity, not decaying snapshots.
- H3 — contract-price coherence (not equity-return betas), possible only because CMP
  gives the stable probability series a coherence test needs.
