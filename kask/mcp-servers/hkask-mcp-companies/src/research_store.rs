//! hKask MCP Companies — research store (companies-specific layer).
//!
//! Research notes, file attachments, and DCF forecast snapshots, keyed by
//! stock symbol. The general-purpose transaction ledger, holdings, and
//! returns live in the `hkask-mcp-portfolio` server; this store opens the
//! same shared DB (`mcp/portfolio/{owner}`) for the ledger context its
//! artifacts attach to, and owns the companies-specific tables (notes,
//! files, forecasts) alongside the portfolio crate's schema.

use hkask_mcp_portfolio::{LedgerFilter, PortfolioStore};
// Re-exported for the tool layer's imports.
pub(crate) use hkask_mcp_portfolio::{PortfolioError, Transaction};
use hkask_types::{WebID, agent_paths::sanitize_name, time::now_rfc3339};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;

/// DDL for the companies-specific tables (notes, files, forecasts) that
/// live alongside the portfolio crate's schema in the same SQLite DB.
/// The portfolio crate owns the `portfolios`, `transactions`, `price_cache`,
/// `daily_holdings`, and `daily_returns` tables; this module owns the rest.
const COMPANIES_SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS notes (
                    id TEXT PRIMARY KEY,
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    symbol TEXT NOT NULL,
                    date TEXT NOT NULL,
                    title TEXT NOT NULL,
                    body TEXT NOT NULL,
                    tags TEXT DEFAULT '[]',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_notes_portfolio ON notes(portfolio_name);
                CREATE INDEX IF NOT EXISTS idx_notes_symbol ON notes(symbol);
                CREATE TABLE IF NOT EXISTS files (
                    id TEXT PRIMARY KEY,
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    symbol TEXT NOT NULL,
                    date TEXT NOT NULL,
                    filename TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    path TEXT NOT NULL,
                    notes TEXT DEFAULT '',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_files_portfolio ON files(portfolio_name);
                CREATE INDEX IF NOT EXISTS idx_files_symbol ON files(symbol);
                CREATE TABLE IF NOT EXISTS forecasts (
                    id TEXT PRIMARY KEY,
                    symbol TEXT NOT NULL,
                    revision_of TEXT,
                    snapshot TEXT NOT NULL,
                    outcomes TEXT NOT NULL DEFAULT '[]',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_forecasts_symbol ON forecasts(symbol);
                CREATE TABLE IF NOT EXISTS screen_jobs (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    definition TEXT NOT NULL,
                    result TEXT,
                    error TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    stage TEXT NOT NULL DEFAULT 'queued',
                    processed INTEGER NOT NULL DEFAULT 0,
                    total INTEGER NOT NULL DEFAULT 0,
                    complete_count INTEGER NOT NULL DEFAULT 0,
                    partial_count INTEGER NOT NULL DEFAULT 0,
                    unavailable_count INTEGER NOT NULL DEFAULT 0,
                    model_sensitive_count INTEGER NOT NULL DEFAULT 0,
                    heartbeat_at TEXT,
                    cancel_requested INTEGER NOT NULL DEFAULT 0,
                    checkpoint TEXT,
                    artifact_path TEXT
                );
                CREATE TABLE IF NOT EXISTS screen_job_items (
                    job_id TEXT NOT NULL REFERENCES screen_jobs(id) ON DELETE CASCADE,
                    issuer_key TEXT NOT NULL,
                    ordinal INTEGER NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    payload TEXT NOT NULL,
                    row_json TEXT,
                    error TEXT,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (job_id, issuer_key)
                );
                CREATE INDEX IF NOT EXISTS idx_screen_job_items_pending
                    ON screen_job_items(job_id, status, ordinal);";

const MAX_ENCODED_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_DECODED_ATTACHMENT_BYTES: usize = 6 * 1024 * 1024;

/// Resolve the SQLite DB path for an owner, mirroring the portfolio crate's
/// `PortfolioStore::new` path resolution. The companies module opens its
/// own connection to the same DB for notes/files/forecasts tables.
///
/// Databases live in the internal data dir. Path is
/// `{kask_data_dir}/mcp/portfolio/{owner}/master.db`.
fn resolve_db_path(owner: &WebID) -> Result<PathBuf, PortfolioError> {
    let mut path = hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
        hkask_types::agent_paths::MCP_DIR,
    ))
    .join("portfolio");
    path.push(sanitize_name(&owner.to_string()));
    // A read-only data dir, a full disk, or a permissions error must surface
    // as an error the server can report, not abort the process at startup.
    // Mirrors `PortfolioStore::new`, which already propagates.
    std::fs::create_dir_all(&path).map_err(|e| {
        PortfolioError::from(format!(
            "failed to create portfolio directory {}: {e}",
            path.display()
        ))
    })?;
    Ok(path.join("master.db"))
}

/// Owner-scoped saved-screen calculation job and immutable result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScreenJobRecord {
    pub id: String,
    pub status: String,
    pub definition: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub stage: String,
    pub processed: usize,
    pub total: usize,
    pub complete_count: usize,
    pub partial_count: usize,
    pub unavailable_count: usize,
    pub model_sensitive_count: usize,
    pub heartbeat_at: Option<String>,
    pub cancel_requested: bool,
    pub checkpoint: Option<serde_json::Value>,
    pub artifact_path: Option<String>,
}

/// One durable unit of saved-screen enrichment, keyed by provisional issuer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScreenJobItemRecord {
    pub issuer_key: String,
    pub ordinal: usize,
    pub status: String,
    pub payload: serde_json::Value,
    pub row: Option<serde_json::Value>,
    pub error: Option<String>,
    pub updated_at: String,
}

impl ScreenJobItemRecord {
    pub(crate) fn pending(issuer_key: &str, ordinal: usize, payload: serde_json::Value) -> Self {
        Self {
            issuer_key: issuer_key.to_string(),
            ordinal,
            status: "pending".to_string(),
            payload,
            row: None,
            error: None,
            updated_at: now_rfc3339(),
        }
    }
}

/// Owner-scoped forecast persisted as structured JSON for later reconstruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedForecast {
    pub id: String,
    pub symbol: String,
    pub revision_of: Option<String>,
    pub snapshot: serde_json::Value,
    #[serde(default)]
    pub outcomes: Vec<serde_json::Value>,
    pub created_at: String,
}

fn parse_forecast_json<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            value.len(),
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, PortfolioError> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("inspect {table} columns: {e}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("query {table} columns: {e}"))?;
    for row in rows {
        if row.map_err(|e| format!("read {table} column: {e}"))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn row_to_screen_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScreenJobRecord> {
    let definition: String = row.get(2)?;
    let result: Option<String> = row.get(3)?;
    let checkpoint: Option<String> = row.get(16)?;
    Ok(ScreenJobRecord {
        id: row.get(0)?,
        status: row.get(1)?,
        definition: parse_forecast_json(definition)?,
        result: result.map(parse_forecast_json).transpose()?,
        error: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        stage: row.get(7)?,
        processed: row.get(8)?,
        total: row.get(9)?,
        complete_count: row.get(10)?,
        partial_count: row.get(11)?,
        unavailable_count: row.get(12)?,
        model_sensitive_count: row.get(13)?,
        heartbeat_at: row.get(14)?,
        cancel_requested: row.get(15)?,
        checkpoint: checkpoint.map(parse_forecast_json).transpose()?,
        artifact_path: row.get(17)?,
    })
}

fn row_to_screen_job_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScreenJobItemRecord> {
    let payload: String = row.get(3)?;
    let row_json: Option<String> = row.get(4)?;
    Ok(ScreenJobItemRecord {
        issuer_key: row.get(0)?,
        ordinal: row.get(1)?,
        status: row.get(2)?,
        payload: parse_forecast_json(payload)?,
        row: row_json.map(parse_forecast_json).transpose()?,
        error: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn row_to_persisted_forecast(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedForecast> {
    let snapshot: String = row.get(3)?;
    let outcomes: String = row.get(4)?;
    Ok(PersistedForecast {
        id: row.get(0)?,
        symbol: row.get(1)?,
        revision_of: row.get(2)?,
        snapshot: parse_forecast_json(snapshot)?,
        outcomes: parse_forecast_json(outcomes)?,
        created_at: row.get(5)?,
    })
}

/// Companies-side research store. Holds a [`PortfolioStore`] (which owns
/// the shared SQLite DB and its general schema) and adds the
/// companies-specific research artifacts: notes, files, and DCF forecast
/// snapshots.
#[derive(Clone)]
pub(crate) struct ResearchStore {
    /// The general-purpose store (owns the SQLite DB + schema).
    store: PortfolioStore,
    /// Path to the same SQLite DB the store uses, for companies-specific
    /// tables (notes, files, forecasts). Mirrored at construction so this
    /// module can open its own connection without reaching into the store.
    db_path: PathBuf,
}

impl ResearchStore {
    /// Creates storage scoped to the authenticated server owner. The
    /// portfolio crate creates the DB and the general schema; this module
    /// adds the companies-specific tables (notes, files, forecasts) on top.
    pub fn new(owner: WebID) -> Result<Self, PortfolioError> {
        let store = PortfolioStore::new(owner)?;
        let db_path = resolve_db_path(&owner)?;
        let manager = Self { store, db_path };
        manager.ensure_companies_schema()?;
        manager.recover_interrupted_screen_jobs()?;
        Ok(manager)
    }

    #[cfg(test)]
    pub(crate) fn with_dir(directory: PathBuf) -> Result<Self, PortfolioError> {
        let manager = Self {
            store: PortfolioStore::with_dir(directory.clone()),
            db_path: directory.join("master.db"),
        };
        manager.ensure_companies_schema()?;
        manager.recover_interrupted_screen_jobs()?;
        Ok(manager)
    }

    /// Open a connection to the same DB the store uses, for companies-specific
    /// tables (notes, files, forecasts).
    fn open(&self) -> Result<Connection, PortfolioError> {
        let conn = Connection::open(&self.db_path).map_err(|e| format!("db open: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| format!("enable foreign keys: {e}"))?;
        Ok(conn)
    }

    fn ensure_companies_schema(&self) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        conn.execute_batch(COMPANIES_SCHEMA_DDL)
            .map_err(|e| format!("failed to initialize companies schema: {e}"))?;
        for (column, definition) in [
            ("stage", "TEXT NOT NULL DEFAULT 'queued'"),
            ("processed", "INTEGER NOT NULL DEFAULT 0"),
            ("total", "INTEGER NOT NULL DEFAULT 0"),
            ("complete_count", "INTEGER NOT NULL DEFAULT 0"),
            ("partial_count", "INTEGER NOT NULL DEFAULT 0"),
            ("unavailable_count", "INTEGER NOT NULL DEFAULT 0"),
            ("model_sensitive_count", "INTEGER NOT NULL DEFAULT 0"),
            ("heartbeat_at", "TEXT"),
            ("cancel_requested", "INTEGER NOT NULL DEFAULT 0"),
            ("checkpoint", "TEXT"),
            ("artifact_path", "TEXT"),
        ] {
            if !table_has_column(&conn, "screen_jobs", column)? {
                conn.execute_batch(&format!(
                    "ALTER TABLE screen_jobs ADD COLUMN {column} {definition};"
                ))
                .map_err(|e| format!("add screen_jobs.{column}: {e}"))?;
            }
        }
        conn.execute(
            "DELETE FROM screen_job_items
             WHERE NOT EXISTS (
                 SELECT 1 FROM screen_jobs WHERE screen_jobs.id = screen_job_items.job_id
             )",
            [],
        )
        .map_err(|e| format!("remove orphaned screen job items: {e}"))?;
        Ok(())
    }

    fn recover_interrupted_screen_jobs(&self) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let now = now_rfc3339();
        let cancelled = conn
            .execute(
                "UPDATE screen_jobs
                 SET status = 'cancelled', stage = 'cancelled', updated_at = ?1,
                     heartbeat_at = ?1
                 WHERE status = 'cancelling' OR cancel_requested = 1",
                params![now],
            )
            .map_err(|error| format!("recover cancelled screen jobs: {error}"))?;
        let resumable = conn
            .execute(
                "UPDATE screen_jobs
                 SET status = 'queued', updated_at = ?1, heartbeat_at = ?1
                 WHERE status = 'executing'",
                params![now],
            )
            .map_err(|error| format!("recover interrupted screen jobs: {error}"))?;
        if cancelled > 0 || resumable > 0 {
            tracing::warn!(
                cancelled,
                resumable,
                "recovered interrupted saved-screen jobs from durable state"
            );
        }
        Ok(())
    }

    // ── Ledger context (read-only views over the shared portfolio DB) ──

    pub fn get_transactions(
        &self,
        name: &str,
        symbol: Option<&str>,
        tx_type: Option<&str>,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<Vec<Transaction>, PortfolioError> {
        self.store.ledger(
            name,
            LedgerFilter {
                symbol,
                tx_type,
                asset_type: None,
                from_date,
                to_date,
            },
        )
    }

    pub fn get_symbols(&self, name: &str) -> Result<Vec<String>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT DISTINCT symbol FROM transactions WHERE portfolio_name = ?1 AND symbol IS NOT NULL AND symbol != ''")
            .map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params![name], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query: {e}"))?;
        let mut symbols = Vec::new();
        for row in rows {
            symbols.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(symbols)
    }

    // ── Companies-specific: screening jobs ─────────────────────────

    pub fn insert_screen_job(&self, job: &ScreenJobRecord) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let definition = serde_json::to_string(&job.definition)
            .map_err(|e| format!("serialize screen definition: {e}"))?;
        conn.execute(
            "INSERT INTO screen_jobs
             (id, status, definition, result, error, created_at, updated_at,
              stage, processed, total, complete_count, partial_count,
              unavailable_count, model_sensitive_count, heartbeat_at,
              cancel_requested, checkpoint, artifact_path)
             VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?4, ?5, ?6, ?7, ?8, ?9,
                     ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                job.id,
                job.status,
                definition,
                job.created_at,
                job.stage,
                job.processed,
                job.total,
                job.complete_count,
                job.partial_count,
                job.unavailable_count,
                job.model_sensitive_count,
                job.heartbeat_at,
                job.cancel_requested,
                job.checkpoint
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| format!("serialize screen checkpoint: {e}"))?,
                job.artifact_path,
            ],
        )
        .map_err(|e| format!("insert screen job: {e}"))?;
        Ok(())
    }

    pub fn update_screen_job(
        &self,
        id: &str,
        status: &str,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
    ) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let result = result
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| format!("serialize screen result: {e}"))?;
        let updated = conn
            .execute(
                "UPDATE screen_jobs
                 SET status = ?2, result = ?3, error = ?4, updated_at = ?5
                 WHERE id = ?1",
                params![id, status, result, error, now_rfc3339()],
            )
            .map_err(|e| format!("update screen job: {e}"))?;
        if updated == 0 {
            return Err(format!("update screen job: job {id:?} was not found").into());
        }
        Ok(())
    }

    pub fn get_screen_job(&self, id: &str) -> Result<Option<ScreenJobRecord>, PortfolioError> {
        let conn = self.open()?;
        conn.query_row(
            "SELECT id, status, definition, result, error, created_at, updated_at,
                    stage, processed, total, complete_count, partial_count,
                    unavailable_count, model_sensitive_count, heartbeat_at,
                    cancel_requested, checkpoint, artifact_path
             FROM screen_jobs WHERE id = ?1",
            params![id],
            row_to_screen_job,
        )
        .optional()
        .map_err(|e| format!("get screen job: {e}").into())
    }

    /// Commit the full financial pass set and its reconstruction metadata in one transaction.
    pub fn persist_screen_pass_set(
        &self,
        id: &str,
        checkpoint: &serde_json::Value,
        items: &[ScreenJobItemRecord],
    ) -> Result<(), PortfolioError> {
        let mut conn = self.open()?;
        let transaction = conn
            .transaction()
            .map_err(|e| format!("begin screen pass-set transaction: {e}"))?;
        let checkpoint = serde_json::to_string(checkpoint)
            .map_err(|e| format!("serialize screen checkpoint: {e}"))?;
        let now = now_rfc3339();
        let total = i64::try_from(items.len())
            .map_err(|e| format!("screen pass-set size exceeds SQLite range: {e}"))?;
        let updated = transaction
            .execute(
                "UPDATE screen_jobs
                 SET status = 'queued', stage = 'enrichment', checkpoint = ?2,
                     processed = 0, total = ?3, complete_count = 0,
                     partial_count = 0, unavailable_count = 0,
                     model_sensitive_count = 0, heartbeat_at = ?4,
                     updated_at = ?4, error = NULL
                 WHERE id = ?1",
                params![id, checkpoint, total, now],
            )
            .map_err(|e| format!("checkpoint screen pass set: {e}"))?;
        if updated == 0 {
            return Err(format!("checkpoint screen pass set: job {id:?} was not found").into());
        }
        for item in items {
            let payload = serde_json::to_string(&item.payload)
                .map_err(|e| format!("serialize screen issuer payload: {e}"))?;
            let ordinal = i64::try_from(item.ordinal)
                .map_err(|e| format!("screen issuer ordinal exceeds SQLite range: {e}"))?;
            transaction
                .execute(
                    "INSERT INTO screen_job_items
                     (job_id, issuer_key, ordinal, status, payload, row_json, error, updated_at)
                     VALUES (?1, ?2, ?3, 'pending', ?4, NULL, NULL, ?5)",
                    params![id, item.issuer_key, ordinal, payload, now],
                )
                .map_err(|e| format!("insert screen issuer work item: {e}"))?;
        }
        transaction
            .commit()
            .map_err(|e| format!("commit screen pass set: {e}"))?;
        Ok(())
    }

    pub fn pending_screen_jobs(&self) -> Result<Vec<ScreenJobRecord>, PortfolioError> {
        let conn = self.open()?;
        let mut statement = conn
            .prepare(
                "SELECT id, status, definition, result, error, created_at, updated_at,
                        stage, processed, total, complete_count, partial_count,
                        unavailable_count, model_sensitive_count, heartbeat_at,
                        cancel_requested, checkpoint, artifact_path
                 FROM screen_jobs
                 WHERE status = 'queued' AND cancel_requested = 0
                 ORDER BY created_at",
            )
            .map_err(|e| format!("prepare pending screen jobs: {e}"))?;
        let rows = statement
            .query_map([], row_to_screen_job)
            .map_err(|e| format!("query pending screen jobs: {e}"))?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|e| format!("read pending screen job: {e}"))?);
        }
        Ok(jobs)
    }

    pub fn pending_screen_items(
        &self,
        id: &str,
    ) -> Result<Vec<ScreenJobItemRecord>, PortfolioError> {
        self.screen_items_with_status(id, Some("pending"))
    }

    pub fn all_screen_items(&self, id: &str) -> Result<Vec<ScreenJobItemRecord>, PortfolioError> {
        self.screen_items_with_status(id, None)
    }

    fn screen_items_with_status(
        &self,
        id: &str,
        status: Option<&str>,
    ) -> Result<Vec<ScreenJobItemRecord>, PortfolioError> {
        let conn = self.open()?;
        let sql = if status.is_some() {
            "SELECT issuer_key, ordinal, status, payload, row_json, error, updated_at
             FROM screen_job_items WHERE job_id = ?1 AND status = ?2 ORDER BY ordinal"
        } else {
            "SELECT issuer_key, ordinal, status, payload, row_json, error, updated_at
             FROM screen_job_items WHERE job_id = ?1 ORDER BY ordinal"
        };
        let mut statement = conn
            .prepare(sql)
            .map_err(|e| format!("prepare screen issuer items: {e}"))?;
        let mut items = Vec::new();
        if let Some(status) = status {
            let rows = statement
                .query_map(params![id, status], row_to_screen_job_item)
                .map_err(|e| format!("query screen issuer items: {e}"))?;
            for row in rows {
                items.push(row.map_err(|e| format!("read screen issuer item: {e}"))?);
            }
        } else {
            let rows = statement
                .query_map(params![id], row_to_screen_job_item)
                .map_err(|e| format!("query screen issuer items: {e}"))?;
            for row in rows {
                items.push(row.map_err(|e| format!("read screen issuer item: {e}"))?);
            }
        }
        Ok(items)
    }

    pub fn mark_screen_job_executing(&self, id: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let now = now_rfc3339();
        let updated = conn
            .execute(
                "UPDATE screen_jobs SET status = 'executing', heartbeat_at = ?2,
                 updated_at = ?2 WHERE id = ?1 AND cancel_requested = 0",
                params![id, now],
            )
            .map_err(|e| format!("mark screen job executing: {e}"))?;
        if updated == 0 {
            return Err(
                format!("mark screen job executing: job {id:?} is missing or cancelled").into(),
            );
        }
        Ok(())
    }

    pub fn heartbeat_screen_job(&self, id: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let now = now_rfc3339();
        conn.execute(
            "UPDATE screen_jobs SET heartbeat_at = ?2, updated_at = ?2
             WHERE id = ?1 AND status IN ('queued', 'executing', 'cancelling')",
            params![id, now],
        )
        .map_err(|e| format!("heartbeat screen job: {e}"))?;
        Ok(())
    }

    pub fn request_screen_cancel(&self, id: &str) -> Result<bool, PortfolioError> {
        let conn = self.open()?;
        let now = now_rfc3339();
        let updated = conn
            .execute(
                "UPDATE screen_jobs SET cancel_requested = 1, status = 'cancelling',
                 stage = 'cancelling', heartbeat_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND status IN ('queued', 'executing')",
                params![id, now],
            )
            .map_err(|e| format!("request screen cancellation: {e}"))?;
        Ok(updated > 0)
    }

    pub fn screen_cancel_requested(&self, id: &str) -> Result<bool, PortfolioError> {
        let conn = self.open()?;
        conn.query_row(
            "SELECT cancel_requested FROM screen_jobs WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
        .map_err(|e| format!("read screen cancellation: {e}").into())
    }

    pub fn complete_screen_item(
        &self,
        id: &str,
        issuer_key: &str,
        classification: &str,
        row: &serde_json::Value,
        error: Option<&str>,
    ) -> Result<(), PortfolioError> {
        if !matches!(
            classification,
            "complete" | "partial" | "unavailable" | "model_sensitive"
        ) {
            return Err(format!("invalid screen issuer classification {classification:?}").into());
        }
        let mut conn = self.open()?;
        let transaction = conn
            .transaction()
            .map_err(|e| format!("begin screen item completion: {e}"))?;
        let row = serde_json::to_string(row)
            .map_err(|e| format!("serialize screen issuer result: {e}"))?;
        let now = now_rfc3339();
        let updated = transaction
            .execute(
                "UPDATE screen_job_items SET status = ?3, row_json = ?4, error = ?5,
                 updated_at = ?6 WHERE job_id = ?1 AND issuer_key = ?2 AND status = 'pending'",
                params![id, issuer_key, classification, row, error, now],
            )
            .map_err(|e| format!("complete screen issuer item: {e}"))?;
        if updated == 0 {
            return Err(format!(
                "complete screen issuer item: pending issuer {issuer_key:?} not found"
            )
            .into());
        }
        let counter = match classification {
            "complete" => "complete_count",
            "partial" => "partial_count",
            "unavailable" => "unavailable_count",
            "model_sensitive" => "model_sensitive_count",
            _ => return Err("validated classification became invalid".into()),
        };
        transaction
            .execute(
                &format!(
                    "UPDATE screen_jobs SET processed = processed + 1,
                     {counter} = {counter} + 1, heartbeat_at = ?2, updated_at = ?2
                     WHERE id = ?1"
                ),
                params![id, now],
            )
            .map_err(|e| format!("advance screen job progress: {e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("commit screen item completion: {e}"))?;
        Ok(())
    }

    pub fn mark_pending_screen_items_unavailable(
        &self,
        id: &str,
        reason: &str,
    ) -> Result<(), PortfolioError> {
        let pending = self.pending_screen_items(id)?;
        for item in pending {
            let row = serde_json::json!({
                "issuer_key": item.issuer_key,
                "data_quality_status": "unavailable",
                "unavailable_reason": reason,
            });
            self.complete_screen_item(id, &item.issuer_key, "unavailable", &row, Some(reason))?;
        }
        Ok(())
    }

    pub fn finish_screen_job(
        &self,
        id: &str,
        status: &str,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
        artifact_path: Option<&str>,
    ) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let result = result
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| format!("serialize screen result: {e}"))?;
        let now = now_rfc3339();
        let updated = conn
            .execute(
                "UPDATE screen_jobs SET status = ?2, stage = ?2, result = ?3,
                 error = ?4, artifact_path = ?5, heartbeat_at = ?6, updated_at = ?6
                 WHERE id = ?1",
                params![id, status, result, error, artifact_path, now],
            )
            .map_err(|e| format!("finish screen job: {e}"))?;
        if updated == 0 {
            return Err(format!("finish screen job: job {id:?} was not found").into());
        }
        Ok(())
    }

    // ── Companies-specific: forecasts ──────────────────────────────

    pub fn save_forecast(&self, forecast: &PersistedForecast) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let snapshot = serde_json::to_string(&forecast.snapshot)
            .map_err(|e| format!("serialize forecast snapshot: {e}"))?;
        let outcomes = serde_json::to_string(&forecast.outcomes)
            .map_err(|e| format!("serialize forecast outcomes: {e}"))?;
        conn.execute(
            "INSERT INTO forecasts (id, symbol, revision_of, snapshot, outcomes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                forecast.id,
                forecast.symbol,
                forecast.revision_of,
                snapshot,
                outcomes,
                forecast.created_at,
            ],
        )
        .map_err(|e| format!("save forecast: {e}"))?;
        Ok(())
    }

    pub fn get_forecast(&self, id: &str) -> Result<Option<PersistedForecast>, PortfolioError> {
        let conn = self.open()?;
        conn.query_row(
            "SELECT id, symbol, revision_of, snapshot, outcomes, created_at FROM forecasts WHERE id = ?1",
            params![id],
            row_to_persisted_forecast,
        )
        .optional()
        .map_err(|e| format!("get forecast: {e}").into())
    }

    pub fn list_forecasts(&self, symbol: &str) -> Result<Vec<PersistedForecast>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, symbol, revision_of, snapshot, outcomes, created_at
                 FROM forecasts WHERE symbol = ?1 ORDER BY created_at DESC",
            )
            .map_err(|e| format!("list forecasts: {e}"))?;
        let rows = stmt
            .query_map(params![symbol], row_to_persisted_forecast)
            .map_err(|e| format!("list forecasts: {e}"))?;
        rows.map(|row| row.map_err(|e| format!("forecast row: {e}").into()))
            .collect()
    }

    pub fn validate_forecast_revision(&self, id: &str, symbol: &str) -> Result<(), PortfolioError> {
        let Some(parent) = self.get_forecast(id)? else {
            return Err(format!("forecast '{id}' not found for this owner").into());
        };
        if parent.symbol != symbol {
            return Err(format!(
                "forecast '{id}' belongs to symbol '{}', not '{symbol}'",
                parent.symbol
            )
            .into());
        }
        Ok(())
    }

    pub fn record_forecast_outcome(
        &self,
        id: &str,
        outcome: serde_json::Value,
    ) -> Result<(), PortfolioError> {
        let mut forecast = self
            .get_forecast(id)?
            .ok_or_else(|| format!("forecast '{id}' not found for this owner"))?;
        forecast.outcomes.push(outcome);
        let outcomes = serde_json::to_string(&forecast.outcomes)
            .map_err(|e| format!("serialize forecast outcomes: {e}"))?;
        let conn = self.open()?;
        conn.execute(
            "UPDATE forecasts SET outcomes = ?1 WHERE id = ?2",
            params![outcomes, id],
        )
        .map_err(|e| format!("record forecast outcome: {e}"))?;
        Ok(())
    }

    // ── Companies-specific: notes ───────────────────────────────────

    pub fn add_note(
        &self,
        portfolio: &str,
        symbol: &str,
        date: &str,
        title: &str,
        body: &str,
        tags: &[String],
    ) -> Result<String, PortfolioError> {
        let conn = self.open()?;
        let id = uuid::Uuid::new_v4().to_string();
        let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
        let now = now_rfc3339();
        conn.execute(
            "INSERT INTO notes (id, portfolio_name, symbol, date, title, body, tags, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, portfolio, symbol, date, title, body, tags_json, now],
        )
        .map_err(|e| format!("add_note: {e}"))?;
        Ok(id)
    }

    pub fn list_notes(
        &self,
        portfolio: &str,
        symbol: &str,
        date_from: Option<&str>,
        date_to: Option<&str>,
        tags: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, PortfolioError> {
        let conn = self.open()?;
        let mut sql = "SELECT id, symbol, date, title, body, tags, created_at FROM notes WHERE portfolio_name = ?1 AND symbol = ?2".to_string();
        let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(portfolio.to_string()),
            Box::new(symbol.to_string()),
        ];

        if let Some(f) = date_from {
            bind_values.push(Box::new(f.to_string()));
            sql.push_str(&format!(" AND date >= ?{}", bind_values.len()));
        }
        if let Some(t) = date_to {
            bind_values.push(Box::new(t.to_string()));
            sql.push_str(&format!(" AND date <= ?{}", bind_values.len()));
        }
        sql.push_str(" ORDER BY date DESC");

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            bind_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let tags_str: String = row.get::<_, String>(5).unwrap_or_default();
                let parsed_tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "symbol": row.get::<_, String>(1)?,
                    "date": row.get::<_, String>(2)?,
                    "title": row.get::<_, String>(3)?,
                    "body": row.get::<_, String>(4)?,
                    "tags": parsed_tags,
                    "created_at": row.get::<_, String>(6)?,
                }))
            })
            .map_err(|e| format!("query: {e}"))?;

        let mut notes = Vec::new();
        for row in rows {
            let note = row.map_err(|e| format!("row: {e}"))?;
            if let Some(filter_tags) = tags {
                let note_tags: Vec<&str> = note["tags"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let has_any = filter_tags.iter().any(|t| note_tags.contains(&t.as_str()));
                if !has_any {
                    continue;
                }
            }
            notes.push(note);
        }
        Ok(notes)
    }

    pub fn delete_note(&self, note_id: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let rows = conn
            .execute("DELETE FROM notes WHERE id = ?1", params![note_id])
            .map_err(|e| format!("delete_note: {e}"))?;
        if rows == 0 {
            return Err(format!("note '{note_id}' not found").into());
        }
        Ok(())
    }

    // ── Companies-specific: file attachments ───────────────────────

    fn base_dir(&self) -> &std::path::Path {
        self.db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
    }

    pub fn attach_file(
        &self,
        portfolio: &str,
        symbol: &str,
        date: &str,
        filename: &str,
        mime_type: &str,
        data_b64: &str,
        notes: &str,
    ) -> Result<String, PortfolioError> {
        if data_b64.len() > MAX_ENCODED_ATTACHMENT_BYTES {
            return Err(format!(
                "encoded attachment exceeds maximum of {MAX_ENCODED_ATTACHMENT_BYTES} bytes"
            )
            .into());
        }
        let conn = self.open()?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data_b64)
            .map_err(|e| format!("invalid base64 data: {e}"))?;
        if bytes.len() > MAX_DECODED_ATTACHMENT_BYTES {
            return Err(format!(
                "decoded attachment exceeds maximum of {MAX_DECODED_ATTACHMENT_BYTES} bytes"
            )
            .into());
        }

        let id = uuid::Uuid::new_v4().to_string();
        let safe_filename = format!("{id}_{}", sanitize_name(filename));
        let files_dir = self.base_dir().join(portfolio).join("files");
        if let Err(error) = std::fs::create_dir_all(&files_dir) {
            tracing::warn!(target: "hkask.mcp.companies", %error, "failed to create files dir");
        }
        let file_path = files_dir.join(&safe_filename);

        std::fs::write(&file_path, &bytes).map_err(|e| format!("write file: {e}"))?;

        let path_str = file_path.to_string_lossy().to_string();
        let size = bytes.len() as i64;
        let now = now_rfc3339();

        if let Err(error) = conn.execute(
            "INSERT INTO files (id, portfolio_name, symbol, date, filename, mime_type, size, path, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![id, portfolio, symbol, date, filename, mime_type, size, path_str, notes, now],
        ) {
            if let Err(cleanup_error) = std::fs::remove_file(&file_path) {
                return Err(format!(
                    "attach_file: {error}; failed to remove written file '{}': {cleanup_error}",
                    file_path.display()
                ).into());
            }
            return Err(format!("attach_file: {error}").into());
        }

        Ok(id)
    }

    pub fn list_files(
        &self,
        portfolio: &str,
        symbol: &str,
    ) -> Result<Vec<serde_json::Value>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, symbol, date, filename, mime_type, size, path, notes, created_at FROM files WHERE portfolio_name = ?1 AND symbol = ?2 ORDER BY date DESC",
            )
            .map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params![portfolio, symbol], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "symbol": row.get::<_, String>(1)?,
                    "date": row.get::<_, String>(2)?,
                    "filename": row.get::<_, String>(3)?,
                    "mime_type": row.get::<_, String>(4)?,
                    "size": row.get::<_, i64>(5)?,
                    "path": row.get::<_, String>(6)?,
                    "notes": row.get::<_, String>(7)?,
                    "created_at": row.get::<_, String>(8)?,
                }))
            })
            .map_err(|e| format!("query: {e}"))?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(files)
    }

    pub fn delete_file(&self, file_id: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let path: String = conn
            .query_row(
                "SELECT path FROM files WHERE id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("lookup: {e}"))?;

        let rows = conn
            .execute("DELETE FROM files WHERE id = ?1", params![file_id])
            .map_err(|e| format!("delete_file: metadata deletion failed: {e}"))?;
        if rows == 0 {
            return Err(format!("delete_file: metadata for '{file_id}' was not found").into());
        }

        std::fs::remove_file(&path).map_err(|e| {
            format!("delete_file: metadata removed but failed to delete file '{path}': {e}")
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn updating_missing_screen_job_surfaces_lost_transition() -> Result<(), PortfolioError> {
        let directory = tempfile::tempdir()
            .map_err(|error| PortfolioError::from(format!("create temp directory: {error}")))?;
        let store = ResearchStore::with_dir(directory.path().to_path_buf())?;
        let error = match store.update_screen_job("missing-job", "failed", None, Some("failure")) {
            Ok(()) => return Err("missing screen job update must fail".into()),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            "update screen job: job \"missing-job\" was not found"
        );
        Ok(())
    }

    #[test]
    fn reopening_store_fails_only_interrupted_screen_jobs() -> Result<(), PortfolioError> {
        let directory = tempfile::tempdir()
            .map_err(|error| PortfolioError::from(format!("create temp directory: {error}")))?;
        let store = ResearchStore::with_dir(directory.path().to_path_buf())?;
        let definition = json!({"name":"restart-contract"});

        for (id, status) in [("queued-job", "queued"), ("executing-job", "executing")] {
            store.insert_screen_job(&ScreenJobRecord {
                id: id.to_string(),
                status: status.to_string(),
                definition: definition.clone(),
                result: None,
                error: None,
                created_at: "2026-09-13T00:00:00Z".to_string(),
                updated_at: "2026-09-13T00:00:00Z".to_string(),
            })?;
        }

        store.insert_screen_job(&ScreenJobRecord {
            id: "completed-job".to_string(),
            status: "queued".to_string(),
            definition,
            result: None,
            error: None,
            created_at: "2026-09-13T00:00:00Z".to_string(),
            updated_at: "2026-09-13T00:00:00Z".to_string(),
        })?;
        let completed_result = json!({"rows":[{"symbol":"TEST.US"}]});
        store.update_screen_job("completed-job", "completed", Some(&completed_result), None)?;
        drop(store);

        let reopened = ResearchStore::with_dir(directory.path().to_path_buf())?;
        for id in ["queued-job", "executing-job"] {
            let job = reopened
                .get_screen_job(id)?
                .ok_or_else(|| PortfolioError::from(format!("missing screen job {id}")))?;
            assert_eq!(job.status, "failed");
            assert_eq!(job.result, None);
            assert_eq!(job.error.as_deref(), Some(SCREEN_JOB_RESTART_ERROR));
        }

        let completed = reopened
            .get_screen_job("completed-job")?
            .ok_or_else(|| PortfolioError::from("missing completed screen job".to_string()))?;
        assert_eq!(completed.status, "completed");
        assert_eq!(completed.result, Some(completed_result));
        assert!(
            store
                .persist_screen_pass_set("atomic-job", &json!({"candidate_count":2}), &duplicate)
                .is_err()
        );
        let job = store
            .get_screen_job("atomic-job")?
            .ok_or_else(|| PortfolioError::from("missing atomic screen job".to_string()))?;
        assert_eq!(job.stage, "queued");
        assert_eq!(job.total, 0);
        assert!(store.pending_screen_items("atomic-job")?.is_empty());
        Ok(())
    }

    /// expect: Existing databases gain lifecycle columns and orphan issuer work is removed idempotently.
    #[test]
    fn lifecycle_migration_upgrades_existing_screen_tables() -> Result<(), PortfolioError> {
        let directory = tempfile::tempdir()
            .map_err(|error| PortfolioError::from(format!("create temp directory: {error}")))?;
        let db_path = directory.path().join("master.db");
        let conn = Connection::open(&db_path).map_err(|error| {
            PortfolioError::from(format!("open legacy screen database: {error}"))
        })?;
        conn.execute_batch(
            "CREATE TABLE screen_jobs (
                id TEXT PRIMARY KEY, status TEXT NOT NULL, definition TEXT NOT NULL,
                result TEXT, error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE screen_job_items (
                job_id TEXT NOT NULL, issuer_key TEXT NOT NULL, ordinal INTEGER NOT NULL,
                status TEXT NOT NULL, payload TEXT NOT NULL, row_json TEXT,
                error TEXT, updated_at TEXT NOT NULL,
                PRIMARY KEY (job_id, issuer_key)
             );
             INSERT INTO screen_job_items VALUES
                ('missing-job', 'issuer:orphan', 0, 'pending', '{}', NULL, NULL, '2026-09-13T00:00:00Z');",
        )
        .map_err(|error| PortfolioError::from(format!("seed legacy schema: {error}")))?;
        drop(conn);

        let store = ResearchStore::with_dir(directory.path().to_path_buf())?;
        assert!(store.pending_screen_items("missing-job")?.is_empty());
        drop(store);
        ResearchStore::with_dir(directory.path().to_path_buf())?;
        Ok(())
    }
}
