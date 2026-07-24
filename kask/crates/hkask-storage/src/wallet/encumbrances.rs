use super::WalletStore;
use crate::database::driver::query_row;
use crate::database::value::DbValue;
use hkask_types::time::now_rfc3339;
use hkask_types::{ApiKeyId, InfrastructureError, WalletId};
use hkask_types::{Encumbrance, EncumbranceStatus, RJoule, WalletError};
use std::str::FromStr;

// ── Encumbrance methods ────────────────────────────────────────────────────────

impl WalletStore {
    /// Lock rJoules from a wallet for an API key's use.
    ///
    /// Debits the wallet balance by `amount_rj` and creates an active
    /// encumbrance row. Returns an error if the key already has an active
    /// encumbrance or the wallet has insufficient balance.
    /// Encumber rJoules for an API key (lock funds for spending).
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — encumber rJoules for key
    /// pre:  wallet_id exists, key_id is valid, amount > 0, balance >= amount
    /// post: rJoules encumbered, balance decreased
    pub fn encumber_rjoules(
        &self,
        wallet_id: WalletId,
        key_id: ApiKeyId,
        amount_rj: RJoule,
    ) -> Result<(), WalletError> {
        let now = now_rfc3339();
        let amount = amount_rj.as_u64() as i64;
        // Check no existing active encumbrance for this key
        let existing: Option<String> = query_row(
            &*self.driver,
            "SELECT status FROM encumbrances WHERE key_id = ?1",
            &[DbValue::Text(key_id.to_string())],
            |row| Ok(row.get_str(0)?.to_string()),
        )?;
        if let Some(status) = existing
            && status == "active"
        {
            return Err(WalletError::EncumbranceAlreadyExists { key_id });
        }
        // Debit wallet
        let rows = self.driver.execute(
            "UPDATE wallet_balances SET balance_rj = balance_rj - ?1, updated_at = ?2 WHERE wallet_id = ?3 AND balance_rj >= ?1",
            &[
                DbValue::Integer(amount),
                DbValue::Text(now.clone()),
                DbValue::Text(wallet_id.to_string()),
            ],
        )?;
        if rows == 0 {
            let balance = self.get_balance(wallet_id)?;
            let have = balance.map(|b| b.rjoules).unwrap_or(0);
            return Err(WalletError::InsufficientBalance {
                have: RJoule::new(have),
                need: amount_rj,
            });
        }
        // Create encumbrance row
        self.driver.execute(
            "INSERT INTO encumbrances (key_id, wallet_id, amount_rj, consumed_rj, status, created_at) VALUES (?1, ?2, ?3, 0, 'active', ?4)",
            &[
                DbValue::Text(key_id.to_string()),
                DbValue::Text(wallet_id.to_string()),
                DbValue::Integer(amount),
                DbValue::Text(now),
            ],
        )?;
        Ok(())
    }

    /// Release an encumbrance, returning unspent rJoules to the wallet.
    ///
    /// Idempotent — releasing an already-released or consumed encumbrance
    /// is a no-op.
    /// Release an encumbrance (return unspent rJoules to wallet).
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — release encumbrance
    /// pre:  key_id has active encumbrance
    /// post: encumbrance released, unspent rJ returned to wallet
    pub fn release_encumbrance(&self, key_id: ApiKeyId) -> Result<(), WalletError> {
        let now = now_rfc3339();
        // Read current state
        let row: Option<(String, i64, i64)> = query_row(
            &*self.driver,
            "SELECT wallet_id, amount_rj, consumed_rj FROM encumbrances WHERE key_id = ?1 AND status = 'active'",
            &[DbValue::Text(key_id.to_string())],
            |row| {
                Ok((
                    row.get_str(0)?.to_string(),
                    row.get_int(1)?,
                    row.get_int(2)?,
                ))
            },
        )?;
        let (wallet_id_str, amount, consumed) = match row {
            Some(r) => r,
            None => return Ok(()), // already released/consumed or doesn't exist
        };
        // Mark released
        self.driver.execute(
            "UPDATE encumbrances SET status = 'released', released_at = ?1 WHERE key_id = ?2 AND status = 'active'",
            &[
                DbValue::Text(now.clone()),
                DbValue::Text(key_id.to_string()),
            ],
        )?;
        // Return unspent rJoules to wallet
        let unspent = amount - consumed;
        if unspent > 0 {
            self.driver.execute(
                "UPDATE wallet_balances SET balance_rj = balance_rj + ?1, updated_at = ?2 WHERE wallet_id = ?3",
                &[
                    DbValue::Integer(unspent),
                    DbValue::Text(now),
                    DbValue::Text(wallet_id_str),
                ],
            )?;
        }
        Ok(())
    }

    /// Atomically consume rJoules from an active encumbrance.
    ///
    /// This is a single SQL UPDATE that checks `amount_rj - consumed_rj >= cost`
    /// and deducts. No separate check+deduct pair — the operation is atomic.
    /// If the encumbrance is fully consumed, status transitions to 'consumed'.
    /// Consume from an encumbrance (spend locked rJoules).
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — consume from encumbrance
    /// pre:  key_id has active encumbrance with sufficient remaining
    /// post: consumed_rj increased, api_keys.spent_rj synced
    /// post: returns Err if insufficient or not active
    pub fn consume_encumbrance(
        &self,
        key_id: ApiKeyId,
        cost_rj: RJoule,
    ) -> Result<(), WalletError> {
        let cost = cost_rj.as_u64() as i64;
        // Atomic consume
        let rows = self.driver.execute(
            "UPDATE encumbrances SET consumed_rj = consumed_rj + ?1 WHERE key_id = ?2 AND status = 'active' AND (amount_rj - consumed_rj) >= ?1",
            &[
                DbValue::Integer(cost),
                DbValue::Text(key_id.to_string()),
            ],
        )?;
        if rows == 0 {
            return Self::diagnose_consume_failure(&*self.driver, key_id, cost_rj);
        }
        // Sync api_keys.spent_rj
        self.driver.execute(
            "UPDATE api_keys SET spent_rj = spent_rj + ?1 WHERE key_id = ?2",
            &[DbValue::Integer(cost), DbValue::Text(key_id.to_string())],
        )?;
        // Transition status if fully consumed
        let now = now_rfc3339();
        self.driver.execute(
            "UPDATE encumbrances SET status = 'consumed', released_at = ?1 WHERE key_id = ?2 AND status = 'active' AND consumed_rj >= amount_rj",
            &[
                DbValue::Text(now),
                DbValue::Text(key_id.to_string()),
            ],
        )?;
        Ok(())
    }

    fn diagnose_consume_failure(
        driver: &dyn crate::database::driver::DatabaseDriver,
        key_id: ApiKeyId,
        cost_rj: RJoule,
    ) -> Result<(), WalletError> {
        let enc_row: Option<(String, i64, i64, String)> = query_row(
            driver,
            "SELECT wallet_id, amount_rj, consumed_rj, status FROM encumbrances WHERE key_id = ?1",
            &[DbValue::Text(key_id.to_string())],
            |row| {
                Ok((
                    row.get_str(0)?.to_string(),
                    row.get_int(1)?,
                    row.get_int(2)?,
                    row.get_str(3)?.to_string(),
                ))
            },
        )?;
        match enc_row {
            Some((_wallet_id_str, amount, consumed, status_str)) => {
                let status = EncumbranceStatus::from_str(&status_str)
                    .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?;
                if status != EncumbranceStatus::Active {
                    return Err(WalletError::EncumbranceNotFound { key_id });
                }
                let remaining = (amount as u64).saturating_sub(consumed as u64);
                Err(WalletError::EncumbranceInsufficient {
                    key_id,
                    remaining: RJoule::new(remaining),
                    need: cost_rj,
                })
            }
            None => Err(WalletError::EncumbranceNotFound { key_id }),
        }
    }

    /// Get an encumbrance by key ID.
    /// Get an encumbrance by key ID.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — get encumbrance
    /// pre:  key_id is valid
    /// post: returns Some(Encumbrance) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn get_encumbrance(&self, key_id: ApiKeyId) -> Result<Option<Encumbrance>, WalletError> {
        let row: Option<(String, i64, i64, String, String, Option<String>)> = query_row(
            &*self.driver,
            "SELECT wallet_id, amount_rj, consumed_rj, status, created_at, released_at FROM encumbrances WHERE key_id = ?1",
            &[DbValue::Text(key_id.to_string())],
            |row| {
                Ok((
                    row.get_str(0)?.to_string(),
                    row.get_int(1)?,
                    row.get_int(2)?,
                    row.get_str(3)?.to_string(),
                    row.get_str(4)?.to_string(),
                    match row.get(5)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                ))
            },
        )?;
        match row {
            Some((wallet_id_str, amount, consumed, status_str, created_at, released_at)) => {
                let wallet_id = WalletId::from_str(&wallet_id_str).map_err(|e| {
                    WalletError::Infra(InfrastructureError::database(e.to_string()))
                })?;
                let status = EncumbranceStatus::from_str(&status_str)
                    .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?;
                Ok(Some(Encumbrance {
                    key_id,
                    wallet_id,
                    amount_rj: amount as u64,
                    consumed_rj: consumed as u64,
                    status,
                    created_at,
                    released_at,
                }))
            }
            None => Ok(None),
        }
    }
}
