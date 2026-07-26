use crate::TrainingServer;
use crate::lora_validation;
use crate::types::TrainValidateConfigRequest;
use hkask_mcp_server::server::execute_tool;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use serde_json::json;

impl TrainingServer {
    #[tool(
        description = "Validate training params against the lora-training skill's math-contract gates (G-M1 no-op-at-init, G-M2 merge equivalence, G-M3 scaling form, G-M4 rank budget, G-Q1 frozen base quantized, G-Q2 adapter dtype, G-Q4 no silent upcast, G-Q5 paged optimizer, G-H1 harness-method compatibility). Also validates dataset size (G-D1) and dataset format compatibility (G-D0) if dataset_path is provided. G-D0 detects the dataset format, checks it against the expected format for the selected trainer, and emits copy-paste Python mapping code when a fixable column-name mismatch is found. When dataset_path is provided, also profiles the dataset (G-D0) and returns a DatasetProfile with format, sample count, content length statistics, token estimates, role distribution, multi-turn detection, vision data detection, and preference pair balance. Returns findings with severity (refuse/warn/info), gate ID, message, source citation, and remediation. Emits reg.lora.audit spans. This is the runtime enforcement point for the lora-training skill's audit-config phase."
    )]
    pub async fn training_validate_config(
        &self,
        Parameters(TrainValidateConfigRequest {
            params,
            dataset_path,
            base_model,
        }): Parameters<TrainValidateConfigRequest>,
    ) -> String {
        execute_tool(self, "training_validate_config", async {
            let mut findings = lora_validation::validate_training_params(&params);

            // Derive the trainer preference from params.trl_trainer for G-D0.
            let trainer_preference = params.trl_trainer.as_ref().map(|t| match t {
                crate::providers::types::TrlTrainer::Sft => "sft",
                crate::providers::types::TrlTrainer::Dpo => "dpo",
                crate::providers::types::TrlTrainer::Kto => "kto",
                crate::providers::types::TrlTrainer::Orpo => "orpo",
                crate::providers::types::TrlTrainer::Reward => "reward",
            });

            // G-D0: Dataset format compatibility (when dataset_path is provided).
            let dataset_format_owned: Option<lora_validation::DatasetFormatResult> =
                if let Some(ref ds_path) = dataset_path {
                    let format_result = lora_validation::validate_dataset_format(
                        std::path::Path::new(ds_path),
                        trainer_preference,
                        None,
                    );
                    findings.extend(format_result.findings.clone());
                    Some(format_result)
                } else {
                    None
                };

            if let Some(ref ds_path) = dataset_path {
                findings.extend(lora_validation::validate_dataset_size(std::path::Path::new(ds_path)));
            }

            if let Some(ref model) = base_model {
                findings.extend(lora_validation::validate_paged_optimizer(&params, model));
            }

            for finding in &findings {
                let severity_str = match finding.severity {
                    lora_validation::ValidationSeverity::Refuse => "refuse",
                    lora_validation::ValidationSeverity::Warn => "warn",
                    lora_validation::ValidationSeverity::Info => "info",
                };
                match finding.severity {
                    lora_validation::ValidationSeverity::Refuse => {
                        tracing::error!(target: "reg.lora.audit", gate = finding.gate_id, severity = severity_str, message = %finding.message, source = %finding.source, "LoRA training-config gate refused");
                    }
                    lora_validation::ValidationSeverity::Warn => {
                        tracing::warn!(target: "reg.lora.audit", gate = finding.gate_id, severity = severity_str, message = %finding.message, source = %finding.source, "LoRA training-config gate warning");
                    }
                    lora_validation::ValidationSeverity::Info => {
                        tracing::info!(target: "reg.lora.audit", gate = finding.gate_id, severity = severity_str, message = %finding.message, source = %finding.source, "LoRA training-config gate info");
                    }
                }
            }

            if findings.is_empty() {
                tracing::info!(target: "reg.lora.audit", gate = "all", severity = "pass", "LoRA training-config audit passed all static gates");
            }

            let has_refusals = lora_validation::has_refusals(&findings);
            let findings_json: Vec<serde_json::Value> = findings
                .iter()
                .map(|f| {
                    json!({
                        "gate_id": f.gate_id,
                        "severity": match f.severity {
                            lora_validation::ValidationSeverity::Refuse => "refuse",
                            lora_validation::ValidationSeverity::Warn => "warn",
                            lora_validation::ValidationSeverity::Info => "info",
                        },
                        "message": f.message,
                        "source": f.source,
                        "remediation": f.remediation,
                    })
                })
                .collect();

            let dataset_format_json = dataset_format_owned.as_ref().map(|r| {
                json!({
                    "verdict": match r.verdict {
                        lora_validation::DatasetFormatVerdict::Ready => "ready",
                        lora_validation::DatasetFormatVerdict::NeedsMapping => "needs_mapping",
                        lora_validation::DatasetFormatVerdict::Incompatible => "incompatible",
                    },
                    "detected_format": format!("{:?}", r.detected_format),
                    "expected_format": format!("{:?}", r.expected_format),
                    "mapping_code": r.mapping_code,
                })
            });

            Ok(json!({
                "params": serde_json::to_value(&params).unwrap_or_default(),
                "findings": findings_json,
                "finding_count": findings.len(),
                "has_refusals": has_refusals,
                "dataset_format": dataset_format_json,
                "verdict": if has_refusals {
                    "fail"
                } else if findings.iter().any(|f| f.severity == lora_validation::ValidationSeverity::Warn) {
                    "conditional"
                } else {
                    "pass"
                },
                "gates_evaluated": ["G-M1", "G-M2", "G-M3", "G-M4", "G-Q1", "G-Q2", "G-Q4", "G-Q5", "G-D0", "G-D1", "G-H1"],
            }))
        })
        .await
    }
}
