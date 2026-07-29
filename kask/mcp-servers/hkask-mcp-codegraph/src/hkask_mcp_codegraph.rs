#![deny(unsafe_code)]
//! MCP server for hkask-codegraph — code understanding tools.
#![allow(unused_crate_dependencies)]

pub mod codegraph;

use crate::codegraph::graph::analysis;
use crate::codegraph::graph::traversal;
use crate::codegraph::indexer::pipeline::IndexPipeline;
use crate::codegraph::types::Direction;
use crate::codegraph::{ContextBudget, graph};
use hkask_mcp_server::run_server;
use hkask_mcp_server::server::{CapabilityTier, CredentialRequirement, McpToolError, execute_tool};
use hkask_types::InferencePort;
use hkask_types::WebID;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

hkask_mcp_server::mcp_server!(
    pub struct CodeGraphServer {
        pub capability_tier: CapabilityTier,
        pipeline: Arc<Mutex<IndexPipeline>>,
        /// Tracks whether the workspace has been indexed at least once.
        /// `ensure_indexed()` checks this to avoid re-walking the workspace on
        /// every read tool call. `codegraph_reindex` resets it to force a
        /// fresh index on the next read.
        indexed_once: Arc<std::sync::atomic::AtomicBool>,
        /// Inference port for embedding generation. Routes embeddings through
        /// zed's `LanguageModelEmbeddingPort` via the IPC bridge, replacing
        /// the old raw-reqwest calls that bypassed zed's credential resolution.
        pub inference_port: Arc<dyn InferencePort>,
    }
);

// Helper: convert any displayable error to McpToolError::internal
fn db_err(e: impl std::fmt::Display) -> McpToolError {
    McpToolError::internal(e.to_string())
}

impl CodeGraphServer {
    fn pipeline_guard(&self) -> Result<std::sync::MutexGuard<'_, IndexPipeline>, McpToolError> {
        self.pipeline
            .lock()
            .map_err(|_| McpToolError::internal("pipeline lock poisoned"))
    }

    fn ensure_indexed(&self) -> Result<(), McpToolError> {
        // Fast path: if we've already indexed, skip the walk entirely.
        // The BLAKE3 hash check inside index_directory still catches changed
        // files on the next explicit codegraph_reindex call.
        if self.indexed_once.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut pipeline = self.pipeline_guard()?;

        // OCAP-governed file access (#5): future integration point.
        // When the daemon provides capability verification, filter paths here
        // via capability tokens before passing to index_directory.
        // For now: index entire workspace (standalone mode).
        let results = pipeline
            .index_directory(&cwd)
            .map_err(|e| McpToolError::internal(format!("index failed: {e}")))?;

        let total: usize = results.iter().map(|r| r.symbols).sum();
        tracing::info!(target: "hkask.mcp.codegraph", symbols = total, "Auto-indexed");

        // Compute PageRank and emit health Regulation events (G7, G8)
        if let Err(e) = pipeline.finalize() {
            tracing::warn!(target: "hkask.mcp.codegraph", error = %e, "Finalize failed");
        }

        // Mark as indexed so subsequent read tool calls skip the walk.
        self.indexed_once
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
}

// ── Request types for tools with structured parameters ────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryRequest {
    query: String,
    #[serde(default = "default_limit")]
    limit: u64,
    /// Optional: look up exact symbol name (replaces codegraph_node)
    #[serde(default)]
    name: Option<String>,
}
fn default_limit() -> u64 {
    10
}

/// Request for `codegraph_index_embeddings` — generate embeddings for all
/// indexed symbols via the configured embedding API.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmbedIndexRequest {
    /// Optional: override the embedding model (default: `HKASK_EMBEDDING_MODEL`
    /// or `DeepInfra/Qwen/Qwen3-Embedding-0.6B`).
    #[serde(default)]
    model: Option<String>,
    /// Batch size for embedding API calls. Default: 32.
    #[serde(default = "default_batch_size")]
    batch_size: u32,
}
fn default_batch_size() -> u32 {
    32
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraverseRequest {
    symbol: String,
    #[serde(default)]
    direction: Direction,
    #[serde(default = "default_depth")]
    max_depth: u64,
}
fn default_depth() -> u64 {
    5
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImpactRequest {
    symbol: String,
    #[serde(default = "default_depth")]
    max_depth: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContextRequest {
    query: String,
    #[serde(default)]
    budget: ContextBudget,
}

/// Analysis type for codegraph_analysis tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisKind {
    DeadCode,
    Complexity,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalysisRequest {
    kind: AnalysisKind,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StructureRequest {
    #[serde(default = "default_structure_limit")]
    limit: u64,
}
fn default_structure_limit() -> u64 {
    20
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatsRequest {
    #[serde(default)]
    include_health: bool,
    /// Include language/file-type breakdown (replaces codegraph_project_meta)
    #[serde(default)]
    include_meta: bool,
}

// ── Tools ─────────────────────────────────────────────────────────

#[tool_router(server_handler)]
impl CodeGraphServer {
    #[tool(
        description = "Search the codebase for symbols, or look up a specific symbol by name (set 'name' field)"
    )]
    pub async fn codegraph_query(&self, Parameters(req): Parameters<QueryRequest>) -> String {
        execute_tool(self, "codegraph_query", async {
            self.ensure_indexed()?;
            let pipeline = self.pipeline_guard()?;
            let results =
                graph::search::search(pipeline.store().conn(), &req.query, req.limit as usize)
                    .map_err(db_err)?;
            // If name provided, return exact symbol match (replaces codegraph_node).
            // Returns an explicit error when the name is not found, matching
            // codegraph_traverse's contract — never a silent null.
            if let Some(ref name) = req.name {
                let exact = results.iter().find(|r| r.symbol.name == *name);
                return match exact {
                    Some(r) => Ok(serde_json::json!(&r.symbol)),
                    None => Ok(serde_json::json!({
                        "error": format!("symbol not found: {name}")
                    })),
                };
            }
            Ok(serde_json::json!(results))
        })
        .await
    }

    #[tool(description = "Traverse the code graph: forward (dependencies) or reverse (callers)")]
    pub async fn codegraph_traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
        execute_tool(self, "codegraph_traverse", async {
            self.ensure_indexed()?;
            let pipeline = self.pipeline_guard()?;
            let id =
                traversal::find_symbol_id(pipeline.store().conn(), &req.symbol).map_err(db_err)?;
            match id {
                Some(id) => {
                    let nodes = traversal::traverse(
                        pipeline.store().conn(),
                        id,
                        req.direction,
                        req.max_depth as usize,
                    )
                    .map_err(db_err)?;
                    Ok(serde_json::json!(nodes))
                }
                None => {
                    Ok(serde_json::json!({"error": format!("symbol not found: {}", req.symbol)}))
                }
            }
        })
        .await
    }

    #[tool(description = "Analyze blast radius for a symbol")]
    pub async fn codegraph_impact(&self, Parameters(req): Parameters<ImpactRequest>) -> String {
        execute_tool(self, "codegraph_impact", async {
            self.ensure_indexed()?;
            let pipeline = self.pipeline_guard()?;
            let id =
                traversal::find_symbol_id(pipeline.store().conn(), &req.symbol).map_err(db_err)?;
            match id {
                Some(id) => {
                    let results = traversal::impact_analysis(
                        pipeline.store().conn(),
                        id,
                        req.max_depth as usize,
                    )
                    .map_err(db_err)?;
                    Ok(serde_json::json!({
                        "symbol": req.symbol,
                        "total_affected": results.len(),
                        "affected": results,
                    }))
                }
                None => {
                    Ok(serde_json::json!({"error": format!("symbol not found: {}", req.symbol)}))
                }
            }
        })
        .await
    }

    #[tool(description = "Run analysis: 'dead_code' or 'complexity'")]
    pub async fn codegraph_analysis(&self, Parameters(req): Parameters<AnalysisRequest>) -> String {
        execute_tool(self, "codegraph_analysis", async {
            self.ensure_indexed()?;
            let pipeline = self.pipeline_guard()?;
            match req.kind {
                AnalysisKind::DeadCode => {
                    let findings =
                        analysis::find_dead_code(pipeline.store().conn()).map_err(db_err)?;
                    Ok(serde_json::json!(findings))
                }
                AnalysisKind::Complexity => {
                    let findings = analysis::find_high_complexity(pipeline.store().conn(), 10, 5)
                        .map_err(db_err)?;
                    Ok(serde_json::json!(findings))
                }
            }
        })
        .await
    }

    #[tool(description = "Assemble token-budgeted context for LLM prompts")]
    pub async fn codegraph_context(&self, Parameters(req): Parameters<ContextRequest>) -> String {
        execute_tool(self, "codegraph_context", async {
            self.ensure_indexed()?;
            let pipeline = self.pipeline_guard()?;
            let assembled =
                crate::codegraph::assemble_context(pipeline.store().conn(), &req.query, req.budget)
                    .map_err(db_err)?;
            Ok(serde_json::json!({
                "context_id": assembled.context_id.to_string(),
                "text": assembled.text,
                "symbols": assembled.symbols,
                "estimated_tokens": assembled.estimated_tokens,
            }))
        })
        .await
    }

    #[tool(description = "Get project overview: top symbols")]
    pub async fn codegraph_structure(
        &self,
        Parameters(req): Parameters<StructureRequest>,
    ) -> String {
        execute_tool(self, "codegraph_structure", async {
            self.ensure_indexed()?;
            let pipeline = self.pipeline_guard()?;
            let conn = pipeline.store().conn();
            let limit = req.limit as i64;
            let mut stmt = conn
                .prepare(
                    "SELECT name, kind, f.path, signature, visibility, pagerank
                 FROM symbols s JOIN code_files f ON s.file_id = f.id
                 ORDER BY pagerank DESC LIMIT ?1",
                )
                .map_err(db_err)?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map(rusqlite::params![limit], |row| {
                    Ok(serde_json::json!({
                        "name": row.get::<_, String>(0)?,
                        "kind": row.get::<_, String>(1)?,
                        "file": row.get::<_, String>(2)?,
                        "signature": row.get::<_, String>(3)?,
                        "visibility": row.get::<_, String>(4)?,
                        "pagerank": row.get::<_, f64>(5)?,
                    }))
                })
                .map_err(db_err)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(serde_json::json!(rows))
        })
        .await
    }

    #[tool(description = "Get index statistics")]
    pub async fn codegraph_stats(&self, Parameters(req): Parameters<StatsRequest>) -> String {
        execute_tool(self, "codegraph_stats", async {
            // Intentionally does NOT call ensure_indexed() — stats is a lightweight
            // query that should return immediately. On a fresh server with no prior
            // tool call, stats returns zeros. Call codegraph_reindex or any other
            // tool first to populate the index.
            let pipeline = self.pipeline_guard()?;
            let stats = pipeline.stats().map_err(db_err)?;
            let mut output = serde_json::json!({
                "files": stats.files, "symbols": stats.symbols, "edges": stats.edges,
            });
            if req.include_health && stats.symbols > 0 {
                let ratio = stats.edges as f64 / stats.symbols as f64;
                output["connectivity_ratio"] = serde_json::json!(ratio);
                output["health"] = serde_json::json!(if ratio < 0.1 {
                    "poor"
                } else if ratio < 0.5 {
                    "fair"
                } else {
                    "good"
                });
            }
            // Include language/file-type breakdown if requested (X4: merged from codegraph_project_meta)
            if req.include_meta {
                let conn = pipeline.store().conn();
                let mut stmt = conn
                    .prepare(
                        "SELECT COUNT(*),
                        SUM(CASE WHEN path LIKE '%.rs' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN path LIKE '%.toml' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN path LIKE '%.md' THEN 1 ELSE 0 END)
                 FROM code_files",
                    )
                    .map_err(db_err)?;
                if let Ok(meta) = stmt.query_row([], |row| {
                    Ok(serde_json::json!({
                        "total": row.get::<_, i64>(0)?,
                        "rust": row.get::<_, i64>(1)?,
                        "toml": row.get::<_, i64>(2)?,
                        "md": row.get::<_, i64>(3)?,
                        "primary_language": "Rust",
                    }))
                }) {
                    output["meta"] = meta;
                }
            }
            Ok(output)
        })
        .await
    }

    #[tool(description = "Force full re-index of the workspace")]
    pub async fn codegraph_reindex(&self) -> String {
        execute_tool(self, "codegraph_reindex", async {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            // Acquire a mutable lock so we can call finalize() (which needs &mut self).
            let mut pipeline = self.pipeline_guard()?;
            let results = pipeline.index_directory(&cwd)
                .map_err(db_err)?;
            let total_sym: usize = results.iter().map(|r| r.symbols).sum();
            let total_edg: usize = results.iter().map(|r| r.edges).sum();
            let indexed: usize = results.iter().filter(|r| !r.skipped).count();
            // Recompute PageRank and reset staleness — matches ensure_indexed() behavior.
            // Without this, codegraph_structure returns stale rankings after a forced reindex.
            if let Err(e) = pipeline.finalize() {
                tracing::warn!(target: "hkask.mcp.codegraph", error = %e, "Finalize failed after reindex");
            }
            let stats = pipeline.stats().map_err(db_err)?;
            // Mark as indexed so subsequent read tool calls skip the walk.
            self.indexed_once.store(true, std::sync::atomic::Ordering::Release);
            Ok(serde_json::json!({
                "files_indexed": indexed, "symbols_added": total_sym, "edges_added": total_edg,
                "total_files": stats.files, "total_symbols": stats.symbols, "total_edges": stats.edges,
            }))
        }).await
    }

    /// Generate embeddings for all indexed symbols via the configured embedding API.
    ///
    /// Reads all symbols from the database, batches them, calls the embedding
    /// API through zed's `LanguageModelEmbeddingPort` (via the IPC bridge),
    /// and stores the resulting vectors in the `symbols_vec` sqlite-vec table
    /// for semantic similarity search.
    ///
    /// Uses `HKASK_EMBEDDING_MODEL` (default: `DeepInfra/Qwen/Qwen3-Embedding-0.6B`) and
    /// `HKASK_EMBEDDING_DIM` (default: 1024). Credentials are resolved from
    /// zed's `LanguageModelRegistry` — no env-var API keys needed.
    #[tool(
        description = "Generate embeddings for all indexed symbols via the embedding API. Routes through zed's inference bridge."
    )]
    pub async fn codegraph_index_embeddings(
        &self,
        Parameters(req): Parameters<EmbedIndexRequest>,
    ) -> String {
        execute_tool(self, "codegraph_index_embeddings", async {
            self.ensure_indexed()?;

            // Resolve the embedding model and dimension.
            let model = req.model.unwrap_or_else(|| {
                std::env::var("HKASK_EMBEDDING_MODEL")
                    .unwrap_or_else(|_| "DeepInfra/Qwen/Qwen3-Embedding-0.6B".to_string())
            });
            let dim: usize = std::env::var("HKASK_EMBEDDING_DIM")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&d: &usize| d > 0)
                .unwrap_or(1024);

            // Collect all symbols for embedding — drop the lock before any
            // async API calls. GraphStore is not Send (RefCell<LruCache>),
            // so we cannot hold the MutexGuard across .await points.
            let symbols: Vec<(i64, String, String)> = {
                let pipeline = self.pipeline_guard()?;
                let store = pipeline.store();
                store.all_symbols_for_embedding().map_err(db_err)?
            };

            if symbols.is_empty() {
                return Ok(serde_json::json!({
                    "symbols_embedded": 0,
                    "model": model,
                    "dim": dim,
                    "errors": [],
                    "note": "no symbols indexed — run codegraph_reindex first"
                }));
            }

            let batch_size = req.batch_size.max(1) as usize;
            let mut embeddings_to_insert: Vec<(i64, Vec<f32>)> = Vec::new();
            let mut errors: Vec<String> = Vec::new();

            for chunk in symbols.chunks(batch_size) {
                let texts: Vec<String> = chunk.iter().map(|(_, _, t)| t.clone()).collect();

                match self.inference_port.embed(&model, &texts).await {
                    Ok(vectors) => {
                        for (i, embedding) in vectors.iter().enumerate() {
                            if i >= chunk.len() {
                                break;
                            }
                            if embedding.len() == dim {
                                embeddings_to_insert.push((chunk[i].0, embedding.clone()));
                            } else {
                                errors.push(format!(
                                    "dimension mismatch: expected {}, got {}",
                                    dim,
                                    embedding.len()
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("embedding API error: {}", e));
                    }
                }
            }

            // Re-acquire the lock to insert embeddings into the database.
            let symbols_embedded = {
                let pipeline = self.pipeline_guard()?;
                let store = pipeline.store();
                let mut count = 0usize;
                for (symbol_id, embedding) in &embeddings_to_insert {
                    match store.upsert_embedding(*symbol_id, embedding) {
                        Ok(()) => count += 1,
                        Err(e) => errors.push(format!("symbol {} insert failed: {}", symbol_id, e)),
                    }
                }
                count
            };

            tracing::info!(
                target: "hkask.mcp.codegraph",
                symbols_embedded,
                total_symbols = symbols.len(),
                model = %model,
                dim,
                error_count = errors.len(),
                "Embedding indexing complete"
            );

            Ok(serde_json::json!({
                "symbols_embedded": symbols_embedded,
                "total_symbols": symbols.len(),
                "model": model,
                "dim": dim,
                "errors": errors,
            }))
        })
        .await
    }
}

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    let db_path = std::env::var("HKASK_CODEGRAPH_DB").ok();
    // Resolve the inference port once — routes embeddings through zed's
    // LanguageModelEmbeddingPort via the IPC bridge.
    let inference_port = hkask_inference::resolve_inference_port().await;
    run_server(
        "hkask-mcp-codegraph",
        SERVER_VERSION,
        |_ctx| {
            let webid = WebID::new();
            let store =
                match &db_path {
                    Some(path) => {
                        crate::codegraph::graph::store::GraphStore::open(path).map_err(|e| {
                            hkask_mcp_server::McpError::UnexpectedResponse {
                                context: "codegraph graph store open".into(),
                                detail: e.to_string(),
                            }
                        })?
                    }
                    None => crate::codegraph::graph::store::GraphStore::open_in_memory().map_err(
                        |e| hkask_mcp_server::McpError::UnexpectedResponse {
                            context: "codegraph graph store open_in_memory".into(),
                            detail: e.to_string(),
                        },
                    )?,
                };
            let pipeline = IndexPipeline::new(store);
            Ok(CodeGraphServer::new(
                webid,
                CapabilityTier::detect(&std::collections::HashMap::new()),
                Arc::new(Mutex::new(pipeline)),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                inference_port.clone(),
            ))
        },
        vec![CredentialRequirement::optional(
            "HKASK_CODEGRAPH_DB",
            "Path to the codegraph SQLite database (in-memory if absent)",
        )],
    )
    .await
}
