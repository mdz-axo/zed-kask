//! TransactionHandle — RAII transaction guard.
//!
//! Begin via `driver.transaction()`, commit with `handle.commit()`.
//! On drop without commit, rolls back automatically.

use super::driver::DatabaseDriver;
use super::types::DbError;

/// RAII transaction guard. Auto-rollbacks on drop if not committed.
///
/// # Example
///
/// ```ignore
/// let tx = driver.transaction()?;
/// driver.execute("INSERT INTO t VALUES (?)", &[DbValue::Integer(42)])?;
/// tx.commit()?;  // consume the handle, commit
/// // If tx is dropped without commit, rollback fires
/// ```
pub struct TransactionHandle<'a> {
    driver: &'a dyn DatabaseDriver,
    committed: bool,
}

impl<'a> TransactionHandle<'a> {
    pub(crate) fn new(driver: &'a dyn DatabaseDriver) -> Self {
        Self {
            driver,
            committed: false,
        }
    }

    /// Commit the transaction, consuming the handle.
    ///
    /// After commit, the handle is consumed — no rollback occurs on drop.
    pub fn commit(mut self) -> Result<(), DbError> {
        self.committed = true;
        // Delegate to the driver's commit implementation
        self.driver.commit_tx()
    }
}

impl<'a> Drop for TransactionHandle<'a> {
    fn drop(&mut self) {
        if !self.committed
            && let Err(e) = self.driver.rollback_tx()
        {
            tracing::error!(
                target: "hkask.database",
                error = %e,
                "Transaction rollback failed — pool connection may be in an invalid state"
            );
        }
    }
}
