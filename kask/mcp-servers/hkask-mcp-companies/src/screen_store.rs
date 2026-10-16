//! hKask MCP Companies — research store (companies-specific layer).
//!
//! Research notes, file attachments, and DCF forecast snapshots, keyed by
//! stock symbol. The general-purpose transaction ledger, holdings, and
//! returns live in the `hkask-mcp-portfolio` server; this store opens the
//! same owner database (`mcp/portfolio/{owner}`) only as the storage location
//! for companies-specific notes, files, forecasts, and screen jobs. It exposes
//! no portfolio ledger or analytics reads.

use hkask_mcp_portfolio::{PortfolioStore, push_optional_filter, query_all_rows};
// Re-exported for the tool layer's imports.
pub(crate) use hkask_mcp_portfolio::PortfolioError;
use hkask_types::{WebID, agent_paths::sanitize_name, time::now_rfc3339};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;

/// DDL for the companies-specific tables (notes, files, forecasts) that
/// live alongside the portfolio crate's schema in the same SQLite DB.
/// The portfolio crate owns the `portfolios`, `transactions`, `price_cache`,
/// `daily_holdings`, and `daily_returns` tables; this module owns the rest.
///
/// Public because the DB is genuinely shared: portfolio's schema-recovery
/// tests seed the mixed-database scenario from this exact DDL (via a
/// dev-dependency) so the fixture cannot drift from the real schema. The
/// screening and file-attachment tables were removed with their tools
/// (2026-10-15); the DDL now creates notes and forecasts.
pub const COMPANIES_SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS notes (
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

                CREATE TABLE IF NOT EXISTS forecasts (
                    id TEXT PRIMARY KEY,
                    symbol TEXT NOT NULL,
                    revision_of TEXT,
                    snapshot TEXT NOT NULL,
                    outcomes TEXT NOT NULL DEFAULT '[]',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_forecasts_symbol ON forecasts(symbol);";

/// Resolve the SQLite DB path for an owner, mirroring the portfolio crate's
/// `PortfolioStore::new` path resolution. The companies module opens its
/// own connection to the same DB for notes/forecasts tables.
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
///
/// The shared DB is plaintext by recorded design decision (operator
/// ruling 2026-09-29, F-P1): MCP-server data stays unencrypted so this
/// server runs in an independent MCP-server pattern without the zed
/// keychain or HKASK_DB_PASSPHRASE. Same rationale as the media gallery
/// DB's site note.
#[derive(Clone)]
pub(crate) struct ResearchStore {
    /// Path to the shared owner database for companies-specific research artifacts.
    db_path: PathBuf,
}

impl ResearchStore {
    /// Creates storage scoped to the authenticated server owner. The
    /// portfolio crate creates the DB and the general schema; this module
    /// adds the companies-specific tables (notes, forecasts) on top.
    pub fn new(owner: WebID) -> Result<Self, PortfolioError> {
        let portfolio_schema = PortfolioStore::new(owner)?;
        drop(portfolio_schema);
        let db_path = resolve_db_path(&owner)?;
        let manager = Self { db_path };
        manager.ensure_companies_schema()?;
        Ok(manager)
    }

    #[cfg(test)]
    pub(crate) fn with_dir(directory: PathBuf) -> Result<Self, PortfolioError> {
        let portfolio_schema = PortfolioStore::with_dir(directory.clone());
        drop(portfolio_schema);
        let manager = Self {
            db_path: directory.join("master.db"),
        };
        manager.ensure_companies_schema()?;
        Ok(manager)
    }

    /// Open a connection to the same DB the store uses, for companies-specific
    /// tables (notes, forecasts).
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

        push_optional_filter(
            &mut bind_values,
            &mut sql,
            "AND date >=",
            date_from.map(str::to_string),
        );
        push_optional_filter(
            &mut bind_values,
            &mut sql,
            "AND date <=",
            date_to.map(str::to_string),
        );
        sql.push_str(" ORDER BY date DESC");

        let mut notes = query_all_rows(&conn, &sql, &bind_values, |row| {
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
        })?;
        if let Some(filter_tags) = tags {
            notes.retain(|note| {
                let note_tags: Vec<&str> = note["tags"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                filter_tags.iter().any(|t| note_tags.contains(&t.as_str()))
            });
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
}
