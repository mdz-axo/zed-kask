use super::WalletStore;
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use hkask_types::{ApiKeyId, InfrastructureError, WalletId};
use hkask_types::{ChainId, PrivacyMode, RJoule, TransactionType, WalletError, WalletTransaction};
use std::str::FromStr;

// ── Row type for query mapping ─────────────────────────────────────────────────

struct WalletTransactionRow {
    id: i64,
    wallet_id: String,
    tx_type: String,
    tx_subtype: Option<String>,
    chain: Option<String>,
    on_chain_tx_hash: Option<String>,
    amount_rj: i64,
    balance_after_rj: i64,
    key_id: Option<String>,
    tool_name: Option<String>,
    gas_units: Option<i64>,
    created_at: String,
}

// ── Transaction methods ────────────────────────────────────────────────────────

impl WalletStore {
    /// Record a transaction in the append-only ledger.
    /// Record a wallet transaction.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — record wallet transaction
    /// pre:  tx has valid wallet_id and rjoules_delta
    /// post: transaction inserted into ledger
    pub fn record_transaction(&self, tx: &WalletTransaction) -> Result<(), WalletError> {
        self.record_transaction_inner(tx)
    }

    /// Record a transaction using the store's driver.
    /// Called by `credit_rjoules` and `debit_rjoules` for atomicity.
    pub(crate) fn record_transaction_inner(
        &self,
        tx: &WalletTransaction,
    ) -> Result<(), WalletError> {
        let (tx_type_str, tx_subtype, chain, tx_hash, key_id, tool_name, gas_units) =
            tx_type_to_columns(&tx.tx_type);
        self.driver.execute(
            "INSERT INTO wallet_transactions (wallet_id, tx_type, tx_subtype, chain, on_chain_tx_hash, amount_rj, balance_after_rj, key_id, tool_name, gas_units) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            &[
                DbValue::Text(tx.wallet_id.to_string()),
                DbValue::Text(tx_type_str.to_string()),
                tx_subtype.map_or(DbValue::Null, DbValue::Text),
                chain.map_or(DbValue::Null, DbValue::Text),
                tx_hash.map_or(DbValue::Null, DbValue::Text),
                DbValue::Integer(tx.rjoules_delta),
                DbValue::Integer(tx.balance_after as i64),
                key_id.map_or(DbValue::Null, DbValue::Text),
                tool_name.map_or(DbValue::Null, DbValue::Text),
                gas_units.map_or(DbValue::Null, DbValue::Integer),
            ],
        )?;
        Ok(())
    }

    /// Get paginated transaction history for a wallet.
    /// Get transactions for a wallet.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — list transactions
    /// pre:  wallet_id is valid
    /// post: returns Vec of transactions, optionally limited
    #[must_use = "result must be used"]
    pub fn get_transactions(
        &self,
        wallet_id: WalletId,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<WalletTransaction>, WalletError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, wallet_id, tx_type, tx_subtype, chain, on_chain_tx_hash, amount_rj, balance_after_rj, key_id, tool_name, gas_units, created_at FROM wallet_transactions WHERE wallet_id = ?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            &[
                DbValue::Text(wallet_id.to_string()),
                DbValue::Integer(limit as i64),
                DbValue::Integer(offset as i64),
            ],
            |row| {
                let r = WalletTransactionRow {
                    id: row.get_int(0)?,
                    wallet_id: row.get_str(1)?.to_string(),
                    tx_type: row.get_str(2)?.to_string(),
                    tx_subtype: match row.get(3)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    chain: match row.get(4)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    on_chain_tx_hash: match row.get(5)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    amount_rj: row.get_int(6)?,
                    balance_after_rj: row.get_int(7)?,
                    key_id: match row.get(8)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    tool_name: match row.get(9)? {
                        DbValue::Null => None,
                        v => Some(v.as_text()?.to_string()),
                    },
                    gas_units: match row.get(10)? {
                        DbValue::Null => None,
                        v => Some(v.as_int()?),
                    },
                    created_at: row.get_str(11)?.to_string(),
                };
                row_to_wallet_transaction(r)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))
            },
        )?)
    }

    /// Check if a transaction with the given on-chain tx_hash already exists.
    /// Used for deposit idempotency — prevents double-crediting on restart.
    /// Check if a transaction hash exists.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P4\] Motivating: Clear Boundaries — anti-replay hash check
    /// pre:  tx_hash is non-empty
    /// post: returns true if hash exists (anti-replay)
    #[must_use = "result must be used"]
    pub fn transaction_exists_by_hash(&self, tx_hash: &str) -> Result<bool, WalletError> {
        let count = query_row(
            &*self.driver,
            "SELECT COUNT(*) FROM wallet_transactions WHERE on_chain_tx_hash = ?1",
            &[DbValue::Text(tx_hash.to_string())],
            |row| row.get_int(0),
        )?
        .unwrap_or(0);
        Ok(count > 0)
    }
}

// ── Row conversion helpers ─────────────────────────────────────────────────────

type TxTypeColumns = (
    &'static str,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

fn tx_type_to_columns(tx_type: &TransactionType) -> TxTypeColumns {
    match tx_type {
        TransactionType::Deposit { tx_hash, .. } => (
            "deposit",
            Some("transparent".to_string()),
            Some("hedera".to_string()),
            Some(tx_hash.clone()),
            None,
            None,
            None,
        ),
        TransactionType::Withdrawal { tx_hash, .. } => (
            "withdrawal",
            Some("transparent".to_string()),
            Some("hedera".to_string()),
            Some(tx_hash.clone()),
            None,
            None,
            None,
        ),
        TransactionType::Spend {
            key_id, tool, gas, ..
        } => (
            "spend",
            None,
            None,
            None,
            Some(key_id.to_string()),
            Some(tool.clone()),
            Some(*gas as i64),
        ),
        TransactionType::Refund { key_id, reason, .. } => (
            "refund",
            None,
            None,
            None,
            Some(key_id.to_string()),
            Some(reason.clone()),
            None,
        ),
        TransactionType::Shield { chain, tx_hash, .. } => (
            "shield",
            None,
            Some(chain.to_string()),
            Some(tx_hash.clone()),
            None,
            None,
            None,
        ),
    }
}

fn row_to_wallet_transaction(r: WalletTransactionRow) -> Result<WalletTransaction, WalletError> {
    let tx_type = match r.tx_type.as_str() {
        "deposit" => TransactionType::Deposit {
            chain: ChainId::from_str(r.chain.as_deref().unwrap_or("hedera"))
                .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?,
            privacy: PrivacyMode::from_str(r.tx_subtype.as_deref().unwrap_or("transparent"))
                .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?,
            tx_hash: r.on_chain_tx_hash.unwrap_or_default(),
            amount_usdc_micro: 0,
        },
        "withdrawal" => TransactionType::Withdrawal {
            chain: ChainId::from_str(r.chain.as_deref().unwrap_or("hedera"))
                .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?,
            privacy: PrivacyMode::from_str(r.tx_subtype.as_deref().unwrap_or("transparent"))
                .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?,
            tx_hash: r.on_chain_tx_hash.unwrap_or_default(),
            amount_usdc_micro: 0,
        },
        "spend" => TransactionType::Spend {
            key_id: ApiKeyId::from_str(r.key_id.as_deref().unwrap_or(""))
                .map_err(|e| WalletError::Infra(InfrastructureError::database(e.to_string())))?,
            tool: r.tool_name.unwrap_or_default(),
            gas: r.gas_units.unwrap_or(0) as u64,
            rj: RJoule::new(r.amount_rj.unsigned_abs()),
        },
        "refund" => TransactionType::Refund {
            key_id: ApiKeyId::from_str(r.key_id.as_deref().unwrap_or(""))
                .map_err(|e| WalletError::Infra(InfrastructureError::database(e.to_string())))?,
            reason: r.tool_name.unwrap_or_default(),
            rj: RJoule::new(r.amount_rj.unsigned_abs()),
        },
        "shield" => TransactionType::Shield {
            chain: ChainId::from_str(r.chain.as_deref().unwrap_or("hedera"))
                .map_err(|e| WalletError::Infra(InfrastructureError::database(e)))?,
            tx_hash: r.on_chain_tx_hash.unwrap_or_default(),
            amount_usdc_micro: 0,
        },
        other => {
            return Err(WalletError::Infra(InfrastructureError::database(format!(
                "unknown tx_type: {other}"
            ))));
        }
    };
    Ok(WalletTransaction {
        id: r.id as u64,
        wallet_id: WalletId::from_str(&r.wallet_id)?,
        tx_type,
        rjoules_delta: r.amount_rj,
        balance_after: r.balance_after_rj as u64,
        timestamp: chrono::NaiveDateTime::parse_from_str(&r.created_at, "%Y-%m-%d %H:%M:%S")
            .map(|dt| dt.and_utc())
            .map_err(|e| WalletError::Infra(InfrastructureError::database(e.to_string())))?,
    })
}
