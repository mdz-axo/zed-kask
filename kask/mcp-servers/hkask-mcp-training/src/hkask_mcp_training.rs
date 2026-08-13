#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
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
//! - training_deploy / training_deployment_status / training_teardown — these
//!   wrapped cloud endpoint provisioning via RunPod. The `AdapterPort` trait
//!   and `AdapterRouter` that replaced them were dead code (zero production
//!   callers) and have been removed. If deployment is needed, it should be
//!   re-added as a thin MCP tool over `AdapterStore` + `InferencePort`.
//! - training_list_adapters / training_delete_adapter — `AdapterStore` CRUD
//!   already covers these. Rare operations; route via CLI.
//! - training_register_adapter — `training_status` auto-registers on completion;
//!   manual registration is an `AdapterStore` API call, not an MCP tool.
//! - training_preflight_check — replaced by `training_validate_config`, which
//!   runs the actual lora-training skill gates (G-M1..G-M4, G-Q1, G-Q2, G-Q4,
//!   G-H1) via `lora_validation::validate_training_params` and emits
//!   `reg.lora.audit` spans.
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
//! - `HKASK_TRAINING_DB` — Path to training database for job/adapter/QA storage (defaults to `mcp/training/training.db`)
//! - `HKASK_DB_PASSPHRASE` — Passphrase for the database (resolved via credentials or keystore)
//! - `HKASK_TRAINING_CACHE_DIR` — Dataset cache directory
//! - `RUNPOD_API_KEY` — Runpod API key
//! - `RUNPOD_TEMPLATE_ID` — Runpod GPU pod template ID with axolotl pre-installed
//! - `RUNPOD_GPU_TYPE_ID` — GPU type ID for Runpod pods (default: "NVIDIA RTX 4090")
//! - `RUNPOD_CONTAINER_DISK_GB` — Container disk GB for Runpod pods (default: 50)
//! - `HKASK_DATASET_URL` — Public URL for dataset download by Runpod pods
//! - `HKASK_PODS_FILE` — Path to RunPod pod ID persistence file (default: data/training-pods.json)
//!   Ensures pod IDs survive restarts so orphaned pods can be terminated.

pub mod adapter;
pub mod adapters;
pub mod dataset;
pub mod huggingface;
pub mod lora_validation;
pub mod providers;
pub mod types;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

mod tools;

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
use hkask_memory::MemoryStore;
use hkask_types::InferencePort;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

// ── Server ───────────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct TrainingServer {
        pub store: Option<MemoryStore>,
        pub host: Box<dyn TrainingHost>,
        pub host_id: TrainingHostId,
        pub harness_id: TrainingHarnessId,
        pub pipeline: Mutex<DatasetPipeline>,
        pub adapter_store: Arc<crate::adapter::AdapterStore>,
        pub job_store: Option<JobStore>,
        pub inference_port: Arc<dyn InferencePort>,
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

impl TrainingServer {
    /// Build a `TrainedLoRAAdapter` from training tool parameters.
    ///
    /// Constructs the canonical adapter type directly, with provenance metadata
    /// linking back to the originating training job.
    ///
    /// `checksum` and `storage_path` are computed from the adapter weights file
    /// when `adapter_weight_path` is provided. When `None`, placeholder values
    /// are used (zero checksum, empty path) — the adapter cannot be deployed
    /// until real values are provided.
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
        // be deployed until real values are set.
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
                // Log at warn (not debug) so manifest parse failures are visible.
                // A malformed manifest (e.g., missing required field) means
                // training_status cannot detect completion — the operator should
                // see this rather than wondering why the job appears stuck.
                tracing::warn!(
                    target: "hkask.training.completion.check",
                    job_id = %job_id,
                    error = %e,
                    "Completion manifest not found or unparseable (training may still be in progress, or the manifest is malformed)"
                );
                None
            }
        }
    }
}

// Tool implementations live in `tools/` submodule — each tool is an
// `impl TrainingServer` block in its own file carrying its own
// `#[tool_router(router = <name>_router)]`. The `combined_router()` below
// merges every sub-router; `#[tool_handler]` wires it as the runtime
// `ServerHandler`. (Before this, a single `#[tool_router(server_handler)]`
// on this block registered zero tools — rmcp's macro only scans the impl
// block it is attached to, so the `#[tool]` methods in `tools/*.rs` were
// silently dropped. Pinned by `tool_surface_is_exactly_*_registered_tools`.)

impl TrainingServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::dataset_router()
            + Self::submit_router()
            + Self::evaluate_router()
            + Self::status_router()
            + Self::cancel_router()
            + Self::validate_router()
    }

    /// Map a tool name to its ML-Schema ontology concept URI. The concept
    /// tags the `reg.tool` span (via `execute_tool_semantic`) for type-aware
    /// feedback routing. Training is an ML-experiment surface, so ML-Schema
    /// — the W3C Community Group standard for ML experiments — is the
    /// natural anchor: dataset tools anchor on `mls:Data`, run-lifecycle
    /// tools on `mls:Run`, and evaluation/validation on `mls:Model`.
    ///
    /// ML-Schema reference: <https://www.w3.org/community/ml-schema/>
    fn ontology_anchor(tool: &str) -> Option<&'static str> {
        use hkask_bridge_ontology::mlschema;
        match tool {
            // Dataset ingestion / assembly — data axis
            "training_ingest_qa" | "training_assemble_dataset" | "training_ingest_dataset" => {
                Some(mlschema::DATA)
            }
            // Run lifecycle — submit, status, cancel
            "training_submit" | "training_status" | "training_cancel" => Some(mlschema::RUN),
            // Model axis — evaluation and config validation
            "training_evaluate" | "training_validate_config" => Some(mlschema::MODEL),
            _ => Some(mlschema::RUN),
        }
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for TrainingServer {}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins the full training tool surface is registered end-to-end. Before the
    // `combined_router()` merge, a single `#[tool_router(server_handler)]` on
    // the main impl block registered ZERO tools — rmcp's macro only scans the
    // impl block it is attached to, so the `#[tool]` methods in `tools/*.rs`
    // were silently dropped (the server exposed nothing). This test makes a
    // regression a test-time failure.
    #[test]
    fn tool_surface_is_exactly_8_registered_tools() {
        let n = TrainingServer::combined_router().list_all().len();
        assert_eq!(
            n, 8,
            "training must register all 8 tools across the sub-routers; got {n}"
        );
    }

    // Coverage: every registered tool must have a non-None ontology anchor.
    // Catches the silent-drop failure mode where a new tool is added to a
    // sub-router without a corresponding arm in ontology_anchor. The count
    // pin above catches addition; this test catches anchoring.
    #[test]
    fn ontology_anchor_covers_all_registered_tools() {
        let router = TrainingServer::combined_router();
        for tool in router.list_all() {
            assert!(
                TrainingServer::ontology_anchor(&tool.name).is_some(),
                "ontology_anchor returned None for registered tool '{}'; \
                 add an explicit arm or adjust the fallback",
                tool.name
            );
        }
    }

    // Regression: the ontology anchor must not collapse to a single constant.
    // Dataset tools anchor on mls:Data; run-lifecycle tools on mls:Run;
    // evaluation/validation on mls:Model. A future stub regression would make
    // these equal.
    #[test]
    fn ontology_anchor_distinguishes_tool_families() {
        use hkask_bridge_ontology::mlschema;
        let ingest = TrainingServer::ontology_anchor("training_ingest_qa");
        let submit = TrainingServer::ontology_anchor("training_submit");
        let evaluate = TrainingServer::ontology_anchor("training_evaluate");
        // Data vs Run vs Model — three distinct ML-Schema categories.
        assert_ne!(
            ingest, submit,
            "training_ingest_qa (Data) and training_submit (Run) must anchor on distinct ML-Schema categories"
        );
        assert_ne!(
            submit, evaluate,
            "training_submit (Run) and training_evaluate (Model) must anchor on distinct ML-Schema categories"
        );
        // Specific concept pins.
        assert_eq!(
            ingest,
            Some(mlschema::DATA),
            "training_ingest_qa must anchor on ML-Schema Data"
        );
        assert_eq!(
            submit,
            Some(mlschema::RUN),
            "training_submit must anchor on ML-Schema Run"
        );
        assert_eq!(
            evaluate,
            Some(mlschema::MODEL),
            "training_evaluate must anchor on ML-Schema Model"
        );
    }
}

// ── Entry point ───────────────────────────────────────────────────────────

/// Run the training MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // Host selection: auto-detect from env vars, or use HKASK_TRAINING_HOST.
    // DeepInfra is preferred when DEEPINFRA_API_KEY is set (B200 at $3.69/hr).
    // Nebius is used when NEBIUS_PROJECT_ID is set (H100 at $3.85/hr).
    // Runpod is the fallback when RUNPOD_API_KEY is set (H100 at $2.39/hr).
    // This matches TrainingHostConfig::default() in providers/mod.rs.
    let host_id = std::env::var("HKASK_TRAINING_HOST")
        .ok()
        .and_then(|h| TrainingHostId::from_str(&h))
        .unwrap_or_else(|| {
            if std::env::var("DEEPINFRA_API_KEY").is_ok() {
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
            // D28 — Standardized Artifact Storage. Adapter weights live
            // under `mcp/training/adapters/`.
            hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
                "mcp/training/adapters",
            ))
            .to_string_lossy()
            .to_string()
        }),
    );
    let pipeline = DatasetPipeline::new(cache_dir);

    // Resolve the inference port before the sync server factory closure.
    // This tries the IPC bridge to zed first, falling back to a MediaRouter.
    let inference_port = hkask_inference::resolve_inference_port().await;

    hkask_mcp_server::run_server_with_preloaded(
        "hkask-mcp-training",
        env!("CARGO_PKG_VERSION"),
        move |ctx: hkask_mcp_server::ServerContext| {
            let inference_port = inference_port.clone();
            (|| -> anyhow::Result<TrainingServer> {
                let db_path = std::env::var("HKASK_TRAINING_DB")
                    .unwrap_or_else(|_| {
                        // D28 — Standardized Artifact Storage. Training DB
                        // lives at `mcp/training/training.db`.
                        hkask_types::agent_paths::resolve_under_data_dir(
                            &hkask_types::agent_paths::mcp_server_db("training", "training"),
                        )
                        .to_string_lossy()
                        .to_string()
                    });

                // Resolve passphrase: credentials → keystore resolve_credential chain
                let passphrase = ctx
                    .credentials
                    .get("HKASK_DB_PASSPHRASE")
                    .cloned()
                    .or_else(|| hkask_mcp_server::resolve_credential("HKASK_DB_PASSPHRASE").ok());

                let (store, adapter_store, job_store) = match passphrase {
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
                        let h_mem_store = hkask_storage::HMemStore::from_driver(Arc::clone(&hmem_driver))
                            .map_err(|e| anyhow::anyhow!("hmem store init: {e}"))?;
                        let embedding_store = hkask_storage::EmbeddingStore::from_driver(
                            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool)),
                            1024,
                        )
                        .map_err(|e| anyhow::anyhow!("embedding store init: {e}"))?;
                        let memory_store =
                            Some(hkask_memory::MemoryStore::new(h_mem_store, embedding_store));
                        // Canonical adapter store: crate::adapter::AdapterStore stores
                        // TrainedLoRAAdapter in trained_adapters.
                        // Schema initialized by from_driver().
                        let adapter_store = crate::adapter::AdapterStore::from_driver(hmem_driver)
                            .map_err(|e| anyhow::anyhow!("adapter store init: {e}"))?;

                        (memory_store, Arc::new(adapter_store), job_store)
                    }
                    None => {
                        // No passphrase configured — fall back to an in-memory driver
                        // so the server still runs (no persistence across restarts).
                        tracing::warn!(
                            target = "hkask.training.init",
                            "HKASK_DB_PASSPHRASE not resolved — falling back to in-memory DB; job/adapter state will NOT persist across restarts. Set HKASK_DB_PASSPHRASE via keychain (kask keystore load) or env var."
                        );
                        let pool = hkask_storage::database::sqlite::SqliteDriver::in_memory_pool()
                            .map_err(|e| anyhow::anyhow!("in-memory pool: {e}"))?;
                        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));
                        let adapter_store = crate::adapter::AdapterStore::from_driver(driver)
                            .map_err(|e| anyhow::anyhow!("adapter store init: {e}"))?;
                        (None, Arc::new(adapter_store), None)
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
                    runpod_gpu_type_id: std::env::var("RUNPOD_GPU_TYPE_ID")
                        .unwrap_or_default(),
                    runpod_container_disk_gb: std::env::var("RUNPOD_CONTAINER_DISK_GB")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                    runpod_docker_image: std::env::var("RUNPOD_DOCKER_IMAGE")
                        .unwrap_or_default(),
                };
                let host = create_host(&host_config)
                    .map_err(|e| anyhow::anyhow!("Failed to create training host: {}", e))?;

                Ok(TrainingServer::new(
                    ctx.webid,
                    store,
                    host,
                    host_config.host,
                    harness_id,
                    Mutex::new(pipeline.clone()),
                    adapter_store,
                    job_store,
                    inference_port,
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
                "DEEPINFRA_API_KEY",
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
                "RUNPOD_TEMPLATE_ID",
                "RunPod template ID; defaults to the canonical Axolotl template when unset",
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

    // D28 — pins the default DB path.
    #[test]
    fn default_db_path_follows_standardized_layout() {
        let relative = hkask_types::agent_paths::mcp_server_db("training", "training");
        assert_eq!(
            relative,
            std::path::PathBuf::from("mcp").join("training").join("training.db"),
            "training default DB path must follow mcp/training/training.db"
        );
    }
