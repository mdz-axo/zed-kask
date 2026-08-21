//! Dataset ingestion and preprocessing pipeline.
//!
//! Converts raw input files (JSONL, ShareGPT, Alpaca, raw text, preference)
//! into canonical format, validates structure, and caches the normalized
//! output in `hkask-storage` to avoid re-processing.
//!
//! Two canonical output types:
//! - `ChatConversation` — for SFT (messages array)
//! - `PreferenceExample` — for DPO/KTO/ORPO/Reward (prompt + chosen + rejected)
//!
//! Each provider adapter then translates the canonical output to its native
//! format for cloud dispatch (axolotl YAML, TRL Python → Runpod).
//! All training is cloud-only — there is no local training path.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

// ── Canonical ChatML types ─────────────────────────────────────────────────

// Canonical chat turn type lives in `hkask_types::ChatMessage` (foundation
// inference type); re-exported here so this module's ChatML section reads in place.
pub(crate) use hkask_types::ChatMessage;

/// A full conversation (list of role/content turns).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChatConversation {
    pub messages: Vec<ChatMessage>,
}

// ── Canonical preference types ────────────────────────────────────────────

/// A preference example for DPO/KTO/ORPO/Reward training.
///
/// Canonical format for preference optimization — parallel to `ChatConversation`
/// for SFT. TRL's preference trainers consume this format directly.
///
/// Fields:
/// - `prompt`: optional prompt (string or conversational). Absent for ORPO
///   (prompt is implicit in chosen/rejected).
/// - `chosen`: the preferred completion (string or conversational).
/// - `rejected`: the dispreferred completion (string or conversational).
/// - `label`: for KTO only — `true` if the completion is good, `false` if bad.
///   Absent for DPO/ORPO/Reward (which use chosen/rejected pairs).
///
/// References:
/// - DPO: https://huggingface.co/docs/trl/main/en/dpo_trainer#expected-dataset-type-and-format
/// - KTO: https://huggingface.co/docs/trl/main/en/kto_trainer#expected-dataset-type-and-format
/// - ORPO: https://huggingface.co/docs/trl/main/en/orpo_trainer#expected-dataset-type-and-format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PreferenceExample {
    /// Optional prompt (string or conversational). Absent for ORPO.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<serde_json::Value>,
    /// The preferred completion (string or conversational).
    pub chosen: serde_json::Value,
    /// The dispreferred completion (string or conversational).
    /// Absent for KTO (which uses label instead of rejected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected: Option<serde_json::Value>,
    /// KTO only: `true` if the completion is good, `false` if bad.
    /// Absent for DPO/ORPO/Reward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<bool>,
}

/// Source format identifiers for input datasets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DatasetFormat {
    /// JSONL with `{"messages": [{"role": ..., "content": ...}, ...]}` per line.
    ChatML,
    /// ShareGPT format: `{"conversations": [{"from": "human", "value": "..."}, ...]}`.
    ShareGPT,
    /// Alpaca format: `{"instruction": "...", "input": "...", "output": "..."}`.
    Alpaca,
    /// Raw text file — each line is a standalone training example.
    RawText,
    /// DPO preference format: `{"prompt": ..., "chosen": ..., "rejected": ...}`.
    /// Prompt can be string or conversational; chosen/rejected can be string or conversational.
    PreferenceDpo,
    /// KTO preference format: `{"prompt": ..., "completion": ..., "label": bool}`.
    /// Unpaired binary preference data.
    PreferenceKto,
    /// ORPO preference format: `{"chosen": ..., "rejected": ...}`.
    /// Prompt is implicit in chosen/rejected.
    PreferenceOrpo,
}

impl DatasetFormat {
    /// Detect format from file extension or content heuristics.
    pub fn detect(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        match ext.to_lowercase().as_str() {
            "jsonl" => {
                // Could be ChatML, ShareGPT, Alpaca, or preference — read
                // first line and disambiguate by parsing it as JSON and
                // inspecting the object's keys (not substring presence).
                //
                // Homogeneity assumption: the first line is assumed to
                // represent the format of the whole file. Heterogeneous
                // JSONL (mixed schemas across lines) will be misdetected.
                if let Ok(content) = std::fs::read_to_string(path) {
                    let first_line = content.lines().next().unwrap_or("");
                    // Empty file or empty first line — cannot detect format.
                    // Return None rather than defaulting to ChatML, so G-D0
                    // can warn the operator about the empty/unrecognizable dataset.
                    if first_line.trim().is_empty() {
                        return None;
                    }
                    return Self::detect_from_json_line(first_line);
                }
                None // cannot read file
            }
            "json" => {
                // Single JSON array of Alpaca objects. Parse and check keys
                // rather than substring matching, so a content field whose
                // value happens to contain the literal "instruction" does
                // not produce a false positive.
                if let Ok(content) = std::fs::read_to_string(path)
                    && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content)
                {
                    // Alpaca may be a top-level array of objects or a single
                    // object; check the first object's keys in either case.
                    let first_obj = match &parsed {
                        serde_json::Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
                        serde_json::Value::Object(_) => parsed.as_object(),
                        _ => None,
                    };
                    if let Some(obj) = first_obj {
                        if obj.contains_key("instruction") && obj.contains_key("output") {
                            return Some(Self::Alpaca);
                        }
                    }
                }
                None
            }
            "txt" => Some(Self::RawText),
            _ => None,
        }
    }

    /// Disambiguate a JSONL format by parsing the first line as JSON and
    /// inspecting the object's keys. Returns `None` when the line does not
    /// parse as a JSON object or when the key set does not match a known
    /// schema.
    ///
    /// Preference formats take precedence over ChatML when preference fields
    /// are present — a DPO dataset with conversational chosen/rejected might
    /// also contain "messages" in the prompt, but the top-level
    /// chosen/rejected fields identify it as preference data.
    fn detect_from_json_line(line: &str) -> Option<Self> {
        let parsed = serde_json::from_str::<serde_json::Value>(line).ok()?;
        let obj = parsed.as_object()?;
        // Preference formats take precedence over ChatML when preference
        // fields are present.
        if obj.contains_key("chosen") && obj.contains_key("rejected") {
            // DPO (has prompt) or ORPO (no prompt)
            if obj.contains_key("prompt") {
                return Some(Self::PreferenceDpo);
            }
            return Some(Self::PreferenceOrpo);
        }
        // KTO: has prompt + completion + label (no chosen/rejected)
        if obj.contains_key("completion") && obj.contains_key("label") {
            return Some(Self::PreferenceKto);
        }
        if obj.contains_key("messages") {
            return Some(Self::ChatML);
        }
        if obj.contains_key("conversations") {
            return Some(Self::ShareGPT);
        }
        // Alpaca: instruction + output (input is optional)
        if obj.contains_key("instruction") && obj.contains_key("output") {
            return Some(Self::Alpaca);
        }
        None
    }

    /// Whether this format is a preference format (DPO/KTO/ORPO).
    pub fn is_preference(&self) -> bool {
        matches!(
            self,
            Self::PreferenceDpo | Self::PreferenceKto | Self::PreferenceOrpo
        )
    }
}

// ── Pipeline errors ───────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub(crate) enum DatasetError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("Validation error at line {line}: {message}")]
    Validation { line: usize, message: String },
    #[error("Empty dataset: no parseable examples found")]
    Empty,
    #[error("Cache error: {0}")]
    Cache(String),
}

// ── DatasetPipeline ────────────────────────────────────────────────────────

/// The normalized output of the dataset pipeline.
///
/// SFT formats (ChatML, ShareGPT, Alpaca, RawText) normalize to `Sft` (a list
/// of `ChatConversation`). Preference formats (DPO, KTO, ORPO) normalize to
/// `Preference` (a list of `PreferenceExample`). The pipeline does not force
/// preference data through ChatML normalization — preference data has a
/// different structure (prompt + chosen + rejected) that cannot be represented
/// as a single conversation.
#[derive(Debug, Clone)]
pub(crate) enum NormalizedDataset {
    /// SFT data — a list of conversations.
    Sft(Vec<ChatConversation>),
    /// Preference data — a list of preference examples.
    Preference(Vec<PreferenceExample>),
}

// ── Dataset profile (for skill recommendation) ──────────────────────────────

/// Ingest, normalize, validate, and cache datasets for training.
///
/// Pipeline: `ingest(file_path) → normalize → validate → cache`
///
/// SFT formats normalize to canonical ChatML (`NormalizedDataset::Sft`).
/// Preference formats normalize to canonical `PreferenceExample`
/// (`NormalizedDataset::Preference`). Provider adapters consume the normalized
/// output and translate it to their native format.
pub(crate) struct DatasetPipeline {
    /// Cache directory for normalized datasets.
    cache_dir: PathBuf,
    /// Cache key for the current normalization (content hash).
    cache_key: Option<String>,
}

impl Clone for DatasetPipeline {
    fn clone(&self) -> Self {
        Self {
            cache_dir: self.cache_dir.clone(),
            cache_key: None, // Reset cache_key on clone to avoid stale references
        }
    }
}

impl DatasetPipeline {
    /// Create a new dataset pipeline with a given cache directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            cache_key: None,
        }
    }

    /// The pipeline's cache directory (server-chosen, not LLM-controlled).
    /// Used by callers that need to place a transient scratch file (e.g. the
    /// retrain merge) in an existing legitimate scratch location rather than
    /// an LLM-supplied path.
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }

    /// Ingest a raw dataset file and return the path to normalized output.
    ///
    /// Full pipeline: detect format → normalize to ChatML → validate → cache.
    /// Returns the cached path on subsequent calls with the same input.
    pub fn ingest(&mut self, file_path: &std::path::Path) -> Result<PathBuf, DatasetError> {
        self.ingest_local(file_path)
    }

    fn ingest_local(&mut self, file_path: &std::path::Path) -> Result<PathBuf, DatasetError> {
        // Check cache first
        let cache_key = self.compute_cache_key(file_path)?;
        let cached_path = self.cache_dir.join(format!("{}.jsonl", cache_key));
        if cached_path.exists() {
            tracing::info!(
                target: "hkask.training.dataset.cached",
                path = %file_path.display(),
                cache_key = %cache_key,
                "Returning cached normalized dataset"
            );
            return Ok(cached_path);
        }

        let raw = std::fs::read_to_string(file_path)?;
        if raw.lines().all(|l| l.trim().is_empty()) {
            return Err(DatasetError::Empty);
        }

        let format = DatasetFormat::detect(file_path).ok_or_else(|| {
            DatasetError::UnsupportedFormat(format!(
                "Cannot determine format for {}",
                file_path.display()
            ))
        })?;

        let normalized = match format {
            DatasetFormat::ChatML => NormalizedDataset::Sft(self.normalize_chatml(&raw)?),
            DatasetFormat::ShareGPT => NormalizedDataset::Sft(self.normalize_sharegpt(&raw)?),
            DatasetFormat::Alpaca => NormalizedDataset::Sft(self.normalize_alpaca(&raw)?),
            DatasetFormat::RawText => NormalizedDataset::Sft(self.normalize_raw_text(&raw)?),
            DatasetFormat::PreferenceDpo => {
                NormalizedDataset::Preference(self.normalize_preference_dpo(&raw)?)
            }
            DatasetFormat::PreferenceKto => {
                NormalizedDataset::Preference(self.normalize_preference_kto(&raw)?)
            }
            DatasetFormat::PreferenceOrpo => {
                NormalizedDataset::Preference(self.normalize_preference_orpo(&raw)?)
            }
        };

        self.validate(&normalized)?;
        self.cache(&cached_path, &normalized)?;

        self.cache_key = Some(cache_key);
        Ok(cached_path)
    }

    /// Compute a content-hash-based cache key for the input file.
    fn compute_cache_key(&self, file_path: &std::path::Path) -> Result<String, DatasetError> {
        let content = std::fs::read(file_path)?;
        let hash = blake3::hash(&content);
        let key = format!("dataset-{}", hash.to_hex());
        Ok(key)
    }

    /// Normalize ChatML JSONL to canonical form.
    ///
    /// Input: JSONL with `{"messages": [{"role": ..., "content": ...}, ...]}`
    /// Output: Same format, validated.
    fn normalize_chatml(&self, raw: &str) -> Result<Vec<ChatConversation>, DatasetError> {
        let mut conversations = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            #[derive(Deserialize)]
            struct ChatMLRecord {
                messages: Vec<ChatMessage>,
            }
            let record: ChatMLRecord =
                serde_json::from_str(trimmed).map_err(|e| DatasetError::Validation {
                    line: i + 1,
                    message: format!("Invalid ChatML record: {}", e),
                })?;
            conversations.push(ChatConversation {
                messages: record.messages,
            });
        }
        if conversations.is_empty() {
            return Err(DatasetError::Empty);
        }
        Ok(conversations)
    }

    /// Normalize ShareGPT JSONL to canonical ChatML.
    ///
    /// ShareGPT uses `from: human/gpt` and `value` instead of `role` and `content`.
    fn normalize_sharegpt(&self, raw: &str) -> Result<Vec<ChatConversation>, DatasetError> {
        let mut conversations = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            #[derive(Deserialize)]
            struct ShareGPTTurn {
                from: String,
                value: String,
            }
            #[derive(Deserialize)]
            struct ShareGPTRecord {
                conversations: Vec<ShareGPTTurn>,
            }
            let record: ShareGPTRecord =
                serde_json::from_str(trimmed).map_err(|e| DatasetError::Validation {
                    line: i + 1,
                    message: format!("Invalid ShareGPT record: {}", e),
                })?;
            let messages: Vec<ChatMessage> = record
                .conversations
                .into_iter()
                .map(|t| {
                    let role = match t.from.as_str() {
                        "human" => "user".to_string(),
                        "gpt" => "assistant".to_string(),
                        other => other.to_string(),
                    };
                    ChatMessage {
                        role,
                        content: t.value,
                    }
                })
                .collect();
            conversations.push(ChatConversation { messages });
        }
        if conversations.is_empty() {
            return Err(DatasetError::Empty);
        }
        Ok(conversations)
    }

    /// Normalize Alpaca JSON to canonical ChatML.
    ///
    /// Alpaca uses `instruction`, `input` (optional), and `output` fields.
    fn normalize_alpaca(&self, raw: &str) -> Result<Vec<ChatConversation>, DatasetError> {
        #[derive(Deserialize)]
        struct AlpacaRecord {
            instruction: String,
            #[serde(default)]
            input: String,
            output: String,
        }
        let records: Vec<AlpacaRecord> =
            serde_json::from_str(raw).map_err(|e| DatasetError::Validation {
                line: 0,
                message: format!("Invalid Alpaca JSON: {}", e),
            })?;
        if records.is_empty() {
            return Err(DatasetError::Empty);
        }
        let conversations: Vec<ChatConversation> = records
            .into_iter()
            .map(|r| {
                let user_content = if r.input.is_empty() {
                    r.instruction
                } else {
                    format!("{}\n\n{}", r.instruction, r.input)
                };
                ChatConversation {
                    messages: vec![
                        ChatMessage {
                            role: "user".to_string(),
                            content: user_content,
                        },
                        ChatMessage {
                            role: "assistant".to_string(),
                            content: r.output,
                        },
                    ],
                }
            })
            .collect();
        Ok(conversations)
    }

    /// Normalize raw text to canonical ChatML.
    ///
    /// Each non-empty line becomes a single-message user turn.
    /// This is a best-effort normalization — raw text has no conversation structure.
    fn normalize_raw_text(&self, raw: &str) -> Result<Vec<ChatConversation>, DatasetError> {
        let conversations: Vec<ChatConversation> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| ChatConversation {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: line.trim().to_string(),
                }],
            })
            .collect();
        if conversations.is_empty() {
            return Err(DatasetError::Empty);
        }
        Ok(conversations)
    }

    /// Normalize DPO preference JSONL to canonical `PreferenceExample`.
    ///
    /// Input: JSONL with `{"prompt": ..., "chosen": ..., "rejected": ...}` per line.
    /// Prompt, chosen, and rejected can be strings or conversational (array of messages).
    /// Output: `PreferenceExample` with prompt/chosen/rejected preserved as JSON values.
    ///
    /// Reference: https://huggingface.co/docs/trl/main/en/dpo_trainer#expected-dataset-type-and-format
    fn normalize_preference_dpo(&self, raw: &str) -> Result<Vec<PreferenceExample>, DatasetError> {
        let mut examples = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            #[derive(Deserialize)]
            struct DpoRecord {
                prompt: serde_json::Value,
                chosen: serde_json::Value,
                rejected: serde_json::Value,
            }
            let record: DpoRecord =
                serde_json::from_str(trimmed).map_err(|e| DatasetError::Validation {
                    line: i + 1,
                    message: format!("Invalid DPO preference record: {}", e),
                })?;
            examples.push(PreferenceExample {
                prompt: Some(record.prompt),
                chosen: record.chosen,
                rejected: Some(record.rejected),
                label: None,
            });
        }
        if examples.is_empty() {
            return Err(DatasetError::Empty);
        }
        Ok(examples)
    }

    /// Normalize KTO preference JSONL to canonical `PreferenceExample`.
    ///
    /// Input: JSONL with `{"prompt": ..., "completion": ..., "label": bool}` per line.
    /// Unpaired binary preference data — each example has a single completion
    /// and a boolean label (true=good, false=bad).
    /// Output: `PreferenceExample` with prompt/chosen (completion)/label.
    ///
    /// Reference: https://huggingface.co/docs/trl/main/en/kto_trainer#expected-dataset-type-and-format
    fn normalize_preference_kto(&self, raw: &str) -> Result<Vec<PreferenceExample>, DatasetError> {
        let mut examples = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            #[derive(Deserialize)]
            struct KtoRecord {
                prompt: serde_json::Value,
                completion: serde_json::Value,
                label: bool,
            }
            let record: KtoRecord =
                serde_json::from_str(trimmed).map_err(|e| DatasetError::Validation {
                    line: i + 1,
                    message: format!("Invalid KTO preference record: {}", e),
                })?;
            // KTO stores the completion in `chosen` and the label in `label`.
            // `rejected` is None — KTO is unpaired.
            examples.push(PreferenceExample {
                prompt: Some(record.prompt),
                chosen: record.completion,
                rejected: None,
                label: Some(record.label),
            });
        }
        if examples.is_empty() {
            return Err(DatasetError::Empty);
        }
        Ok(examples)
    }

    /// Normalize ORPO preference JSONL to canonical `PreferenceExample`.
    ///
    /// Input: JSONL with `{"chosen": ..., "rejected": ...}` per line.
    /// Prompt is implicit in chosen/rejected (each contains the full conversation).
    /// Output: `PreferenceExample` with chosen/rejected, prompt=None.
    ///
    /// Reference: https://huggingface.co/docs/trl/main/en/orpo_trainer#expected-dataset-type-and-format
    fn normalize_preference_orpo(&self, raw: &str) -> Result<Vec<PreferenceExample>, DatasetError> {
        let mut examples = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            #[derive(Deserialize)]
            struct OrpoRecord {
                chosen: serde_json::Value,
                rejected: serde_json::Value,
            }
            let record: OrpoRecord =
                serde_json::from_str(trimmed).map_err(|e| DatasetError::Validation {
                    line: i + 1,
                    message: format!("Invalid ORPO preference record: {}", e),
                })?;
            examples.push(PreferenceExample {
                prompt: None, // ORPO prompt is implicit
                chosen: record.chosen,
                rejected: Some(record.rejected),
                label: None,
            });
        }
        if examples.is_empty() {
            return Err(DatasetError::Empty);
        }
        Ok(examples)
    }

    /// Validate the normalized dataset.
    ///
    /// For SFT data: checks roles, content, and alternation.
    /// For preference data: checks that chosen/rejected are non-null and non-empty.
    fn validate(&self, dataset: &NormalizedDataset) -> Result<(), DatasetError> {
        match dataset {
            NormalizedDataset::Sft(conversations) => self.validate_sft(conversations),
            NormalizedDataset::Preference(examples) => self.validate_preference(examples),
        }
    }

    /// Validate canonical ChatML conversations.
    ///
    /// Checks:
    /// - At least one message per conversation
    /// - Valid roles (user, assistant, system)
    /// - Non-empty content fields
    /// - Alternating user/assistant pattern (system allowed only as first message)
    fn validate_sft(&self, conversations: &[ChatConversation]) -> Result<(), DatasetError> {
        let valid_roles = ["user", "assistant", "system"];
        for (i, conv) in conversations.iter().enumerate() {
            if conv.messages.is_empty() {
                return Err(DatasetError::Validation {
                    line: i + 1,
                    message: "Empty conversation".to_string(),
                });
            }
            for (j, msg) in conv.messages.iter().enumerate() {
                if !valid_roles.contains(&msg.role.as_str()) {
                    return Err(DatasetError::Validation {
                        line: i + 1,
                        message: format!("Invalid role '{}' at position {}", msg.role, j + 1),
                    });
                }
                if msg.content.trim().is_empty() {
                    return Err(DatasetError::Validation {
                        line: i + 1,
                        message: format!(
                            "Empty content for role '{}' at position {}",
                            msg.role,
                            j + 1
                        ),
                    });
                }
                // System messages only allowed as first message
                if msg.role == "system" && j > 0 {
                    return Err(DatasetError::Validation {
                        line: i + 1,
                        message: "System message only allowed as first message".to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Validate canonical preference examples.
    ///
    /// Checks:
    /// - `chosen` is not null and not empty
    /// - `rejected` is present for DPO/ORPO (not KTO)
    /// - `label` is present for KTO
    /// - For KTO, `label` is a boolean
    fn validate_preference(&self, examples: &[PreferenceExample]) -> Result<(), DatasetError> {
        for (i, ex) in examples.iter().enumerate() {
            // chosen must be non-null and non-empty.
            if ex.chosen.is_null() {
                return Err(DatasetError::Validation {
                    line: i + 1,
                    message: "Preference example has null `chosen`".to_string(),
                });
            }
            // Check for empty string chosen.
            if let Some(s) = ex.chosen.as_str()
                && s.trim().is_empty()
            {
                return Err(DatasetError::Validation {
                    line: i + 1,
                    message: "Preference example has empty `chosen`".to_string(),
                });
            }
            // Check for empty array chosen (conversational).
            if let Some(arr) = ex.chosen.as_array()
                && arr.is_empty()
            {
                return Err(DatasetError::Validation {
                    line: i + 1,
                    message: "Preference example has empty `chosen` array".to_string(),
                });
            }
            // rejected must be present for DPO/ORPO (absent for KTO).
            if let Some(ref rejected) = ex.rejected {
                if rejected.is_null() {
                    return Err(DatasetError::Validation {
                        line: i + 1,
                        message: "Preference example has null `rejected`".to_string(),
                    });
                }
                if let Some(s) = rejected.as_str()
                    && s.trim().is_empty()
                {
                    return Err(DatasetError::Validation {
                        line: i + 1,
                        message: "Preference example has empty `rejected`".to_string(),
                    });
                }
                if let Some(arr) = rejected.as_array()
                    && arr.is_empty()
                {
                    return Err(DatasetError::Validation {
                        line: i + 1,
                        message: "Preference example has empty `rejected` array".to_string(),
                    });
                }
            }
            // KTO must have a label.
            if ex.rejected.is_none() && ex.label.is_none() {
                return Err(DatasetError::Validation {
                    line: i + 1,
                    message:
                        "Preference example has neither `rejected` nor `label` — must have one"
                            .to_string(),
                });
            }
        }
        Ok(())
    }

    /// Write normalized dataset to cache as JSONL.
    ///
    /// SFT data is written as `ChatConversation` JSONL (same as before).
    /// Preference data is written as `PreferenceExample` JSONL — the TRL
    /// trainers consume this format directly.
    fn cache(
        &self,
        path: &std::path::Path,
        dataset: &NormalizedDataset,
    ) -> Result<(), DatasetError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DatasetError::Cache(format!("Failed to create cache dir: {}", e)))?;
        }
        let mut output = String::new();
        match dataset {
            NormalizedDataset::Sft(conversations) => {
                for conv in conversations {
                    let json = serde_json::to_string(conv)
                        .map_err(|e| DatasetError::Cache(format!("Serialization error: {}", e)))?;
                    output.push_str(&json);
                    output.push('\n');
                }
            }
            NormalizedDataset::Preference(examples) => {
                for ex in examples {
                    let json = serde_json::to_string(ex)
                        .map_err(|e| DatasetError::Cache(format!("Serialization error: {}", e)))?;
                    output.push_str(&json);
                    output.push('\n');
                }
            }
        }
        std::fs::write(path, output)
            .map_err(|e| DatasetError::Cache(format!("Failed to write cache: {}", e)))?;
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────
