//! Escalation Queue — Persistent queue for escalated alerts requiring human review.
//!
//! The escalation queue is a Cybernetics (Loop 6) algedonic regulation mechanism.
//! Governed by the Cybernetics loop, which receives CuratorDirectives from Curation
//! and escalation signals from algedonic variety deficit detection.
use crate::database::value::DbValue;
use crate::impl_from_db_error;
use chrono::{DateTime, Utc};
use hkask_types::time::now_rfc3339;
use hkask_types::{BotID, EscalationID, InfrastructureError, NotFound, TemplateID};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationEntry {
    pub id: EscalationID,
    pub template_id: TemplateID,
    pub bot_id: BotID,
    pub output: String,
    pub confidence: f64,
    pub retry_count: u32,
    pub error_context: String,
    pub created_at: DateTime<Utc>,
    pub status: EscalationStatus,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_by: Option<String>,
}
impl EscalationEntry {
    /// Create a pending escalation entry with auto-generated id, timestamps, and defaults.
    /// Create a pending escalation signal.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — create pending escalation entry
    /// post: returns EscalationSignal with Pending status
    pub fn pending(output: String, confidence: f64, error_context: String) -> Self {
        Self {
            id: EscalationID::new(),
            template_id: TemplateID::new(),
            bot_id: BotID::new(),
            output,
            confidence,
            retry_count: 0,
            error_context,
            created_at: Utc::now(),
            status: EscalationStatus::Pending,
            resolved_at: None,
            resolved_by: None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationStatus {
    Pending,
    Resolved,
    Dismissed,
}
pub struct EscalationQueue {
    driver: Arc<dyn crate::database::driver::DatabaseDriver>,
}
#[derive(Error, Debug)]
pub enum EscalationError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
    #[error("Escalation not found: {0}")]
    NotFound(NotFound),
}
impl_from_db_error!(EscalationError, Infra);
impl EscalationQueue {
    /// Create a new escalation queue backed by a driver.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — create escalation queue
    /// pre:  driver is a valid database driver
    /// post: returns EscalationQueue with schema initialized
    pub fn from_driver(
        driver: Arc<dyn crate::database::driver::DatabaseDriver>,
    ) -> Result<Self, EscalationError> {
        let queue = Self { driver };
        queue.init()?;
        Ok(queue)
    }
    fn init(&self) -> Result<(), EscalationError> {
        self.driver
            .execute_batch(
                r#"CREATE TABLE IF NOT EXISTS escalations (
                id TEXT PRIMARY KEY,
                template_id TEXT NOT NULL,
                bot_id TEXT NOT NULL,
                output TEXT NOT NULL,
                confidence REAL NOT NULL,
                retry_count INTEGER NOT NULL DEFAULT 0,
                error_context TEXT,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                resolved_at TEXT,
                resolved_by TEXT
            )
        "#,
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        Ok(())
    }
    /// Add an escalation entry.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — add escalation entry
    /// pre:  entry has valid domain and output
    /// post: entry inserted into escalations
    pub fn add(
        &self,
        template_id: TemplateID,
        bot_id: BotID,
        output: String,
        confidence: f64,
        retry_count: u32,
        error_context: String,
    ) -> Result<EscalationID, EscalationError> {
        let id = EscalationID::new();
        let now = now_rfc3339();
        self.driver
            .execute(
                r#"INSERT INTO escalations (id, template_id, bot_id, output, confidence, retry_count, error_context, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending')"#,
                &[
                    DbValue::Text(id.to_string()),
                    DbValue::Text(template_id.to_string()),
                    DbValue::Text(bot_id.as_uuid().to_string()),
                    DbValue::Text(output),
                    DbValue::Real(confidence),
                    DbValue::Integer(retry_count as i64),
                    DbValue::Text(error_context),
                    DbValue::Text(now),
                ],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        Ok(id)
    }
    /// List pending escalations.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — list pending escalations
    /// post: returns Vec of pending EscalationEntry
    #[must_use = "result must be used"]
    pub fn list_pending(&self) -> Result<Vec<EscalationEntry>, EscalationError> {
        let rows = self
            .driver
            .query(
                r#"SELECT id, template_id, bot_id, output, confidence, retry_count, error_context, created_at, status, resolved_at, resolved_by
             FROM escalations WHERE status = 'pending' ORDER BY created_at ASC"#,
                &[],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        rows.iter()
            .map(|row| {
                let created_at = DateTime::parse_from_rfc3339(row.get(7)?.as_text()?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| {
                        EscalationError::Infra(InfrastructureError::database("invalid created_at"))
                    })?;
                Ok(EscalationEntry {
                    id: row.get(0)?.as_text()?.parse().map_err(|e| {
                        EscalationError::Infra(InfrastructureError::database(format!(
                            "invalid escalation ID: {e}"
                        )))
                    })?,
                    template_id: row.get(1)?.as_text()?.parse().map_err(|e| {
                        EscalationError::Infra(InfrastructureError::database(format!(
                            "invalid template ID: {e}"
                        )))
                    })?,
                    bot_id: row.get(2)?.as_text()?.parse().map_err(|e| {
                        EscalationError::Infra(InfrastructureError::database(format!(
                            "invalid bot ID: {e}"
                        )))
                    })?,
                    output: row.get(3)?.as_text()?.to_string(),
                    confidence: row.get(4)?.as_real()?,
                    retry_count: row.get(5)?.as_int()? as u32,
                    error_context: row.get(6)?.as_text()?.to_string(),
                    created_at,
                    status: EscalationStatus::Pending,
                    resolved_at: None,
                    resolved_by: None,
                })
            })
            .collect()
    }
    /// Get an escalation by ID.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — get escalation by ID
    /// pre:  id is non-empty
    /// post: returns Some(entry) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn get(&self, id: &str) -> Result<Option<EscalationEntry>, EscalationError> {
        let rows = self
            .driver
            .query(
                "SELECT id, template_id, bot_id, output, confidence, retry_count, error_context, created_at, status, resolved_at, resolved_by
             FROM escalations WHERE id = ?1",
                &[DbValue::Text(id.to_string())],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        match rows.first() {
            None => Ok(None),
            Some(row) => {
                let status_str = row.get(8)?.as_text()?.to_string();
                let status = match status_str.as_str() {
                    "pending" => EscalationStatus::Pending,
                    "resolved" => EscalationStatus::Resolved,
                    "dismissed" => EscalationStatus::Dismissed,
                    _ => EscalationStatus::Pending,
                };
                let created_at = DateTime::parse_from_rfc3339(row.get(7)?.as_text()?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| {
                        EscalationError::Infra(InfrastructureError::database("invalid created_at"))
                    })?;
                let resolved_at = match row.get(9)? {
                    DbValue::Null => None,
                    v => DateTime::parse_from_rfc3339(v.as_text()?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok(),
                };
                let resolved_by = match row.get(10)? {
                    DbValue::Null => None,
                    v => Some(v.as_text()?.to_string()),
                };
                Ok(Some(EscalationEntry {
                    id: row
                        .get(0)?
                        .as_text()?
                        .parse()
                        .unwrap_or_else(|_| EscalationID::new()),
                    template_id: row
                        .get(1)?
                        .as_text()?
                        .parse()
                        .unwrap_or_else(|_| TemplateID::new()),
                    bot_id: row
                        .get(2)?
                        .as_text()?
                        .parse()
                        .unwrap_or_else(|_| BotID::new()),
                    output: row.get(3)?.as_text()?.to_string(),
                    confidence: row.get(4)?.as_real()?,
                    retry_count: row.get(5)?.as_int()? as u32,
                    error_context: row.get(6)?.as_text()?.to_string(),
                    created_at,
                    status,
                    resolved_at,
                    resolved_by,
                }))
            }
        }
    }
    /// Resolve an escalation.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — resolve escalation
    /// pre:  id is non-empty, resolved_by is non-empty
    /// post: escalation status set to Resolved
    pub fn resolve(&self, id: &str, resolved_by: &str) -> Result<(), EscalationError> {
        let now = now_rfc3339();
        let affected = self
            .driver
            .execute(
                r#"UPDATE escalations SET status = 'resolved', resolved_at = ?1, resolved_by = ?2 WHERE id = ?3"#,
                &[
                    DbValue::Text(now),
                    DbValue::Text(resolved_by.to_string()),
                    DbValue::Text(id.to_string()),
                ],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        if affected == 0 {
            return Err(EscalationError::NotFound(NotFound {
                entity_type: "escalation".to_string(),
                id: id.to_string(),
            }));
        }
        Ok(())
    }
    /// Dismiss an escalation.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — dismiss escalation
    /// pre:  id is non-empty, resolved_by is non-empty
    /// post: escalation status set to Dismissed
    pub fn dismiss(&self, id: &str, resolved_by: &str) -> Result<(), EscalationError> {
        let now = now_rfc3339();
        let affected = self
            .driver
            .execute(
                r#"UPDATE escalations SET status = 'dismissed', resolved_at = ?1, resolved_by = ?2 WHERE id = ?3"#,
                &[
                    DbValue::Text(now),
                    DbValue::Text(resolved_by.to_string()),
                    DbValue::Text(id.to_string()),
                ],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        if affected == 0 {
            return Err(EscalationError::NotFound(NotFound {
                entity_type: "escalation".to_string(),
                id: id.to_string(),
            }));
        }
        Ok(())
    }
    /// Get escalation statistics.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P8\] Motivating: Semantic Grounding — escalation statistics
    /// post: returns EscalationStats with counts by status
    #[must_use = "result must be used"]
    pub fn stats(&self) -> Result<EscalationStats, EscalationError> {
        let rows = self
            .driver
            .query(
                r#"SELECT
                COUNT(*) as total,
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending,
                SUM(CASE WHEN status = 'resolved' THEN 1 ELSE 0 END) as resolved,
                SUM(CASE WHEN status = 'dismissed' THEN 1 ELSE 0 END) as dismissed
             FROM escalations"#,
                &[],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        let row = rows.first().ok_or_else(|| {
            EscalationError::Infra(InfrastructureError::database("empty stats result"))
        })?;
        Ok(EscalationStats {
            total: row.get(0)?.as_int()?,
            pending: row.get(1)?.as_int()?,
            resolved: row.get(2)?.as_int()?,
            dismissed: row.get(3)?.as_int()?,
        })
    }
}
/// Aggregated stats over escalation queue.
///
/// The algedonic channel's value is inversely proportional to its traffic
/// (VSM algedonic paradox). Batching reduces noise while preserving signal fidelity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationBatch {
    pub id: EscalationID,
    pub entries: Vec<EscalationEntry>,
    pub domain: String,
    pub created_at: DateTime<Utc>,
    pub threshold: usize,
}
impl EscalationBatch {
    /// Create a new escalation summary.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — create escalation summary
    /// pre:  domain is non-empty, threshold > 0
    /// post: returns EscalationSummary
    pub fn new(entries: Vec<EscalationEntry>, domain: &str, threshold: usize) -> Self {
        Self {
            id: EscalationID::new(),
            entries,
            domain: domain.to_string(),
            created_at: Utc::now(),
            threshold,
        }
    }
    /// Generate a human-readable summary.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// \[P3\] Motivating: Generative Space — generate summary text
    /// post: returns summary string with counts and threshold info
    pub fn summary(&self) -> String {
        let count = self.entries.len();
        let domains: std::collections::HashSet<&str> = self
            .entries
            .iter()
            .map(|e| e.output.split(':').next().unwrap_or("unknown"))
            .collect();
        format!(
            "System attention required: {} escalation(s) across {} domain(s) [{}]",
            count,
            domains.len(),
            self.domain
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationStats {
    pub total: i64,
    pub pending: i64,
    pub resolved: i64,
    pub dismissed: i64,
}

// ── EscalationPort implementation ────────────────────────────────────

use hkask_types::escalation::EscalationPort;

impl EscalationPort for EscalationQueue {
    fn list_pending(
        &self,
    ) -> Result<Vec<hkask_types::escalation::EscalationEntry>, InfrastructureError> {
        self.list_pending()
            .map(|entries| entries.into_iter().map(|e| e.into()).collect())
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    fn get(
        &self,
        id: &str,
    ) -> Result<Option<hkask_types::escalation::EscalationEntry>, InfrastructureError> {
        self.get(id)
            .map(|opt| opt.map(|e| e.into()))
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    fn resolve(&self, id: &str, resolved_by: &str) -> Result<(), InfrastructureError> {
        self.resolve(id, resolved_by)
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    fn dismiss(&self, id: &str, resolved_by: &str) -> Result<(), InfrastructureError> {
        self.dismiss(id, resolved_by)
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    fn persist_batch(
        &self,
        _batch: &hkask_types::escalation::EscalationBatch,
    ) -> Result<(), InfrastructureError> {
        // Forward-compat: batch persistence not yet wired through the port.
        // Individual entries are added via `add()`.
        Ok(())
    }

    fn add(
        &self,
        template_id: TemplateID,
        bot_id: BotID,
        output: String,
        confidence: f64,
        retry_count: u32,
        error_context: String,
    ) -> Result<EscalationID, InfrastructureError> {
        self.add(
            template_id,
            bot_id,
            output,
            confidence,
            retry_count,
            error_context,
        )
        .map_err(|e| InfrastructureError::database(e.to_string()))
    }
}

impl From<EscalationEntry> for hkask_types::escalation::EscalationEntry {
    fn from(e: EscalationEntry) -> Self {
        Self {
            id: e.id,
            template_id: e.template_id,
            bot_id: e.bot_id,
            output: e.output,
            confidence: e.confidence,
            retry_count: e.retry_count,
            error_context: e.error_context,
            created_at: e.created_at,
            status: match e.status {
                EscalationStatus::Pending => hkask_types::escalation::EscalationStatus::Pending,
                EscalationStatus::Resolved => hkask_types::escalation::EscalationStatus::Resolved,
                EscalationStatus::Dismissed => hkask_types::escalation::EscalationStatus::Dismissed,
            },
            resolved_at: e.resolved_at,
            resolved_by: e.resolved_by,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_queue() -> EscalationQueue {
        let driver = crate::database::sqlite::SqliteDriver::in_memory_driver();
        EscalationQueue::from_driver(driver).expect("init queue")
    }
    #[test]
    fn resolve_missing_id_returns_not_found() {
        let q = make_queue();
        let result = q.resolve("no-such-id", "tester");
        assert!(
            matches!(result, Err(EscalationError::NotFound(_))),
            "expected NotFound, got {:?}",
            result
        );
    }
    #[test]
    fn dismiss_missing_id_returns_not_found() {
        let q = make_queue();
        let result = q.dismiss("no-such-id", "tester");
        assert!(
            matches!(result, Err(EscalationError::NotFound(_))),
            "expected NotFound, got {:?}",
            result
        );
    }
    #[test]
    fn resolve_existing_id_succeeds() {
        let q = make_queue();
        let id = q
            .add(
                TemplateID::new(),
                BotID::new(),
                "output".into(),
                0.9,
                0,
                "ctx".into(),
            )
            .expect("add escalation");
        assert!(q.resolve(&id.to_string(), "tester").is_ok());
    }
    #[test]
    fn dismiss_existing_id_succeeds() {
        let q = make_queue();
        let id = q
            .add(
                TemplateID::new(),
                BotID::new(),
                "output".into(),
                0.8,
                0,
                "ctx".into(),
            )
            .expect("add escalation");
        assert!(q.dismiss(&id.to_string(), "tester").is_ok());
    }
}
