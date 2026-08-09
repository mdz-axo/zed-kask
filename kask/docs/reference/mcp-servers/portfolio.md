---
title: "Portfolio MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-06
version: "0.33.7"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle]
---

# Portfolio MCP Server Reference

**Crate:** `mcp-servers/hkask-mcp-portfolio`
**Tools:** 13 — `portfolio_create`, `portfolio_delete`, `portfolio_list`, `ledger_apply`, `ledger_read`, `portfolio_snapshot`, `portfolio_returns`, `ledger_import`, `ledger_export`, `portfolio_seed_price`, `portfolio_roll`, `portfolio_rebuild_views`, `portfolio_materialize_returns`, `portfolio_daily_returns`
**Auto-start:** No (requires explicit opt-in via KaskSettings toggle (D9a))

The portfolio server is the general-purpose transaction-ledger portfolio store.
It is provider-agnostic — it knows nothing about FMP/EODHD stock prices or
Kalshi/Polymarket contract feeds. Callers resolve prices externally and feed
them to `portfolio_returns` via the `price_cache` table (seeded with
`portfolio_seed_price`).

## Architecture

A portfolio is an append-only transaction ledger. Everything else — holdings,
returns, validation — is a projection over that ledger at a point in time.
The store supports nested portfolios (a portfolio of CMP indices, each of which
is a portfolio of contracts) via the `AssetType` enum (`Stock`,
`PredictionContract`, `Portfolio`).

### Materialized views

- **`daily_holdings`** — end-of-day positions, cached for fast retrieval by the
  portfolio viewer. Computed by `portfolio_snapshot`, invalidated by `ledger_apply`.
- **`daily_returns`** — daily P&L (market value + cash + total + daily return),
  computed by `portfolio_materialize_returns` (incremental O(N+D) walk), and
  by `portfolio_rebuild_views` (full rebuild from the ledger).

Both views are rebuildable from the ledger (the append-only source of truth)
via `portfolio_rebuild_views`.

### The 7-method operational interface (essentialist G2)

The `PortfolioStore` has 7 core operational methods:
`list`, `create`, `delete`, `apply`, `ledger`, `snapshot`, `rebuild_views`.
Plus `materialize_returns` and `daily_returns` for the returns view (9 total —
one over the guideline; each has a distinct purpose).

## Source modules

| Module | Role |
|--------|------|
| `hkask_mcp_portfolio.rs` | The `PortfolioStore` — ledger, holdings, returns, import/export |
| `server.rs` | MCP server — 13 tools + schema-compliance tests |
| `main.rs` | Binary entrypoint |

## Tool surface

| Tool | Role |
|------|------|
| `portfolio_create` | Create a portfolio (stock, prediction-contract, or nested) |
| `portfolio_delete` | Delete a portfolio + all data (FK cascade) |
| `portfolio_list` | List all portfolios |
| `ledger_apply` | Append a transaction (buy, sell, roll, weight_adjust, deposit, withdrawal, dividend) |
| `ledger_read` | Read transactions with optional filter (symbol, type, asset_type, date range) |
| `portfolio_snapshot` | Materialized end-of-day holdings (cached) |
| `portfolio_returns` | TWR + IRR for a date range (reads from price cache) |
| `ledger_import` | Import CSV/JSON (auto-creates portfolio) |
| `ledger_export` | Export CSV/JSON |
| `portfolio_seed_price` | Seed the price cache for (portfolio, symbol, date) |
| `portfolio_roll` | Roll a constituent to a successor contract (CMP index maintenance) |
| `portfolio_rebuild_views` | Rebuild all materialized views from the ledger |
| `portfolio_materialize_returns` | Materialize the daily returns view for a date range |
| `portfolio_daily_returns` | Read the materialized daily returns |

## Consumers

- **`hkask-mcp-companies`** — delegates portfolio CRUD, ledger, and returns
  computation to this crate. The companies `portfolio_returns` tool seeds the
  price cache from FMP/EODHD, then calls `compute_returns` here. Provenance
  points to `hkask-mcp-portfolio`.
- **`hkask-mcp-prediction-markets`** — stores CMP indices as transaction-ledger
  portfolios via `market_cmp_index_store` and `market_cmp_portfolio_store`.
- **`hkask-portfolio-widget`** — renders holdings + returns for any portfolio
  type (stock, prediction-event, CMP index) via the `HoldingsBody` block field.

## Credential allowlist

The portfolio server is provider-agnostic: `credentials: Some(&[])`,
`config_env: Some(&[])`. It reads only `HKASK_WEBID` (identity, injected by the
runtime) and writes to the owner-scoped SQLite DB under the config dir.
