use crate::TrainingServer;
use crate::adapters::AdapterMetrics;
use crate::providers::TrainingJobStatus;
use crate::types::TrainStatusRequest;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use serde_json::json;

impl TrainingServer {
    #[tool(
        description = "Check the status of a training job. Returns pod status, SSH connection info, uptime, GPU type, and recent log lines. When training completes (detected via HuggingFace completion manifest), automatically registers the adapter with metadata from the manifest."
    )]
    pub async fn training_status(
        &self,
        Parameters(TrainStatusRequest { job_id }): Parameters<TrainStatusRequest>,
    ) -> String {
        execute_tool(self, "training_status", async {
            match self.host.status(&job_id).await {
                Ok(pod_status) => {
                    // The pod stays RUNNING (exec sleep infinity for SSH
                    // debugging), so RunPod's desiredStatus alone cannot signal
                    // completion. Check for a completion manifest on HuggingFace.
                    let (status, manifest) = if pod_status.status == TrainingJobStatus::Running {
                        self.check_completion_manifest(&job_id)
                            .await
                            .unwrap_or((TrainingJobStatus::Running, None))
                    } else {
                        (pod_status.status, None)
                    };

                    let mut result = json!({
                        "job_id": job_id,
                        "status": serde_json::to_value(status).unwrap_or_default(),
                        "pod_id": pod_status.pod_id,
                        "ssh_command": pod_status.ssh_command,
                        "pod_ip": pod_status.ip,
                        "ssh_port": pod_status.ssh_port,
                        "is_public_ip": pod_status.is_public_ip,
                        "uptime_seconds": pod_status.uptime_seconds,
                        "gpu_type": pod_status.gpu_type,
                    });

                    // Surface failure reason when the pod failed (e.g. "out of capacity").
                    if let Some(ref reason) = pod_status.fail_reason {
                        result["fail_reason"] = json!(reason);
                    }

                    // If no public SSH, warn loudly — the operator cannot debug.
                    if !pod_status.ssh_command.is_empty() {
                        tracing::info!(
                            target: "hkask.training.status.ssh",
                            job_id = %job_id,
                            ssh = %pod_status.ssh_command,
                            "Pod is accessible via SSH"
                        );
                    } else if pod_status.status == TrainingJobStatus::Running {
                        tracing::warn!(
                            target: "hkask.training.status.ssh",
                            job_id = %job_id,
                            "Pod has NO public SSH — cannot debug. Ensure cloudType: SECURE and supportPublicIp: true."
                        );
                        result["ssh_warning"] = json!(
                            "No public SSH available. Cannot inspect pod logs or debug. \
                             Ensure pods deploy to Secure Cloud with public IP support."
                        );
                    }

                    // Fetch recent log lines via SSH for real-time visibility.
                    if !pod_status.ssh_command.is_empty() && status == TrainingJobStatus::Running
                        && let Some(logs) = crate::providers::types::fetch_pod_logs(
                            &pod_status.ssh_command, 20
                        ).await
                    {
                        result["recent_logs"] = json!(logs);
                    }

                    // Persist status update
                    if let Some(ref job_store) = self.job_store {
                        let status_str = format!("{:?}", status).to_lowercase();
                        if let Err(e) = job_store.update_status(&job_id, &status_str) {
                            tracing::warn!(
                                target: "hkask.training.job.persist",
                                job_id = %job_id, error = %e,
                                "Failed to update job status"
                            );
                        }
                    }

                    // Auto-register adapter on completion
                    if status == TrainingJobStatus::Completed {
                        let adapter: crate::adapter::TrainedLoRAAdapter = match self
                            .adapter_store
                            .get_by_id(uuid::Uuid::parse_str(&job_id).unwrap_or_default())
                            .map_err(|e| McpToolError::internal(format!("Adapter store error: {e}")))?
                        {
                            Some(existing) => {
                                result["adapter_registered"] = json!(true);
                                result["adapter_note"] = json!("Already registered (pre-registered by retrain)");
                                existing
                            }
                            None => {
                                if let Some(ref manifest) = manifest {
                                    let base_model = manifest.base_model.clone().unwrap_or_default();
                                    let adapter_name = format!("adapter-{}", &job_id[..8]);
                                    let weight_path = manifest.adapter.repository.clone();
                                    let adapter = Self::build_trained_adapter(
                                        job_id.clone(),
                                        adapter_name,
                                        base_model.clone(),
                                        String::new(),
                                        job_id.clone(),
                                        chrono::Utc::now().timestamp(),
                                        0,
                                        String::new(),
                                        1,
                                        Some(AdapterMetrics {
                                            loss: manifest.loss.map(|v| v as f32),
                                            perplexity: None,
                                            training_duration_secs: manifest.training_duration_secs,
                                            tokens_processed: None,
                                        }),
                                        Some(std::path::Path::new(&weight_path)),
                                    );
                                    match self.adapter_store.store(&adapter)
                                        .map_err(|e| McpToolError::internal(format!("Adapter store error: {e}")))
                                    {
                                        Ok(()) => {
                                            result["adapter_registered"] = json!(true);
                                            result["adapter_name"] = json!(adapter.expertise.name);
                                            result["base_model"] = json!(base_model);
                                            result["adapter_repository"] = json!(manifest.adapter.repository);
                                            result["adapter_path"] = json!(manifest.adapter.path);
                                            tracing::info!(
                                                target: "hkask.training.adapter.created",
                                                adapter_id = %job_id,
                                                "Adapter auto-registered from completion manifest"
                                            );
                                            adapter
                                        }
                                        Err(e) => {
                                            result["adapter_registered"] = json!(false);
                                            result["adapter_error"] = json!(e.to_string());
                                            return Ok(result);
                                        }
                                    }
                                } else {
                                    result["adapter_registered"] = json!(false);
                                    result["adapter_note"] = json!("No completion manifest available");
                                    return Ok(result);
                                }
                            }
                        };

                        // A/B comparison (skill retraining only)
                        let adapter_skill = adapter.skill_name.clone().unwrap_or_default();
                        if !adapter_skill.is_empty() {
                            let current_loss = Self::metrics_from_trained(&adapter).and_then(|m| m.loss);
                            if let Some(prev) = self.adapter_store
                                .get_previous_by_skill_name(&adapter_skill, adapter.id)
                                .map_err(|e| McpToolError::internal(format!("Adapter store error: {e}")))?
                                && let (Some(new_loss), Some(prev_loss)) = (
                                    current_loss,
                                    Self::metrics_from_trained(&prev).and_then(|m| m.loss),
                                ) {
                                    let improved = new_loss < prev_loss;
                                    result["ab_comparison"] = json!({
                                        "skill_name": adapter_skill,
                                        "previous_version": prev.version,
                                        "previous_loss": prev_loss,
                                        "new_loss": new_loss,
                                        "loss_improved": improved,
                                        "auto_promoted": improved,
                                    });
                                }
                        }
                    }

                    Ok(result)
                }
                Err(e) => Err(McpToolError::internal(format!("Status query failed: {e}"))),
            }
        })
        .await
    }
}
