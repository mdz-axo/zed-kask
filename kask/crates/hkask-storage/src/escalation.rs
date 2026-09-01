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
                        .unwrap_or_else(|e| {
                            tracing::warn!(target: "reg.storage", error = %e, "Failed to parse escalation ID from DB — using a fresh random ID. The returned entry will not match its DB row, breaking resolve/dismiss.");
                            EscalationID::new()
                        }),
                    template_id: row
                        .get(1)?
                        .as_text()?
                        .parse()
                        .unwrap_or_else(|e| {
                            tracing::warn!(target: "reg.storage", error = %e, "Failed to parse template_id from DB — using a fresh random ID.");
                            TemplateID::new()
                        }),
                    bot_id: row
                        .get(2)?
                        .as_text()?
                        .parse()
                        .unwrap_or_else(|e| {
                            tracing::warn!(target: "reg.storage", error = %e, "Failed to parse bot_id from DB — using a fresh random ID.");
                            BotID::new()
                        }),
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
    /// Check whether any pending escalation shares the given `output` string.
    ///
    /// Used for deduplication at the source: the regulation loop can sense the
    /// same deficit every cycle and would otherwise flood the queue with
    /// identical alerts. Calling this before `add` prevents runaway escalation
    /// floods when an efferent action is unwired or a deficit is persistent.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// post: returns true if at least one pending escalation has this output
    #[must_use = "result must be used"]
    pub fn has_pending_with_output(&self, output: &str) -> Result<bool, EscalationError> {
        let rows = self
            .driver
            .query(
                "SELECT COUNT(*) as cnt FROM escalations WHERE status = 'pending' AND output = ?1",
                &[DbValue::Text(output.to_string())],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        let count = rows
            .first()
            .and_then(|row| row.get(0).ok())
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0);
        Ok(count > 0)
    }

    /// Dismiss all pending escalations matching a given `output` string.
    ///
    /// Returns the number of escalations dismissed. Used by the
    /// `curator_escalation_dismiss_by_pattern` MCP tool to clear runaway
    /// floods from a single broken feedback loop in one operation, rather
    /// than dismissing each duplicate individually.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// pre:  output is non-empty, resolved_by is non-empty
    /// post: all pending escalations with this output are set to Dismissed
    #[must_use = "result must be used"]
    pub fn dismiss_pending_by_output(
        &self,
        output: &str,
        resolved_by: &str,
    ) -> Result<usize, EscalationError> {
        let now = now_rfc3339();
        let affected = self
            .driver
            .execute(
                r#"UPDATE escalations SET status = 'dismissed', resolved_at = ?1, resolved_by = ?2
             WHERE status = 'pending' AND output = ?3"#,
                &[
                    DbValue::Text(now),
                    DbValue::Text(resolved_by.to_string()),
                    DbValue::Text(output.to_string()),
                ],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        Ok(affected)
    }

    /// Resolve all pending escalations matching a given `output` string.
    ///
    /// Returns the number of escalations resolved. Used by the regulation
    /// loop's auto-resolve path: when `verify_impact` produces an `Accept`
    /// ImpactReport for a previously-escalated condition, the triggering
    /// deviation has cleared and the escalation is stale. This method resolves
    /// it without operator intervention, closing the stuck-loop pattern where
    /// a transient degradation self-resolves but the escalation sits in the
    /// queue until manual review.
    ///
    /// Mirrors `dismiss_pending_by_output` but sets status to `resolved`
    /// (condition cleared) rather than `dismissed` (not actionable).
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// pre:  output is non-empty, resolved_by is non-empty
    /// post: all pending escalations with this output are set to Resolved
    #[must_use = "result must be used"]
    pub fn resolve_pending_by_output(
        &self,
        output: &str,
        resolved_by: &str,
    ) -> Result<usize, EscalationError> {
        let now = now_rfc3339();
        let affected = self
            .driver
            .execute(
                r#"UPDATE escalations SET status = 'resolved', resolved_at = ?1, resolved_by = ?2
             WHERE status = 'pending' AND output = ?3"#,
                &[
                    DbValue::Text(now),
                    DbValue::Text(resolved_by.to_string()),
                    DbValue::Text(output.to_string()),
                ],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        Ok(affected)
    }

    /// Pending outputs whose condition key matches `condition` — exact
    /// match or `condition + " — "` prefix (the separator `alert_message`
    /// places between the stable reason and the per-cycle value), ordered
    /// oldest first. Matching on the condition (not the full output) keeps
    /// dedup effective for persistently re-sensed conditions whose embedded
    /// value changes every cycle.
    fn pending_outputs_matching_condition(
        &self,
        condition: &str,
    ) -> Result<Vec<String>, EscalationError> {
        let prefix = format!("{condition} — ");
        let rows = self
            .driver
            .query(
                "SELECT output FROM escalations WHERE status = 'pending' ORDER BY created_at ASC",
                &[],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        Ok(rows
            .iter()
            .filter_map(|row| row.get(0).ok().and_then(|v| v.as_text().ok()))
            .map(|output| output.to_string())
            .filter(|output| output == condition || output.starts_with(&prefix))
            .collect())
    }

    /// Check whether any pending escalation matches the given condition
    /// key (exact output, or output beginning with `condition + " — "`).
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// post: returns true if at least one pending escalation matches
    #[must_use = "result must be used"]
    pub fn has_pending_with_condition(&self, condition: &str) -> Result<bool, EscalationError> {
        Ok(!self
            .pending_outputs_matching_condition(condition)?
            .is_empty())
    }

    /// Supersede the oldest pending escalation matching `condition` with
    /// the latest alert data — update output/confidence/error_context in
    /// place and increment `retry_count` (which becomes the number of times
    /// the condition re-fired while pending). Returns `true` when a pending
    /// escalation was superseded; `false` when none exists (the caller
    /// inserts a new row).
    ///
    /// This is the append-to-update fix for escalation floods: a condition
    /// re-sensed every cycle updates ONE reviewable row instead of adding a
    /// row per cycle. The original `created_at` is preserved so triage sees
    /// when the condition first escalated; `retry_count` shows persistence.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// pre:  condition is non-empty
    /// post: the oldest matching pending escalation carries the latest
    ///       output/context and an incremented retry_count
    #[must_use = "result must be used"]
    pub fn supersede_pending_by_condition(
        &self,
        condition: &str,
        output: &str,
        confidence: f64,
        error_context: &str,
    ) -> Result<bool, EscalationError> {
        let Some(existing_output) = self
            .pending_outputs_matching_condition(condition)?
            .into_iter()
            .next()
        else {
            return Ok(false);
        };
        let affected = self
            .driver
            .execute(
                r#"UPDATE escalations SET output = ?1, confidence = ?2, error_context = ?3, retry_count = retry_count + 1
             WHERE status = 'pending' AND output = ?4"#,
                &[
                    DbValue::Text(output.to_string()),
                    DbValue::Real(confidence),
                    DbValue::Text(error_context.to_string()),
                    DbValue::Text(existing_output),
                ],
            )
            .map_err(|e| EscalationError::Infra(InfrastructureError::from(e)))?;
        Ok(affected > 0)
    }

    /// Resolve all pending escalations matching the given condition key.
    ///
    /// Condition-based counterpart of `resolve_pending_by_output`: when a
    /// triggering condition clears, the persisted escalation's embedded
    /// value differs from the clearing cycle's reconstruction (they were
    /// sensed in different cycles), so exact-output matching would miss it.
    /// Returns the number of escalations resolved.
    ///
    /// expect: "The system provides durable storage for escalation data"
    /// pre:  condition is non-empty, resolved_by is non-empty
    /// post: all pending escalations matching the condition are Resolved
    #[must_use = "result must be used"]
    pub fn resolve_pending_by_condition(
        &self,
        condition: &str,
        resolved_by: &str,
    ) -> Result<usize, EscalationError> {
        let mut resolved = 0;
        for output in self.pending_outputs_matching_condition(condition)? {
            resolved += self.resolve_pending_by_output(&output, resolved_by)?;
        }
        Ok(resolved)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;

    fn queue() -> EscalationQueue {
        let driver = SqliteDriver::in_memory_driver();
        EscalationQueue::from_driver(driver).expect("escalation queue init")
    }

    fn add_pending(queue: &EscalationQueue, output: &str) {
        queue
            .add(
                TemplateID::new(),
                BotID::new(),
                output.to_string(),
                1.0,
                0,
                "{}".to_string(),
            )
            .expect("add pending escalation");
    }

    /// Condition matching must span the per-cycle value: a pending
    /// "… — value 53 …" must match the condition of a "… — value 2149 …"
    /// re-sense. Exact-match dedup never hits for persistently re-sensed
    /// conditions — this is the flood mechanism this test pins shut.
    #[test]
    fn has_pending_with_condition_matches_prefix_not_value() {
        let queue = queue();
        add_pending(
            &queue,
            "variety_deficit_exceeded — value 53 exceeds threshold 20",
        );

        assert!(
            queue
                .has_pending_with_condition("variety_deficit_exceeded")
                .expect("has_pending_with_condition")
        );
        // A different condition must not match.
        assert!(
            !queue
                .has_pending_with_condition("tool_reliability_degraded")
                .expect("has_pending_with_condition")
        );
        // The " — " suffix prevents prefix collisions between conditions.
        assert!(
            !queue
                .has_pending_with_condition("variety_deficit")
                .expect("has_pending_with_condition")
        );
    }

    /// Supersede updates the oldest matching pending row in place (latest
    /// output, incremented retry_count) instead of appending a duplicate.
    #[test]
    fn supersede_updates_oldest_pending_and_increments_retry_count() {
        let queue = queue();
        add_pending(
            &queue,
            "variety_deficit_exceeded — value 53 exceeds threshold 20",
        );

        let superseded = queue
            .supersede_pending_by_condition(
                "variety_deficit_exceeded",
                "variety_deficit_exceeded — value 2149 exceeds threshold 20",
                1.0,
                "{\"deficit\":2149}",
            )
            .expect("supersede_pending_by_condition");
        assert!(superseded, "an existing pending row must be superseded");

        let pending = queue.list_pending().expect("list_pending");
        assert_eq!(pending.len(), 1, "supersede must not append a row");
        assert_eq!(
            pending[0].output,
            "variety_deficit_exceeded — value 2149 exceeds threshold 20"
        );
        assert_eq!(pending[0].retry_count, 1, "retry_count counts re-fires");
    }

    /// Supersede returns false (no row touched) when no pending escalation
    /// matches the condition — the caller then inserts.
    #[test]
    fn supersede_returns_false_without_pending_match() {
        let queue = queue();
        let superseded = queue
            .supersede_pending_by_condition("variety_deficit_exceeded", "any", 1.0, "{}")
            .expect("supersede_pending_by_condition");
        assert!(!superseded);
        assert_eq!(queue.list_pending().expect("list_pending").len(), 0);
    }

    /// Auto-resolve must clear every value-variant of the condition: the
    /// persisted escalation and the clearing cycle's reconstruction embed
    /// different values, so exact-output matching would leave the stale
    /// escalation pending forever.
    #[test]
    fn resolve_pending_by_condition_resolves_all_value_variants() {
        let queue = queue();
        add_pending(
            &queue,
            "variety_deficit_exceeded — value 53 exceeds threshold 20",
        );
        add_pending(
            &queue,
            "variety_deficit_exceeded — value 153 exceeds threshold 20",
        );

        let resolved = queue
            .resolve_pending_by_condition("variety_deficit_exceeded", "test:auto_resolve")
            .expect("resolve_pending_by_condition");
        assert_eq!(resolved, 2, "both value variants must resolve");
        assert_eq!(queue.list_pending().expect("list_pending").len(), 0);
    }
}
