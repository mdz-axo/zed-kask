//! HuggingFace infrastructure traits — model registry, adapter registry, dataset registry.
//!
//! These are *infrastructure* traits, not a 4th training host. They enhance the
//! existing hosts (Runpod) transparently, providing:
//! - Model resolution (provider-prefix → HF model ID)
//! - Adapter publication/pull (Runpod pulls from HF)
//! - Dataset remote sourcing (hf://datasets/ URLs)
//!
//! MDS categories:
//! - ModelRegistry → Domain entity: ModelSource with hf:// URI scheme
//! - AdapterRegistry → Lifecycle entity: AdapterPublication
//! - DatasetRegistry → Domain entity: DatasetSource

use hf_hub::HFClient;
use hf_hub::repository::{AddSource, RepoTypeDataset, RepoTypeModel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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

/// Resolves and downloads base models from HuggingFace Hub.
///
/// Used by: (removed) (hf:// mount for model loading)
///
/// MDS: Domain entity — `ModelSource` with `hf://` URI scheme.
/// Composition: `CAN resolve_model_id|download_weights|list_variants ON ModelSource VIA API`
///
/// pre:  HF_TOKEN set for gated models
/// post: resolved HF model ID or downloaded weight path

#[async_trait::async_trait]
pub trait ModelRegistry: Send + Sync {
    /// Resolve a provider-prefixed base_model to a HuggingFace model ID.
    ///
    /// Strips known prefixes (OM/, DI/, FA/, TG/, OR/) from the base_model string.
    /// Returns the raw HF model ID (e.g., "Qwen/Qwen3.5-9B").
    fn resolve_model_id(&self, base_model: &str) -> String;

    /// Download model weights to a local cache directory.
    ///
    /// Uses HF_TOKEN for gated model access.
    /// Returns the path to the downloaded weights.
    async fn download_weights(
        &self,
        hf_model_id: &str,
        cache_dir: &Path,
    ) -> Result<PathBuf, HuggingFaceError>;

    /// List available variants/checkpoints for a model.
    ///
    /// Returns a list of branch/tag names (e.g., ["main", "fp16", "gguf"]).
    async fn list_variants(&self, hf_model_id: &str) -> Result<Vec<String>, HuggingFaceError>;
}

/// Publishes and retrieves LoRA adapters via HuggingFace Hub.
///
/// Used by: Runpod (pulls adapters from HF for deployment)
///
/// MDS: Lifecycle entity — `AdapterPublication`.
/// Composition: `CAN publish_adapter|pull_adapter ON Adapter VIA API`
///
/// pre:  adapter weights exist (local or remote)
/// post: adapter published to / pulled from HF Hub
///
/// semantic-graph-audit (M3): `ModelRegistry` (base-model download/variants)
/// and this trait are the same HuggingFace Hub API split in two. Base-vs-adapter
/// is a parameter, not a capability boundary. Candidate to merge into one
/// `HuggingFaceRegistry { publish, pull, variants, download_weights }` to
/// remove the R1 redundancy — defer until the merge is load-bearing.

#[async_trait::async_trait]
pub trait AdapterRegistry: Send + Sync {
    /// Publish a LoRA adapter to a HuggingFace repository.
    ///
    /// Uploads adapter weights + adapter_config.json to the specified repo.
    /// Creates the repo if it doesn't exist. Requires HF_TOKEN with write access.
    ///
    /// Returns the HF Hub URL of the published adapter.
    async fn publish_adapter(
        &self,
        adapter_id: &str,
        weight_path: &Path,
        hf_repo: &str,
    ) -> Result<String, HuggingFaceError>;

    /// Pull/download a LoRA adapter from HuggingFace to local cache.
    ///
    /// Downloads adapter weights + config from the specified HF repo.
    /// Returns the local path to the downloaded adapter directory.
    async fn pull_adapter(
        &self,
        hf_repo: &str,
        revision: Option<&str>,
        cache_dir: &Path,
    ) -> Result<PathBuf, HuggingFaceError>;
}

/// Resolves and downloads training datasets from HuggingFace Hub.
///
/// Used by: DatasetPipeline (optional remote source for hf://datasets/ URLs)
///
/// MDS: Domain entity — `DatasetSource`.
/// Composition: `CAN resolve_dataset|download_dataset ON DatasetSource VIA API`
///
/// pre:  dataset exists on HF Hub
/// post: resolved dataset URL or downloaded local path

#[async_trait::async_trait]
pub trait DatasetRegistry: Send + Sync {
    /// Resolve a HuggingFace dataset ID to a download URL.
    ///
    /// Accepts hf://datasets/username/dataset-name format or bare HF dataset IDs.
    /// Returns a direct download URL for the dataset.
    fn resolve_dataset(&self, dataset_id: &str) -> Result<String, HuggingFaceError>;

    /// Download a dataset from HuggingFace to a local file.
    ///
    /// Returns the local path to the downloaded dataset.
    async fn download_dataset(
        &self,
        dataset_id: &str,
        cache_dir: &Path,
    ) -> Result<PathBuf, HuggingFaceError>;
}

// ── Default implementation for model ID resolution ─────────────────────────

/// Strip known provider prefixes to extract the raw HuggingFace model ID.
///
/// This is the canonical resolution logic used by removed provider.
/// Provider prefixes: DI/ (DeepInfra), FA/ (fal.ai), TG/ (Together), OR/ (OpenRouter).
pub fn resolve_model_id(base_model: &str) -> String {
    let known_prefixes = ["DI/", "FA/", "TG/", "OR/"];
    let mut model = base_model;
    for prefix in &known_prefixes {
        if model.starts_with(prefix) {
            model = &model[prefix.len()..];
            break;
        }
    }
    model.to_string()
}

// ── Reqwest-based ModelRegistry implementation ─────────────────────────────

/// HuggingFace Hub model registry using the HF REST API.
pub struct HfModelRegistry {
    client: reqwest::Client,
    api_key: String,
}

impl HfModelRegistry {
    /// Create a new HuggingFace model registry.
    ///
    /// `api_key` is the HF_TOKEN for gated model access.
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }
}
#[async_trait::async_trait]
impl ModelRegistry for HfModelRegistry {
    fn resolve_model_id(&self, base_model: &str) -> String {
        resolve_model_id(base_model)
    }

    async fn download_weights(
        &self,
        _hf_model_id: &str,
        _cache_dir: &Path,
    ) -> Result<PathBuf, HuggingFaceError> {
        // Weight download via huggingface_hub Python library or hf_transfer.
        // For now, cloud hosts mount via hf:// directly — no local download needed.
        // Local download would use: huggingface_hub.snapshot_download()
        Err(HuggingFaceError::Download(
            "Direct weight download via REST not implemented — use hf:// mount or huggingface_hub CLI".to_string(),
        ))
    }

    async fn list_variants(&self, hf_model_id: &str) -> Result<Vec<String>, HuggingFaceError> {
        let url = format!("https://huggingface.co/api/models/{}/refs", hf_model_id);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| HuggingFaceError::Api(format!("API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(HuggingFaceError::ModelNotFound(hf_model_id.to_string()));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| HuggingFaceError::Api(format!("Parse error: {}", e)))?;

        let branches = json
            .get("branches")
            .and_then(|b| b.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| b.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(branches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_provider_prefix() {
        assert_eq!(resolve_model_id("DI/some-model"), "some-model");
    }

    #[test]
    fn resolve_together_prefix() {
        assert_eq!(resolve_model_id("TG/Qwen/Qwen3.5-9B"), "Qwen/Qwen3.5-9B");
    }

    #[test]
    fn resolve_no_prefix_passthrough() {
        assert_eq!(resolve_model_id("Qwen/Qwen3.5-9B"), "Qwen/Qwen3.5-9B");
    }

    #[test]
    fn resolve_deepinfra_prefix() {
        assert_eq!(
            resolve_model_id("DI/meta-llama/Llama-3.3-70B-Instruct"),
            "meta-llama/Llama-3.3-70B-Instruct"
        );
    }

    #[test]
    fn resolve_fireworks_prefix() {
        assert_eq!(
            resolve_model_id("FA/accounts/fireworks/models/my-model"),
            "accounts/fireworks/models/my-model"
        );
    }
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
pub trait ModelResolver: Send + Sync {
    fn resolve(&self, model_id: &str) -> Result<ModelProvenance, HuggingFaceError>;
    fn validate(&self, model_id: &str) -> bool {
        self.resolve(model_id).is_ok()
    }
}

/// Static model resolver using built-in known-model registry.
#[derive(Default)]
pub struct LocalModelResolver;

impl ModelResolver for LocalModelResolver {
    fn resolve(&self, model_id: &str) -> Result<ModelProvenance, HuggingFaceError> {
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

/// Evidence written by the training pod after training completes, uploaded to
/// the private model repository at `jobs/{job_id}/completion-manifest.json`.
/// Fetched by `training_status` to detect completion (the pod stays RUNNING for
/// SSH debugging, so RunPod's desiredStatus alone cannot signal completion).
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

impl CompletionManifest {
    pub fn validate_for(
        &self,
        job_id: &str,
        dataset_sha256: &str,
    ) -> Result<(), TrainingArtifactError> {
        if self.job_id != job_id {
            return Err(TrainingArtifactError::InvalidManifest(
                "job ID does not match the submitted job".to_string(),
            ));
        }
        if self.status != "success" && self.status != "succeeded" {
            return Err(TrainingArtifactError::InvalidManifest(format!(
                "status is not success or succeeded (got: {})",
                self.status
            )));
        }
        if self.dataset_sha256 != dataset_sha256 {
            return Err(TrainingArtifactError::InvalidManifest(
                "dataset hash does not match the submitted dataset".to_string(),
            ));
        }
        if self.adapter.repository.is_empty() || self.adapter.path.is_empty() {
            return Err(TrainingArtifactError::InvalidManifest(
                "adapter reference is incomplete (repository and path required)".to_string(),
            ));
        }
        Ok(())
    }
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

#[cfg(test)]
mod training_artifact_tests {
    use super::*;

    fn training() -> HuggingFaceTraining {
        HuggingFaceTraining {
            config: HuggingFaceTrainingConfig {
                owner: "owner".to_string(),
                dataset_repo: "datasets".to_string(),
                model_repo: "models".to_string(),
            },
            client: HFClient::builder()
                .token("token-not-logged")
                .build()
                .expect("client"),
        }
    }

    fn dataset_artifact() -> TrainingArtifact {
        TrainingArtifact {
            repository: "owner/datasets".to_string(),
            revision: "a".repeat(40),
            path: "jobs/job-1/dataset.jsonl".to_string(),
            sha256: "a".repeat(64),
        }
    }

    #[tokio::test]
    async fn publish_dataset_rejects_mismatched_sha256_before_network_access() {
        let error = training()
            .publish_dataset("job-1", b"dataset".to_vec(), &"0".repeat(64))
            .await
            .expect_err("mismatched hash must be rejected");
        assert!(matches!(
            error,
            TrainingArtifactError::InvalidConfiguration(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires a dedicated Hugging Face test repository"]
    async fn valid_dataset_upload_uses_the_hub_client_boundary() {
        let bytes = b"dataset".to_vec();
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let error = training()
            .publish_dataset("job-1", bytes, &sha256)
            .await
            .expect_err("test credentials must not authorize a Hub upload");
        assert!(matches!(error, TrainingArtifactError::Upload(_)));
    }

    #[tokio::test]
    async fn artifacts_use_job_scoped_completion_manifest_path() {
        let artifacts = training()
            .prepare_training_artifacts("job-1", dataset_artifact())
            .await
            .expect("valid configured reference creates offline training artifacts");
        assert_eq!(artifacts.model_repository, "owner/models");
        assert_eq!(
            artifacts.completion_manifest_path,
            "jobs/job-1/completion-manifest.json"
        );
    }

    #[tokio::test]
    #[ignore = "requires a dedicated Hugging Face test repository"]
    async fn manifest_retrieval_uses_the_hub_client_boundary() {
        let artifacts = training()
            .prepare_training_artifacts("job-1", dataset_artifact())
            .await
            .expect("valid configured reference creates offline training artifacts");
        let error = training()
            .fetch_completion_manifest(&artifacts)
            .await
            .expect_err("test credentials must not authorize Hub retrieval");
        assert!(matches!(error, TrainingArtifactError::Retrieval(_)));
    }

    #[test]
    fn completion_manifest_parses_without_network_access() {
        let manifest = HuggingFaceTraining::parse_completion_manifest(
            br#"{
                "job_id":"job-1",
                "status":"succeeded",
                "dataset_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "adapter":{
                    "repository":"owner/models",
                    "revision":"revision",
                    "path":"adapter_model.safetensors",
                    "sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                },
                "finished_at":"2026-07-10T00:00:00Z"
            }"#,
        )
        .expect("manifest JSON is valid");
        assert_eq!(manifest.job_id, "job-1");
    }

    #[test]
    fn completion_manifest_requires_matching_job_and_dataset() {
        let manifest = CompletionManifest {
            job_id: "job-1".to_string(),
            status: "success".to_string(),
            dataset_sha256: "dataset-hash".to_string(),
            adapter: TrainingArtifact {
                repository: "owner/models".to_string(),
                revision: "revision".to_string(),
                path: "adapter_model.safetensors".to_string(),
                sha256: "adapter-hash".to_string(),
            },
            finished_at: "2026-07-10T00:00:00Z".to_string(),
            base_model: Some("Qwen3.5-9B".to_string()),
            harness: Some("axolotl".to_string()),
            training_duration_secs: Some(3600),
            loss: Some(0.123),
            output_dir: Some("/workspace/outputs/job-1".to_string()),
        };
        assert!(manifest.validate_for("job-1", "dataset-hash").is_ok());
        assert!(manifest.validate_for("job-2", "dataset-hash").is_err());
        assert!(manifest.validate_for("job-1", "other-hash").is_err());
    }
}
