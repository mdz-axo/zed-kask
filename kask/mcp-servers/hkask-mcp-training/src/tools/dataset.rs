use crate::TrainingServer;
use crate::dataset::DatasetPipeline;
use crate::tools::error_mapping::map_dataset_error;
use crate::types::{AssembleDatasetRequest, IngestQaRequest, TrainIngestDatasetRequest};
use hkask_mcp_server::server::{
    McpToolError, execute_tool_semantic, map_io_error, map_memory_store_error,
};
use hkask_storage::HMem;
use hkask_types::{HMemOntology, Visibility};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde_json::json;
use std::path::PathBuf;

#[tool_router(router = dataset_router, vis = "pub")]
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
        execute_tool_semantic(self, "training_ingest_qa", Self::ontology_anchor("training_ingest_qa"), async {
            let Some(store) = &self.store else {
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
                // State-axis anchoring (P5.4): a QA pair is a training-dataset
                // record, not a process step. The bloom level is the pedagogic
                // classification, so it belongs on the subject axis alongside
                // the dataset name.
                let ontology = HMemOntology::state(
                    "dcterms:Dataset",
                    vec![ds.to_string(), level.to_string()],
                    source.clone(),
                );
                let h_mem = HMem::new(&entity, "training_qa_pair", value, self.webid)
                    .with_visibility(Visibility::Public)
                    .with_confidence(1.0)
                    .with_ontology(ontology);
                match store.store(h_mem) {
                    Ok(()) => stored += 1,
                    Err(e) => errors.push(format!("Item {i}: {e}")),
                }
            }
            if errors.is_empty() {
                Ok(json!({ "stored": stored, "source": source, "dataset": ds }))
            } else {
                Err(McpToolError::internal(json!({ "stored": stored, "errors": errors, "source": source, "dataset": ds }).to_string())) // rr0044-ok: partial-store-failure-aggregate
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
            db_path,
            passphrase,
        }): Parameters<AssembleDatasetRequest>,
    ) -> String {
        execute_tool_semantic(self, "training_assemble_dataset", Self::ontology_anchor("training_assemble_dataset"), async {
            // When db_path is provided, open that DB instead of using the
            // training server's default store. This bridges the corpus→training
            // gap: corpus_ingest_qa stores to the corpus DB (via its db_path
            // parameter), while training_assemble_dataset defaults to the
            // training DB. Without this override, QA pairs ingested via the
            // corpus pipeline are invisible to the training assembler.
            let h_mems = if let Some(db_path) = db_path.as_deref() {
                let passphrase = passphrase
                    .clone()
                    .or_else(|| self.db_passphrase.clone())
                    .unwrap_or_default();
                if passphrase.is_empty() {
                    return Err(McpToolError::permission_denied(
                        "db_path provided but passphrase is empty",
                    ));
                }
                let store = hkask_memory::MemoryStore::open(db_path, &passphrase, hkask_storage::embedding_dim())
                    .map_err(|e| McpToolError::internal(format!("Cannot open memory DB '{db_path}': {e}")))?;
                store.query_by_attribute("training_qa_pair")
                    .map_err(|e| map_memory_store_error(e, "semantic memory query"))?
            } else {
                let Some(store) = &self.store else {
                    return Err(McpToolError::permission_denied(
                        "Semantic memory not available — set HKASK_MEMORY_DB and HKASK_DB_PASSPHRASE, or provide db_path",
                    ));
                };
                store.query_by_attribute("training_qa_pair")
                    .map_err(|e| map_memory_store_error(e, "semantic memory query"))?
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
            let write_jsonl = |path: &std::path::Path, items: &[serde_json::Value]| -> Result<usize, std::io::Error> {
                let mut output = String::new();
                for item in items {
                    output.push_str(
                        &serde_json::to_string(item)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
                    );
                    output.push('\n');
                }
                std::fs::write(path, output)?;
                Ok(items.len())
            };
            // Contain the LLM-supplied output path (CWE-73): a write to
            // ~/.ssh/authorized_keys or /etc/cron.d/... must be rejected.
            let train_path = hkask_mcp_server::contain_for_write(&output_path)?;
            let train_items = &conversations[..train_count];
            match write_jsonl(&train_path, train_items) {
                Ok(n) => {
                    let mut result = json!({"train_examples": n, "train_path": output_path, "total_matched": total});
                    if train_count < limit {
                        let test_path = PathBuf::from(format!("{}.test.jsonl", train_path.to_string_lossy()));
                        let test_items = &conversations[train_count..];
                        match write_jsonl(&test_path, test_items) {
                            Ok(m) => { result["test_examples"] = json!(m); result["test_path"] = json!(test_path.to_string_lossy().to_string()); }
                            Err(e) => { result["test_write_error"] = json!(e.to_string()); }
                        }
                    }
                    Ok(result)
                }
                Err(e) => Err(map_io_error(e, "Failed to write dataset file")),
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
        execute_tool_semantic(self, "training_ingest_dataset", Self::ontology_anchor("training_ingest_dataset"), async {
            // Contain the caller-supplied dataset read path (CWE-200) and the
            // optional cache_dir write target (CWE-73) before any pipeline op.
            let file_path = hkask_mcp_server::contain_for_read(&dataset_path)?;
            let mut pipeline = if let Some(ref dir) = cache_dir {
                DatasetPipeline::new(hkask_mcp_server::contain_for_write(dir)?)
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
                Err(e) => Err(map_dataset_error(e)),
            }
        })
        .await
    }
}
