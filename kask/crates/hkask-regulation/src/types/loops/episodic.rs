//! Loop 2a: Episodic Memory — private, agent-scoped experience.
//!
//! Moved from hkask-regulation to hkask-types to break the circular dependency
//! that prevented extracting Regulation subcrates.

/// Classification of an episodic experience for encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceClassification {
    Success,
    Failure,
}

impl ExperienceClassification {
    pub fn default_confidence(&self) -> f64 {
        match self {
            ExperienceClassification::Success => 0.9,
            ExperienceClassification::Failure => 0.3,
        }
    }
}

impl std::fmt::Display for ExperienceClassification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExperienceClassification::Success => write!(f, "success"),
            ExperienceClassification::Failure => write!(f, "failure"),
        }
    }
}
