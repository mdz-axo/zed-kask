//! Verification error type.

use thiserror::Error;

/// Errors from the verification store (the central grounding ledger).
///
/// The store returns `Err` on DB failures rather than collapsing to an empty
/// trend — the `.rules` broken-feedback-loop trap: a DB outage must not read
/// as "no deviation."
#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("verification store query failed: {0}")]
    Query(String),
    #[error("verification store write failed: {0}")]
    Write(String),
}
