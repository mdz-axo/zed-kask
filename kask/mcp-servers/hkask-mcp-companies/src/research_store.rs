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
                CREATE INDEX IF NOT EXISTS idx_forecasts_symbol ON forecasts(symbol);";

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
        Ok(manager)
    }

    #[cfg(test)]
    pub(crate) fn with_dir(directory: PathBuf) -> Result<Self, PortfolioError> {
        let manager = Self {
            store: PortfolioStore::with_dir(directory.clone()),
            db_path: directory.join("master.db"),
        };
        manager.ensure_companies_schema()?;
        Ok(manager)
    }

    /// Open a connection to the same DB the store uses, for companies-specific
    /// tables (notes, files, forecasts).
    fn open(&self) -> Result<Connection, PortfolioError> {
        Connection::open(&self.db_path).map_err(|e| format!("db open: {e}").into())
    }

    fn ensure_companies_schema(&self) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        conn.execute_batch(COMPANIES_SCHEMA_DDL)
            .map_err(|e| format!("failed to initialize companies schema: {e}"))?;
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
