---
title: "Portfolio MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-09-17
version: "0.40.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle]
---

# Portfolio MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-portfolio`
**Tools:** 18 — the 13 ledger/return tools plus `portfolio_contribution`, `portfolio_characteristics`, `portfolio_attribution`, `portfolio_what_if`, and `portfolio_historical_what_if`. (2026-09-17: investor reports moved into their authoritative portfolio server; daily-return data remains an internal calculation/materialization surface, not the portfolio panel's user experience.)
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.portfolio=false` disables this server (`kask/crates/kask_bridge/src/settings.rs:140-165`; `kask/crates/kask_bridge/src/mcp_servers.rs:55-79,704`).

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

### Shared-database recovery — D01 ratified 2026-09-07

The operator confirmed that companies research notes and forecasts **must survive
portfolio schema recovery** ("D01 yes. please proceed"). This supersedes the
whole-file discard policy introduced by `804cf441a8`/`f28789fc81` where the
portfolio database also contains another domain's data. Portfolio-only legacy
data remains disposable; this decision does not authorize deleting research or
silently migrating its relationships.

Recovery must refuse a destructive reset when non-portfolio tables are present,
preserving their rows and the portfolio parents referenced by research notes
and attachments. An incompatible shared database may report an explicit startup
error rather than lose data. Compatible shared databases continue to open.

Enforced transactionally in `open_with_schema_recovery`
(`kask/mcp-servers/hkask-mcp-portfolio/src/store.rs`): an IMMEDIATE transaction wraps DDL, the `sqlite_schema`
ownership inspection (internal `sqlite_*` names excluded via GLOB), and the
portfolio-only drop-and-rebuild; any other table, an inspection error, or a
failed reset rolls back. Verified 2026-09-07: the five `schema_recovery_*`
regressions in `kask/mcp-servers/hkask-mcp-portfolio/src/tests.rs` pass, including observed pre-fix RED (the
unlink/recreate recovery destroyed seeded research rows) and post-fix GREEN;
the companies crate's 68-test suite is green over the shared DB. The
user-visible trade-off stands: incompatible shared databases now error at
startup instead of silently losing research — no migration is attempted.

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
| `kask/mcp-servers/hkask-mcp-portfolio/src/hkask_mcp_portfolio.rs` | Library root and generated tool-name pin |
| `kask/mcp-servers/hkask-mcp-portfolio/src/store.rs` | `PortfolioStore` — ledger, holdings, returns, import/export |
| `kask/mcp-servers/hkask-mcp-portfolio/src/analysis.rs` | Provider-agnostic investor reports and counterfactual projections |
| `kask/mcp-servers/hkask-mcp-portfolio/src/server.rs` | MCP server — 18 tools and live router |
| `kask/mcp-servers/hkask-mcp-portfolio/src/main.rs` | Binary entrypoint |

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
| `portfolio_contribution` | Absolute security profit contribution; includes trades, commissions, and symbol-assigned dividends and reconciles to portfolio return |
| `portfolio_characteristics` | Composition, concentration, classifications, and supplied company metrics with metric-specific aggregation and coverage |
| `portfolio_attribution` | Explicit-benchmark Brinson–Fachler allocation, selection, and separately reported interaction effects |
| `portfolio_what_if` | Prospective same-date composition changes and characteristic comparison over cloned ledger state. Optional `presentation: "workbook_what_if"` additionally publishes the transaction set and report deltas as an editable workbook revision (a ` ```spreadsheet ` block rendered by the spreadsheet widget; the portfolio report hint is unchanged) |
| `portfolio_historical_what_if` | Retrospective opportunity-cost comparison using realized subsequent prices; explicitly not an ex-ante forecast |
| `ledger_import` | Import CSV/JSON (auto-creates portfolio) |
| `ledger_export` | Export CSV/JSON |
| `portfolio_seed_price` | Seed the price cache for one (symbol, date) or a batch (`prices` array); invalidates materialized views from each seeded date forward |
| `portfolio_rebuild_views` | Rebuild all materialized views from the ledger |
| `portfolio_materialize_returns` | Materialize the daily returns view for a date range |
| `portfolio_daily_returns` | Read the materialized daily returns |

**Price semantics (2026-09-03):** the cached resolver is as-of — the
latest price on or before each date (weekends carry the prior close). A
held position with no resolvable price is a data gap, NOT a zero
valuation: `portfolio_returns` and `portfolio_materialize_returns`
error naming the missing (symbol, date) pairs instead of fabricating
returns. The gate is skipped for `NoPrices` portfolios (CMP indices —
holdings value at zero by design), which `rebuild_views` selects
automatically by the portfolio's asset type. `portfolio_seed_price`
invalidates materialized views from the seeded date forward, so
materialize-then-seed never serves stale rows.

## Consumers

- **`hkask-mcp-companies`** — shares the database for owner-scoped company research artifacts, but does not register portfolio analytics or ledger tools; the 40-tool pin keeps ownership with this server (`kask/mcp-servers/hkask-mcp-companies/src/hkask_mcp_companies.rs:482-492`).
- **`hkask-mcp-prediction-markets`** — stores CMP indices as transaction-ledger
  portfolios via `market_cmp_index_store` and `market_cmp_portfolio_store`.
- **`portfolio_panel`** — observes the active or resumed Steer thread and renders server-authored investor report display hints in its upper viewer.
- **`hkask-portfolio-widget`** — preserves existing inline holdings + returns rendering for any portfolio type (stock, prediction-event, CMP index).

## Credential allowlist

The portfolio server is provider-agnostic: `credentials: Some(&[])`. Its current config allowlist is exactly `HKASK_DATA_DIR`, `HKASK_ARTIFACTS_DIR`, and `HKASK_TRANSACTIONS_DIR`: the database remains under the internal data root, while transaction import files resolve under the visible artifacts root. The descriptor and an allowlist-alignment test pin this boundary (`kask/crates/kask_bridge/src/mcp_servers.rs:55-79,1084-1120`).
