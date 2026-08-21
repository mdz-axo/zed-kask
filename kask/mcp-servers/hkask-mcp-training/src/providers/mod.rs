//! Training provider abstraction — pluggable backend adapter for model fine-tuning.
//!
//! Each provider wraps a different training framework behind a common
//! `TrainingHost` trait. The MCP server maps its tool surface (`submit`,
//! `status`, `cancel`) to provider methods, isolating the MCP surface from
//! host-specific API differences.
//!
//! Architecture (cloud-only — no local training):
//!   TrainingHostConfig × HarnessAdapter → cloud Host → TrainingJob
//!
//! Provider selection is driven by `training.harness` in settings.json,
//! routed through `hkask-services` shared config init. The host is fixed
//! to Runpod (cloud-only, single host).

pub(crate) mod harness;
pub(crate) mod nebius;
pub(crate) mod runpod;
pub(crate) mod trl_harness;
pub(crate) mod types;

// ── Re-exports for lib.rs compatibility ──────────────────────────────────

pub(crate) use harness::{AxolotlHarness, LudwigHarness};
pub(crate) use nebius::NebiusHost;
pub(crate) use runpod::RunpodHost;
pub(crate) use trl_harness::TrlHarness;
pub(crate) use types::{
    HostProviderError, TrainingHarnessId, TrainingHost, TrainingHostId, TrainingJob,
    TrainingJobStatus, TrainingParams,
};

// ── Host factory ───────────────────────────────────────────────────────────

/// Create a training host from configuration.
///
/// Supports two providers: Runpod and Nebius.
/// The provider is selected from `config.host`.
pub(crate) fn create_host(
    config: &TrainingHostConfig,
) -> Result<Box<dyn TrainingHost>, HostProviderError> {
    match config.host {
        TrainingHostId::Runpod => {
            if config.runpod_api_key.is_empty() {
                return Err(HostProviderError::NotConfigured(
                    "Runpod API key not configured (set RUNPOD_API_KEY)".to_string(),
                ));
            }
            Ok(Box::new(RunpodHost::new(runpod::RunpodHostInit {
                api_key: config.runpod_api_key.clone(),
                template_id: config.runpod_template_id.clone(),
                gpu_type_id: config.runpod_gpu_type_id.clone(),
                container_disk_gb: config.runpod_container_disk_gb,
                docker_image: config.runpod_docker_image.clone(),
            })))
        }
        TrainingHostId::Nebius => {
            let project_id = std::env::var("NEBIUS_PROJECT_ID").map_err(|_| {
                HostProviderError::NotConfigured("NEBIUS_PROJECT_ID not configured".to_string())
            })?;
            let subnet_id = std::env::var("NEBIUS_SUBNET_ID").map_err(|_| {
                HostProviderError::NotConfigured("NEBIUS_SUBNET_ID not configured".to_string())
            })?;
            let ssh_key = read_ssh_public_key()?;
            let gpu_platform =
                std::env::var("NEBIUS_GPU_PLATFORM").unwrap_or_else(|_| "gpu-h100-sxm".to_string());
            let gpu_preset = std::env::var("NEBIUS_GPU_PRESET")
                .unwrap_or_else(|_| "1gpu-16vcpu-200gb".to_string());
            let image_family = std::env::var("NEBIUS_IMAGE_FAMILY")
                .unwrap_or_else(|_| "ubuntu24.04-cuda13.0".to_string());
            Ok(Box::new(NebiusHost::new(
                project_id,
                subnet_id,
                ssh_key,
                gpu_platform,
                gpu_preset,
                image_family,
            )))
        }
    }
}

/// Read the SSH public key from ~/.ssh/id_ed25519.pub (or id_rsa.pub as fallback).
fn read_ssh_public_key() -> Result<String, HostProviderError> {
    let home = dirs::home_dir()
        .ok_or_else(|| HostProviderError::Unavailable("Cannot find home directory".to_string()))?;
    let ed25519 = home.join(".ssh/id_ed25519.pub");
    let rsa = home.join(".ssh/id_rsa.pub");
    let path = if ed25519.exists() { ed25519 } else { rsa };
    std::fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|e| HostProviderError::Unavailable(format!("Cannot read SSH public key: {e}")))
}

// ── Training host config ──────────────────────────────────────────────────

/// Training host configuration resolved from hKask settings.
///
/// Supports two providers: Runpod and Nebius.
/// The provider is selected from `host` field. Runpod-specific fields
/// are only used when `host == TrainingHostId::Runpod`. Nebius reads its
/// configuration from environment variables at
/// `create_host` time.
#[derive(Debug, Clone)]
pub(crate) struct TrainingHostConfig {
    /// Selected training host.
    pub host: TrainingHostId,
    /// Runpod API key.
    pub runpod_api_key: String,
    /// Runpod GPU pod template ID with axolotl pre-installed.
    pub runpod_template_id: String,
    /// Runpod GPU type ID (e.g. `"NVIDIA H100 80GB HBM3"`).
    pub runpod_gpu_type_id: String,
    /// Container disk in GB.
    pub runpod_container_disk_gb: u32,
    /// Docker image name.
    pub runpod_docker_image: String,
}

impl Default for TrainingHostConfig {
    fn default() -> Self {
        // Auto-detect: prefer Nebius (H100), then Runpod (H100) as fallback.
        // HKASK_TRAINING_HOST overrides this selection.
        // This matches the auto-detection in lib.rs::run().
        let host = if let Ok(h) = std::env::var("HKASK_TRAINING_HOST") {
            TrainingHostId::from_str(&h).unwrap_or(TrainingHostId::Runpod)
        } else if std::env::var("NEBIUS_PROJECT_ID").is_ok() {
            TrainingHostId::Nebius
        } else {
            TrainingHostId::Runpod
        };
        Self {
            host,
            runpod_api_key: String::new(),
            runpod_template_id: String::new(),
            runpod_gpu_type_id: String::new(),
            runpod_container_disk_gb: 0,
            runpod_docker_image: String::new(),
        }
    }
}
