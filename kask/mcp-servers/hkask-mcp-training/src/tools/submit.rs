use crate::TrainingServer;
use crate::huggingface::HuggingFaceTraining;
use crate::lora_validation;
use crate::providers::{TrainingHostId, TrainingJob, TrainingJobStatus};
use crate::tools::error_mapping::{
    map_adapter_store_error, map_host_provider_error, map_job_store_error,
    map_training_artifact_error,
};
use crate::types::TrainSubmitRequest;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic, map_io_error};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde_json::json;
use sha2::Digest;

/// A/B baseline for retrain comparison.
struct AbBaseline {
    previous_version: u32,
    previous_loss: f32,
    previous_perplexity: f32,
}

#[tool_router(router = submit_router, vis = "pub")]
impl TrainingServer {
    #[tool(
        description = "Submit a training job for execution. Ingests, normalizes, and submits a dataset for LoRA fine-tuning via the selected harness (Axolotl YAML, TRL Python, or Ludwig YAML) on Runpod. The harness is selected from params.harness (operator-accepted from the lora-training skill's G6 gate), defaulting to Axolotl. When `feedback_path` is provided, enters retrain mode: merges the original dataset with curated feedback, deduplicates by user question, increments the adapter version based on existing adapters with the same `skill_name`, and pre-registers adapter metadata so training_status can complete the A/B comparison on job completion."
    )]
    pub async fn training_submit(
        &self,
        Parameters(TrainSubmitRequest {
            dataset_path,
            base_model,
            params,
            feedback_path,
            skill_name,
            adapter_name,
            merged_output_path,
            confirmed,
        }): Parameters<TrainSubmitRequest>,
    ) -> String {
        execute_tool_semantic(self, "training_submit", Self::ontology_anchor("training_submit"), async {
            // P2 consent gate — enforce operator authorization before GPU spend.
            // The historical pipeline runner enforced this but was lost when the
            // runner was removed. The manifest's `requires_consent: true` is
            // documentation; this is the enforcement point.
            if !confirmed {
                return Err(McpToolError::permission_denied(
                    "Consent required: training_submit spends real money on GPU time. \
                     The agent must present the estimated cost to the operator and \
                     receive explicit approval before setting `confirmed: true`."
                ));
            }

            // Contain the caller-supplied dataset path before any read: an
            // absolute path like /etc/passwd or a traversal must not reach the
            // pipeline reads (CWE-200).
            let file_path = hkask_mcp_server::contain_for_read(&dataset_path)?;

            // G-P1: Persistence preflight — verify HuggingFace artifact persistence
            // is configured before the expensive dataset normalization step.
            let hf_training_result = HuggingFaceTraining::from_env()
                .map(|_| ())
                .map_err(|error| error.to_string());
            let persistence_refuse = match (&self.host_id, &hf_training_result) {
                (crate::providers::TrainingHostId::Runpod, Err(reason)) => {
                    tracing::error!(
                        target: "reg.lora.audit",
                        gate = "G-P1",
                        severity = "refuse",
                        message = %reason,
                        "Runpod persistence env vars not configured"
                    );
                    Some(format!(
                        "G-P1: Runpod host requires HuggingFace persistence env vars \
                         (HKASK_HF_ARTIFACT_OWNER, HKASK_HF_MODEL_REPO, HF_TOKEN) — \
                         without them, the adapter and completion manifest are lost when the \
                         ephemeral pod terminates. Error: {reason}"
                    ))
                }
                (crate::providers::TrainingHostId::Nebius, _) => {
                    tracing::warn!(
                        target: "reg.lora.audit",
                        gate = "G-P1",
                        severity = "warn",
                        host = ?self.host_id,
                        "Host does not configure HuggingFace artifact persistence — completion cannot be detected: training_status will report 'Running' indefinitely (Runpod is the only host with completion detection, via HuggingFace artifacts)"
                    );
                    None
                }
                _ => None,
            };
            if let Some(msg) = persistence_refuse {
                return Err(McpToolError::failed_precondition(format!(
                    "Training persistence not configured: {msg}"
                )));
            }

            let retrain_mode = feedback_path.is_some();
            let mut ab_baseline: Option<AbBaseline> = None;
            let mut version: u32 = 1;
            let resolved_skill_name: Option<String> = skill_name.clone();
            let mut resolved_adapter_name: Option<String> = adapter_name.clone();
            // Hoisted to the outer scope so the result JSON can report retrain
            // provenance to the operator. Only meaningful when `retrain_mode`.
            let mut previous_adapter_exists: bool = false;
            let mut original_examples: usize = 0;
            let mut feedback_examples: usize = 0;

            let normalized_path = if retrain_mode {
                let feedback = hkask_mcp_server::contain_for_read(
                    feedback_path.as_ref().expect("retrain_mode guard ensures feedback_path is Some"),
                )?;
                let skill = skill_name.clone().unwrap_or_default();
                if skill.is_empty() {
                    return Err(McpToolError::invalid_argument("skill_name is required when feedback_path is set (retrain mode)"));
                }
                hkask_mcp_server::validate_identifier("skill_name", &skill, 64)?;
                tracing::info!(target: "hkask.training.retrain.started", skill = %skill, "Retraining job initiated");

                let original_content = std::fs::read_to_string(&file_path).map_err(|e| McpToolError::invalid_argument(format!("Failed to read original dataset: {e}")))?;
                let feedback_content = std::fs::read_to_string(&feedback).map_err(|e| McpToolError::invalid_argument(format!("Failed to read feedback dataset: {e}")))?;

                let mut merged = String::new();
                let mut seen_questions: std::collections::HashSet<String> = std::collections::HashSet::new();

                for (content, counter) in [(&original_content, &mut original_examples), (&feedback_content, &mut feedback_examples)] {
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() { continue; }
                        if let Ok(record) = serde_json::from_str::<serde_json::Value>(trimmed)
                            && let Some(messages) = record.get("messages").and_then(|m| m.as_array())
                        {
                            let question = messages.iter()
                                .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                                .and_then(|m| m.get("content").and_then(|c| c.as_str()))
                                .unwrap_or("");
                            if !question.is_empty() && seen_questions.insert(question.to_string()) {
                                merged.push_str(trimmed);
                                merged.push('\n');
                                *counter += 1;
                            }
                        }
                    }
                }

                if merged.is_empty() {
                    return Err(McpToolError::invalid_argument("No valid examples found in either dataset"));
                }

                let merged_path = match merged_output_path {
                    // LLM-supplied write target: contain to the project root so a
                    // path like ~/.ssh/authorized_keys is rejected (CWE-73).
                    Some(p) => hkask_mcp_server::contain_for_write(&p)?,
                    // Server-chosen scratch: the pipeline cache dir is not
                    // LLM-controlled, so no containment is needed (and it lives
                    // under the data dir, outside the project root anyway).
                    None => self
                        .pipeline
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .cache_dir()
                        .join(format!("hkask-retrain-{skill}.jsonl")),
                };
                std::fs::write(&merged_path, &merged).map_err(|e| map_io_error(e, "Failed to write merged dataset"))?;

                match self.adapter_store.get_by_skill_name(&skill) {
                    Ok(Some(prev)) => {
                        let prev_version = prev.version.as_deref().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                        version = prev_version + 1;
                        previous_adapter_exists = true;
                        ab_baseline = Self::metrics_from_trained(&prev).map(|m| AbBaseline {
                            previous_version: prev_version,
                            previous_loss: m.loss.unwrap_or(0.0),
                            previous_perplexity: m.perplexity.unwrap_or(0.0),
                        });
                    }
                    _ => { version = 1; previous_adapter_exists = false; }
                }

                if resolved_adapter_name.is_none() {
                    resolved_adapter_name = Some(format!("{skill}-v{version}"));
                }

                match self.pipeline.lock().unwrap_or_else(|e| e.into_inner()).ingest(&merged_path) {
                    Ok(path) => path,
                    Err(e) => return Err(McpToolError::invalid_argument(format!("Dataset pipeline error: {e}"))),
                }
            } else {
                match self.pipeline.lock().unwrap_or_else(|e| e.into_inner()).ingest(&file_path) {
                    Ok(path) => path,
                    Err(e) => return Err(McpToolError::invalid_argument(format!("Dataset pipeline error: {e}"))),
                }
            };

            let mut token_warnings: Vec<serde_json::Value> = Vec::new();
            if let Ok(normalized_content) = std::fs::read_to_string(&normalized_path) {
                for (i, line) in normalized_content.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    let approx_tokens = trimmed.len() / 4;
                    if approx_tokens > 4096 {
                        token_warnings.push(json!({"line": i + 1, "approx_tokens": approx_tokens, "severity": "error", "message": "Example likely exceeds 16K context window — may be truncated during training"}));
                    } else if approx_tokens > 2048 {
                        token_warnings.push(json!({"line": i + 1, "approx_tokens": approx_tokens, "severity": "warning", "message": "Example approaches 8K context limit — consider truncation"}));
                    }
                }
            }

            let resolver = crate::huggingface::LocalModelResolver;
            let provenance = resolver.resolve(&base_model);
            if let Ok(ref p) = provenance {
                tracing::info!(target: "hkask.training.provenance.resolved", model_id = %p.model_id, architecture = %p.architecture, lora_compatible = p.lora_compatible, is_gated = p.is_gated, "Model provenance resolved");
            }

            let num_epochs = params.as_ref().map(|p| p.num_epochs).unwrap_or(3);
            let resolved_params = params.unwrap_or_default();

            let validation_findings = lora_validation::validate_training_params(&resolved_params);

            // G-D0: Dataset format compatibility check. Run before the refusal
            // gate so incompatible-dataset refusals are reported alongside
            // config refusals. Derive trainer preference from trl_trainer.
            let trainer_pref = resolved_params
                .trl_trainer
                .as_ref()
                .map(|t| t.as_dataset_preference());
            let dataset_format_result = lora_validation::validate_dataset_format(
                &file_path,
                trainer_pref,
                None,
            );
            let mut validation_findings = validation_findings;
            validation_findings.extend(dataset_format_result.findings.clone());

            if lora_validation::has_refusals(&validation_findings) {
                let refusals: Vec<_> = validation_findings.iter().filter(|f| f.severity == lora_validation::ValidationSeverity::Refuse).collect();
                let messages: Vec<String> = refusals.iter().map(|f| format!("{}: {}", f.gate_id, f.message)).collect();
                for f in &refusals {
                    tracing::error!(target: "reg.lora.audit", gate = f.gate_id, severity = "refuse", message = %f.message, source = %f.source, "LoRA training-config gate refused at submit");
                }
                return Err(McpToolError::invalid_argument(format!("Training config failed math-contract validation: {}", messages.join("; "))));
            }
            // G-D0 NeedsMapping: warn but do not block submit. The operator may
            // have already applied the mapping code or accepted the risk.
            if dataset_format_result.verdict == lora_validation::DatasetFormatVerdict::NeedsMapping {
                tracing::warn!(
                    target: "reg.lora.audit",
                    gate = "G-D0",
                    severity = "warn",
                    verdict = "needs_mapping",
                    detected_format = ?dataset_format_result.detected_format,
                    expected_format = ?dataset_format_result.expected_format,
                    "Dataset format needs mapping — mapping code emitted"
                );
            }
            for finding in &validation_findings {
                if finding.severity == lora_validation::ValidationSeverity::Warn {
                    tracing::warn!(
                        target: "reg.lora.audit",
                        gate = finding.gate_id,
                        severity = finding.severity.as_str(),
                        message = %finding.message,
                        source = %finding.source,
                        "LoRA training-config gate warning at submit"
                    );
                    tracing::warn!(
                        target: "hkask.training.validation.warn",
                        gate = finding.gate_id,
                        message = %finding.message,
                        remediation = %finding.remediation,
                        "Training config warning"
                    );
                } else if finding.severity == lora_validation::ValidationSeverity::Info {
                    tracing::info!(
                        target: "reg.lora.audit",
                        gate = finding.gate_id,
                        severity = finding.severity.as_str(),
                        message = %finding.message,
                        source = %finding.source,
                        "LoRA training-config gate info at submit"
                    );
                }
            }

            let mut job = TrainingJob {
                id: uuid::Uuid::new_v4().to_string(),
                dataset_path: normalized_path.clone(),
                base_model: base_model.clone(),
                params: resolved_params.clone(),
                status: TrainingJobStatus::Queued,
                created_at: chrono::Utc::now(),
                host: self.host_id,
                harness: resolved_params.harness.unwrap_or(self.harness_id),
                owner: None,
                skill_name: resolved_skill_name.clone(),
                estimated_cost_urj: crate::providers::types::estimate_training_cost_urj(&self.host_id, num_epochs, &base_model),
                artifacts: None,
            };

            if self.host_id == TrainingHostId::Runpod {
                let bytes = std::fs::read(&normalized_path).map_err(|error| map_io_error(error, "Read normalized dataset for publication"))?;
                let dataset_sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
                let training = HuggingFaceTraining::from_env().map_err(|error| McpToolError::failed_precondition(format!("Configure Hugging Face training artifacts: {error}")))?;
                let dataset = training.publish_dataset(&job.id, bytes, &dataset_sha256).await.map_err(map_training_artifact_error)?;
                job.artifacts = Some(training.prepare_training_artifacts(&job.id, dataset).await.map_err(map_training_artifact_error)?);
            } else {
                // Completion detection is not yet wired for Nebius:
                // `check_completion_manifest` short-circuits when `job.artifacts`
                // is `None`, so `training_status` stays `Running` indefinitely.
                // Runpod is the only host that publishes HuggingFace artifacts.
                tracing::warn!(
                    target: "hkask.training.completion",
                    host = ?self.host_id,
                    "Training completion detection is not yet wired for this host. \
                     training_status will report 'Running' indefinitely. \
                     Runpod is the only host with completion detection via HuggingFace artifacts."
                );
            }

            // Persistence precondition: without a durable job store, the job's
            // state is lost on restart and `training_status` cannot detect
            // completion (it reads artifacts back from `job_store`). The init-time
            // warn when `HKASK_DB_PASSPHRASE` is missing is not a tool-level signal;
            // fail fast here so the operator sees the precondition before GPU spend.
            if self.job_store.is_none() {
                return Err(McpToolError::permission_denied(
                    "Training persistence not available — set HKASK_DB_PASSPHRASE for encrypted persistent storage. In-memory mode loses all job/adapter state on restart and prevents completion detection.",
                ));
            }

            if let Some(ref job_store) = self.job_store {
                let params_json = serde_json::to_string(&job.params).unwrap_or_default();
                let status_str = format!("{:?}", TrainingJobStatus::Queued).to_lowercase();
                if let Err(e) = job_store.store(&job.id, &job.base_model, &job.dataset_path.to_string_lossy(), &params_json, &status_str, job.created_at.timestamp(), &format!("{:?}", job.host).to_lowercase()) {
                    tracing::warn!(target: "hkask.training.job.persist", job_id = %job.id, error = %e, "Failed to persist job");
                }
            }

            if let (Some(job_store), Some(artifacts)) = (&self.job_store, &job.artifacts) {
                job_store.update_artifacts(&job.id, artifacts).map_err(map_job_store_error)?;
            }

            if retrain_mode {
                let adapter = Self::build_trained_adapter(
                    job.id.clone(), resolved_adapter_name.clone().unwrap_or_default(),
                    base_model.clone(), String::new(), job.id.clone(),
                    chrono::Utc::now().timestamp(), 0,
                    resolved_skill_name.clone().unwrap_or_default(), version, None, None,
                );
                if let Err(e) = self.adapter_store.store(&adapter).map_err(map_adapter_store_error) {
                    tracing::warn!(target: "hkask.training.retrain", adapter_id = %job.id, error = %e, "Failed to pre-register adapter metadata");
                }
            }

            match self.host.submit(&job).await {
                Ok(provider_job_id) => {
                    if let Some(job_store) = &self.job_store {
                        job_store.update_provider_job_id(&job.id, &provider_job_id).map_err(map_job_store_error)?;
                    }
                    let mut result = json!({"job_id": job.id, "provider_job_id": provider_job_id, "status": "queued", "base_model": base_model, "host": format!("{:?}", self.host_id)});
                    result["estimated_cost_urj"] = json!(job.estimated_cost_urj);
                    if retrain_mode {
                        result["retrain"] = json!(true);
                        result["skill_name"] = json!(resolved_skill_name);
                        result["adapter_name"] = json!(resolved_adapter_name);
                        result["version"] = json!(version);
                        result["previous_adapter_exists"] = json!(previous_adapter_exists);
                        result["original_examples"] = json!(original_examples);
                        result["feedback_examples"] = json!(feedback_examples);
                        if let Some(b) = &ab_baseline {
                            result["ab_baseline"] = json!({"previous_version": b.previous_version, "previous_loss": b.previous_loss, "previous_perplexity": b.previous_perplexity, "description": "A/B baseline from previous adapter."});
                        }
                    }
                    tracing::info!(target: "hkask.qa.cost.training_job", job_id = %job.id, provider_job_id = %provider_job_id, estimated_cost_urj = job.estimated_cost_urj, retrain = retrain_mode, "Training job submitted");
                    if !token_warnings.is_empty() {
                        result["token_warnings"] = json!(token_warnings);
                        result["token_warning_count"] = json!(token_warnings.len());
                    }
                    Ok(result)
                }
                Err(e) => {
                    if let Some(job_store) = &self.job_store
                        && let Err(store_error) = job_store.update_status(&job.id, "failed")
                    {
                        tracing::warn!(target: "hkask.training.job.persist", job_id = %job.id, error = %store_error, "Failed to persist submission failure");
                    }
                    tracing::error!(target: "hkask.training.job.fail", job_id = %job.id, error = %e, "Training job submission failed");
                    Err(map_host_provider_error(e))
                }
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterStore;
    use crate::dataset::DatasetPipeline;
    use crate::providers::types::PodStatus;
    use crate::providers::{HostProviderError, TrainingHost, TrainingHostId, TrainingJob};
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_types::WebID;
    use hkask_types::{InferenceError, InferencePort, InferenceResult, LLMParameters};
    use rmcp::handler::server::wrapper::Parameters;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;

    /// Mock `TrainingHost` whose `submit` is never reached in the
    /// `job_store.is_none()` test — the guard fires before `host.submit`.
    /// If it were reached, it would error loudly rather than silently pass.
    struct UnreachedHost;
    #[async_trait::async_trait]
    impl TrainingHost for UnreachedHost {
        async fn submit(&self, _job: &TrainingJob) -> Result<String, HostProviderError> {
            Err(HostProviderError::Unavailable(
                "UnreachedHost.submit should not be reached — job_store guard must fire first"
                    .into(),
            ))
        }
        async fn status(&self, _job_id: &str) -> Result<PodStatus, HostProviderError> {
            Err(HostProviderError::Unavailable("mock".into()))
        }
        async fn cancel(&self, _job_id: &str) -> Result<(), HostProviderError> {
            Err(HostProviderError::Unavailable("mock".into()))
        }
    }

    /// Minimal `InferencePort` — `training_submit` does not call inference, so
    /// any method that is reached returns an error (test fails loudly).
    struct StubInference;
    impl InferencePort for StubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async {
                Err(InferenceError::Generation(
                    "StubInference: generate should not be reached".into(),
                ))
            })
        }
    }

    /// Build a `TrainingServer` with `job_store = None` (the in-memory fallback
    /// when `HKASK_DB_PASSPHRASE` is unset). Uses `DeepInfra` host_id so the
    /// Runpod-only HuggingFace artifact publish step is skipped — the
    /// `job_store.is_none()` guard is reachable without HF credentials.
    fn server_without_job_store(cache_dir: std::path::PathBuf) -> TrainingServer {
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> = {
            let pool = SqliteDriver::in_memory_pool().expect("in-memory pool");
            Arc::new(SqliteDriver::new(pool))
        };
        let adapter_store = AdapterStore::from_driver(driver).expect("adapter store init");
        TrainingServer::new(
            WebID::new(),
            std::sync::Arc::new(hkask_verification::VerificationStore::in_memory()),
            None, // store: Option<MemoryStore> — unused by training_submit
            Box::new(UnreachedHost),
            TrainingHostId::DeepInfra,
            crate::providers::TrainingHarnessId::Axolotl,
            Mutex::new(DatasetPipeline::new(cache_dir)),
            Arc::new(adapter_store),
            None, // job_store — the load-bearing None under test
            Arc::new(StubInference),
        )
    }

    // P1 regression: `training_submit` must surface a missing `job_store` as
    // `permission_denied`, NOT silently proceed and lose the job's state on
    // restart. The init-time warn when `HKASK_DB_PASSPHRASE` is missing is
    // not a tool-level signal; the guard here makes the missing-persistence
    // case loud and actionable at the tool boundary, before any GPU spend.
    //
    // We drive the real `training_submit` tool with a minimal ChatML dataset
    // and `job_store = None`. The guard fires after dataset ingestion and
    // validation (which pass) but before `host.submit` (which is never
    // reached — `UnreachedHost` would error if it were). The error must
    // classify as `permission_denied` and name `HKASK_DB_PASSPHRASE`.
    //
    // The dataset must live under the project root because `training_submit`
    // contains caller-supplied paths to the current working directory
    // (CWE-22/CWE-200). `target/` is inside the project root and gitignored,
    // so it's a safe scratch location for test fixtures.
    #[tokio::test]
    async fn training_submit_returns_permission_denied_when_job_store_missing() {
        let scratch_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("training_submit_test");
        std::fs::create_dir_all(&scratch_dir).expect("create scratch dir");
        let cache_dir = scratch_dir.join("cache");
        std::fs::create_dir_all(&cache_dir).expect("create cache dir");

        // Minimal valid ChatML dataset — one example, enough to pass ingestion.
        let dataset_path = scratch_dir.join("ds.jsonl");
        std::fs::write(
            &dataset_path,
            r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#,
        )
        .expect("write dataset");

        let server = server_without_job_store(cache_dir);
        let req = TrainSubmitRequest {
            dataset_path: dataset_path.to_string_lossy().to_string(),
            base_model: "Qwen/Qwen2.5-0.5B".to_string(),
            params: None,
            feedback_path: None,
            skill_name: None,
            adapter_name: None,
            merged_output_path: None,
            confirmed: true,
        };
        let out = server.training_submit(Parameters(req)).await;

        // The tool returns a String envelope; an Err path serializes as
        // `{"error": <msg>, "kind": "permission_denied"}`.
        let envelope = hkask_types::tool_response::parse_tool_error(&out).unwrap_or_else(|| {
            panic!(
                "training_submit with job_store=None must return an error envelope, \
                 not a success payload; got: {out}"
            );
        });
        assert_eq!(
            envelope.kind,
            Some(hkask_types::McpErrorKind::PermissionDenied),
            "missing job_store must classify as permission_denied (authorization \
             failure — persistence is a precondition for GPU spend), not \
             unavailable or a silent success; got message: {}",
            envelope.message,
        );
        assert!(
            envelope.message.contains("HKASK_DB_PASSPHRASE"),
            "error message must name HKASK_DB_PASSPHRASE so the operator knows \
             which credential to set; got: {}",
            envelope.message,
        );

        // Clean up the scratch fixture so the test is self-contained.
        let _ = std::fs::remove_dir_all(&scratch_dir);
    }
}
