//! Goal storage — transient coordination substrates.
//! Long-term retention lives in agent memory (episodic/semantic).
use crate::database::value::DbValue;
use crate::impl_from_db_error;
use crate::now_rfc3339;
use chrono::Utc;
use hkask_goal::{Goal, GoalArtifact, GoalCriterion};
use hkask_types::GoalID;
use hkask_types::GoalState;
use hkask_types::InfrastructureError;
use hkask_types::NotFound;
use hkask_types::event::RegulationSink;
use hkask_types::id::WebID;
use hkask_types::visibility::Visibility;
use std::sync::Arc;
use thiserror::Error;
/// Shared column list for all goal SELECT statements.
const GOAL_COLUMNS: &str = "id, webid, text, state, visibility, created_at, completed_at, parent_goal_id, depth, display_name";
#[derive(Debug, Error)]
pub enum GoalRepositoryError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
    #[error("Visibility denied: {0}")]
    VisibilityDenied(String),
    #[error("Goal not found: {0}")]
    NotFound(NotFound),
    #[error("Invalid goal state transition: {0}")]
    InvalidTransition(String),
    #[error("Subgoal depth exceeded: {0}")]
    MaxDepthExceeded(String),
    #[error("Corrupt goal data: {0}")]
    Corrupt(String),
    #[error("Quarantine failed: {0}")]
    QuarantineFailed(String),
}
impl_from_db_error!(GoalRepositoryError, Infra);
pub type Result<T> = std::result::Result<T, GoalRepositoryError>;
/// A goal moved to quarantine due to data corruption.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuarantinedGoal {
    pub id: GoalID,
    pub original_data: String,
    pub quarantine_reason: String,
    pub quarantined_at: chrono::DateTime<chrono::Utc>,
    pub repair_attempts: u32,
    pub repaired: bool,
}

pub struct SqliteGoalRepository {
    driver: std::sync::Arc<dyn crate::database::driver::DatabaseDriver>,
    /// Optional Regulation telemetry sink for observability.
    telemetry: Option<Arc<dyn RegulationSink>>,
}

impl SqliteGoalRepository {
    /// Create a new goal repository backed by the given driver.
    pub fn from_driver(
        driver: std::sync::Arc<dyn crate::database::driver::DatabaseDriver>,
    ) -> Self {
        Self {
            driver,
            telemetry: None,
        }
    }
    /// Attach a Regulation telemetry sink for observability.
    /// Enable Regulation telemetry for goal operations.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — attach Regulation telemetry
    /// post: returns Self with telemetry sink configured
    pub fn with_telemetry(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.telemetry = Some(sink);
        self
    }

    /// Access the underlying driver.
    pub fn driver(&self) -> &Arc<dyn crate::database::driver::DatabaseDriver> {
        &self.driver
    }

    fn quarantined_from_row(row: &crate::database::value::DbRow) -> Result<QuarantinedGoal> {
        Ok(QuarantinedGoal {
            id: row
                .get(0)?
                .as_text()?
                .parse()
                .map_err(|_| crate::database::types::DbError::Database("invalid goal id".into()))?,
            original_data: row.get(1)?.as_text()?.to_string(),
            quarantine_reason: row.get(2)?.as_text()?.to_string(),
            quarantined_at: chrono::DateTime::parse_from_rfc3339(row.get(3)?.as_text()?)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_default(),
            repair_attempts: row.get(4)?.as_int()? as u32,
            repaired: row.get(5)?.as_int()? != 0,
        })
    }

    fn load_goal(&self, goal_id: GoalID) -> Result<Goal> {
        let rows = self
            .driver
            .query(
                &format!("SELECT {GOAL_COLUMNS} FROM goals WHERE id = ?1"),
                &[DbValue::Text(goal_id.to_string())],
            )
            .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
        rows.first().map(Self::goal_from_row).ok_or_else(|| {
            GoalRepositoryError::NotFound(NotFound {
                entity_type: "goal".to_string(),
                id: goal_id.to_string(),
            })
        })?
    }

    /// Construct a Goal from a DbRow.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — parse Goal from DbRow
    /// post: returns Goal from row columns
    pub fn goal_from_row(row: &crate::database::value::DbRow) -> Result<Goal> {
        let id: GoalID = row
            .get(0)?
            .as_text()?
            .parse()
            .map_err(|_| GoalRepositoryError::Corrupt("invalid goal id".into()))?;
        let webid: WebID = row
            .get(1)?
            .as_text()?
            .parse()
            .map_err(|_| GoalRepositoryError::Corrupt("invalid webid".into()))?;
        let text: String = row.get(2)?.as_text()?.to_string();
        let state: GoalState = GoalState::parse_str(row.get(3)?.as_text()?)
            .ok_or_else(|| GoalRepositoryError::Corrupt("invalid goal state".to_string()))?;
        let visibility: Visibility = Visibility::parse_str(row.get(4)?.as_text()?)
            .ok_or_else(|| GoalRepositoryError::Corrupt("invalid visibility".into()))?;
        let created_at_raw = row.get(5)?.as_text()?.to_string();
        let completed_at_raw = row.get(6)?.as_text().ok().map(|s| s.to_string());
        let parent_goal_id: Option<GoalID> = match row.get(7)? {
            DbValue::Null => None,
            dbv => Some(
                dbv.as_text()?
                    .parse()
                    .map_err(|_| GoalRepositoryError::Corrupt("invalid parent_goal_id".into()))?,
            ),
        };
        let depth_i32: i32 = row.get(8)?.as_int()? as i32;
        let display_name: Option<String> = match row.get(9)? {
            DbValue::Null => None,
            dbv => Some(dbv.as_text()?.to_string()),
        };
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_raw)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| {
                GoalRepositoryError::Corrupt(format!("unparseable created_at: {created_at_raw:?}"))
            })?;
        let completed_at = completed_at_raw
            .map(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| {
                        GoalRepositoryError::Corrupt(format!("unparseable completed_at: {s:?}"))
                    })
            })
            .transpose()?;
        let depth = u8::try_from(depth_i32)
            .map_err(|_| GoalRepositoryError::Corrupt(format!("invalid depth: {depth_i32}")))?;
        Ok(Goal {
            id,
            webid,
            text,
            state,
            visibility,
            created_at,
            completed_at,
            parent_goal_id,
            depth,
            display_name,
        })
    }

    fn exec(&self, sql: &str, params: &[DbValue]) -> Result<()> {
        self.driver
            .execute(sql, params)
            .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
        Ok(())
    }
}

impl SqliteGoalRepository {
    /// Create a new goal.
    /// Create a new goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — create a goal
    /// pre:  webid is valid, text is non-empty
    /// post: goal created and returned
    pub fn create_goal(&self, webid: &WebID, text: &str, visibility: Visibility) -> Result<Goal> {
        let goal = Goal::new(*webid, text, visibility);
        // Persist created_at explicitly in RFC3339 so it round-trips through
        // the strict reader. The SQLite `datetime('now')` default produces a
        // non-RFC3339 string that the reader (correctly) rejects as corrupt.
        self.exec(
            "INSERT INTO goals (id, webid, text, state, visibility, depth, created_at, display_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            &[
                DbValue::Text(goal.id.to_string()),
                DbValue::Text(goal.webid.to_string()),
                DbValue::Text(goal.text.clone()),
                DbValue::Text(goal.state.as_str().to_string()),
                DbValue::Text(visibility.as_str().to_string()),
                DbValue::Integer(goal.depth as i64),
                DbValue::Text(goal.created_at.to_rfc3339()),
                goal.display_name.clone().map_or(DbValue::Null, DbValue::Text),
            ],
        )?;
        Ok(goal)
    }
    /// Get a goal by ID.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — get goal by ID
    /// pre:  goal_id is valid
    /// post: returns Some(Goal) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn get_goal(&self, goal_id: GoalID) -> Result<Option<Goal>> {
        self.load_goal(goal_id).map(Some).or_else(|e| {
            if matches!(e, GoalRepositoryError::NotFound(_)) {
                Ok(None)
            } else {
                Err(e)
            }
        })
    }
    /// Update a goal's state.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — update goal state
    /// pre:  goal_id is valid, state is valid
    /// post: goal state updated
    pub fn update_goal_state(&self, goal_id: GoalID, state: GoalState) -> Result<()> {
        let goal = self.load_goal(goal_id)?;
        if !goal.state.can_transition_to(state) {
            return Err(GoalRepositoryError::InvalidTransition(format!(
                "{} -> {} on {}",
                goal.state.as_str(),
                state.as_str(),
                goal_id
            )));
        }
        let completed_at = state.is_terminal().then(now_rfc3339);
        if let Some(completed) = completed_at {
            self.exec(
                "UPDATE goals SET state = ?1, completed_at = ?2 WHERE id = ?3",
                &[
                    DbValue::Text(state.as_str().to_string()),
                    DbValue::Text(completed),
                    DbValue::Text(goal_id.to_string()),
                ],
            )?;
        } else {
            self.exec(
                "UPDATE goals SET state = ?1 WHERE id = ?2",
                &[
                    DbValue::Text(state.as_str().to_string()),
                    DbValue::Text(goal_id.to_string()),
                ],
            )?;
        }
        Ok(())
    }
    /// List goals for a WebID with optional state filter.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — list goals for WebID
    /// pre:  webid is valid
    /// post: returns Vec of goals, optionally filtered by state
    #[must_use = "result must be used"]
    pub fn list_goals(&self, webid: &WebID, state_filter: Option<GoalState>) -> Result<Vec<Goal>> {
        let (sql, params) = match state_filter {
            Some(state) => (
                format!(
                    "SELECT {GOAL_COLUMNS} FROM goals WHERE webid = ?1 AND state = ?2 ORDER BY created_at DESC"
                ),
                vec![
                    DbValue::Text(webid.to_string()),
                    DbValue::Text(state.as_str().to_string()),
                ],
            ),
            None => (
                format!(
                    "SELECT {GOAL_COLUMNS} FROM goals WHERE webid = ?1 ORDER BY created_at DESC"
                ),
                vec![DbValue::Text(webid.to_string())],
            ),
        };
        let rows = self
            .driver
            .query(&sql, &params)
            .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
        rows.iter().map(Self::goal_from_row).collect()
    }
    /// Add a criterion to a goal.
    /// Add a criterion to a goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — add criterion to goal
    /// pre:  goal_id is valid, criterion has description
    /// post: criterion added to goal
    pub fn add_criterion(&self, goal_id: GoalID, criterion: GoalCriterion) -> Result<()> {
        // The criterion must target the goal named by the caller.
        if criterion.goal_id != goal_id {
            return Err(GoalRepositoryError::InvalidTransition(format!(
                "Criterion targets goal {} but operation named goal {}",
                criterion.goal_id, goal_id
            )));
        }
        let _goal = self.load_goal(goal_id)?;
        self.exec(
            "INSERT INTO goal_criteria (id, goal_id, type, description, satisfied) VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                DbValue::Text(criterion.id.to_string()),
                DbValue::Text(criterion.goal_id.to_string()),
                DbValue::Text(criterion.criterion_type),
                DbValue::Text(criterion.description),
                DbValue::Integer(criterion.satisfied as i64),
            ],
        )?;
        Ok(())
    }
    /// Add an artifact to a goal.
    /// Add an artifact to a goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — add artifact to goal
    /// pre:  goal_id is valid, artifact has content
    /// post: artifact added to goal
    pub fn add_artifact(&self, goal_id: GoalID, artifact: GoalArtifact) -> Result<()> {
        if artifact.goal_id != goal_id {
            return Err(GoalRepositoryError::InvalidTransition(format!(
                "Artifact targets goal {} but operation named goal {}",
                artifact.goal_id, goal_id
            )));
        }
        let _goal = self.load_goal(goal_id)?;
        self.exec(
            "INSERT INTO goal_artifacts (id, goal_id, artifact_ref, artifact_type, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                DbValue::Text(artifact.id.to_string()),
                DbValue::Text(artifact.goal_id.to_string()),
                DbValue::Text(artifact.artifact_ref),
                DbValue::Text(artifact.artifact_type),
                DbValue::Text(artifact.created_at.to_rfc3339()),
            ],
        )?;
        Ok(())
    }
    /// Get criteria for a goal.
    /// Get criteria for a goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — get criteria for goal
    /// pre:  goal_id is valid
    /// post: returns Vec of GoalCriterion
    pub fn get_criteria(&self, goal_id: GoalID) -> Result<Vec<GoalCriterion>> {
        let rows = self
            .driver
            .query(
                "SELECT id, goal_id, type, description, satisfied FROM goal_criteria WHERE goal_id = ?1",
                &[DbValue::Text(goal_id.to_string())],
            )
            .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
        rows.iter()
            .map(|row| {
                Ok(GoalCriterion {
                    id: row
                        .get(0)?
                        .as_text()?
                        .parse()
                        .map_err(|_| GoalRepositoryError::Corrupt("invalid criterion id".into()))?,
                    goal_id: row.get(1)?.as_text()?.parse().map_err(|_| {
                        GoalRepositoryError::Corrupt("invalid criterion goal_id".into())
                    })?,
                    criterion_type: row.get(2)?.as_text()?.to_string(),
                    description: row.get(3)?.as_text()?.to_string(),
                    satisfied: row.get(4)?.as_int()? != 0,
                })
            })
            .collect()
    }
    /// Get artifacts for a goal.
    /// Get artifacts for a goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — get artifacts for goal
    /// pre:  goal_id is valid
    /// post: returns Vec of GoalArtifact
    pub fn get_artifacts(&self, goal_id: GoalID) -> Result<Vec<GoalArtifact>> {
        let rows = self
            .driver
            .query(
                "SELECT id, goal_id, artifact_ref, artifact_type, created_at FROM goal_artifacts WHERE goal_id = ?1",
                &[DbValue::Text(goal_id.to_string())],
            )
            .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
        rows.iter()
            .map(|row| {
                let raw = row.get(4)?.as_text()?.to_string();
                let created_at = chrono::DateTime::parse_from_rfc3339(&raw)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| {
                        GoalRepositoryError::Corrupt(format!("corrupt artifact created_at '{raw}'"))
                    })?;
                Ok(GoalArtifact {
                    id: row
                        .get(0)?
                        .as_text()?
                        .parse()
                        .map_err(|_| GoalRepositoryError::Corrupt("invalid artifact id".into()))?,
                    goal_id: row.get(1)?.as_text()?.parse().map_err(|_| {
                        GoalRepositoryError::Corrupt("invalid artifact goal_id".into())
                    })?,
                    artifact_ref: row.get(2)?.as_text()?.to_string(),
                    artifact_type: row.get(3)?.as_text()?.to_string(),
                    created_at,
                })
            })
            .collect()
    }
    /// Create a subgoal under a parent goal.
    /// Create a subgoal under a parent goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — create subgoal
    /// pre:  parent_id is valid, text is non-empty
    /// post: subgoal created with depth = parent.depth + 1
    pub fn create_subgoal(
        &self,
        parent_id: GoalID,
        webid: &WebID,
        text: &str,
        visibility: Visibility,
    ) -> Result<Goal> {
        let parent = self.get_goal(parent_id)?.ok_or_else(|| {
            GoalRepositoryError::NotFound(NotFound {
                entity_type: "goal".to_string(),
                id: format!("Parent goal {} not found", parent_id),
            })
        })?;
        if !parent.can_have_subgoals() {
            return Err(GoalRepositoryError::MaxDepthExceeded(format!(
                "Parent goal at depth {} cannot have subgoals",
                parent.depth
            )));
        }
        let subgoal = Goal::new(*webid, text, visibility).with_parent(parent_id, parent.depth);
        self.exec(
            "INSERT INTO goals (id, webid, text, state, visibility, parent_goal_id, depth, created_at, display_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            &[
                DbValue::Text(subgoal.id.to_string()),
                DbValue::Text(subgoal.webid.to_string()),
                DbValue::Text(subgoal.text.clone()),
                DbValue::Text(subgoal.state.as_str().to_string()),
                DbValue::Text(subgoal.visibility.as_str().to_string()),
                DbValue::Text(parent_id.to_string()),
                DbValue::Integer(subgoal.depth as i64),
                DbValue::Text(subgoal.created_at.to_rfc3339()),
                subgoal.display_name.clone().map_or(DbValue::Null, DbValue::Text),
            ],
        )?;
        Ok(subgoal)
    }
    /// Get subgoals for a parent goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — list subgoals
    /// pre:  parent_id is valid
    /// post: returns Vec of child goals
    #[must_use = "result must be used"]
    pub fn get_subgoals(&self, parent_id: GoalID) -> Result<Vec<Goal>> {
        let rows = self
            .driver
            .query(
                &format!("SELECT {GOAL_COLUMNS} FROM goals WHERE parent_goal_id = ?1 ORDER BY created_at ASC"),
                &[DbValue::Text(parent_id.to_string())],
            )
            .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
        rows.iter().map(Self::goal_from_row).collect()
    }
    /// Delete a goal and its subgoals.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — delete goal and subgoals
    /// pre:  goal_id is valid
    /// post: goal and subgoals deleted
    pub fn delete_goal(&self, goal_id: GoalID) -> Result<()> {
        let _goal = self.load_goal(goal_id)?;
        self.exec(
            "DELETE FROM goals WHERE id = ?1",
            &[DbValue::Text(goal_id.to_string())],
        )?;
        Ok(())
    }
    /// Move a corrupted goal to the quarantine table.
    ///
    /// This removes the goal from the main `goals` table and inserts a forensic
    /// record into `quarantined_goals` for later repair or human review.
    /// The goal's current state is serialized into `original_data` so it can be
    /// restored during repair.
    /// Quarantine a goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — quarantine a goal
    /// pre:  goal_id is valid, reason is non-empty
    /// post: goal moved to quarantine
    pub fn quarantine_goal(&self, goal_id: GoalID, reason: &str) -> Result<()> {
        // Load the goal before removing it so we can snapshot its state.
        let goal = self.load_goal(goal_id)?;
        let original_data = serde_json::to_string(&goal).unwrap_or_default();
        let quarantine_result = (|| -> Result<()> {
            self.exec(
                "INSERT INTO quarantined_goals (id, original_data, quarantine_reason, quarantined_at, repair_attempts, repaired)
                 VALUES (?1, ?2, ?3, ?4, 0, 0)",
                &[
                    DbValue::Text(goal_id.to_string()),
                    DbValue::Text(original_data),
                    DbValue::Text(reason.to_string()),
                    DbValue::Text(now_rfc3339()),
                ],
            )?;
            self.exec(
                "DELETE FROM goals WHERE id = ?1",
                &[DbValue::Text(goal_id.to_string())],
            )?;
            Ok(())
        })();
        quarantine_result
            .map_err(|e: GoalRepositoryError| GoalRepositoryError::QuarantineFailed(e.to_string()))
    }
    /// Repair a quarantined goal.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — restore quarantined goal
    /// pre:  goal_id is valid
    /// post: goal restored from quarantine
    pub fn repair_quarantined_goal(
        &self,
        goal_id: GoalID,
        _event_sink: &dyn RegulationSink,
    ) -> Result<bool> {
        let quarantined = {
            let rows = self
                .driver
                .query(
                    "SELECT id, original_data, quarantine_reason, quarantined_at, repair_attempts, repaired FROM quarantined_goals WHERE id = ?1",
                    &[DbValue::Text(goal_id.to_string())],
                )
                .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
            match rows.first() {
                Some(row) => Self::quarantined_from_row(row)?,
                None => {
                    return Err(GoalRepositoryError::NotFound(NotFound {
                        entity_type: "goal".to_string(),
                        id: goal_id.to_string(),
                    }));
                }
            }
        };
        let goal: Goal = match serde_json::from_str(&quarantined.original_data) {
            Ok(goal) => goal,
            Err(_) => {
                self.exec(
                    "UPDATE quarantined_goals SET repair_attempts = repair_attempts + 1 WHERE id = ?1",
                    &[DbValue::Text(goal_id.to_string())],
                )
                .map_err(|e| GoalRepositoryError::QuarantineFailed(e.to_string()))?;
                return Ok(false);
            }
        };
        let repair_result = (|| -> Result<()> {
            self.exec(
                "INSERT INTO goals (id, webid, text, state, visibility, created_at, completed_at, parent_goal_id, depth, display_name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                &[
                    DbValue::Text(goal.id.to_string()),
                    DbValue::Text(goal.webid.to_string()),
                    DbValue::Text(goal.text.clone()),
                    DbValue::Text(goal.state.as_str().to_string()),
                    DbValue::Text(goal.visibility.as_str().to_string()),
                    DbValue::Text(goal.created_at.to_rfc3339()),
                    goal.completed_at.map_or(DbValue::Null, |dt| DbValue::Text(dt.to_rfc3339())),
                    goal.parent_goal_id.map_or(DbValue::Null, |id| DbValue::Text(id.to_string())),
                    DbValue::Integer(goal.depth as i64),
                    goal.display_name.clone().map_or(DbValue::Null, DbValue::Text),
                ],
            )?;
            self.exec(
                "UPDATE quarantined_goals SET repaired = 1, repair_attempts = repair_attempts + 1 WHERE id = ?1",
                &[DbValue::Text(goal_id.to_string())],
            )?;
            Ok(())
        })();
        repair_result.map_err(|e: GoalRepositoryError| {
            GoalRepositoryError::QuarantineFailed(e.to_string())
        })?;
        Ok(true)
    }
    /// List all quarantined goals.
    ///
    /// expect: "The system provides durable storage for goal data"
    /// \[P3\] Motivating: Generative Space — list quarantined goals
    /// post: returns Vec of QuarantinedGoal
    #[must_use = "result must be used"]
    pub fn list_quarantined_goals(&self) -> Result<Vec<QuarantinedGoal>> {
        let rows = self
            .driver
            .query(
                "SELECT id, original_data, quarantine_reason, quarantined_at, repair_attempts, repaired FROM quarantined_goals ORDER BY quarantined_at DESC",
                &[],
            )
            .map_err(|e| GoalRepositoryError::Infra(InfrastructureError::from(e)))?;
        rows.iter().map(Self::quarantined_from_row).collect()
    }
}
