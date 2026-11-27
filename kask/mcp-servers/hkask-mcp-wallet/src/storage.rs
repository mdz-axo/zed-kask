//! SQLCipher-encrypted storage for CDK (`WalletSqlcipherDb`) and the
//! append-only `wallet_ledger` audit log (D110 §5.1).
//!
//! Adapts `cdk-sql-common::SQLWalletDatabase` over the workspace's
//! `rusqlite@0.32` (`bundled-sqlcipher`) so there is a single `libsqlite3-sys`
//! across the entire workspace. Enforces `0600` permissions on Unix and WAL
//! mode with `synchronous = FULL`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cdk_common::database::Error as CdkDbError;
use cdk_sql_common::SQLWalletDatabase;
use cdk_sql_common::database::{DatabaseConnector, DatabaseExecutor, DatabaseTransaction};
use cdk_sql_common::pool::{self, DatabasePool};
use cdk_sql_common::run_db_operation_sync;
use cdk_sql_common::stmt::{Column, SqlPart, Statement, query};
use cdk_sql_common::value::Value;
use rusqlite::{CachedStatement, Connection, Error as SqliteError, ErrorCode, ffi, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

/// CDK wallet database backed by workspace `rusqlite@0.32` + SQLCipher.
pub type WalletSqlcipherDb = SQLWalletDatabase<SqlcipherConnectionManager>;

/// Configuration for opening a SQLCipher (or in-memory) SQLite connection.
pub struct SqlcipherConfig {
    path: Option<String>,
    password: Option<Zeroizing<String>>,
    in_memory_opened: AtomicBool,
}

impl Clone for SqlcipherConfig {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            password: self.password.clone(),
            in_memory_opened: AtomicBool::new(false),
        }
    }
}

impl fmt::Debug for SqlcipherConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqlcipherConfig")
            .field("path", &self.path)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl SqlcipherConfig {
    pub fn encrypted_file(path: PathBuf, passphrase: Zeroizing<String>) -> Self {
        Self {
            path: Some(path.to_string_lossy().into_owned()),
            password: Some(passphrase),
            in_memory_opened: AtomicBool::new(false),
        }
    }

    pub fn in_memory() -> Self {
        Self {
            path: None,
            password: None,
            in_memory_opened: AtomicBool::new(false),
        }
    }
}

impl pool::DatabaseConfig for SqlcipherConfig {
    fn default_timeout(&self) -> Duration {
        Duration::from_secs(5)
    }

    fn max_size(&self) -> usize {
        if self.path.is_none() { 1 } else { 8 }
    }
}

/// Connection manager for `cdk_sql_common::SQLWalletDatabase`.
#[derive(Debug)]
pub struct SqlcipherConnectionManager;

fn open_rusqlite_connection(
    path: Option<&str>,
    passphrase: Option<&str>,
) -> Result<Connection, SqliteError> {
    let conn = match path {
        Some(p) => {
            let path_buf = PathBuf::from(p);
            if let Some(parent) = path_buf.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    if let Err(err) = std::fs::create_dir_all(parent) {
                        tracing::warn!(
                            target: "hkask.wallet",
                            path = %parent.display(),
                            %err,
                            "failed to create wallet database parent directory"
                        );
                    }
                }
            }
            let conn = Connection::open(p)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                if let Err(err) =
                    std::fs::set_permissions(&path_buf, std::fs::Permissions::from_mode(0o600))
                {
                    tracing::warn!(
                        target: "hkask.wallet",
                        path = %path_buf.display(),
                        %err,
                        "failed to enforce 0600 permissions on SQLCipher wallet.db"
                    );
                }
            }
            conn
        }
        None => Connection::open_in_memory()?,
    };

    if let Some(pass) = passphrase {
        conn.pragma_update(None, "key", pass)?;
    }

    conn.execute_batch(
        r#"
        PRAGMA busy_timeout = 10000;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        PRAGMA temp_store = memory;
        PRAGMA foreign_keys = ON;
        "#,
    )?;
    conn.busy_timeout(Duration::from_secs(10))?;
    Ok(conn)
}

impl DatabasePool for SqlcipherConnectionManager {
    type Config = SqlcipherConfig;
    type Connection = AsyncSqlcipher;
    type Error = SqliteError;

    fn new_resource(
        config: &Self::Config,
        _stale: Arc<AtomicBool>,
        _timeout: Duration,
    ) -> Result<Self::Connection, pool::Error<Self::Error>> {
        let in_memory = config.path.is_none();
        if in_memory && config.in_memory_opened.load(Ordering::SeqCst) {
            return Err(pool::Error::Resource(SqliteError::SqliteFailure(
                ffi::Error::new(ffi::SQLITE_CANTOPEN),
                Some("in-memory database was lost after its connection was discarded".to_owned()),
            )));
        }

        let conn = open_rusqlite_connection(
            config.path.as_deref(),
            config.password.as_deref().map(String::as_str),
        )?;

        if in_memory {
            config.in_memory_opened.store(true, Ordering::SeqCst);
        }

        Ok(AsyncSqlcipher::new(conn))
    }
}

#[inline(always)]
fn to_sqlite(v: Value) -> rusqlite::types::Value {
    match v {
        Value::Blob(blob) => rusqlite::types::Value::Blob(blob),
        Value::Integer(i) => rusqlite::types::Value::Integer(i),
        Value::Null => rusqlite::types::Value::Null,
        Value::Text(t) => rusqlite::types::Value::Text(t),
        Value::Real(r) => rusqlite::types::Value::Real(r),
    }
}

#[inline(always)]
fn from_sqlite(v: rusqlite::types::Value) -> Value {
    match v {
        rusqlite::types::Value::Blob(blob) => Value::Blob(blob),
        rusqlite::types::Value::Integer(i) => Value::Integer(i),
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Text(t) => Value::Text(t),
        rusqlite::types::Value::Real(r) => Value::Real(r),
    }
}

#[inline(always)]
fn to_cdk_error(err: SqliteError) -> CdkDbError {
    if let SqliteError::SqliteFailure(
        ffi::Error {
            code,
            extended_code,
        },
        _,
    ) = err
    {
        if code == ErrorCode::ConstraintViolation
            && (extended_code == ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                || extended_code == ffi::SQLITE_CONSTRAINT_UNIQUE)
        {
            return CdkDbError::Duplicate;
        }
    }
    CdkDbError::Database(Box::new(err))
}

#[derive(Debug)]
pub struct AsyncSqlcipher {
    inner: Mutex<Connection>,
}

impl AsyncSqlcipher {
    pub fn new(inner: Connection) -> Self {
        Self {
            inner: Mutex::new(inner),
        }
    }

    fn get_stmt<'a>(
        &self,
        conn: &'a Connection,
        statement: Statement,
    ) -> Result<(String, CachedStatement<'a>), CdkDbError> {
        let (sql, placeholder_values) = statement.to_sql()?;
        let new_sql = sql.trim().trim_end_matches("FOR UPDATE");
        let mut stmt = conn
            .prepare_cached(new_sql)
            .map_err(|e| CdkDbError::Database(Box::new(e)))?;
        for (i, value) in placeholder_values.into_iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, to_sqlite(value))
                .map_err(|e| CdkDbError::Database(Box::new(e)))?;
        }
        Ok((sql, stmt))
    }
}

pub struct SqlcipherTransactionHandler;

#[async_trait::async_trait]
impl DatabaseTransaction<AsyncSqlcipher> for SqlcipherTransactionHandler {
    async fn commit(conn: &mut AsyncSqlcipher) -> Result<(), CdkDbError> {
        query("COMMIT")?.execute(conn).await?;
        Ok(())
    }

    async fn begin(conn: &mut AsyncSqlcipher) -> Result<(), CdkDbError> {
        query("BEGIN IMMEDIATE")?.execute(conn).await?;
        Ok(())
    }

    async fn rollback(conn: &mut AsyncSqlcipher) -> Result<(), CdkDbError> {
        if conn.inner.get_mut().is_autocommit() {
            return Ok(());
        }
        query("ROLLBACK")?.execute(conn).await?;
        Ok(())
    }
}

impl DatabaseConnector for AsyncSqlcipher {
    type Transaction = SqlcipherTransactionHandler;
}

#[async_trait::async_trait]
impl DatabaseExecutor for AsyncSqlcipher {
    fn name() -> &'static str {
        "sqlite"
    }

    async fn execute(&self, statement: Statement) -> Result<usize, CdkDbError> {
        let conn = self.inner.lock().await;
        let (sql, mut stmt) = self
            .get_stmt(&conn, statement)
            .map_err(|e| CdkDbError::Database(Box::new(e)))?;
        run_db_operation_sync(&sql, || stmt.raw_execute(), to_cdk_error)
    }

    async fn fetch_one(&self, statement: Statement) -> Result<Option<Vec<Column>>, CdkDbError> {
        let conn = self.inner.lock().await;
        let (sql, mut stmt) = self
            .get_stmt(&conn, statement)
            .map_err(|e| CdkDbError::Database(Box::new(e)))?;
        run_db_operation_sync(
            &sql,
            || {
                let columns = stmt.column_count();
                let mut rows = stmt.raw_query();
                rows.next()?
                    .map(|row| {
                        (0..columns)
                            .map(|i| row.get(i).map(from_sqlite))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()
            },
            to_cdk_error,
        )
    }

    async fn fetch_all(&self, statement: Statement) -> Result<Vec<Vec<Column>>, CdkDbError> {
        let conn = self.inner.lock().await;
        let (sql, mut stmt) = self
            .get_stmt(&conn, statement)
            .map_err(|e| CdkDbError::Database(Box::new(e)))?;
        let columns = stmt.column_count();
        run_db_operation_sync(
            &sql,
            || {
                let mut rows = stmt.raw_query();
                let mut results = Vec::new();
                while let Some(row) = rows.next()? {
                    results.push(
                        (0..columns)
                            .map(|i| row.get(i).map(from_sqlite))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                }
                Ok(results)
            },
            to_cdk_error,
        )
    }

    async fn pluck(&self, statement: Statement) -> Result<Option<Column>, CdkDbError> {
        let conn = self.inner.lock().await;
        let (sql, mut stmt) = self
            .get_stmt(&conn, statement)
            .map_err(|e| CdkDbError::Database(Box::new(e)))?;
        run_db_operation_sync(
            &sql,
            || {
                let mut rows = stmt.raw_query();
                rows.next()?
                    .map(|row| row.get(0usize).map(from_sqlite))
                    .transpose()
            },
            to_cdk_error,
        )
    }

    async fn batch(&self, mut statement: Statement) -> Result<(), CdkDbError> {
        let sql = {
            let part = statement
                .parts
                .pop()
                .ok_or_else(|| CdkDbError::Internal("Empty SQL".to_owned()))?;
            if !statement.parts.is_empty() || matches!(part, SqlPart::Placeholder(_, _)) {
                return Err(CdkDbError::Internal(
                    "Invalid usage, batch does not support placeholders".to_owned(),
                ));
            }
            match part {
                SqlPart::Raw(sql) => sql,
                _ => unreachable!(),
            }
        };
        let conn = self.inner.lock().await;
        run_db_operation_sync(&sql, || conn.execute_batch(&sql), to_cdk_error)
    }
}

// ---------------------------------------------------------------------------
// Append-only `wallet_ledger` audit table (D110 §5.1, Threat T8)
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("duplicate idempotency_key or active payment_hash: {0}")]
    Duplicate(String),
    #[error("ledger database error: {0}")]
    Sqlite(#[from] SqliteError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerRow {
    pub id: String,
    pub idempotency_key: String,
    pub direction: String,
    pub rail: String,
    pub network: String,
    pub mint_url: Option<String>,
    pub payment_hash: Option<String>,
    pub amount_sats: u64,
    pub fee_sats: u64,
    pub status: String,
    pub target_summary: String,
    pub nostr_event_id: Option<String>,
    pub approval_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Local SQLCipher-backed append-only audit ledger (`wallet_ledger`).
pub struct WalletLedger {
    conn: std::sync::Mutex<Connection>,
}

impl WalletLedger {
    pub fn open_encrypted(path: &Path, passphrase: &str) -> Result<Self, LedgerError> {
        let conn = open_rusqlite_connection(Some(&path.to_string_lossy()), Some(passphrase))?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self, LedgerError> {
        let conn = open_rusqlite_connection(None, None)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    fn init_schema(conn: &Connection) -> Result<(), LedgerError> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS wallet_ledger (
                id TEXT PRIMARY KEY,
                idempotency_key TEXT NOT NULL UNIQUE,
                direction TEXT NOT NULL,
                rail TEXT NOT NULL,
                network TEXT NOT NULL,
                mint_url TEXT,
                payment_hash TEXT,
                amount_sats INTEGER NOT NULL,
                fee_sats INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                target_summary TEXT NOT NULL,
                nostr_event_id TEXT,
                approval_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_wallet_ledger_outbound_payment_hash
                ON wallet_ledger(payment_hash)
                WHERE direction = 'outbound'
                  AND payment_hash IS NOT NULL
                  AND status IN ('drafted', 'awaiting_approval', 'pending', 'reconciling', 'settled');
            CREATE INDEX IF NOT EXISTS idx_wallet_ledger_status_created
                ON wallet_ledger(status, created_at DESC);
            "#,
        )?;
        Ok(())
    }

    pub fn insert(&self, row: &LedgerRow) -> Result<(), LedgerError> {
        let conn = self.conn.lock().map_err(|_| SqliteError::InvalidQuery)?;
        let res = conn.execute(
            r#"
            INSERT INTO wallet_ledger (
                id, idempotency_key, direction, rail, network, mint_url,
                payment_hash, amount_sats, fee_sats, status, target_summary,
                nostr_event_id, approval_id, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            "#,
            params![
                row.id,
                row.idempotency_key,
                row.direction,
                row.rail,
                row.network,
                row.mint_url,
                row.payment_hash,
                row.amount_sats as i64,
                row.fee_sats as i64,
                row.status,
                row.target_summary,
                row.nostr_event_id,
                row.approval_id,
                row.created_at as i64,
                row.updated_at as i64,
            ],
        );
        match res {
            Ok(_) => Ok(()),
            Err(SqliteError::SqliteFailure(
                ffi::Error {
                    code: ErrorCode::ConstraintViolation,
                    ..
                },
                _,
            )) => Err(LedgerError::Duplicate(row.idempotency_key.clone())),
            Err(e) => Err(LedgerError::Sqlite(e)),
        }
    }

    pub fn update_status(
        &self,
        id: &str,
        status: &str,
        fee_sats: Option<u64>,
        approval_id: Option<&str>,
        updated_at: u64,
    ) -> Result<(), LedgerError> {
        let conn = self.conn.lock().map_err(|_| SqliteError::InvalidQuery)?;
        conn.execute(
            r#"
            UPDATE wallet_ledger
            SET status = ?2,
                fee_sats = COALESCE(?3, fee_sats),
                approval_id = COALESCE(?4, approval_id),
                updated_at = ?5
            WHERE id = ?1
            "#,
            params![
                id,
                status,
                fee_sats.map(|f| f as i64),
                approval_id,
                updated_at as i64,
            ],
        )?;
        Ok(())
    }

    pub fn get_by_idempotency_key(&self, key: &str) -> Result<Option<LedgerRow>, LedgerError> {
        let conn = self.conn.lock().map_err(|_| SqliteError::InvalidQuery)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, idempotency_key, direction, rail, network, mint_url,
                   payment_hash, amount_sats, fee_sats, status, target_summary,
                   nostr_event_id, approval_id, created_at, updated_at
            FROM wallet_ledger
            WHERE idempotency_key = ?1
            "#,
        )?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_ledger(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn count_pending(&self) -> Result<u64, LedgerError> {
        let conn = self.conn.lock().map_err(|_| SqliteError::InvalidQuery)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wallet_ledger WHERE status IN ('pending', 'reconciling')",
            [],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u64)
    }

    pub fn list(
        &self,
        rail: Option<&str>,
        status: Option<&str>,
        since: Option<u64>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<LedgerRow>, LedgerError> {
        let conn = self.conn.lock().map_err(|_| SqliteError::InvalidQuery)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, idempotency_key, direction, rail, network, mint_url,
                   payment_hash, amount_sats, fee_sats, status, target_summary,
                   nostr_event_id, approval_id, created_at, updated_at
            FROM wallet_ledger
            WHERE (?1 IS NULL OR rail = ?1)
              AND (?2 IS NULL OR status = ?2)
              AND (?3 IS NULL OR created_at >= ?3)
            ORDER BY created_at DESC, id DESC
            LIMIT ?4 OFFSET ?5
            "#,
        )?;
        let rows = stmt.query_map(
            params![
                rail,
                status,
                since.map(|s| s as i64),
                limit as i64,
                offset as i64,
            ],
            row_to_ledger,
        )?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

fn row_to_ledger(row: &rusqlite::Row<'_>) -> Result<LedgerRow, SqliteError> {
    Ok(LedgerRow {
        id: row.get(0)?,
        idempotency_key: row.get(1)?,
        direction: row.get(2)?,
        rail: row.get(3)?,
        network: row.get(4)?,
        mint_url: row.get(5)?,
        payment_hash: row.get(6)?,
        amount_sats: row.get::<_, i64>(7)?.max(0) as u64,
        fee_sats: row.get::<_, i64>(8)?.max(0) as u64,
        status: row.get(9)?,
        target_summary: row.get(10)?,
        nostr_event_id: row.get(11)?,
        approval_id: row.get(12)?,
        created_at: row.get::<_, i64>(13)?.max(0) as u64,
        updated_at: row.get::<_, i64>(14)?.max(0) as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk_common::database::WalletDatabase as _;

    #[tokio::test]
    async fn sqlcipher_wallet_db_encrypts_at_rest_and_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("wallet.db");
        let passphrase = Zeroizing::new("a".repeat(64));

        let db = WalletSqlcipherDb::new(SqlcipherConfig::encrypted_file(
            db_path.clone(),
            passphrase.clone(),
        ))
        .await
        .expect("open encrypted CDK DB");

        let mint_url = "https://mint.example.com".parse().expect("mint url");
        db.add_mint(mint_url, None).await.expect("add mint");

        // Ledger opens on the same encrypted SQLCipher file
        let ledger = WalletLedger::open_encrypted(&db_path, &passphrase).expect("open ledger");
        let row = LedgerRow {
            id: "tx-1".into(),
            idempotency_key: "idem-1".into(),
            direction: "outbound".into(),
            rail: "cashu".into(),
            network: "regtest".into(),
            mint_url: Some("https://mint.example.com".into()),
            payment_hash: Some("11".repeat(32)),
            amount_sats: 100,
            fee_sats: 2,
            status: "pending".into(),
            target_summary: "test".into(),
            nostr_event_id: None,
            approval_id: Some("appr-1".into()),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
        };
        ledger.insert(&row).expect("insert ledger row");

        // Duplicate idempotency_key is refused (T8)
        assert!(matches!(
            ledger.insert(&row),
            Err(LedgerError::Duplicate(_))
        ));

        // Duplicate active outbound payment_hash is refused even with a new idempotency_key (T8)
        let mut dup_hash = row;
        dup_hash.id = "tx-2".into();
        dup_hash.idempotency_key = "idem-2".into();
        assert!(matches!(
            ledger.insert(&dup_hash),
            Err(LedgerError::Duplicate(_))
        ));

        // Raw file bytes must NOT start with plaintext "SQLite format 3"
        let raw = std::fs::read(&db_path).expect("read db file");
        assert!(
            !raw.starts_with(b"SQLite format 3"),
            "SQLCipher DB must be encrypted at rest"
        );

        // Wrong passphrase must fail closed and never corrupt the intact DB
        drop(ledger);
        drop(db);
        let wrong_pass = "f".repeat(64);
        assert!(
            WalletLedger::open_encrypted(&db_path, &wrong_pass).is_err(),
            "opening encrypted wallet.db with wrong passphrase must fail closed"
        );
        let reopened =
            WalletLedger::open_encrypted(&db_path, &passphrase).expect("reopen with right key");
        assert!(
            reopened
                .get_by_idempotency_key("idem-1")
                .expect("query")
                .is_some(),
            "data must remain intact after failed wrong-passphrase attempt"
        );
    }
}
