---
title: "Companies MCP Server — User Guide"
audience: [analysts, developers, agents]
last_updated: 2026-07-17
version: "0.31.0"
status: "Active"
domain: "Companies"
mds_categories: [domain, lifecycle]
last-verified-against: "fae4d94"
---

# Companies MCP Server — User Guide

**Diataxis type:** How-To

Task-oriented procedures for company valuation, forecasting, and portfolio analysis with the companies MCP server. Each section answers "how do I achieve X?" with direct, imperative instructions. For the complete tool catalog and behavioral boundaries, see the [Companies MCP Server Reference](../reference/mcp-servers/companies.md).

## Prerequisites

1. Build hKask: `cargo build --release` (see [Install and Configure](install-and-configure.md)).
2. Obtain API keys from Financial Modeling Prep and EOD Historical Data.
3. Export the required credentials:

```bash
export HKASK_FMP_API_KEY=your_fmp_key
export HKASK_EODHD_API_KEY=your_eodhd_key
```

4. Optional research providers (enable `research_search`):

```bash
export HKASK_EXA_API_KEY=your_exa_key
export HKASK_TAVILY_API_KEY=your_tavily_key
export HKASK_BRAVE_API_KEY=your_brave_key
```

5. Start the server:

```bash
kask mcp start companies
```

Tools are invoked by an agent holding a companies capability token (via `kask chat` or any MCP client pointed at the server). The examples below show the tool name and the arguments to supply.

## How to fetch financial statements

1. Ask for the income statement, balance sheet, or cash flow statement by symbol.
2. Supply a `limit` (periods to retrieve; default 5).

```
income_statement  { "symbol": "AAPL", "limit": 10 }
balance_sheet     { "symbol": "AAPL", "limit": 5 }
cash_flow_statement { "symbol": "AAPL", "limit": 5 }
```

The server routes to FMP or EODHD based on symbol shape, normalizes EODHD responses to FMP format, and returns JSON. International symbols (e.g. `VOD.L`, `BMW.DE`) route to EODHD as primary.

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

The forecast persists as an owner-scoped snapshot. Record the `forecast_id` — you need it to record the outcome later. The model projects revenue, COGS, gross profit, D&A, EBIT, tax, NOPAT, capex, net working-capital change, and free cash flow, with a Gordon-growth terminal value.

## How to solve for market-implied growth

1. Call `reverse_dcf` with the symbol and current price.
2. Read `implied_growth` — the revenue growth rate the market price implies.

```
reverse_dcf { "symbol": "AAPL", "current_price": 195.50 }
```

Compare `implied_growth` against your own estimate and management guidance to spot an expectations gap.

## How to run scenario and Monte Carlo analyses

1. Call `scenario_analysis` for the fixed growth × margin matrix.

```
scenario_analysis { "symbol": "AAPL" }
```

2. Call `monte_carlo_dcf` for a distribution over intrinsic value.

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

3. Call `sensitivity_analysis` to rank which inputs move intrinsic value the most.

```
sensitivity_analysis { "symbol": "AAPL" }
```

## How to calibrate and record a forecast

1. Call `calibrate_forecast` with growth and margin estimates and confidence weights.

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

The server reloads the stored snapshot, computes Brier scores, and performs a return-gap decomposition across the 11 line items.

4. List or retrieve prior forecasts for a symbol:

```
forecast_list { "symbol": "AAPL" }
forecast_get  { "forecast_id": "abc-123" }
```

## How to feed the provider-learning loop

1. After any financial-data tool returns, rate the result quality.

```
result_feedback {
  "tool": "income_statement",
  "symbol": "AAPL",
  "provider": "FMP",
  "score": 5
}
```

Scores 4–5 count as successes; 1–3 count as failures. The `LearningState` Beta posterior updates, and a provider that falls below P(success) = 0.70 with 5+ observations is flagged flaky and bypassed in future routing.

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

The server rejects imports above the request byte limit or the transaction count limit. See the [reference](../reference/mcp-servers/companies.md#behavioral-boundaries) for the exact limits.

## How to compute portfolio returns and attribution

1. Compute time-weighted and money-weighted returns over a date range:

```
portfolio_returns {
  "name": "core",
  "start_date": "2026-01-01",
  "end_date": "2026-06-30"
}
```

2. Rank which positions moved the portfolio:

```
portfolio_attribution { "name": "core", "start_date": "2026-01-01", "end_date": "2026-06-30" }
```

3. Compute weighted-average portfolio fundamentals:

```
portfolio_characteristics { "name": "core" }
```

## How to compare two portfolios

```
portfolio_comparison { "name_a": "core", "name_b": "satellite" }
```

Returns overlap, shared symbols, and positions unique to each portfolio.

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

2. List notes with optional filters:

```
note_list { "symbol": "AAPL", "tags": ["margin"] }
```

3. Attach a file (base64-encoded):

```
file_attach {
  "symbol": "AAPL",
  "filename": "model.xlsx",
  "content": "<base64-encoded bytes>"
}
```

Encoded payloads above the attachment byte limit are rejected. List and delete with `file_list` and `file_delete`.

## How to search for fundamental research

1. Ensure at least one research provider key is set (`HKASK_EXA_API_KEY`, `HKASK_TAVILY_API_KEY`, or `HKASK_BRAVE_API_KEY`).
2. Search across Exa, Tavily, and Brave:

```
research_search {
  "query": "AAPL services segment gross margin 2026",
  "max_results": 10
}
```

Claims are classified, tickers are detected, and numeric values are extracted. `research_search` bypasses the FMP/EODHD provider path.

## How to screen companies

```
company_screener {
  "query": "market cap over 100 billion, gross margin above 40%, dividend yield above 1%"
}
```

The natural-language criteria map to FMP screener parameters. `company_screener` is FMP-specific and bypasses the dual-provider routing.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `permission_denied` | No `DelegationToken` for the companies capability | Launch the agent with a companies capability token; see [Sovereignty and Observability](sovereignty-and-observability.md) |
| `invalid_argument: symbol must be ...` | Symbol exceeds 32 chars or contains invalid characters | Use a valid exchange symbol; international symbols are supported (e.g. `VOD.L`) |
| Provider returns stale data | Provider flagged chronically stale (>90 days) | Call `result_feedback` with a low score to update the `LearningState`; the flaky override reroutes future calls |
| `forecast task failed` | Portfolio SQLite error or owner mismatch | Verify the `forecast_id` belongs to the authenticated owner; forecasts are owner-scoped |
| `research_search` returns empty | No research provider keys configured | Export at least one of `HKASK_EXA_API_KEY`, `HKASK_TAVILY_API_KEY`, `HKASK_BRAVE_API_KEY` |

## Cross-links

- [Companies MCP Server Reference](../reference/mcp-servers/companies.md) — full tool catalog, configuration, and behavioral boundaries
- [Tool Routing and Dispatch Flow](../reference/mcp-servers/companies.md) — DIAG-RF-004 dispatch diagram (inline)
- [Install and Configure](install-and-configure.md) — build and profile setup
- [Sovereignty and Observability](sovereignty-and-observability.md) — capability tokens and Regulation alerts
- [Superforecasting: Layered Model](../explanation/forecasting-and-scenarios.md) — three-layer forecasting architecture