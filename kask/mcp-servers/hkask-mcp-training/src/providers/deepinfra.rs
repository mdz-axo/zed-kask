//! DeepInfra GPU container training host.
//!
//! Uses the DeepInfra REST API to create/destroy dedicated GPU containers
//! with SSH access. Containers get public IPs, pre-installed PyTorch + CUDA,
//! and cloud-init for script injection.
//!
//! API: https://api.deepinfra.com/v1/containers
//! Auth: Bearer token (DI_API_KEY)
//! Billing: Per-minute, B200 at $3.69/hr (dedicated GPU, 180GB HBM3e)
//! GPU configs: 1xB200-180GB, 8xB200-180GB
//! Note: DeepInfra offers B200 GPUs only (not H100). B200 is NVIDIA Blackwell,
//!       faster than H100 for training workloads.
//!
//! ARCHITECTURAL REQUIREMENT: Every container gets a public IP and SSH access.
//! The operator can always SSH in to inspect logs, debug failures, and monitor
//! training progress in real time.

use crate::providers::types::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// DeepInfra GPU container configuration.
pub struct DeepInfraHost {
    api_key: String,
    /// GPU config string (e.g. "1xB200-180GB", "8xB200-180GB").
    gpu_config: String,
    /// Container image (e.g. "di-cont-ubuntu-torch:latest").
    container_image: String,
    /// SSH public key for cloud-init.
    ssh_public_key: String,
    client: reqwest::Client,
    /// job_id -> container_name mapping for status/cancel.
    containers: Arc<Mutex<HashMap<String, String>>>,
    /// job_id -> SSH command string.
    ssh_commands: Arc<Mutex<HashMap<String, String>>>,
}

impl DeepInfraHost {
    pub fn new(
        api_key: String,
        gpu_config: String,
        container_image: String,
        ssh_public_key: String,
    ) -> Self {
        Self {
            api_key,
            gpu_config,
            container_image,
            ssh_public_key,
            client: reqwest::Client::new(),
            containers: Arc::new(Mutex::new(HashMap::new())),
            ssh_commands: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn container_name(job_id: &str) -> String {
        format!("hkask-training-{}", &job_id[..8.min(job_id.len())])
    }

    async fn api_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ProviderError> {
        let url = format!("https://api.deepinfra.com/v1/{}", path);
        let mut req = self
            .client
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ProviderError::Backend(format!("DeepInfra API: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ProviderError::Backend(format!("DeepInfra response: {e}")))?;
        if !status.is_success() {
            return Err(ProviderError::Backend(format!(
                "DeepInfra API error ({}): {}",
                status, text
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| ProviderError::Backend(format!("DeepInfra JSON parse: {e}")))
    }
}

#[async_trait::async_trait]
impl TrainingHost for DeepInfraHost {
    async fn submit(&self, job: &TrainingJob) -> Result<String, ProviderError> {
        let container_name = Self::container_name(&job.id);
        let install_script = crate::providers::runpod::generate_install_script(
            job,
            job.params.harness.unwrap_or(job.harness),
        )?;

        // Build cloud-init user-data that creates a user with SSH access,
        // writes the install script, and executes it.
        let cloud_init = format!(
            r#"#cloud-config
users:
  - name: ubuntu
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - {ssh_key}
write_files:
  - path: /workspace/install_and_train.sh
    content: |
{script_indented}
    permissions: '0755'
runcmd:
  - mkdir -p /workspace/logs /workspace/outputs
  - bash /workspace/install_and_train.sh 2>&1 | tee /workspace/logs/entrypoint.log
"#,
            ssh_key = self.ssh_public_key,
            script_indented = install_script
                .lines()
                .map(|l| format!("      {l}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let body = json!({
            "name": container_name,
            "gpu_config": self.gpu_config,
            "container_image": self.container_image,
            "cloud_init_user_data": cloud_init,
        });

        let result = self
            .api_request(reqwest::Method::POST, "containers", Some(body))
            .await?;

        let container_id = result["container_id"]
            .as_str()
            .or_else(|| result["id"].as_str())
            .or_else(|| result["name"].as_str())
            .unwrap_or(&container_name)
            .to_string();

        if let Ok(mut map) = self.containers.lock() {
            map.insert(job.id.clone(), container_id.clone());
        }

        tracing::info!(
            target: "hkask.training.deepinfra.submit",
            job_id = %job.id,
            container = %container_id,
            gpu_config = %self.gpu_config,
            "DeepInfra container submitted"
        );

        Ok(container_id)
    }

    async fn status(&self, job_id: &str) -> Result<PodStatus, ProviderError> {
        let container_name = {
            let map = self
                .containers
                .lock()
                .map_err(|e| ProviderError::Backend(format!("Lock: {e}")))?;
            map.get(job_id).cloned()
        };
        let container_name = match container_name {
            Some(n) => n,
            None => {
                return Err(ProviderError::JobFailed(format!(
                    "No container for job {job_id}"
                )));
            }
        };

        let result = self
            .api_request(
                reqwest::Method::GET,
                &format!("containers/{container_name}"),
                None,
            )
            .await?;

        let state = result["status"]
            .as_str()
            .or_else(|| result["state"].as_str())
            .unwrap_or("unknown");

        let status = match state {
            "creating" | "starting" => TrainingJobStatus::Queued,
            "running" => TrainingJobStatus::Running,
            "failed" | "deleted" | "shutting_down" => TrainingJobStatus::Failed,
            _ => TrainingJobStatus::Running,
        };

        // Extract SSH info — DeepInfra containers get public IPs once running.
        // IP is null when the container is still starting or has failed.
        let ip = result["ip"]
            .as_str()
            .or_else(|| result["ssh_host"].as_str())
            .unwrap_or("");
        let ssh_command = if !ip.is_empty() {
            format!("ssh ubuntu@{ip}")
        } else {
            String::new()
        };

        // Extract failure reason when the container failed (e.g. "out of capacity").
        let fail_reason = result["fail_reason"].as_str().map(|s| s.to_string());
        if let Some(ref reason) = fail_reason {
            tracing::warn!(
                target: "hkask.training.deepinfra.status",
                job_id = %job_id, fail_reason = %reason,
                "DeepInfra container failed"
            );
        }

        // Compute uptime from start_ts (Unix timestamp) when the container is running.
        let uptime_seconds = result["start_ts"]
            .as_i64()
            .map(|start| {
                let now = chrono::Utc::now().timestamp();
                (now - start).max(0) as u64
            })
            .unwrap_or(0);

        if !ssh_command.is_empty() {
            if let Ok(mut ssh_map) = self.ssh_commands.lock() {
                ssh_map.insert(job_id.to_string(), ssh_command.clone());
            }
            tracing::info!(
                target: "hkask.training.deepinfra.ssh",
                job_id = %job_id, ssh = %ssh_command,
                "DeepInfra container SSH available"
            );
        }

        Ok(PodStatus {
            status,
            pod_id: container_name,
            ssh_command,
            ip: ip.to_string(),
            ssh_port: 22,
            is_public_ip: !ip.is_empty(),
            uptime_seconds, // computed from start_ts
            gpu_type: self.gpu_config.clone(),
            fail_reason,
        })
    }

    async fn cancel(&self, job_id: &str) -> Result<(), ProviderError> {
        let container_name = {
            let map = self
                .containers
                .lock()
                .map_err(|e| ProviderError::Backend(format!("Lock: {e}")))?;
            map.get(job_id).cloned()
        };
        let container_name = match container_name {
            Some(n) => n,
            None => {
                tracing::warn!(target: "hkask.training.deepinfra.cancel", job_id = %job_id, "No container found");
                return Ok(());
            }
        };

        self.api_request(
            reqwest::Method::DELETE,
            &format!("containers/{container_name}"),
            None,
        )
        .await?;

        if let Ok(mut map) = self.containers.lock() {
            map.remove(job_id);
        }

        tracing::info!(
            target: "hkask.training.deepinfra.cancel",
            job_id = %job_id, container = %container_name,
            "DeepInfra container terminated"
        );
        Ok(())
    }
}
