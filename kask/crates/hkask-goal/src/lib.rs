#![forbid(unsafe_code)]
//! Goal types — Cross-cutting infrastructure
//!
//! Goals are a minimal coordination substrate for multi-agent collaboration.
//! Multiple loops interact with goals: Curation evaluates them, Cybernetics
//! allocates energy, Communication coordinates agents around them.

//!
//! Goals are scoped by `&WebID`.

use std::fmt;

// SYSTEM_MAX_RECURSION (from hkask-capability) = 7
const MAX_NESTING: u8 = 7;
use chrono::{DateTime, Utc};
use hkask_types::GoalID;
use hkask_types::Visibility;
use hkask_types::WebID;
use serde::{Deserialize, Serialize};

/// Error returned when a goal state transition violates the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IllegalGoalTransition {
    pub from: GoalState,
    pub to: GoalState,
}

impl fmt::Display for IllegalGoalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "illegal goal state transition: {} → {}",
            self.from.as_str(),
            self.to.as_str()
        )
    }
}

impl std::error::Error for IllegalGoalTransition {}

// GoalState is defined in hkask-types (for SQL impls — Rust orphan rule).
// (GoalState lives in hkask-types for SQL impls — Rust orphan rule.)
use hkask_types::GoalState;

/// Goal criterion — completion condition (LLM-judged)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalCriterion {
    pub id: String,
    pub goal_id: GoalID,
    pub criterion_type: String,
    pub description: String,
    pub satisfied: bool,
}

impl GoalCriterion {
    #[must_use]
    pub fn new(goal_id: GoalID, criterion_type: &str, description: &str) -> Self {
        Self {
            id: format!("gc_{}", uuid::Uuid::new_v4().simple()),
            goal_id,
            criterion_type: criterion_type.to_string(),
            description: description.to_string(),
            satisfied: false,
        }
    }

    pub fn mark_satisfied(&mut self) {
        self.satisfied = true;
    }
}

/// Goal artifact — output produced while working toward goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalArtifact {
    pub id: String,
    pub goal_id: GoalID,
    pub artifact_ref: String,
    pub artifact_type: String,
    pub created_at: DateTime<Utc>,
}

impl GoalArtifact {
    #[must_use]
    pub fn new(goal_id: GoalID, artifact_ref: &str, artifact_type: &str) -> Self {
        Self {
            id: format!("ga_{}", uuid::Uuid::new_v4().simple()),
            goal_id,
            artifact_ref: artifact_ref.to_string(),
            artifact_type: artifact_type.to_string(),
            created_at: Utc::now(),
        }
    }
}

/// Goal — minimal coordination substrate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: GoalID,
    pub webid: WebID,
    pub text: String,
    pub state: GoalState,
    pub visibility: Visibility,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub parent_goal_id: Option<GoalID>,
    pub depth: u8,
    pub display_name: Option<String>,
}

impl Goal {
    #[must_use]
    pub fn new(webid: WebID, text: &str, visibility: Visibility) -> Self {
        Self {
            id: GoalID::new(),
            webid,
            text: text.to_string(),
            state: GoalState::Pending,
            visibility,
            created_at: Utc::now(),
            completed_at: None,
            parent_goal_id: None,
            depth: 0,
            display_name: None,
        }
    }

    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    #[must_use]
    pub fn with_parent(mut self, parent_id: GoalID, parent_depth: u8) -> Self {
        self.parent_goal_id = Some(parent_id);
        self.depth = parent_depth + 1;
        self
    }

    #[must_use = "result must be used"]
    pub fn transition(&mut self, new_state: GoalState) -> Result<(), IllegalGoalTransition> {
        if !self.state.can_transition_to(new_state) {
            return Err(IllegalGoalTransition {
                from: self.state,
                to: new_state,
            });
        }
        if self.state != new_state {
            self.state = new_state;
            if new_state.is_terminal() && self.completed_at.is_none() {
                self.completed_at = Some(Utc::now());
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn can_have_subgoals(&self) -> bool {
        !self.state.is_terminal() && self.depth < MAX_NESTING
    }
}
