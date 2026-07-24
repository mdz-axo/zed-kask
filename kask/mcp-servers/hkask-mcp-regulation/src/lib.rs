#![forbid(unsafe_code)]
//! MCP server for hkask-regulation — regulation span history query tools.
//!
//! Exposes two tools for reading regulation regulation record history from the persistent
//! `RegulationArchive`:
//! - `reg_query_spans` — query events by span_category prefix within a time window
//! - `reg_span_stats`  — aggregate counts by span_category
//!
//! These tools are the runtime telemetry surface that the
//! `runtime-posture-monitor` skill consumes to observe `reg.guard.*`,
//! `reg.regulation`, and `hkask.*` performative spans.
//!
//! The stored `span_category` column holds the short name (e.g. "guard.input",
//! "regulation", "gas") — i.e. the `SpanNamespace::short_name()` with the
//! `reg.` prefix stripped. Callers pass the full `reg.*` namespace (e.g.
//! "reg.guard"); the server strips the `reg.` prefix before querying so the
//! `LIKE 'prefix%'` predicate hits the index on `(span_category, phase)`.
//!
//! Port-ified (T0.6): `RegulationArchive` is backed by `StorageDriver` from
//! `hkask_types::storage` (the port). The concrete driver is provided by
//! `kask_bridge` over zed's `sqlez`. The original `hkask-storage` crate is
//! deleted per the architecture plan.

#![allow(unused_crate_dependencies)]

use hkask_mcp_server::DaemonClient;
use hkask_mcp_server::run_server;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::id::{EventID, WebID};
use hkask_types::storage::{DbRow, DbValue, StorageDriver, query_map};
use hkask_types::{InfrastructureError, RegulationSink};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

const SERVER_NAME: &str = "hkask-mcp-regulation";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

hkask_mcp_server::mcp_server!(
    pub struct RegulationServer {
        regulation_store: Option<Arc<RegulationArchive>>,
    }
);

// ── RegulationArchive (port-ified) ──────────────────────────────────────────
//
// Migrated from the deleted `hkask-storage` crate. Backed by `StorageDriver`
// (the port) instead of `rusqlite`/`SqliteDriver`. Only the two query methods
// needed by this server are ported; `persist` is provided so tests can seed
// the store. The full persistence path (with algedonic replay, cursors, etc.)
// lives in `hkask-regulation`'s runtime and is also port-ified separately.

/// Persistent store for regulation regulation records, backed by a `StorageDriver`.
///
/// Schema (`reg_records`, `reg_cursors`) is initialized idempotently in
/// `from_driver`. The stored `span_category` column holds the short name
/// (e.g. "guard.input", "regulation", "gas").
pub struct RegulationArchive {
    driver: Arc<dyn StorageDriver>,
}

impl RegulationArchive {
    /// Create a new store backed by the given driver, initializing the schema.
    pub fn from_driver(driver: Arc<dyn StorageDriver>) -> Self {
        let _ = driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS reg_records (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                observer_webid TEXT NOT NULL,
                span_category TEXT NOT NULL,
                span_path TEXT NOT NULL,
                phase TEXT NOT NULL,
                observation TEXT NOT NULL,
                regulation TEXT,
                outcome TEXT,
                recursion_depth INTEGER NOT NULL DEFAULT 0,
                parent_event TEXT,
                visibility TEXT NOT NULL DEFAULT 'internal'
            );
            CREATE INDEX IF NOT EXISTS idx_reg_records_timestamp ON reg_records(timestamp);
            CREATE INDEX IF NOT EXISTS idx_reg_records_span_category_phase ON reg_records(span_category, phase);
            CREATE TABLE IF NOT EXISTS reg_cursors (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );",
        );
        Self { driver }
    }

    /// Persist a regulation record. Implements `RegulationSink`.
    fn insert(&self, event: &RegulationRecord) -> Result<(), InfrastructureError> {
        let (span_category, span_path) = span_to_columns(&event.span);
        self.driver
            .execute(
                "INSERT INTO reg_records (id, timestamp, observer_webid, span_category, span_path, phase, observation, regulation, outcome, recursion_depth, parent_event, visibility)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                &[
                    DbValue::Text(event.id.to_string()),
                    DbValue::Text(event.timestamp.to_rfc3339()),
                    DbValue::Text(event.observer_webid.to_string()),
                    DbValue::Text(span_category.to_string()),
                    DbValue::Text(span_path.to_string()),
                    DbValue::Text(event.phase.as_str().to_string()),
                    DbValue::Text(
                        serde_json::to_string(&event.observation)
                            .map_err(|e| InfrastructureError::database(e.to_string()))?,
                    ),
                    event
                        .regulation
                        .as_ref()
                        .and_then(|v| serde_json::to_string(v).ok())
                        .map_or(DbValue::Null, DbValue::Text),
                    event
                        .outcome
                        .as_ref()
                        .and_then(|v| serde_json::to_string(v).ok())
                        .map_or(DbValue::Null, DbValue::Text),
                    DbValue::Integer(event.recursion_depth as i64),
                    event
                        .parent_event
                        .map_or(DbValue::Null, |e| DbValue::Text(e.to_string())),
                    DbValue::Text(event.visibility.clone()),
                ],
            )
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        Ok(())
    }

    /// Query events by span_category prefix (short name, e.g. "guard", "regulation", "gas").
    ///
    /// Returns events with `span_category LIKE 'prefix%'` since the given
    /// timestamp, ordered by timestamp ASC, limited to `limit` results.
    pub fn query_by_namespace(
        &self,
        namespace_prefix: &str,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        let since_str = since.to_rfc3339();
        let prefix_pattern = format!("{}%", namespace_prefix);
        let sql = "SELECT id, timestamp, observer_webid, span_category, span_path, phase, \
                   observation, regulation, outcome, recursion_depth, parent_event, visibility \
                   FROM reg_records \
                   WHERE timestamp > ?1 AND span_category LIKE ?2 \
                   ORDER BY timestamp ASC \
                   LIMIT ?3";
        let params: Vec<DbValue> = vec![
            DbValue::Text(since_str),
            DbValue::Text(prefix_pattern),
            DbValue::Integer(limit as i64),
        ];
        query_map(&*self.driver, sql, &params, |row| {
            row_to_regulation_record(row).map_err(|e| hkask_types::DbError::Database(e.to_string()))
        })
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    /// Count events by span_category prefix, grouped by exact span_category.
    ///
    /// Returns `Vec<(span_category, count)>` ordered by count DESC.
    pub fn query_span_stats(
        &self,
        namespace_prefix: &str,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<(String, u64)>, InfrastructureError> {
        let since_str = since.to_rfc3339();
        let prefix_pattern = format!("{}%", namespace_prefix);
        let sql = "SELECT span_category, COUNT(*) as cnt \
                   FROM reg_records \
                   WHERE timestamp > ?1 AND span_category LIKE ?2 \
                   GROUP BY span_category \
                   ORDER BY cnt DESC";
        let params: Vec<DbValue> = vec![DbValue::Text(since_str), DbValue::Text(prefix_pattern)];
        query_map(&*self.driver, sql, &params, |row| {
            let cat: String = row
                .get_str(0)
                .map_err(|e| hkask_types::DbError::Database(e.to_string()))?
                .to_string();
            let cnt: i64 = row
                .get_int(1)
                .map_err(|e| hkask_types::DbError::Database(e.to_string()))?;
            Ok((cat, cnt as u64))
        })
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }
}

impl RegulationSink for RegulationArchive {
    fn persist(&self, event: &RegulationRecord) -> Result<(), InfrastructureError> {
        Self::insert(self, event)
    }
}

// ── Row mapping helpers ─────────────────────────────────────────────────────

/// Reconstruct a `RegulationRecord` from a database row.
fn row_to_regulation_record(row: &DbRow) -> anyhow::Result<RegulationRecord> {
    let id: String = row.get_str(0).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();
    let timestamp_str: String = row.get_str(1).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();
    let observer_webid: String = row.get_str(2).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();
    let span_category: String = row.get_str(3).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();
    let span_path: String = row.get_str(4).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();
    let phase_str: String = row.get_str(5).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();
    let observation_str: String = row.get_str(6).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();
    let regulation_str: Option<String> = match row.get(7).map_err(|e| anyhow::anyhow!("{e}"))? {
        DbValue::Null => None,
        v => Some(v.as_text().map_err(|e| anyhow::anyhow!("{e}"))?.to_string()),
    };
    let outcome_str: Option<String> = match row.get(8).map_err(|e| anyhow::anyhow!("{e}"))? {
        DbValue::Null => None,
        v => Some(v.as_text().map_err(|e| anyhow::anyhow!("{e}"))?.to_string()),
    };
    let recursion_depth: i64 = row.get_int(9).map_err(|e| anyhow::anyhow!("{e}"))?;
    let parent_event: Option<String> = match row.get(10).map_err(|e| anyhow::anyhow!("{e}"))? {
        DbValue::Null => None,
        v => Some(v.as_text().map_err(|e| anyhow::anyhow!("{e}"))?.to_string()),
    };
    let visibility_str: String = row.get_str(11).map_err(|e| anyhow::anyhow!("{e}"))?.to_string();

    let timestamp = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_utc();
    // Reconstruct Span from stored category + path.
    let namespace_str = format!("reg.{}", span_category);
    let namespace = SpanNamespace::parse(&namespace_str)
        .unwrap_or_else(|| SpanNamespace::new("reg.gas").expect("reg.gas must be canonical"));
    // Extract the local path part after the namespace prefix.
    let ns_str = namespace.as_str();
    let local_path = if span_path.starts_with(ns_str)
        && span_path.len() > ns_str.len()
        && span_path.as_bytes().get(ns_str.len()) == Some(&b'.')
    {
        &span_path[ns_str.len() + 1..]
    } else {
        span_path.as_str()
    };
    let span = Span::new(namespace, local_path);
    let phase = CyclePhase::from_str(&phase_str);
    let observation: serde_json::Value =
        serde_json::from_str(&observation_str).map_err(|e| anyhow::anyhow!("{e}"))?;
    let regulation = regulation_str
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let outcome = outcome_str
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    Ok(RegulationRecord {
        id: EventID::from_uuid(uuid::Uuid::parse_str(&id).map_err(|e| anyhow::anyhow!("{e}"))?),
        timestamp,
        observer_webid: WebID::from_uuid(
            uuid::Uuid::parse_str(&observer_webid).map_err(|e| anyhow::anyhow!("{e}"))?,
        ),
        span,
        phase,
        observation,
        regulation,
        outcome,
        recursion_depth: recursion_depth as u8,
        parent_event: parent_event
            .map(|s| EventID::from_uuid(uuid::Uuid::parse_str(&s).unwrap_or_default())),
        visibility: visibility_str,
    })
}

/// Split a `Span` into the `(span_category, span_path)` columns.
fn span_to_columns(span: &Span) -> (&str, &str) {
    (span.namespace.short_name(), span.path.as_str())
}

// ── Request types ─────────────────────────────────────────────────

/// Request for `reg_query_spans`.
///
/// `namespace` is the full canonical regulation namespace prefix (e.g. "reg.guard",
/// "reg.outcome", "hkask"). The server strips the `reg.` prefix before
/// querying the `span_category` column, which stores short names.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuerySpansRequest {
    /// Canonical namespace prefix (e.g. "reg.guard", "reg.outcome", "hkask").
    /// Empty string is rejected with `invalid_argument`.
    namespace: String,
    /// Lookback window in hours (default 1.0).
    #[serde(default = "default_since_hours")]
    since_hours: f64,
    /// Maximum number of events to return (default 100).
    #[serde(default = "default_limit")]
    limit: u64,
}

fn default_since_hours() -> f64 {
    1.0
}

fn default_limit() -> u64 {
    100
}

/// Request for `reg_span_stats`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpanStatsRequest {
    /// Canonical namespace prefix (e.g. "reg.guard", "reg.outcome", "hkask").
    /// Empty string is rejected with `invalid_argument`.
    namespace: String,
    /// Lookback window in hours (default 1.0).
    #[serde(default = "default_since_hours")]
    since_hours: f64,
}

// ── Tools ──────────────────────────────────────────────────────────

#[tool_router(server_handler)]
impl RegulationServer {
    #[tool(
        description = "Query regulation record history by namespace prefix within a time window. Returns events ordered by timestamp ASC. Use 'reg.guard' for guard violations, 'reg.outcome' for regulation events, 'hkask' for performative telemetry."
    )]
    pub async fn reg_query_spans(&self, Parameters(req): Parameters<QuerySpansRequest>) -> String {
        execute_tool(self, "reg_query_spans", async {
            let namespace = req.namespace.trim();
            if namespace.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "namespace must be a non-empty string (e.g. \"reg.guard\", \"reg.outcome\", \"hkask\")",
                ));
            }
            let Some(ref store) = self.regulation_store else {
                return Err(McpToolError::permission_denied(
                    "RegulationArchive not available — set HKASK_DB_PATH and HKASK_DB_PASSPHRASE",
                ));
            };
            let since = chrono::Utc::now()
                - chrono::Duration::seconds((req.since_hours * 3600.0) as i64);
            // The stored span_category column holds the short name (e.g. "guard.input",
            // "regulation", "gas"). Strip the "reg." prefix so LIKE 'prefix%' hits the
            // (span_category, phase) index.
            let short_prefix = strip_reg_prefix(namespace);
            let events = store
                .query_by_namespace(short_prefix, since, req.limit)
                .map_err(|e| McpToolError::internal(format!("Regulation query failed: {e}")))?;
            let count = events.len();
            let serialized: Vec<serde_json::Value> = events
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id.to_string(),
                        "timestamp": e.timestamp.to_rfc3339(),
                        "observer_webid": e.observer_webid.to_string(),
                        "namespace": e.span.namespace.as_str(),
                        "path": e.span.path,
                        "phase": e.phase.as_str(),
                        "observation": e.observation,
                        "regulation": e.regulation,
                        "outcome": e.outcome,
                        "recursion_depth": e.recursion_depth,
                        "parent_event": e.parent_event.map(|id| id.to_string()),
                        "visibility": e.visibility,
                    })
                })
                .collect();
            Ok(serde_json::json!({
                "namespace": namespace,
                "since": since.to_rfc3339(),
                "limit": req.limit,
                "count": count,
                "events": serialized,
            }))
        })
        .await
    }

    #[tool(
        description = "Aggregate regulation regulation record counts by exact span_category within a namespace prefix and time window. Returns a JSON object mapping each span_category to its count, ordered by count DESC."
    )]
    pub async fn reg_span_stats(&self, Parameters(req): Parameters<SpanStatsRequest>) -> String {
        execute_tool(self, "reg_span_stats", async {
            let namespace = req.namespace.trim();
            if namespace.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "namespace must be a non-empty string (e.g. \"reg.guard\", \"reg.outcome\", \"hkask\")",
                ));
            }
            let Some(ref store) = self.regulation_store else {
                return Err(McpToolError::permission_denied(
                    "RegulationArchive not available — set HKASK_DB_PATH and HKASK_DB_PASSPHRASE",
                ));
            };
            let since = chrono::Utc::now()
                - chrono::Duration::seconds((req.since_hours * 3600.0) as i64);
            let short_prefix = strip_reg_prefix(namespace);
            let stats = store
                .query_span_stats(short_prefix, since)
                .map_err(|e| McpToolError::internal(format!("Regulation stats query failed: {e}")))?;
            let total: u64 = stats.iter().map(|(_, c)| *c).sum();
            let mut categories: HashMap<String, u64> = HashMap::new();
            for (cat, cnt) in stats {
                categories.insert(cat, cnt);
            }
            Ok(serde_json::json!({
                "namespace": namespace,
                "since": since.to_rfc3339(),
                "total_events": total,
                "categories": categories,
            }))
        })
        .await
    }
}

/// Strip the `reg.` prefix from a namespace so it matches the short-name
/// `span_category` column. Non-`reg.` namespaces (e.g. `hkask`) are returned
/// as-is so callers can query performative telemetry too.
fn strip_reg_prefix(namespace: &str) -> &str {
    namespace.strip_prefix("reg.").unwrap_or(namespace)
}

// ── Server startup ─────────────────────────────────────────────────────────

/// Open the RegulationArchive from the configured database.
///
/// Uses `ServerContext::open_database`, which delegates to `kask_bridge` for
/// the concrete `StorageDriver` (file-based opening requires the bridge —
/// T1.4). Returns `None` (graceful degradation) when the database cannot be
/// opened — the tools then return `permission_denied` so callers see a clear
/// message.
fn open_regulation_store(
    ctx: &hkask_mcp_server::server::ServerContext,
) -> Option<Arc<RegulationArchive>> {
    let driver = match ctx.open_database("HKASK_DB_PATH") {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                target: "hkask.mcp.regulation",
                error = %e,
                "Failed to open regulation database (kask_bridge not yet wired)"
            );
            return None;
        }
    };
    Some(Arc::new(RegulationArchive::from_driver(driver)))
}

pub async fn run(
    userpod: String,
    daemon_client: Option<DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    run_server(
        SERVER_NAME,
        SERVER_VERSION,
        |ctx: hkask_mcp_server::server::ServerContext| {
            let regulation_store = open_regulation_store(&ctx);
            Ok(RegulationServer::new(
                ctx.webid,
                userpod.clone(),
                daemon_client.clone(),
                regulation_store,
            ))
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PATH",
                "Path to the SQLCipher database holding the reg_records table",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PASSPHRASE",
                "SQLCipher encryption passphrase",
            ),
        ],
    )
    .await
}
