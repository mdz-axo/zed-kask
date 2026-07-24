use crate::TrainingServer;
use crate::dataset::DatasetPipeline;
use crate::types::{AssembleDatasetRequest, IngestQaRequest, TrainIngestDatasetRequest};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_storage::HMem;
use hkask_types::Visibility;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use serde_json::json;
use std::path::PathBuf;

impl TrainingServer {
    #[tool(
        description = "Ingest QA pairs for model training. Stores question-answer pairs with provenance in semantic memory for future fine-tuning dataset assembly."
    )]
    pub async fn training_ingest_qa(
        &self,
        Parameters(IngestQaRequest {
            qa_items,
            source,
            dataset,
        }): Parameters<IngestQaRequest>,
    ) -> String {
        execute_tool(self, "training_ingest_qa", async {
            let Some(semantic) = &self.semantic else {
                return Err(McpToolError::permission_denied(
                    "Semantic memory not available — set HKASK_MEMORY_DB and HKASK_DB_PASSPHRASE",
                ));
            };
            if qa_items.is_empty() {
                return Err(McpToolError::invalid_argument("qa_items must not be empty"));
            }
            hkask_mcp_server::validate_identifier("source", &source, 256)?;
            let ds = dataset.as_deref().unwrap_or("default");
            let mut stored = 0;
            let mut errors = Vec::new();
            for (i, qa) in qa_items.iter().enumerate() {
                let entity = format!("training:qa:manual:{ds}:{source}:{i}");
                let level = qa.bloom_level.as_deref().unwrap_or("factual");
                let value = json!({"question": qa.question, "answer": qa.answer, "bloom_level": level, "source": source, "dataset": ds});
                let h_mem = HMem::new(&entity, "training_qa_pair", value, self.webid)
                    .with_visibility(Visibility::Public)
                    .with_confidence(1.0);
                match semantic.store(h_mem) {
                    Ok(()) => stored += 1,
                    Err(e) => errors.push(format!("Item {i}: {e}")),
                }
            }
            if errors.is_empty() {
                Ok(json!({ "stored": stored, "source": source, "dataset": ds }))
            } else {
                Err(McpToolError::internal(json!({ "stored": stored, "errors": errors, "source": source, "dataset": ds }).to_string()))
            }
        })
        .await
    }

    #[tool(
        description = "Assemble stored QA pairs into a ChatML JSONL training dataset file. Queries semantic memory for training_qa_pair h_mems, filters by dataset/source/bloom level, and writes a file ready for training_submit. Optionally splits into train/test."
    )]
    pub async fn training_assemble_dataset(
        &self,
        Parameters(AssembleDatasetRequest {
            dataset,
            source,
            bloom_level,
            output_path,
            train_split,
            max_examples,
            system_prompt,
        }): Parameters<AssembleDatasetRequest>,
    ) -> String {
        execute_tool(self, "training_assemble_dataset", async {
            let Some(semantic) = &self.semantic else {
                return Err(McpToolError::permission_denied(
                    "Semantic memory not available — set HKASK_MEMORY_DB and HKASK_DB_PASSPHRASE",
                ));
            };
            let h_mems = match semantic.query_by_attribute("training_qa_pair") {
                Ok(t) => t,
                Err(e) => return Err(McpToolError::internal(format!("Failed to query QA h_mems: {e}"))),
            };
            if h_mems.is_empty() {
                return Err(McpToolError::invalid_argument("No training_qa_pair h_mems found. Ingest QA pairs first with training_ingest_qa."));
            }
            let mut conversations: Vec<serde_json::Value> = Vec::new();
            for h_mem in &h_mems {
                let value = &h_mem.value;
                let q_ds = value.get("dataset").and_then(|v| v.as_str()).unwrap_or("");
                let q_source = value.get("source").and_then(|v| v.as_str()).unwrap_or("");
                let q_bloom = value.get("bloom_level").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(ref ds) = dataset && q_ds != ds.as_str() { continue; }
                if let Some(ref src) = source && q_source != src.as_str() { continue; }
                if let Some(ref bl) = bloom_level && q_bloom != bl.as_str() { continue; }
                let question = value.get("question").and_then(|v| v.as_str()).unwrap_or("");
                let answer = value.get("answer").and_then(|v| v.as_str()).unwrap_or("");
                if question.is_empty() || answer.is_empty() { continue; }
                let mut messages = vec![json!({"role": "user", "content": question}), json!({"role": "assistant", "content": answer})];
                if let Some(ref sys) = system_prompt { messages.insert(0, json!({"role": "system", "content": sys})); }
                conversations.push(json!({ "messages": messages }));
            }
            if conversations.is_empty() {
                return Err(McpToolError::invalid_argument("No QA pairs matched the given filters."));
            }
            let total = conversations.len();
            let limit = max_examples.unwrap_or(total).min(total);
            conversations.truncate(limit);
            let train_count = if let Some(split) = train_split {
                let split = split.clamp(0.0, 1.0);
                (limit as f64 * split) as usize
            } else { limit };
            let write_jsonl = |path: &str, items: &[serde_json::Value]| -> Result<usize, std::io::Error> {
                let mut output = String::new();
                for item in items {
                    output.push_str(&serde_json::to_string(item).expect("Value serialization cannot fail"));
                    output.push('\n');
                }
                std::fs::write(path, output)?;
                Ok(items.len())
            };
            let train_items = &conversations[..train_count];
            match write_jsonl(&output_path, train_items) {
                Ok(n) => {
                    let mut result = json!({"train_examples": n, "train_path": output_path, "total_matched": total});
                    if train_count < limit {
                        let test_path = format!("{output_path}.test.jsonl");
                        let test_items = &conversations[train_count..];
                        match write_jsonl(&test_path, test_items) {
                            Ok(m) => { result["test_examples"] = json!(m); result["test_path"] = json!(test_path); }
                            Err(e) => { result["test_write_error"] = json!(e.to_string()); }
                        }
                    }
                    Ok(result)
                }
                Err(e) => Err(McpToolError::internal(format!("Failed to write dataset file: {e}"))),
            }
        })
        .await
    }

    #[tool(
        description = "Ingest a raw dataset file into the normalized cache without submitting a training job. Detects format (ChatML, ShareGPT, Alpaca, raw text, DPO preference, KTO preference, ORPO preference), normalizes to canonical format, validates, and caches. Returns the cached path for use with training_submit."
    )]
    pub async fn training_ingest_dataset(
        &self,
        Parameters(TrainIngestDatasetRequest {
            dataset_path,
            cache_dir,
        }): Parameters<TrainIngestDatasetRequest>,
    ) -> String {
        execute_tool(self, "training_ingest_dataset", async {
            let file_path = PathBuf::from(&dataset_path);
            if !file_path.exists() {
                return Err(McpToolError::invalid_argument(format!("Dataset file not found: {dataset_path}")));
            }
            let mut pipeline = if let Some(ref dir) = cache_dir {
                DatasetPipeline::new(PathBuf::from(dir))
            } else {
                self.pipeline.lock().unwrap_or_else(|e| e.into_inner()).clone()
            };
            let format = crate::dataset::DatasetFormat::detect(&file_path);
            match pipeline.ingest(&file_path) {
                Ok(normalized_path) => {
                    let is_preference = format.map(|f| f.is_preference()).unwrap_or(false);
                    Ok(json!({
                        "dataset_path": dataset_path,
                        "normalized_path": normalized_path.to_string_lossy(),
                        "detected_format": format.map(|f| format!("{f:?}")).unwrap_or_else(|| "unknown".to_string()),
                        "is_preference": is_preference, "cached": true,
                    }))
                }
                Err(e) => Err(McpToolError::invalid_argument(format!("Dataset ingest error: {e}"))),
            }
        })
        .await
    }
}
