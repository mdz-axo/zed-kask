//! Dataset-format validation (G-D0) — detects the on-disk dataset format and
//! checks it against the format expected for the selected training method.
//! Mirrors the HuggingFace `dataset_inspector.py` three-state pattern:
//! `Ready` (use directly), `NeedsMapping` (compatible but needs column-name
//! mapping — copy-paste Python is provided), `Incompatible` (cannot be used).
//!
//! Extracted from `lora_validation.rs` (deep-module split: dataset-format
//! compatibility is independent of the LoRA hyperparameter gates).

use super::{ValidationFinding, ValidationSeverity};

use crate::dataset::DatasetFormat;
/// Verdict from G-D0 dataset format compatibility check.
///
/// Mirrors the HuggingFace `dataset_inspector.py` three-state pattern:
/// `Ready` (use directly), `NeedsMapping` (compatible but needs preprocessing —
/// mapping code is provided), `Incompatible` (cannot be used for this method).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DatasetFormatVerdict {
    /// Dataset format matches the expected format for the selected method.
    Ready,
    /// Dataset is compatible but needs column-name mapping. `mapping_code` is
    /// copy-paste Python that transforms the dataset into the expected schema.
    NeedsMapping,
    /// Dataset cannot be used for the selected method (e.g., SFT data for DPO).
    Incompatible,
    /// Dataset format could not be detected, so compatibility with the
    /// expected format could not be determined. Callers should treat this
    /// like `Incompatible` for refusal purposes (an expected format was
    /// derivable but the dataset could not be matched against it), or surface
    /// it distinctly to the operator.
    Undetermined,
}

/// Result of G-D0 dataset format validation.
#[derive(Debug, Clone)]
pub(crate) struct DatasetFormatResult {
    /// The verdict: ready, needs mapping, or incompatible.
    pub verdict: DatasetFormatVerdict,
    /// The format detected from the dataset file (None if detection failed).
    pub detected_format: Option<DatasetFormat>,
    /// The format expected by the selected trainer/method (None if undetermined).
    pub expected_format: Option<DatasetFormat>,
    /// Copy-paste Python mapping code when verdict is `NeedsMapping`.
    /// Empty string otherwise.
    pub mapping_code: String,
    /// Validation findings (G-D0 warnings/refusals).
    pub findings: Vec<ValidationFinding>,
}

/// G-D0: Dataset format compatibility check.
///
/// Detects the dataset format from the file and checks it against the format
/// expected by the selected trainer/method. When the detected format is
/// structurally compatible but uses non-standard column names, emits
/// copy-paste Python mapping code (mirrors HF's `dataset_inspector.py`).
///
/// This is the runtime enforcement point for the lora-training skill's G-D0
/// gate. Called from `training_validate_config` and `training_submit`.
///
/// # Arguments
/// * `dataset_path` - Path to the dataset file.
/// * `trainer_preference` - The operator-selected trainer (sft/dpo/kto/orpo/etc.).
/// * `adapter_purpose` - The adapter purpose (instruction/preference/reward_model/etc.).
///
/// # Returns
/// A `DatasetFormatResult` with the verdict, detected/expected formats,
/// mapping code (if applicable), and any findings.
pub(crate) fn validate_dataset_format(
    dataset_path: &std::path::Path,
    trainer_preference: Option<&str>,
    adapter_purpose: Option<&str>,
) -> DatasetFormatResult {
    let mut findings = Vec::new();

    let detected_format = DatasetFormat::detect(dataset_path);
    let expected_format = derive_expected_format(trainer_preference, adapter_purpose);

    match (&detected_format, &expected_format) {
        (None, Some(_)) => {
            // Detection failed but an expected format was derivable — the
            // dataset could not be matched against the expected format, so a
            // caller branching on `verdict` must not treat this as ready.
            findings.push(ValidationFinding {
                gate_id: "G-D0",
                severity: ValidationSeverity::Warn,
                message: format!(
                    "Could not detect dataset format from file: {} — format compatibility with the expected format could not be verified",
                    dataset_path.display()
                ),
                source: "hKask dataset pipeline — DatasetFormat::detect",
                remediation: "Ensure the dataset file has a .jsonl, .json, or .txt extension and is non-empty with a recognized schema".to_string(),
            });
            DatasetFormatResult {
                verdict: DatasetFormatVerdict::Undetermined,
                detected_format,
                expected_format,
                mapping_code: String::new(),
                findings,
            }
        }
        (None, None) => {
            // Detection failed and no expected format was derivable either —
            // nothing to compare against. Warn the operator but keep Ready
            // (the dataset may well be fine; the operator just didn't declare
            // a trainer and detection couldn't classify it).
            findings.push(ValidationFinding {
                gate_id: "G-D0",
                severity: ValidationSeverity::Warn,
                message: format!(
                    "Could not detect dataset format from file: {} — format detection requires .jsonl/.json/.txt extension",
                    dataset_path.display()
                ),
                source: "hKask dataset pipeline — DatasetFormat::detect",
                remediation: "Ensure the dataset file has a .jsonl, .json, or .txt extension".to_string(),
            });
            DatasetFormatResult {
                verdict: DatasetFormatVerdict::Ready,
                detected_format,
                expected_format,
                mapping_code: String::new(),
                findings,
            }
        }
        (Some(_detected), None) => {
            // No expected format derivable — cannot check compatibility.
            // This is not a failure; the operator may not have declared a
            // trainer. But the "not checked" state must be visible so it is
            // distinguishable from "validated and compatible."
            findings.push(ValidationFinding {
                gate_id: "G-D0",
                severity: ValidationSeverity::Info,
                message: "No trainer or adapter_purpose declared — dataset format compatibility not checked".to_string(),
                source: "hKask dataset pipeline — derive_expected_format",
                remediation: "Declare a trainer (sft/dpo/kto/orpo) or adapter_purpose (instruction/preference) to enable format compatibility checking".to_string(),
            });
            DatasetFormatResult {
                verdict: DatasetFormatVerdict::Ready,
                detected_format,
                expected_format,
                mapping_code: String::new(),
                findings,
            }
        }
        (Some(detected), Some(expected)) => {
            if detected == expected {
                // Exact match — ready.
                DatasetFormatResult {
                    verdict: DatasetFormatVerdict::Ready,
                    detected_format,
                    expected_format,
                    mapping_code: String::new(),
                    findings,
                }
            } else if is_format_compatible(detected, expected) {
                // Structurally compatible. SFT formats (ChatML, ShareGPT, Alpaca,
                // RawText) are auto-normalized by the dataset pipeline — no manual
                // mapping needed, so verdict is Ready. Preference formats need
                // manual column mapping, so verdict is NeedsMapping.
                let needs_manual_mapping = detected.is_preference() && expected.is_preference();
                if needs_manual_mapping {
                    let mapping_code = generate_mapping_code(detected, expected);
                    findings.push(ValidationFinding {
                        gate_id: "G-D0",
                        severity: ValidationSeverity::Warn,
                        message: format!(
                            "Dataset format {:?} needs mapping to {:?} for the selected method — mapping code provided",
                            detected, expected
                        ),
                        source: "HF dataset_inspector.py pattern — huggingface.co/datasets/mcp-tools/skills",
                        remediation: "Apply the mapping code below before training to avoid format-mismatch failures".to_string(),
                    });
                    DatasetFormatResult {
                        verdict: DatasetFormatVerdict::NeedsMapping,
                        detected_format,
                        expected_format,
                        mapping_code,
                        findings,
                    }
                } else {
                    // SFT format conversion — auto-normalized by the pipeline.
                    DatasetFormatResult {
                        verdict: DatasetFormatVerdict::Ready,
                        detected_format,
                        expected_format,
                        mapping_code: String::new(),
                        findings,
                    }
                }
            } else {
                // Incompatible — e.g., SFT data (ChatML) for DPO training.
                findings.push(ValidationFinding {
                    gate_id: "G-D0",
                    severity: ValidationSeverity::Refuse,
                    message: format!(
                        "Dataset format {:?} is incompatible with expected format {:?} — cannot use this dataset for the selected method",
                        detected, expected
                    ),
                    source: "TRL dataset formats — huggingface.co/docs/trl/main/en/dataset_formats",
                    remediation: format!(
                        "Use a {:?} dataset for the selected method, or change the trainer to match the {:?} dataset",
                        expected, detected
                    ),
                });
                DatasetFormatResult {
                    verdict: DatasetFormatVerdict::Incompatible,
                    detected_format,
                    expected_format,
                    mapping_code: String::new(),
                    findings,
                }
            }
        }
    }
}

/// Derive the expected dataset format from the trainer preference and adapter purpose.
///
/// Returns `None` when neither input is sufficient to determine the expected format.
fn derive_expected_format(
    trainer_preference: Option<&str>,
    adapter_purpose: Option<&str>,
) -> Option<DatasetFormat> {
    // Trainer preference takes precedence.
    if let Some(trainer) = trainer_preference {
        match trainer {
            "sft" | "undetermined" => return Some(DatasetFormat::ChatML),
            "dpo" => return Some(DatasetFormat::PreferenceDpo),
            "kto" => return Some(DatasetFormat::PreferenceKto),
            "orpo" => return Some(DatasetFormat::PreferenceOrpo),
            // Reward models consume preference pairs (chosen/rejected) —
            // ORPO format (prompt implicit in chosen/rejected) is the closest
            // match in the current DatasetFormat taxonomy.
            "reward" => return Some(DatasetFormat::PreferenceOrpo),
            // GRPO consumes preference data (prompt + chosen + rejected +
            // optionally per-token logprobs), but the current DatasetFormat
            // taxonomy has no GRPO-specific variant and GRPO is not a TRL
            // trainer supported by this pipeline (it is Ludwig-only and
            // deferred per TrlTrainer). Return None so G-D0 surfaces
            // "expected format not derivable" rather than silently mapping
            // to ChatML (GRPO does not consume ChatML).
            // TODO: verify against TRL GRPOTrainer's expected format when
            // GRPO support lands (P7 — evolutionary architecture).
            "grpo" => return None,
            _ => {}
        }
    }
    // Fall back to adapter purpose.
    if let Some(purpose) = adapter_purpose {
        match purpose {
            "instruction" | "reasoning" | "vision" | "reward_model" | "undetermined" => {
                return Some(DatasetFormat::ChatML);
            }
            "preference" => return Some(DatasetFormat::PreferenceDpo),
            _ => {}
        }
    }
    None
}

/// Check whether a detected format is structurally compatible with the expected
/// format (i.e., can be mapped via column renaming, not a fundamental mismatch).
fn is_format_compatible(detected: &DatasetFormat, expected: &DatasetFormat) -> bool {
    use DatasetFormat::*;
    // SFT formats are interchangeable via normalization (ChatML, ShareGPT, Alpaca, RawText).
    let sft_formats = [ChatML, ShareGPT, Alpaca, RawText];
    let detected_is_sft = sft_formats.contains(detected);
    let expected_is_sft = sft_formats.contains(expected);
    if detected_is_sft && expected_is_sft {
        return true;
    }
    // Preference formats are interchangeable via column mapping (DPO, KTO, ORPO).
    let preference_formats = [PreferenceDpo, PreferenceKto, PreferenceOrpo];
    let detected_is_preference = preference_formats.contains(detected);
    let expected_is_preference = preference_formats.contains(expected);
    if detected_is_preference && expected_is_preference {
        return true;
    }
    // SFT data cannot be used for preference training and vice versa.
    false
}

/// Generate copy-paste Python mapping code for a compatible format mismatch.
///
/// Mirrors the HF `dataset_inspector.py` "MAPPING CODE" section pattern.
fn generate_mapping_code(detected: &DatasetFormat, expected: &DatasetFormat) -> String {
    use DatasetFormat::*;
    match (detected, expected) {
        // SFT format conversions — the pipeline normalizes these, so no manual mapping needed.
        (ShareGPT, ChatML) | (Alpaca, ChatML) | (RawText, ChatML) => {
            "# The hKask dataset pipeline normalizes this format to ChatML automatically.\n# No manual mapping code needed — proceed with training.".to_string()
        }
        (ChatML, ShareGPT) | (Alpaca, ShareGPT) | (RawText, ShareGPT) => {
            "# The hKask dataset pipeline normalizes this format to ChatML automatically.\n# No manual mapping code needed — proceed with training.".to_string()
        }
        // Preference format conversions — column-name mapping.
        (PreferenceKto, PreferenceDpo) => {
            "# Map KTO format (prompt/completion/label) to DPO format (prompt/chosen/rejected):\n\ndef format_for_dpo(example):\n    return {\n        'prompt': example['prompt'],\n        'chosen': example['completion'] if example.get('label', True) else '',\n        'rejected': example['completion'] if not example.get('label', True) else '',\n    }\n\n# Apply before training:\n# dataset = dataset.map(format_for_dpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceOrpo, PreferenceDpo) => {
            "# Map ORPO format (chosen/rejected, no prompt) to DPO format (prompt/chosen/rejected):\n# ORPO's chosen/rejected typically contain the prompt implicitly.\n\ndef format_for_dpo(example):\n    # ORPO chosen/rejected are conversational; extract the prompt from the first turn.\n    chosen = example['chosen']\n    rejected = example['rejected']\n    # If chosen is a list of messages, extract prompt from the first user turn.\n    prompt = ''\n    if isinstance(chosen, list) and len(chosen) > 0:\n        prompt = chosen[0].get('content', '') if isinstance(chosen[0], dict) else str(chosen[0])\n    return {'prompt': prompt, 'chosen': chosen, 'rejected': rejected}\n\n# Apply before training:\n# dataset = dataset.map(format_for_dpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceDpo, PreferenceKto) => {
            "# Map DPO format (prompt/chosen/rejected) to KTO format (prompt/completion/label):\n\ndef format_for_kto(example):\n    return {\n        'prompt': example['prompt'],\n        'completion': example['chosen'],\n        'label': True,\n    }\n\n# Note: DPO data only provides positive examples. For KTO, you also need\n# negative examples. Consider adding rejected completions as label=False rows.\n# Apply before training:\n# dataset = dataset.map(format_for_kto, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceDpo, PreferenceOrpo) => {
            "# Map DPO format (prompt/chosen/rejected) to ORPO format (chosen/rejected):\n\ndef format_for_orpo(example):\n    return {\n        'chosen': example['chosen'],\n        'rejected': example['rejected'],\n    }\n\n# Apply before training:\n# dataset = dataset.map(format_for_orpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceKto, PreferenceOrpo) => {
            "# Map KTO format (prompt/completion/label) to ORPO format (chosen/rejected):\n# KTO is unpaired; ORPO is paired. This mapping loses information.\n# Consider collecting paired preference data instead.\n\ndef format_for_orpo(example):\n    completion = example['completion']\n    label = example.get('label', True)\n    return {\n        'chosen': completion if label else '',\n        'rejected': completion if not label else '',\n    }\n\n# Apply before training:\n# dataset = dataset.map(format_for_orpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceOrpo, PreferenceKto) => {
            "# Map ORPO format (chosen/rejected) to KTO format (prompt/completion/label):\n\ndef format_for_kto(example):\n    chosen = example['chosen']\n    rejected = example['rejected']\n    # Extract prompt from chosen (first user turn if conversational).\n    prompt = ''\n    if isinstance(chosen, list) and len(chosen) > 0:\n        prompt = chosen[0].get('content', '') if isinstance(chosen[0], dict) else str(chosen[0])\n    return [\n        {'prompt': prompt, 'completion': chosen, 'label': True},\n        {'prompt': prompt, 'completion': rejected, 'label': False},\n    ]\n\n# Apply before training (flatten the resulting list):\n# dataset = dataset.map(format_for_kto, remove_columns=dataset.column_names)\n# dataset = datasets.concatenate_datasets([dataset])  # flatten if needed".to_string()
        }
        _ => {
            format!(
                "# No automatic mapping code available for {:?} → {:?}.\n# Inspect the dataset columns and write a custom mapping function.",
                detected, expected
            )
        }
    }
}
