---
title: "Prediction Markets MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-28
version: "0.39.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle]
---

# Prediction Markets MCP Server Reference

**Crate:** `mcp-servers/hkask-mcp-prediction-markets`
**Tools:** 33 — 18 market tools (`market_lookup`, `market_match`, `market_ontology_map`, `market_calibration`, `market_record_resolution`, `market_subscribe_resolutions`, `market_ladder`, `market_cmp`, `market_cmp_index`, `market_cmp_indices`, `market_cmp_index_store`, `market_cmp_portfolio_store`, `market_cmp_context_suggest`, `market_volatility`, `market_residual`, `market_check_resolutions`, `market_history`, `prediction_markets_status`) plus 15 economic-data tools in `src/economic_data_tools.rs` (`fred_search_series`, `fred_get_observations`, `fred_get_series_info`, `fred_list_categories`, `fred_get_release`, `wb_search_indicators`, `wb_get_observations`, `wb_list_countries`, `wb_list_topics`, `wb_get_indicator_info`, `dbnomics_search`, `dbnomics_list_providers`, `dbnomics_get_dataset`, `dbnomics_get_series`, `market_score_rationale`)
**Auto-start:** No (requires explicit opt-in via KaskSettings toggle (D9a))

> **Tool count note:** the server registers **32 `#[tool]` methods** — 17 in
> `src/hkask_mcp_prediction_markets.rs` + 15 in `src/economic_data_tools.rs`, both
> merged into `combined_router()` at `src/hkask_mcp_prediction_markets.rs:85-89`
> (verified 2026-08-28 by `#[tool`-attribute grep excluding `#[cfg(test)]` regions;
> the method reproduces the pinned counts on media and scenarios exactly). The
> operational surface is the set of market tools; the status tool is listed under
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

The server closes a self-feeding calibration loop. The scan is two-phase
so the scored probability is honest:

1. **Sense (snapshots):** `market_check_resolutions` first scans OPEN
   markets and snapshots each one's current price as the pre-resolution
   probability-at-observation (the EARLIEST snapshot per market is kept —
   a later price is resolution-informed). Snapshots persist in a
   pending journal alongside the observation journal.
2. **Sense (resolutions):** newly resolved markets then consume their
   snapshot — the Brier loop scores the price the scanner first saw,
   never the post-resolution price (scoring the terminal price would be
   self-fulfilling: the outcome is derived from that same price, so
   Brier ≈ 0 by construction and the demotion gate could never fire). A
   market that resolves before its first scan is counted in
   `resolved_without_snapshot` and skipped — never fabricated. Ambiguous
   50-50 resolutions are skipped the same way. The scan is idempotent.
3. **Record:** `market_record_resolution` is the manual sense arm — it writes a
   (bucket, probability-at-observation, outcome) observation.
4. **Evaluate:** `market_calibration` computes per-bucket Brier scores from the
   accrued observations. A bucket with no resolved data returns `stale: true` —
   never a synthetic Brier of 0.
5. **Act:** poorly calibrated buckets are demoted to lower reliability tiers on
   subsequent `market_lookup` / `market_match` calls, which downstream
   consumers (`scenario_from_markets`) read as a gate on base-rate anchoring.

Consequence: scans must run often enough that open markets are snapshotted
before they resolve — a high `resolved_without_snapshot` rate means the
scan cadence is too slow (see the `calibration-stewardship` skill).

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
| `market_check_resolutions` | Two-phase scan: (1) snapshot open markets' current prices as pre-resolution probability-at-observation (earliest per market kept), (2) consume snapshots for newly resolved markets. Resolutions without a snapshot are counted (`resolved_without_snapshot`) and skipped — the post-resolution price is never scored. Idempotent. | `series`, `limit` |
| `market_subscribe_resolutions` | Subscribe to Polymarket's public market channel for resolution events on given CLOB asset IDs; events arrive as notifications and do NOT write calibration observations. | `asset_ids`, `bucket`, `max_resolutions` |

### Term structure and decomposition

| Tool | Description | Key params |
|------|-------------|------------|
| `market_ladder` | Ladder of contracts in a series ordered by deadline, each annotated with `time_to_maturity` in fractional years. Kalshi series ticker or Polymarket event slug; unparsable deadlines sort last with null maturity — never fabricated. | `series` |
| `market_cmp` | Constant Maturity Prediction: synthesize a fixed-tenor probability for a registered base event by interpolating its family's markets in log-odds space. Sparse coverage returns `bucketed_sparse` with the bracket width. Base events come only from `HKASK_PREDICTION_MARKETS_BASE_EVENTS` — unregistered series refused. | `series`, `tenor_days` |
| `market_cmp_index` | Full CMP index for a registered base event: probability curve across the standard tenor grid (7d/30d/90d/180d/1y/2y), log-odds interpolated, with curve slope (log-odds/year) as the term-structure signal. Uncovered tenors return null. | `series` |
| `market_cmp_indices` | Build provenance-carrying CMP indices (ProvenancedCmpIndex objects) from live open markets per (family, venue) — the producer for `scenario_from_cmp_indices` (hkask-mcp-scenarios). Withheld buckets and rejection reasons are surfaced; never fabricated. | `series`, `venue`, `limit`, `reference`, `volatility`, `predicted_level`, `direction_up` |
| `market_residual` | Decompose a niche market's movement into base-event exposure (log-odds beta) plus idiosyncratic residual. Refuses with `insufficient_overlap` below 10 shared observations; output carries `r_squared` and `observations`. | `market_ticker`, `base_ticker`, `window_days` |

### History

| Tool | Description | Key params |
|------|-------------|------------|
| `market_history` | Fetch a market's price history with `realized_variance` populated (log-odds step variance) plus the volatility regime (smooth vs jump-like). Kalshi: candlesticks; Polymarket: CLOB prices-history. | `market`, `source`, `window_days` |

### Economic data — FRED

Five tools wrapping the FRED (Federal Reserve Economic Data) API, defined in
`src/economic_data_tools.rs:34-169` and implemented in
`src/economic_data/fred.rs`. **All five require the `HKASK_FRED_API_KEY`
credential** — read from `ctx.credentials` at
`src/hkask_mcp_prediction_markets.rs:1606` and enforced by `require_api_key`
(`src/economic_data/fred.rs:77-80`), which returns `MissingApiKey` when the
key is absent or empty (a missing credential is an authorization failure,
not a silent fallback).

| Tool | Description | Key params |
|------|-------------|------------|
| `fred_search_series` | Search FRED economic data series by text. Returns series IDs with title, units, frequency, and popularity. (`economic_data_tools.rs:40-63`) | `search_text`, `category_id`, `tag_names`, `limit`, `order_by` |
| `fred_get_observations` | Fetch FRED time series observations by series ID. Returns date-value pairs (most recent first). Supports date range, frequency, and units transformations. (`economic_data_tools.rs:68-91`) | `series_id`, `observation_start`, `observation_end`, `frequency`, `units` |
| `fred_get_series_info` | Get FRED series metadata: title, units, frequency, seasonal adjustment, date range, notes. (`economic_data_tools.rs:94-117`) | `series_id` |
| `fred_list_categories` | Browse FRED category tree. Returns child categories for a given parent (default: root). Use to discover economic data by domain. (`economic_data_tools.rs:120-143`) | `category_id` |
| `fred_get_release` | Get FRED release metadata (name, description, last_updated, next_release) and its series list. Use to track data release schedules. (`economic_data_tools.rs:146-169`) | `release_id` |

### Economic data — World Bank

Five tools wrapping the World Bank Indicators API, defined in
`src/economic_data_tools.rs:171-282` and implemented in
`src/economic_data/worldbank.rs`. No API key required — the World Bank API
is keyless and covers ~29,500 indicators across 45+ databases for all
countries, the global complement to FRED's US-centric data.

| Tool | Description | Key params |
|------|-------------|------------|
| `wb_search_indicators` | Search World Bank indicators by text. Returns indicator IDs with name, unit, source, and topics. Covers ~29,500 indicators (global, no API key needed). (`economic_data_tools.rs:177-196`) | `query`, `topic_id`, `limit` |
| `wb_get_observations` | Fetch World Bank time series observations by indicator ID and country code. Returns date-value pairs. (`economic_data_tools.rs:200-219`) | `indicator_id`, `country_code`, `date_start`, `date_end`, `limit` |
| `wb_list_countries` | List World Bank countries with ISO3 codes, regions, income levels, and capital cities. Optional income_group filter: 'hic', 'mic', 'lic'. (`economic_data_tools.rs:222-241`) | `income_group`, `limit` |
| `wb_list_topics` | Browse World Bank topics (e.g., Poverty, Education, Health, Trade, Climate Change). Returns topic IDs and names for use with `wb_search_indicators` topic_id filter. (`economic_data_tools.rs:244-260`) | — |
| `wb_get_indicator_info` | Get World Bank indicator metadata: name, unit, source, description, source organization, and topics. (`economic_data_tools.rs:263-282`) | `indicator_id` |

### Economic data — DBnomics

Four tools wrapping the DBnomics API, defined in `src/economic_data_tools.rs:284-373`
and implemented in `src/economic_data/dbnomics.rs`. No API key required —
DBnomics aggregates 1.7B+ series from 700+ providers (IMF, OECD, ECB, INSEE,
World Bank, FRED mirrors, etc.), the global superset of FRED and the World
Bank Indicators API.

| Tool | Description | Key params |
|------|-------------|------------|
| `dbnomics_search` | Search DBnomics economic time series by full-text query across all providers (IMF, OECD, ECB, INSEE, World Bank, FRED mirrors, etc.). 1.7B+ series, no API key needed. (`economic_data_tools.rs:290-308`) | `query`, `limit`, `offset` |
| `dbnomics_list_providers` | List DBnomics statistical providers (700+ institutions: IMF, OECD, ECB, INSEE, World Bank, etc.). Returns provider code, name, region, and website. (`economic_data_tools.rs:311-330`) | `limit`, `offset` |
| `dbnomics_get_dataset` | Get DBnomics dataset metadata (name, description, dimensions, last update). Supports the `:latest` release alias (e.g., dataset_code='WEO:latest'). (`economic_data_tools.rs:333-352`) | `provider_code`, `dataset_code` |
| `dbnomics_get_series` | Get DBnomics series observations by provider/dataset/series code. Returns series metadata + observations array [{period, value}]. (`economic_data_tools.rs:355-373`) | `provider_code`, `dataset_code`, `series_code`, `observations`, `limit` |

### EQM rationale scoring

| Tool | Description | Key params |
|------|-------------|------------|
| `market_score_rationale` | Score a forecast rationale against Explanation Quality Markers (EQMs). Returns composite score, per-marker scores, red flags (warning signs), and green flags (good habits). Based on Karvetski et al. (2026), Forecasting Research Institute. Cost: ~$0.007 per rationale. (`economic_data_tools.rs:383-405`) | `rationale`, `forecast_probability`, `question` |

Unlike the data wrappers, this tool forwards to the `eqm` module
(`src/eqm.rs`, `eqm::score_rationale` at `economic_data_tools.rs:396`) and
uses the server's inference port for LLM scoring, not an external HTTP API.

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
- [The Forecasting Stack: Three-Layer Architecture](README.md#the-forecasting-stack-three-layer-architecture) — how this server feeds the scenarios/companies forecasting layers
- [MCP Server Registry](README.md) — built-in server index

## Footnotes

[^tetlock-pm-ref]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*. Crown Publishers.
    Cited for the outside-view / base-rate anchoring discipline the server's never-bare-probability invariant enforces.
