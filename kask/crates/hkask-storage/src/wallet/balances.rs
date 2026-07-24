use super::WalletStore;
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use hkask_types::time::now_rfc3339;
use hkask_types::{InfrastructureError, WalletId};
use hkask_types::{RJoule, TransactionType, WalletBalance, WalletError, WalletTransaction};
use std::str::FromStr;

// ── Balance & wallet lifecycle ─────────────────────────────────────────────────

impl WalletStore {
    /// Get wallet balance.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — get wallet balance
    /// pre:  wallet_id is valid
    /// post: returns Some(WalletBalance) if wallet exists, None otherwise
    #[must_use = "result must be used"]
    pub fn get_balance(&self, wallet_id: WalletId) -> Result<Option<WalletBalance>, WalletError> {
        let rows: Vec<WalletBalance> = query_map(
            &*self.driver,
            "SELECT wallet_id, balance_rj, usdc_equivalent_micro FROM wallet_balances WHERE wallet_id = ?1",
            &[DbValue::Text(wallet_id.to_string())],
            |row| {
                Ok(WalletBalance {
                    wallet_id: WalletId::from_str(row.get_str(0)?)
                        .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?,
                    rjoules: row.get_int(1)? as u64,
                    usdc_equivalent_micro: row.get_int(2)? as u64,
                    gas_equivalent: 0, // computed by caller with config
                })
            },
        )?;
        Ok(rows.into_iter().next())
    }

    /// Ensure a wallet row exists (idempotent — creates if missing).
    fn ensure_wallet_inner(&self, wallet_id: WalletId) -> Result<(), WalletError> {
        self.driver.execute(
            "INSERT OR IGNORE INTO wallet_balances (wallet_id) VALUES (?1)",
            &[DbValue::Text(wallet_id.to_string())],
        )?;
        Ok(())
    }

    /// Ensure a wallet exists (idempotent).
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — idempotently ensure wallet row
    /// pre:  wallet_id is valid
    /// post: wallet row exists (created if missing)
    pub fn ensure_wallet(&self, wallet_id: WalletId) -> Result<(), WalletError> {
        self.ensure_wallet_inner(wallet_id)
    }

    /// List all wallet IDs.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P8\] Motivating: Semantic Grounding — list wallet IDs
    /// post: returns Vec of all WalletId
    pub fn list_wallet_ids(&self) -> Result<Vec<WalletId>, WalletError> {
        let rows: Vec<String> = query_map(
            &*self.driver,
            "SELECT wallet_id FROM wallet_balances",
            &[],
            |row| Ok(row.get_str(0)?.to_string()),
        )?;
        rows.into_iter()
            .map(|s| WalletId::from_str(&s))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WalletError::Infra(InfrastructureError::database(e.to_string())))
    }

    /// Credit rJoules to a wallet. Records the transaction atomically.
    /// Creates the wallet row if it doesn't exist.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — credit rJoules
    /// pre:  wallet_id exists, amount > 0
    /// post: balance increased by amount, transaction recorded
    #[must_use = "result must be used"]
    pub fn credit_rjoules(
        &self,
        wallet_id: WalletId,
        amount: RJoule,
        tx_type: TransactionType,
    ) -> Result<WalletBalance, WalletError> {
        self.ensure_wallet_inner(wallet_id)?;
        let amount_u64 = amount.as_u64();
        let amount_i64 = i64::try_from(amount_u64).map_err(|_| {
            WalletError::Infra(InfrastructureError::database(
                "credit amount exceeds i64::MAX",
            ))
        })?;
        let current: i64 = query_row(
            &*self.driver,
            "SELECT balance_rj FROM wallet_balances WHERE wallet_id = ?1",
            &[DbValue::Text(wallet_id.to_string())],
            |row| row.get_int(0),
        )?
        .unwrap_or(0);
        let new_balance = current.checked_add(amount_i64).ok_or_else(|| {
            WalletError::Infra(InfrastructureError::database("balance overflow on credit"))
        })?;
        let now = now_rfc3339();
        self.driver.execute(
            "UPDATE wallet_balances SET balance_rj = ?1, updated_at = ?2 WHERE wallet_id = ?3",
            &[
                DbValue::Integer(new_balance),
                DbValue::Text(now.clone()),
                DbValue::Text(wallet_id.to_string()),
            ],
        )?;
        // Record the transaction atomically
        self.record_transaction_inner(&WalletTransaction {
            id: 0,
            wallet_id,
            tx_type,
            rjoules_delta: amount_i64,
            balance_after: new_balance as u64,
            timestamp: chrono::Utc::now(),
        })?;
        self.get_balance(wallet_id)?
            .ok_or(WalletError::Infra(InfrastructureError::database(
                "wallet vanished after credit",
            )))
    }

    /// Debit rJoules from a wallet. Records the transaction atomically.
    /// Returns error if balance insufficient.
    ///
    /// **Idempotency:** This operation is NOT idempotent.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — debit rJoules
    /// pre:  wallet_id exists, amount > 0, balance >= amount
    /// post: balance decreased by amount, transaction recorded
    /// post: returns Err if insufficient balance
    #[must_use = "result must be used"]
    pub fn debit_rjoules(
        &self,
        wallet_id: WalletId,
        amount: RJoule,
        tx_type: TransactionType,
    ) -> Result<WalletBalance, WalletError> {
        let current: i64 = query_row(
            &*self.driver,
            "SELECT balance_rj FROM wallet_balances WHERE wallet_id = ?1",
            &[DbValue::Text(wallet_id.to_string())],
            |row| row.get_int(0),
        )?
        .unwrap_or(0);
        let amount_i64 = i64::try_from(amount.as_u64()).map_err(|_| {
            WalletError::Infra(InfrastructureError::database(
                "debit amount exceeds i64::MAX",
            ))
        })?;
        if current < amount_i64 {
            return Err(WalletError::InsufficientBalance {
                have: RJoule::new(current as u64),
                need: amount,
            });
        }
        let new_balance = current - amount_i64;
        let now = now_rfc3339();
        self.driver.execute(
            "UPDATE wallet_balances SET balance_rj = balance_rj - ?1, updated_at = ?2 WHERE wallet_id = ?3",
            &[
                DbValue::Integer(amount_i64),
                DbValue::Text(now.clone()),
                DbValue::Text(wallet_id.to_string()),
            ],
        )?;
        // Record the transaction atomically
        self.record_transaction_inner(&WalletTransaction {
            id: 0,
            wallet_id,
            tx_type,
            rjoules_delta: -amount_i64,
            balance_after: new_balance as u64,
            timestamp: chrono::Utc::now(),
        })?;
        self.get_balance(wallet_id)?
            .ok_or(WalletError::Infra(InfrastructureError::database(
                "wallet vanished after debit",
            )))
    }
}
