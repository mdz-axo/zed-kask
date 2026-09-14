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
| `company_screener` | One universe-screening capability. Immediate mode parses ad hoc criteria; saved-screen mode renders a registered Jinja or direct API `ScreenDefinition`, submits an asynchronous calculate job, exposes status, and pages an immutable columnar result. |
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
| `expectations_gap` | Compare implied revenue growth and profitability with demonstrated capability using the investor's target return as the equity component of modified WACC; surface DuPont ROE and Higgins SGR separately. |

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
├── screener.rs         natural-language screening prompt parser (EODHD)
├── fibo.rs             FIBO concept identifiers used by derived outputs
└── portfolio.rs        SQLite-backed ledger, notes, and attachments
```

### Behavioral boundaries

- Financial-data tools route eligible symbol lookups between FMP and EODHD. `company_screener` saved screens use `calculate` → `status`/`cancel` → paginated `results`. Expectations-gap jobs checkpoint the deterministic financial pass set before enrichment, persist one resumable work item per issuer, publish stage/progress/outcome counts plus a two-second heartbeat, rate-limit issuer enrichment, retain failures as explicit `unavailable` rows, and save the terminal JSON report under `companies-mcp/reports/`. Restart recovery requeues durable jobs and skips universe acquisition when the checkpoint exists. The financial pass applies USD capitalization and normalized `adjusted_close × avgvol_200d` liquidity before one fundamentals request per provisional issuer; pagination includes full exclusions only on the first page. `research_search` remains downstream and is not a screening stage.
- One authoritative driver model serves DCF, reverse DCF, expectations gaps, scenarios, sensitivity, Monte Carlo, equity duration, calibration, and outcome decomposition. Non-financial projections reconcile gross profit, SG&A, D&A, and an explicit other-operating-expense residual to reported operating income before projecting linked statements and FCFF; missing or contradictory reconciliation is unavailable. Financial issuers use residual income. Growth fades to terminal growth, balance differences remain observable rather than being plugged, and terminal value uses Gordon growth.
- `scenario_analysis` runs a fixed revenue-growth × gross-margin matrix.
- DCF and calibrated forecasts persist as owner-scoped structured JSON snapshots. `forecast_get` retrieves one record, `forecast_list` returns a symbol's history, and `revision_of` links a same-symbol revision. `forecast_record` appends outcomes and reloads the stored snapshot for decomposition.
- Some derived responses include a `fibo` map. Raw provider payloads are returned without a FIBO mapping, and emitted identifiers are compact strings rather than a JSON-LD context.

### Acquisition and DCF preparation decision — 2026-09-06

Operator-approved acquisition/valuation slice, preserving the typed-view intent of `160cef9fab`, the two-layer cache of `43e28cc484`, and visible data gaps from `b8057bb2f4`:

- `CompaniesServer::fetch_response` owns acquisition, learning-aware routing and cache policy. Raw readers and typed readers use the same normalized metrics. FMP ratios/growth join by fiscal date; EODHD metrics come only from EODHD annual statements, with approximation warnings (including its existing simplified ROIC).
- The public `key_metrics` content is now `{data: [...], provider: "FMP" | "EODHD", warnings: [...]}` (plus ontology). Consumers of the former bare array must read `data`. Other raw financial-data tools retain their existing output shapes.
- Acquisition cache keys use `normalized-v3:` and store the normalized payload, actual provider and warnings together. Raw-only, pre-sanitizer `normalized-v1:`, and pre-coercion `normalized-v2:` entries are never read or logged by acquisition; a one-time on-demand fetch populates the current representation. Old bytes remain on disk, not migrated or deleted. Endpoint TTLs and the second-layer concept extraction remain; the concept-cache wire/remove decision is not part of this change. No research/portfolio data is deleted.
- Targets and concurrent peers share the server acquisition path. Comparison rows include endpoint provenance, warnings and explicit errors; absent metrics are omitted rather than zeroed. Cached warnings remain visible after reopening the server.
- `valuation_service::prepare_dcf` owns typed financial inputs, history, sector/price/share/history guards and common assumption preparation for standalone DCF and comparable overlays. Identical inputs and common request assumptions produce identical intrinsic value, price and margin of safety. Tool handlers retain formatting and forecast persistence; EP, Monte Carlo and the other valuation methods remain distinct. Missing price/shares now produce explicit DCF unavailability rather than nominal zero-price/share fallbacks. An overlay provider outage preserves the comparison table with an explicit overlay error; invalid assumptions remain typed request errors.

Review hardening of this same slice:

- FMP/EODHD acquisition transport errors remove the credential-bearing URL before formatting the cause chain for warnings, logs or cache. Provider, endpoint and operation context remain.
- History and DCF preparation share one optional numeric share resolver: income diluted/basic → metrics diluted/basic → profile. Null/missing/nonnumeric candidates do not resolve; explicit numeric zero/negative values are not bypassed and DCF rejects nonpositive/nonfinite values. Other models retain their existing nominal fallback when no numeric source resolves.
- `PreparedDcf` computes the existing `ModelInputQuality` once. Standalone and overlay serialize that same type under `data_quality`, including `quality_warning`; model-quality rules and projection math are unchanged.
- One comparison-row builder validates both target and peer results. Empty/nonarray metrics produce an endpoint-specific error without hiding an available profile, comparison table or overlay.

## Expectations-gap definition — amended 2026-09-14

Operator ruling, superseding both the guidance-gap definition that arrived with the hkask migration (`af7613e11a`) and the 2026-09-10 use of Higgins SGR as the primary revenue-growth benchmark:

- The gap is between what the price implies and what the company has demonstrated it can do — never between price and management guidance. Guidance is a context annotation only.
- Demonstrated capability is the DuPont decomposition: ROE = net profit margin × asset turnover × equity multiplier, plus the Higgins sustainable growth rate SGR = ROE × retention (self-funding growth without external financing). Asset turnover and ROE use average beginning/ending assets and equity for each measured income period; displayed components are robust per-period medians.
- Industry-aware profitability headline: ROE for financial-sector companies (price-implied ROE from the justified P/B identity, P/B = (ROE − g)/(r − g), where `r` is the investor target return); net margin for everyone else — net income / revenue, interest at demonstrated leverage and tax included.
- Discounting follows the MAIA investor perspective (`MA_Guidebook_July23.md`, “A Note on Valuation” and “Valuation”): the equity component is the investor's required return, 15% by MAIA default or an explicit request override. Modified WACC = equity weight × investor target return + debt weight × pre-tax debt cost × (1 − tax rate). For financial issuers the target return is used directly.
- For non-financials, the primary growth gap is like-for-like: reverse-DCF-implied revenue growth minus demonstrated full-period revenue CAGR. Financing headroom is separately reported as implied revenue growth minus Higgins SGR; SGR is not treated as demonstrated revenue growth.
- The profitability gap is implied net margin at demonstrated revenue CAGR minus demonstrated net margin (median actual net income / revenue). Financials continue to use implied ROE minus demonstrated ROE.
- Before either price-implied solve, the selected security's price is converted from its listing currency and unit into the normalized financial-statement currency using cached EODHD USD cross-rates. Minor units are explicit (`GBX` → `GBP` at 0.01); missing currency metadata or rates surface as unavailable rather than entering valuation unconverted.
- Capability estimates at or beyond the reverse DCF's validated growth range are `model_sensitive` and carry explicit flags. Gap-data completeness is independent of model sensitivity: mixed or positive gaps remain complete when both legs were calculated.
- The prior square-root composite score is withheld because it has no external calibration. Saved-screen output exposes raw growth, financing-headroom, and profitability dimensions without a synthetic ranking.
- Net margin is net income / revenue. It is never approximated by a pre-interest operating formula: the solve uses GM = NM/(1−tax) + SG&A% + other operating expense% + interest% + D&A%, holding modeled expenses at demonstrated revenue shares, so the solved value satisfies NI = (EBIT − interest) × (1 − tax) by construction. Gross margin is the internal projection parameter only, never a reported expectations quantity.

## Validation

Offline library suite (including loopback HTTP fixtures at the actual provider boundary):

```bash
env -u HKASK_FMP_API_KEY -u HKASK_EODHD_API_KEY cargo test -p hkask-mcp-companies --lib
GITHUB_ACTIONS=1 ./script/clippy -p hkask-mcp-companies
```

Published numeric oracles include Wall Street Prep's 12.4% reverse-DCF case, AnalystPrep CFA's 8.33% DuPont case, and Wall Street Prep's 12.5% Higgins SGR case. `src/acquisition_tests.rs` covers acquisition order, date joins and supplement failures, EODHD normalization, legacy/warm-cache behavior and provenance, target/peer cache and learning policy, concurrent peers, DCF equivalence/guards, listing-price to statement-currency normalization, and the screener's per-exchange fan-out, unconverted USD bounds, and offset-cap pagination (parser contracts are pinned in `src/screener.rs` tests). The library's live checks skip without provider keys. `tests/fmp_endpoint_schema.rs` calls real FMP endpoints when `HKASK_FMP_API_KEY` is set; it is not an offline fixture suite. These tests call real tool handlers but do not exercise MCP transport framing.
