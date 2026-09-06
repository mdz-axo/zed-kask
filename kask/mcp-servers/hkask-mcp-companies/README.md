# hkask-mcp-companies

Company-finance MCP server for provider-routed market data, fundamental analysis, valuation, research retrieval, and local portfolio-ledger operations.

## Tools (42)

| Group | Tools |
|---|---:|
| Financial data | 8 |
| Analysis and research | 5 |
| Portfolio analytics and DCF | 5 |
| Valuation and forecasting | 9 |
| Economic-profit and expectations analysis | 2 |
| Portfolio ledger, notes, and files | 13 |

### Financial data

| Tool | Description |
|---|---|
| `company_profile` | Get a company profile. |
| `stock_quote` | Get a stock quote. |
| `income_statement` | Get an income statement. |
| `balance_sheet` | Get a balance sheet. |
| `cash_flow_statement` | Get a cash-flow statement. |
| `key_metrics` | Get key financial metrics. |
| `historical_price` | Get historical price data. |
| `symbol_search` | Search for symbols. |

### Analysis and research

| Tool | Description |
|---|---|
| `moat_check` | Analyze competitive moat through gross-margin stability and working-capital market-power signals. |
| `management_scorecard` | Score CEO capital allocation against returns on capital and invested capital. |
| `working_capital_cycle` | Analyze days payable, days sales outstanding, and cash-conversion cycle. |
| `company_screener` | Screen companies from natural-language criteria using the FMP stock screener. |
| `research_search` | Search Exa, Tavily, and Brave for company-specific fundamental-research claims. |

### Portfolio analytics and DCF

| Tool | Description |
|---|---|
| `portfolio_attribution` | Rank position contributions to portfolio movement. |
| `portfolio_characteristics` | Calculate weighted-average portfolio valuation, profitability, leverage, growth, and composition. |
| `dcf_valuation` | Build a two-stage DCF valuation and return an intrinsic value and forecast ID. |
| `reverse_dcf` | Solve for the revenue growth implied by the current market price. |
| `scenario_analysis` | Run four growth-by-margin scenarios and return the intrinsic-value range. |

### Valuation and forecasting

| Tool | Description |
|---|---|
| `comparable_analysis` | Compare peer valuation multiples with a DCF overlay. |
| `sensitivity_analysis` | Rank DCF inputs by their effect on intrinsic value. |
| `monte_carlo_dcf` | Simulate DCF assumptions and return an intrinsic-value distribution. |
| `scenario_impact_valuation` | Compose a company's DCF from scenario event-tree impact mappings. Reverse bridge from hkask-mcp-scenarios. |
| `calibrate_forecast` | Calibrate growth and margin estimates into scenario-weighted intrinsic value. |
| `forecast_get` | Retrieve one durable forecast and its recorded outcomes for the authenticated owner. |
| `forecast_list` | List an authenticated owner's durable forecasts for a symbol. |
| `forecast_record` | Record a forecast outcome, Brier scores, and optional return-gap decomposition. |
| `result_feedback` | Rate a previous tool result to feed the provider-learning loop. |

### Economic-profit and expectations analysis

| Tool | Description |
|---|---|
| `ep_valuation` | Value a company from book value plus discounted future economic profit with competitive fade. |
| `expectations_gap` | Compare market-implied growth with management guidance and a supplied estimate. |

### Portfolio ledger, notes, and files

| Tool | Description |
|---|---|
| `portfolio_list` | List portfolios. |
| `portfolio_delete` | Delete a portfolio and all its data. |
| `ledger_import` | Import CSV or JSON transactions into a portfolio ledger. |
| `ledger_export` | Export a portfolio ledger as CSV or JSON. |
| `transaction_note_append` | Append a note to an existing transaction. |
| `portfolio_comparison` | Compare two portfolios' positions, overlap, and unique symbols. |
| `portfolio_returns` | Calculate time-weighted and money-weighted returns for a date range. |
| `note_add` | Add a dated note to a company or security. |
| `note_list` | List notes for a symbol, optionally filtered by date range or tags. |
| `note_delete` | Delete a note by ID. |
| `file_attach` | Attach a base64-encoded file to a company or security. |
| `file_list` | List a portfolio's attached files for a symbol. |
| `file_delete` | Delete an attached file by ID. |

See the [Companies MCP Server Reference](../../docs/reference/mcp-servers/companies.md) for the full tool catalog, behavioral boundaries, and the code-anchored tool-routing diagram (DIAG-RF-004). The [Companies User Guide](../../docs/how-to/companies-mcp.md) covers task-oriented procedures for valuation, forecasting, and portfolio operations.

## Configuration

| Variable | Required | Description |
|---|---|---|
| `HKASK_FMP_API_KEY` | Yes | Financial Modeling Prep API key |
| `HKASK_EODHD_API_KEY` | Yes | EOD Historical Data API key |
| `HKASK_EXA_API_KEY` | No | Exa research-search provider key |
| `HKASK_TAVILY_API_KEY` | No | Tavily research-search provider key |
| `HKASK_BRAVE_API_KEY` | No | Brave research-search provider key |
| `HKASK_FERMI_DEFAULTS` | No | JSON object with `growth` and `margin` Fermi-question arrays |
| `HKASK_CHRONIC_STALENESS_DAYS` | No | Chronic-staleness threshold in days for the `LearningState` provider-learning loop (default `90`); a provider whose latest filing is older than this is bypassed by `preferred_provider` |

Example Fermi defaults:

```bash
export HKASK_FERMI_DEFAULTS='{"growth":[{"estimate":0.70,"confidence":0.8}],"margin":[{"estimate":0.30,"confidence":0.7}]}'
```

## Architecture

```text
src/
├── lib.rs              server composition, forecast store, dispatch
├── learning.rs         provider-learning regulator (Beta posterior, staleness)
├── tools/              MCP tool routers and handlers
├── types.rs            MCP request schemas
├── providers.rs        FMP/EODHD routing and normalization
├── analysis.rs         MAIA-style fundamental calculations
├── financial_model.rs  two-stage financial-statement projection model
├── economic_profit.rs  economic-profit valuation model
├── scenarios.rs        fixed growth × gross-margin scenario matrix
├── superforecast.rs    Fermi calibration and Brier scoring
├── research.rs         Exa, Tavily, and Brave research retrieval
├── fibo.rs             FIBO concept identifiers used by derived outputs
└── portfolio.rs        SQLite-backed ledger, notes, and attachments
```

### Behavioral boundaries

- Financial-data tools route eligible symbol lookups between FMP and EODHD. `company_screener` is FMP-specific; `research_search` uses its own research providers.
- The DCF projection is a two-stage model using a Gordon-growth terminal value. It models revenue, COGS, gross profit, D&A, EBIT, tax, NOPAT, capex, net working-capital change, and free cash flow. It does not model SG&A as a separate line item, an exit-multiple terminal method, or other non-operating assets in the equity bridge.
- `scenario_analysis` runs a fixed revenue-growth × gross-margin matrix.
- DCF and calibrated forecasts persist as owner-scoped structured JSON snapshots. `forecast_get` retrieves one record, `forecast_list` returns a symbol's history, and `revision_of` links a same-symbol revision. `forecast_record` appends outcomes and reloads the stored snapshot for decomposition.
- Some derived responses include a `fibo` map. Raw provider payloads are returned without a FIBO mapping, and emitted identifiers are compact strings rather than a JSON-LD context.

### Acquisition and DCF preparation decision — 2026-09-06

Operator-approved acquisition/valuation slice, preserving the typed-view intent of `160cef9fab`, the two-layer cache of `43e28cc484`, and visible data gaps from `b8057bb2f4`:

- `CompaniesServer::fetch_response` owns acquisition, learning-aware routing and cache policy. Raw readers and typed readers use the same normalized metrics. FMP ratios/growth join by fiscal date; EODHD metrics come only from EODHD annual statements, with approximation warnings (including its existing simplified ROIC).
- The public `key_metrics` content is now `{data: [...], provider: "FMP" | "EODHD", warnings: [...]}` (plus ontology). Consumers of the former bare array must read `data`. Other raw financial-data tools retain their existing output shapes.
- Acquisition cache keys use `normalized-v1:` and store the normalized payload, actual provider and warnings together. This supersedes the ambiguous raw/enriched cache representation: existing rows are ignored, not migrated or deleted. Endpoint TTLs and the second-layer concept extraction remain; the concept-cache wire/remove decision is not part of this change. No research/portfolio data is deleted.
- Targets and concurrent peers share the server acquisition path. Comparison rows include endpoint provenance, warnings and explicit errors; absent metrics are omitted rather than zeroed. Cached warnings remain visible after reopening the server.
- `valuation_service::prepare_dcf` owns typed financial inputs, history, sector/price/share/history guards and common assumption preparation for standalone DCF and comparable overlays. Identical inputs and common request assumptions produce identical intrinsic value, price and margin of safety. Tool handlers retain formatting and forecast persistence; EP, Monte Carlo and the other valuation methods remain distinct. Missing price/shares now produce explicit DCF unavailability rather than nominal zero-price/share fallbacks. An overlay provider outage preserves the comparison table with an explicit overlay error; invalid assumptions remain typed request errors.

## Validation

Offline library suite (including loopback HTTP fixtures at the actual provider boundary):

```bash
env -u HKASK_FMP_API_KEY -u HKASK_EODHD_API_KEY cargo test -p hkask-mcp-companies --lib
GITHUB_ACTIONS=1 ./script/clippy -p hkask-mcp-companies
```

`src/acquisition_tests.rs` covers acquisition order, date joins and supplement failures, EODHD normalization, legacy/warm-cache behavior and provenance, target/peer cache and learning policy, concurrent peers, and DCF equivalence/guards. The library's live checks skip without provider keys. `tests/fmp_endpoint_schema.rs` calls real FMP endpoints when `HKASK_FMP_API_KEY` is set; it is not an offline fixture suite. These tests call real tool handlers but do not exercise MCP transport framing.
