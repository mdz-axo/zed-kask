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
    if let Err(e) = std::fs::create_dir_all(&path) {
        tracing::warn!(
            target: "hkask.services_core",
            error = %e,
            path = %path.display(),
            "Failed to create hkask config directory — \
             settings persistence will fail if the directory doesn't exist."
        );
    }
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
    pub embedding_model: String,

    /// Primary classifier model for corpus pipeline classification.
    /// Override: `HKASK_CLASSIFIER_MODEL` env var.
    pub classifier_model: String,

    /// Default OCR model for scanned PDF fallback.
    /// Override: `HKASK_OCR_MODEL` env var.
    pub ocr_model: String,

    /// Default max tokens per chunk for document chunking.
    /// 256 tokens ≈ 192 words — paragraph-level granularity suitable for
    /// QA generation and semantic search. Override: `HKASK_CHUNK_MAX_TOKENS` env var.
    pub chunk_max_tokens: usize,
}

/// Default max tokens per chunk for document chunking.
///
/// 256 tokens ≈ 192 words — paragraph-level granularity suitable for
/// QA generation and semantic search.
pub(crate) const DEFAULT_CHUNK_MAX_TOKENS: usize = 256;

fn default_embedding_model() -> String {
    // Single source of truth: hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL.
    // Do not duplicate the model id here — resolve via the canonical constant.
    hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL.to_string()
}

fn default_classifier_model() -> String {
    // Single source of truth: hkask_inference::model_constants::DEFAULT_CLASSIFIER_MODEL.
    // Do not duplicate the model id here — resolve via the canonical constant.
    hkask_inference::model_constants::DEFAULT_CLASSIFIER_MODEL.to_string()
}

fn default_ocr_model() -> String {
    // Single source of truth: hkask_inference::model_constants::DEFAULT_OCR_MODEL.
    // Do not duplicate the model id here — resolve via the canonical constant.
    hkask_inference::model_constants::DEFAULT_OCR_MODEL.to_string()
}

fn default_chunk_max_tokens() -> usize {
    DEFAULT_CHUNK_MAX_TOKENS
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
    /// post: returns HkaskSettings from disk; HkaskSettings::default() if file missing or unparsable
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
        // A malformed or non-positive numeric env var must warn, not silently
        // fall back — an operator cannot distinguish "not configured" from
        // "configured but broken" otherwise (`.rules` failure-signal trap).
        // Mirrors the `HKASK_OCR_CONCURRENCY` reference in `hkask-mcp-corpus`.
        match std::env::var("HKASK_CHUNK_MAX_TOKENS") {
            Ok(raw) => match raw.parse::<usize>() {
                Ok(n) if n > 0 => n,
                Ok(_non_positive) => {
                    tracing::warn!(
                        target: "hkask.services.settings",
                        value = %raw,
                        "HKASK_CHUNK_MAX_TOKENS must be > 0 — falling back to {default}",
                        default = self.chunk_max_tokens
                    );
                    self.chunk_max_tokens
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.services.settings",
                        value = %raw,
                        error = %e,
                        "HKASK_CHUNK_MAX_TOKENS malformed — falling back to {default}",
                        default = self.chunk_max_tokens
                    );
                    self.chunk_max_tokens
                }
            },
            Err(_) => self.chunk_max_tokens,
        }
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
/// Falls back to `T::default()` if the file doesn't exist or is unparsable.
///
/// This is the shared load path for CLI (`ReplSettings`), API (`SettingsResponse`),
/// and any future surface that needs LLM parameter persistence.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  T must implement DeserializeOwned + Default
/// post: returns T from disk; T::default() if file missing or unparsable
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
