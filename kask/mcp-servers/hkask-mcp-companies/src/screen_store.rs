//! hKask MCP Companies — research store (companies-specific layer).
//!
//! Research notes and DCF forecast snapshots, keyed by stock symbol, in the
//! companies server's own owner-scoped SQLite database
//! (`mcp/companies/{owner}/master.db`). It exposes no portfolio ledger or
//! analytics reads.

use hkask_mcp_server::server::McpToolError;
use hkask_types::{WebID, agent_paths::sanitize_name, time::now_rfc3339};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;

/// Store-layer error. Each variant maps to a distinct `McpToolError` kind in
/// [`map_research_error`]:
/// - `InvalidArgument` → `invalid_argument` (caller input defect)
/// - `NotFound` → `not_found` (note or forecast does not exist)
/// - `Database` → `internal` (SQLite error — system failure)
/// - `Serialize` → `internal` (serialization failure — system bug)
#[derive(Debug, thiserror::Error)]
pub(crate) enum ResearchError {
    #[error("{0}")]
    InvalidArgument(String),
    #[error("{0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("serialize error: {0}")]
    Serialize(String),
}

impl From<rusqlite::Error> for ResearchError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e.to_string())
    }
}

impl From<serde_json::Error> for ResearchError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e.to_string())
    }
}

impl From<String> for ResearchError {
    fn from(s: String) -> Self {
        Self::InvalidArgument(s)
    }
}

impl From<&str> for ResearchError {
    fn from(s: &str) -> Self {
        Self::InvalidArgument(s.to_string())
    }
}

impl From<ResearchError> for hkask_mcp_server::McpError {
    fn from(e: ResearchError) -> Self {
        hkask_mcp_server::McpError::UnexpectedResponse {
            context: "companies research store".to_string(),
            detail: e.to_string(),
        }
    }
}

/// Classify [`ResearchError`] for MCP dispatch: each variant maps to a
/// distinct `McpToolError` kind so callers can distinguish "record does not
/// exist" from "database is broken" from "bad input".
pub(crate) fn map_research_error(e: ResearchError) -> McpToolError {
    match e {
        ResearchError::InvalidArgument(_) => McpToolError::invalid_argument(e.to_string()),
        ResearchError::NotFound(_) => McpToolError::not_found(e.to_string()),
        ResearchError::Database(_) | ResearchError::Serialize(_) => {
            McpToolError::internal(e.to_string())
        }
    }
}

/// DDL for the companies research tables (notes, forecasts) in the companies
/// server's own database. The `portfolio_name` column on notes is the
/// caller-supplied grouping key from the note tools' `portfolio` parameter —
/// a free-form label, not a reference to any portfolio entity.
///
/// Public because the portfolio crate's schema-recovery tests
/// (dev-dependency) seed the mixed-database scenario from this exact DDL so
/// the fixture cannot drift from the real schema; that crate's removal —
/// the companion change in this same change-set — retires this export. The
/// screening and file-attachment tables were removed with their tools
/// (2026-10-15); the DDL creates notes and forecasts.
pub const COMPANIES_SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS notes (
                    id TEXT PRIMARY KEY,
                    portfolio_name TEXT NOT NULL,
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

/// Resolve the SQLite DB path for an owner.
///
/// Databases live in the internal data dir. Path is
/// `{kask_data_dir}/mcp/companies/{owner}/master.db`.
fn resolve_db_path(owner: &WebID) -> Result<PathBuf, ResearchError> {
    let mut path = hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
        hkask_types::agent_paths::MCP_DIR,
    ))
    .join("companies");
    path.push(sanitize_name(&owner.to_string()));
    // A read-only data dir, a full disk, or a permissions error must surface
    // as an error the server can report, not abort the process at startup.
    std::fs::create_dir_all(&path).map_err(|e| {
        ResearchError::from(format!(
            "failed to create companies store directory {}: {e}",
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

/// Companies-side research store: research notes and DCF forecast snapshots
/// in the companies server's own owner-scoped database.
///
/// The DB is plaintext by recorded design decision (operator
/// ruling 2026-09-29, F-P1): MCP-server data stays unencrypted so this
/// server runs in an independent MCP-server pattern without the zed
/// keychain or HKASK_DB_PASSPHRASE. Same rationale as the media gallery
/// DB's site note.
#[derive(Clone)]
pub(crate) struct ResearchStore {
    /// Path to the owner-scoped research database.
    db_path: PathBuf,
}

impl ResearchStore {
    /// Creates storage scoped to the authenticated server owner: the
    /// database directory, the file, and the companies research schema
    /// (notes, forecasts).
    pub fn new(owner: WebID) -> Result<Self, ResearchError> {
        let db_path = resolve_db_path(&owner)?;
        let manager = Self { db_path };
        manager.ensure_companies_schema()?;
        Ok(manager)
    }

    #[cfg(test)]
    pub(crate) fn with_dir(directory: PathBuf) -> Result<Self, ResearchError> {
        std::fs::create_dir_all(&directory).map_err(|e| {
            ResearchError::from(format!(
                "failed to create store directory {}: {e}",
                directory.display()
            ))
        })?;
        let manager = Self {
            db_path: directory.join("master.db"),
        };
        manager.ensure_companies_schema()?;
        Ok(manager)
    }

    /// Open a connection to the research database.
    fn open(&self) -> Result<Connection, ResearchError> {
        let conn = Connection::open(&self.db_path).map_err(|e| format!("db open: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| format!("enable foreign keys: {e}"))?;
        Ok(conn)
    }

    fn ensure_companies_schema(&self) -> Result<(), ResearchError> {
        let conn = self.open()?;
        conn.execute_batch(COMPANIES_SCHEMA_DDL)
            .map_err(|e| format!("failed to initialize companies schema: {e}"))?;
        Ok(())
    }

    // ── Companies-specific: forecasts ──────────────────────────────

    pub fn save_forecast(&self, forecast: &PersistedForecast) -> Result<(), ResearchError> {
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

    pub fn get_forecast(&self, id: &str) -> Result<Option<PersistedForecast>, ResearchError> {
        let conn = self.open()?;
        conn.query_row(
            "SELECT id, symbol, revision_of, snapshot, outcomes, created_at FROM forecasts WHERE id = ?1",
            params![id],
            row_to_persisted_forecast,
        )
        .optional()
        .map_err(|e| format!("get forecast: {e}").into())
    }

    pub fn list_forecasts(&self, symbol: &str) -> Result<Vec<PersistedForecast>, ResearchError> {
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

    pub fn validate_forecast_revision(&self, id: &str, symbol: &str) -> Result<(), ResearchError> {
        let Some(parent) = self.get_forecast(id)? else {
            return Err(ResearchError::NotFound(format!(
                "forecast '{id}' not found for this owner"
            )));
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
    ) -> Result<(), ResearchError> {
        let mut forecast = self.get_forecast(id)?.ok_or_else(|| {
            ResearchError::NotFound(format!("forecast '{id}' not found for this owner"))
        })?;
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
    ) -> Result<String, ResearchError> {
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
    ) -> Result<Vec<serde_json::Value>, ResearchError> {
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

    pub fn delete_note(&self, note_id: &str) -> Result<(), ResearchError> {
        let conn = self.open()?;
        let rows = conn
            .execute("DELETE FROM notes WHERE id = ?1", params![note_id])
            .map_err(|e| format!("delete_note: {e}"))?;
        if rows == 0 {
            return Err(ResearchError::NotFound(format!(
                "note '{note_id}' not found"
            )));
        }
        Ok(())
    }
}

/// Push an optional bind value and its SQL fragment onto a dynamically
/// filtered query under construction.
fn push_optional_filter<T: rusqlite::types::ToSql + 'static>(
    bind_values: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    sql: &mut String,
    fragment: &str,
    value: Option<T>,
) {
    let Some(value) = value else { return };
    bind_values.push(Box::new(value));
    sql.push_str(&format!(" {fragment} ?{}", bind_values.len()));
}

/// Run a dynamically-filtered query built with [`push_optional_filter`]:
/// bind the boxed values, prepare, map every row through `map_row`, and
/// collect. Prepare and row-mapping failures surface as the same
/// `"query: …"` / `"row: …"` messages the store has always emitted.
fn query_all_rows<T>(
    conn: &rusqlite::Connection,
    sql: &str,
    bind_values: &[Box<dyn rusqlite::types::ToSql>],
    map_row: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, ResearchError> {
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(sql).map_err(|e| format!("query: {e}"))?;
    let rows = stmt
        .query_map(params_refs.as_slice(), map_row)
        .map_err(|e| format!("query: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("row: {e}"))?);
    }
    Ok(out)
}
