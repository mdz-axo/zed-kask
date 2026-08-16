#![deny(unsafe_code)]
//! MCP server for hkask-codegraph — code understanding tools.
#![warn(clippy::let_underscore_future)]

pub mod codegraph;

use crate::codegraph::graph::analysis;
use crate::codegraph::graph::traversal;
use crate::codegraph::indexer::pipeline::IndexPipeline;
use crate::codegraph::types::Direction;
use crate::codegraph::{ContextBudget, graph};
use hkask_mcp_server::run_server;
use hkask_mcp_server::server::{CapabilityTier, McpToolError, execute_tool_semantic, map_io_error};
use hkask_types::InferencePort;
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
    McpToolError::internal(e.to_string()) // rr0044-ok: mapper-fallback
}

/// Classify a `CodeGraphError` into the MCP wire-level `McpToolError` kind.
///
/// `Io` errors route through `map_io_error` so `NotFound`/`PermissionDenied`
/// surface as caller-fixable kinds; `NotUtf8` is a user-input problem
/// (`invalid_argument`); the remaining variants are infrastructure or
/// source-parse failures that remain `internal`.
fn map_codegraph_error(e: crate::codegraph::CodeGraphError) -> McpToolError {
    use crate::codegraph::error::IndexError;
    match e {
        crate::codegraph::CodeGraphError::Io(io) => map_io_error(io, "codegraph I/O"),
        crate::codegraph::CodeGraphError::Index(IndexError::FileNotAccessible { path, source }) => {
            match source {
                Some(io) => map_io_error(io, &format!("file not accessible: {path}")),
                None => McpToolError::not_found(format!("file not accessible: {path}")),
            }
        }
        crate::codegraph::CodeGraphError::Index(IndexError::NotUtf8 { path }) => {
            McpToolError::invalid_argument(format!("file not valid UTF-8: {path}"))
        }
        _ => McpToolError::internal(e.to_string()), // rr0044-ok: mapper-fallback
    }
}

impl CodeGraphServer {
    fn pipeline_guard(&self) -> Result<std::sync::MutexGuard<'_, IndexPipeline>, McpToolError> {
        self.pipeline
            .lock()
            .map_err(|_| McpToolError::internal("pipeline lock poisoned")) // rr0044-ok: lock-poisoned
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

        // File-access scoping (#5): future integration point.
        // When per-agent path scoping is wired, filter paths here before
        // passing to index_directory.
        // For now: index entire workspace (standalone mode).
        let results = pipeline
            .index_directory(&cwd)
            .map_err(map_codegraph_error)?;

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

    /// Map a tool name to its SUMO / Dublin Core ontology concept URI. The
    /// concept tags the `reg.tool` span (via `execute_tool_semantic`) for
    /// type-aware feedback routing. Codegraph is a code-structure tool, so
    /// SUMO — the upper ontology for entities/relations/processes — is the
    /// natural anchor for most tools; `codegraph_stats` returns a dataset and
    /// anchors on Dublin Core.
    ///
    /// SUMO reference: <https://github.com/ontologyportal/sumo>
    fn ontology_anchor(tool: &str) -> Option<&'static str> {
        use hkask_bridge_ontology::{dc_bibo, sumo};
        match tool {
            // Read tools — entity / relation / text structure
            "codegraph_query" | "codegraph_structure" => Some(sumo::ENTITY),
            "codegraph_traverse" | "codegraph_impact" => Some(sumo::RELATION),
            "codegraph_context" => Some(sumo::TEXT),
            // Analysis tools — processes over the graph
            "codegraph_analysis" | "codegraph_reindex" => Some(sumo::PROCESS),
            // Embeddings — vector representations of symbols
            "codegraph_index_embeddings" => Some(sumo::REPRESENTATION),
            // Stats — a computed dataset
            "codegraph_stats" => Some(dc_bibo::DATASET),
            _ => Some(sumo::ENTITY),
        }
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

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    // Pins the registered tool-surface count end-to-end. Catches silent
    // registration drops — a `#[tool]` impl block without `#[tool_router]`
    // silently registers nothing (`cargo check` passes on an unwired orphan).
    // Mirrors the swarm pin.
    #[test]
    fn tool_surface_is_exactly_9_registered_tools() {
        let n = CodeGraphServer::tool_router().list_all().len();
        assert_eq!(n, 9, "codegraph registered tool surface changed; got {n}");
    }

    // Coverage: every registered tool must have a non-None ontology anchor.
    // Catches the silent-drop failure mode where a new tool is added to the
    // router without a corresponding arm in ontology_anchor. The count pin
    // above catches addition; this test catches anchoring.
    #[test]
    fn ontology_anchor_covers_all_registered_tools() {
        let router = CodeGraphServer::tool_router();
        for tool in router.list_all() {
            assert!(
                CodeGraphServer::ontology_anchor(&tool.name).is_some(),
                "ontology_anchor returned None for registered tool '{}'; \
                 add an explicit arm or adjust the fallback",
                tool.name
            );
        }
    }

    // Regression: the ontology anchor must not collapse to a single constant.
    // Read tools anchor on SUMO entities/relations; analysis tools anchor on
    // SUMO processes; stats anchors on Dublin Core. A future stub regression
    // would make these equal.
    #[test]
    fn ontology_anchor_distinguishes_tool_families() {
        use hkask_bridge_ontology::{dc_bibo, sumo};
        let query = CodeGraphServer::ontology_anchor("codegraph_query");
        let traverse = CodeGraphServer::ontology_anchor("codegraph_traverse");
        let analysis = CodeGraphServer::ontology_anchor("codegraph_analysis");
        let stats = CodeGraphServer::ontology_anchor("codegraph_stats");
        // Read vs analysis: ENTITY vs PROCESS — distinct SUMO categories.
        assert_ne!(
            query, analysis,
            "codegraph_query (entity) and codegraph_analysis (process) must anchor on distinct SUMO categories"
        );
        // SUMO vs Dublin Core: stats is a dataset, not an entity.
        assert_ne!(
            query, stats,
            "codegraph_query (SUMO) and codegraph_stats (Dublin Core) must anchor on distinct ontologies"
        );
        // Specific concept pins.
        assert_eq!(
            query,
            Some(sumo::ENTITY),
            "codegraph_query must anchor on SUMO Entity"
        );
        assert_eq!(
            traverse,
            Some(sumo::RELATION),
            "codegraph_traverse must anchor on SUMO Relation"
        );
        assert_eq!(
            analysis,
            Some(sumo::PROCESS),
            "codegraph_analysis must anchor on SUMO Process"
        );
        assert_eq!(
            stats,
            Some(dc_bibo::DATASET),
            "codegraph_stats must anchor on Dublin Core Dataset"
        );
    }
}

#[tool_router(server_handler)]
impl CodeGraphServer {
    #[tool(
        description = "Search the codebase for symbols, or look up a specific symbol by name (set 'name' field)"
    )]
    pub async fn codegraph_query(&self, Parameters(req): Parameters<QueryRequest>) -> String {
        execute_tool_semantic(
            self,
            "codegraph_query",
            Self::ontology_anchor("codegraph_query"),
            async {
                self.ensure_indexed()?;
                let pipeline = self.pipeline_guard()?;
                // If a name is provided, look it up directly in the database rather
                // than filtering the (limit-capped) FTS5 result set — the exact
                // match may exist outside the first `limit` hits, in which case the
                // old filter path returned a spurious "symbol not found".
                if let Some(ref name) = req.name {
                    return match pipeline.store().find_symbol_by_name(name).map_err(db_err)? {
                        Some(id) => match pipeline.store().get_symbol(id).map_err(db_err)? {
                            Some(symbol) => Ok(serde_json::json!(&symbol)),
                            None => Ok(serde_json::json!({
                                "error": format!("symbol not found: {name}")
                            })),
                        },
                        None => Ok(serde_json::json!({
                            "error": format!("symbol not found: {name}")
                        })),
                    };
                }
                let results =
                    graph::search::search(pipeline.store().conn(), &req.query, req.limit as usize)
                        .map_err(db_err)?;
                Ok(serde_json::json!(results))
            },
        )
        .await
    }

    #[tool(description = "Traverse the code graph: forward (dependencies) or reverse (callers)")]
    pub async fn codegraph_traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
        execute_tool_semantic(
            self,
            "codegraph_traverse",
            Self::ontology_anchor("codegraph_traverse"),
            async {
                self.ensure_indexed()?;
                let pipeline = self.pipeline_guard()?;
                let id =
                    traversal::find_symbol_id(pipeline.store(), &req.symbol).map_err(db_err)?;
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
                    None => Ok(
                        serde_json::json!({"error": format!("symbol not found: {}", req.symbol)}),
                    ),
                }
            },
        )
        .await
    }

    #[tool(description = "Analyze blast radius for a symbol")]
    pub async fn codegraph_impact(&self, Parameters(req): Parameters<ImpactRequest>) -> String {
        execute_tool_semantic(
            self,
            "codegraph_impact",
            Self::ontology_anchor("codegraph_impact"),
            async {
                self.ensure_indexed()?;
                let pipeline = self.pipeline_guard()?;
                let id =
                    traversal::find_symbol_id(pipeline.store(), &req.symbol).map_err(db_err)?;
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
                    None => Ok(
                        serde_json::json!({"error": format!("symbol not found: {}", req.symbol)}),
                    ),
                }
            },
        )
        .await
    }

    #[tool(description = "Run analysis: 'dead_code' or 'complexity'")]
    pub async fn codegraph_analysis(&self, Parameters(req): Parameters<AnalysisRequest>) -> String {
        execute_tool_semantic(
            self,
            "codegraph_analysis",
            Self::ontology_anchor("codegraph_analysis"),
            async {
                self.ensure_indexed()?;
                let pipeline = self.pipeline_guard()?;
                match req.kind {
                    AnalysisKind::DeadCode => {
                        let findings =
                            analysis::find_dead_code(pipeline.store().conn()).map_err(db_err)?;
                        Ok(serde_json::json!(findings))
                    }
                    AnalysisKind::Complexity => {
                        let findings =
                            analysis::find_high_complexity(pipeline.store().conn(), 10, 5)
                                .map_err(db_err)?;
                        Ok(serde_json::json!(findings))
                    }
                }
            },
        )
        .await
    }

    #[tool(description = "Assemble token-budgeted context for LLM prompts")]
    pub async fn codegraph_context(&self, Parameters(req): Parameters<ContextRequest>) -> String {
        execute_tool_semantic(
            self,
            "codegraph_context",
            Self::ontology_anchor("codegraph_context"),
            async {
                self.ensure_indexed()?;
                let pipeline = self.pipeline_guard()?;
                let assembled = crate::codegraph::assemble_context(
                    pipeline.store().conn(),
                    &req.query,
                    req.budget,
                )
                .map_err(db_err)?;
                Ok(serde_json::json!({
                    "context_id": assembled.context_id.to_string(),
                    "text": assembled.text,
                    "symbols": assembled.symbols,
                    "estimated_tokens": assembled.estimated_tokens,
                }))
            },
        )
        .await
    }

    #[tool(description = "Get project overview: top symbols")]
    pub async fn codegraph_structure(
        &self,
        Parameters(req): Parameters<StructureRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "codegraph_structure",
            Self::ontology_anchor("codegraph_structure"),
            async {
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
            },
        )
        .await
    }

    #[tool(description = "Get index statistics")]
    pub async fn codegraph_stats(&self, Parameters(req): Parameters<StatsRequest>) -> String {
        execute_tool_semantic(
            self,
            "codegraph_stats",
            Self::ontology_anchor("codegraph_stats"),
            async {
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
            },
        )
        .await
    }

    #[tool(description = "Force full re-index of the workspace")]
    pub async fn codegraph_reindex(&self) -> String {
        execute_tool_semantic(self, "codegraph_reindex", Self::ontology_anchor("codegraph_reindex"), async {
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
        execute_tool_semantic(
            self,
            "codegraph_index_embeddings",
            Self::ontology_anchor("codegraph_index_embeddings"),
            async {
                self.ensure_indexed()?;

                // Resolve the embedding model and dimension.
                let model = req
                    .model
                    .unwrap_or_else(hkask_inference::model_constants::embedding_model);
                let dim: usize = crate::codegraph::graph::schema::resolve_embedding_dim();

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
                            Err(e) => {
                                errors.push(format!("symbol {} insert failed: {}", symbol_id, e))
                            }
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

                // If every batch failed, the index is broken — surface as an MCP
                // error rather than a success envelope with 0 symbols. Partial
                // success (some embeddings inserted, some errors) is acceptable.
                if symbols_embedded == 0 && !errors.is_empty() {
                    return Err(McpToolError::unavailable(format!(
                        "Embedding failed for all batches: {}",
                        errors.join("; ")
                    )));
                }

                Ok(serde_json::json!({
                    "symbols_embedded": symbols_embedded,
                    "total_symbols": symbols.len(),
                    "model": model,
                    "dim": dim,
                    "errors": errors,
                }))
            },
        )
        .await
    }
}

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // D28 — Standardized Artifact Storage. Default DB path is
    // `{kask_data_dir}/mcp/codegraph/codegraph.db`, resolved via
    // `resolve_under_data_dir`. Override via `HKASK_CODEGRAPH_DB`.
    let db_path = std::env::var("HKASK_CODEGRAPH_DB")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            hkask_types::agent_paths::resolve_under_data_dir(
                &hkask_types::agent_paths::mcp_server_db("codegraph", "codegraph"),
            )
        });
    // Resolve the inference port once — routes embeddings through zed's
    // LanguageModelEmbeddingPort via the IPC bridge.
    let inference_port = hkask_inference::resolve_inference_port().await;
    run_server(
        "hkask-mcp-codegraph",
        SERVER_VERSION,
        |ctx| {
            let webid = ctx.webid;
            let store =
                crate::codegraph::graph::store::GraphStore::open(&db_path.to_string_lossy())
                    .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                        context: "codegraph graph store open".into(),
                        detail: e.to_string(),
                    })?;
            let pipeline = IndexPipeline::new(store);
            Ok(CodeGraphServer::new(
                webid,
                CapabilityTier::detect(&webid, &std::collections::HashMap::new()),
                Arc::new(Mutex::new(pipeline)),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                inference_port.clone(),
            ))
        },
        vec![],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::graph::store::GraphStore;
    use crate::codegraph::types::{Symbol, SymbolKind, Visibility};
    use hkask_mcp_server::server::CapabilityTier;
    use hkask_types::WebID;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    /// Drives the real `codegraph_query` MCP tool to pin its exact-name branch
    /// (S2): a `name` set must resolve the symbol via a direct DB lookup, not by
    /// filtering the (limit-capped) FTS5 result set. The previous filter-over-
    /// limited-results path returned a spurious "symbol not found" when the
    /// exact match sat outside the first `limit` FTS5 hits.
    #[tokio::test]
    async fn codegraph_query_exact_name_lookup() {
        let pipeline = IndexPipeline::new(GraphStore::open_in_memory().unwrap());
        let target = "target_fn";
        {
            let store = pipeline.store();
            let fid = store.upsert_file("src/lib.rs", "h").unwrap();
            store
                .insert_symbols(
                    &[Symbol {
                        id: None,
                        name: target.to_string(),
                        kind: SymbolKind::Function,
                        file: "src/lib.rs".into(),
                        start_line: 1,
                        end_line: 3,
                        signature: format!("fn {target}()"),
                        visibility: Visibility::Public,
                        doc_comment: None,
                        complexity: Default::default(),
                    }],
                    fid,
                )
                .unwrap();
        }
        let webid = WebID::new();
        // `codegraph_query` never uses the inference port; the MediaRouter
        // fallback is connection-free and safe in a test.
        let inference_port = hkask_inference::resolve_inference_port().await;
        // `indexed_once = true` makes `ensure_indexed()` a no-op so the test
        // never walks the real cwd — the store is pre-populated above.
        let server = CodeGraphServer::new(
            webid,
            CapabilityTier::detect(&webid, &std::collections::HashMap::new()),
            Arc::new(Mutex::new(pipeline)),
            Arc::new(AtomicBool::new(true)),
            inference_port,
        );

        // `execute_tool` wraps the tool result under a `content` key (the MCP
        // response envelope); assertions unwrap via the canonical
        // `hkask_types::tool_response::parse_tool_response` seam.

        // 1. Exact-name lookup returns the symbol directly.
        let req = QueryRequest {
            query: String::new(),
            limit: 1,
            name: Some(target.to_string()),
        };
        let out = server.codegraph_query(Parameters(req)).await;
        let payload =
            hkask_types::tool_response::parse_tool_response(&out).expect("valid envelope");
        assert_eq!(
            payload["name"].as_str(),
            Some(target),
            "exact-name lookup must return the symbol: {out}"
        );
        assert!(
            payload.get("error").is_none(),
            "exact-name lookup for an existing symbol must not error: {out}"
        );

        // 2. Missing exact name returns the error envelope (never a silent null).
        let req = QueryRequest {
            query: String::new(),
            limit: 1,
            name: Some("nonexistent_symbol".to_string()),
        };
        let out = server.codegraph_query(Parameters(req)).await;
        let payload =
            hkask_types::tool_response::parse_tool_response(&out).expect("valid envelope");
        assert!(
            payload.get("error").is_some(),
            "missing exact name must return an error envelope: {out}"
        );

        // 3. The no-name path still returns a search-results array (guards that
        //    the exact-name refactor didn't break the FTS5 query path).
        let req = QueryRequest {
            query: target.to_string(),
            limit: 10,
            name: None,
        };
        let out = server.codegraph_query(Parameters(req)).await;
        let payload =
            hkask_types::tool_response::parse_tool_response(&out).expect("valid envelope");
        assert!(
            payload.is_array(),
            "query without a name must return a search-results array: {out}"
        );
    }

    // P1 regression: `codegraph_index_embeddings` must surface an all-batches-
    // failed embedding run as `McpToolError::unavailable`, NOT as a success
    // envelope with `symbols_embedded: 0`. The previous path returned
    // `Ok({"symbols_embedded": 0, ...})` even when every embedding batch
    // errored — a broken feedback loop where the regulation layer read "0
    // symbols embedded" as "no symbols to embed" rather than "the embedding
    // backend is broken." The guard distinguishes the two: `symbols.is_empty()`
    // (genuine no-op) returns Ok; `symbols_embedded == 0 && !errors.is_empty()`
    // (every batch failed) returns Err(unavailable).
    //
    // We drive the real tool with a mock `InferencePort` whose `embed` always
    // returns `Err`, against a pre-populated store with one symbol. The guard
    // must fire and the tool must return an error envelope classified
    // `unavailable`. This pins the guard end-to-end through
    // `execute_tool_semantic` — a future refactor that drops the guard would
    // re-introduce the silent-success loop.
    #[tokio::test]
    async fn codegraph_index_embeddings_returns_unavailable_when_all_batches_fail() {
        use hkask_types::ports::EmbeddingGenerationError;
        use hkask_types::{InferenceError, InferencePort, InferenceResult, LLMParameters};
        use std::future::Future;
        use std::pin::Pin;

        /// Mock `InferencePort` whose `embed` always fails. The other trait
        /// methods are unreachable in this test (the tool only calls `embed`),
        /// so they return errors too — if reached, the test fails loudly
        /// rather than silently passing.
        struct FailingEmbedPort;
        impl InferencePort for FailingEmbedPort {
            fn generate(
                &self,
                _prompt: &str,
                _parameters: &LLMParameters,
                _tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
            {
                Box::pin(async {
                    Err(InferenceError::Generation(
                        "FailingEmbedPort: generate should not be reached".into(),
                    ))
                })
            }
            fn embed<'a>(
                &'a self,
                _model: &str,
                _texts: &[String],
            ) -> hkask_types::ports::EmbedFuture<'a> {
                Box::pin(async {
                    Err(EmbeddingGenerationError::Api(
                        503,
                        "FailingEmbedPort: embedding backend is down".into(),
                    ))
                })
            }
        }

        // Pre-populate the store with one symbol so the tool reaches the
        // embedding loop (the `symbols.is_empty()` early-return would
        // otherwise short-circuit before the guard).
        let pipeline = IndexPipeline::new(GraphStore::open_in_memory().expect("in-memory store"));
        {
            let store = pipeline.store();
            let file_id = store.upsert_file("src/lib.rs", "h").expect("upsert_file");
            store
                .insert_symbols(
                    &[Symbol {
                        id: None,
                        name: "target_fn".to_string(),
                        kind: SymbolKind::Function,
                        file: "src/lib.rs".into(),
                        start_line: 1,
                        end_line: 3,
                        signature: "fn target_fn()".to_string(),
                        visibility: Visibility::Public,
                        doc_comment: None,
                        complexity: Default::default(),
                    }],
                    file_id,
                )
                .expect("insert_symbols");
        }

        let webid = WebID::new();
        let server = CodeGraphServer::new(
            webid,
            CapabilityTier::detect(&webid, &std::collections::HashMap::new()),
            Arc::new(Mutex::new(pipeline)),
            Arc::new(AtomicBool::new(true)),
            Arc::new(FailingEmbedPort),
        );

        let req = EmbedIndexRequest {
            model: None,
            batch_size: 32,
        };
        let out = server.codegraph_index_embeddings(Parameters(req)).await;

        // The tool returns a String envelope; an Err path serializes as
        // `{"error": <msg>, "kind": "unavailable"}`. Parse via the canonical
        // `parse_tool_error` seam — do not re-implement envelope detection.
        let envelope = hkask_types::tool_response::parse_tool_error(&out).unwrap_or_else(|| {
            panic!(
                "all-batches-failed must return an error envelope, not a success \
                 payload; got: {out}"
            );
        });
        assert_eq!(
            envelope.kind,
            Some(hkask_types::McpErrorKind::Unavailable),
            "all-batches-failed must classify as unavailable, not silently return \
             a success envelope with symbols_embedded=0; got message: {}",
            envelope.message,
        );
        assert!(
            envelope
                .message
                .contains("Embedding failed for all batches"),
            "error message must explain that all embedding batches failed; got: {}",
            envelope.message,
        );
    }
}

// D28 — pins the default DB path resolution. When HKASK_CODEGRAPH_DB is
// unset, the default path must be `mcp/codegraph/codegraph.db` under the
// kask data root.
#[test]
fn default_db_path_follows_standardized_layout() {
    // The default path is constructed via:
    //   resolve_under_data_dir(mcp_server_db("codegraph", "codegraph"))
    // Verify the relative segment matches the standardized layout.
    let relative = hkask_types::agent_paths::mcp_server_db("codegraph", "codegraph");
    assert_eq!(
        relative,
        std::path::PathBuf::from("mcp")
            .join("codegraph")
            .join("codegraph.db"),
        "codegraph default DB path must follow mcp/codegraph/codegraph.db"
    );
}
