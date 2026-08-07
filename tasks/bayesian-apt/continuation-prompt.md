---
dcterms:title: "Continuation Prompt — CMP Index Construction (C0.4 + CP-CMP)"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-07"
rdf:type: bibo:Document
pko:procedure-target: "Drive the next session: construct 1m/3m/6m CMP indices from the pulled catalogs and clear the CP-CMP checkpoint"
---

# Continuation Prompt — CMP Index Construction

## Mission

You are continuing the **Bayesian-APT / CMP-first** research program
(`tasks/bayesian-apt/plan.md`). Phase 0 (the CMP foundation layer) is partially
landed: C0.1 (base-event registry), C0.2 (semantic eligibility), and C0.3
(weight solver + roll rules) are done in `kask/crates/hkask-forecast/src/`.
The event catalogs have been pulled. The next deliverable is **C0.4 —
construct 1m/3m/6m CMP indices per (family, orientation), per-venue** and to
clear the **CP-CMP checkpoint** (rates family produces a continuous
1m/3m/6m series on both Kalshi and Polymarket, ≥90% of days within maturity
tolerance, eligibility classifications human-reviewed).

## Context to read first (in order)

1. `tasks/bayesian-apt/plan.md` — the v2 CMP-first plan. Phase 0 §C0.4 and
   Checkpoint CP-CMP are your acceptance criteria.
2. `tasks/bayesian-apt/cmp-foundation.md` — the foundation spec. §5 (three-step
   index process), §6 (what each index publishes daily), §7 (the complete
   passed-variables list — nothing else is a number), §8 (relationship to
   platform assets). Re-read §7 before writing any threshold.
3. `tasks/bayesian-apt/todo.md` — open items: C0.4, CP-CMP, ONT-6.
4. `tasks/bayesian-apt/hypothesis-dossier.md` — H2/H3 depend on CMP-controlled
   inputs; your indices are what makes those tests admissible. Do not run
   H-tests on raw contracts.
5. `kask/crates/hkask-forecast/src/base_event.rs` and `cmp_portfolio.rs` —
   the C0.1–C0.3 machinery you are building on top of. Read before extending.
6. `kask/crates/hkask-forecast/src/hkask_forecast.rs` — the pure-math home;
   C0.4 indices belong here (no MCP deps).

## Data substrate (already pulled — do not re-pull)

- `tasks/bayesian-apt/catalogs/gamma_events.jsonl` — 2,100 Polymarket event
  records (full catalog, JSONL). **Contains many non-base events** (politics,
  elections, sports, pop culture). You MUST filter to the base families via
  `classify_base_event` / `evaluate_record`. House-election markets (e.g.
  `co-02-house-election-winner`, `fl-16-house-election-winner`,
  `nj-03-house-election-winner`) are **not base events** — they are downstream
  H1/H3 material, not CMP index constituents. Do not index them.
- `tasks/bayesian-apt/catalogs/kalshi_events.jsonl` — 9,542 Kalshi event
  records. Same filtering discipline.
- `tasks/bayesian-apt/catalogs/contracts/<family>/{gamma,kalshi}.jsonl` —
  per-family contract splits for the seven base families:
  `bitcoin_price`, `consumer_price_inflation`, `crude_oil_price`,
  `ethereum_price`, `natural_gas_price`, `policy_interest_rate`,
  `real_gdp_growth`. These are the CMP index constituents. The seventh
  family (`real_gdp_growth`) is beyond the plan's initial six — confirm
  whether it has continuously-available contracts on both venues before
  building indices for it; if not, withhold (never fabricate).

The Polymarket event schema (from the pulled records): each line is an event
with `id`, `ticker`, `slug`, `title`, `description`, `endDate`, `markets[]`
(each with `question`, `outcomes`, `outcomePrices`, `endDate`, `liquidity`,
`volume`, `bestBid`, `bestAsk`, `spread`, `lastTradePrice`, `competitive`,
`clobTokenIds`, etc.), and `eventMetadata.context_description`. The
`outcomePrices` field is a JSON-encoded string of `[yes_price, no_price]`.
Map these onto `MarketRecord` (or a thin adapter) before feeding
`evaluate_record`.

## Code substrate (landed, build on top)

- `base_event.rs`: `BaseEventFamily` (six families), `classify_base_event`
  over question/description/series/category, per-family materiality settings.
- `cmp_portfolio.rs`: `evaluate_record` (classify → materiality → orientation
  → maturity window → reliability floor, with rejection reasons),
  `CmpConfig` (all thresholds — no magic numbers), bracket-pair weight
  solver, withhold-when-no-bracket.
- `hkask_forecast.rs`: pure-math home. C0.4 indices go here.

## Deliverable

### C0.4 — CMP indices

For each of the six (or seven, if `real_gdp_growth` qualifies) base families,
for each orientation ∈ {increase, decline, stable}, for each target maturity
∈ {1m, 3m, 6m}, for each venue ∈ {Kalshi, Polymarket}:

1. **Filter** the venue's event catalog to the family via `classify_base_event`.
2. **Classify** each eligible contract to (family, orientation, magnitude)
   via `evaluate_record`. Record rejections with reasons.
3. **Build the index** for each (family, orientation, target, venue) by
   solving the two-constraint weight problem (maturity match ± tolerance AND
   magnitude match within tolerance) over the eligible bracket. Withhold
   when no bracket spans the target — never fabricate.
4. **Publish** (per cmp-foundation §6): index probability, constituent
   contracts/weights/maturities, maturity-matching error, reliability floor,
   and for Stable: direct-vs-balanced flag + net-orientation residual.
5. **Roll rule**: smooth hand-off (passed variable, default 3 days) as the
   front contract's maturity decays below the eligibility window. No
   cliff-edge probability jumps.

Output: a `CmpIndex` type in `hkask-forecast` + a builder function that
takes (catalog records, `CmpConfig`) and returns `Result<Vec<CmpIndex>,
CmpError>` (withhold is a `CmpError::NoBracket`, not a panic). Provenance
records the (family, orientation, target maturity, venue), not a decaying
contract.

### CP-CMP checkpoint

The **rates family** (`policy_interest_rate`) must produce a continuous
1m/3m/6m CMP series on **both** venues for a trailing window (you choose the
window length as a passed variable; document it). Acceptance:

- ≥ 90% of days in the window have a non-withheld index at each tenor on
  each venue.
- Maturity-matching error within tolerance on ≥ 90% of published days.
- You human-review the eligibility classifications on a sample (≥ 20
  contracts) and record any misclassifications as issues.

If the rates family fails CP-CMP on either venue, **stop and diagnose**
before building indices for the other families. A failed checkpoint means
either the catalog pull missed contracts, the eligibility classifier is
wrong, or the venue genuinely lacks a continuous ladder — each has a
different fix. Do not paper over it.

### ONT-6 (conditional, parallel)

`todo.md` flags ONT-6 ("rewire `economic_object.rs` and `base_event.rs` onto
FIBO-anchored classification through the corpus pipeline; delete the
substring synonym-closure loop") as "the next step." It is a refinement of
C0.2's FIBO anchoring. **Do ONT-6 only if `evaluate_record`'s signature
matching misclassifies real contracts during C0.4** — i.e., if the
substring classifier is the bottleneck on CP-CMP. If signature matching
works on the real catalog, defer ONT-6 and note it. Do not refactor
speculatively.

## Discipline (non-negotiable)

- **Never fabricate.** Withhold when no bracket spans the target. A withheld
  index is an honest `NoBracket` result; a fabricated index is the disease
  CMP exists to cure. (cmp-foundation §5, plan.md "never-fabricate posture".)
- **All thresholds are passed variables in `CmpConfig`.** No magic numbers
  in the index builder. (cmp-foundation §7 — the complete list.)
- **Per-venue indices.** Do not pool Kalshi and Polymarket. The
  law-of-one-price failure (arXiv:2601.01706) is the reason per-venue
  indices exist. (plan.md C0.4 AC.)
- **Provenance.** Every published index probability carries its constituent
  contracts, weights, maturities, and reliability floor. No bare
  probability. (cmp-foundation §6; mirrors `MarketRecord`'s never-bare rule.)
- **Equity-pricing discipline.** Equities are priced on fundamental forecast
  models (DCF/RIM, MAIA). The arbitrage-pricing apparatus applies to the
  contracts. Do not build equity-return regressions, betas, or factor
  loadings in this phase. (plan.md "Equity pricing discipline".)
- **No upstream edits.** Everything you write lives in `kask/`. Do not
  touch upstream Zed files. (`.rules` DIVERGENCE.md seam discipline.)
- **Errors propagate, never `unwrap_or(0)`.** Catalog parsing, JSON
  decoding, date parsing — propagate errors with `?` or log them. A
  silently-zeroed field is a broken feedback loop. (`.rules`.)

## Validation

- `cargo test -p hkask-forecast` — all existing tests pass plus new tests
  for `CmpIndex` construction (hand-checkable: a 2-contract bracket with
  known weights; a withheld case; a roll hand-off).
- `./script/clippy` — clean.
- Property tests (proptest): weights in [0,1], Σw = 1, maturity error ≤
  tolerance when a bracket exists, withhold when no bracket.
- CP-CMP acceptance criteria above.
- Update `tasks/bayesian-apt/todo.md`: check C0.4 and CP-CMP when done,
  record the trailing window chosen and the rates-family pass rate.

## Open questions to resolve during the work (cmp-foundation §6)

Record your resolution for each in a short note appended to
`cmp-foundation.md` §6 or a new `c0.4-decisions.md`:

1. **Magnitude bands**: fixed per family vs continuous with tolerance?
2. **Orientation**: independent increase/decrease index pairs vs one signed
   index? (The plan defaults to independent pairs; confirm on real data.)
3. **Cross-venue**: per-venue indices (recommended, mandated above) vs
   pooled with adjustment — confirm per-venue is right by measuring the
   cross-venue divergence on the rates family.
4. **Sparse ladders**: withhold (recommended) vs publish degraded with
   wide error — confirm withhold is right by checking how often it fires
   on the rates family.

## Out of scope for this session

- R1–R6 (re-pointing machinery at CMP, risk core, coherence test,
  falsification suite). These are blocked on CP-CMP. Do not start them.
- H1–H5 empirical tests. Blocked on CMP-controlled inputs.
- T8b platform surface. Blocked on H3 corroboration.
- Political / election / sports markets in the catalogs. They are
  downstream H1/H3 material, not CMP constituents.

## When you finish

- Summarize: which families passed CP-CMP on which venues, the rates-family
  pass rate, any misclassifications found, and the resolution of the four
  open questions.
- Flag any family that fails to produce a continuous series and diagnose
  why (catalog gap, classifier error, or genuine venue absence).
- Do not commit or open a PR unless asked.
