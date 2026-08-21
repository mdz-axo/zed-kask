//! rJoule cost tracking for corpus API calls and operations.
//!
//! 1 rJoule = 1 USD. The ledger stores amounts in µrJ (micro-rJoule), so
//! 1 USD = 1,000,000 µrJ. Every API call (embedding, classification, fetch)
//! posts a cost transaction to `cost:api/<provider>` so the corpus server
//! can find efficiencies and inefficiencies in LLM and data-service
//! interactions.
//!
//! The `cost:api/<provider>` accounts are the canonical per-provider cost
//! accounts: every API call posts its cost here, so both the call counts
//! and the USD totals are available to any consumer that reads the ledger
//! balances.
//!
//! Error handling follows the `.rules` "unwrap_or(0) on regulation sense
//! inputs is a broken feedback loop" trap: a failed cost post emits a
//! `tracing::warn!` naming the provider and the amount, never silently
//! dropping the error. A failed post does not block the calling operation
//! (the embedding/classification/fetch result is still returned to the
//! user) — the cost is best-effort, but the failure is visible.

use hkask_ledger::{Ledger, LedgerError, LedgerTransaction, Posting};
use hkask_storage::database::driver::DatabaseDriver;
use std::sync::Arc;

/// The asset symbol for rJoule costs in the ledger.
pub(crate) const COST_ASSET: &str = "urj";

/// The namespace for cost accounts (`cost:api/<provider>`).
pub(crate) const COST_NAMESPACE: &str = "cost";

/// Record a cost (in µrJ) against a provider's cost account.
///
/// Posts a double-entry transaction: `external` → `cost:api/<provider>`
/// with `amount` µrJ of asset `urj`. The `reference` must be unique (it
/// backs the ledger's idempotency check); callers mint a fresh UUID per
/// call. The `metadata` is stored verbatim on the transaction (e.g.
/// `{"operation":"classify","tokens":1234}`).
///
/// Best-effort: a `LedgerError` emits a `tracing::warn!` naming the
/// provider, amount, and error, then returns `Err`. The caller decides
/// whether to propagate; the common pattern is to log and continue (the
/// operation's result is already computed; the cost post is observability,
/// not a gate).
pub(crate) fn record_cost(
    driver: &Arc<dyn DatabaseDriver>,
    provider: &str,
    amount_urj: i64,
    reference: &str,
    metadata: &serde_json::Value,
) -> Result<(), LedgerError> {
    if amount_urj <= 0 {
        return Ok(());
    }

    let ledger = Ledger::from_driver(driver.clone())?;
    let account = format!("cost:api/{provider}");
    ledger.ensure_account(&account, COST_NAMESPACE)?;

    let tx = LedgerTransaction {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        reference: reference.to_string(),
        postings: vec![Posting {
            source: "external".to_string(),
            destination: account,
            asset: COST_ASSET.to_string(),
            amount: amount_urj,
        }],
        metadata: metadata.clone(),
    };
    ledger.commit(&tx)
}

/// Best-effort cost recording: posts the cost and warns on failure.
///
/// Use this when the calling operation has already succeeded and the cost
/// post is observability, not a gate. The `tracing::warn!` names the
/// provider, amount, and error so an operator reading logs can see the
/// failed post (the `.rules` "advertised invariants need enforcement
/// points" trap: a silent `unwrap_or(())` would hide the failure).
pub(crate) fn record_cost_best_effort(
    driver: &Arc<dyn DatabaseDriver>,
    provider: &str,
    amount_urj: i64,
    reference: &str,
    metadata: &serde_json::Value,
) {
    if let Err(e) = record_cost(driver, provider, amount_urj, reference, metadata) {
        tracing::warn!(
            target: "hkask.corpus.cost",
            provider = %provider,
            amount_urj = amount_urj,
            error = %e,
            "Failed to post rJoule cost to ledger — cost tracking gap (the operation succeeded; this is observability, not a gate)"
        );
    }
}
