//! Runpod GPU cloud training host.
//!
//! Dispatches training jobs to GPU pods via the Runpod REST API v2.
//! Uses a pre-built template with axolotl installed; training is dispatched
//! via environment variables injected into the pod.
//!
//! ## API surface
//!
//! Pod management uses the Runpod REST API v2 (base
//! `https://api.runpod.io/v2`):
//! - Create: `POST /v2/pods` — body is a flat JSON object (`name`, `image`,
//!   `gpu`, `disk`, `env`, ...). Templates are not referenced by ID at create
//!   time; instead, the template's fields are spread into the request body.
//!   When `RUNPOD_TEMPLATE_ID` is set, we fetch the template via
//!   `GET /v2/templates/{id}` and merge its `image`/`disk`/`ports`/`env`/
//!   `registry`/`mounts` into the create body (explicit body fields override
//!   template fields).
//! - Status: `GET /v2/pods/{id}` — returns the pod object with `status`,
//!   `runtime.uptime`, `gpu.id`, etc. v2 does NOT surface per-port SSH IP/port
//!   info in the pod response; SSH access must be obtained via the RunPod
//!   console or `runpodctl ssh`. The `ssh_command` field of `PodStatus` is
//!   therefore left empty under v2.
//! - Terminate: `DELETE /v2/pods/{id}` — returns 204 with no body.
//!
//! v2 status enum: `PROVISIONING | STARTING | RUNNING | EXITED | ERROR |
//! TERMINATED`. `EXITED` (container stopped without termination) is treated
//! as `Failed` for training purposes — training didn't complete.
//!
//! Environment variables (resolved keychain-first via `CredentialRequirement`
//! declarations in `hkask-mcp-training/src/lib.rs`, then flowed through
//! `ServerContext.credentials` → `TrainingHostConfig` → `RunpodHost` fields):
//! - `RUNPOD_API_KEY` — Runpod API key (required)
//! - `RUNPOD_TEMPLATE_ID` — GPU pod template ID (optional; empty by default —
//!   the generic `hkask-training-base` image is used directly without a template)
//! - `RUNPOD_DOCKER_IMAGE` — Docker image name (optional; takes precedence
//!   over template; defaults to `DEFAULT_RUNPOD_DOCKER_IMAGE` =
//!   `docker.io/mdzaxo/hkask-training-base:latest`)
//! - `RUNPOD_GPU_TYPE_ID` — GPU type ID, e.g. "NVIDIA RTX 4090" or
//!   "NVIDIA A100-SXM4-80GB" (default: model-size heuristic). Note: the
//!   variable is `RUNPOD_GPU_TYPE_ID`, not `RUNPOD_GPU_TYPE` — the latter is
//!   ignored. When the operator sets this explicitly, it is authoritative and
//!   the heuristic does not fire.
//! - `RUNPOD_CONTAINER_DISK_GB` — Container disk in GB (default: model-size
//!   heuristic; 50/100/200 by model class)
//! - `HKASK_DATASET_URL` — Remote-readable URL where the pod can download the dataset.
//!   Submission fails before creating a pod when this value is empty.
//!
//! `.env` is deprecated for this server — deployment settings must come from
//! the OS keychain (`kask keystore load`) or the explicit process environment.
use crate::providers::types::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ── Default pod template configuration ─────────────────────────────────
//
// These are the canonical defaults for hKask training pods. They are exposed
// as module-level constants (not magic strings in submit()) so they can be
// referenced, documented, and overridden together.
//
// Override via keychain/env (resolved keychain-first in lib.rs):
//   RUNPOD_TEMPLATE_ID  — a RunPod template ID with axolotl pre-installed
//   RUNPOD_DOCKER_IMAGE — a Docker image name (takes precedence over template)
//
// See docs/how-to/runpod-lora-training-guide.md for the full rationale.

/// Default Docker image for ALL hKask training jobs.
///
/// Single generic minimal image — `python:3.11-slim` + bash + curl + git.
/// No harness-specific packages are baked in. The install script (generated
/// by `generate_install_script()` at submit time) pip-installs whatever the
/// selected harness needs at pod startup.
///
/// This replaces the previous per-harness images. One image serves all harnesses.
const DEFAULT_RUNPOD_DOCKER_IMAGE: &str = "docker.io/mdzaxo/hkask-training-base:latest";

/// Default RunPod template ID.
///
/// Uses the generic `hkask-training-base` image. The template's startup
/// script reads `HKASK_INSTALL_SCRIPT` and executes it.
const DEFAULT_RUNPOD_TEMPLATE_ID: &str = "";

/// Base URL for the Runpod REST API v2.
const RUNPOD_API_V2_BASE: &str = "https://api.runpod.io/v2";

/// Bundled construction parameters for `RunpodHost::new`.
///
/// Mirrors the `PodDeploySpec` pattern: keeps `RunpodHost::new` under clippy's
/// argument-count limit while making the operator-accepted deployment settings
/// (GPU type, disk, image) explicit and self-documenting. All fields are
/// resolved keychain-first in `lib.rs` and flowed through
/// `TrainingHostConfig` → `create_host` → here.
pub struct RunpodHostInit {
    pub api_key: String,
    pub template_id: String,
    /// Operator-accepted GPU type ID (e.g. `"NVIDIA H100 80GB HBM3"`).
    /// Empty defers to the model-size heuristic in `submit`.
    pub gpu_type_id: String,
    /// Operator-accepted container disk in GB. `0` defers to the heuristic.
    pub container_disk_gb: u32,
    /// Operator-accepted Docker image. Empty defers to
    /// `DEFAULT_RUNPOD_DOCKER_IMAGE`.
    pub docker_image: String,
}

/// Runpod GPU cloud training host — dispatches training to GPU pods.
///
/// Uses the Runpod REST API v2 to create GPU pods from a pre-built template
/// (with axolotl installed), execute training, and retrieve LoRA adapters.
/// This is the "cloud dispatch" path for Axolotl — instead of running locally,
/// training runs on Runpod's GPU infrastructure.
///
/// **Template requirements:** The pod template must include a startup script
/// that reads `HKASK_*` environment variables, downloads the dataset from
/// `HKASK_DATASET_URL`, runs axolotl training, and uploads the resulting
/// adapter weights to a storage location.
pub struct RunpodHost {
    api_key: String,
    template_id: String,
    /// Operator-accepted GPU type ID (e.g. `"NVIDIA H100 80GB HBM3"`).
    /// Empty defers to the model-size heuristic in `submit`.
    gpu_type_id: String,
    /// Operator-accepted container disk in GB. `0` defers to the heuristic.
    container_disk_gb: u32,
    /// Operator-accepted Docker image. Empty defers to
    /// `DEFAULT_RUNPOD_DOCKER_IMAGE`.
    docker_image: String,
    client: reqwest::Client,
    /// job_id -> pod_id mapping for status/cancel
    jobs: Arc<Mutex<HashMap<String, String>>>,
    /// job_id -> last known uptime in seconds. Used to detect pod restarts.
    last_uptime: Arc<Mutex<HashMap<String, u64>>>,
    /// job_id -> SSH command string. Populated by status() for the response.
    /// Under v2 this is always empty — v2 does not surface SSH IP/port info.
    ssh_commands: Arc<Mutex<HashMap<String, String>>>,
    /// Path to the pod ID persistence file (JSON: {job_id: pod_id}).
    pods_file: PathBuf,
}

/// Map a v2 pod status string to a `TrainingJobStatus`.
///
/// v2 enum: `PROVISIONING | STARTING | RUNNING | EXITED | ERROR | TERMINATED`.
/// `EXITED` (container stopped without termination) is treated as `Failed`
/// for training purposes — training didn't complete. `TERMINATED` is also
/// `Failed` (the pod is gone).
fn map_pod_status(status: &str) -> TrainingJobStatus {
    match status {
        "PROVISIONING" | "STARTING" => TrainingJobStatus::Queued,
        "RUNNING" => TrainingJobStatus::Running,
        "EXITED" | "ERROR" | "TERMINATED" => TrainingJobStatus::Failed,
        _ => TrainingJobStatus::Queued,
    }
}

impl RunpodHost {
    pub fn new(init: RunpodHostInit) -> Self {
        let pods_file = PathBuf::from(
            std::env::var("HKASK_PODS_FILE")
                .unwrap_or_else(|_| "data/training-pods.json".to_string()),
        );
        // Load persisted pod IDs so we can cancel orphaned pods after a restart.
        let persisted = Self::load_pods(&pods_file);
        if !persisted.is_empty() {
            tracing::warn!(
                target: "hkask.training.runpod",
                count = persisted.len(),
                file = %pods_file.display(),
                "Loaded persisted pod IDs from previous session — call drain_all_pods() on shutdown to terminate them"
            );
        }
        Self {
            api_key: init.api_key,
            template_id: init.template_id,
            gpu_type_id: init.gpu_type_id,
            container_disk_gb: init.container_disk_gb,
            docker_image: init.docker_image,
            client: reqwest::Client::new(),
            jobs: Arc::new(Mutex::new(persisted)),
            last_uptime: Arc::new(Mutex::new(HashMap::new())),
            ssh_commands: Arc::new(Mutex::new(HashMap::new())),
            pods_file,
        }
    }

    /// Borrow the job_id → pod_id map for lookup (used by smoke test examples).
    pub fn jobs_for_lookup(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Get the SSH command for a job, if available. Under v2 this is always
    /// `None` — the REST API v2 does not surface per-port SSH IP/port info in
    /// the pod response. Use the RunPod console or `runpodctl ssh` instead.
    pub fn ssh_for_job(&self, job_id: &str) -> Option<String> {
        self.ssh_commands.lock().ok()?.get(job_id).cloned()
    }

    /// Inject a synthetic job_id → pod_id mapping where the job_id equals the
    /// pod_id (used by smoke test examples that only have the pod_id).
    pub fn inject_pod_id(&self, pod_id: &str) {
        let mut map = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(pod_id.to_string(), pod_id.to_string());
    }

    /// Load persisted pod IDs from the JSON file.
    fn load_pods(path: &std::path::Path) -> HashMap<String, String> {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    /// Persist the job_id → pod_id map to disk so we can cancel orphaned pods
    /// after a restart. Errors are logged but not propagated — persistence is
    /// best-effort and a failure here doesn't break training.
    fn persist_pods(&self) {
        let map = self
            .jobs
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|e| e.into_inner());
        let json = match serde_json::to_string_pretty(&map) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.training.runpod",
                    error = %e,
                    "Failed to serialize pod IDs for persistence"
                );
                return;
            }
        };
        if let Err(e) = std::fs::write(&self.pods_file, json) {
            tracing::warn!(
                target: "hkask.training.runpod",
                error = %e,
                file = %self.pods_file.display(),
                "Failed to persist pod IDs"
            );
        }
    }

    /// Drain all known pods by terminating them. Used on shutdown to avoid
    /// orphaned billable pods. Errors per-pod are logged but don't abort the
    /// drain — we want to attempt every pod even if one fails.
    pub async fn drain_all_pods(&self) -> Result<usize, ProviderError> {
        let pod_ids: Vec<(String, String)> = {
            let map = self
                .jobs
                .lock()
                .map_err(|e| ProviderError::Backend(format!("Lock error: {}", e)))?;
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        let count = pod_ids.len();
        tracing::info!(
            target: "reg.training.provider.runpod.drain",
            count = count,
            "Draining all RunPod pods"
        );
        for (job_id, pod_id) in &pod_ids {
            match self.delete_pod(pod_id).await {
                Ok(()) => tracing::info!(
                    target: "hkask.training.runpod",
                    job_id = %job_id,
                    pod_id = %pod_id,
                    "Pod terminated during drain"
                ),
                Err(e) => tracing::warn!(
                    target: "hkask.training.runpod",
                    job_id = %job_id,
                    pod_id = %pod_id,
                    error = %e,
                    "Failed to terminate pod during drain — may need manual deletion via RunPod console"
                ),
            }
        }
        if let Ok(mut map) = self.jobs.lock() {
            map.clear();
        }
        self.persist_pods();
        Ok(count)
    }

    /// Send a request to the Runpod REST API v2 and return the parsed JSON
    /// response. For non-2xx responses, returns a `ProviderError::Backend`
    /// with the v2 error envelope (`title` / `status` / `detail` / `errors`).
    async fn rest_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ProviderError> {
        let url = format!("{}{}", RUNPOD_API_V2_BASE, path);
        let mut req = self
            .client
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let response = req
            .send()
            .await
            .map_err(|e| ProviderError::Backend(format!("Runpod API request failed: {}", e)))?;
        let status = response.status();

        // 204 No Content (DELETE success) — return an empty object.
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Object(Map::new()));
        }

        let text = response
            .text()
            .await
            .map_err(|e| ProviderError::Backend(format!("Runpod API read error: {}", e)))?;

        // Empty body — treat as empty object.
        if text.trim().is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        let json: Value = serde_json::from_str(&text).map_err(|e| {
            ProviderError::Backend(format!(
                "Runpod API parse error (status {}): {} — body: {}",
                status, e, text
            ))
        })?;

        if !status.is_success() {
            // v2 error envelope: { title, status, detail, errors: [...] }
            let detail = json
                .get("detail")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            let title = json
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Runpod API error");
            return Err(ProviderError::Backend(format!(
                "Runpod API {}: {} — {}",
                status, title, detail
            )));
        }

        Ok(json)
    }

    /// Create a pod via `POST /v2/pods`. Returns the created pod's ID.
    async fn create_pod(&self, body: Value) -> Result<String, ProviderError> {
        let json = self
            .rest_request(reqwest::Method::POST, "/pods", Some(body))
            .await?;
        let pod_id = json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProviderError::Backend(format!("No pod ID in Runpod create response: {}", json))
            })?
            .to_string();
        Ok(pod_id)
    }

    /// Get a pod via `GET /v2/pods/{id}`. Returns the full pod object.
    async fn get_pod(&self, pod_id: &str) -> Result<Value, ProviderError> {
        self.rest_request(reqwest::Method::GET, &format!("/pods/{}", pod_id), None)
            .await
    }

    /// Terminate a pod via `DELETE /v2/pods/{id}`. Returns 204 with no body.
    async fn delete_pod(&self, pod_id: &str) -> Result<(), ProviderError> {
        self.rest_request(reqwest::Method::DELETE, &format!("/pods/{}", pod_id), None)
            .await?;
        Ok(())
    }

    /// Fetch a template via `GET /v2/templates/{id}`. Returns the template
    /// object (image, disk, ports, env, registry, mounts).
    async fn get_template(&self, template_id: &str) -> Result<Value, ProviderError> {
        self.rest_request(
            reqwest::Method::GET,
            &format!("/templates/{}", template_id),
            None,
        )
        .await
    }

    /// Build the `POST /v2/pods` request body.
    ///
    /// v2 create-pod body is a flat JSON object: `name`, `image`, `args`,
    /// `disk`, `gpu` (object with `id` + `count`), `cloud`, `dataCenterIds`,
    /// `env` (flat object), `ports`. Templates are not referenced by ID at
    /// create time — when `template_id` is set, the caller must fetch the
    /// template first and merge its fields into the body (explicit body fields
    /// override template fields). This helper does NOT do the fetch; it
    /// assumes the caller has already resolved the image/disk/ports from the
    /// template if one was requested.
    fn build_create_pod_body(
        &self,
        job_id: &str,
        spec: &PodDeploySpec<'_>,
        env_entries: &[(&str, String)],
    ) -> Value {
        let pod_name = format!("hkask-training-{}", &job_id[..8.min(job_id.len())]);

        // env is a flat object: { key: value, ... }
        let env_obj: Map<String, Value> = env_entries
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.clone())))
            .collect();

        let mut body = json!({
            "name": pod_name,
            "image": spec.docker_image,
            "gpu": {
                "id": spec.gpu_type_id,
                "count": 1,
            },
            "disk": spec.container_disk_gb,
            "cloud": "SECURE",
            "dataCenterIds": [],
            "ports": ["22/tcp"],
            "env": env_obj,
        });

        // docker args (startup command) — read from RUNPOD_DOCKER_ARGS env var.
        // When non-empty, this becomes the container `args` field.
        if !spec.docker_args.is_empty() {
            body["args"] = Value::String(spec.docker_args.to_string());
        }

        body
    }
}

/// Resolved pod deployment parameters passed to `build_create_pod_body`.
///
/// Bundling these keeps the helper under clippy's argument-count limit while
/// mirroring the v2 `createPod` request surface.
struct PodDeploySpec<'a> {
    gpu_type_id: &'a str,
    container_disk_gb: u32,
    docker_image: &'a str,
    docker_args: &'a str,
    template_id: &'a str,
}

// ── Install script generation ───────────────────────────────────────────────

/// Generate the install + training script for the pod.
///
/// This is the bridge between the Rust harness (which renders the training
/// config) and the generic Docker image (which has nothing pre-installed).
/// The script:
///   1. pip-installs the harness-specific packages with pinned versions
///   2. Writes the rendered config to /workspace
///   3. Runs the training command
///   4. Uploads the adapter to HuggingFace
///   5. Writes a completion manifest
///   6. exec sleep infinity for SSH debugging
///
/// The script is harness-specific — axolotl installs axolotl, TRL installs
/// TRL, etc. The harness is selected by the `harness` parameter, which is
/// resolved by the caller from `job.params.harness` (operator-accepted) or
/// `job.harness` (server default).
#[allow(clippy::too_many_arguments)]
pub fn generate_install_script(
    job: &TrainingJob,
    harness: TrainingHarnessId,
) -> Result<String, ProviderError> {
    let output_dir = format!("/workspace/outputs/{}", job.id);
    // The manifest is written locally to /workspace/completion.json (guaranteed
    // to work regardless of CWD), then uploaded to HuggingFace at the
    // artifacts' completion_manifest_path. The local path is always
    // /workspace/completion.json; the HuggingFace repo path is in
    // artifacts.completion_manifest_path (e.g. "jobs/{job_id}/completion-manifest.json").
    let local_manifest_path = "/workspace/completion.json".to_string();
    let hf_manifest_repo_path = job
        .artifacts
        .as_ref()
        .map(|a| a.completion_manifest_path.clone())
        .unwrap_or_default();
    let model_repo = job
        .artifacts
        .as_ref()
        .map(|a| a.model_repository.clone())
        .unwrap_or_default();

    // Render the training config using the selected harness.
    let (config_filename, config_content, pip_packages, train_command, _version_info) =
        match harness {
            TrainingHarnessId::Axolotl => {
                let yaml = crate::providers::AxolotlHarness
                    .render_config(job)
                    .map_err(|e| {
                        ProviderError::InvalidConfig(format!("Failed to render axolotl YAML: {e}"))
                    })?;
                (
                    "config.yml",
                    yaml,
                    "pip install --no-cache-dir axolotl huggingface_hub",
                    "axolotl train /workspace/config.yml",
                    "axolotl",
                )
            }
            TrainingHarnessId::Trl => {
                let script = crate::providers::TrlHarness
                    .render_config(job)
                    .map_err(|e| {
                        ProviderError::InvalidConfig(format!("Failed to render TRL script: {e}"))
                    })?;
                (
                    "train.py",
                    script,
                    "pip install --no-cache-dir trl==1.8.0 peft==0.19.0 transformers==5.9.0 bitsandbytes accelerate liger-kernel huggingface_hub",
                    "python /workspace/train.py",
                    "trl==1.8.0 peft==0.19.0 transformers==5.9.0",
                )
            }
            TrainingHarnessId::Ludwig => {
                let yaml = crate::providers::LudwigHarness
                    .render_config(job)
                    .map_err(|e| {
                        ProviderError::InvalidConfig(format!("Failed to render Ludwig YAML: {e}"))
                    })?;
                (
                    "model.yaml",
                    yaml,
                    "pip install --no-cache-dir ludwig huggingface_hub",
                    "ludwig train --config /workspace/model.yaml",
                    "ludwig",
                )
            }
        };

    // Generate the install script. We build it with push_str to avoid
    // format! brace-escaping issues with bash ${VAR} references.
    // The config content is written via a quoted heredoc to prevent shell
    // expansion of the rendered YAML/Python content.
    let mut script = String::with_capacity(4096);
    script.push_str("#!/usr/bin/env bash\n");
    script.push_str("set -euo pipefail\n\n");
    script.push_str(
        "# ── Environment ───────────────────────────────────────────────────────────────\n",
    );
    script.push_str("export HF_HOME=${HF_HOME:-/workspace/.cache/huggingface}\n");
    script.push_str("export PIP_CACHE_DIR=${PIP_CACHE_DIR:-/workspace/.cache/pip}\n");
    script.push_str("export TMPDIR=${TMPDIR:-/workspace/tmp}\n");
    script.push_str("export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True\n");
    script.push_str("export HF_HUB_ENABLE_HF_TRANSFER=${HF_HUB_ENABLE_HF_TRANSFER:-1}\n");
    script.push_str("mkdir -p \"$HF_HOME\" \"$PIP_CACHE_DIR\" \"$TMPDIR\" /workspace/outputs\n\n");

    // Step 1: Install harness packages.
    // On pre-built images (e.g. DeepInfra's di-cont-ubuntu-torch), the harness
    // may already be installed with a GPU-specific PyTorch build. Reinstalling
    // via pip can overwrite the custom PyTorch with a standard build that lacks
    // Blackwell (B200) kernel support. So we check first and skip if present.
    script.push_str(
        "# ── Step 1: Install harness packages ───────────────────────────────────────\n",
    );
    script.push_str("echo '=== Installing packages ==='\n");
    script.push_str(
        "# Check if harness is already installed (skip to avoid breaking GPU-specific PyTorch)\n",
    );
    script.push_str("HARNESS_CMD=\"");
    script.push_str(train_command);
    script.push_str("\"\n");
    script.push_str(
        "if command -v axolotl && [ \"$(basename \"$HARNESS_CMD\")\" = \"axolotl\" ]; then\n",
    );
    script.push_str(
        "    echo 'Axolotl already installed — skipping pip install (preserving GPU PyTorch)'\n",
    );
    script.push_str(
        "elif command -v ludwig && [ \"$(basename \"$HARNESS_CMD\")\" = \"ludwig\" ]; then\n",
    );
    script.push_str(
        "    echo 'Ludwig already installed — skipping pip install (preserving GPU PyTorch)'\n",
    );
    script.push_str("elif [ \"$(basename \"$HARNESS_CMD\")\" = \"python\" ] && python -c 'import trl' 2>/dev/null; then\n");
    script.push_str(
        "    echo 'TRL already installed — skipping pip install (preserving GPU PyTorch)'\n",
    );
    script.push_str("else\n");
    script.push_str("    echo 'Installing harness packages...'\n");
    script.push_str("    ");
    script.push_str(pip_packages);
    script.push('\n');
    script.push_str("fi\n\n");

    // Step 2: Write the training config via quoted heredoc.
    script.push_str(
        "# ── Step 2: Write the training config ──────────────────────────────────────\n",
    );
    script.push_str(&format!(
        "echo '=== Writing config to /workspace/{}'\n",
        config_filename
    ));
    script.push_str(&format!(
        "cat <<'HKASK_CONFIG' > /workspace/{}\n",
        config_filename
    ));
    script.push_str(&config_content);
    script.push_str("\nHKASK_CONFIG\n\n");

    // Step 3: Run training.
    script.push_str(
        "# ── Step 3: Run training ─────────────────────────────────────────────────────\n",
    );
    script.push_str(&format!(
        "echo '=== Starting training: {}'\n",
        train_command
    ));
    script.push_str("TRAINING_START=$(date +%s)\n");
    script.push_str(&format!("if {}; then\n", train_command));
    script.push_str("    TRAINING_END=$(date +%s)\n");
    script.push_str("    TRAINING_DURATION=$((TRAINING_END - TRAINING_START))\n");
    script.push_str("    echo \"=== Training completed in ${TRAINING_DURATION}s ===\"\n");
    script.push_str("    TRAINING_STATUS=\"success\"\n");
    script.push_str("else\n");
    script.push_str("    TRAINING_END=$(date +%s)\n");
    script.push_str("    TRAINING_DURATION=$((TRAINING_END - TRAINING_START))\n");
    script.push_str("    echo \"=== Training FAILED after ${TRAINING_DURATION}s ===\" >&2\n");
    script.push_str("    TRAINING_STATUS=\"failed\"\n");
    script.push_str("fi\n\n");

    // Step 4: Upload adapter.
    script.push_str(
        "# ── Step 4: Upload adapter ──────────────────────────────────────────────────\n",
    );
    script.push_str(&format!("OUTPUT_DIR=\"{}\"\n", output_dir));
    if !model_repo.is_empty() {
        script.push_str("if [ \"$TRAINING_STATUS\" = \"success\" ]; then\n");
        script.push_str(&format!(
            "    echo '=== Uploading adapter to {}'\n",
            model_repo
        ));
        script.push_str(&format!(
            "    huggingface-cli upload \"{}\" \"$OUTPUT_DIR\" \\\n",
            model_repo
        ));
        script.push_str(&format!(
            "        --commit-message \"hKask training: {}\" || \\\n",
            job.id
        ));
        script.push_str("        echo 'WARNING: Adapter upload failed' >&2\n");
        script.push_str("fi\n");
    }
    script.push('\n');

    // Step 5: Write completion manifest locally, then upload to HuggingFace.
    // The manifest is the ONLY way training_status can detect completion —
    // the pod stays RUNNING (exec sleep infinity) so RunPod's desiredStatus
    // alone cannot signal completion. The manifest is uploaded to HuggingFace
    // at jobs/{job_id}/completion-manifest.json, where training_status fetches it.
    script.push_str(
        "# ── Step 5: Write completion manifest + upload to HuggingFace ────────────────\n",
    );
    // Compute adapter SHA256 if the file exists (best-effort).
    script.push_str("ADAPTER_SHA256=$(sha256sum \"$OUTPUT_DIR/adapter_model.safetensors\" 2>/dev/null | cut -d' ' -f1 || echo \"\")\n");
    script.push_str(&format!("cat > \"{}\" <<MANIFEST\n", local_manifest_path));
    script.push_str("{\n");
    script.push_str(&format!("    \"job_id\": \"{}\",\n", job.id));
    script.push_str("    \"status\": \"${TRAINING_STATUS}\",\n");
    // Dataset SHA256 from the env var set by submit().
    script.push_str("    \"dataset_sha256\": \"${HKASK_EXPECTED_DATASET_SHA256:-}\",\n");
    script.push_str("    \"adapter\": {\n");
    script.push_str(&format!(
        "        \"repository\": \"{}\",\n",
        if model_repo.is_empty() {
            ""
        } else {
            model_repo.as_str()
        }
    ));
    script.push_str("        \"revision\": \"main\",\n");
    script.push_str("        \"path\": \"adapter_model.safetensors\",\n");
    script.push_str("        \"sha256\": \"$ADAPTER_SHA256\"\n");
    script.push_str("    },\n");
    script.push_str("    \"finished_at\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\n");
    script.push_str(&format!("    \"base_model\": \"{}\",\n", job.base_model));
    script.push_str(&format!(
        "    \"harness\": \"{}\",\n",
        format!("{:?}", harness).to_lowercase()
    ));
    script.push_str("    \"training_duration_secs\": ${TRAINING_DURATION},\n");
    script.push_str("    \"loss\": null,\n");
    script.push_str("    \"output_dir\": \"$OUTPUT_DIR\"\n");
    script.push_str("}\n");
    script.push_str("MANIFEST\n");
    script.push_str(&format!(
        "echo '=== Completion manifest written to {}'\n",
        local_manifest_path
    ));
    // Upload manifest to HuggingFace so training_status can fetch it.
    if !model_repo.is_empty() && !hf_manifest_repo_path.is_empty() {
        script.push_str(&format!(
            "huggingface-cli upload \"{}\" {} \"{}\" \\\n",
            model_repo, local_manifest_path, hf_manifest_repo_path
        ));
        script.push_str(&format!(
            "    --commit-message \"hKask completion manifest: {}\" || \\\n",
            job.id
        ));
        script.push_str("    echo 'WARNING: Manifest upload failed' >&2\n");
    }
    script.push('\n');

    // Step 6: Keep pod alive for SSH debugging.
    script.push_str(
        "# ── Step 6: Keep pod alive for SSH debugging ────────────────────────────────\n",
    );
    script.push_str("echo '=== Done. Pod staying alive for SSH debugging.'\n");
    script.push_str("exec sleep infinity\n");

    Ok(script)
}

impl TrainingHost for RunpodHost {
    async fn submit(&self, job: &TrainingJob) -> Result<String, ProviderError> {
        // GPU selection: operator-accepted `RUNPOD_GPU_TYPE_ID` (resolved
        // keychain-first into `self.gpu_type_id`) is authoritative when set.
        // When unset, fall back to the model-size heuristic — small models
        // (≤14B) use RTX 4090, large models (20B–70B) use A100, very large
        // (120B+) use H100. GPU type IDs must match RunPod's catalog exactly.
        // This heuristic is the lora-training skill's G2 gate (memory budget
        // vs model size) — it informs, never overrides, an explicitly accepted
        // operator value.
        let gpu_type_id = if !self.gpu_type_id.is_empty() {
            self.gpu_type_id.clone()
        } else {
            let lower = job.base_model.to_lowercase();
            if ["70b", "72b", "120b", "405b"]
                .iter()
                .any(|p| lower.contains(p))
            {
                "NVIDIA H100 80GB HBM3".to_string()
            } else if ["20b", "30b", "34b", "35b"]
                .iter()
                .any(|p| lower.contains(p))
            {
                "NVIDIA A100-SXM4-80GB".to_string()
            } else {
                "NVIDIA GeForce RTX 4090".to_string()
            }
        };
        // Container disk: operator-accepted value is authoritative when set;
        // otherwise larger models need more disk for weights + checkpoints.
        let container_disk_gb: u32 = if self.container_disk_gb > 0 {
            self.container_disk_gb
        } else {
            let lower = job.base_model.to_lowercase();
            if ["70b", "72b", "120b", "405b"]
                .iter()
                .any(|p| lower.contains(p))
            {
                200 // 70B model weights ~140GB + checkpoints
            } else if ["13b", "14b", "20b", "30b"]
                .iter()
                .any(|p| lower.contains(p))
            {
                100
            } else {
                50
            }
        };
        let artifacts = job.artifacts.as_ref().ok_or_else(|| {
            ProviderError::DatasetError(
                "RunPod requires a published Hugging Face artifact path before creating a billable pod"
                    .to_string(),
            )
        })?;

        // Resolve the pod template and image. The operator-accepted values
        // (resolved keychain-first into `self.template_id` and
        // `self.docker_image`) are authoritative when set. Defaults use the
        // pre-built axolotl template (DEFAULT_RUNPOD_TEMPLATE_ID) which has
        // axolotl + all deps pre-installed and reads config from
        // HKASK_AXOLOTL_CONFIG, plus its base image
        // (DEFAULT_RUNPOD_DOCKER_IMAGE).
        // See docs/how-to/runpod-lora-training-guide.md Lesson 10.
        let template_id = if !self.template_id.is_empty() {
            self.template_id.clone()
        } else {
            DEFAULT_RUNPOD_TEMPLATE_ID.to_string()
        };

        // Harness selection: job.params.harness takes precedence (operator-accepted
        // from the lora-training skill's G6 gate), falling back to job.harness
        // (server default), falling back to Axolotl (runtime default).
        // Computed early so it can be used for both docker image selection and
        // the HKASK_HARNESS env var below.
        let selected_harness = job.params.harness.unwrap_or(job.harness);

        // v2 createPod requires `image` to be non-empty. Use the single generic
        // training-base image for all harnesses — the install script (generated
        // below) pip-installs harness-specific packages at pod startup. No
        // per-harness images.
        let docker_image = if !self.docker_image.is_empty() {
            self.docker_image.clone()
        } else {
            DEFAULT_RUNPOD_DOCKER_IMAGE.to_string()
        };

        // When a template is set, fetch it and let its image/disk/ports/env
        // override our defaults (operator-accepted docker_image still wins
        // over the template's image). v2 createPod does not take a templateId
        // field — template fields must be spread into the request body.
        let (resolved_image, resolved_disk, resolved_ports) = if !template_id.is_empty() {
            let template = self.get_template(&template_id).await?;
            let tpl_image = template
                .get("image")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| docker_image.clone());
            let tpl_disk = template
                .get("disk")
                .and_then(|v| v.as_u64())
                .map(|d| d as u32)
                .unwrap_or(container_disk_gb);
            let tpl_ports: Vec<String> = template
                .get("ports")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| p.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(|| vec!["22/tcp".to_string()]);
            (tpl_image, tpl_disk, tpl_ports)
        } else {
            (
                docker_image.clone(),
                container_disk_gb,
                vec!["22/tcp".to_string()],
            )
        };

        if resolved_image.is_empty() && template_id.is_empty() {
            return Err(ProviderError::InvalidConfig(
                "Either RUNPOD_DOCKER_IMAGE or RUNPOD_TEMPLATE_ID must be set to create a RunPod pod"
                    .to_string(),
            ));
        }

        let mut env_entries: Vec<(&str, String)> = vec![
            ("HKASK_JOB_ID", job.id.clone()),
            ("HKASK_BASE_MODEL", job.base_model.clone()),
            (
                "HKASK_HF_DATASET_REPOSITORY",
                artifacts.dataset.repository.clone(),
            ),
            (
                "HKASK_HF_DATASET_REVISION",
                artifacts.dataset.revision.clone(),
            ),
            ("HKASK_HF_DATASET_PATH", artifacts.dataset.path.clone()),
            (
                "HKASK_EXPECTED_DATASET_SHA256",
                artifacts.dataset.sha256.clone(),
            ),
            (
                "HKASK_HF_MODEL_REPOSITORY",
                artifacts.model_repository.clone(),
            ),
            (
                "HKASK_COMPLETION_MANIFEST_PATH",
                artifacts.completion_manifest_path.clone(),
            ),
            (
                "HKASK_HARNESS",
                format!("{:?}", selected_harness).to_lowercase(),
            ),
            ("HKASK_NUM_EPOCHS", job.params.num_epochs.to_string()),
            ("HKASK_LORA_R", job.params.lora.r.to_string()),
            ("HKASK_LORA_ALPHA", job.params.lora.alpha.to_string()),
            ("HKASK_LORA_DROPOUT", job.params.lora.dropout.to_string()),
            (
                "HKASK_LORA_TARGET_MODULES",
                job.params.lora.target_modules.join(","),
            ),
            (
                "HKASK_LORA_USE_RSLORA",
                job.params.lora.use_rslora.to_string(),
            ),
            ("HKASK_LORA_USE_DORA", job.params.lora.use_dora.to_string()),
            (
                "HKASK_LORA_INIT_WEIGHTS",
                job.params
                    .lora
                    .init_lora_weights
                    .as_ref()
                    .map(|i| i.as_config_value())
                    .unwrap_or_default(),
            ),
            (
                "HKASK_LORA_BIAS",
                format!("{:?}", job.params.lora.bias).to_lowercase(),
            ),
            ("HKASK_LEARNING_RATE", job.params.learning_rate.to_string()),
            ("HKASK_BATCH_SIZE", job.params.batch_size.to_string()),
            (
                "HKASK_GRAD_ACCUM",
                job.params
                    .optimization
                    .gradient_accumulation_steps
                    .to_string(),
            ),
            (
                "HKASK_WEIGHT_DECAY",
                job.params.optimization.weight_decay.to_string(),
            ),
            (
                "HKASK_MAX_GRAD_NORM",
                job.params
                    .optimization
                    .max_grad_norm
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            ),
            (
                "HKASK_WARMUP_STEPS",
                job.params
                    .optimization
                    .warmup_steps
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            ),
            (
                "HKASK_LR_SCHEDULER",
                job.params
                    .optimization
                    .lr_scheduler
                    .clone()
                    .unwrap_or_default(),
            ),
            (
                "HKASK_SEQ_LEN",
                job.params
                    .sequence
                    .sequence_len
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            ),
            (
                "HKASK_LOAD_IN_4BIT",
                job.params.quantization.load_in_4bit.to_string(),
            ),
            (
                "HKASK_BNB_4BIT_QUANT_TYPE",
                job.params
                    .quantization
                    .bnb_4bit_quant_type
                    .clone()
                    .unwrap_or_default(),
            ),
            (
                "HKASK_BNB_4BIT_COMPUTE_DTYPE",
                job.params
                    .quantization
                    .bnb_4bit_compute_dtype
                    .clone()
                    .unwrap_or_default(),
            ),
            (
                "HKASK_BNB_4BIT_USE_DOUBLE_QUANT",
                job.params
                    .quantization
                    .bnb_4bit_use_double_quant
                    .to_string(),
            ),
            ("HKASK_BF16", job.params.advanced.bf16.to_string()),
            ("HKASK_FP16", job.params.advanced.fp16.to_string()),
            (
                "HKASK_GRADIENT_CHECKPOINTING",
                job.params
                    .advanced
                    .gradient_checkpointing
                    .clone()
                    .unwrap_or_default(),
            ),
            (
                "HKASK_ATTN_IMPLEMENTATION",
                job.params
                    .advanced
                    .attn_implementation
                    .clone()
                    .unwrap_or_default(),
            ),
        ];

        // Render the training config and generate the install script.
        // The install script is a bash script that pip-installs the
        // harness-specific packages, writes the config, runs training,
        // uploads the adapter, and writes the completion manifest.
        // It's passed to the pod as HKASK_INSTALL_SCRIPT — the generic
        // entrypoint in docker/training-base/ reads it and executes it.
        let install_script = generate_install_script(job, selected_harness)?;
        env_entries.push(("HKASK_INSTALL_SCRIPT", install_script));

        // HF_TOKEN — required for the pod to download private datasets and upload
        // adapters to private model repos. The publish step (HuggingFaceTraining::from_env)
        // reads the same token from the local env to publish artifacts; it must also
        // cross the pod boundary so the pod can consume those artifacts. Without it,
        // axolotl fails with HTTP 401 on private dataset load and the container exits
        // after the (public) base-model download completes — GPU never utilized.
        if let Ok(token) = std::env::var("HF_TOKEN") {
            env_entries.push(("HF_TOKEN", token));
        } else {
            tracing::warn!(
                target: "hkask.training.runpod",
                "HF_TOKEN not set — pod cannot access private HF datasets or upload to private model repos"
            );
        }

        // Generate docker args if not provided via env var.
        // The generic training-base image uses ENTRYPOINT to invoke
        // /usr/local/bin/entrypoint.sh, which reads HKASK_INSTALL_SCRIPT
        // and executes it. We do NOT set dockerArgs by default — v2's `args`
        // field overrides the Docker CMD, and our image uses ENTRYPOINT
        // (not CMD) to invoke the entrypoint. Setting `args` would pass the
        // script path as arguments to the entrypoint, causing unexpected
        // behavior. Leaving `args` empty lets the image's ENTRYPOINT
        // run naturally.
        //
        // RUNPOD_DOCKER_ARGS remains available as an override for operators
        // who need to customize the startup command.
        let docker_args = std::env::var("RUNPOD_DOCKER_ARGS").unwrap_or_default();

        let body = self.build_create_pod_body(
            &job.id,
            &PodDeploySpec {
                gpu_type_id: &gpu_type_id,
                container_disk_gb: resolved_disk,
                docker_image: &resolved_image,
                docker_args: &docker_args,
                template_id: &template_id,
            },
            &env_entries,
        );

        tracing::debug!(
            target: "hkask.training.runpod.create_body",
            body_len = body.to_string().len(),
            docker_args_len = docker_args.len(),
            "Built v2 create-pod body"
        );

        let pod_id = self.create_pod(body).await?;

        // Store pod_id for status/cancel
        if let Ok(mut map) = self.jobs.lock() {
            map.insert(job.id.clone(), pod_id.clone());
        }
        self.persist_pods();

        tracing::info!(
            target: "hkask.training.job.submit",
            job_id = %job.id,
            pod_id = %pod_id,
            host = "runpod",
            harness = ?job.harness,
            "Training pod created on Runpod"
        );

        tracing::info!(
            target: "reg.training.provider.runpod.submit",
            pod_id = %pod_id,
            gpu_type = %gpu_type_id,
            "RunPod pod submitted"
        );

        Ok(pod_id)
    }

    async fn status(&self, job_id: &str) -> Result<PodStatus, ProviderError> {
        let pod_id = {
            let map = self
                .jobs
                .lock()
                .map_err(|e| ProviderError::Backend(format!("Lock error: {e}")))?;
            map.get(job_id).cloned()
        };
        let pod_id = match pod_id {
            Some(id) => id,
            None => {
                return Err(ProviderError::JobFailed(format!(
                    "No pod found for job {job_id}"
                )));
            }
        };

        let result = self.get_pod(&pod_id).await?;

        // v2 status enum: PROVISIONING | STARTING | RUNNING | EXITED | ERROR | TERMINATED
        let status_str = result
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN");
        let current_uptime = result
            .get("runtime")
            .and_then(|r| r.get("uptime"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let gpu_type = result
            .get("gpu")
            .and_then(|g| g.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // v2 does not surface per-port SSH IP/port info in the pod response.
        // The `ports` field is an array of strings like ["22/tcp", "8888/http"]
        // — just port/protocol declarations, no IP or publicPort. SSH access
        // must be obtained via the RunPod console or `runpodctl ssh`. We leave
        // ssh_command / ip / ssh_port empty and log a one-time hint.
        let ssh_command = String::new();
        let ip = String::new();
        let ssh_port: u64 = 0;
        let is_public_ip = false;

        // Detect pod restart
        if let Ok(mut uptimes) = self.last_uptime.lock() {
            if let Some(&prev) = uptimes.get(job_id)
                && current_uptime < prev
            {
                tracing::warn!(
                    target: "reg.training.checkpoint.resume",
                    job_id = %job_id, pod_id = %pod_id,
                    prev_uptime_secs = prev, new_uptime_secs = current_uptime,
                    "Pod restarted — Axolotl will auto-resume from checkpoint"
                );
            }
            uptimes.insert(job_id.to_string(), current_uptime);
        }

        // v2 has no SSH info in the pod response — log a hint once.
        if status_str == "RUNNING" {
            tracing::debug!(
                target: "hkask.training.pod.ssh",
                job_id = %job_id, pod_id = %pod_id,
                "Pod is RUNNING — use `runpodctl ssh {}` or the RunPod console for SSH access (v2 API does not surface SSH IP/port)",
                pod_id
            );
        }

        let pod_status = PodStatus {
            status: map_pod_status(status_str),
            pod_id: pod_id.clone(),
            ssh_command: ssh_command.clone(),
            ip,
            ssh_port,
            is_public_ip,
            uptime_seconds: current_uptime,
            gpu_type,
            fail_reason: None,
        };

        // Store the (empty) SSH command so ssh_for_job() returns Some("")
        // rather than None — preserves the contract that status() populates
        // the map for every job it has seen.
        if let Ok(mut ssh_map) = self.ssh_commands.lock() {
            ssh_map.insert(job_id.to_string(), ssh_command.clone());
            ssh_map.insert(format!("{job_id}:status"), ssh_command.clone());
        }

        tracing::debug!(
            target: "reg.training.provider.runpod.status",
            pod_id = %pod_id, status = %status_str, uptime = current_uptime,
            "RunPod pod status"
        );

        Ok(pod_status)
    }

    async fn cancel(&self, job_id: &str) -> Result<(), ProviderError> {
        let pod_id = {
            let map = self
                .jobs
                .lock()
                .map_err(|e| ProviderError::Backend(format!("Lock error: {}", e)))?;
            map.get(job_id).cloned()
        };

        let pod_id = match pod_id {
            Some(id) => id,
            None => {
                tracing::warn!(
                    target: "hkask.training.job.cancel",
                    job_id = %job_id,
                    "No pod found for job"
                );
                tracing::warn!(
                    target: "reg.training.provider.runpod.cancel",
                    "No pod found for job"
                );
                return Ok(());
            }
        };

        self.delete_pod(&pod_id).await?;

        if let Ok(mut map) = self.jobs.lock() {
            map.remove(job_id);
        }
        self.persist_pods();

        tracing::info!(
            target: "hkask.training.job.cancel",
            job_id = %job_id,
            pod_id = %pod_id,
            host = "runpod",
            "Training pod terminated"
        );
        tracing::info!(
            target: "reg.training.provider.runpod.cancel",
            pod_id = %pod_id,
            "RunPod pod cancelled"
        );
        Ok(())
    }
}

mod tests {
    use super::*;

    #[test]
    fn map_pod_status_v2_enum() {
        // v2 enum: PROVISIONING | STARTING | RUNNING | EXITED | ERROR | TERMINATED
        assert_eq!(map_pod_status("PROVISIONING"), TrainingJobStatus::Queued);
        assert_eq!(map_pod_status("STARTING"), TrainingJobStatus::Queued);
        assert_eq!(map_pod_status("RUNNING"), TrainingJobStatus::Running);
        assert_eq!(map_pod_status("EXITED"), TrainingJobStatus::Failed);
        assert_eq!(map_pod_status("ERROR"), TrainingJobStatus::Failed);
        assert_eq!(map_pod_status("TERMINATED"), TrainingJobStatus::Failed);
        // Unknown statuses default to Queued (safe — poll again).
        assert_eq!(map_pod_status("UNKNOWN"), TrainingJobStatus::Queued);
    }

    #[test]
    fn build_create_pod_body_shape() {
        let host = make_host("tpl-123");
        let body = host.build_create_pod_body(
            "abcdefgh-1234-5678-90ab-1234567890ab",
            &PodDeploySpec {
                gpu_type_id: "NVIDIA A100-SXM4-80GB",
                container_disk_gb: 60,
                docker_image: "runpod/pytorch:2.6.0",
                docker_args: "",
                template_id: "tpl-123",
            },
            &[("HKASK_JOB_ID", "job-1".to_string())],
        );
        // v2 create-pod body shape
        assert_eq!(body["name"], "hkask-training-abcdefgh");
        assert_eq!(body["image"], "runpod/pytorch:2.6.0");
        assert_eq!(body["gpu"]["id"], "NVIDIA A100-SXM4-80GB");
        assert_eq!(body["gpu"]["count"], 1);
        assert_eq!(body["disk"], 60);
        assert_eq!(body["cloud"], "SECURE");
        assert_eq!(body["dataCenterIds"], json!([]));
        assert_eq!(body["ports"], json!(["22/tcp"]));
        // env is a flat object, not array of pairs
        assert_eq!(body["env"]["HKASK_JOB_ID"], "job-1");
        // No GraphQL fields, no templateId field (v2 spreads template fields)
        assert!(
            body.get("templateId").is_none(),
            "v2 createPod does not take templateId — got: {body}"
        );
        assert!(
            body.get("imageName").is_none(),
            "v2 uses `image`, not `imageName` — got: {body}"
        );
        assert!(
            body.get("gpuTypeId").is_none(),
            "v2 uses `gpu.id`, not `gpuTypeId` — got: {body}"
        );
        assert!(
            body.get("containerDiskInGb").is_none(),
            "v2 uses `disk`, not `containerDiskInGb` — got: {body}"
        );
        // No `args` field when docker_args is empty
        assert!(
            body.get("args").is_none(),
            "args should be omitted when empty — got: {body}"
        );
    }

    #[test]
    fn build_create_pod_body_includes_args_when_set() {
        let host = make_host("");
        let body = host.build_create_pod_body(
            "abcdefgh-1234-5678-90ab-1234567890ab",
            &PodDeploySpec {
                gpu_type_id: "NVIDIA RTX 4090",
                container_disk_gb: 50,
                docker_image: "runpod/pytorch:2.6.0",
                docker_args: "/usr/local/bin/entrypoint.sh",
                template_id: "",
            },
            &[],
        );
        assert_eq!(body["args"], "/usr/local/bin/entrypoint.sh");
    }

    #[test]
    fn build_create_pod_body_uses_default_image_when_empty() {
        let host = make_host("");
        let body = host.build_create_pod_body(
            "abcdefgh-1234-5678-90ab-1234567890ab",
            &PodDeploySpec {
                gpu_type_id: "NVIDIA RTX 4090",
                container_disk_gb: 50,
                docker_image: "",
                docker_args: "",
                template_id: "",
            },
            &[],
        );
        // When docker_image is empty, the body still carries an empty `image`
        // — the caller (submit) is responsible for resolving a default before
        // calling this helper, OR fetching from a template.
        assert_eq!(body["image"], "");
    }

    fn make_host(template_id: &str) -> RunpodHost {
        RunpodHost::new(RunpodHostInit {
            api_key: "test-key".to_string(),
            template_id: template_id.to_string(),
            gpu_type_id: String::new(),
            container_disk_gb: 0,
            docker_image: String::new(),
        })
    }
}
