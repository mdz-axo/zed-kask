use crate::TrainingServer;
use crate::adapters::AdapterMetrics;
use crate::providers::TrainingJobStatus;
use crate::tools::error_mapping::{map_adapter_store_error, map_host_provider_error};
use crate::types::TrainStatusRequest;
use hkask_mcp_server::server::McpToolError;
use hkask_mcp_server::server::execute_tool;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde_json::json;

#[tool_router(router = status_router, vis = "pub")]
impl TrainingServer {
    #[tool(
        description = "Check the status of a training job. Returns pod status, SSH connection info, uptime, GPU type, and recent log lines. When training completes (detected via HuggingFace completion manifest), automatically registers the adapter with metadata from the manifest."
    )]
    pub async fn training_status(
        &self,
        Parameters(TrainStatusRequest { job_id }): Parameters<TrainStatusRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "training_status", async {
            match self.host.status(&job_id).await {
                Ok(pod_status) => {
                    // The pod stays RUNNING (exec sleep infinity for SSH
                    // debugging), so RunPod's desiredStatus alone cannot signal
                    // completion. Check for a completion manifest on HuggingFace.
                    // D7 (operator ruling 2026-09-29): a FAILED check surfaces
                    // RunningUnknown + the reason — "Running" claims the check
                    // confirmed training in progress, and the pod staying up
                    // after completion means an unverified Running can hide a
                    // finished job. The legitimate short-circuit (HF not
                    // configured / no artifacts) keeps Running.
                    let (status, manifest, check_note) =
                        if pod_status.status == TrainingJobStatus::Running {
                            match self.check_completion_manifest(&job_id).await {
                                Ok(Some((status, manifest))) => (status, manifest, None),
                                Ok(None) => (TrainingJobStatus::Running, None, None),
                                Err(reason) => (
                                    TrainingJobStatus::RunningUnknown,
                                    None,
                                    Some(reason),
                                ),
                            }
                        } else {
                            (pod_status.status, None, None)
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
                    if let Some(reason) = check_note {
                        result["completion_check_unavailable"] = json!(reason);
                    }

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
                    // RunningUnknown keeps the fetch: the pod is up (the check
                    // that failed is the COMPLETION check, not the pod), and the
                    // logs are exactly what the operator needs to diagnose.
                    if !pod_status.ssh_command.is_empty()
                        && matches!(
                            status,
                            TrainingJobStatus::Running | TrainingJobStatus::RunningUnknown
                        )
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
                        // G-R1: Evaluate runtime metrics from the completion manifest
                        // and emit reg.lora.runtime spans. Mirrors the HF trackio
                        // alert pattern — loss spikes, NaN gradients, vanishing loss.
                        if let Some(ref manifest) = manifest {
                            let runtime_findings =
                                crate::lora_validation::validate_runtime_metrics(manifest);
                            for finding in &runtime_findings {
                                tracing::warn!(
                                    target: "reg.lora.runtime",
                                    gate = finding.gate_id,
                                    severity = finding.severity.as_str(),
                                    message = %finding.message,
                                    source = %finding.source,
                                    job_id = %job_id,
                                    "LoRA runtime alert gate"
                                );
                            }
                            if runtime_findings.is_empty() && manifest.loss.is_some() {
                                tracing::info!(
                                    target: "reg.lora.runtime",
                                    gate = "G-R1",
                                    severity = "pass",
                                    job_id = %job_id,
                                    loss = ?manifest.loss,
                                    "Runtime metrics passed G-R1"
                                );
                            }
                            if !runtime_findings.is_empty() {
                                result["runtime_findings"] = json!(
                                    runtime_findings.iter().map(|f| f.to_json()).collect::<Vec<_>>()
                                );
                            }
                        }

                        self.finalize_completed_job(&job_id, manifest.as_ref(), &mut result)
                            .await?;
                    }

                    Ok(result)
                }
                Err(e) => Err(map_host_provider_error(e)),
            }
        })
        .await
    }

    /// The completion finalization — adapter registration from
    /// the completion manifest plus the skill-retrain A/B comparison.
    /// Extracted from `training_status` so the manifest path is testable
    /// without a live HuggingFace fetch (tests pass a fixture manifest
    /// directly).
    pub(crate) async fn finalize_completed_job(
        &self,
        job_id: &str,
        manifest: Option<&crate::huggingface::CompletionManifest>,
        result: &mut serde_json::Value,
    ) -> Result<(), McpToolError> {
        // A malformed job id is a caller error — never a silent nil-UUID
        // lookup miss (pre-fix, `unwrap_or_default` made the
        // pre-registration check silently miss and proceed to register
        // under the malformed id).
        let adapter_uuid = uuid::Uuid::parse_str(job_id).map_err(|error| {
            McpToolError::invalid_argument(format!("malformed job id {job_id:?}: {error}"))
        })?;
        let adapter: crate::adapter::TrainedLoRAAdapter = match self
            .adapter_store
            .get_by_id(adapter_uuid)
            .map_err(map_adapter_store_error)?
        {
            Some(existing) => {
                result["adapter_registered"] = json!(true);
                result["adapter_note"] = json!("Already registered (pre-registered by retrain)");
                existing
            }
            None => {
                if let Some(manifest) = manifest {
                    let base_model = manifest.base_model.clone().unwrap_or_default();
                    let adapter_name = format!("adapter-{}", &job_id[..8.min(job_id.len())]);
                    let weight_path = manifest.adapter.repository.clone();
                    let adapter = Self::build_trained_adapter(
                        job_id.to_string(),
                        adapter_name,
                        base_model.clone(),
                        String::new(),
                        job_id.to_string(),
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
                    match self
                        .adapter_store
                        .store(&adapter)
                        .map_err(map_adapter_store_error)
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
                            return Ok(());
                        }
                    }
                } else {
                    result["adapter_registered"] = json!(false);
                    result["adapter_note"] = json!("No completion manifest available");
                    return Ok(());
                }
            }
        };

        // A/B comparison (skill retraining only)
        let adapter_skill = adapter.skill_name.clone().unwrap_or_default();
        if !adapter_skill.is_empty() {
            let current_loss = Self::metrics_from_trained(&adapter).and_then(|m| m.loss);
            if let Some(prev) = self
                .adapter_store
                .get_previous_by_skill_name(&adapter_skill, adapter.id)
                .map_err(map_adapter_store_error)?
                && let (Some(new_loss), Some(prev_loss)) = (
                    current_loss,
                    Self::metrics_from_trained(&prev).and_then(|m| m.loss),
                )
            {
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
        Ok(())
    }
}

#[cfg(test)]
mod status_contract_tests {
    use crate::TrainingServer;
    use crate::adapter::AdapterStore;
    use crate::dataset::DatasetPipeline;
    use crate::providers::types::{PodStatus, TrainingJobStatus};
    use crate::providers::{
        HostProviderError, TrainingHarnessId, TrainingHost, TrainingHostId, TrainingJob,
    };
    use crate::types::TrainStatusRequest;
    use hkask_storage::database::driver::DatabaseDriver;
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_types::ports::{InferenceError, InferencePort, InferenceResult};
    use hkask_types::template::LLMParameters;
    use hkask_types::{ChatToolDefinition, WebID};
    use rmcp::handler::server::wrapper::Parameters;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    /// No-op inference port — mirrors the smoke module's.
    struct NoopInferencePort;

    impl InferencePort for NoopInferencePort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async { Err(InferenceError::NotConfigured("noop".into())) })
        }
    }

    /// A host whose pod is up and Running — the D7 test fixture.
    struct RunningPodHost;

    #[async_trait::async_trait]
    impl TrainingHost for RunningPodHost {
        async fn submit(&self, _job: &TrainingJob) -> Result<String, HostProviderError> {
            Err(HostProviderError::NotConfigured("test host".into()))
        }
        async fn status(&self, _job_id: &str) -> Result<PodStatus, HostProviderError> {
            Ok(PodStatus {
                status: TrainingJobStatus::Running,
                pod_id: "pod-test".into(),
                ssh_command: String::new(),
                ip: "10.0.0.1".into(),
                ssh_port: 0,
                is_public_ip: false,
                uptime_seconds: 60,
                gpu_type: "test-gpu".into(),
                fail_reason: None,
            })
        }
        async fn cancel(&self, _job_id: &str) -> Result<(), HostProviderError> {
            Err(HostProviderError::NotConfigured("test host".into()))
        }
    }

    fn server_with_running_host() -> TrainingServer {
        let pool = SqliteDriver::in_memory_pool().expect("in-memory pool");
        let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new(pool));
        let adapter_store = Arc::new(AdapterStore::from_driver(driver).expect("adapter store"));
        let pipeline = Mutex::new(DatasetPipeline::new(
            std::env::temp_dir().join("hkask-mcp-training-status-test"),
        ));
        let inference_port: Arc<dyn InferencePort> = Arc::new(NoopInferencePort);
        TrainingServer::new(
            WebID::new(),
            None,
            None,
            Box::new(RunningPodHost),
            TrainingHostId::Runpod,
            TrainingHarnessId::Axolotl,
            pipeline,
            adapter_store,
            // job_store: none — the short-circuit fires before the store read.
            None,
            inference_port,
        )
    }

    /// expect: "RunningUnknown serializes snake_case and persists as the
    /// Debug-lowercase form the job store writes" [P4] — the wire and store
    /// shapes of the D7 state are pinned so consumers cannot drift.
    #[test]
    fn running_unknown_serializes_and_persists_stably() {
        assert_eq!(
            serde_json::to_value(TrainingJobStatus::RunningUnknown).expect("serialize"),
            serde_json::json!("running_unknown")
        );
        assert_eq!(
            format!("{:?}", TrainingJobStatus::RunningUnknown).to_lowercase(),
            "runningunknown"
        );
    }

    /// expect: "The legitimate short-circuit keeps Running — no HF
    /// configuration is not an unknown, it is nothing to check" [P4].
    /// D7's boundary pin: only a FAILED check produces RunningUnknown;
    /// an unconfigured HF path must not.
    #[tokio::test]
    async fn unconfigured_completion_check_keeps_running_not_unknown() {
        let server = server_with_running_host();
        // The test env has no HF_TOKEN, so check_completion_manifest
        // short-circuits to Ok(None) — the legitimate boundary.
        let output = server
            .training_status(Parameters(TrainStatusRequest {
                job_id: "job-without-hf".to_string(),
            }))
            .await
            .expect("status call succeeds");
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("json");
        let body = &parsed["content"];
        assert_eq!(
            body["status"].as_str(),
            Some("running"),
            "the unconfigured short-circuit must keep Running, not RunningUnknown: {parsed}"
        );
        assert!(
            body.get("completion_check_unavailable").is_none(),
            "no failure note on the legitimate short-circuit: {parsed}"
        );
    }
}
