//! Pipeline manifest — data processing pipeline FlowDef type.
//!
//! Distinct from BundleManifest (skill composition). PipelineManifest defines
//! a linear data processing flow: extract → chunk → embed → QA → enrich → train.
//! Each step maps to an MCP tool or CLI subprocess with verification gates.
//!
//! Registered in `registry/manifests/` alongside skill manifests. Uses
//! PipelineState from `hkask-ports::pipeline_state` for checkpoint/resume.

use super::pipeline_runner::PipelineError;
use serde::{Deserialize, Serialize};

/// A verification gate for a pipeline step's output.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineVerifyGate {
    /// Field name in the step output to check.
    pub field: String,
    /// Minimum value (numeric or count).
    #[serde(default)]
    pub min: Option<serde_json::Value>,
    /// Exact value match.
    #[serde(default)]
    pub equals: Option<serde_json::Value>,
    /// Minimum string length.
    #[serde(default)]
    pub min_len: Option<usize>,
    /// Quality string (e.g., ">= 60% parseable JSON").
    #[serde(default)]
    pub quality: Option<String>,
}

/// Loop configuration for batch step execution.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineLoopConfig {
    /// Expression referencing previous step output for iteration.
    pub over: Option<String>,
    /// Number of concurrent executions.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// Bloom's taxonomy levels (for QA generation steps).
    #[serde(default)]
    pub bloom_levels: Option<Vec<String>>,
    /// Quality dimensions for rewrite steps.
    #[serde(default)]
    pub dimensions: Option<Vec<String>>,
}

fn default_concurrency() -> usize {
    4
}

/// A single step in a pipeline manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineStep {
    pub id: String,
    pub description: Option<String>,
    /// MCP tool name or CLI subcommand to execute.
    pub tool: String,
    /// Parameters passed to the tool.
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    /// Verification gate — step fails if output doesn't satisfy.
    #[serde(default)]
    pub verify: Option<PipelineVerifyGate>,
    /// Convergence threshold (1.0 = all-or-nothing, <1.0 = partial OK).
    #[serde(default = "default_converge")]
    pub converge: f64,
    /// P2: Affirmative consent required for billable operations (e.g., GPU training).
    #[serde(default)]
    pub requires_consent: bool,
    /// Loop configuration for batch operations.
    #[serde(default)]
    pub r#loop: Option<PipelineLoopConfig>,
    /// Store configuration — composite step that persists output.
    #[serde(default)]
    pub store: Option<PipelineStoreConfig>,
    /// Composite step: execute this after the main step.
    #[serde(default)]
    pub then: Option<Box<PipelineStep>>,
}

fn default_converge() -> f64 {
    1.0
}

/// Store configuration for composite persist steps.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineStoreConfig {
    pub tool: String,
    #[serde(default)]
    pub entity_prefix: Option<String>,
    #[serde(default)]
    pub db_path: Option<String>,
}

/// A pipeline manifest — top-level YAML structure.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipelineManifest {
    pub id: String,
    pub version: String,
    pub description: Option<String>,
    pub steps: Vec<PipelineStep>,
}

/// Output from executing a single pipeline step.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineStepOutput {
    pub step_id: String,
    pub status: String, // "complete", "skipped", "failed"
    pub output: serde_json::Value,
}

impl PipelineManifest {
    /// Verify a step's output against its verification gate.
    pub fn verify_output(
        step: &PipelineStep,
        output: &serde_json::Value,
    ) -> Result<(), PipelineError> {
        let Some(gate) = &step.verify else {
            return Ok(());
        };

        // Check min (numeric comparison)
        if let Some(ref min) = gate.min {
            let field_val = output.get(&gate.field).unwrap_or(&serde_json::Value::Null);
            let passes = match (min, field_val) {
                (serde_json::Value::Number(min_n), serde_json::Value::Number(val_n)) => min_n
                    .as_f64()
                    .and_then(|m| val_n.as_f64().map(|v| v >= m))
                    .unwrap_or(false),
                _ => false,
            };
            if !passes {
                return Err(PipelineError::VerificationFailed {
                    step_id: step.id.clone(),
                    message: format!(
                        "field '{}' value {:?} < min {:?}",
                        gate.field, field_val, min
                    ),
                });
            }
        }

        // Check min_len (string length)
        if let Some(min_len) = gate.min_len {
            let field_val = output
                .get(&gate.field)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if field_val.len() < min_len {
                return Err(PipelineError::VerificationFailed {
                    step_id: step.id.clone(),
                    message: format!(
                        "field '{}' length {} < min {}",
                        gate.field,
                        field_val.len(),
                        min_len
                    ),
                });
            }
        }

        // Check equals
        if let Some(ref expected) = gate.equals {
            let field_val = output.get(&gate.field).unwrap_or(&serde_json::Value::Null);
            if field_val != expected {
                return Err(PipelineError::VerificationFailed {
                    step_id: step.id.clone(),
                    message: format!(
                        "field '{}' value {:?} != expected {:?}",
                        gate.field, field_val, expected
                    ),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_capabilities_researcher_flowdef_parses() {
        let yaml = include_str!("../../../../corpus/pipeline-capabilities-researcher.yaml");
        let manifest: PipelineManifest =
            serde_yaml_neo::from_str(yaml).expect("authoritative corpus FlowDef must parse");

        // Structural assertions — not exact values that rot when the corpus evolves
        assert!(!manifest.id.is_empty(), "pipeline must have an id");
        assert!(!manifest.version.is_empty(), "pipeline must have a version");
        assert!(
            manifest.steps.len() >= 2,
            "pipeline must have at least 2 steps, got {}",
            manifest.steps.len()
        );
        for step in &manifest.steps {
            assert!(!step.tool.is_empty(), "every step must name a tool");
        }
        let training_step = manifest
            .steps
            .iter()
            .find(|step| step.tool == "training_submit")
            .expect("pipeline must have a training_submit step");
        let training_params = training_step
            .params
            .as_ref()
            .expect("training_submit must declare typed parameters");
        assert_eq!(
            training_params["params"]["lora"]["init_lora_weights"],
            "eva"
        );
        assert_eq!(training_params["params"]["lora"]["r"], 32);
        assert!(training_params.get("config_file").is_none());
        assert!(training_params.get("host").is_none());
        assert!(
            manifest.steps.iter().any(|s| s.tool.contains("corpus")),
            "pipeline must have a corpus processing step"
        );
    }

    #[test]
    fn verify_min_numeric() {
        let step = PipelineStep {
            id: "test".to_string(),
            description: None,
            tool: "test".to_string(),
            params: None,
            verify: Some(PipelineVerifyGate {
                field: "count".to_string(),
                min: Some(serde_json::json!(10)),
                equals: None,
                min_len: None,
                quality: None,
            }),
            converge: 1.0,
            requires_consent: false,
            r#loop: None,
            store: None,
            then: None,
        };
        let output = serde_json::json!({"count": 15});
        assert!(PipelineManifest::verify_output(&step, &output).is_ok());
        let output_fail = serde_json::json!({"count": 5});
        assert!(PipelineManifest::verify_output(&step, &output_fail).is_err());
    }
}
