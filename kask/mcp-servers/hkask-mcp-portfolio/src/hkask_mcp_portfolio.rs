#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Portfolio — general-purpose transaction-ledger portfolio store.
//!
//! A portfolio is an append-only transaction ledger. Everything else —
//! holdings, returns, validation — is a projection over that ledger at a
//! point in time. This crate is provider-agnostic: it knows nothing about
//! FMP/EODHD stock prices or Kalshi/Polymarket contract feeds. Callers
//! resolve prices externally and feed them to [`returns`].
//!
//! ## Asset types and nested portfolios
//!
//! The ledger is polymorphic over the asset being transacted. [`AssetType`]
//! discriminates stocks, prediction-event contracts, and nested portfolios
//! (a portfolio of CMP indices, each of which is itself a portfolio of
//! contracts). A nested-portfolio holding is a weighted reference to another
//! portfolio by name — the store resolves it recursively on demand.
//!
//! ## Materialized views
//!
//! [`PortfolioStore::snapshot`] computes end-of-day holdings from the ledger.
//! The result is cached in a `daily_holdings` materialized-view table so the
//! portfolio viewer retrieves it without recomputing the full ledger history.
//! [`returns`] computes daily P&L from two snapshots + a price resolver; the
//! per-day result is cached in `daily_returns`. Both views are rebuildable
//! from the ledger (the append-only source of truth) via
//! [`PortfolioStore::rebuild_views`].

mod returns;
mod store;
mod types;

pub use returns::{
    CachedPriceResolver, compute_irr, export_csv, export_json, import_csv, import_json, parse_ymd,
    returns,
};
pub use store::PortfolioStore;
pub use types::{
    AssetType, DailyReturnRow, Holding, HoldingsSnapshot, LedgerFilter, NoPrices, PortfolioError,
    PriceResolver, ReturnsReport, Transaction, TxType,
};

#[allow(unused_imports)]
pub(crate) use types::{
    MAX_IMPORT_REQUEST_BYTES, MAX_IMPORT_TRANSACTION_COUNT, check_request_size,
};

pub mod server;

pub use server::{map_portfolio_error, run};

