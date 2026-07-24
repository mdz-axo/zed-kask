use super::WalletStore;
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use hkask_types::time::now_rfc3339;
use hkask_types::{ChainId, DepositAddress, DepositReference, PrivacyMode, WalletError};
use hkask_types::{InfrastructureError, WalletId};
use std::str::FromStr;

// ── Row type for query mapping ─────────────────────────────────────────────────

#[allow(dead_code)] // fields populated by query mapping
struct DepositAddressRow {
    chain: String,
    address: String,
    privacy_mode: String,
}

// ── Deposit Addresses ──────────────────────────────────────────────────────────

impl WalletStore {
    /// Store a derived deposit address for a wallet.
    /// Store a deposit address.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — store deposit address
    /// pre:  address has valid wallet_id and chain
    /// post: deposit address stored
    pub fn store_deposit_address(
        &self,
        wallet_id: WalletId,
        address: &str,
        index: u64,
        chain: ChainId,
        privacy_mode: PrivacyMode,
    ) -> Result<(), WalletError> {
        self.driver.execute(
            "INSERT OR IGNORE INTO deposit_addresses (wallet_id, chain, address, derivation_index, privacy_mode) VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                DbValue::Text(wallet_id.to_string()),
                DbValue::Text(chain.to_string()),
                DbValue::Text(address.to_string()),
                DbValue::Integer(index as i64),
                DbValue::Text(privacy_mode.to_string()),
            ],
        )?;
        Ok(())
    }

    /// Get all deposit addresses for a wallet.
    /// Get deposit addresses for a wallet.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — list deposit addresses
    /// pre:  wallet_id is valid
    /// post: returns Vec of deposit addresses
    pub fn get_deposit_addresses(
        &self,
        wallet_id: WalletId,
    ) -> Result<Vec<DepositAddress>, WalletError> {
        Ok(query_map(
            &*self.driver,
            "SELECT wallet_id, chain, address, derivation_index, privacy_mode FROM deposit_addresses WHERE wallet_id = ?1 ORDER BY derivation_index",
            &[DbValue::Text(wallet_id.to_string())],
            |row| {
                let chain = ChainId::from_str(row.get_str(1)?)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
                let privacy_mode = PrivacyMode::from_str(row.get_str(4)?)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
                Ok(DepositAddress {
                    address: row.get_str(2)?.to_string(),
                    chain,
                    privacy_mode,
                })
            },
        )?)
    }

    /// Resolve which wallet owns a deposit address (reverse lookup).
    ///
    /// Used by the deposit monitor to credit incoming transfers to the
    /// correct wallet in a multi-wallet setup.
    /// Resolve wallet for a deposit address.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — resolve wallet for address
    /// pre:  chain is valid, privacy_mode is valid, address is non-empty
    /// post: returns Some(WalletId) if found, None otherwise
    pub fn resolve_wallet_for_address(
        &self,
        address: &str,
        chain: ChainId,
        privacy_mode: PrivacyMode,
    ) -> Result<Option<WalletId>, WalletError> {
        let wallet_id_str: Option<String> = query_row(
            &*self.driver,
            "SELECT wallet_id FROM deposit_addresses WHERE address = ?1 AND chain = ?2 AND privacy_mode = ?3",
            &[
                DbValue::Text(address.to_string()),
                DbValue::Text(chain.to_string()),
                DbValue::Text(privacy_mode.to_string()),
            ],
            |row| Ok(row.get_str(0)?.to_string()),
        )?;
        match wallet_id_str {
            Some(s) => Ok(Some(WalletId::from_str(&s)?)),
            None => Ok(None),
        }
    }

    // ── Deposit References ─────────────────────────────────────────────────

    /// Store a one-time shielded deposit reference.
    /// Store a deposit reference for anti-replay.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — store deposit reference
    /// pre:  reference has valid fields
    /// post: deposit reference stored
    pub fn store_deposit_reference(&self, reference: &DepositReference) -> Result<(), WalletError> {
        self.driver.execute(
            "INSERT INTO deposit_references (reference, wallet_id, chain, expires_at) VALUES (?1, ?2, ?3, ?4)",
            &[
                DbValue::Text(reference.reference.clone()),
                DbValue::Text(reference.wallet_id.to_string()),
                DbValue::Text(reference.chain.to_string()),
                DbValue::Text(reference.expires_at.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Consume a deposit reference — atomically marks it spent and returns the wallet_id.
    /// Returns None if the reference doesn't exist, is already spent, or has expired.
    /// Consume a deposit reference (anti-replay).
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — consume deposit reference
    /// pre:  reference is valid and not expired
    /// post: reference consumed, wallet credited
    /// post: returns Err if already consumed or expired
    pub fn consume_deposit_reference(
        &self,
        reference: &str,
    ) -> Result<Option<WalletId>, WalletError> {
        let now = now_rfc3339();
        // Atomic check-and-set: only consume if not already spent and not expired
        let rows = self.driver.execute(
            "UPDATE deposit_references SET spent = 1 WHERE reference = ?1 AND spent = 0 AND expires_at > ?2",
            &[
                DbValue::Text(reference.to_string()),
                DbValue::Text(now),
            ],
        )?;
        if rows == 0 {
            return Ok(None); // not found, already spent, or expired
        }
        let wallet_id_str: String = query_row(
            &*self.driver,
            "SELECT wallet_id FROM deposit_references WHERE reference = ?1",
            &[DbValue::Text(reference.to_string())],
            |row| Ok(row.get_str(0)?.to_string()),
        )?
        .ok_or_else(|| WalletError::Infra(InfrastructureError::database("reference vanished")))?;
        Ok(Some(WalletId::from_str(&wallet_id_str)?))
    }

    /// Purge expired deposit references. Returns count of purged rows.
    /// Purge expired deposit references.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — purge expired references
    /// post: expired references deleted
    /// post: returns count of deleted references
    pub fn purge_expired_references(&self) -> Result<u64, WalletError> {
        let now = now_rfc3339();
        let rows = self.driver.execute(
            "DELETE FROM deposit_references WHERE expires_at <= ?1",
            &[DbValue::Text(now)],
        )?;
        Ok(rows as u64)
    }
}
