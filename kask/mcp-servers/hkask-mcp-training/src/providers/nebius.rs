//! Nebius AI Cloud VM training host.
//!
//! Uses the Nebius CLI to create/destroy GPU VMs with SSH access and
//! pre-installed CUDA drivers. VMs get public IPs by default.
//!
//! CLI: nebius compute instance create / get / stop
//! Auth: Federation profile (browser-based, stored in ~/.nebius/credentials.yaml)
//! Billing: Per-second, H100 at $3.85/hr ($2.15/hr preemptible)
//! Stopped VMs don't charge for compute (only disk storage).
//!
//! ARCHITECTURAL REQUIREMENT: Every VM gets a public IP and SSH access.
//! The operator can always SSH in to inspect logs, debug failures, and monitor
//! training progress in real time.

use crate::providers::types::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Nebius AI Cloud VM configuration.
pub struct NebiusHost {
    /// Project ID (parent-id for CLI commands).
    project_id: String,
    /// Subnet ID for network interface.
    subnet_id: String,
    /// SSH public key for cloud-init.
    ssh_public_key: String,
    /// GPU platform (e.g. "gpu-h100-sxm").
    gpu_platform: String,
    /// Resource preset (e.g. "1gpu-16vcpu-200gb").
    gpu_preset: String,
    /// Boot disk image family (e.g. "ubuntu24.04-cuda13.0").
    image_family: String,
    /// Path to nebius CLI binary.
    nebius_cli: String,
    /// job_id -> VM ID mapping.
    vms: Arc<Mutex<HashMap<String, String>>>,
    /// job_id -> SSH command.
    ssh_commands: Arc<Mutex<HashMap<String, String>>>,
}

impl NebiusHost {
    pub fn new(
        project_id: String,
        subnet_id: String,
        ssh_public_key: String,
        gpu_platform: String,
        gpu_preset: String,
        image_family: String,
    ) -> Self {
        let nebius_cli = std::env::var("NEBIUS_CLI_PATH").unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|h| h.join(".nebius/bin/nebius").to_string_lossy().to_string())
                .unwrap_or_else(|| "nebius".to_string())
        });
        Self {
            project_id,
            subnet_id,
            ssh_public_key,
            gpu_platform,
            gpu_preset,
            image_family,
            nebius_cli,
            vms: Arc::new(Mutex::new(HashMap::new())),
            ssh_commands: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn vm_name(job_id: &str) -> String {
        format!("hkask-training-{}", &job_id[..8.min(job_id.len())])
    }

    async fn run_cli(&self, args: &[&str]) -> Result<String, ProviderError> {
        let output = tokio::process::Command::new(&self.nebius_cli)
            .args(args)
            .output()
            .await
            .map_err(|e| ProviderError::Backend(format!("Nebius CLI: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::Backend(format!(
                "Nebius CLI error: {stderr}"
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[async_trait::async_trait]
impl TrainingHost for NebiusHost {
    async fn submit(&self, job: &TrainingJob) -> Result<String, ProviderError> {
        let vm_name = Self::vm_name(&job.id);
        let install_script = crate::providers::runpod::generate_install_script(
            job,
            job.params.harness.unwrap_or(job.harness),
        )?;

        // Build cloud-init user-data
        let cloud_init = format!(
            r#"#cloud-config
users:
  - name: user
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

        // Step 1: Create boot disk from Ubuntu+CUDA image
        let disk_name = format!("{vm_name}-disk");
        let disk_output = self
            .run_cli(&[
                "compute",
                "disk",
                "create",
                "--parent-id",
                &self.project_id,
                "--name",
                &disk_name,
                "--size-gibibytes",
                "200",
                "--type",
                "network_ssd",
                "--source-image-family-image-family",
                &self.image_family,
                "--format",
                "json",
            ])
            .await?;
        let disk_id = extract_json_field(&disk_output, "id")
            .ok_or_else(|| ProviderError::Backend("Failed to get disk ID from Nebius".into()))?;

        // Step 2: Create VM with GPU, public IP, and cloud-init
        let network_spec = format!(
            r#"[{{"name": "net1", "subnet_id": "{}", "ip_address": {{}}, "public_ip_address": {{}}}}]"#,
            self.subnet_id
        );

        let vm_output = self
            .run_cli(&[
                "compute",
                "instance",
                "create",
                "--parent-id",
                &self.project_id,
                "--name",
                &vm_name,
                "--resources-platform",
                &self.gpu_platform,
                "--resources-preset",
                &self.gpu_preset,
                "--boot-disk-existing-disk-id",
                &disk_id,
                "--boot-disk-attach-mode",
                "read_write",
                "--cloud-init-user-data",
                &cloud_init,
                "--network-interfaces",
                &network_spec,
                "--format",
                "json",
            ])
            .await?;
        let vm_id = extract_json_field(&vm_output, "id")
            .ok_or_else(|| ProviderError::Backend("Failed to get VM ID from Nebius".into()))?;

        if let Ok(mut map) = self.vms.lock() {
            map.insert(job.id.clone(), vm_id.clone());
        }

        tracing::info!(
            target: "hkask.training.nebius.submit",
            job_id = %job.id,
            vm_id = %vm_id,
            vm_name = %vm_name,
            gpu = %self.gpu_platform,
            "Nebius VM submitted"
        );

        Ok(vm_id)
    }

    async fn status(&self, job_id: &str) -> Result<PodStatus, ProviderError> {
        let vm_id = {
            let map = self
                .vms
                .lock()
                .map_err(|e| ProviderError::Backend(format!("Lock: {e}")))?;
            map.get(job_id).cloned()
        };
        let vm_id = match vm_id {
            Some(id) => id,
            None => return Err(ProviderError::JobFailed(format!("No VM for job {job_id}"))),
        };

        let output = self
            .run_cli(&[
                "compute", "instance", "get", "--id", &vm_id, "--format", "json",
            ])
            .await?;

        // State is nested at status.state (verified via live API 2026-07-23).
        // extract_json_field only checks metadata and top-level, so we use
        // extract_nested_field for the nested path.
        let state = extract_nested_field(&output, &["status", "state"]).unwrap_or("unknown".into());
        let status = match state.as_str() {
            "CREATING" | "STARTING" => TrainingJobStatus::Queued,
            "RUNNING" | "ACTIVE" => TrainingJobStatus::Running,
            "STOPPED" | "STOPPING" | "DELETING" | "DELETED" => TrainingJobStatus::Failed,
            _ => TrainingJobStatus::Running,
        };

        // Extract public IP from network_interfaces
        let public_ip = extract_nested_field(
            &output,
            &[
                "status",
                "network_interfaces",
                "0",
                "public_ip_address",
                "address",
            ],
        )
        .unwrap_or_default();
        let ssh_command = if !public_ip.is_empty() {
            format!("ssh user@{public_ip}")
        } else {
            String::new()
        };

        if !ssh_command.is_empty() {
            if let Ok(mut ssh_map) = self.ssh_commands.lock() {
                ssh_map.insert(job_id.to_string(), ssh_command.clone());
            }
            tracing::info!(
                target: "hkask.training.nebius.ssh",
                job_id = %job_id, ssh = %ssh_command,
                "Nebius VM SSH available"
            );
        }

        let is_public_ip = !public_ip.is_empty();

        Ok(PodStatus {
            status,
            pod_id: vm_id,
            ssh_command,
            ip: public_ip,
            ssh_port: 22,
            is_public_ip,
            uptime_seconds: 0,
            gpu_type: self.gpu_platform.clone(),
            fail_reason: None,
        })
    }

    async fn cancel(&self, job_id: &str) -> Result<(), ProviderError> {
        let vm_id = {
            let map = self
                .vms
                .lock()
                .map_err(|e| ProviderError::Backend(format!("Lock: {e}")))?;
            map.get(job_id).cloned()
        };
        let vm_id = match vm_id {
            Some(id) => id,
            None => {
                tracing::warn!(target: "hkask.training.nebius.cancel", job_id = %job_id, "No VM found");
                return Ok(());
            }
        };

        // Delete the VM and its managed disks (stops all billing — compute + storage).
        // We use delete instead of stop because stop leaves the disk running
        // (storage charges continue). Delete cleans up everything.
        let _ = self
            .run_cli(&["compute", "instance", "delete", "--id", &vm_id])
            .await;

        if let Ok(mut map) = self.vms.lock() {
            map.remove(job_id);
        }

        tracing::info!(
            target: "hkask.training.nebius.cancel",
            job_id = %job_id, vm_id = %vm_id,
            "Nebius VM deleted (all billing stopped)"
        );
        Ok(())
    }
}

/// Extract a top-level field from JSON output.
fn extract_json_field(json: &str, field: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    // Try metadata.id first (Nebius convention), then top-level
    v.get("metadata")
        .and_then(|m| m.get(field))
        .or_else(|| v.get(field))
        .and_then(|f| f.as_str())
        .map(String::from)
}

/// Extract a nested field from JSON using a path of string keys and array indices.
fn extract_nested_field(json: &str, path: &[&str]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut current = &v;
    for key in path {
        if let Ok(idx) = key.parse::<usize>() {
            current = current.get(idx)?;
        } else {
            current = current.get(*key)?;
        }
    }
    current.as_str().map(|s| {
        // IP addresses may have CIDR suffix — strip it
        s.split('/').next().unwrap_or(s).to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verified Nebius CLI JSON output (captured from live API 2026-07-23)
    const DISK_CREATE_RESPONSE: &str = r#"{
  "metadata": {
    "id": "computedisk-e00pffkt6am6v07jrv",
    "parent_id": "project-e00czb3vpr00hw3jgsf18x",
    "name": "hkask-verify-img",
    "resource_version": "1",
    "created_at": "2026-07-23T11:58:50.684229Z"
  },
  "spec": {
    "size_gibibytes": "200",
    "type": "NETWORK_SSD",
    "source_image_family": {
      "image_family": "ubuntu24.04-cuda13.0"
    }
  },
  "status": {
    "state": "READY"
  }
}"#;

    // Verified Nebius CLI instance get response (captured from live API 2026-07-23)
    const INSTANCE_GET_RESPONSE: &str = r#"{
  "metadata": {
    "id": "computeinstance-e00srz5jr06c09vv1q",
    "parent_id": "project-e00czb3vpr00hw3jgsf18x",
    "name": "hkask-verify-vm"
  },
  "spec": {
    "resources": {
      "platform": "gpu-h100-sxm",
      "preset": "1gpu-16vcpu-200gb"
    }
  },
  "status": {
    "state": "RUNNING",
    "network_interfaces": [
      {
        "name": "net1",
        "ip_address": {
          "address": "10.0.0.11/32"
        },
        "public_ip_address": {
          "address": "89.169.112.136/32"
        }
      }
    ]
  }
}"#;

    #[test]
    fn extract_disk_id_from_metadata() {
        let id = extract_json_field(DISK_CREATE_RESPONSE, "id");
        assert_eq!(id, Some("computedisk-e00pffkt6am6v07jrv".to_string()));
    }

    #[test]
    fn extract_vm_id_from_metadata() {
        let id = extract_json_field(INSTANCE_GET_RESPONSE, "id");
        assert_eq!(id, Some("computeinstance-e00srz5jr06c09vv1q".to_string()));
    }

    #[test]
    fn extract_vm_state_via_nested_path() {
        // State is at status.state (not top-level or metadata).
        // The status() method uses extract_nested_field for this.
        let state = extract_nested_field(INSTANCE_GET_RESPONSE, &["status", "state"]);
        assert_eq!(state, Some("RUNNING".to_string()));
    }

    #[test]
    fn extract_public_ip_with_cidr_stripped() {
        let ip = extract_nested_field(
            INSTANCE_GET_RESPONSE,
            &[
                "status",
                "network_interfaces",
                "0",
                "public_ip_address",
                "address",
            ],
        );
        assert_eq!(ip, Some("89.169.112.136".to_string()));
    }

    #[test]
    fn extract_internal_ip_with_cidr_stripped() {
        let ip = extract_nested_field(
            INSTANCE_GET_RESPONSE,
            &["status", "network_interfaces", "0", "ip_address", "address"],
        );
        assert_eq!(ip, Some("10.0.0.11".to_string()));
    }

    #[test]
    fn extract_nested_field_returns_none_for_bad_path() {
        let result =
            extract_nested_field(INSTANCE_GET_RESPONSE, &["status", "nonexistent", "field"]);
        assert_eq!(result, None);
    }

    #[test]
    fn extract_json_field_returns_none_for_missing_field() {
        let result = extract_json_field(DISK_CREATE_RESPONSE, "nonexistent");
        assert_eq!(result, None);
    }
}
