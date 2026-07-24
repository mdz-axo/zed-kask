use crate::TrainingServer;
use crate::huggingface::HuggingFaceTraining;
use crate::lora_validation;
use crate::providers::{TrainingHostId, TrainingJob, TrainingJobStatus};
use crate::types::TrainSubmitRequest;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use serde_json::json;
use sha2::Digest;
use std::path::PathBuf;

/// A/B baseline for retrain comparison.
struct AbBaseline {
    previous_version: u32,
    previous_loss: f32,
    previous_perplexity: f32,
}

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
        }): Parameters<TrainSubmitRequest>,
    ) -> String {
        execute_tool(self, "training_submit", async {
            let file_path = PathBuf::from(&dataset_path);
            if !file_path.exists() {
                return Err(McpToolError::invalid_argument(format!("Dataset file not found: {dataset_path}")));
            }

            let retrain_mode = feedback_path.is_some();
            let mut ab_baseline: Option<AbBaseline> = None;
            let mut version: u32 = 1;
            let resolved_skill_name: Option<String> = skill_name.clone();
            let mut resolved_adapter_name: Option<String> = adapter_name.clone();

            let normalized_path = if retrain_mode {
                let feedback = PathBuf::from(feedback_path.as_ref().unwrap());
                hkask_mcp_server::validate_path("feedback_path", feedback.to_str().unwrap_or(""), 4096)?;
                if !feedback.exists() {
                    return Err(McpToolError::invalid_argument(format!("Feedback file not found: {}", feedback.display())));
                }
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
                let mut original_examples = 0usize;
                let mut feedback_examples = 0usize;

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

                let merged_path = merged_output_path.unwrap_or_else(|| format!("/tmp/hkask-retrain-{skill}.jsonl"));
                hkask_mcp_server::validate_path("merged_output_path", &merged_path, 4096)?;
                std::fs::write(&merged_path, &merged).map_err(|e| McpToolError::internal(format!("Failed to write merged dataset: {e}")))?;

                let previous_adapter_exists: bool;
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
                let _ = (previous_adapter_exists, original_examples, feedback_examples);

                match self.pipeline.lock().unwrap_or_else(|e| e.into_inner()).ingest(&PathBuf::from(&merged_path)) {
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
            let provenance = crate::huggingface::ModelResolver::resolve(&resolver, &base_model);
            if let Ok(ref p) = provenance {
                tracing::info!(target: "hkask.training.provenance.resolved", model_id = %p.model_id, architecture = %p.architecture, lora_compatible = p.lora_compatible, is_gated = p.is_gated, "Model provenance resolved");
            }

            let num_epochs = params.as_ref().map(|p| p.num_epochs).unwrap_or(3);
            let resolved_params = params.unwrap_or_default();

            let validation_findings = lora_validation::validate_training_params(&resolved_params);
            if lora_validation::has_refusals(&validation_findings) {
                let refusals: Vec<_> = validation_findings.iter().filter(|f| f.severity == lora_validation::ValidationSeverity::Refuse).collect();
                let messages: Vec<String> = refusals.iter().map(|f| format!("{}: {}", f.gate_id, f.message)).collect();
                for f in &refusals {
                    tracing::error!(target: "reg.lora.audit", gate = f.gate_id, severity = "refuse", message = %f.message, source = %f.source, "LoRA training-config gate refused at submit");
                }
                return Err(McpToolError::invalid_argument(format!("Training config failed math-contract validation: {}", messages.join("; "))));
            }
            for finding in &validation_findings {
                let severity_str = match finding.severity {
                    lora_validation::ValidationSeverity::Warn => "warn",
                    lora_validation::ValidationSeverity::Info => "info",
                    lora_validation::ValidationSeverity::Refuse => "refuse",
                };
                if finding.severity == lora_validation::ValidationSeverity::Warn {
                    tracing::warn!(target: "reg.lora.audit", gate = finding.gate_id, severity = severity_str, message = %finding.message, source = %finding.source, "LoRA training-config gate warning at submit");
                    tracing::warn!(target: "hkask.training.validation.warn", gate = finding.gate_id, message = %finding.message, remediation = %finding.remediation, "Training config warning");
                } else if finding.severity == lora_validation::ValidationSeverity::Info {
                    tracing::info!(target: "reg.lora.audit", gate = finding.gate_id, severity = severity_str, message = %finding.message, source = %finding.source, "LoRA training-config gate info at submit");
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
                let bytes = std::fs::read(&normalized_path).map_err(|error| McpToolError::internal(format!("Read normalized dataset for publication: {error}")))?;
                let dataset_sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
                let training = HuggingFaceTraining::from_env().map_err(|error| McpToolError::failed_precondition(format!("Configure Hugging Face training artifacts: {error}")))?;
                let dataset = training.publish_dataset(&job.id, bytes, &dataset_sha256).await.map_err(|error| McpToolError::internal(format!("Publish training dataset: {error}")))?;
                job.artifacts = Some(training.prepare_training_artifacts(&job.id, dataset).await.map_err(|error| McpToolError::internal(format!("Prepare training artifacts: {error}")))?);
            }

            if let Some(ref job_store) = self.job_store {
                let params_json = serde_json::to_string(&job.params).unwrap_or_default();
                let status_str = format!("{:?}", TrainingJobStatus::Queued).to_lowercase();
                if let Err(e) = job_store.store(&job.id, &job.base_model, &job.dataset_path.to_string_lossy(), &params_json, &status_str, job.created_at.timestamp(), &format!("{:?}", job.host).to_lowercase()) {
                    tracing::warn!(target: "hkask.training.job.persist", job_id = %job.id, error = %e, "Failed to persist job");
                }
            }

            if let (Some(job_store), Some(artifacts)) = (&self.job_store, &job.artifacts) {
                job_store.update_artifacts(&job.id, artifacts).map_err(|error| McpToolError::internal(format!("Persist training artifacts: {error}")))?;
            }

            if retrain_mode {
                let adapter = Self::build_trained_adapter(
                    job.id.clone(), resolved_adapter_name.clone().unwrap_or_default(),
                    base_model.clone(), String::new(), job.id.clone(),
                    chrono::Utc::now().timestamp(), 0,
                    resolved_skill_name.clone().unwrap_or_default(), version, None, None,
                );
                if let Err(e) = self.adapter_store.store(&adapter).map_err(|e| McpToolError::internal(format!("Adapter store error: {e}"))) {
                    tracing::warn!(target: "hkask.training.retrain", adapter_id = %job.id, error = %e, "Failed to pre-register adapter metadata");
                }
            }

            match self.host.submit(&job).await {
                Ok(provider_job_id) => {
                    if let Some(job_store) = &self.job_store {
                        job_store.update_provider_job_id(&job.id, &provider_job_id).map_err(|error| McpToolError::internal(format!("Persist provider job ID: {error}")))?;
                    }
                    let mut result = json!({"job_id": job.id, "provider_job_id": provider_job_id, "status": "queued", "base_model": base_model, "host": format!("{:?}", self.host_id)});
                    result["estimated_cost_urj"] = json!(job.estimated_cost_urj);
                    if retrain_mode {
                        result["retrain"] = json!(true);
                        result["skill_name"] = json!(resolved_skill_name);
                        result["adapter_name"] = json!(resolved_adapter_name);
                        result["version"] = json!(version);
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
                    Err(McpToolError::internal(format!("Training job failed: {e}")))
                }
            }
        })
        .await
    }
}
