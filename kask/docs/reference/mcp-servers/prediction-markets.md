---
title: "Prediction Markets MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-06
version: "0.33.5"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle]
---

# Prediction Markets MCP Server Reference

**Crate:** `mcp-servers/hkask-mcp-prediction-markets`
**Tools:** 18 — `market_lookup`, `market_match`, `market_ontology_map`, `market_calibration`, `market_record_resolution`, `market_subscribe_resolutions`, `market_ladder`, `market_cmp`, `market_cmp_index`, `market_cmp_index_store`, `market_cmp_portfolio_store`, `market_cmp_context_suggest`, `market_volatility`, `market_residual`, `market_check_resolutions`, `market_history`, plus `prediction_markets_status`
**Auto-start:** No (requires explicit opt-in via KaskSettings toggle (D9a))

> **Tool count note:** the server registers **16 market tools** plus a
> `prediction_markets_status` state tool (17 `#[tool]` methods total in
> `src/hkask_mcp_prediction_markets.rs`). This reference catalogues the 16
> market tools as the operational surface; the status tool is listed under
> Independent.

The prediction-markets server turns Polymarket and Kalshi market prices into
calibrated, ontology-annotated base rates for the forecasting stack. Its
governing invariant: **never return a bare probability** — every `MarketRecord`
pairs its probability with spread, volume, calibration, volatility, a
reliability tier, and a dual-axis (PKO + Dublin Core) ontology mapping. The
server is the outside-view sense arm for the scenarios server:
`scenario_from_markets` / `scenario_from_markets_set` in
`hkask-mcp-scenarios` consume its records directly.[^tetlock-pm-ref]

## Source modules

| Module | Role |
|--------|------|
| `provider_polymarket.rs` | Polymarket CLOB + Gamma API provider |
| `provider_kalshi.rs` | Kalshi REST + candlestick provider |
| `calibration.rs` | Per-bucket Brier scoring and reliability tiers (via `hkask-forecast`) |
| `cmp.rs` | Constant Maturity Prediction curve construction (log-odds interpolation) |
| `cmp_portfolio.rs` | Solved-portfolio CMP index set (maturity-bucketed, orientation-tagged) |
| `volatility.rs` | DR-AS structural volatility model (arXiv:2607.08199) |
| `base_event.rs` | Base-event registry + curated economic context + strike extraction |
| `residual.rs` | Niche-market decomposition into base-event beta + idiosyncratic residual |
| `streaming.rs` | Polymarket websocket subscription for resolution events |
| `matcher.rs` | Question → market candidate matching (token overlap + deadline alignment) |
| `ontology.rs` | Dual-axis (PKO process + Dublin Core state) mapping document |

## The calibration feedback loop

The server closes a self-feeding calibration loop:

1. **Sense:** `market_check_resolutions` scans both platforms for newly
   resolved markets and records definitive outcomes into the calibration store
   (idempotent; only terminal prices ≥0.99/≤0.01 or explicit Kalshi results
   count — ambiguous 50-50 resolutions are skipped, never fabricated).
2. **Record:** `market_record_resolution` is the manual sense arm — it writes a
   (bucket, probability-at-observation, outcome) observation.
3. **Evaluate:** `market_calibration` computes per-bucket Brier scores from the
   accrued observations. A bucket with no resolved data returns `stale: true` —
   never a synthetic Brier of 0.
4. **Act:** poorly calibrated buckets are demoted to lower reliability tiers on
   subsequent `market_lookup` / `market_match` calls, which downstream
   consumers (`scenario_from_markets`) read as a gate on base-rate anchoring.

`market_subscribe_resolutions` streams Polymarket resolution events as
notifications only — the wire carries no pre-resolution probability, and
fabricating one would corrupt the Brier loop. Pair a notification with
`market_record_resolution` (which takes the pre-resolution probability) to
feed the loop.

## Tool reference

### Discovery and matching

| Tool | Description | Key params |
|------|-------------|------------|
| `market_lookup` | Look up prediction markets across Polymarket and Kalshi by free-text query. Returns annotated `MarketRecord`s — every probability paired with spread/volume/calibration/volatility/reliability tier and ontology mapping. Never a bare probability. | `query`, `category`, `limit` |
| `market_match` | Resolve a scenario or forecasting question to candidate markets about the same underlying event; confidence-tiered candidates with deterministic match basis (token overlap + deadline alignment). Refuses low-confidence matches rather than anchoring on a wrong-event market. | `question`, `limit` |
| `market_ontology_map` | Return the dual-axis (PKO process + Dublin Core state) ontology mapping document annotating every `MarketRecord`, including market lifecycle stages and field-level mappings. Fetch before interpreting market records. | — |

### Calibration (sense and evaluate)

| Tool | Description | Key params |
|------|-------------|------------|
| `market_calibration` | Calibration reading (Brier score, sample size, staleness) for a domain or series bucket, computed from resolved observations via `hkask-forecast`. No-data buckets return `stale: true`. | `bucket` |
| `market_record_resolution` | Record a resolved market outcome (bucket, probability-at-observation, outcome) into the calibration store — the manual sense arm of the feedback loop. | `bucket`, `probability`, `outcome` |
| `market_check_resolutions` | Scan both platforms for newly resolved markets and record definitive outcomes (idempotent). Terminal prices or explicit Kalshi results only; ambiguous resolutions skipped. | `series`, `limit` |
| `market_subscribe_resolutions` | Subscribe to Polymarket's public market channel for resolution events on given CLOB asset IDs; events arrive as notifications and do NOT write calibration observations. | `asset_ids`, `bucket`, `max_resolutions` |

### Term structure and decomposition

| Tool | Description | Key params |
|------|-------------|------------|
| `market_ladder` | Ladder of contracts in a series ordered by deadline, each annotated with `time_to_maturity` in fractional years. Kalshi series ticker or Polymarket event slug; unparseable deadlines sort last with null maturity — never fabricated. | `series` |
| `market_cmp` | Constant Maturity Prediction: synthesize a fixed-tenor probability for a registered base event by interpolating its family's markets in log-odds space. Sparse coverage returns `bucketed_sparse` with the bracket width. Base events come only from `HKASK_PREDICTION_MARKETS_BASE_EVENTS` — unregistered series refused. | `series`, `tenor_days` |
| `market_cmp_index` | Full CMP index for a registered base event: probability curve across the standard tenor grid (7d/30d/90d/180d/1y/2y), log-odds interpolated, with curve slope (log-odds/year) as the term-structure signal. Uncovered tenors return null. | `series` |
| `market_residual` | Decompose a niche market's movement into base-event exposure (log-odds beta) plus idiosyncratic residual. Refuses with `insufficient_overlap` below 10 shared observations; output carries `r_squared` and `observations`. | `market_ticker`, `base_ticker`, `window_days` |

### History

| Tool | Description | Key params |
|------|-------------|------------|
| `market_history` | Fetch a market's price history with `realized_variance` populated (log-odds step variance) plus the volatility regime (smooth vs jump-like). Kalshi: candlesticks; Polymarket: CLOB prices-history. | `market`, `source`, `window_days` |

### Independent

| Tool | Description | Key params |
|------|-------------|------------|
| `prediction_markets_status` | Current server state: cache TTL, ontology mapping version, tools called this session. | — |

## Configuration

Settings live in the `kask.prediction_markets` subsection
(`kask/crates/kask_bridge/src/settings.rs:451-458`,
`KaskPredictionMarketsSettings`):

| Setting | Description |
|---------|-------------|
| `data_dir` | Data directory for the calibration journal. When empty, in-memory. |
| `cache_ttl_secs` | Cache TTL in seconds for market-data responses (0 = server default). |
| `base_events` | Base-event registry: `"domain:series,..."` pairs for CMP construction. |

At runtime the base-event registry is read from
`HKASK_PREDICTION_MARKETS_BASE_EVENTS` (see `market_cmp`).

## Consumers

- **`hkask-mcp-scenarios`** — `scenario_from_markets` and
  `scenario_from_markets_set` convert `market_lookup` / `market_match` records
  into scenario events and event trees (see
  [Scenarios MCP Server Reference](scenarios.md)).
- **`hkask-mcp-companies`** — `equity_duration` pairs a company's cash-flow
  maturity profile with prediction-market `time_to_maturity` for
  duration-matching across horizons.

## Project record

The full design → build → verify record lives in
`docs/reports/prediction-markets/` (00 spike through 06 verification). The
stopping-point status — what's shipped, what's deferred, and the re-entry
triggers — is `07-project-status.md`.

## Cross-links

- [Scenarios MCP Server Reference](scenarios.md) — event-tree forecasting that consumes market base rates
- [Companies MCP Server Reference](companies.md) — equity duration pairing
- [Superforecasting: Layered Model](../../explanation/forecasting-and-scenarios.md) — three-layer architecture
- [MCP Server Registry](README.md) — built-in server index

## Footnotes

[^tetlock-pm-ref]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited for the outside-view / base-rate anchoring discipline the server's never-bare-probability invariant enforces.
