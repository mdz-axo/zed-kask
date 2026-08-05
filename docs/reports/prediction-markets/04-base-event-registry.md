# Base-Event Registry for Constant Maturity Predictions

**Date:** 2026-08-05
**Method:** hypothesis-framer (what makes a base event?) + live API sampling (Kalshi series counts, Polymarket event volumes, 2026-08-05).
**Companion:** `02-zed-kask-integration.md` §5.6 (CMP design), `00-api-shape-spike.md` §3 (feasibility).

---

## 1. What makes a base event (the treasury analogy, made operational)

A base event is to scenario forecasting what the constant-maturity treasury is to fixed income: a **widely-traded, continuously-priced benchmark** whose term structure other events are priced against. Selection criteria (FINER-adapted):

| Criterion | Test | Why |
|---|---|---|
| **Systematic** | The event's outcome moves many downstream events | It must be a shared risk factor, not idiosyncratic |
| **Tenor density** | ≥3 distinct deadline cohorts live simultaneously | CMP interpolation needs bracketing points (T14) |
| **Liquidity** | Reliable two-sided quotes + meaningful volume/OI | The reliability gate (reliability_tier) must clear |
| **Clean resolution** | Unambiguous, machine-readable resolution source | Feeds the calibration loop without dispute noise |
| **Persistence** | The series recurs (new cohorts replace expiring ones) | A one-off event can't sustain a curve |

## 2. Live sampling results (2026-08-05)

**Kalshi** (open-market counts, limit 50): `KXCPI` 50, `KXFED` 50, `KXPAYROLLS` 50, `KXBTCD` 50, `KXWTI` 50 (27 for `KXWTIW`), `KXNASDAQ100` 30, `KXGDP` 36, `KXJOBLESSCLAIMS` 9. Not found/absent: `KXRECESSION`, `KXSP500`, `KXUST10Y*`, `KXGOLD`, `KXETHW`.

**Polymarket** (top economy-tag events by volume): `how-many-fed-rate-cuts-in-2026` $46.9M, `fed-decision-in-september` $14.9M, `fed-rate-hike-in-2026` $6.3M, `us-recession-by-end-of-2026` $1.7M, `what-will-fed-rate-hit-before-2027` $1.7M, `how-high-will-inflation-get-in-2026` $1.4M.

**Key structural finding:** Kalshi's macro series are **strike ladders per meeting/release** (e.g. KXFEDDECISION has rate-bucket strikes per FOMC date — 4+ deadline cohorts verified in T0). Polymarket's macro coverage is **event-level tenor ladders** (rate cuts "in 2026" vs "by September" vs "before 2027"). The two shapes are complementary: Kalshi gives tenor density *within* a question family; Polymarket gives tenor density *across* deadline variants of the same risk.

## 3. Recommended registry (v1)

```
economics:KXFEDDECISION      # Fed policy — the anchor "risk-free" frame (verified 12 cohorts, live CMP)
economics:KXCPI              # Inflation releases — strike ladders per print
economics:KXPAYROLLS         # Labor market — strike ladders per jobs report
economics:KXGDP              # Growth — 36 live markets
crypto:KXBTCD                # BTC daily — deep strike ladders (50 live)
crypto:KXETH                 # ETH (verified live 2026-08-05: 50 markets, dated strike ladders)
energy:KXWTI                 # Crude oil — 50 live markets
equities:KXNASDAQ100         # Index level — 30 live markets
```

**Polymarket complements (event-level tenor ladders):**

```
economics:how-many-fed-rate-cuts-in-2026   # rate-path count ($46.9M — deepest macro event live)
economics:us-recession-by-end-of-2026      # recession risk
economics:how-high-will-inflation-get-in-2026  # inflation tail
politics:presidential-election-winner-2028 # politics base event (128 legs, $677M)
```

## 4. Deliberate exclusions (essentialist)

- **Sports/entertainment** — well-calibrated (2604.20421 §6.1) but idiosyncratic; no systematic risk to decompose into. A Lakers game is not a base rate for anything but itself.
- **`KXJOBLESSCLAIMS`** (9 markets) — below tenor-density threshold today; revisit if coverage thickens.
- **Single-deadline one-offs** (e.g. "Kraken IPO 2025") — cannot sustain a curve by construction.
- **Any auto-promotion** — registry stays config-only (T14's pinned guardrail): a manipulated thin market must never become the frame.

## 5. Caveats (labeled)

- **IS:** counts/volumes above are live samples as of 2026-08-05.
- ~~KXETHD ticker~~ — resolved: `KXETH` is the live series (50 markets, verified 2026-08-05).
- **Hypothesis:** Polymarket event-slug stability — slugs may rotate as new cycle variants are created ("in-2026" events expire and are replaced). The registry should expect periodic refresh; consider series-level Polymarket endpoints if they exist for these families (open question).
- **OUGHT:** start with the Kalshi macro four (FED/CPI/PAYROLLS/GDP) — they satisfy all five criteria today; add Polymarket complements once the event-level tenor bucketing path (T14's `bucketed_sparse`) has been exercised against live data.

## 6. Config snippet

```jsonc
// settings.json → kask.prediction_markets
{
  "kask": {
    "prediction_markets": {
      "base_events": "economics:KXFEDDECISION,economics:KXCPI,economics:KXPAYROLLS,economics:KXGDP,crypto:KXBTCD,crypto:KXETH,energy:KXWTI,equities:KXNASDAQ100"
    }
  }
}
```
