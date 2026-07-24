#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask MCP Training — Model training data ingestion and fine-tuning server.
//!
//! Exposes 8 tools (simplified from 21 → 15 → 8 across 2026-07-19 cleanups):
//! - `training_ingest_qa` — Ingest QA pairs for model fine-tuning
//! - `training_ingest_dataset` — Ingest a raw dataset into the normalized cache (SFT or preference)
//! - `training_assemble_dataset` — Assemble stored QA pairs into a ChatML JSONL dataset file
//! - `training_submit` — Submit a training job (also handles retrain via optional feedback_path)
//! - `training_status` — Query training job status (auto-registers adapter on completion)
//! - `training_cancel` — Cancel a running job
//! - `training_evaluate` — Evaluate a trained adapter against a test dataset
//! - `training_validate_config` — Run lora-training skill's static math-contract gates on a config
//!
//! Deleted tools (2026-07-19 cleanup, second pass):
//! - training_deploy / training_deployment_status / training_teardown — replaced by
//!   `crate::adapter::AdapterPort::{create_endpoint, endpoint_status, teardown_endpoint}`.
//!   The MCP server was a thin wrapper; deployment now goes through the canonical
//!   AdapterPort surface directly.
//! - training_list_adapters / training_delete_adapter — `AdapterPort::list_adapters` and
//!   `AdapterStore::delete` already cover these. Rare operations; route via CLI.
//! - training_register_adapter — `training_status` auto-registers on completion; manual
//!   registration is an `AdapterStore` API call, not an MCP tool.
//! - training_preflight_check — replaced by `training_validate_config`, which runs the
//!   actual lora-training skill gates (G-M1..G-M4, G-Q1, G-Q2, G-Q4, G-H1) via
//!   `lora_validation::validate_training_params` and emits `reg.lora.audit` spans.
//! - training_retrain — merged into `training_submit` as optional `feedback_path` +
//!   `skill_name` + `adapter_name` parameters (merge + version-bump logic moved there).
//!
//! Deleted tools (2026-07-19 cleanup, first pass):
//! - training_generate_traces, training_generate_chain_of_thought (inference, not training)
//! - training_sweep (use submit in a loop)
//! - training_merge_adapters (speculative, never produced output)
//! - training_record_invocation, training_curate_feedback (data curation, not training)
//! - training_recommend_model (can be done offline)
//!
//! Architecture:
//!   Dataset file → DatasetPipeline → normalized ChatML → TrainingJob → TrainingHost → TrainedLoRAAdapter
//!
//! Host selection: Runpod is the only cloud host. Harness default is Axolotl;
//! per-job harness selection via `TrainingParams.harness` (operator-accepted
//! from the lora-training skill's G6 gate) is honored at submit time.
//! All harnesses support their full trainer taxonomy: Axolotl (SFT),
//! TRL (SFT/DPO/KTO/ORPO/Reward), Ludwig (SFT/DPO/KTO/ORPO/GRPO).
//! Routed through the shared `hkask-services` config init. Host pluggability
//! is via the `TrainingHost` trait, isolating the MCP surface from
//! framework-specific details.
//!
//! lora-training skill integration:
//!   `training_validate_config` is the runtime enforcement point for the
//!   `.agents/skills/lora-training/` skill's `audit-config` phase. The skill
//!   reasons over config files and proposes regressions; this server enforces
//!   the static subset of gates at submit time and emits the `reg.lora.*` spans
//!   the skill's convergence-check phase consumes.
//!
//! # Environment Variables
//!
//! - `HKASK_TRAINING_DB` — Path to per-agent training database for job/adapter/QA storage (defaults to `agents/{userpod}/training.db`)
//! - `HKASK_DB_PASSPHRASE` — Passphrase for the database (resolved via credentials or keystore)
//! - `HKASK_TRAINING_CACHE_DIR` — Dataset cache directory
//! - `RUNPOD_API_KEY` — Runpod API key
//! - `RUNPOD_TEMPLATE_ID` — Runpod GPU pod template ID with axolotl pre-installed
//! - `RUNPOD_GPU_TYPE_ID` — GPU type ID for Runpod pods (default: "NVIDIA RTX 4090")
//! - `RUNPOD_CONTAINER_DISK_GB` — Container disk GB for Runpod pods (default: 50)
//! - `RUNPOD_MIN_MEMORY_GB` — Minimum memory GB for Runpod pods (default: 24)
//! - `HKASK_DATASET_URL` — Public URL for dataset download by Runpod pods
//! - `HKASK_PODS_FILE` — Path to RunPod pod ID persistence file (default: data/training-pods.json)
//!   Ensures pod IDs survive restarts so orphaned pods can be terminated.

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod adapter;
pub mod adapters;
pub mod dataset;
pub mod huggingface;
pub mod lora_validation;
pub mod mlschema;
pub mod providers;
pub mod types;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

mod tools;

use crate::adapter::AdapterRouter;
use crate::adapter::adapter_store::Checksum;
use crate::adapter::expertise::{AdapterLifecycle, Expertise, MdsDomain, TrainingProvenance};
use crate::adapter::{AdapterSource, TrainedLoRAAdapter};
use crate::adapters::{AdapterMetrics, JobStore};
use crate::dataset::DatasetPipeline;
use crate::huggingface::{CompletionManifest, HuggingFaceTraining};
use crate::providers::{
    TrainingHarnessId, TrainingHost, TrainingHostConfig, TrainingHostId, TrainingJobStatus,
    create_host,
};
use hkask_inference::InferenceConfig;
use hkask_memory::SemanticMemory;

use rmcp::tool_router;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

// ── Server ───────────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct TrainingServer {
        pub semantic: Option<SemanticMemory>,
        pub host: Box<dyn TrainingHost>,
        pub host_id: TrainingHostId,
        pub harness_id: TrainingHarnessId,
        pub pipeline: Mutex<DatasetPipeline>,
        pub adapter_store: Arc<crate::adapter::AdapterStore>,
        pub job_store: Option<JobStore>,
        pub adapter_router: Option<Arc<AdapterRouter>>,
        pub inference_config: InferenceConfig,
    }
);

// ── Tools ────────────────────────────────────────────────────────────────

/// Compute SHA-256 checksum of the adapter weights file.
/// Returns `None` if the file cannot be read.
fn compute_adapter_checksum(path: &std::path::Path) -> Option<Checksum> {
    use sha2::Digest;
    let data = std::fs::read(path).ok()?;
    let hash = sha2::Sha256::digest(&data);
    Some(Checksum::from_hex(&format!("{:x}", hash)))
}

#[tool_router(server_handler)]
impl TrainingServer {
    /// Build a `TrainedLoRAAdapter` from training tool parameters.
    ///
    /// Constructs the canonical adapter type directly, with provenance metadata
    /// linking back to the originating training job.
    ///
    /// `checksum` and `storage_path` are computed from the adapter weights file
    /// when `adapter_weight_path` is provided. When `None`, placeholder values
    /// are used (zero checksum, empty path) — the adapter cannot be deployed
    /// via `AdapterRouter` until real values are provided.
    #[allow(clippy::too_many_arguments)]
    fn build_trained_adapter(
        id: String,
        name: String,
        base_model: String,
        dataset_hash: String,
        training_job_id: String,
        created_at_ts: i64,
        size_bytes: u64,
        skill_name: String,
        version: u32,
        metrics: Option<AdapterMetrics>,
        adapter_weight_path: Option<&std::path::Path>,
    ) -> TrainedLoRAAdapter {
        let metrics_json = metrics
            .as_ref()
            .and_then(|m| serde_json::to_value(m).ok())
            .unwrap_or_default();
        let created_at = chrono::DateTime::from_timestamp(created_at_ts, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();
        let provenance = TrainingProvenance {
            training_run_id: training_job_id,
            training_source: String::new(),
            completed_at: created_at.clone(),
            base_model_family: base_model.clone(),
            dataset_hash: if dataset_hash.is_empty() {
                None
            } else {
                Some(dataset_hash)
            },
            training_metrics: metrics_json,
        };
        let expertise = Expertise::new(
            if name.trim().is_empty() {
                format!("adapter-{}", &id[..8.min(id.len())])
            } else {
                name.clone()
            },
            MdsDomain::CodeGeneration,
            serde_json::Value::Null,
            provenance,
        )
        .unwrap_or_else(|_| Expertise {
            name,
            domain: MdsDomain::CodeGeneration,
            capability_manifest: serde_json::Value::Null,
            training_source: TrainingProvenance {
                training_run_id: String::new(),
                training_source: String::new(),
                completed_at: String::new(),
                base_model_family: String::new(),
                dataset_hash: None,
                training_metrics: serde_json::Value::Null,
            },
        });
        let uuid = uuid::Uuid::parse_str(&id).unwrap_or_else(|_| uuid::Uuid::new_v4());
        // Compute checksum and storage_path from the adapter weights file.
        // When no path is provided, use placeholder values — the adapter cannot
        // be deployed via AdapterRouter until real values are set.
        let (checksum, storage_path) = match adapter_weight_path {
            Some(path) => {
                let storage = path.to_string_lossy().to_string();
                let hash = compute_adapter_checksum(path)
                    .unwrap_or_else(|| Checksum::from_hex("0000000000000000"));
                (hash, storage)
            }
            None => (Checksum::from_hex("0000000000000000"), String::new()),
        };
        TrainedLoRAAdapter {
            id: uuid,
            expertise,
            checksum,
            storage_path,
            base_model_family: base_model,
            version: Some(version.to_string()),
            source: AdapterSource::HuggingFace {
                repo: format!("hkask-training/{}", uuid),
            },
            size_bytes: if size_bytes > 0 {
                Some(size_bytes)
            } else {
                None
            },
            owner: hkask_types::id::WebID::from_persona(b"training-pipeline"),
            skill_name: if skill_name.is_empty() {
                None
            } else {
                Some(skill_name)
            },
            lifecycle: AdapterLifecycle::Durable,
            created_at,
        }
    }

    /// Parse the `training_metrics` JSON value from a `TrainedLoRAAdapter` back into
    /// `AdapterMetrics`. Returns `None` if the value is null or cannot be deserialized.
    fn metrics_from_trained(adapter: &TrainedLoRAAdapter) -> Option<AdapterMetrics> {
        serde_json::from_value(adapter.expertise.training_source.training_metrics.clone()).ok()
    }

    /// Check for a completion manifest on HuggingFace to detect whether a
    /// training job has finished. The pod stays RUNNING (exec sleep infinity)
    /// so RunPod's desiredStatus alone cannot signal completion. The install
    /// script writes a manifest to /workspace/completion.json and uploads it
    /// to HuggingFace at jobs/{job_id}/completion-manifest.json after training.
    ///
    /// Returns `Some((status, manifest))` if a manifest was found, or `None`
    /// if no manifest exists yet (training still in progress or HF not configured).
    async fn check_completion_manifest(
        &self,
        job_id: &str,
    ) -> Option<(TrainingJobStatus, Option<CompletionManifest>)> {
        let hf_training = HuggingFaceTraining::from_env().ok()?;
        let job_store = self.job_store.as_ref()?;
        let artifacts = job_store.artifacts(job_id).ok().flatten()?;

        match hf_training.fetch_completion_manifest(&artifacts).await {
            Ok(manifest) => {
                let status = if manifest.status == "success" || manifest.status == "succeeded" {
                    TrainingJobStatus::Completed
                } else {
                    TrainingJobStatus::Failed
                };
                tracing::info!(
                    target: "hkask.training.completion.detected",
                    job_id = %job_id,
                    manifest_status = %manifest.status,
                    detected_status = ?status,
                    "Completion detected via HuggingFace manifest"
                );
                Some((status, Some(manifest)))
            }
            Err(e) => {
                tracing::debug!(
                    target: "hkask.training.completion.check",
                    job_id = %job_id,
                    error = %e,
                    "No completion manifest found (training may still be in progress)"
                );
                None
            }
        }
    }
}

// Tool implementations live in `tools/` submodule — each tool is an
// `impl TrainingServer` block in its own file. The `#[tool_router]` above
// registers all `#[tool]` methods across all impl blocks in the crate.

// ── Entry point ───────────────────────────────────────────────────────────

/// Run the training MCP server (used by binary target).
pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    // Host selection: auto-detect from env vars, or use HKASK_TRAINING_HOST.
    // DeepInfra is preferred when DI_API_KEY is set (B200 at $3.69/hr).
    // Nebius is used when NEBIUS_PROJECT_ID is set (H100 at $3.85/hr).
    // Runpod is the fallback when RUNPOD_API_KEY is set (H100 at $2.39/hr).
    // This matches TrainingHostConfig::default() in providers/mod.rs.
    let host_id = std::env::var("HKASK_TRAINING_HOST")
        .ok()
        .and_then(|h| TrainingHostId::from_str(&h))
        .unwrap_or_else(|| {
            if std::env::var("DI_API_KEY").is_ok() {
                TrainingHostId::DeepInfra
            } else if std::env::var("NEBIUS_PROJECT_ID").is_ok() {
                TrainingHostId::Nebius
            } else {
                TrainingHostId::Runpod
            }
        });
    let harness_id = TrainingHarnessId::Axolotl;

    let cache_dir = PathBuf::from(
        std::env::var("HKASK_TRAINING_CACHE_DIR").unwrap_or_else(|_| {
            hkask_types::agent_paths::userpod_adapters_dir(&userpod)
                .to_string_lossy()
                .to_string()
        }),
    );
    let pipeline = DatasetPipeline::new(cache_dir);

    hkask_mcp_server::run_server_with_preloaded(
        "hkask-mcp-training",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            (|| -> anyhow::Result<TrainingServer> {
                let db_path = ctx
                    .credentials
                    .get("HKASK_TRAINING_DB")
                    .cloned()
                    .unwrap_or_else(|| {
                        let relative = hkask_types::agent_paths::userpod_training_db(&userpod);
                        hkask_types::agent_paths::resolve_under_data_dir(&relative)
                            .to_string_lossy()
                            .to_string()
                    });

                // Resolve passphrase: credentials → keystore resolve_credential chain
                let passphrase = ctx
                    .credentials
                    .get("HKASK_DB_PASSPHRASE")
                    .cloned()
                    .or_else(|| hkask_mcp_server::resolve_credential("HKASK_DB_PASSPHRASE").ok());

                let (semantic, adapter_store, job_store, adapter_router) = match passphrase {
                    Some(passphrase) => {
                        let db = hkask_storage::Database::open(&db_path, &passphrase)
                            .map_err(|e| {
                                anyhow::anyhow!(
                                    "Failed to open training database at {}: {}",
                                    db_path,
                                    e
                                )
                            })?;
                        let pool = db.sqlite_pool().map_err(|e| anyhow::anyhow!("pool: {e}"))?;
                        let job_store = Some(
                            JobStore::new(pool.clone())
                                .map_err(|error| anyhow::anyhow!("job store schema: {error}"))?,
                        );
                        let hmem_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool.clone()));
                        let h_mem_store = hkask_storage::HMemStore::from_driver(Arc::clone(&hmem_driver));
                        let embedding_store = hkask_storage::EmbeddingStore::from_driver(
                            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool)),
                            1024,
                        );
                        let semantic = Some(hkask_memory::SemanticMemory::new(
                            h_mem_store,
                            embedding_store,
                        ));
                        // Canonical adapter store: crate::adapter::AdapterStore stores
                        // TrainedLoRAAdapter in trained_adapters + active_endpoints + lora_blobs.
                        // Schema initialized by from_driver().
                        let store = crate::adapter::AdapterStore::from_driver(hmem_driver);

                        // Build the canonical adapter router (used by AdapterPort for
                        // deployment, status, teardown — the MCP server no longer wraps these).
                        let router = AdapterRouter::new(std::sync::Arc::new(store.clone()));
                        let adapter_router = Some(std::sync::Arc::new(router));

                        (semantic, Arc::new(store), job_store, adapter_router)
                    }
                    None => {
                        // No passphrase configured — fall back to an in-memory driver
                        // so the server still runs (no persistence across restarts).
                        let pool = hkask_storage::database::sqlite::SqliteDriver::in_memory_pool()
                            .map_err(|e| anyhow::anyhow!("in-memory pool: {e}"))?;
                        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));
                        let store = crate::adapter::AdapterStore::from_driver(driver);
                        (None, Arc::new(store), None, None)
                    }
                };

                let host_config = TrainingHostConfig {
                    host: host_id,
                    runpod_api_key: ctx
                        .credentials
                        .get("RUNPOD_API_KEY")
                        .cloned()
                        .unwrap_or_default(),
                    runpod_template_id: ctx
                        .credentials
                        .get("RUNPOD_TEMPLATE_ID")
                        .cloned()
                        .unwrap_or_default(),
                    runpod_gpu_type_id: ctx
                        .credentials
                        .get("RUNPOD_GPU_TYPE_ID")
                        .cloned()
                        .unwrap_or_default(),
                    runpod_container_disk_gb: parse_optional_u32(
                        ctx.credentials.get("RUNPOD_CONTAINER_DISK_GB"),
                    ),
                    runpod_min_memory_gb: parse_optional_u32(
                        ctx.credentials.get("RUNPOD_MIN_MEMORY_GB"),
                    ),
                    runpod_min_vcpu_count: parse_optional_u32(
                        ctx.credentials.get("RUNPOD_MIN_VCPU_COUNT"),
                    ),
                    runpod_docker_image: ctx
                        .credentials
                        .get("RUNPOD_DOCKER_IMAGE")
                        .cloned()
                        .unwrap_or_default(),
                };
                let host = create_host(&host_config)
                    .map_err(|e| anyhow::anyhow!("Failed to create training host: {}", e))?;

                let inference_config = InferenceConfig::from_env();

                Ok(TrainingServer::new(
                    ctx.webid,
                    userpod.clone(),
                    daemon_client.clone(),
                    semantic,
                    host,
                    host_config.host,
                    harness_id,
                    Mutex::new(pipeline.clone()),
                    adapter_store,
                    job_store,
                    adapter_router,
                    inference_config,
                ))
            })()
            .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                context: "training server init".into(),
                detail: e.to_string(),
            })
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "RUNPOD_API_KEY",
                "RunPod API key (required only when using RunPod host)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "DI_API_KEY",
                "DeepInfra API key (required when using DeepInfra host)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "NEBIUS_PROJECT_ID",
                "Nebius project ID (required when using Nebius host)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "NEBIUS_SUBNET_ID",
                "Nebius subnet ID (required when using Nebius host)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_TRAINING_HOST",
                "Training host: runpod, deepinfra, or nebius (auto-detected from available API keys)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "RUNPOD_TEMPLATE_ID",
                "RunPod template ID; defaults to the canonical Axolotl template when unset",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "RUNPOD_GPU_TYPE_ID",
                "RunPod GPU type ID (e.g. \"NVIDIA H100 80GB HBM3\"). Authoritative when set; empty defers to the model-size heuristic",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "RUNPOD_CONTAINER_DISK_GB",
                "Container disk in GB. Authoritative when set; 0/empty defers to the model-size heuristic",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "RUNPOD_MIN_MEMORY_GB",
                "Minimum pod memory in GB. Authoritative when set; 0/empty defers to the default (24)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "RUNPOD_MIN_VCPU_COUNT",
                "Minimum vCPU count. Authoritative when set; 0/empty defers to the default (8)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "RUNPOD_DOCKER_IMAGE",
                "Docker image name. Authoritative when set; empty defers to the canonical Axolotl image",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_TRAINING_DB",
                "Path to per-agent training database for job/adapter/QA storage (defaults to agents/{userpod}/training.db)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PASSPHRASE",
                "Passphrase for the training database (resolved via credentials or keystore; in-memory if unavailable)",
            ),
        ],
        std::collections::HashMap::new(),
    )
    .await
}

/// Parse an optional `u32` credential value.
///
/// Used for the Runpod deployment settings (`RUNPOD_CONTAINER_DISK_GB`,
/// `RUNPOD_MIN_MEMORY_GB`, `RUNPOD_MIN_VCPU_COUNT`) that flow through
/// `ServerContext.credentials` as strings. Returns `0` for `None`, empty
/// string, or unparseable input — `RunpodHost::submit` treats `0` as
/// "operator did not set this" and falls back to the documented default.
///
/// post: returns 0 iff the input is absent, empty, or not a valid u32
fn parse_optional_u32(value: Option<&String>) -> u32 {
    value
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}
