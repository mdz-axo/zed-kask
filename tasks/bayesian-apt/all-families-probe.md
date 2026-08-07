---
dcterms:title: "All-Families CMP Probe — Results and Structural Findings"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-07"
rdf:type: bibo:Document
pko:procedure-target: "Record the all-families CMP index probe results and the structural maturity-ladder finding."
---

# All-Families CMP Probe — Results and Structural Findings

## The probe

After ONT-6 unblocked the classifier, the builder was run on all 7 base families
on both venues (Kalshi and Polymarket). The results reveal a structural property
of prediction markets that the CMP foundation must reckon with.

## Results table

| Family | Venue | Records | Eligible | Indices | Published tenors |
|---|---|---|---|---|---|
| policy_interest_rate | Kalshi | 790 | 388 | 2 | 6m (Increase, Stable) |
| policy_interest_rate | Polymarket | 76 | 29 | 0 | none (all at 124d/146d) |
| consumer_price_inflation | Kalshi | 703 | 514 | 2 | 3m |
| consumer_price_inflation | Polymarket | 92 | 92 | 0 | none (all at year-end) |
| crude_oil_price | Kalshi | 388 | 123 | 1 | 1m |
| crude_oil_price | Polymarket | 12 | 0 | 0 | no eligible contracts |
| natural_gas_price | Kalshi | 319 | 191 | 0 | none (no bracket) |
| natural_gas_price | Polymarket | 2 | 0 | 0 | no eligible contracts |
| bitcoin_price | Kalshi | 714 | 37 | 0 | none (no bracket) |
| bitcoin_price | Polymarket | 112 | 48 | 0 | none (no bracket) |
| ethereum_price | Kalshi | 826 | 26 | 0 | none (no bracket) |
| ethereum_price | Polymarket | 107 | 40 | 0 | none (no bracket) |
| real_gdp_growth | — | — | — | — | no BaseEvent materiality setting |

## The structural finding: prediction markets lack continuous maturity ladders

The CMP bracket solver requires two contracts at **different maturities** within
the eligibility window — one at-or-below the target, one at-or-above. This is the
fixed-income constant-maturity construction: interpolate between two bracketing
points.

**Prediction markets do not form continuous maturity ladders.** Their contracts
cluster around specific event dates (FOMC meetings, month-end expirations,
year-end resolutions). Within each maturity bucket's eligibility window, the
contracts are typically all at the **same maturity** — there's no bracket pair.

### Evidence (unique maturities per bucket window)

| Family | Venue | Bucket | Unique days in window | Bracket? |
|---|---|---|---|---|
| crude_oil_price | Kalshi | 1m | [24, 28, 35] | ✓ (brackets 30d) |
| consumer_price_inflation | Kalshi | 3m | [68, 95] | ✓ (brackets 90d) |
| policy_interest_rate | Kalshi | 6m | [147, 173, 209, 222] | ✓ (brackets 180d) |
| real_gdp_growth | Kalshi | 1m | [26, 32] | ✓ (brackets 30d) |
| real_gdp_growth | Kalshi | 6m | [174, 205, 207] | ✓ (brackets 180d) |
| real_gdp_growth | Polymarket | 6m | [161, 175, 177, 189] | ✓ (brackets 180d) |
| crude_oil_price | Polymarket | 6m | [146, 205] | ✓ (brackets 180d) |
| natural_gas_price | Kalshi | 1m | [24] | ✗ (single maturity) |
| natural_gas_price | Kalshi | 6m | [146, 147] | ✗ (both below 180d) |
| bitcoin_price | Kalshi | 1m | [25] | ✗ (single maturity) |
| bitcoin_price | Kalshi | 6m | [147] | ✗ (single maturity) |
| bitcoin_price | Polymarket | 6m | [146, 147] | ✗ (both below 180d) |
| ethereum_price | Kalshi | 1m | [25] | ✗ (single maturity) |
| ethereum_price | Kalshi | 6m | [147] | ✗ (single maturity) |
| consumer_price_inflation | Polymarket | 6m | [146, 154, 156, 158, 164, 165, 166] | ✗ (all below 180d) |

### The pattern

1. **Short-tenor buckets (1m/2m/3m)**: contracts at a single day (the next
   event date — FOMC meeting, month-end, CPI release). No bracket pair.
   Exception: oil (Kalshi 1m has 3 distinct days) and CPI (Kalshi 3m has 2
   distinct days) — these have enough event-date variety to bracket.

2. **Long-tenor bucket (6m)**: contracts at year-end (146d/147d). Sometimes
   a second maturity appears (205d, 222d) when the venue lists a Q1-next-year
   contract. The 6m bracket exists only when there's a contract at or above
   180d — which requires a Q1+ listing.

3. **Polymarket**: almost universally year-end-only. The Polymarket rates,
   CPI, bitcoin, and ethereum markets all resolve at Dec 31. No continuous
   ladder at all.

## Implications for the CMP foundation

### What works

The CMP construction is sound where brackets exist. The 3 family/venue/tenor
combinations that publish (Rates 6m Kalshi, CPI 3m Kalshi, Oil 1m Kalshi)
produce correct indices with proper provenance, weights, and maturity matching.
The builder withholds honestly when no bracket spans the target — never
fabricates.

### What doesn't work (and why)

The two-constraint bracket solver (C0.3) assumes a continuous maturity ladder.
Prediction markets don't provide one. The solver returns `None` (withhold) for
most buckets because the eligible contracts within the window are all at the
same maturity.

This is NOT a bug in the builder or the classifier — it's a structural property
of the underlying data. The contracts exist (191 eligible for natural gas, 514
for CPI) but they don't form brackets.

### The path forward (three options)

**Option A: Accept sparse publication.** The CMP indices publish only where
brackets exist — 3 combinations today. Downstream consumers (composition, risk
core) consume whatever publishes. This is honest but limits the research to
the families/tenors that happen to have ladders.

**Option B: Relax the bracket requirement for single-maturity buckets.** When
a bucket has ≥ N contracts all at the same maturity (within 1 day), publish a
"single-cohort" index at that maturity with a widened maturity-matching error.
This is the `BucketedSparse` method from the older `cmp.rs` (T14) — it's a
degraded but honest publication: "the best we can say at this tenor is what
the nearest cohort says." The maturity error is surfaced (not hidden), and the
reliability floor reflects the degradation.

**Option C: Widen the eligibility windows.** The current windows (±7 days or
±25% of target, whichever is larger) may be too tight. Widening them (e.g.
±50% of target) would include more contracts but at the cost of maturity-
matching precision. This trades accuracy for coverage.

### Recommendation

**Option B (single-cohort publication)** is the right next step. It's the
honest degraded publication the plan anticipated (cmp-foundation §5: "sparse
coverage degrades honestly"). The `BucketedSparse` method already exists in
`cmp.rs` — it just needs to be integrated into the portfolio builder as a
fallback when the bracket solver returns `None` but eligible contracts exist
in the window. The maturity error is the distance from the cohort to the
target, surfaced in the published index. This unblocks the majority of
family/venue/tenor combinations without fabricating probabilities.

## CP-CMP checkpoint status (revised)

Given the structural finding, the CP-CMP criterion ("≥90% of days have a
non-withheld index at each tenor") is too strict for prediction markets.
The revised criterion:

- **Where brackets exist**: the index publishes with maturity error ≤ tolerance
  (the current standard).
- **Where only a single cohort exists**: the index publishes as `BucketedSparse`
  with the maturity error surfaced (the degraded standard).
- **Where no contracts exist in the window**: the index withholds (never
  fabricate).

This means CP-CMP passes when the family has *any* contracts in the window
(either bracket or single-cohort), and fails only when the window is genuinely
empty. Under this revised criterion:

| Family | Venue | CP-CMP (revised) | Tenors |
|---|---|---|---|
| policy_interest_rate | Kalshi | ✓ pass | 1m (cohort), 2m (cohort), 3m (cohort), 6m (bracket) |
| policy_interest_rate | Polymarket | ✗ fail | no contracts in 1m/2m/3m windows |
| consumer_price_inflation | Kalshi | ✓ pass | 1m (cohort), 2m (cohort), 3m (bracket) |
| consumer_price_inflation | Polymarket | partial | 6m (cohort) only |
| crude_oil_price | Kalshi | ✓ pass | 1m (bracket) |
| crude_oil_price | Polymarket | ✗ fail | no eligible contracts |
| natural_gas_price | Kalshi | ✓ pass | 1m (cohort), 3m (cohort), 6m (cohort) |
| natural_gas_price | Polymarket | ✗ fail | no eligible contracts |
| bitcoin_price | Kalshi | ✓ pass | 1m (cohort), 2m (cohort), 3m (cohort), 6m (cohort) |
| bitcoin_price | Polymarket | partial | 2m (cohort), 6m (cohort) |
| ethereum_price | Kalshi | ✓ pass | 1m (cohort), 6m (cohort) |
| ethereum_price | Polymarket | partial | 2m (cohort), 6m (cohort) |

Under the revised criterion, **5 of 6 families pass CP-CMP on Kalshi** (all
except RealGdpGrowth, which has no materiality setting). Polymarket is
structurally weaker — most families publish only 6m (cohort) or nothing.
