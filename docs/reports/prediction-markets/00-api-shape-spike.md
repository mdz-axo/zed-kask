# T0 Spike: Live API Shape Findings

**Date:** 2026-08-05 (live requests, all succeeded unless noted)
**Purpose:** Falsification gate for R1/R2 before implementation; CMP feasibility sample for T14.

---

## 1. Polymarket Gamma — VERIFIED (R1 resolved)

`GET https://gamma-api.polymarket.com/events?limit=3&active=true&closed=false` → 200, JSON array.

**Event fields (live):** `id`, `ticker`, `slug`, `title`, `description`, `resolutionSource`, `startDate`, `endDate`, `active`, `closed`, `archived`, `liquidity`, `volume`, `openInterest`, `volume24hr`, `volume1wk`, `volume1mo`, `volume1yr`, `enableOrderBook`, `liquidityClob`, `negRisk`, `competitive`, `markets[]` (embedded).

**Embedded market fields (live):** `id`, `question`, `conditionId`, `slug`, `endDate`, `outcomes` (JSON-string array, e.g. `"[\"Yes\", \"No\"]"`), **`outcomePrices` (JSON-string array of decimal strings, e.g. `"[\"0\", \"1\"]"`)**, `volume`, `active`, `closed`, `closedTime`, `resolvedBy`, `questionID`, `umaEndDate`, **`umaResolutionStatus: "resolved"`** (confirms the previously-unverified resolution field), `umaBond`, `umaReward`, `clobTokenIds` (JSON-string array of ERC1155 token IDs), `orderPriceMinTickSize`, `orderMinSize`, `negRisk`, `volumeNum`, plus `volume{1wk,1mo,1yr}Clob`.

**Contract mapping notes:**
- `probability` ← `outcomePrices[0]` (Yes leg) parsed from the embedded JSON string. Prices are decimal strings in [0,1].
- `deadline` ← market `endDate`; `status` ← `active`/`closed` + `umaResolutionStatus`; `resolution_source` ← `resolvedBy` address presence + `umaResolutionStatus`.
- `volatility`/`price_history` requires CLOB `/prices-history` (per token ID from `clobTokenIds[0]`) — not in Gamma.
- **Quirk:** several fields are JSON-encoded strings inside JSON (`outcomes`, `outcomePrices`, `clobTokenIds`) — parser must double-decode. Pinned in T2 fixtures.

## 2. Kalshi REST — VERIFIED (R1 resolved; R2 partially)

`GET https://external-api.kalshi.com/trade-api/v2/events?limit=3&status=open` → 200. No auth required for reads. Cursor pagination (`cursor` field).

**Event fields (live):** `event_ticker`, `series_ticker`, `title`, `sub_title`, `category` (e.g. "World", "Elections", "Economics"), `mutually_exclusive`, `settlement_sources[]` (name+url — rich provenance for `dcterms:provenance`), `available_on_brokers`, `last_updated_ts`.

**Market fields (live, via `GET /markets?series_ticker=KXFED`):** `ticker`, `event_ticker`, `title`, `subtitle`, `yes_bid_dollars`, `yes_ask_dollars`, `no_bid_dollars`, `no_ask_dollars`, `yes_bid_size_fp`, `yes_ask_size_fp`, `last_price_dollars`, `previous_price_dollars`, `volume_fp`, `volume_24h_fp`, `open_interest_fp`, `liquidity_dollars`, `status: "active"`, `close_time`, `expiration_time`, `expected_expiration_time`, `floor_strike`, `strike_type` ("greater"), `price_ranges`, `market_type: "binary"`, `rules_primary`, `rules_secondary`, `result` (empty pre-settlement), `settlement_timer_seconds`, `can_close_early`.

**Contract mapping notes:**
- `probability` ← midpoint of `yes_bid_dollars`/`yes_ask_dollars` (both present live — the "bids-only orderbook" concern R13 applies to the `/orderbook` endpoint; the market object itself carries both sides). `spread` = `yes_ask_dollars − yes_bid_dollars` directly.
- **R12 resolved:** this API generation returns `_dollars` fixed-point strings throughout (no legacy cents fields observed in these responses). Parser should still tolerate both, but dollars are what production serves.
- `open_interest` ← `open_interest_fp` (string, fixed-point). `volume` ← `volume_fp`. **All numeric fields are strings** — parse via a decimal/f64-from-string helper, never bare `f64` serde.

**R2 downgraded (was "shape unverified", now "endpoint not live at documented path"):**
`GET /events/{ticker}/forecast-percentile-history` (and `forecast_percentile_history`, with/without percentiles param, on 3 different tickers) → **404 on all attempts**. The endpoint is documented in Kalshi's llms.txt index but does not respond on the public v2 path as of 2026-08-05. **Decision:** drop `kalshi_percentile_history` as a `probability_method`; use candlesticks as the history source. Re-check endpoint availability at T3 implementation time.
**Fallback VERIFIED:** `GET /markets/candlesticks?market_tickers={T}&start_ts&end_ts&period_interval=1440` → 200 with `yes_bid`/`yes_ask` OHLC (`open/high/low/close_dollars`), `volume_fp`, `open_interest_fp` per period. Requires `market_tickers` (plural, comma-joined) — singular form returns a 400 with a clear error message.

## 3. CMP feasibility sample — FEASIBLE for macro base events (T14 green-lit)

`GET /markets?limit=20&status=open&series_ticker=KXFEDDECISION` → **4 distinct deadline cohorts observed** (2027-09-15, 2027-10-27, 2027-12-08, 2028-01-26), each with a 5-strike ladder (H26/H25/H0/C25/C26 hike/hold/cut contracts), live two-sided quotes, and open interest. Time-to-resolution spread ≈ 4.5 months between nearest and farthest cohort. This is sufficient per-tenor coverage for log-odds interpolation across the 30d–180d tenors for the Fed base event.

Polymarket top events by volume: `presidential-election-winner-2028` (128 embedded markets), Dem/GOP nominee 2028 (128 each) — multi-market events with staggered sub-deadlines, suitable as politics base events, though their sub-markets are candidate legs (mutually exclusive sets) rather than tenor ladders; CMP for politics events applies at the event level, not per-candidate.

**Conclusion:** T14 proceeds with `method: "interpolated"` for macro (Kalshi series-based) base events; politics base events use event-level tenor bucketing (`bucketed_sparse` fallback where deadline cohorts < 3).

## 4. Implications for task specs

- **T2 fixtures:** double-decoded JSON-string fields (`outcomes`, `outcomePrices`, `clobTokenIds`); `umaResolutionStatus` enum (observed: `"resolved"`).
- **T3 fixtures:** all-numerics-as-strings; `_dollars`/`_fp` suffix conventions; candlesticks as history source (not percentile-history); `settlement_sources[]` → `dcterms:provenance`.
- **T4 contract:** `probability_method` enum loses `"kalshi_percentile_history"`, gains `"kalshi_candlestick_history"`. Kalshi `probability` = yes-bid/ask midpoint (both sides available on the market object).
- **T14:** interpolated method confirmed viable for macro; politics uses event-level bucketing.
- **R13 narrowed:** bids-only applies to `/orderbook` endpoint only; market objects carry both sides. Spread computation from market objects is direct.

## 5. What remains unverified (small residual)

- Polymarket CLOB `/prices-history` response shape (not yet called live; T2 will pin with its first integration test).
- Kalshi WS handshake/auth flow (T11b scope).
- `umaResolutionStatus` full enum (only `"resolved"` observed live).
