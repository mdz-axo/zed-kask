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

/// A single conversation turn in canonical ChatML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// A full conversation (list of role/content turns).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConversation {
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
pub struct PreferenceExample {
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
pub enum DatasetFormat {
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
                // Could be ChatML, ShareGPT, or preference — read first line to disambiguate.
                if let Ok(content) = std::fs::read_to_string(path) {
                    let first_line = content.lines().next().unwrap_or("");
                    // Preference formats take precedence over ChatML when preference
                    // fields are present — a DPO dataset with conversational chosen/rejected
                    // might also contain "messages" in the prompt, but the top-level
                    // chosen/rejected fields identify it as preference data.
                    if first_line.contains("\"chosen\"") && first_line.contains("\"rejected\"") {
                        // DPO (has prompt) or ORPO (no prompt)
                        if first_line.contains("\"prompt\"") {
                            return Some(Self::PreferenceDpo);
                        }
                        return Some(Self::PreferenceOrpo);
                    }
                    // KTO: has prompt + completion + label (no chosen/rejected)
                    if first_line.contains("\"completion\"") && first_line.contains("\"label\"") {
                        return Some(Self::PreferenceKto);
                    }
                    if first_line.contains("\"messages\"") {
                        return Some(Self::ChatML);
                    }
                    if first_line.contains("\"conversations\"") {
                        return Some(Self::ShareGPT);
                    }
                }
                Some(Self::ChatML) // default for .jsonl
            }
            "json" => {
                // Single JSON array of Alpaca objects.
                if let Ok(content) = std::fs::read_to_string(path)
                    && content.contains("\"instruction\"")
                    && content.contains("\"output\"")
                {
                    return Some(Self::Alpaca);
                }
                None
            }
            "txt" => Some(Self::RawText),
            _ => None,
        }
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
pub enum DatasetError {
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
pub enum NormalizedDataset {
    /// SFT data — a list of conversations.
    Sft(Vec<ChatConversation>),
    /// Preference data — a list of preference examples.
    Preference(Vec<PreferenceExample>),
}

impl NormalizedDataset {
    /// Number of examples in the dataset.
    pub fn len(&self) -> usize {
        match self {
            Self::Sft(conv) => conv.len(),
            Self::Preference(examples) => examples.len(),
        }
    }

    /// Whether the dataset is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether this is a preference dataset.
    pub fn is_preference(&self) -> bool {
        matches!(self, Self::Preference(_))
    }
}

// ── Dataset profile (for skill recommendation) ──────────────────────────────

/// A statistical profile of a dataset, derived by probing the actual file.
///
/// This is the output of `DatasetPipeline::profile()` — a read-only analysis
/// that characterizes the dataset's structure, size, and quality signals
/// without modifying it. The lora-training skill's G-D0 gate consumes this
/// profile to customize its recommendation for the dataset's characteristics.
///
/// All fields are `Option` because the profile is best-effort — if the file
/// can't be read or parsed, the fields remain `None` and the skill falls back
/// to its declared-input reasoning.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatasetProfile {
    /// Detected format (ChatML, ShareGPT, Alpaca, RawText, PreferenceDpo, etc.).
    pub format: Option<DatasetFormat>,
    /// Number of examples (non-empty lines for JSONL, array length for JSON).
    pub n_samples: Option<usize>,
    /// Average content length in characters across all examples.
    /// For SFT: average total message content length per conversation.
    /// For preference: average (chosen + rejected) length per example.
    pub avg_content_chars: Option<f64>,
    /// Maximum content length in characters across all examples.
    pub max_content_chars: Option<usize>,
    /// Estimated average token count (chars / 4 heuristic).
    pub avg_token_estimate: Option<f64>,
    /// Estimated maximum token count.
    pub max_token_estimate: Option<usize>,
    /// For SFT: average number of messages per conversation.
    /// For preference: always 1 (each example is one preference pair).
    pub avg_messages_per_example: Option<f64>,
    /// For SFT: distribution of roles (e.g., {"user": 0.4, "assistant": 0.5, "system": 0.1}).
    /// For preference: None.
    pub role_distribution: Option<serde_json::Value>,
    /// For preference data: average length ratio of chosen vs rejected.
    /// Values near 1.0 indicate balanced preference pairs; values far from 1.0
    /// may indicate length-biased preference data.
    pub chosen_rejected_length_ratio: Option<f64>,
    /// Whether the dataset contains system messages (SFT only).
    pub has_system_messages: Option<bool>,
    /// Whether the dataset contains multi-turn conversations (SFT only, >2 messages).
    pub has_multi_turn: Option<bool>,
    /// Whether the dataset appears to contain vision/image data (heuristic: presence
    /// of "image" or "images" keys in the JSON).
    pub has_vision_data: Option<bool>,
}

/// Ingest, normalize, validate, and cache datasets for training.
///
/// Pipeline: `ingest(file_path) → normalize → validate → cache`
///
/// SFT formats normalize to canonical ChatML (`NormalizedDataset::Sft`).
/// Preference formats normalize to canonical `PreferenceExample`
/// (`NormalizedDataset::Preference`). Provider adapters consume the normalized
/// output and translate it to their native format.
pub struct DatasetPipeline {
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

    /// Ingest a raw dataset file and return the path to normalized output.
    ///
    /// Full pipeline: detect format → normalize to ChatML → validate → cache.
    /// Returns the cached path on subsequent calls with the same input.
    pub fn ingest(&mut self, file_path: &std::path::Path) -> Result<PathBuf, DatasetError> {
        self.ingest_local(file_path)
    }

    /// Profile a dataset file — read-only statistical analysis.
    ///
    /// Probes the actual dataset file to derive characteristics that feed into
    /// the lora-training skill's recommendation. This is a read-only operation:
    /// it does not normalize, validate, or cache. It reads the raw file and
    /// computes statistics.
    ///
    /// The profile is best-effort: if the file can't be read or parsed, fields
    /// remain `None` and the caller falls back to declared-input reasoning.
    ///
    /// This method is called by the `training_validate_config` MCP tool when a
    /// `dataset_path` is provided, and the resulting `DatasetProfile` is
    /// included in the tool's response for the skill to consume.
    pub fn profile(file_path: &std::path::Path) -> DatasetProfile {
        // Detect format first (avoids field_reassign_with_default clippy lint).
        let format = DatasetFormat::detect(file_path);

        // Read the raw file content.
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => {
                return DatasetProfile {
                    format,
                    ..Default::default()
                };
            }
        };

        // Count non-empty lines (each line is one example in JSONL).
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        let n_samples = lines.len();

        // Heuristic: check for vision data (presence of "image" or "images" keys).
        let has_vision_data = content.contains("\"image\"") || content.contains("\"images\"");

        // Parse each line as JSON and compute statistics.
        let mut total_chars: usize = 0;
        let mut max_chars: usize = 0;
        let mut total_messages: usize = 0;
        let mut role_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut has_system = false;
        let mut has_multi_turn = false;
        let mut chosen_rejected_ratios: Vec<f64> = Vec::new();

        for line in &lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Try to parse as JSON.
            let json: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue, // Skip unparseable lines.
            };

            // SFT: {"messages": [{"role": ..., "content": ...}, ...]}
            if let Some(messages) = json.get("messages").and_then(|v| v.as_array()) {
                let msg_count = messages.len();
                total_messages += msg_count;
                if msg_count > 2 {
                    has_multi_turn = true;
                }
                let mut line_chars = 0;
                for msg in messages {
                    if let Some(role) = msg.get("role").and_then(|v| v.as_str()) {
                        *role_counts.entry(role.to_string()).or_insert(0) += 1;
                        if role == "system" {
                            has_system = true;
                        }
                    }
                    if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                        line_chars += content.len();
                    }
                }
                total_chars += line_chars;
                if line_chars > max_chars {
                    max_chars = line_chars;
                }
            }

            // Preference DPO: {"prompt": ..., "chosen": ..., "rejected": ...}
            // Preference ORPO: {"chosen": ..., "rejected": ...}
            if let (Some(chosen), Some(rejected)) = (json.get("chosen"), json.get("rejected")) {
                let chosen_len = Self::json_content_len(chosen);
                let rejected_len = Self::json_content_len(rejected);
                total_chars += chosen_len + rejected_len;
                if chosen_len + rejected_len > max_chars {
                    max_chars = chosen_len + rejected_len;
                }
                if rejected_len > 0 {
                    chosen_rejected_ratios.push(chosen_len as f64 / rejected_len as f64);
                }
            }

            // Preference KTO: {"prompt": ..., "completion": ..., "label": bool}
            if let Some(completion) = json.get("completion") {
                let comp_len = Self::json_content_len(completion);
                total_chars += comp_len;
                if comp_len > max_chars {
                    max_chars = comp_len;
                }
            }
        }

        let n = n_samples.max(1);
        let avg_content_chars = total_chars as f64 / n as f64;
        let avg_token_estimate = avg_content_chars / 4.0;
        let max_token_estimate = max_chars / 4;

        // SFT-specific stats.
        let (avg_messages_per_example, has_system_messages, has_multi_turn_sft, role_distribution) =
            if total_messages > 0 {
                let avg_msgs = total_messages as f64 / n as f64;
                let total_roles: usize = role_counts.values().sum();
                let role_dist = if total_roles > 0 {
                    let dist: serde_json::Map<String, serde_json::Value> = role_counts
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                serde_json::json!((*v as f64) / (total_roles as f64)),
                            )
                        })
                        .collect();
                    Some(serde_json::Value::Object(dist))
                } else {
                    None
                };
                (
                    Some(avg_msgs),
                    Some(has_system),
                    Some(has_multi_turn),
                    role_dist,
                )
            } else {
                (None, None, None, None)
            };

        // Preference-specific stats.
        let chosen_rejected_length_ratio = if !chosen_rejected_ratios.is_empty() {
            let avg_ratio =
                chosen_rejected_ratios.iter().sum::<f64>() / chosen_rejected_ratios.len() as f64;
            Some(avg_ratio)
        } else {
            None
        };

        DatasetProfile {
            format,
            n_samples: Some(n_samples),
            avg_content_chars: Some(avg_content_chars),
            max_content_chars: Some(max_chars),
            avg_token_estimate: Some(avg_token_estimate),
            max_token_estimate: Some(max_token_estimate),
            avg_messages_per_example,
            role_distribution,
            chosen_rejected_length_ratio,
            has_system_messages,
            has_multi_turn: has_multi_turn_sft,
            has_vision_data: Some(has_vision_data),
        }
    }

    /// Compute the content length of a JSON value (string or array of messages).
    fn json_content_len(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::String(s) => s.len(),
            serde_json::Value::Array(arr) => arr
                .iter()
                .map(|msg| {
                    msg.get("content")
                        .and_then(|c| c.as_str())
                        .map(|s| s.len())
                        .unwrap_or(0)
                })
                .sum(),
            _ => 0,
        }
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

        let format = DatasetFormat::detect(file_path).ok_or_else(|| {
            DatasetError::UnsupportedFormat(format!(
                "Cannot determine format for {}",
                file_path.display()
            ))
        })?;

        let raw = std::fs::read_to_string(file_path)?;
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

// ── Provider-specific format converters ────────────────────────────────────

/// Convert canonical ChatML JSONL to axolotl-compatible ChatML format.
///
/// Axolotl expects ChatML with explicit `type: chatml` in config.
/// The path returned is the cached normalized file — the provider's
/// config YAML references it directly.
pub fn to_axolotl_format(normalized_path: &std::path::Path) -> PathBuf {
    normalized_path.to_path_buf()
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn ingest_chatml_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("test.jsonl");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("create cache dir");

        // Write a minimal ChatML dataset (simulating pragmatic-semantics traces)
        let records = vec![
            serde_json::json!({"messages": [
                {"role": "system", "content": "You classify constraints."},
                {"role": "user", "content": "Classify: must never expose memory."},
                {"role": "assistant", "content": "Prohibition (Rank 1)."}
            ]}),
            serde_json::json!({"messages": [
                {"role": "system", "content": "You classify constraints."},
                {"role": "user", "content": "Classify: prefer local models."},
                {"role": "assistant", "content": "Guideline (Rank 3)."}
            ]}),
        ];
        let mut file = std::fs::File::create(&input).expect("create input");
        for record in &records {
            writeln!(file, "{}", serde_json::to_string(record).unwrap()).expect("write");
        }

        let mut pipeline = DatasetPipeline::new(cache.clone());
        let normalized = pipeline.ingest(&input).expect("ingest should succeed");

        assert!(normalized.exists(), "normalized output should exist");
        let content = std::fs::read_to_string(&normalized).expect("read output");
        let lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "should have 2 conversations");

        // Verify each line is valid ChatML JSON
        for line in &lines {
            let conv: ChatConversation = serde_json::from_str(line).expect("valid ChatML JSON");
            assert!(!conv.messages.is_empty());
            let roles: Vec<_> = conv.messages.iter().map(|m| m.role.as_str()).collect();
            assert_eq!(roles, vec!["system", "user", "assistant"]);
        }
    }

    #[test]
    fn ingest_caches_result() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("test.jsonl");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("create cache dir");

        let record = serde_json::json!({"messages": [
            {"role": "user", "content": "What is P5?"},
            {"role": "assistant", "content": "Minimal Architecture."}
        ]});
        std::fs::write(
            &input,
            format!("{}\n", serde_json::to_string(&record).unwrap()),
        )
        .expect("write");

        let mut pipeline = DatasetPipeline::new(cache.clone());
        let first = pipeline.ingest(&input).expect("first ingest");
        let second = pipeline.ingest(&input).expect("second ingest");

        assert_eq!(first, second, "cached path should match");
        assert!(first.starts_with(&cache), "output should be in cache dir");
    }

    #[test]
    fn ingest_empty_dataset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("empty.jsonl");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("create cache dir");
        std::fs::write(&input, "\n\n").expect("write empty");

        let mut pipeline = DatasetPipeline::new(cache);
        let result = pipeline.ingest(&input);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DatasetError::Empty));
    }

    // ── Preference format detection tests ──

    #[test]
    fn detect_dpo_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("dpo.jsonl");
        let record = serde_json::json!({
            "prompt": "What is P5?",
            "chosen": "Minimal Architecture.",
            "rejected": "Maximum Architecture."
        });
        std::fs::write(&input, format!("{}\n", record)).expect("write");
        let format = DatasetFormat::detect(&input).expect("detect");
        assert_eq!(format, DatasetFormat::PreferenceDpo);
        assert!(format.is_preference());
    }

    #[test]
    fn detect_kto_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("kto.jsonl");
        let record = serde_json::json!({
            "prompt": "What is P5?",
            "completion": "Minimal Architecture.",
            "label": true
        });
        std::fs::write(&input, format!("{}\n", record)).expect("write");
        let format = DatasetFormat::detect(&input).expect("detect");
        assert_eq!(format, DatasetFormat::PreferenceKto);
        assert!(format.is_preference());
    }

    #[test]
    fn detect_orpo_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("orpo.jsonl");
        let record = serde_json::json!({
            "chosen": [{"role": "user", "content": "What is P5?"}, {"role": "assistant", "content": "Minimal Architecture."}],
            "rejected": [{"role": "user", "content": "What is P5?"}, {"role": "assistant", "content": "Maximum Architecture."}]
        });
        std::fs::write(&input, format!("{}\n", record)).expect("write");
        let format = DatasetFormat::detect(&input).expect("detect");
        assert_eq!(format, DatasetFormat::PreferenceOrpo);
        assert!(format.is_preference());
    }

    #[test]
    fn detect_chatml_not_confused_with_preference() {
        // A ChatML dataset should NOT be detected as preference, even though
        // it might contain the word "chosen" in the content.
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("chatml.jsonl");
        let record = serde_json::json!({"messages": [
            {"role": "user", "content": "What is the chosen method?"},
            {"role": "assistant", "content": "LoRA is chosen for efficiency."}
        ]});
        std::fs::write(&input, format!("{}\n", record)).expect("write");
        let format = DatasetFormat::detect(&input).expect("detect");
        assert_eq!(format, DatasetFormat::ChatML);
        assert!(!format.is_preference());
    }

    // ── Preference ingestion tests ──

    #[test]
    fn ingest_dpo_preference() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("dpo.jsonl");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("create cache dir");

        let records = vec![
            serde_json::json!({
                "prompt": "What is P5?",
                "chosen": "Minimal Architecture.",
                "rejected": "Maximum Architecture."
            }),
            serde_json::json!({
                "prompt": [{"role": "user", "content": "What is P1?"}],
                "chosen": [{"role": "assistant", "content": "User Sovereignty."}],
                "rejected": [{"role": "assistant", "content": "Admin Control."}]
            }),
        ];
        let mut file = std::fs::File::create(&input).expect("create input");
        for record in &records {
            writeln!(file, "{}", serde_json::to_string(record).unwrap()).expect("write");
        }

        let mut pipeline = DatasetPipeline::new(cache.clone());
        let normalized = pipeline.ingest(&input).expect("ingest should succeed");

        assert!(normalized.exists(), "normalized output should exist");
        let content = std::fs::read_to_string(&normalized).expect("read output");
        let lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "should have 2 preference examples");

        // Verify each line is valid PreferenceExample JSON
        for line in &lines {
            let ex: PreferenceExample =
                serde_json::from_str(line).expect("valid PreferenceExample JSON");
            assert!(ex.prompt.is_some(), "DPO should have prompt");
            assert!(!ex.chosen.is_null(), "chosen should be non-null");
            assert!(ex.rejected.is_some(), "DPO should have rejected");
            assert!(ex.label.is_none(), "DPO should not have label");
        }
    }

    #[test]
    fn ingest_kto_preference() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("kto.jsonl");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("create cache dir");

        let records = vec![
            serde_json::json!({
                "prompt": "What is P5?",
                "completion": "Minimal Architecture.",
                "label": true
            }),
            serde_json::json!({
                "prompt": "What is P5?",
                "completion": "I don't know.",
                "label": false
            }),
        ];
        let mut file = std::fs::File::create(&input).expect("create input");
        for record in &records {
            writeln!(file, "{}", serde_json::to_string(record).unwrap()).expect("write");
        }

        let mut pipeline = DatasetPipeline::new(cache.clone());
        let normalized = pipeline.ingest(&input).expect("ingest should succeed");

        let content = std::fs::read_to_string(&normalized).expect("read output");
        let lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "should have 2 KTO examples");

        for line in &lines {
            let ex: PreferenceExample =
                serde_json::from_str(line).expect("valid PreferenceExample JSON");
            assert!(ex.prompt.is_some(), "KTO should have prompt");
            assert!(
                !ex.chosen.is_null(),
                "chosen (completion) should be non-null"
            );
            assert!(ex.rejected.is_none(), "KTO should not have rejected");
            assert!(ex.label.is_some(), "KTO should have label");
        }
    }

    #[test]
    fn ingest_orpo_preference() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("orpo.jsonl");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("create cache dir");

        let records = vec![serde_json::json!({
            "chosen": [{"role": "user", "content": "What is P5?"}, {"role": "assistant", "content": "Minimal Architecture."}],
            "rejected": [{"role": "user", "content": "What is P5?"}, {"role": "assistant", "content": "Maximum Architecture."}]
        })];
        let mut file = std::fs::File::create(&input).expect("create input");
        for record in &records {
            writeln!(file, "{}", serde_json::to_string(record).unwrap()).expect("write");
        }

        let mut pipeline = DatasetPipeline::new(cache.clone());
        let normalized = pipeline.ingest(&input).expect("ingest should succeed");

        let content = std::fs::read_to_string(&normalized).expect("read output");
        let lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1, "should have 1 ORPO example");

        let ex: PreferenceExample =
            serde_json::from_str(lines[0]).expect("valid PreferenceExample JSON");
        assert!(
            ex.prompt.is_none(),
            "ORPO should not have prompt (implicit)"
        );
        assert!(!ex.chosen.is_null(), "chosen should be non-null");
        assert!(ex.rejected.is_some(), "ORPO should have rejected");
        assert!(ex.label.is_none(), "ORPO should not have label");
    }

    #[test]
    fn ingest_dpo_rejects_null_chosen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("bad_dpo.jsonl");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("create cache dir");

        let record = serde_json::json!({
            "prompt": "What is P5?",
            "chosen": null,
            "rejected": "Maximum Architecture."
        });
        std::fs::write(&input, format!("{}\n", record)).expect("write");

        let mut pipeline = DatasetPipeline::new(cache);
        let result = pipeline.ingest(&input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DatasetError::Validation { .. }
        ));
    }

    // ── Dataset profiling tests ──

    #[test]
    fn profile_chatml_dataset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("test.jsonl");

        let records = vec![
            serde_json::json!({"messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "What is P5?"},
                {"role": "assistant", "content": "Minimal Architecture."}
            ]}),
            serde_json::json!({"messages": [
                {"role": "user", "content": "What is P1?"},
                {"role": "assistant", "content": "User Sovereignty."}
            ]}),
        ];
        let mut file = std::fs::File::create(&input).expect("create input");
        for record in &records {
            writeln!(file, "{}", serde_json::to_string(record).unwrap()).expect("write");
        }

        let profile = DatasetPipeline::profile(&input);

        assert_eq!(profile.format, Some(DatasetFormat::ChatML));
        assert_eq!(profile.n_samples, Some(2));
        assert!(profile.avg_content_chars.is_some());
        assert!(profile.max_content_chars.is_some());
        assert!(profile.avg_token_estimate.is_some());
        assert!(profile.max_token_estimate.is_some());
        assert_eq!(profile.avg_messages_per_example, Some(2.5)); // (3+2)/2
        assert_eq!(profile.has_system_messages, Some(true));
        assert_eq!(profile.has_multi_turn, Some(true)); // first conv has 3 msgs (>2)
        assert_eq!(profile.has_vision_data, Some(false));
        assert!(profile.role_distribution.is_some());
        // Role distribution: system=1, user=2, assistant=2 → total=5
        let dist = profile.role_distribution.unwrap();
        assert!((dist["system"].as_f64().unwrap() - 0.2).abs() < 0.01);
        assert!((dist["user"].as_f64().unwrap() - 0.4).abs() < 0.01);
        assert!((dist["assistant"].as_f64().unwrap() - 0.4).abs() < 0.01);
    }

    #[test]
    fn profile_dpo_preference_dataset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("dpo.jsonl");

        let records = vec![
            serde_json::json!({
                "prompt": "What is P5?",
                "chosen": "Minimal Architecture.",
                "rejected": "Maximum Architecture."
            }),
            serde_json::json!({
                "prompt": "What is P1?",
                "chosen": "User Sovereignty.",
                "rejected": "Admin Control."
            }),
        ];
        let mut file = std::fs::File::create(&input).expect("create input");
        for record in &records {
            writeln!(file, "{}", serde_json::to_string(record).unwrap()).expect("write");
        }

        let profile = DatasetPipeline::profile(&input);

        assert_eq!(profile.format, Some(DatasetFormat::PreferenceDpo));
        assert_eq!(profile.n_samples, Some(2));
        assert!(profile.chosen_rejected_length_ratio.is_some());
        // chosen="Minimal Architecture." (21), rejected="Maximum Architecture." (21) → 1.0
        // chosen="User Sovereignty." (18), rejected="Admin Control." (14) → 1.286
        // average = (1.0 + 1.286) / 2 ≈ 1.143
        let ratio = profile.chosen_rejected_length_ratio.unwrap();
        assert!(
            ratio > 0.9 && ratio < 1.3,
            "expected ratio near 1.0, got {}",
            ratio
        );
        assert_eq!(profile.has_vision_data, Some(false));
    }

    #[test]
    fn profile_nonexistent_file_returns_empty_profile() {
        let profile = DatasetPipeline::profile(std::path::Path::new("/nonexistent/path.jsonl"));
        // Format detection may succeed based on extension alone, but all stats are None.
        assert_eq!(profile.n_samples, None);
        assert_eq!(profile.avg_content_chars, None);
    }

    #[test]
    fn profile_detects_vision_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("vlm.jsonl");
        let record = serde_json::json!({
            "messages": [
                {"role": "user", "content": "Describe this image"},
                {"role": "assistant", "content": "A cat sitting on a mat."}
            ],
            "image": "path/to/image.jpg"
        });
        std::fs::write(&input, format!("{}\n", record)).expect("write");

        let profile = DatasetPipeline::profile(&input);
        assert_eq!(profile.has_vision_data, Some(true));
    }
}
