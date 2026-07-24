//! RegulationArchive — Persistent storage for Regulation regulation records

use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use crate::define_driver_store;
use crate::now_rfc3339;
use hkask_types::event::{CyclePhase, Span, SpanCategory, SpanNamespace};
use hkask_types::id::{EventID, WebID};
use hkask_types::{InfrastructureError, RegulationRecord, RegulationSink};

/// Per-domain decay constants for weighted replay.
///
/// Each loop domain has its own `λ` (lambda) for exponential decay.
/// Half-life = ln(2)/λ. A Cybernetics half-life of ~5min (λ≈0.0023/s)
/// means events older than ~30min are negligible (weight < 0.001).
#[derive(Debug, Clone)]
pub struct DecayConfig {
    /// Cybernetics decay constant (1/s). Default: ln(2)/300 ≈ 0.00231 (5min half-life)
    pub cybernetics_lambda: f64,
    /// Curation decay constant (1/s). Default: ln(2)/900 ≈ 0.00077 (15min half-life)
    pub curation_lambda: f64,
    /// Inference decay constant (1/s). Default: ln(2)/120 ≈ 0.00578 (2min half-life)
    pub inference_lambda: f64,
    /// Episodic decay constant (1/s). Default: ln(2)/600 ≈ 0.00116 (10min half-life)
    pub episodic_lambda: f64,
    /// Minimum weight threshold — events below this are not replayed. Default: 0.001
    pub weight_threshold: f64,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            cybernetics_lambda: std::f64::consts::LN_2 / 300.0,
            curation_lambda: std::f64::consts::LN_2 / 900.0,
            inference_lambda: std::f64::consts::LN_2 / 120.0,
            episodic_lambda: std::f64::consts::LN_2 / 600.0,
            weight_threshold: 0.001,
        }
    }
}

/// A RegulationRecord with its computed replay weight.
#[derive(Debug, Clone)]
pub struct WeightedEvent {
    pub event: RegulationRecord,
    pub weight: f64,
}

/// Algedonic-significant span categories for Curation review.
///
/// These are the Regulation span namespaces that produce events requiring
/// Curation (Loop 5) attention: energy deficits, variety imbalances,
/// agent pod failures, wallet key lifecycle events, and communication
/// activity (Matrix messages, thread lifecycle).
///
/// Matched against the stored `span_category` column (which holds the
/// full `short_name()` — e.g., `"wallet.key_expired"`).
const ALGEDONIC_SPAN_CATEGORIES: &[&str] = &[
    "gas",
    "variety",
    "pod",
    "wallet.key_expired",
    "wallet.key_exhausted",
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
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
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
            );"
        );
        tracing::info!(target: "hkask.storage", "RegulationArchive schema initialized");
    }

    /// Replay events with exponentially decaying weights.
    ///
    /// Events are weighted by `exp(-λ · Δt)` where Δt is the time elapsed
    /// since the event, and λ is the per-domain decay constant. Events with
    /// weight below `config.weight_threshold` are excluded.
    ///
    /// The domain is determined from the event's span namespace:
    /// - "variety", "gas" → cybernetics_lambda
    /// - "curation", "spec" → curation_lambda
    /// - "inference" → inference_lambda
    /// - "agent_pod", "connector" → episodic_lambda
    /// - everything else → cybernetics_lambda (safe default)
    ///
    /// Replay events with temporal decay weighting.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P3\] Motivating: Generative Space — replay events with temporal decay
    /// pre:  observer is valid, category is valid, lookback_secs > 0
    /// post: returns `Vec<RegulationRecord>` within lookback window, weighted by recency
    pub fn replay_weighted(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
        config: &DecayConfig,
    ) -> Result<Vec<WeightedEvent>, InfrastructureError> {
        let events = self.query_algedonic(since, limit)?;
        let now = chrono::Utc::now();
        let weighted: Vec<WeightedEvent> = events
            .into_iter()
            .filter_map(|event| {
                let delta_secs = (now - event.timestamp).num_seconds().max(0) as f64;
                // F-SYN-009: typed dispatch via SpanCategory.
                let lambda = Self::lambda_for(event.span.namespace.category(), config);
                let weight = (-lambda * delta_secs).exp();
                if weight >= config.weight_threshold {
                    Some(WeightedEvent { event, weight })
                } else {
                    None
                }
            })
            .collect();
        Ok(weighted)
    }

    /// Returns the decay constant `λ` for a `SpanCategory`.
    ///
    /// Unknown categories fall back to `cybernetics_lambda`.
    /// The fallback is explicit at the type level via `SpanCategory::Unknown`.
    /// Get the decay lambda for a span category.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P3\] Motivating: Generative Space — get decay lambda for category
    /// pre:  category is a valid SpanCategory
    /// post: returns decay lambda from config or default
    pub fn lambda_for(category: SpanCategory, config: &DecayConfig) -> f64 {
        match category {
            SpanCategory::Cybernetics => config.cybernetics_lambda,
            SpanCategory::Curation => config.curation_lambda,
            SpanCategory::Inference => config.inference_lambda,
            SpanCategory::Episodic => config.episodic_lambda,
            SpanCategory::Wallet => config.cybernetics_lambda, // wallet ops are cybernetic (energy budget)
            SpanCategory::Unknown => config.cybernetics_lambda, // safe default
        }
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

    /// Persist a loop cursor value for crash recovery.
    ///
    /// Loop cursors (e.g., `curation_last_review_ms`) track the last-processed
    /// event timestamp. Persisting them ensures the system doesn't re-process
    /// all historical events after a restart.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P3\] Motivating: Generative Space — persist replay cursor
    /// pre:  key is non-empty
    /// post: cursor value stored
    pub fn persist_cursor(&self, key: &str, value: i64) -> Result<(), InfrastructureError> {
        self.driver
            .execute(
                "INSERT OR REPLACE INTO reg_cursors (key, value, updated_at) VALUES (?1, ?2, ?3)",
                &[
                    DbValue::Text(key.to_string()),
                    DbValue::Integer(value),
                    DbValue::Text(now_rfc3339()),
                ],
            )
            .map_err(|e| InfrastructureError::database(e.to_string()))?;
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

    /// Query events by span_category prefix (e.g., "reg.guard" matches "reg.guard.input",
    /// "reg.guard.output", etc.).
    ///
    /// The stored `span_category` column holds the short name (e.g., "guard.input",
    /// "regulation", "gas"). Callers pass the short-name prefix (e.g., "guard",
    /// "regulation", "gas") — NOT the full `reg.*` namespace.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — query Regulation span history
    /// pre:  `namespace_prefix` is a non-empty short-name prefix (e.g., "guard", "regulation", "gas")
    /// post: returns Vec of RegulationRecords with span_category starting with the prefix, since the given
    ///       timestamp, ordered by timestamp ASC, limited to `limit` results
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
            row_to_regulation_record(row).map_err(|e| db_error(e.to_string()))
        })
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    /// Count events by span_category prefix, grouped by exact span_category.
    ///
    /// The stored `span_category` column holds the short name (e.g., "guard.input",
    /// "regulation", "gas"). Callers pass the short-name prefix.
    ///
    /// expect: "The system provides durable storage for event data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — aggregate Regulation span stats
    /// pre:  `namespace_prefix` is a non-empty short-name prefix
    /// post: returns Vec of (span_category, count) tuples, ordered by count DESC
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
                .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?
                .to_string();
            let cnt: i64 = row
                .get_int(1)
                .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
            Ok((cat, cnt as u64))
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
        let since_str = since.to_rfc3339();
        // Build IN clause for algedonic span categories
        let placeholders: Vec<String> = ALGEDONIC_SPAN_CATEGORIES
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 2)) // ?2, ?3, ... (since is ?1)
            .collect();
        let sql = format!(
            "SELECT id, timestamp, observer_webid, span_category, span_path, phase, \
             observation, regulation, outcome, recursion_depth, parent_event, visibility \
             FROM reg_records \
             WHERE timestamp > ?1 AND span_category IN ({}) AND phase = 'act' \
             ORDER BY timestamp ASC \
             LIMIT ?{}",
            placeholders.join(", "),
            ALGEDONIC_SPAN_CATEGORIES.len() + 2
        );
        // Params: since, then each span category, then limit
        let mut params: Vec<DbValue> = Vec::with_capacity(2 + ALGEDONIC_SPAN_CATEGORIES.len());
        params.push(DbValue::Text(since_str));
        for &cat in ALGEDONIC_SPAN_CATEGORIES {
            params.push(DbValue::Text(cat.to_string()));
        }
        params.push(DbValue::Integer(limit as i64));
        query_map(&*self.driver, &sql, &params, |row| {
            row_to_regulation_record(row).map_err(|e| db_error(e.to_string()))
        })
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

impl hkask_types::LedgerStoragePort for RegulationArchive {
    fn query_algedonic(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        self.query_algedonic(since, limit)
    }

    fn replay_weighted(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
        config: &hkask_types::DecayConfig,
    ) -> Result<Vec<hkask_types::WeightedEvent>, InfrastructureError> {
        self.replay_weighted(since, limit, &map_config(config))
            .map(|events| {
                events
                    .into_iter()
                    .map(|we| hkask_types::WeightedEvent {
                        event: we.event,
                        weight: we.weight,
                    })
                    .collect()
            })
    }

    fn persist_cursor(&self, key: &str, value: i64) -> Result<(), InfrastructureError> {
        self.persist_cursor(key, value)
    }

    fn load_cursor(&self, key: &str) -> Result<Option<i64>, InfrastructureError> {
        self.load_cursor(key)
    }

    fn query_by_namespace(
        &self,
        namespace_prefix: &str,
        since: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError> {
        self.query_by_namespace(namespace_prefix, since, limit)
    }

    fn query_span_stats(
        &self,
        namespace_prefix: &str,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<(String, u64)>, InfrastructureError> {
        self.query_span_stats(namespace_prefix, since)
    }
}

/// Map from port-level DecayConfig to the local storage type.
fn map_config(config: &hkask_types::DecayConfig) -> DecayConfig {
    DecayConfig {
        cybernetics_lambda: config.cybernetics_lambda,
        curation_lambda: config.curation_lambda,
        inference_lambda: config.inference_lambda,
        episodic_lambda: config.episodic_lambda,
        weight_threshold: config.weight_threshold,
    }
}

#[cfg(test)]
mod tests {
    use hkask_types::event::{Span, SpanNamespace};

    #[test]
    fn local_path_extraction_does_not_panic_on_short_span_path() {
        let ns = SpanNamespace::new("reg.gas").unwrap();
        let span = Span::new(ns, "short");
        let (cat, path) = super::span_to_columns(&span);
        assert_eq!(cat, "gas");
        assert!(!path.is_empty());
    }

    #[test]
    fn local_path_extraction_does_not_panic_on_exact_namespace_match() {
        let ns = SpanNamespace::new("reg.gas").unwrap();
        let span = Span::new(ns.clone(), "reg.gas");
        let (cat, path) = super::span_to_columns(&span);
        assert_eq!(cat, "gas");
        assert!(!path.is_empty());
    }

    #[test]
    fn local_path_extraction_succeeds_on_well_formed_path() {
        let ns = SpanNamespace::new("reg.gas").unwrap();
        let span = Span::new(ns, "agent_pod.monitor");
        let (cat, path) = super::span_to_columns(&span);
        assert_eq!(cat, "gas");
        assert_eq!(path, "reg.gas.agent_pod.monitor");
    }

    #[test]
    fn persist_if_absent_accepts_a_regulation_record_only_once() {
        use crate::RegulationArchive;
        use crate::database::sqlite::SqliteDriver;
        use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink};
        use hkask_types::id::WebID;
        use std::sync::Arc;

        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let store = RegulationArchive::from_driver(Arc::new(SqliteDriver::new(pool)));
        let event = RegulationRecord::new(
            WebID::from_persona(b"listener"),
            Span::new(
                SpanNamespace::new("reg.communication.message").unwrap(),
                "observed",
            ),
            CyclePhase::Act,
            serde_json::json!({"source_event_id": "$event"}),
            0,
        );

        assert!(
            store
                .persist_if_absent("$event", &event)
                .expect("first observation persists")
        );
        assert!(
            !store
                .persist_if_absent("$event", &event)
                .expect("duplicate observation is ignored")
        );
    }

    #[test]
    fn communication_message_namespace_is_canonical() {
        let ns = SpanNamespace::new("reg.communication.message").unwrap();
        assert!(ns.as_str().contains("communication"));
    }

    #[test]
    fn communication_thread_namespace_is_canonical() {
        let ns = SpanNamespace::new("reg.communication.thread").unwrap();
        assert!(ns.as_str().contains("communication"));
    }

    #[test]
    fn communication_agent_namespace_is_canonical() {
        let ns = SpanNamespace::new("reg.communication.agent").unwrap();
        assert!(ns.as_str().contains("communication"));
    }

    #[test]
    fn communication_listener_namespace_is_canonical() {
        let ns = SpanNamespace::new("reg.communication.listener").unwrap();
        assert!(ns.as_str().contains("communication"));
    }

    #[test]
    fn communication_span_short_name_matches_algedonic_category() {
        let ns = SpanNamespace::new("reg.communication.message").unwrap();
        assert!(super::ALGEDONIC_SPAN_CATEGORIES.contains(&ns.short_name()));
    }

    #[test]
    fn replay_weighted_clamps_future_timestamps() {
        use crate::database::driver::DatabaseDriver;
        use crate::database::sqlite::SqliteDriver;
        use crate::{DecayConfig, RegulationArchive};
        use std::sync::Arc;

        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = SqliteDriver::new(pool);
        driver
            .execute_batch(
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
            CREATE TABLE IF NOT EXISTS reg_cursors (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );",
            )
            .unwrap();
        let store = RegulationArchive::from_driver(Arc::new(driver));
        let config = DecayConfig::default();

        let future = chrono::Utc::now() + chrono::Duration::days(1);
        let result = store.replay_weighted(future, 10, &config);
        // Should succeed — future timestamps should not panic in weight calculation
        assert!(result.is_ok());
    }
}
