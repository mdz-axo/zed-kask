#![forbid(unsafe_code)]
//! # hKask Ledger — Double-Entry Accounting
//!
//! Immutable double-entry ledger backed by the provider-agnostic `DatabaseDriver`
//! abstraction (ADR-043). Three domain ledgers (cost, crypto, securities) share
//! this crate with separate database files.
//!
//! ## Invariants
//!
//! 1. **Idempotency** — same `reference` with identical postings is a no-op.
//!    Different postings with the same reference return `IdempotencyConflict`.
//! 2. **Double-entry** — every transaction's postings must sum to 0.
//! 3. **Immutability** — committed transactions are never modified or deleted.
//!    DO NOT use INSERT OR REPLACE on the transactions table — it would
//!    cascade-delete postings and retroactively change account balances.

mod schema;
mod types;

pub use types::{AccountBalance, DateRange, LedgerError, LedgerTransaction, Posting, QueryFilter};

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::value::DbValue;
use std::sync::Arc;

/// The double-entry ledger.
///
/// Backed by a provider-agnostic `DatabaseDriver` (ADR-043). The driver
/// abstracts over SQLite and PostgreSQL, so the ledger works with either
/// provider. `Ledger` is `Send + Sync` and can be shared via `Arc`.
pub struct Ledger {
    driver: Arc<dyn DatabaseDriver>,
}

impl Ledger {
    /// Create a ledger from a `DatabaseDriver` — the provider-agnostic constructor.
    ///
    /// This is the canonical constructor per ADR-043. All stores should
    /// construct via `from_driver`. The driver handles connection pooling,
    /// provider dispatch, and transaction management.
    ///
    /// REQ: P8-ledger-from-driver
    /// expect: "I can create a ledger from any DatabaseDriver and it initializes the schema" \[P8\]
    /// pre:  driver is connected and ready for queries
    /// post: returns Ledger with accounts, transactions, postings tables created
    /// inv:  idempotent — calling from_driver twice with the same driver creates the same tables
    /// \[P8\] Constraining: Persistence — data survives process restarts
    pub fn from_driver(driver: Arc<dyn DatabaseDriver>) -> Result<Self, LedgerError> {
        schema::init_schema(&driver)?;
        Ok(Self { driver })
    }

    /// REQ: P8-ledger-ensure-account
    /// expect: "I can create a named account and doing it twice is harmless" \[P8\]
    /// pre:  id and namespace are non-empty strings
    /// post: account exists in the database; second call with same id is a no-op
    /// inv:  idempotent — calling ensure_account twice with same id does not error
    /// \[P8\] Constraining: Persistence — account survives restarts
    pub fn ensure_account(&self, id: &str, namespace: &str) -> Result<(), LedgerError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.driver.execute(
            "INSERT OR IGNORE INTO accounts (id, namespace, created_at) VALUES (?1, ?2, ?3)",
            &[
                DbValue::Text(id.to_string()),
                DbValue::Text(namespace.to_string()),
                DbValue::Text(now),
            ],
        )?;
        Ok(())
    }

    /// REQ: P8-ledger-commit
    /// expect: "I can commit a transaction and the postings are stored immutably" \[P8\]
    /// pre:  tx.id is unique, tx.reference is unique, tx.postings is non-empty
    /// post: transaction and all postings are stored; balances reflect new postings
    /// inv:  idempotent by reference — identical postings succeed silently;
    ///       different postings with same reference return IdempotencyConflict
    /// \[P4\] Constraining: Clear Boundaries — committed transactions cannot be modified
    /// \[P8\] Constraining: Persistence — committed data survives restarts
    pub fn commit(&self, tx: &LedgerTransaction) -> Result<(), LedgerError> {
        if tx.postings.is_empty() {
            return Err(LedgerError::DoubleEntryViolation(0));
        }

        let now = chrono::Utc::now().to_rfc3339();

        // Check if this reference already exists
        let existing = self.driver.query_optional(
            "SELECT id FROM transactions WHERE reference = ?1",
            &[DbValue::Text(tx.reference.clone())],
        )?;

        if let Some(row) = existing {
            let existing_id = row.get_str(0)?.to_string();
            // Reference exists — verify the postings match for true idempotency
            let existing_postings = self.driver.query(
                "SELECT source, destination, asset, amount
                 FROM postings WHERE transaction_id = ?1 ORDER BY id",
                &[DbValue::Text(existing_id)],
            )?;

            if existing_postings.len() != tx.postings.len() {
                return Err(LedgerError::IdempotencyConflict {
                    reference: tx.reference.clone(),
                });
            }
            for (i, p) in tx.postings.iter().enumerate() {
                let row = &existing_postings[i];
                let src = row.get_str(0)?;
                let dst = row.get_str(1)?;
                let ast = row.get_str(2)?;
                let amt = row.get_int(3)?;
                if p.source != src || p.destination != dst || p.asset != ast || p.amount != amt {
                    return Err(LedgerError::IdempotencyConflict {
                        reference: tx.reference.clone(),
                    });
                }
            }
            // Postings match — true idempotent, no-op
            return Ok(());
        }

        // Wrap in transaction for atomicity
        self.driver.execute_batch("BEGIN IMMEDIATE")?;

        self.driver.execute(
            "INSERT INTO transactions (id, timestamp, reference, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                DbValue::Text(tx.id.clone()),
                DbValue::Text(tx.timestamp.clone()),
                DbValue::Text(tx.reference.clone()),
                DbValue::Text(tx.metadata.to_string()),
                DbValue::Text(now.clone()),
            ],
        )?;

        for posting in &tx.postings {
            self.driver.execute(
                "INSERT INTO postings (transaction_id, source, destination, asset, amount, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                &[
                    DbValue::Text(tx.id.clone()),
                    DbValue::Text(posting.source.clone()),
                    DbValue::Text(posting.destination.clone()),
                    DbValue::Text(posting.asset.clone()),
                    DbValue::Integer(posting.amount),
                    DbValue::Text(now.clone()),
                ],
            )?;
        }
        self.driver.execute_batch("COMMIT")?;

        Ok(())
    }

    /// REQ: P9-ledger-balance
    /// expect: "I can query the balance of any account and see all credits minus debits" \[P9\]
    /// pre:  account is a valid account ID (may or may not exist)
    /// post: returns sum(destination amounts) - sum(source amounts) for matching asset
    /// inv:  read-only — does not modify the ledger; non-existent account returns 0
    /// \[P9\] Constraining: Observability — balances are visible to the user
    pub fn balance(&self, account: &str, asset: Option<&str>) -> Result<i64, LedgerError> {
        let (sql, params) = if let Some(a) = asset {
            (
                "SELECT
                        COALESCE(SUM(CASE WHEN destination = ?1 THEN amount ELSE 0 END), 0)
                      - COALESCE(SUM(CASE WHEN source = ?1 THEN amount ELSE 0 END), 0)
                     FROM postings WHERE (source = ?1 OR destination = ?1) AND asset = ?2",
                vec![
                    DbValue::Text(account.to_string()),
                    DbValue::Text(a.to_string()),
                ],
            )
        } else {
            (
                "SELECT
                        COALESCE(SUM(CASE WHEN destination = ?1 THEN amount ELSE 0 END), 0)
                      - COALESCE(SUM(CASE WHEN source = ?1 THEN amount ELSE 0 END), 0)
                     FROM postings WHERE source = ?1 OR destination = ?1",
                vec![DbValue::Text(account.to_string())],
            )
        };
        let row = self.driver.query_optional(sql, &params)?;
        let balance = row.map(|r| r.get_int(0).unwrap_or(0)).unwrap_or(0);
        Ok(balance)
    }

    /// REQ: P9-ledger-namespace-balances
    /// expect: "I can see all balances in a domain (cost, wallet, portfolio) at once" \[P9\]
    /// pre:  namespace is a valid namespace string
    /// post: returns all (account, asset, balance) h_mems for accounts in the namespace
    /// inv:  read-only; returns empty vec for unknown namespace
    /// \[P9\] Constraining: Observability — all domain balances are visible at once
    pub fn namespace_balances(&self, namespace: &str) -> Result<Vec<AccountBalance>, LedgerError> {
        let rows = self.driver.query(
            "SELECT a.id,
                    COALESCE(p.asset, '') AS asset,
                    COALESCE(SUM(CASE WHEN p.destination = a.id THEN p.amount ELSE 0 END), 0)
                  - COALESCE(SUM(CASE WHEN p.source = a.id THEN p.amount ELSE 0 END), 0) AS balance
             FROM accounts a
             LEFT JOIN postings p ON (p.source = a.id OR p.destination = a.id)
             WHERE a.namespace = ?1
             GROUP BY a.id, p.asset
             ORDER BY a.id, p.asset",
            &[DbValue::Text(namespace.to_string())],
        )?;

        let mut balances = Vec::new();
        for row in rows {
            balances.push(AccountBalance {
                account: row.get_str(0)?.to_string(),
                asset: row.get_str(1)?.to_string(),
                balance: row.get(2)?.as_int().unwrap_or(0),
            });
        }
        Ok(balances)
    }

    /// REQ: P9-ledger-transaction-count
    /// expect: "I can count how many transactions reference a specific account" \[P9\]
    /// pre:  destination is a valid account ID
    /// post: returns count of unique transactions with a posting to that account
    /// inv:  read-only
    /// \[P9\] Constraining: Observability — transaction volume is queryable
    pub fn transaction_count(&self, destination: &str) -> Result<u64, LedgerError> {
        let row = self.driver.query_optional(
            "SELECT COUNT(DISTINCT transaction_id) FROM postings WHERE destination = ?1",
            &[DbValue::Text(destination.to_string())],
        )?;
        let count = row.map(|r| r.get_int(0).unwrap_or(0)).unwrap_or(0);
        Ok(u64::try_from(count).unwrap_or_else(|_| {
            tracing::warn!(target: "ledger", count, "Negative transaction count from database — clamping to 0");
            0
        }))
    }

    /// REQ: P9-ledger-query
    /// expect: "I can query transactions by time range and filter by account or asset" \[P9\]
    /// pre:  range.start <= range.end (ISO 8601 strings)
    /// post: returns all transactions whose timestamp falls within the range,
    ///       filtered by optional account/asset/namespace criteria
    /// inv:  read-only; returns empty vec if no matches
    /// \[P9\] Constraining: Observability — transaction history is queryable
    pub fn query(
        &self,
        range: &DateRange,
        filter: &QueryFilter,
    ) -> Result<Vec<LedgerTransaction>, LedgerError> {
        // Build query with parameterized conditions
        let mut conditions = vec!["t.timestamp >= ?1 AND t.timestamp <= ?2".to_string()];
        if filter.account.is_some() {
            conditions.push("(p.source = ?3 OR p.destination = ?3)".to_string());
        }
        if filter.asset.is_some() {
            let idx = if filter.account.is_some() { 4 } else { 3 };
            conditions.push(format!("p.asset = ?{idx}"));
        }
        if filter.namespace.is_some() {
            let idx = 3 + filter.account.is_some() as usize + filter.asset.is_some() as usize;
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM accounts a WHERE (a.id = p.source OR a.id = p.destination) AND a.namespace = ?{idx})"
            ));
        }

        let sql = format!(
            "SELECT DISTINCT t.id, t.timestamp, t.reference, t.metadata, t.created_at
             FROM transactions t
             JOIN postings p ON p.transaction_id = t.id
             WHERE {}
             ORDER BY t.timestamp, t.id",
            conditions.join(" AND ")
        );

        let params = build_query_params(range, filter);
        let tx_rows = self.driver.query(&sql, &params)?;

        // For each transaction, load its postings
        let mut result = Vec::new();
        for row in tx_rows {
            let id = row.get_str(0)?.to_string();
            let timestamp = row.get_str(1)?.to_string();
            let reference = row.get_str(2)?.to_string();
            let metadata_str = row.get_str(3)?.to_string();

            let postings = self.driver.query(
                "SELECT source, destination, asset, amount
                 FROM postings WHERE transaction_id = ?1 ORDER BY id",
                &[DbValue::Text(id.clone())],
            )?;

            let postings: Vec<Posting> = postings
                .iter()
                .map(|p| Posting {
                    source: p.get_str(0).unwrap_or("").to_string(),
                    destination: p.get_str(1).unwrap_or("").to_string(),
                    asset: p.get_str(2).unwrap_or("").to_string(),
                    amount: p.get_int(3).unwrap_or(0),
                })
                .collect();

            let tx_id = id.clone();
            result.push(LedgerTransaction {
                id,
                timestamp,
                reference,
                postings,
                metadata: serde_json::from_str(&metadata_str).unwrap_or_else(|e| {
                    tracing::warn!(target: "ledger", error = %e, transaction_id = %tx_id, "Corrupted metadata JSON");
                    serde_json::Value::Null
                }),
            });
        }

        Ok(result)
    }
}

/// Build parameter list for the query() method.
fn build_query_params(range: &DateRange, filter: &QueryFilter) -> Vec<DbValue> {
    let mut params: Vec<DbValue> = vec![
        DbValue::Text(range.start.clone()),
        DbValue::Text(range.end.clone()),
    ];
    if let Some(ref account) = filter.account {
        params.push(DbValue::Text(account.clone()));
    }
    if let Some(ref asset) = filter.asset {
        params.push(DbValue::Text(asset.clone()));
    }
    if let Some(ref ns) = filter.namespace {
        params.push(DbValue::Text(ns.clone()));
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;

    fn db() -> Ledger {
        Ledger::from_driver(SqliteDriver::in_memory_driver()).expect("ledger from driver")
    }

    #[test]
    fn ledger_open_creates_schema() {
        let ledger = db();
        // Verify tables exist by inserting and querying
        ledger.ensure_account("test:asset", "test").unwrap();
        let bal = ledger.balance("test:asset", None).unwrap();
        assert_eq!(bal, 0);
    }

    #[test]
    fn ledger_open_is_idempotent() {
        let driver = SqliteDriver::in_memory_driver();
        let _l1 = Ledger::from_driver(driver.clone()).unwrap();
        let _l2 = Ledger::from_driver(driver).unwrap();
        // No error — schema creation is idempotent
    }

    #[test]
    fn ensure_account_creates() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        let bal = ledger.balance("alice", None).unwrap();
        assert_eq!(bal, 0);
    }

    #[test]
    fn ensure_account_is_idempotent() {
        let ledger = db();
        ledger.ensure_account("bob", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();
        // No error — second call is a no-op
    }

    fn sample_tx(reference: &str) -> LedgerTransaction {
        LedgerTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reference: reference.to_string(),
            postings: vec![
                Posting {
                    source: "alice".to_string(),
                    destination: "bob".to_string(),
                    asset: "usd".to_string(),
                    amount: 100,
                },
                Posting {
                    source: "bob".to_string(),
                    destination: "alice".to_string(),
                    asset: "usd".to_string(),
                    amount: -100,
                },
            ],
            metadata: serde_json::json!({"reason": "test"}),
        }
    }

    #[test]
    fn commit_stores_transaction() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();
        let tx = sample_tx("tx-001");
        ledger.commit(&tx).unwrap();

        let bal = ledger.balance("bob", Some("usd")).unwrap();
        assert_eq!(bal, 200); // +100 from alice, -(-100) from alice's second posting
    }

    #[test]
    fn commit_is_idempotent() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();
        let tx = sample_tx("tx-002");
        ledger.commit(&tx).unwrap();
        // Second commit with same reference + same postings — no-op
        ledger.commit(&tx).unwrap();
        let bal = ledger.balance("bob", Some("usd")).unwrap();
        assert_eq!(bal, 200); // unchanged
    }

    #[test]
    fn commit_rejects_idempotency_conflict() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();
        let tx1 = sample_tx("tx-003");
        ledger.commit(&tx1).unwrap();

        let mut tx2 = sample_tx("tx-003"); // same reference
        tx2.postings[0].amount = 999; // different postings
        let result = ledger.commit(&tx2);
        assert!(matches!(
            result,
            Err(LedgerError::IdempotencyConflict { .. })
        ));
    }

    #[test]
    fn commit_rejects_empty_postings() {
        let ledger = db();
        let tx = LedgerTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reference: "tx-empty".to_string(),
            postings: vec![],
            metadata: serde_json::json!({}),
        };
        let result = ledger.commit(&tx);
        assert!(matches!(result, Err(LedgerError::DoubleEntryViolation(0))));
    }

    #[test]
    fn balance_after_commit() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();

        let tx = LedgerTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reference: "tx-bal-1".to_string(),
            postings: vec![
                Posting {
                    source: "alice".to_string(),
                    destination: "bob".to_string(),
                    asset: "usd".to_string(),
                    amount: 500,
                },
                Posting {
                    source: "bob".to_string(),
                    destination: "alice".to_string(),
                    asset: "usd".to_string(),
                    amount: -500,
                },
            ],
            metadata: serde_json::json!({}),
        };
        ledger.commit(&tx).unwrap();

        assert_eq!(ledger.balance("bob", Some("usd")).unwrap(), 1000);
        assert_eq!(ledger.balance("alice", Some("usd")).unwrap(), -1000);
    }

    #[test]
    fn balance_nonexistent_returns_zero() {
        let ledger = db();
        assert_eq!(ledger.balance("nobody", None).unwrap(), 0);
    }

    #[test]
    fn balances_sum_to_zero() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();
        ledger.ensure_account("carol", "wallet").unwrap();

        let tx = LedgerTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reference: "tx-sum-1".to_string(),
            postings: vec![
                Posting {
                    source: "alice".to_string(),
                    destination: "bob".to_string(),
                    asset: "usd".to_string(),
                    amount: 300,
                },
                Posting {
                    source: "bob".to_string(),
                    destination: "carol".to_string(),
                    asset: "usd".to_string(),
                    amount: 300,
                },
                Posting {
                    source: "carol".to_string(),
                    destination: "alice".to_string(),
                    asset: "usd".to_string(),
                    amount: 300,
                },
                Posting {
                    source: "alice".to_string(),
                    destination: "alice".to_string(),
                    asset: "usd".to_string(),
                    amount: -900,
                },
            ],
            metadata: serde_json::json!({}),
        };
        ledger.commit(&tx).unwrap();

        let total: i64 = ["alice", "bob", "carol"]
            .iter()
            .map(|a| ledger.balance(a, Some("usd")).unwrap())
            .sum();
        assert_eq!(total, 0);
    }

    #[test]
    fn namespace_balances_returns_all_accounts() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();
        ledger.ensure_account("carol", "cost").unwrap();

        let tx = LedgerTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reference: "tx-ns-1".to_string(),
            postings: vec![
                Posting {
                    source: "alice".to_string(),
                    destination: "bob".to_string(),
                    asset: "usd".to_string(),
                    amount: 100,
                },
                Posting {
                    source: "bob".to_string(),
                    destination: "alice".to_string(),
                    asset: "usd".to_string(),
                    amount: -100,
                },
            ],
            metadata: serde_json::json!({}),
        };
        ledger.commit(&tx).unwrap();

        let wallet_balances = ledger.namespace_balances("wallet").unwrap();
        assert!(wallet_balances.iter().any(|b| b.account == "alice"));
        assert!(wallet_balances.iter().any(|b| b.account == "bob"));
        assert!(!wallet_balances.iter().any(|b| b.account == "carol"));
    }

    #[test]
    fn namespace_balances_empty_for_unknown() {
        let ledger = db();
        let balances = ledger.namespace_balances("nonexistent").unwrap();
        assert!(balances.is_empty());
    }

    #[test]
    fn query_by_time_range() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();

        let tx = sample_tx("tx-query-1");
        ledger.commit(&tx).unwrap();

        let range = DateRange {
            start: "2000-01-01T00:00:00Z".to_string(),
            end: "2099-12-31T23:59:59Z".to_string(),
        };
        let results = ledger.query(&range, &QueryFilter::default()).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|t| t.reference == "tx-query-1"));
    }

    #[test]
    fn query_empty_when_no_matches() {
        let ledger = db();
        let range = DateRange {
            start: "2000-01-01T00:00:00Z".to_string(),
            end: "2000-01-02T00:00:00Z".to_string(),
        };
        let results = ledger.query(&range, &QueryFilter::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn query_filter_by_account() {
        let ledger = db();
        ledger.ensure_account("alice", "wallet").unwrap();
        ledger.ensure_account("bob", "wallet").unwrap();

        let tx = sample_tx("tx-filter-1");
        ledger.commit(&tx).unwrap();

        let range = DateRange {
            start: "2000-01-01T00:00:00Z".to_string(),
            end: "2099-12-31T23:59:59Z".to_string(),
        };
        let filter = QueryFilter {
            account: Some("alice".to_string()),
            ..Default::default()
        };
        let results = ledger.query(&range, &filter).unwrap();
        assert!(!results.is_empty());
    }
}
