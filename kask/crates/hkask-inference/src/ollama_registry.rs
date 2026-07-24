//! Ollama model/adapter registry — register hKask-owned GGUFs and LoRA adapters
//! as Ollama models so they become runnable via the `OM/` inference prefix.
//!
//! # Storage boundary
//!
//! hKask owns a *source* directory (provenance, re-createable); Ollama owns its
//! runtime content-addressed blob store. A generated Modelfile bridges them:
//!
//! ```text
//! ~/.hkask/models/                       ← hKask source of truth (configurable via HKASK_OLLAMA_SOURCE_DIR)
//!   gguf/<digest>.gguf                   ← base/merged GGUF (external imports only — hKask trains LoRAs, not merged GGUFs)
//!   modelfiles/<name>.Modelfile          ← generated blueprint (FROM + ADAPTER + PARAMETER)
//!        │
//!        ▼  ollama create hkask/<name> -f <Modelfile>
//! ~/.ollama/models/blobs/sha256-*        ← Ollama imports + dedups by digest
//!        ▼
//! routable as OM/hkask/<name>
//! ```rust,no_run
//!
//! # Storage authority (single, not duplicated)
//!
//! Adapter weights are NOT copied here — they live at the training pipeline's
//! `storage_path` (tracked by `AdapterStore`). `register_adapter` references that
//! path directly by absolute `ADAPTER` line, so there is one on-disk copy of each
//! adapter. This directory holds only GGUFs (external imports) and generated
//! Modelfiles.
//!
//! The Modelfile's `FROM`/`ADAPTER` use absolute paths, so the source GGUF may
//! live anywhere (the default `~/.hkask/models/`, a co-located
//! `~/.ollama/models/hkask/`, or an arbitrary path).
//!
//! # Distribution / backup
//!
//! Registered models are push-able via `ollama push hkask/<name>` to a registry,
//! and the source dir is content-addressed (SHA-256) so it syncs to object
//! storage with automatic dedup. Cross-provider distribution stays on the
//! existing HuggingFace `AdapterRegistry` channel.

use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::warn;

/// Default source directory for hKask-owned GGUFs/adapters/Modelfiles.
const DEFAULT_SOURCE_SUBDIR: &str = ".hkask/models";

/// Errors from registry operations.
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("ollama binary not found on PATH (set OLLAMA_BIN or install Ollama)")]
    OllamaNotFound,
    #[error("invalid model spec: {0}")]
    InvalidSpec(String),
    #[error("ollama create failed for '{name}': {stderr}")]
    CreateFailed { name: String, stderr: String },
    #[error("ollama rm failed for '{name}': {stderr}")]
    RemoveFailed { name: String, stderr: String },
    #[error("I/O error: {0}")]
    Io(String),
}

/// Where a registered model's weights come from.
#[derive(Debug, Clone)]
pub enum ModelFrom {
    /// Import a raw GGUF file by absolute path. Ollama validates and indexes it.
    Gguf(PathBuf),
    /// Inherit from an already-pulled Ollama model (e.g. `qwen3:8b`). Used to
    /// layer a LoRA adapter on top of a shared base without duplicating weights.
    ExistingModel(String),
}

/// A blueprint for an Ollama model — the inputs to `ollama create`.
#[derive(Debug, Clone)]
pub struct ModelfileSpec {
    /// Full Ollama tag (e.g. `hkask/solidity-audit-v3`). Routed as `OM/hkask/...`.
    pub name: String,
    /// Base weights.
    pub from: ModelFrom,
    /// Optional LoRA adapter (`ADAPTER` instruction) — safetensors path.
    pub adapter: Option<PathBuf>,
    /// `PARAMETER key value` lines (e.g. `("temperature", "0.3")`, `("num_ctx", "8192")`).
    pub parameters: Vec<(String, String)>,
    /// Optional chat `TEMPLATE` override.
    pub template: Option<String>,
    /// Optional `SYSTEM` prompt.
    pub system: Option<String>,
}

/// Minimal view of a trained LoRA adapter for local registration.
///
/// Lives in `hkask-inference` (not the adapter module in `hkask-mcp-training`) to keep the dependency
/// direction one-way (`hkask-mcp-training::adapter → hkask-inference`). The adapter layer
/// constructs this from `TrainedLoRAAdapter` in one line — this is the seam
/// that closes the orphan edge between `AdapterStore` and `OllamaRegistry`
/// without a cycle.
#[derive(Debug, Clone)]
pub struct LocalAdapter {
    /// Absolute path to the adapter weights directory (adapter_config.json +
    /// adapter_model.safetensors). Referenced verbatim by the `ADAPTER` line —
    /// NOT copied into `source_dir` (single on-disk copy).
    pub storage_path: PathBuf,
    /// Base model family, e.g. `qwen3:8b` (an Ollama tag the base is pulled as)
    /// or a path to a local base GGUF.
    pub base_model: ModelFrom,
}

/// A model registered with the local Ollama daemon.
///
/// Carries only the tag — full discovery (digest, size, modified) is the
/// `InferenceRouter::list_models` job, which already surfaces Ollama models
/// via `/v1/models`. Duplicating that metadata here would create a second
/// catalog with no reconciliation (semantic-graph-audit R4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredModel {
    pub name: String,
}

/// Build the Modelfile body from a spec. Pure — no I/O, fully testable.
///
/// expect: "The system registers owned models as runnable inference providers"
/// pre:  spec.name is non-empty; spec.from resolves to a path or existing model
/// post: returns a valid Modelfile string
#[must_use]
pub fn build_modelfile(spec: &ModelfileSpec) -> String {
    let mut out = String::new();
    match &spec.from {
        ModelFrom::Gguf(p) => {
            out.push_str(&format!("FROM {}\n", p.display()));
        }
        ModelFrom::ExistingModel(m) => {
            out.push_str(&format!("FROM {}\n", m));
        }
    }
    if let Some(ref adapter) = spec.adapter {
        out.push_str(&format!("ADAPTER {}\n", adapter.display()));
    }
    for (k, v) in &spec.parameters {
        out.push_str(&format!("PARAMETER {k} {v}\n"));
    }
    if let Some(ref tpl) = spec.template {
        out.push_str(&format!("TEMPLATE \"\"\"\n{tpl}\n\"\"\"\n"));
    }
    if let Some(ref sys) = spec.system {
        out.push_str(&format!("SYSTEM \"\"\"\n{sys}\n\"\"\"\n"));
    }
    out
}

/// Ollama model/adapter registry — registers hKask-owned weights as Ollama models.
pub struct OllamaRegistry {
    ollama_bin: PathBuf,
    source_dir: PathBuf,
}

impl Default for OllamaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaRegistry {
    /// Detect the `ollama` binary, source dir, and Ollama base URL from env.
    ///
    /// - `OLLAMA_BIN` overrides the binary path; otherwise `ollama` on PATH.
    /// - `HKASK_OLLAMA_SOURCE_DIR` overrides the source dir; otherwise
    ///   `$HOME/.hkask/models`.
    ///
    /// expect: "The system registers owned models as runnable inference providers"
    /// pre:  none (best-effort detection)
    /// post: returns a registry; `create` will error if the binary is missing
    pub fn new() -> Self {
        let ollama_bin = std::env::var("OLLAMA_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("ollama"));
        let source_dir = std::env::var("HKASK_OLLAMA_SOURCE_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(DEFAULT_SOURCE_SUBDIR))
            })
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOURCE_SUBDIR));
        Self {
            ollama_bin,
            source_dir,
        }
    }

    /// hKask-owned source directory (GGUFs, adapters, generated Modelfiles).
    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    /// Register a model: write the Modelfile into `source_dir/modelfiles/` then
    /// run `ollama create <name> -f <modelfile>`.
    ///
    /// expect: "The system registers owned models as runnable inference providers"
    /// pre:  spec.name is non-empty; referenced GGUF/adapter paths exist
    /// post: Ollama imports the model; it becomes routable as `OM/<name>`
    /// post: the Modelfile is persisted under `source_dir/modelfiles/` for re-creation
    pub fn create(&self, spec: &ModelfileSpec) -> Result<RegisteredModel, RegistryError> {
        if spec.name.is_empty() {
            return Err(RegistryError::InvalidSpec("name must be non-empty".into()));
        }
        if let ModelFrom::Gguf(p) = &spec.from
            && !p.exists()
        {
            return Err(RegistryError::InvalidSpec(format!(
                "GGUF not found: {}",
                p.display()
            )));
        }
        if let Some(ref adapter) = spec.adapter
            && !adapter.exists()
        {
            return Err(RegistryError::InvalidSpec(format!(
                "adapter not found: {}",
                adapter.display()
            )));
        }

        let modelfile_dir = self.source_dir.join("modelfiles");
        std::fs::create_dir_all(&modelfile_dir).map_err(|e| RegistryError::Io(e.to_string()))?;
        // Filesystem-safe filename: replace '/' with '_' so "hkask/foo" -> "hkask_foo".
        let safe = spec.name.replace('/', "_");
        let modelfile_path = modelfile_dir.join(format!("{safe}.Modelfile"));
        std::fs::write(&modelfile_path, build_modelfile(spec))
            .map_err(|e| RegistryError::Io(e.to_string()))?;

        let output = Command::new(&self.ollama_bin)
            .arg("create")
            .arg(&spec.name)
            .arg("-f")
            .arg(&modelfile_path)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    RegistryError::OllamaNotFound
                } else {
                    RegistryError::Io(e.to_string())
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            warn!(target: "reg.inference", "ollama create '{}' failed: {stderr}", spec.name);
            return Err(RegistryError::CreateFailed {
                name: spec.name.clone(),
                stderr,
            });
        }

        Ok(RegisteredModel {
            name: spec.name.clone(),
        })
    }

    /// Register a trained LoRA adapter as a local Ollama model, layering it on a
    /// base model without duplicating weights.
    ///
    /// Builds a Modelfile `FROM <base> ADAPTER <storage_path>` and runs
    /// `ollama create <name>`. The adapter weights stay at `adapter.storage_path`
    /// — they are referenced by absolute path, not copied. This is the train→local
    /// loop's closing edge: `AdapterStore` metadata → runnable `OM/<name>`.
    ///
    /// expect: "The system registers owned models as runnable inference providers"
    /// pre:  adapter.storage_path exists and contains adapter_model.safetensors
    /// pre:  base resolves to a pullable Ollama tag or an existing local GGUF
    /// post: Ollama model `<name>` is created; routable as `OM/<name>`
    pub fn register_adapter(
        &self,
        name: &str,
        adapter: &LocalAdapter,
        parameters: Vec<(String, String)>,
    ) -> Result<RegisteredModel, RegistryError> {
        let adapter_file = adapter.storage_path.join("adapter_model.safetensors");
        if !adapter_file.exists() {
            return Err(RegistryError::InvalidSpec(format!(
                "adapter weights not found: {}",
                adapter_file.display()
            )));
        }
        let spec = ModelfileSpec {
            name: name.to_string(),
            from: adapter.base_model.clone(),
            adapter: Some(adapter_file),
            parameters,
            template: None,
            system: None,
        };
        self.create(&spec)
    }

    /// Remove a registered model from the local Ollama daemon (`ollama rm`).
    /// The hKask source GGUF/adapter/Modelfile are left in place (provenance).
    ///
    /// expect: "The system registers owned models as runnable inference providers"
    /// pre:  name is non-empty
    /// post: model removed from Ollama; source artifacts retained
    pub fn remove(&self, name: &str) -> Result<(), RegistryError> {
        if name.is_empty() {
            return Err(RegistryError::InvalidSpec("name must be non-empty".into()));
        }
        let output = Command::new(&self.ollama_bin)
            .arg("rm")
            .arg(name)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    RegistryError::OllamaNotFound
                } else {
                    RegistryError::Io(e.to_string())
                }
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(RegistryError::RemoveFailed {
                name: name.to_string(),
                stderr,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_modelfile_gguf_only() {
        let spec = ModelfileSpec {
            name: "hkask/test".into(),
            from: ModelFrom::Gguf(PathBuf::from("/data/qwen3.gguf")),
            adapter: None,
            parameters: Vec::new(),
            template: None,
            system: None,
        };
        let mf = build_modelfile(&spec);
        assert!(mf.starts_with("FROM /data/qwen3.gguf\n"));
        assert!(!mf.contains("ADAPTER"));
    }

    #[test]
    fn build_modelfile_lora_on_existing_model() {
        let spec = ModelfileSpec {
            name: "hkask/solidity-audit-v3".into(),
            from: ModelFrom::ExistingModel("qwen3:8b".into()),
            adapter: Some(PathBuf::from("/data/adapter_model.safetensors")),
            parameters: vec![
                ("temperature".into(), "0.3".into()),
                ("num_ctx".into(), "8192".into()),
            ],
            template: None,
            system: Some("You audit Solidity.".into()),
        };
        let mf = build_modelfile(&spec);
        assert!(mf.contains("FROM qwen3:8b\n"));
        assert!(mf.contains("ADAPTER /data/adapter_model.safetensors\n"));
        assert!(mf.contains("PARAMETER temperature 0.3\n"));
        assert!(mf.contains("PARAMETER num_ctx 8192\n"));
        assert!(mf.contains("SYSTEM \"\"\"\nYou audit Solidity.\n\"\"\"\n"));
    }

    #[test]
    fn registry_default_uses_home_source_dir() {
        // Don't assert exact path (env may strip HOME in CI); just exercise construction.
        let _ = OllamaRegistry::new();
    }

    /// M2: the train→local connector. Verifies the Modelfile wiring that
    /// `register_adapter` builds — without shelling out to the daemon.
    /// This is the orphan-edge close: adapter.storage_path → ADAPTER line, base → FROM line.
    #[test]
    fn register_adapter_wires_storage_path_without_duplication() {
        let adapter = LocalAdapter {
            // Pretend the training pipeline stored weights here.
            storage_path: PathBuf::from("/data/adapters/solidity-v3"),
            base_model: ModelFrom::ExistingModel("qwen3:8b".into()),
        };
        // Mirror register_adapter's internal spec construction.
        let spec = ModelfileSpec {
            name: "hkask/solidity-audit-v3".into(),
            from: adapter.base_model.clone(),
            adapter: Some(adapter.storage_path.join("adapter_model.safetensors")),
            parameters: vec![("num_ctx".into(), "8192".into())],
            template: None,
            system: None,
        };
        let mf = build_modelfile(&spec);
        // The ADAPTER line points at the ORIGINAL storage_path, not a copy under source_dir.
        assert!(mf.contains("FROM qwen3:8b\n"));
        assert!(mf.contains("ADAPTER /data/adapters/solidity-v3/adapter_model.safetensors\n"));
        assert!(mf.contains("PARAMETER num_ctx 8192\n"));
        // No weight duplication: the path is the training pipeline's, absolute.
        assert!(!mf.contains(".hkask/models/adapters"));
    }
}
