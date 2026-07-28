//! Kata execution state — runtime accumulator and output envelope.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::error::KataError;
use super::history::{ImprovementSignal, StepExperience};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KataState {
    pub step_outputs: HashMap<String, serde_json::Value>,
    pub learner_bot: String,
    pub context: HashMap<String, String>,
    pub gas_consumed: u64,
    pub current_step: usize,
    #[serde(default)]
    pub manifest_id: String,
    #[serde(default)]
    pub metric_before: Option<serde_json::Value>,
    #[serde(default)]
    pub metric_after: Option<serde_json::Value>,
    #[serde(default)]
    pub ik_state_ref: Option<String>,
    #[serde(default)]
    pub step_experiences: Vec<StepExperience>,
}

impl KataState {
    /// \[P9\] Motivating: Homeostatic Self-Regulation — kata state persisted for resume.
    /// pre:  self is a valid KataState; path is a writable filesystem location
    /// post: state serialized to JSON at path, or Err if serialization/write fails
    #[must_use = "result must be used"]
    pub fn save(&self, path: &std::path::Path) -> Result<(), KataError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                KataError::LoadFailed(format!(
                    "Failed to create save directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| KataError::LoadFailed(format!("Failed to serialize state: {}", e)))?;
        std::fs::write(path, &json).map_err(|e| {
            KataError::LoadFailed(format!(
                "Failed to write state to {}: {}",
                path.display(),
                e
            ))
        })?;
        Ok(())
    }

    /// \[P9\] Motivating: Homeostatic Self-Regulation — kata state restored on resume.
    /// pre:  path points to a valid KataState JSON file
    /// post: returns Ok(KataState) deserialized from file, or Err on I/O or parse failure
    #[must_use = "result must be used"]
    pub fn load(path: &std::path::Path) -> Result<Self, KataError> {
        let json = std::fs::read_to_string(path).map_err(|e| {
            KataError::LoadFailed(format!(
                "Failed to read state from {}: {}",
                path.display(),
                e
            ))
        })?;
        serde_json::from_str(&json)
            .map_err(|e| KataError::ParseFailed(format!("Failed to parse state: {}", e)))
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize)]
pub struct KataResult {
    pub manifest_id: String,
    pub kata_type: String,
    pub steps_completed: usize,
    pub total_steps: usize,
    pub gas_consumed: u64,
    pub gas_cap: u64,
    pub state: KataState,
    pub outcome: Option<String>,
    pub improvement_signal: Option<ImprovementSignal>,
    pub step_experiences: Vec<StepExperience>,
    pub automaticity_delta: Option<f64>,
}
