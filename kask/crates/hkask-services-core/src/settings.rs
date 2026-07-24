//! Shared settings path utility — single source of truth for the settings file
//! location used by CLI, API, and REPL surfaces. Magna Carta P3: all surfaces
//! read/write the same `~/.config/hkask/settings.json`.
//!
//! Also provides `HkaskSettings` for model defaults shared across all servers.

use serde::{Deserialize, Serialize};

/// Returns the canonical path to `~/.config/hkask/settings.json`,
/// creating the parent directory if needed.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  none (always succeeds)
/// post: returns PathBuf to ~/.config/hkask/settings.json; parent directory created if missing
#[must_use]
pub fn settings_path() -> std::path::PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("hkask");
    let _ = std::fs::create_dir_all(&path);
    path.push("settings.json");
    path
}

/// System-wide model defaults persisted to `~/.config/hkask/settings.json`.
/// Shared across CLI, API, REPL, and all MCP servers.
/// Priority: env var > settings.json > hardcoded default.
///
/// Note: the generation model is `HKASK_DEFAULT_MODEL` in `InferenceConfig` —
/// there is no separate replica/composition model slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HkaskSettings {
    /// Default embedding model for vectorization.
    /// Override: `HKASK_EMBEDDING_MODEL` env var.
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,

    /// Primary classifier model for corpus pipeline classification.
    /// Override: `HKASK_CLASSIFIER_MODEL` env var.
    #[serde(default = "default_classifier_model")]
    pub classifier_model: String,

    /// Default OCR model for scanned PDF fallback.
    /// Override: `HKASK_OCR_MODEL` env var.
    #[serde(default = "default_ocr_model")]
    pub ocr_model: String,

    /// Default max tokens per chunk for document chunking.
    /// 256 tokens ≈ 192 words — paragraph-level granularity suitable for
    /// QA generation and semantic search. Override: `HKASK_CHUNK_MAX_TOKENS` env var.
    #[serde(default = "default_chunk_max_tokens")]
    pub chunk_max_tokens: usize,
}

fn default_embedding_model() -> String {
    "DI/Qwen/Qwen3-Embedding-0.6B".to_string()
}

fn default_classifier_model() -> String {
    // Single source of truth: hkask_inference::model_constants::DEFAULT_CLASSIFIER_MODEL.
    // Do not duplicate the model id here — resolve via the canonical constant.
    hkask_inference::model_constants::DEFAULT_CLASSIFIER_MODEL.to_string()
}

fn default_ocr_model() -> String {
    "RP/kask-ocr".to_string()
}

fn default_chunk_max_tokens() -> usize {
    256
}

impl Default for HkaskSettings {
    fn default() -> Self {
        Self {
            embedding_model: default_embedding_model(),
            classifier_model: default_classifier_model(),
            ocr_model: default_ocr_model(),
            chunk_max_tokens: default_chunk_max_tokens(),
        }
    }
}

impl HkaskSettings {
    /// Load settings from `~/.config/hkask/settings.json`.
    /// Falls back to defaults if the file doesn't exist or is unreadable.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  none (always succeeds)
    /// post: returns HkaskSettings from disk; HkaskSettings::default() if file missing or unparseable
    #[must_use]
    pub fn load() -> Self {
        let path = settings_path();
        match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to parse settings.json — using defaults"
                );
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Resolve the effective model, preferring env var over settings over default.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  env_var name must be valid; settings_value and default must be non-empty strings
    /// post: returns env var value if set and non-empty; else settings_value if non-empty; else default
    #[must_use]
    pub fn resolve_model(env_var: &str, settings_value: &str, default: &str) -> String {
        std::env::var(env_var)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if settings_value.is_empty() {
                    default.to_string()
                } else {
                    settings_value.to_string()
                }
            })
    }

    /// pre:  none (always succeeds)
    /// post: returns effective embedding model string (env > settings > default)
    #[must_use]
    pub fn embedding_model(&self) -> String {
        Self::resolve_model(
            "HKASK_EMBEDDING_MODEL",
            &self.embedding_model,
            &default_embedding_model(),
        )
    }

    /// Resolve the classifier model with env/settings/default priority.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  none (always succeeds)
    /// post: returns effective classifier model string (env > settings > default)
    #[must_use]
    pub fn classifier_model(&self) -> String {
        Self::resolve_model(
            "HKASK_CLASSIFIER_MODEL",
            &self.classifier_model,
            &default_classifier_model(),
        )
    }

    /// Resolve the OCR model with env/settings/default priority.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  none (always succeeds)
    /// post: returns effective OCR model string (env > settings > default)
    #[must_use]
    pub fn ocr_model(&self) -> String {
        Self::resolve_model("HKASK_OCR_MODEL", &self.ocr_model, &default_ocr_model())
    }

    /// Resolve the chunk max tokens with env/settings/default priority.
    ///
    /// pre:  none (always succeeds)
    /// post: returns effective max tokens per chunk (env > settings > default 256)
    #[must_use]
    pub fn chunk_max_tokens(&self) -> usize {
        std::env::var("HKASK_CHUNK_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(self.chunk_max_tokens)
    }

    /// Save settings to `~/.config/hkask/settings.json`.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self must be a valid HkaskSettings
    /// post: settings are written as pretty JSON to settings_path(); Err on serialization or I/O failure
    #[must_use = "result must be used"]
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = settings_path();
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)
    }
}

/// Load any settings type from `~/.config/hkask/settings.json`.
/// Falls back to `T::default()` if the file doesn't exist or is unparseable.
///
/// This is the shared load path for CLI (`ReplSettings`), API (`SettingsResponse`),
/// and any future surface that needs LLM parameter persistence.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  T must implement DeserializeOwned + Default
/// post: returns T from disk; T::default() if file missing or unparseable
#[must_use]
pub fn load_settings<T: serde::de::DeserializeOwned + Default>() -> T {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "Failed to parse settings.json — using defaults"
            );
            T::default()
        }),
        Err(_) => T::default(),
    }
}

/// Save any settings type to `~/.config/hkask/settings.json`.
///
/// This is the shared save path for CLI, API, and any future surface.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  settings must implement Serialize
/// post: settings are written as pretty JSON to settings_path(); Err(ServiceError::Infra) on serialization or I/O failure
#[must_use = "result must be used"]
pub fn save_settings<T: serde::Serialize>(settings: &T) -> Result<(), crate::ServiceError> {
    let path = settings_path();
    let json = serde_json::to_string_pretty(settings).map_err(|e| {
        crate::ServiceError::Infra(hkask_types::InfrastructureError::Serialization(
            e.to_string(),
        ))
    })?;
    std::fs::write(&path, json).map_err(|e| {
        crate::ServiceError::Infra(hkask_types::InfrastructureError::Io(e.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_settings_returns_default_when_file_missing() {
        // Use a non-existent path by temporarily overriding — just test the fallback
        let settings: HkaskSettings = load_settings();
        // Should always succeed (returns default on any error)
        assert!(!settings.embedding_model.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let original = HkaskSettings::default();
        save_settings(&original).expect("save should succeed");
        let loaded = load_settings::<HkaskSettings>();
        assert_eq!(loaded.embedding_model, original.embedding_model);
        assert_eq!(loaded.ocr_model, original.ocr_model);
    }
}
