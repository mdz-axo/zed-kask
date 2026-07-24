//! AdapterConfig — parses adapter_config.json from Hugging Face PEFT format.
//!
//! Every LoRA adapter directory contains `adapter_config.json` which describes
//! the adapter's rank, alpha, target modules, base model, and format version.
//! Providers validate this config before accepting the adapter for upload.

use serde::{Deserialize, Serialize};

/// Parsed contents of adapter_config.json (Hugging Face PEFT format).
///
/// vLLM uses this file to validate adapter compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterConfig {
    /// Base model name or path the adapter was trained on
    #[serde(alias = "base_model_name_or_path")]
    pub base_model_name_or_path: String,
    /// PEFT method type (e.g. "lora")
    pub peft_type: Option<String>,
    /// Task type (e.g. "CAUSAL_LM")
    pub task_type: Option<String>,
    /// LoRA rank (r) — dimension of the low-rank matrices
    pub r: Option<u32>,
    /// LoRA alpha — scaling factor
    pub lora_alpha: Option<f64>,
    /// Target modules where LoRA was applied (e.g. ["q_proj", "v_proj"])
    pub target_modules: Option<Vec<String>>,
    /// Whether the adapter uses DoRA (Weight-Decomposed Low-Rank Adaptation)
    pub use_dora: Option<bool>,
    /// LoftQ configuration (if quantized)
    pub loftq_config: Option<serde_json::Value>,
    /// Revision of the base model (if specified)
    pub revision: Option<String>,
    /// Any other fields we didn't explicitly model
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl AdapterConfig {
    /// Parse adapter_config.json from raw bytes.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// \[P8\] Semantic Grounding — adapter config carries training provenance
    /// pre:  bytes is valid JSON matching the PEFT adapter_config.json schema
    /// post: returns AdapterConfig with base_model_name_or_path populated
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AdapterConfigError> {
        serde_json::from_slice(bytes).map_err(AdapterConfigError::Parse)
    }

    /// Read adapter_config.json from a directory path.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// pre:  storage_path is a readable directory containing adapter_config.json
    /// post: returns AdapterConfig parsed from adapter_config.json
    pub fn from_dir(storage_path: &str) -> Result<Self, AdapterConfigError> {
        let config_path = std::path::Path::new(storage_path).join("adapter_config.json");
        let bytes = std::fs::read(&config_path).map_err(|e| AdapterConfigError::Io {
            path: config_path.display().to_string(),
            error: e.to_string(),
        })?;
        Self::from_bytes(&bytes)
    }

    /// Validate that this adapter config is compatible with the expected base model.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// pre:  expected_family is non-empty
    /// post: returns Ok if base_model_name_or_path contains expected_family, Err otherwise
    pub fn validate_base_model(&self, expected_family: &str) -> Result<(), AdapterConfigError> {
        let actual = &self.base_model_name_or_path;
        // Flexible match — the config may contain full HuggingFace path like
        // "meta-llama/Llama-3.3-70B-Instruct" but we only check the family name
        if !actual
            .to_lowercase()
            .contains(&expected_family.to_lowercase())
        {
            return Err(AdapterConfigError::BaseModelMismatch {
                expected: expected_family.into(),
                actual: actual.clone(),
            });
        }
        Ok(())
    }
}

/// Errors for adapter config operations.
#[derive(Debug, thiserror::Error)]
pub enum AdapterConfigError {
    #[error("Failed to parse adapter_config.json: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("Failed to read {path}: {error}")]
    Io { path: String, error: String },

    #[error("Base model mismatch: expected '{expected}', got '{actual}'")]
    BaseModelMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_adapter_config() {
        let json = r#"{
            "base_model_name_or_path": "meta-llama/Llama-3.3-70B-Instruct",
            "peft_type": "LORA",
            "task_type": "CAUSAL_LM",
            "r": 16,
            "lora_alpha": 32.0,
            "target_modules": ["q_proj", "v_proj"]
        }"#;

        let config = AdapterConfig::from_bytes(json.as_bytes()).expect("parse");
        assert_eq!(
            config.base_model_name_or_path,
            "meta-llama/Llama-3.3-70B-Instruct"
        );
        assert_eq!(config.peft_type.as_deref(), Some("LORA"));
        assert_eq!(config.r, Some(16));
        assert_eq!(
            config.target_modules.as_deref(),
            Some(&["q_proj".to_string(), "v_proj".to_string()][..])
        );
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let result = AdapterConfig::from_bytes(b"not json");
        assert!(result.is_err());
    }

    #[test]
    fn validate_base_model_match() {
        let config = AdapterConfig {
            base_model_name_or_path: "meta-llama/Llama-3.3-70B-Instruct".into(),
            peft_type: Some("LORA".into()),
            task_type: None,
            r: None,
            lora_alpha: None,
            target_modules: None,
            use_dora: None,
            loftq_config: None,
            revision: None,
            extra: serde_json::Value::Null,
        };

        assert!(config.validate_base_model("llama-3.3-70b").is_ok());
        assert!(config.validate_base_model("qwen2.5-72b").is_err());
    }
}
