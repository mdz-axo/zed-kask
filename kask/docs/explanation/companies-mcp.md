---
title: "Companies MCP Server — User Guide"
audience: [analysts, developers, agents]
last_updated: 2026-08-05
version: "0.34.0"
status: "Active"
domain: "Companies"
mds_categories: [domain, lifecycle]
---

# Companies MCP Server — User Guide

**Diataxis type:** How-To

Task-oriented procedures for company valuation, forecasting, and portfolio analysis with the companies MCP server. Each section answers "how do I achieve X?" with direct, imperative instructions. For the complete tool catalog and behavioral boundaries, see the [Companies MCP Server Reference](../reference/mcp-servers/companies.md).

The server exposes **42 tools** (`#[tool]` functions under `kask/mcp-servers/hkask-mcp-companies/src/tools/`), grouped into seven tool modules:

| Module | Tools | Count |
|--------|-------|-------|
| `tools/financial_data.rs` | `company_profile`, `stock_quote`, `income_statement`, `balance_sheet`, `cash_flow_statement`, `key_metrics`, `historical_price`, `symbol_search` | 8 |
| `tools/analysis.rs` | `moat_check`, `management_scorecard`, `working_capital_cycle`, `company_screener`, `research_search` | 5 |
| `tools/expectations.rs` | `expectations_gap` | 1 |
| `tools/analytics.rs` | `portfolio_attribution`, `portfolio_characteristics`, `dcf_valuation`, `reverse_dcf`, `scenario_analysis` | 5 |
| `tools/economic_profit.rs` | `ep_valuation` | 1 |
| `tools/valuation.rs` | `comparable_analysis`, `sensitivity_analysis`, `equity_duration`, `monte_carlo_dcf`, `calibrate_forecast`, `forecast_get`, `forecast_list`, `forecast_record`, `result_feedback` | 9 |
| `tools/portfolio.rs` | `portfolio_delete`, `portfolio_list`, `ledger_import`, `ledger_export`, `transaction_note_append`, `portfolio_comparison`, `portfolio_returns`, `note_add`, `note_list`, `note_delete`, `file_attach`, `file_list`, `file_delete` | 13 |

## Prerequisites

The companies server is a builtin in-process MCP server registered inside the zed-kask editor (D1–D3). It is not started via a standalone CLI; zed-kask loads it automatically as part of its MCP server set. To use it:

1. Build zed-kask: `cargo build --release` (see the [zed-kask Host Architecture Plan](../architecture/zed-host-architecture-plan.md) for build and integration details).
2. Obtain API keys from Financial Modeling Prep and EOD Historical Data.
3. Configure the credentials via **Settings → Kask → Data Services** (recommended — keys are stored in the system keychain and injected automatically), or via environment variables:

```
HKASK_FMP_API_KEY=your_fmp_key
HKASK_EODHD_API_KEY=your_eodhd_key
```

4. Optional research providers (enable `research_search`) — also configurable via Settings → Kask → Data Services:

```
HKASK_EXA_API_KEY=your_exa_key
HKASK_TAVILY_API_KEY=your_tavily_key
HKASK_BRAVE_API_KEY=your_brave_key
```

5. Open the zed-kask agent panel and invoke the companies tools from a native agent. The server is already running in-process; there is no `kask mcp start` step.[^mcp-spec-companies]

Tools are invoked by an agent holding a companies capability token. The examples below show the tool name and the arguments to supply.

## Provider routing (FMP `/stable` + EODHD fallback)

Tools are provider-agnostic: each tool routes to FMP or EODHD based on symbol shape (`providers.rs` in `src/`). US-style symbols route to FMP's `/stable` endpoints; international symbols (e.g. `VOD.L`, `BMW.DE`) route to EODHD as primary, and EODHD responses are normalized to FMP format so downstream tools see one schema. A provider flagged chronically stale or flaky by the learning loop is bypassed in future routing.[^fibo-companies]

## How to fetch financial statements and quotes

1. Ask for the income statement, balance sheet, cash flow statement, or key metrics by symbol.
2. Supply a `limit` (periods to retrieve; default 5).

```
income_statement  { "symbol": "AAPL", "limit": 10 }
balance_sheet     { "symbol": "AAPL", "limit": 5 }
cash_flow_statement { "symbol": "AAPL", "limit": 5 }
key_metrics       { "symbol": "AAPL", "limit": 10 }
```

3. Fetch point-in-time data:

```
company_profile { "symbol": "AAPL" }
stock_quote     { "symbol": "AAPL" }
historical_price { "symbol": "AAPL", "from": "2026-01-01", "to": "2026-06-30" }
```

4. Resolve a ticker from a name or partial symbol:

```
symbol_search { "query": "Apple" }
```

## How to analyze competitive position (MAIA framework)

The analysis tools implement the MAIA methodology: gross-margin stability and working-capital market power signal the moat; returns on capital vs invested capital over time rate management's capital allocation.

1. Check the moat:

```
moat_check { "symbol": "AAPL" }
```

2. Score management's capital allocation:

```
management_scorecard { "symbol": "AAPL" }
```

3. Track the working capital cycle (days payable, days sales outstanding, cash conversion cycle):

```
working_capital_cycle { "symbol": "AAPL" }
```

## How to run a two-stage DCF valuation

1. Call `dcf_valuation` with a symbol and your growth and margin assumptions.
2. Read the `intrinsic_per_share` and `forecast_id` from the response.

```
dcf_valuation {
  "symbol": "AAPL",
  "revenue_growth": 0.08,
  "terminal_growth": 0.03,
  "gross_margin": 0.44,
  "discount_rate": 0.09
}
```

The forecast persists as an owner-scoped snapshot. Record the `forecast_id` — you need it to record the outcome later. The model projects revenue, COGS, gross profit, D&A, EBIT, tax, NOPAT, capex, net working-capital change, and free cash flow (11 line items per period), with a Gordon-growth terminal value. Default: 10-year model, 3-year stage 1, 7-year stage 2, 10% WACC, 2.5% terminal growth.[^gordon-growth]

## How to solve for market-implied growth

1. Call `reverse_dcf` with the symbol and current price.
2. Read `implied_growth` — the revenue growth rate the market price implies.

```
reverse_dcf { "symbol": "AAPL", "current_price": 195.50 }
```

Compare `implied_growth` against your own estimate and management guidance to spot an expectations gap (Mauboussin's *Expectations Investing*).[^damodaran-reverse-dcf]

## How to quantify the expectations gap

1. Populate claims with `research_search` (management guidance, analyst estimates).
2. Call `expectations_gap` with the symbol, your own growth estimate, and the gathered claims.

```
expectations_gap {
  "symbol": "AAPL",
  "own_growth_estimate": 0.07
}
```

The tool compares three growth estimates — market-implied (reverse DCF), management guidance extracted from research claims, and your own — and returns a structured gap report showing whether the market is pricing in more or less growth than guidance and your thesis.

## How to run scenario and Monte Carlo analyses

1. Call `scenario_analysis` for the fixed growth × margin matrix.[^schwartz-2x2]

```
scenario_analysis { "symbol": "AAPL" }
```

Four Schwartz scenarios (Bull, Land Grab, Cash Cow, Bear) run a DCF each; the response is the intrinsic value range. Adjustable multipliers tune scenario severity.

2. Call `monte_carlo_dcf` for a distribution over intrinsic value.[^monte-carlo-companies]

```
monte_carlo_dcf {
  "symbol": "AAPL",
  "revenue_growth_mean": 0.08,
  "revenue_growth_std": 0.02,
  "gross_margin_mean": 0.44,
  "gross_margin_std": 0.01,
  "discount_rate": 0.09,
  "simulations": 1000
}
```

Returns percentiles (p10/p25/median/p75/p90), a histogram, and the probability of undervaluation. Simulations are clamped to 100–10,000.

3. Call `sensitivity_analysis` to rank which inputs move intrinsic value the most (tornado chart).

```
sensitivity_analysis { "symbol": "AAPL" }
```

## How to value with Economic Profit and comparables

1. Economic Profit valuation (Bergen et al. 2025): book value + PV of future economic profits with competitive fade. The IVM ratio (intrinsic value / market cap) is the primary screening metric; the moat classification from `moat_check` determines how long economic profits persist.

```
ep_valuation { "symbol": "AAPL" }
```

2. Comparable company analysis — peer multiples (P/E, P/B, P/S, EV/EBITDA) with a DCF overlay for the target:

```
comparable_analysis { "symbol": "AAPL", "peers": "MSFT,GOOG" }
```

## How to measure equity duration

`equity_duration` computes a Macaulay-style duration of the company's projected free cash flows — D = Σ t·PV(CF_t) / Σ PV(CF_t) over the projection plus the terminal value timed at the horizon year. It also reports terminal/stage-1/stage-2 PV shares: the maturity profile of the equity claim.

```
equity_duration { "symbol": "AAPL" }
```

Pair with prediction-market `time_to_maturity` (hkask-mcp-prediction-markets `market_ladder`) for duration-matching across horizons.

## How to calibrate and record a forecast

1. Call `calibrate_forecast` with growth and margin estimates and confidence weights (Tetlock GJP methodology: Fermi decomposition, outside view, inside view, probability distribution over the four Schwartz scenarios).

```
calibrate_forecast {
  "symbol": "AAPL",
  "growth_estimates": [{"estimate": 0.08, "confidence": 0.7}],
  "margin_estimates": [{"estimate": 0.44, "confidence": 0.8}]
}
```

2. Wait for the forecast period to resolve.
3. Call `forecast_record` with the `forecast_id` and the actual outcome.

```
forecast_record {
  "forecast_id": "abc-123",
  "outcome": { "actual_revenue_growth": 0.06, "actual_price": 210.00 }
}
```

The server reloads the stored snapshot, computes Brier scores, and performs a return-gap decomposition across the 11 line items (revenue growth, gross margin, D&A, capex, NWC, multiple expansion, net debt).[^brier-companies]

4. List or retrieve prior forecasts for a symbol:

```
forecast_list { "symbol": "AAPL" }
forecast_get  { "forecast_id": "abc-123" }
```

Forecasts are durable and owner-scoped: list/get only return the authenticated owner's records.

## How to feed the provider-learning loop

1. After any tool returns, rate the result quality.

```
result_feedback {
  "tool": "income_statement",
  "symbol": "AAPL",
  "provider": "fmp",
  "score": 5
}
```

Score 1–5 (5 = exceeded expectations, 3 = met, 1 = missed); both score and comments are optional. The `provider` field names the data provider explicitly; when omitted it is inferred from the symbol/query. Scores 4–5 count as successes; 1–3 as failures. The `LearningState` Beta posterior updates, and a provider that falls below P(success) = 0.70 with 5+ observations is flagged flaky and bypassed in future routing.[^gelman-bda-companies]

## How to manage portfolios

1. List portfolios; delete one and all its data when done:

```
portfolio_list {}
portfolio_delete { "name": "old-sandbox" }
```

2. Compare two portfolios side by side (positions, overlap, unique symbols):

```
portfolio_comparison { "name_a": "core", "name_b": "satellite" }
```

## How to import a portfolio ledger

1. Prepare a CSV or JSON of transactions. CSV columns: `date,tx_type,symbol,quantity,price,commission,amount,currency,notes`. `tx_type` is one of `Buy`, `Sell`, `Dividend`, `Deposit`, `Withdrawal`.

```csv
date,tx_type,symbol,quantity,price,commission,amount,currency,notes
2026-01-15,Buy,AAPL,10,185.00,1.00,-1851.00,USD,opening
2026-02-20,Dividend,AAPL,,,0.00,12.50,USD,q1 div
```

2. Import the file (the portfolio is created if it does not exist):

```
ledger_import {
  "name": "core",
  "format": "csv",
  "content": "<base64-encoded CSV>"
}
```

3. Verify the import:

```
portfolio_list {}
ledger_export { "name": "core", "format": "json" }
```

4. Append a note to a specific transaction:

```
transaction_note_append { "transaction_id": 42, "note": "Added on margin inflection." }
```

The server rejects imports above the request byte limit or the transaction count limit. See the [reference](../reference/mcp-servers/companies.md#behavioral-boundaries) for the exact limits.[^input-validation-companies]

## How to compute portfolio returns and attribution

1. Compute time-weighted and money-weighted returns over a date range:[^bacon-returns]

```
portfolio_returns {
  "name": "core",
  "start_date": "2026-01-01",
  "end_date": "2026-06-30"
}
```

2. Rank which positions moved the portfolio (each position's weight, return, and contribution):

```
portfolio_attribution { "name": "core", "start_date": "2026-01-01", "end_date": "2026-06-30" }
```

3. Compute weighted-average portfolio fundamentals (valuation, profitability, leverage, growth, composition):

```
portfolio_characteristics { "name": "core" }
```

## How to attach notes and files to a security

1. Add a dated research note:

```
note_add {
  "symbol": "AAPL",
  "date": "2026-07-17",
  "content": "Services gross margin inflected to 70.5%.",
  "tags": ["services", "margin"]
}
```

2. List notes with optional filters; delete by ID:

```
note_list { "symbol": "AAPL", "tags": ["margin"] }
note_delete { "note_id": 7 }
```

3. Attach a file (base64-encoded); list and delete attachments:

```
file_attach {
  "symbol": "AAPL",
  "filename": "model.xlsx",
  "content": "<base64-encoded bytes>"
}
file_list   { "symbol": "AAPL", "portfolio": "core" }
file_delete { "file_id": 3 }
```

Encoded payloads above the attachment byte limit are rejected. `file_delete` removes both the record and the file from disk.

## How to search for fundamental research

1. Ensure at least one research provider key is set (`HKASK_EXA_API_KEY`, `HKASK_TAVILY_API_KEY`, or `HKASK_BRAVE_API_KEY`).
2. Search across Exa, Tavily, and Brave:

```
research_search {
  "query": "AAPL services segment gross margin 2026",
  "max_results": 10
}
```

Claims are classified, tickers are detected, and numeric values are extracted. `research_search` bypasses the FMP/EODHD provider path. Pair it with the `thesis_test`, `scenario_weight`, or `guidance_check` skills for structured analysis.[^rag-companies]

## How to screen companies

```
company_screener {
  "query": "market cap over 100 billion, gross margin above 40%, dividend yield above 1%"
}
```

Natural-language criteria map to FMP screener parameters (market cap, price, volume, P/E, dividend yield, beta, sector, industry, country, exchange, ROE, ROIC, and more). Use `criteria_overrides` to adjust parsed criteria; reply with a modified prompt to refine results. `company_screener` is FMP-specific and bypasses the dual-provider routing.[^fmp-screener]

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `permission_denied` | No `DelegationToken` for the companies capability | Invoke the companies tools from an agent session that holds a companies capability token; see [Capability Tokens](../diataxis/hkask-capability/explanation.md) |
| `invalid_argument: symbol must be ...` | Symbol exceeds 32 chars or contains invalid characters | Use a valid exchange symbol; international symbols are supported (e.g. `VOD.L`) |
| Provider returns stale data | Provider flagged chronically stale (>90 days) | Call `result_feedback` with a low score to update the `LearningState`; the flaky override reroutes future calls |
| `forecast task failed` | Portfolio SQLite error or owner mismatch | Verify the `forecast_id` belongs to the authenticated owner; forecasts are owner-scoped |
| `research_search` returns empty | No research provider keys configured | Set at least one of `HKASK_EXA_API_KEY`, `HKASK_TAVILY_API_KEY`, `HKASK_BRAVE_API_KEY` via Settings → Kask → Data Services or env vars |

## Cross-links

- [Companies MCP Server Reference](../reference/mcp-servers/companies.md) — full tool catalog, configuration, and behavioral boundaries
- [Tool Routing and Dispatch Flow](../reference/mcp-servers/companies.md) — DIAG-RF-004 dispatch diagram (inline)
- [zed-kask Host Architecture Plan](../architecture/zed-host-architecture-plan.md) — D1–D23 integration seams
- [Sovereignty and Observability](../diataxis/hkask-capability/explanation.md) — capability tokens and Regulation alerts
- [Superforecasting: Layered Model](forecasting-and-scenarios.md) — three-layer forecasting architecture
- [Earnings Transcript Analysis Design](earnings-transcript-analysis-design.md) — FMP-sourced transcript analysis design exploration

## Footnotes

[^mcp-spec-companies]: Anthropic. (2024). *Model Context Protocol Specification*. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the MCP protocol the companies server implements as an in-process builtin.

[^fibo-companies]: EDM Council. (2024). *Financial Industry Business Ontology (FIBO) Specification*. Enterprise Data Management Council. https://spec.edmcouncil.org/fibo/
    Cited for the financial-data ontology the dual-provider routing normalizes responses against.

[^gordon-growth]: Gordon, M. J., & Shapiro, E. (1956). Capital equipment analysis: The required rate of profit. *Management Science*, 2(1), 102–110. https://doi.org/10.1287/mnsc.2.1.102
    Cited for the Gordon-growth terminal-value model the two-stage DCF uses.

[^damodaran-reverse-dcf]: Damodaran, A. (2012). *Investment Valuation: Tools and Techniques for Determining the Value of Any Asset* (3rd ed.). John Wiley & Sons.
    Cited for the reverse-DCF methodology that solves for market-implied growth.

[^schwartz-2x2]: Schwartz, P. (1991). *The Art of the Long View*. Doubleday.
    Cited for the 2×2 growth-by-margin scenario matrix the `scenario_analysis` tool implements.

[^monte-carlo-companies]: Metropolis, N., & Ulam, S. (1949). The Monte Carlo method. *Journal of the American Statistical Association*, 44(247), 335–341. https://doi.org/10.1080/01621459.1949.10483310
    Cited for the Monte Carlo simulation methodology the `monte_carlo_dcf` tool uses.

[^brier-companies]: Brier, G. W. (1950). Verification of forecasts expressed in terms of probability. *Monthly Weather Review*, 78(1), 1–3. https://doi.org/10.1175/1520-0493(1950)078<0001:VOFERT>2.0.CO;2
    Cited for the Brier scoring formula the forecast calibration loop applies.

[^gelman-bda-companies]: Gelman, A., Carlin, J. B., Stern, H. S., Dunson, D. B., Vehtari, A., & Rubin, D. B. (2013). *Bayesian Data Analysis* (3rd ed.). CRC Press. https://www.routledge.com/books/Bayesian-Data-Analysis/9781439840955
    Cited for the Beta(α+1, β+1) conjugate-prior model the provider-learning loop uses.

[^input-validation-companies]: OWASP. (2023). *OWASP Input Validation Cheat Sheet*. OWASP Foundation. https://cheatsheetseries.owasp.org/cheatsheets/Input_Validation_Cheat_Sheet.html
    Cited for the input-size validation principle the ledger import and file attachment limits enforce.

[^bacon-returns]: Bacon, C. P. (1966). The arithmetic of yield and capital gains/losses. *Financial Analysts Journal*, 22(6), 102–109.
    Cited for the time-weighted and money-weighted return methodologies the portfolio returns tools compute.

[^rag-companies]: Lewis, P., et al. (2020). Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks. arXiv. https://arxiv.org/abs/2005.11401
    Cited for the retrieval-augmented generation paradigm the `research_search` tool follows — classify, detect tickers, extract values from retrieved text.

[^fmp-screener]: Financial Modeling Prep. (2024). *FMP Stock Screener API*. https://site.financialmodelingprep.com/developer/docs/stock-screener-api
    Cited for the FMP screener endpoint the natural-language criteria map to.
