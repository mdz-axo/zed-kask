//! Ledger domain types — errors, postings, transactions, balances, query filters.

use hkask_storage::database::types::DbError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors the ledger can produce.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("double-entry violation: postings sum to {0}, must sum to 0")]
    DoubleEntryViolation(i64),
    #[error("idempotency conflict: reference '{reference}' already exists with different postings")]
    IdempotencyConflict { reference: String },
}

/// A single entry in a transaction — moves `amount` of `asset` from
/// `source` account to `destination` account. Amount is in the asset's
/// smallest integer unit (µrJ for rJ, µUSD for USD, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Posting {
    pub source: String,
    pub destination: String,
    pub asset: String,
    pub amount: i64,
}

/// An immutable transaction containing one or more postings. The `reference`
/// field provides idempotency — committing the same reference twice is a no-op.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerTransaction {
    pub id: String,
    pub timestamp: String,
    pub reference: String,
    pub postings: Vec<Posting>,
    pub metadata: serde_json::Value,
}

/// A computed balance for an account + asset pair.
#[derive(Debug, Clone, Serialize)]
pub struct AccountBalance {
    pub account: String,
    pub asset: String,
    pub balance: i64,
}

/// A time range for querying transactions.
#[derive(Debug, Clone)]
pub struct DateRange {
    pub start: String, // ISO 8601
    pub end: String,   // ISO 8601
}

/// Filters for transaction queries.
#[derive(Debug, Clone, Default)]
pub struct QueryFilter {
    pub asset: Option<String>,
    pub account: Option<String>,
    pub namespace: Option<String>,
}
