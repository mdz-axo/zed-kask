//! RegulationArchive — Persistent storage for Regulation regulation records

use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use crate::define_driver_store;
use hkask_types::event::{CyclePhase, Span, SpanNamespace};
use hkask_types::id::{EventID, WebID};
use hkask_types::{InfrastructureError, RegulationRecord, RegulationSink};

/// Algedonic-significant span categories for Curation review.
///
/// These are the Regulation span namespaces that produce events requiring
/// Curation (Loop 5) attention: energy deficits, variety imbalances,
/// agent failures (the `pod` entry is historical — pods were removed
/// 2026-09-09; it remains so archived spans still classify), and
/// communication activity (Matrix messages, thread lifecycle).
///
/// Matched against the stored `span_category` column (which holds the
/// full `short_name()`). Wallet key-lifecycle entries removed 2026-08-30
/// with the deleted wallet module (219c74b180).
const ALGEDONIC_SPAN_CATEGORIES: &[&str] = &[
    "variety",
    "pod",
    "communication.message",
    "communication.thread",
    "outcome",
    "contract.violated",
];

define_driver_store!(RegulationArchive);

impl RegulationArchive {
    /// Initialize the reg_records and reg_cursors tables (idempotent).
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P3\] Motivating: Generative Space — reg_records schema
    /// post: reg_records and reg_cursors tables exist
    fn init_schema(
        driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>,
    ) -> Result<(), InfrastructureError> {
        driver.execute_batch(
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
            );"
        )?;
        tracing::info!(target: "hkask.storage", "RegulationArchive schema initialized");
        Ok(())
    }

    pub(crate) fn insert(&self, event: &RegulationRecord) -> Result<(), InfrastructureError> {
        self.insert_with_sql(
            "INSERT INTO reg_records (id, timestamp, observer_webid, span_category, span_path, phase, observation, regulation, outcome, recursion_depth, parent_event, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            event,
        )
        .map(|_| ())
    }

    fn insert_if_absent(&self, event: &RegulationRecord) -> Result<bool, InfrastructureError> {
        self.insert_with_sql(
            "INSERT OR IGNORE INTO reg_records (id, timestamp, observer_webid, span_category, span_path, phase, observation, regulation, outcome, recursion_depth, parent_event, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            event,
        )
        .map(|rows| rows == 1)
    }

    fn insert_with_sql(
        &self,
        sql: &str,
        event: &RegulationRecord,
    ) -> Result<usize, InfrastructureError> {
        let (span_category, span_path) = span_to_columns(&event.span);
        self.driver
            .execute(
                sql,
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
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    /// Delete regulation records older than the given cutoff timestamp.
    ///
    /// Bounds the growth of the `reg_records` table in long-running sessions.
    /// Called by the maintenance tick (wired in the composition root) on a
    /// configurable cadence (default: daily, retaining 30 days of history).
    ///
    /// Returns the number of rows deleted.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — retention prevents unbounded growth
    /// pre:  `before` is a valid timestamp
    /// post: records older than `before` are deleted; returns count of deleted rows
    pub fn delete_older_than(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, InfrastructureError> {
        let before_str = before.to_rfc3339();
        let count = self.driver.execute(
            "DELETE FROM reg_records WHERE timestamp < ?1",
            &[DbValue::Text(before_str)],
        )?;
        if count > 0 {
            tracing::info!(
                target: "hkask.storage",
                deleted = count,
                cutoff = %before.to_rfc3339(),
                "RegulationArchive retention: deleted old reg_records"
            );
        }
        Ok(count)
    }

    /// Delete regulation records older than the given cutoff within one span
    /// path prefix (the exact path or dot-delimited descendants, mirroring
    /// `query_records`'s namespace predicate).
    ///
    /// Per-span retention: the `reg.tool.completed` per-call telemetry is 98%
    /// the regulation loop's own 10s sensing polls (measured 2026-09-30:
    /// 7,245 each of kanban_task_list / kanban_board_list per day vs ~300 real
    /// agent calls), so it ages out at 7 days while audit spans keep
    /// `delete_older_than`'s uniform 30-day cutoff. The aggregates the sensors
    /// read are journaled separately under `reg.outcome.*`.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — per-span retention bounds telemetry volume
    /// pre:  `before` is a valid timestamp
    /// post: records older than `before` under `span_path` are deleted; returns count of deleted rows
    pub fn delete_older_than_in_span(
        &self,
        before: chrono::DateTime<chrono::Utc>,
        span_path: &str,
    ) -> Result<usize, InfrastructureError> {
        let before_str = before.to_rfc3339();
        let count = self.driver.execute(
            "DELETE FROM reg_records WHERE timestamp < ?1 \
               AND (span_path = ?2 OR substr(span_path, 1, length(?2) + 1) = ?2 || '.')",
            &[
                DbValue::Text(before_str),
                DbValue::Text(span_path.to_string()),
            ],
        )?;
        if count > 0 {
            tracing::info!(
                target: "hkask.storage",
                deleted = count,
                cutoff = %before.to_rfc3339(),
                span = %span_path,
                "RegulationArchive retention: deleted old reg_records in span"
            );
        }
        Ok(count)
    }

    /// Run a passive WAL checkpoint, reclaim free pages, and analyze indices.
    ///
    /// Wraps the same PRAGMA sequence as `Database::checkpoint()` but runs it
    /// through the driver's connection pool (the `Database` struct is dropped
    /// after pool extraction in `open_regulation_archive`, so the archive
    /// holds only the driver). Called by the maintenance tick on a slow
    /// cadence (default: every 5 minutes) to prevent WAL checkpoint
    /// starvation under long-lived readers and vec0 shadow-table bloat from
    /// re-embedding churn.
    ///
    /// `incremental_vacuum` reclaims pages freed by vec0 DELETE operations
    /// (shadow tables are not reclaimed by ordinary VACUUM). `PRAGMA optimize`
    /// refreshes index statistics.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — checkpoint prevents storage bloat
    /// post: WAL checkpointed, free pages reclaimed, index stats refreshed
    pub fn checkpoint(&self) -> Result<(), InfrastructureError> {
        self.driver
            .execute_batch(
                "PRAGMA wal_checkpoint(PASSIVE);
                 PRAGMA incremental_vacuum;
                 PRAGMA optimize;",
            )
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
        tracing::debug!(
            target: "hkask.storage",
            "RegulationArchive checkpoint completed (WAL + incremental_vacuum + optimize)"
        );
        Ok(())
    }

    /// Load a persisted loop cursor value.
    ///
    /// Returns `Ok(None)` if no cursor has been persisted for the given key
    /// (e.g., first run after schema creation).
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P3\] Motivating: Generative Space — load replay cursor
    /// pre:  key is non-empty
    /// post: returns Some(value) if cursor exists, None otherwise
    pub fn load_cursor(&self, key: &str) -> Result<Option<i64>, InfrastructureError> {
        query_row(
            &*self.driver,
            "SELECT value FROM reg_cursors WHERE key = ?1",
            &[DbValue::Text(key.to_string())],
            |row| row.get_int(0),
        )
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    /// Query Regulation records in chronological order.
    ///
    /// When `namespace` is present, it matches either the exact stored path or
    /// dot-delimited descendants. The namespace predicate is evaluated in SQL
    /// before `limit`, so unrelated earlier records cannot consume the result
    /// budget.
    pub fn query_records(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        namespace: Option<&str>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        let since = DbValue::Text(since.to_rfc3339());
        let limit = DbValue::Integer(limit as i64);
        let (sql, params) = if let Some(namespace) = namespace {
            (
                "SELECT id, timestamp, observer_webid, span_category, span_path, phase, \
                 observation, regulation, outcome, recursion_depth, parent_event, visibility \
                 FROM reg_records \
                 WHERE timestamp > ?1 \
                   AND (span_path = ?2 \
                        OR substr(span_path, 1, length(?2) + 1) = ?2 || '.') \
                 ORDER BY timestamp ASC \
                 LIMIT ?3",
                vec![since, DbValue::Text(namespace.to_string()), limit],
            )
        } else {
            (
                "SELECT id, timestamp, observer_webid, span_category, span_path, phase, \
                 observation, regulation, outcome, recursion_depth, parent_event, visibility \
                 FROM reg_records \
                 WHERE timestamp > ?1 \
                 ORDER BY timestamp ASC \
                 LIMIT ?2",
                vec![since, limit],
            )
        };
        query_map(&*self.driver, sql, &params, |row| {
            row_to_regulation_record(row).map_err(|e| db_error(e.to_string()))
        })
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    /// Query algedonic signals from the event store.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — query algedonic signals
    /// post: returns Vec of algedonic signal events
    pub fn query_algedonic(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        self.query_algedonic_ordered(since, limit, "ASC")
    }

    /// Query the newest act-phase algedonic records for operational review.
    pub fn query_recent_algedonic(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        self.query_algedonic_ordered(since, limit, "DESC")
    }

    fn query_algedonic_ordered(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
        ordering: &str,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        let since_str = since.to_rfc3339();
        let placeholders: Vec<String> = ALGEDONIC_SPAN_CATEGORIES
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 2))
            .collect();
        let sql = format!(
            "SELECT id, timestamp, observer_webid, span_category, span_path, phase, \
             observation, regulation, outcome, recursion_depth, parent_event, visibility \
             FROM reg_records \
             WHERE timestamp > ?1 AND span_category IN ({}) AND phase = 'act' \
             ORDER BY timestamp {ordering} \
             LIMIT ?{}",
            placeholders.join(", "),
            ALGEDONIC_SPAN_CATEGORIES.len() + 2
        );
        let mut params: Vec<DbValue> = Vec::with_capacity(2 + ALGEDONIC_SPAN_CATEGORIES.len());
        params.push(DbValue::Text(since_str));
        for &category in ALGEDONIC_SPAN_CATEGORIES {
            params.push(DbValue::Text(category.to_string()));
        }
        params.push(DbValue::Integer(limit as i64));
        query_map(&*self.driver, &sql, &params, |row| {
            row_to_regulation_record(row).map_err(|e| db_error(e.to_string()))
        })
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    /// Load durable operator-feedback observations in chronological order.
    ///
    /// The caller rebuilds its bounded working view from these records after
    /// restart. Archive retention remains the durable history bound; the
    /// working `SkillSpanStore` applies its configured per-skill cap.
    pub fn query_operator_feedback(&self) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        query_map(
            &*self.driver,
            "SELECT id, timestamp, observer_webid, span_category, span_path, phase, \
             observation, regulation, outcome, recursion_depth, parent_event, visibility \
             FROM reg_records \
             WHERE span_category = 'skill' \
               AND span_path LIKE 'reg.skill.%.operator_feedback' \
             ORDER BY timestamp ASC",
            &[],
            |row| row_to_regulation_record(row).map_err(|e| db_error(e.to_string())),
        )
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }
}

/// Small helper to map string errors to DbError.
fn db_error(e: String) -> crate::database::types::DbError {
    crate::database::types::DbError::Database(e)
}

/// Reconstruct a RegulationRecord from a database row.
fn row_to_regulation_record(
    row: &crate::database::value::DbRow,
) -> anyhow::Result<RegulationRecord> {
    let id: String = row
        .get_str(0)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let timestamp_str: String = row
        .get_str(1)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let observer_webid: String = row
        .get_str(2)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let span_category: String = row
        .get_str(3)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let span_path: String = row
        .get_str(4)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let phase_str: String = row
        .get_str(5)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let observation_str: String = row
        .get_str(6)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
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
    let visibility_str: String = row
        .get_str(11)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let timestamp = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_utc();
    // Reconstruct Span from stored category + path
    let namespace_str = format!("reg.{}", span_category);
    let namespace = SpanNamespace::parse(&namespace_str).unwrap_or_else(|| {
        tracing::warn!(
            target: "reg.storage",
            namespace_str = %namespace_str,
            "Failed to parse span namespace from stored span_category — \
             defaulting to reg.inference. The stored span_category may be corrupt."
        );
        SpanNamespace::new("reg.inference").expect("reg.inference must be canonical")
    });
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
        parent_event: parent_event.map(|s| {
            EventID::from_uuid(uuid::Uuid::parse_str(&s).unwrap_or_else(|e| {
                tracing::warn!(
                    target: "reg.storage",
                    error = %e,
                    raw_uuid = %s,
                    "Failed to parse parent_event UUID — \
                     using nil UUID. The event DAG causality link may be broken."
                );
                uuid::Uuid::nil()
            }))
        }),
        visibility: visibility_str,
    })
}

fn span_to_columns(span: &Span) -> (&str, &str) {
    (span.namespace.short_name(), span.path.as_str())
}

impl RegulationSink for RegulationArchive {
    fn persist(&self, event: &RegulationRecord) -> Result<(), InfrastructureError> {
        self.insert(event)
    }

    fn persist_if_absent(
        &self,
        _source_event_id: &str,
        event: &RegulationRecord,
    ) -> Result<bool, InfrastructureError> {
        self.insert_if_absent(event)
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;

    fn record(category: &str, local_path: &str, days_ago: i64) -> RegulationRecord {
        let namespace = SpanNamespace::new(&format!("reg.{category}"))
            .unwrap_or_else(|| SpanNamespace::new("reg.tool").expect("reg.tool is canonical"));
        RegulationRecord {
            id: EventID::from_uuid(uuid::Uuid::new_v4()),
            timestamp: chrono::Utc::now() - chrono::Duration::days(days_ago),
            observer_webid: WebID::from_uuid(uuid::Uuid::new_v4()),
            span: Span::new(namespace, local_path),
            phase: CyclePhase::Act,
            observation: serde_json::json!({"test": local_path}),
            regulation: None,
            outcome: None,
            recursion_depth: 0,
            parent_event: None,
            visibility: "internal".to_string(),
        }
    }

    /// Per-span retention: the `reg.tool.completed` telemetry cutoff deletes
    /// ONLY old records in that span — an old audit-span record of the same
    /// age survives (it is gemba evidence), and a recent telemetry record
    /// survives. The span predicate matches the exact path or dot-delimited
    /// descendants, mirroring `query_records`.
    #[test]
    fn span_retention_deletes_only_old_records_in_the_named_span() -> anyhow::Result<()> {
        let archive = RegulationArchive::from_driver(SqliteDriver::in_memory_driver())?;
        archive.insert(&record("tool", "completed", 10))?; // old telemetry → deleted
        archive.insert(&record("tool", "completed", 1))?; // recent telemetry → survives
        archive.insert(&record("skill", "program-manager.outcome", 10))?; // old audit span → survives

        let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
        let deleted = archive.delete_older_than_in_span(cutoff, "reg.tool.completed")?;
        assert_eq!(deleted, 1, "only the old telemetry record is deleted");

        let since = chrono::Utc::now() - chrono::Duration::days(30);
        let telemetry = archive.query_records(since, Some("reg.tool.completed"), 10)?;
        assert_eq!(telemetry.len(), 1, "the recent telemetry record survives");
        let audit = archive.query_records(since, Some("reg.skill"), 10)?;
        assert_eq!(
            audit.len(),
            1,
            "the old audit-span record survives the span cutoff"
        );
        Ok(())
    }
}
