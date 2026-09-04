//! Standalone (non-zed) settings layer — single source of truth for the
//! settings file location used by CLI, API, and REPL surfaces. Magna Carta
//! P3: all surfaces read/write the same `~/.config/hkask/settings.json`.
//! Also provides `HkaskSettings` for model defaults shared across all servers.
//!
//! # The two settings layers
//!
//! There are TWO settings modules in the workspace — this file is the
//! **standalone layer**, not the zed layer. Do not confuse them:
//!
//! - **This file** (`hkask-services-core/src/standalone_settings.rs`):
//!   `~/.config/zed-kask/settings.json` + env vars. Read by CLI, API, REPL,
//!   and by MCP servers when they are NOT launched by zed (no IPC bridge).
//!   Priority: env var > settings.json > hardcoded default.
//! - **The zed layer** (`kask/crates/kask_bridge/src/settings.rs`): zed's
//!   settings store (`KaskSettings`, `From<Content>` conversions, schema in
//!   `crates/settings_content/src/settings_content.rs`). Read when servers
//!   ARE launched by zed; `KaskSettings::mcp_env()` translates it into the
//!   same env vars this layer reads.
//!
//! The layers meet at the env vars (`HKASK_*_MODEL` etc.): zed writes them
//! from its settings store at launch; standalone surfaces read them from the
//! shell or fall back to this file.

use serde::{Deserialize, Serialize};

/// Returns the canonical path to `~/.config/hkask/settings.json`,
/// creating the parent directory if needed.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  none (always succeeds)
/// post: returns PathBuf to ~/.config/zed-kask/settings.json; parent directory created if missing
#[must_use]
pub(crate) fn settings_path() -> std::path::PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("zed-kask");
    if let Err(e) = std::fs::create_dir_all(&path) {
        tracing::warn!(
            target: "hkask.services_core",
            error = %e,
            path = %path.display(),
            "Failed to create zed-kask config directory — \
             settings persistence will fail if the directory doesn't exist."
        );
    }
    path.push("settings.json");
    path
}

/// System-wide model defaults persisted to `~/.config/zed-kask/settings.json`.
/// Shared across CLI, API, REPL, and all MCP servers.
/// Priority: env var > settings.json > hardcoded default.
///
/// Note: the generation model is `HKASK_DEFAULT_MODEL` in `InferenceConfig` —
/// there is no separate replica/composition model slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    // Code default (operator ruling 2026-09-04, superseding the
    // no-hidden-models spec): the operator's configured models are the
    // defaults so the code works out of the box; settings.json / env vars
    // override them.
    "ollama/qwen3-embedding:0.6b".to_string()
}

fn default_classifier_model() -> String {
    // glm-5.2, not glm-5.3-flash: the classifier must be a non-thinking
    // model (or one where thinking is disable-able via
    // `reasoning_effort: "none"`) — classification and tagging need
    // output tokens, not reasoning tokens. glm-5.3-flash is a thinking
    // model that cannot disable it (operator ruling 2026-09-04).
    "OpenRouter/z-ai/glm-5.2".to_string()
}

fn default_ocr_model() -> String {
    "ollama/glm-ocr:latest".to_string()
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
    /// Load settings from the shared `~/.config/zed-kask/settings.json`.
    /// Falls back to defaults if the file doesn't exist or is unreadable.
    ///
    /// The file is zed's settings file; kask model settings live under its
    /// `kask.models` section (`KaskModelsSettingsContent` in
    /// `settings_content.rs`). Fields absent from that section fall back to
    /// `Default` — the file never needs to carry all four fields.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  none (always succeeds)
    /// post: returns HkaskSettings with `kask.models` overrides applied over defaults; defaults if file missing or unparsable
    #[must_use]
    pub fn load() -> Self {
        let path = settings_path();
        match std::fs::read_to_string(&path) {
            Ok(json) => Self::parse_over_defaults(&json, &path),
            Err(_) => Self::default(),
        }
    }

    /// Parse the shared settings file, overlaying the `kask.models` section
    /// onto `Default`. The former whole-file `from_str::<HkaskSettings>` read
    /// required `embedding_model` et al. at the file's top level — keys that
    /// never exist in zed's settings file — so every parse failed and every
    /// surface silently ran on defaults (observed live: `missing field
    /// 'embedding_model' at line 147`, every boot).
    ///
    /// \[P5\] pre:  none
    /// post: defaults with `kask.models.{embedding_model, classifier_model, ocr_model}` overlaid when present and well-typed
    fn parse_over_defaults(json: &str, path: &std::path::Path) -> Self {
        let user: serde_json::Value = match serde_json::from_str(json) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "Failed to parse settings.json — using defaults"
                );
                return Self::default();
            }
        };
        let mut merged = match serde_json::to_value(Self::default()) {
            Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
            _ => return Self::default(),
        };
        if let Some(user_models) = user
            .get("kask")
            .and_then(|kask| kask.get("models"))
            .and_then(|models| models.as_object())
        {
            if let Some(merged_object) = merged.as_object_mut() {
                for key in ["embedding_model", "classifier_model", "ocr_model"] {
                    if let Some(value) = user_models.get(key) {
                        merged_object.insert(key.to_string(), value.clone());
                    }
                }
            }
        }
        serde_json::from_value(merged).unwrap_or_else(|error| {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "Failed to parse kask.models settings — using defaults"
            );
            Self::default()
        })
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

    // `save()` was removed 2026-08-30: zero callers anywhere in the
    // workspace, and its behavior was a data-loss hazard — it serialized
    // `HkaskSettings` alone to the shared settings path, which would
    // clobber zed's entire settings file (every key outside the four model
    // fields) if it were ever called. Settings writes belong to the zed
    // layer (`kask_bridge/src/settings.rs`), which merges into the real
    // store.
}

#[cfg(test)]
mod tests {
    use super::HkaskSettings;

    /// The shared settings file is zed's — kask model settings live under
    /// `kask.models`. A zed-shaped file (the operator's actual file shape)
    /// must parse without error, overlaying present fields and defaulting
    /// absent ones. The former whole-file read failed on exactly this shape.
    #[test]
    fn zed_shaped_settings_file_parses_with_kask_models_overrides() {
        let json = r#"{
            "theme": "one",
            "language_models": {"open_router": {"api_url": "https://openrouter.ai/api/v1"}},
            "kask": {
                "models": {
                    "default_model": "OpenRouter/z-ai/glm-5.2",
                    "ocr_model": "RunPod/kask-ocr-v2"
                },
                "memory": {"memory_life_days": 30.0}
            },
            "context_servers": {}
        }"#;
        let settings =
            HkaskSettings::parse_over_defaults(json, std::path::Path::new("/test/settings.json"));
        assert_eq!(settings.ocr_model, "RunPod/kask-ocr-v2");
        // Absent fields fall back to the code defaults (operator ruling
        // 2026-09-04: defaults in code so the code works; settings
        // override).
        assert_eq!(settings.embedding_model, "ollama/qwen3-embedding:0.6b");
        assert_eq!(settings.classifier_model, "OpenRouter/z-ai/glm-5.2");
    }

    /// A file with no `kask` section at all (pure zed settings) yields pure
    /// defaults — no warn-worthy failure, since the file is valid.
    #[test]
    fn settings_file_without_kask_section_yields_defaults() {
        let json = r#"{"theme": "one", "context_servers": {}}"#;
        let settings =
            HkaskSettings::parse_over_defaults(json, std::path::Path::new("/test/settings.json"));
        assert_eq!(settings, HkaskSettings::default());
    }

    /// Unparsable JSON still degrades to defaults (surfaced by the warn in
    /// the parse path, not by a panic).
    #[test]
    fn unparsable_settings_file_yields_defaults() {
        let settings = HkaskSettings::parse_over_defaults(
            "{ not json",
            std::path::Path::new("/test/settings.json"),
        );
        assert_eq!(settings, HkaskSettings::default());
    }
}
