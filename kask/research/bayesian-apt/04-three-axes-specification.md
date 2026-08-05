# Three-Axes Specification: Time/Duration, Return, Risk

**Design constraint (OUGHT/Core):** Keep the math of time and return as
simple as possible so the complexity budget is spent on the event-tree
probabilities and the risk calculations. This allocation is justified by the
essentialist deletion test (H4), not by assertion.

---

## Axis 1 — Time / Duration (SIMPLE)

### 1.1 The model

A single, transparent duration mapping for both equity and prediction-market
contracts, derived from cash-flow timing.

**Equity duration** (Macaulay-style, on free cash flow):

```
D_equity = Σ_t [ t · FCF_t / (1+r)^t ] / Σ_t [ FCF_t / (1+r)^t ]
```

where `FCF_t` is the projected free cash flow at year *t* (from the existing
`financial_model.rs` two-stage DCF), `r` is the discount rate (WACC), and the
sum runs over the explicit forecast horizon plus the terminal value (treated
as a single cash flow at the horizon's end, per standard practice).

**Prediction-market-contract duration:**

```
D_market = (deadline − now) in years,   probability-weighted by resolution certainty
```

For a binary market with probability *p* and deadline *T*:
`D_market ≈ T · (1 − |2p − 1|)` — the effective duration shrinks as the
market approaches certainty (a 99% market resolves "early" in expectation
because the outcome is nearly known). This is deliberately simple; it uses
only the `deadline` and `probability` fields already in `MarketRecord`.

### 1.2 Why simple (deletion-test result)

- **Delete the simple duration model** → replace with a flat 1-year horizon
  for all assets. *Predicted consequence:* the risk calculation cannot
  distinguish a 30-year equity from a 30-day prediction market; the
  maturity-transformation thesis (H2) becomes untestable. Complexity
  reappears as *unexplained risk-structure drift*. → Duration earns its
  place, even simple.
- **Promote time to complex** (stochastic discount factor, term-structure
  modeling) → *Predicted consequence:* the complexity budget is consumed by
  time, leaving insufficient budget for the event-tree probability machinery
  (the actual locus of uncertainty). The risk core degrades. → Complex time
  fails the deletion test *in the other direction*: it over-earns complexity.
- **Result:** Simple duration survives. The complexity is in *what* duration
  measures (the cash-flow timing, which depends on the scenario tree), not
  in *how* it is computed (a weighted average).

### 1.3 Implementation surface

- **Equity:** Promote `FadeHorizon` from categorical (5/10/20y) to
  continuous. The RIM already computes `EP_t` per year; the duration is a
  byproduct of the same loop. Add a `duration: f64` field to the valuation
  output.
- **Markets:** Add a `duration: f64` field to `MarketRecord`, computed in
  `assemble` from `deadline_days` and `probability` (both already present).
- **Matching:** A single function `match_durations(D_equity, D_market) ->
  DurationGap` returns the ratio and a transformation-weight. This is the
  maturity-transformation operator.

### 1.4 What this axis does NOT do (by design)

- No term-structure modeling.
- No stochastic discount factor.
- No yield-curve construction.
- No interest-rate-path simulation.

These are explicitly out of scope. If H4's falsifier (complex-time ΔR² >
0.05) fires, this boundary is revisited.

---

## Axis 2 — Return (SIMPLE)

### 2.1 The model

Expected return implied by prices/probabilities, with no factor-model
machinery on this axis (factors live on the risk axis).

**Prediction-market-implied return:**

For a binary market at price *p* (≈ probability) paying 1 if YES:
`E[return_market] = (1 − p) / p` per unit at risk, over the contract's
duration. Annualized: `(1 + E[return_market])^(1/D_market) − 1`.

**Equity expected return** (from the companies server's existing DCF/RIM):
`E[return_equity] = (IV / current_price)^(1/D_equity) − 1`, where IV is the
intrinsic value from the RIM and `current_price` is the market price. This is
the implied annualized return of holding the equity to its intrinsic-value
realization.

### 2.2 Why simple (deletion-test result)

- **Delete the return model** → return is just "price goes up/down." The
  foundation cannot compare a prediction-market bet to an equity holding.
  Complexity reappears as *incomparable position sizing*. → Return earns its
  place, even simple.
- **Promote return to complex** (CAPM beta, multi-factor expected-return
  models) → the factor machinery collides with the risk axis (H3), creating
  a duplicated complexity surface. → Complex return fails the deletion test
  (redundant with risk).

### 2.3 What this axis does NOT do

- No CAPM. No Fama-French expected-return factors. No implied-cost-of-capital
  inversion beyond the simple IV/price ratio.
- The *factor exposures* that APT requires live on the **risk** axis, derived
  from the scenario tree. Return is the *consequence* of price + probability,
  not a separate factor model.

---

## Axis 3 — Risk (COMPLEX)

### 3.1 The model

This is where the research effort concentrates. Risk has three components,
fused into one measure:

1. **Volatility** — from the prediction-market price series (realized) and
   the structural-flag machinery (`NearDeadline`, `NearCoinflip`).
2. **Default / payoff uncertainty** — the probability that the scenario
   branch resolves against the position, weighted by the conditional
   probability table.
3. **Event-tree Bayesian probabilities** — the full joint distribution over
   scenario outcomes, propagated through the `EventDependency` conditional
   tables via `compute_marginal_probabilities`.

### 3.2 The risk core (formal sketch)

For a company *c* with scenario tree *T*:

```
σ_scenario(c) = sqrt( Σ_{b ∈ branches(T)} p(b) · [r(c|b) − E[r(c)]]² )

where:
  p(b)         = joint probability of branch b (from EventTree.joint_probability
                 machinery, extended to per-branch)
  r(c|b)       = company return conditional on branch b (from the DCF/RIM
                 re-evaluated under branch b's assumptions)
  E[r(c)]      = Σ_b p(b) · r(c|b)   (probability-weighted expected return)
```

This is the **scenario-implied volatility**: the standard deviation of
company returns across the scenario tree's branches, weighted by branch
probabilities. It is the direct descendant of the existing
`variance_contribution` field on `EventTreeNode`, promoted from a per-node
sensitivity proxy to a per-company aggregate risk measure.

### 3.3 The APT bridge (factor exposures)

The scenario tree's branches *are* the factors. Each branch *b* defines a
factor portfolio (long companies that benefit from *b*, short those that
don't). The company's loading on factor *b* is:

```
β(c, b) = Cov(r(c), 1_b) / Var(1_b)
```

where `1_b` is the branch indicator. Because branches are
probability-weighted and conditionally dependent (via `EventDependency`),
this is a *Bayesian* factor model — the factor structure is the belief
hierarchy, per Bhattacharya (2211.03244).

### 3.4 Why complex (deletion-test result)

- **Delete the complex risk core** → replace with historical volatility only.
  *Predicted consequence:* the foundation cannot distinguish a company whose
  risk comes from a low-probability high-impact scenario branch (tail risk)
  from one whose risk comes from uniform variance. The scenario tree becomes
  a forecasting artifact with no pricing consequence. Complexity reappears as
  *unexplained tail events*. → The complex risk core earns its place.
- **Simplify risk to historical volatility** → H1 and H3 are untestable
  (there's no scenario-implied risk measure to regress against realized
  risk). → Simplification fails the deletion test (disables the hypotheses).

### 3.5 What this axis DOES do (by design)

- Full Bayesian event-tree probability propagation (already exists in
  `compute_marginal_probabilities`).
- Per-branch company return re-evaluation (new: re-run DCF/RIM per branch).
- Factor-loading extraction from the tree (new: the APT bridge).
- Volatility fusion: combine realized market volatility, structural flags,
  and scenario-implied volatility into a single risk number (new).

### 3.6 Implementation surface (new capabilities required)

- `scenario_risk_measure(tree, company) -> RiskMeasure` — the aggregate
  σ_scenario.
- `scenario_factor_loadings(tree, company) -> Vec<(BranchId, f64)>` — the
  APT factor loadings.
- `branch_return(company, branch) -> f64` — re-evaluate DCF/RIM under a
  branch's assumptions.
- `fuse_volatility(realized, structural, scenario_implied) -> f64` — the
  fusion operator.

---

## Complexity-budget summary

| Axis | Complexity | Deletion-test verdict | Justification |
|---|---|---|---|
| Time/Duration | Simple (weighted average) | Survives | Deleting → unexplained risk-structure drift; promoting → steals budget from risk |
| Return | Simple (price/probability ratio) | Survives | Deleting → incomparable positions; promoting → duplicates risk axis |
| Risk | Complex (Bayesian event tree + factor extraction + volatility fusion) | Survives | Deleting → unexplained tail events; disables H1, H3 |

**The complexity allocation is justified, not assumed.** Each axis's verdict
is tied to a falsifier in H4.
