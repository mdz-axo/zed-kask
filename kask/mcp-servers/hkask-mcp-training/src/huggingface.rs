//! HuggingFace infrastructure — training-artifact publishing.
//!
//! The prior `ModelRegistry` / `AdapterRegistry` / `DatasetRegistry` traits and
//! their `HfModelRegistry` impl were removed as dead code: zero production
//! callers and `HfModelRegistry::new` was never constructed. Dataset/adapter
//! Hub operations live in `HuggingFaceTraining`.

use hf_hub::HFClient;
use hf_hub::repository::{AddSource, RepoTypeDataset, RepoTypeModel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── HuggingFace error ─────────────────────────────────────────────────────
#[derive(Debug, thiserror::Error)]
pub enum HuggingFaceError {
    #[error("HuggingFace API error: {0}")]
    Api(String),
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Adapter not found: {0}")]
    AdapterNotFound(String),
    #[error("Dataset not found: {0}")]
    DatasetNotFound(String),
    #[error("Download failed: {0}")]
    Download(String),
    #[error("Authentication failed (set HF_TOKEN)")]
    AuthRequired,
}

// ── Model provenance ──────────────────────────────────────────────────────

/// Resolved model provenance — what we know about a model before training.
#[derive(Debug, Clone, Serialize)]
pub struct ModelProvenance {
    pub model_id: String,
    pub architecture: String,
    pub license: Option<String>,
    pub lora_compatible: bool,
    pub is_gated: bool,
}

/// ModelResolver — resolves model identity and provenance before training.
/// Static model resolver using built-in known-model registry.
#[derive(Default)]
pub struct LocalModelResolver;

impl LocalModelResolver {
    pub fn resolve(&self, model_id: &str) -> Result<ModelProvenance, HuggingFaceError> {
        if !model_id.contains('/') {
            return Err(HuggingFaceError::ModelNotFound(model_id.to_string()));
        }
        let (org, model) = model_id
            .split_once('/')
            .ok_or_else(|| HuggingFaceError::ModelNotFound(model_id.to_string()))?;
        let model_lower = model.to_lowercase();
        let (arch, license, lora_ok, gated) = if model_lower.contains("llama") {
            ("llama", Some("llama3"), true, org == "meta-llama")
        } else if model_lower.contains("mistral") {
            ("mistral", Some("apache-2.0"), true, false)
        } else if model_lower.contains("qwen") {
            ("qwen", Some("apache-2.0"), true, false)
        } else if model_lower.contains("gemma") {
            ("gemma", Some("gemma"), true, org == "google")
        } else if model_lower.contains("phi") {
            ("phi", Some("mit"), true, false)
        } else if model_lower.contains("deepseek") {
            ("deepseek", Some("mit"), true, false)
        } else if model_lower.contains("yi") {
            ("yi", Some("apache-2.0"), true, false)
        } else {
            ("unknown", None, true, false)
        };
        Ok(ModelProvenance {
            model_id: model_id.to_string(),
            architecture: arch.to_string(),
            license: license.map(|s| s.to_string()),
            lora_compatible: lora_ok,
            is_gated: gated,
        })
    }

    pub fn validate(&self, model_id: &str) -> bool {
        self.resolve(model_id).is_ok()
    }
}

// ── Training artifacts ─────────────────────────────────────────────────────

/// An immutable artifact published for a remote training job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingArtifact {
    pub repository: String,
    pub revision: String,
    pub path: String,
    pub sha256: String,
}

/// Immutable input and output locations for a Hugging Face training job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingArtifacts {
    pub dataset: TrainingArtifact,
    pub model_repository: String,
    pub completion_manifest_path: String,
}

/// A single runtime alert from the training loop, stored in the completion manifest.
///
/// Mirrors the HuggingFace trackio alert pattern. Each alert becomes a G-R1
/// finding with `evidence_kind: runtime_measurement` when the manifest is
/// evaluated by `validate_runtime_metrics`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAlert {
    /// Alert title (e.g., "Loss divergence", "Vanishing loss").
    #[serde(default)]
    pub title: String,
    /// Alert severity: "info", "warn", "error", "critical".
    /// Unknown levels default to "warn" in `validate_runtime_metrics`.
    #[serde(default = "default_alert_level")]
    pub level: String,
    /// Alert text/body.
    #[serde(default)]
    pub text: String,
    /// Step at which the alert fired.
    #[serde(default)]
    pub step: Option<u32>,
}

fn default_alert_level() -> String {
    "warn".to_string()
}

/// Evidence written by the training pod after training completes, uploaded to
/// the private model repository at `jobs/{job_id}/completion-manifest.json`.
/// Fetched by `training_status` to detect completion (the pod stays RUNNING for
/// SSH debugging, so RunPod's desiredStatus alone cannot signal completion).
///
/// v0.32.0: extended with `grad_norm`, `current_step`, `total_steps`, and
/// `alerts` to support G-R1 (runtime alert gate). All new fields are
/// `#[serde(default)]` for backward compatibility with existing manifests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionManifest {
    pub job_id: String,
    /// "success" or "failed" — written by the install script.
    pub status: String,
    pub dataset_sha256: String,
    pub adapter: TrainingArtifact,
    pub finished_at: String,
    /// Base model used for training.
    #[serde(default)]
    pub base_model: Option<String>,
    /// Training harness used (axolotl, trl, ludwig).
    #[serde(default)]
    pub harness: Option<String>,
    /// Training duration in seconds.
    #[serde(default)]
    pub training_duration_secs: Option<u64>,
    /// Final training loss, if available from the harness.
    #[serde(default)]
    pub loss: Option<f64>,
    /// Final gradient norm, if available from the harness (v0.32.0).
    /// Consumed by G-R1 (runtime alert gate) for NaN/infinite detection.
    #[serde(default)]
    pub grad_norm: Option<f64>,
    /// Current training step at completion (v0.32.0). Consumed by G-R1 for
    /// loss-divergence detection (loss > 5.0 after step 100).
    #[serde(default)]
    pub current_step: Option<u32>,
    /// Total training steps (v0.32.0). Used to compute training progress.
    #[serde(default)]
    pub total_steps: Option<u32>,
    /// Runtime alerts from the training loop (v0.32.0). Each alert becomes a
    /// G-R1 finding with `evidence_kind: runtime_measurement`. Mirrors the
    /// HF trackio alert pattern.
    #[serde(default)]
    pub alerts: Vec<TrainingAlert>,
    /// Output directory on the pod where the adapter was saved.
    #[serde(default)]
    pub output_dir: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TrainingArtifactError {
    #[error("artifact configuration is invalid: {0}")]
    InvalidConfiguration(String),
    #[error("artifact upload failed: {0}")]
    Upload(String),
    #[error("artifact retrieval failed: {0}")]
    Retrieval(String),
    #[error("completion manifest is invalid: {0}")]
    InvalidManifest(String),
}

/// Hugging Face Hub configuration for remote training artifacts.
///
/// All repositories are addressed through an explicit owner and are private.
/// The token is deliberately never exposed by this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HuggingFaceTrainingConfig {
    owner: String,
    dataset_repo: String,
    model_repo: String,
}

impl HuggingFaceTrainingConfig {
    fn from_env() -> Result<(Self, String), TrainingArtifactError> {
        let token = required_env("HF_TOKEN")?;
        let owner = required_env("HKASK_HF_ARTIFACT_OWNER")?;
        let dataset_repo = required_env("HKASK_HF_DATASET_REPO")?;
        let model_repo = required_env("HKASK_HF_MODEL_REPO")?;
        let config = Self {
            owner,
            dataset_repo,
            model_repo,
        };
        config.validate()?;
        Ok((config, token))
    }

    fn validate(&self) -> Result<(), TrainingArtifactError> {
        for (name, value) in [
            ("HKASK_HF_ARTIFACT_OWNER", &self.owner),
            ("HKASK_HF_DATASET_REPO", &self.dataset_repo),
            ("HKASK_HF_MODEL_REPO", &self.model_repo),
        ] {
            if value.is_empty() || value.contains('/') || value.chars().any(char::is_whitespace) {
                return Err(TrainingArtifactError::InvalidConfiguration(format!(
                    "{name} must be a non-empty Hugging Face name without '/' or whitespace"
                )));
            }
        }
        Ok(())
    }

    fn dataset_repository(&self) -> String {
        format!("{}/{}", self.owner, self.dataset_repo)
    }

    fn model_repository(&self) -> String {
        format!("{}/{}", self.owner, self.model_repo)
    }
}

/// Private-only Hugging Face training artifact client.
pub struct HuggingFaceTraining {
    config: HuggingFaceTrainingConfig,
    client: HFClient,
}

impl HuggingFaceTraining {
    /// Reads `HF_TOKEN`, `HKASK_HF_ARTIFACT_OWNER`, `HKASK_HF_DATASET_REPO`,
    /// and `HKASK_HF_MODEL_REPO` from the runtime environment.
    pub fn from_env() -> Result<Self, TrainingArtifactError> {
        let (config, token) = HuggingFaceTrainingConfig::from_env()?;
        let client = HFClient::builder()
            .token(token)
            .build()
            .map_err(|error| TrainingArtifactError::InvalidConfiguration(error.to_string()))?;
        Ok(Self { config, client })
    }

    async fn ensure_private_repositories(&self) -> Result<(), TrainingArtifactError> {
        let dataset_repository = self.config.dataset_repository();
        self.client
            .create_repository()
            .repo_id(&dataset_repository)
            .repo_type(RepoTypeDataset)
            .private(true)
            .exist_ok(true)
            .send()
            .await
            .map_err(|error| {
                TrainingArtifactError::Upload(format!("create private dataset repository: {error}"))
            })?;
        let model_repository = self.config.model_repository();
        self.client
            .create_repository()
            .repo_id(&model_repository)
            .repo_type(RepoTypeModel)
            .private(true)
            .exist_ok(true)
            .send()
            .await
            .map_err(|error| {
                TrainingArtifactError::Upload(format!("create private model repository: {error}"))
            })?;
        Ok(())
    }

    fn validate_job_id(job_id: &str) -> Result<(), TrainingArtifactError> {
        if job_id.is_empty()
            || job_id.contains('/')
            || job_id.contains('\\')
            || job_id.chars().any(char::is_whitespace)
        {
            return Err(TrainingArtifactError::InvalidConfiguration(
                "job ID must be non-empty and must not contain path separators or whitespace"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn completion_manifest_path(job_id: &str) -> String {
        format!("jobs/{job_id}/completion-manifest.json")
    }

    /// Parses completion-manifest bytes retrieved from the private model repository.
    pub fn parse_completion_manifest(
        bytes: &[u8],
    ) -> Result<CompletionManifest, TrainingArtifactError> {
        serde_json::from_slice(bytes).map_err(|error| {
            TrainingArtifactError::InvalidManifest(format!(
                "could not parse completion manifest: {error}"
            ))
        })
    }

    pub async fn publish_dataset(
        &self,
        job_id: &str,
        bytes: Vec<u8>,
        sha256: &str,
    ) -> Result<TrainingArtifact, TrainingArtifactError> {
        Self::validate_job_id(job_id)?;
        let calculated = format!("{:x}", Sha256::digest(&bytes));
        if sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !sha256.eq_ignore_ascii_case(&calculated)
        {
            return Err(TrainingArtifactError::InvalidConfiguration(
                "provided SHA-256 does not match dataset bytes".to_string(),
            ));
        }

        self.ensure_private_repositories().await?;
        let path = format!("jobs/{job_id}/dataset.jsonl");
        let repository = self.config.dataset_repository();
        let commit = self
            .client
            .dataset(&self.config.owner, &self.config.dataset_repo)
            .upload_file()
            .source(AddSource::Bytes(bytes.into()))
            .path_in_repo(path.clone())
            .commit_message(format!("Publish training dataset for {job_id}"))
            .send()
            .await
            .map_err(|error| {
                TrainingArtifactError::Upload(format!("publish dataset artifact: {error}"))
            })?;
        let revision = commit.commit_oid.ok_or_else(|| {
            TrainingArtifactError::Upload(
                "Hub upload response omitted the immutable commit revision".to_string(),
            )
        })?;
        Ok(TrainingArtifact {
            repository,
            revision,
            path,
            sha256: sha256.to_ascii_lowercase(),
        })
    }

    pub async fn prepare_training_artifacts(
        &self,
        job_id: &str,
        dataset: TrainingArtifact,
    ) -> Result<TrainingArtifacts, TrainingArtifactError> {
        Self::validate_job_id(job_id)?;
        if dataset.repository != self.config.dataset_repository()
            || dataset.revision.is_empty()
            || dataset.path.is_empty()
            || dataset.sha256.len() != 64
            || !dataset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(TrainingArtifactError::InvalidConfiguration(
                "dataset reference is not an immutable artifact in the configured private dataset repository"
                    .to_string(),
            ));
        }

        Ok(TrainingArtifacts {
            dataset,
            model_repository: self.config.model_repository(),
            completion_manifest_path: Self::completion_manifest_path(job_id),
        })
    }

    pub async fn fetch_completion_manifest(
        &self,
        artifacts: &TrainingArtifacts,
    ) -> Result<CompletionManifest, TrainingArtifactError> {
        if artifacts.model_repository != self.config.model_repository()
            || artifacts.completion_manifest_path.is_empty()
        {
            return Err(TrainingArtifactError::InvalidConfiguration(
                "training artifacts do not target the configured private model repository"
                    .to_string(),
            ));
        }

        let path = self
            .client
            .model(&self.config.owner, &self.config.model_repo)
            .download_file()
            .filename(artifacts.completion_manifest_path.clone())
            .send()
            .await
            .map_err(|error| {
                TrainingArtifactError::Retrieval(format!("download completion manifest: {error}"))
            })?;
        let bytes = std::fs::read(path).map_err(|error| {
            TrainingArtifactError::Retrieval(format!("read completion manifest: {error}"))
        })?;
        Self::parse_completion_manifest(&bytes)
    }
}

fn required_env(name: &str) -> Result<String, TrainingArtifactError> {
    std::env::var(name)
        .map_err(|_| {
            TrainingArtifactError::InvalidConfiguration(format!("{name} must be set and non-empty"))
        })
        .and_then(|value| {
            if value.is_empty() {
                Err(TrainingArtifactError::InvalidConfiguration(format!(
                    "{name} must be set and non-empty"
                )))
            } else {
                Ok(value)
            }
        })
}
