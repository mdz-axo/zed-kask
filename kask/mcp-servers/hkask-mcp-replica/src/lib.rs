#![forbid(unsafe_code)]
//! hkask-mcp-replica — Build and compose authorial style replicas (library).
//!
//! Exports ReplicaServer struct and tool methods for fuzz testability (P5 Testing
//! Discipline, P4 Clear Boundaries). The binary entrypoint in main.rs delegates
//! to `run()`.
//!
//! Tools:
//! - replica_build    — embed a corpus, create a style replica
//! - replica_compose  — generate prose in an author's style
//! - replica_mashup   — blend two authors' styles via centroid interpolation
//! - replica_compare  — measure stylistic distance between two authors
//! - replica_registry — list, inspect, and manage built replicas
//! - replica_explain  — explain centroids and style-space topology
//! - replica_discover — discover an academic author's work and generate corpus.yaml
//! - replica_cache_work — cache extracted work content to disk

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod golem;
pub mod types;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_inference::EmbeddingRouter;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_services_compose::cosine_distance;
use hkask_services_core::HkaskSettings;
use hkask_services_corpus::{EmbedProgress, EmbedService};
use hkask_services_inference::InferenceContext;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{Database, EmbeddingStore};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// MCP tool parameter struct for replica_compose.
/// Separate from `ComposeRequest` to avoid requiring `InferenceContext` serialization.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplicaComposeParams {
    pub author: String,
    pub prompt: String,
    pub db_path: String,
    pub passphrase: String,
    #[serde(default)]
    pub no_validate: bool,
}
use types::*;

/// Default embedding model (DeepInfra Qwen3-Embedding-0.6B).
/// Override via settings.json or HKASK_EMBEDDING_MODEL env var.
fn embedding_model() -> String {
    HkaskSettings::load().embedding_model()
}

/// Default generation model for prose composition — from InferenceConfig.
fn generation_model() -> String {
    hkask_inference::InferenceConfig::from_env().default_model
}

fn inference_config() -> hkask_inference::InferenceConfig {
    hkask_inference::InferenceConfig::from_env()
}

fn database_passphrase() -> Result<String, McpToolError> {
    hkask_keystore::keychain::resolve_db_passphrase_string()
        .map(|value| value.to_string())
        .map_err(|e| McpToolError::failed_precondition(e.to_string()))
}

hkask_mcp_server::mcp_server!(
    pub struct ReplicaServer;
);

// ── Response types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct BuildResult {
    author: String,
    purged: usize,
    total_passages: usize,
    centroid_ref: String,
    centroid_stored: bool,
    passage_count: usize,
    budget: usize,
    tagged_passages: usize,
    triples_stored: usize,
    embedding_only: usize,
}

#[derive(Debug, Serialize)]
struct ComposeResult {
    prose: String,
    exemplar_count: usize,
    centroid_distance: Option<f64>,
    style_passed: Option<bool>,
}

fn qualitative_label(distance: f64) -> String {
    if distance < 0.20 {
        "Excellent".to_string()
    } else if distance < 0.40 {
        "Good".to_string()
    } else if distance < 0.60 {
        "Fair".to_string()
    } else {
        "Needs Work".to_string()
    }
}

fn is_centroid_entity(entity_ref: &str) -> bool {
    // Match composite centroid (style:persona:centroid) and
    // per-dimension centroids (style:persona:dimension-centroid)
    if let Some(last) = entity_ref.rsplit(':').next() {
        last == "centroid" || last.ends_with("-centroid")
    } else {
        false
    }
}

#[derive(Debug, Serialize)]
struct DimensionScore {
    /// Dimension name: "Gentle", "Schriver", "Hopper", "Lovelace".
    dimension_name: String,
    /// Entity ref in the embedding store (e.g., "style:gentle-lovelace:gentle-centroid").
    centroid_ref: String,
    /// Human-readable description of what this dimension measures.
    description: String,
    /// Cosine distance from document to dimension centroid (lower = stronger alignment).
    cosine_distance: f64,
    /// Number of passages used to compute this centroid.
    passage_count: usize,
    /// Qualitative rating: "Excellent", "Good", "Fair", or "Needs Work".
    qualitative: String,
}

#[derive(Debug, Serialize)]
struct PersonaCompareResult {
    /// Persona name (e.g., "gentle-lovelace").
    persona: String,
    /// Comparison mode used: "per-dimension" or "composite".
    compare_mode: String,
    /// Embedding model used for document embedding.
    embedding_model: String,
    /// Composite weighted centroid score (always present).
    composite_score: Option<DimensionScore>,
    /// Per-dimension scores (only in "per-dimension" mode).
    dimension_scores: Vec<DimensionScore>,
    /// Elapsed time for the comparison.
    elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
struct AuthorInfo {
    name: String,
    centroid_ref: String,
    passage_count: usize,
}

#[derive(Debug, Serialize)]
struct CompareResult {
    authors: Vec<AuthorInfo>,
    distances: Vec<AuthorDistance>,
}

#[derive(Debug, Serialize)]
struct AuthorDistance {
    author_a: String,
    author_b: String,
    cosine_distance: f64,
    compatible: bool,
}

#[derive(Debug, Serialize)]
struct MashupResult {
    prose: String,
    exemplar_count: usize,
    blend_ratio: f64,
    blended_centroid_ref: String,
    centroid_distance: Option<f64>,
    distance_a: f64,
    distance_b: f64,
}

#[derive(Debug, Serialize)]
struct RegistryEntry {
    name: String,
    centroid_ref: String,
    passage_count: usize,
}

#[derive(Debug, Serialize)]
struct RegistryResult {
    entries: Vec<RegistryEntry>,
    message: String,
}

// ── Replica Discovery types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct DiscoverResult {
    /// The manifest ID to execute for discovery
    manifest_id: String,
    /// Parameters forwarded to the manifest
    parameters: serde_json::Value,
    /// Human-readable summary of what will happen
    summary: String,
    /// Estimated phases
    phases: Vec<DiscoverPhase>,
}

#[derive(Debug, Serialize)]
struct DiscoverPhase {
    ordinal: u32,
    name: String,
    description: String,
    sources: Vec<String>,
}

// ── Cache Work types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct CacheWorkResult {
    slug: String,
    path: String,
    bytes_written: u64,
}

// ── Pipeline request types ───────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PipelineRunRequest {
    /// Path to the PipelineManifest YAML file
    pub manifest_path: String,
}

// ── Server implementation ───────────────────────────────────────────────────

#[tool_router(server_handler)]
impl ReplicaServer {
    #[tool(
        description = "Embed a style corpus and create an authorial replica. Downloads public domain texts, chunks them, generates embeddings, and computes a style centroid."
    )]
    pub async fn replica_build(&self, Parameters(params): Parameters<BuildRequest>) -> String {
        let config_path = PathBuf::from(&params.config_path);

        execute_tool(self, "replica_build", async {
            if !config_path.exists() {
                return Err(McpToolError::invalid_argument(format!(
                    "Config file not found: {}",
                    params.config_path
                )));
            }

            let progress = Arc::new(|p: &EmbedProgress| {
                tracing::info!(
                    target: "hkask.mcp.replica",
                    phase = ?p.phase,
                    author = %p.author,
                    work = %p.current_work,
                    done = p.completed_passages,
                    total = p.total_passages,
                    "Embedding progress"
                );
            });

            let passphrase = match params.passphrase {
                Some(passphrase) => passphrase,
                None => database_passphrase()?,
            };
            let result = EmbedService::embed_corpus(
                &config_path,
                &params.db_path,
                &passphrase,
                None,
                Some(progress),
            )
            .await
            .map_err(|e| McpToolError::internal(e.to_string()))?;

            let json_str = serde_json::to_string(&BuildResult {
                author: result.author,
                purged: result.purged,
                total_passages: result.total_passages,
                centroid_ref: result.centroid_ref,
                centroid_stored: result.centroid_stored,
                passage_count: result.passage_count,
                budget: result.budget,
                tagged_passages: result.tagged_passages,
                triples_stored: result.triples_stored,
                embedding_only: result.embedding_only,
            })
            .map_err(|e| McpToolError::internal(e.to_string()))?;

            let parsed: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
            Ok(parsed)
        })
        .await
    }

    #[tool(description = "Generate prose in an author's style.")]
    pub async fn replica_compose(
        &self,
        Parameters(params): Parameters<ReplicaComposeParams>,
    ) -> String {
        execute_tool(self, "replica_compose", async {
            let model = embedding_model();
            let gen_model = generation_model();
            let inf_cfg = inference_config();
            let config = hkask_services_compose::CognitionConfig {
                author: params.author.clone(),
                jinja2_template: None,
                embedding: hkask_services_compose::EmbeddingSection {
                    model: model.clone(),
                    dim: 1024,
                    centroid_entity_ref: format!("style:{}:centroid", params.author),
                    retrieval: Default::default(),
                },
                validation: hkask_services_compose::ValidationSection {
                    centroid_distance_max: 0.25,
                },
            };

            let inference_ctx = InferenceContext::from_parts(None, &gen_model, inf_cfg);

            let request = hkask_services_compose::ComposeRequest {
                prompt: params.prompt,
                db_path: PathBuf::from(&params.db_path),
                db_passphrase: params.passphrase,
                cognition: config,
                inference_ctx,
                no_validate: params.no_validate,
            };

            let result = hkask_services_compose::ComposeService::compose(request)
                .await
                .map_err(|e| McpToolError::internal(e.to_string()))?;

            let json_str = serde_json::to_string(&ComposeResult {
                prose: result.generated_prose,
                exemplar_count: result.exemplar_count,
                centroid_distance: result.validation.as_ref().map(|v| v.distance),
                style_passed: result.validation.map(|v| v.passed),
            })
            .map_err(|e| McpToolError::internal(e.to_string()))?;

            let parsed: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
            Ok(parsed)
        })
        .await
    }

    #[tool(
        description = "Rewrite a passage or code snippet in an author's style, optimized for a specific quality dimension (gentle/schriver/hopper/lovelace/composite). Delegates to replica_compose with dimension-specific guidance."
    )]
    pub async fn replica_rewrite(&self, Parameters(params): Parameters<RewriteRequest>) -> String {
        execute_tool(self, "replica_rewrite", async {
            let dimension_guidance = match params.dimension.to_lowercase().as_str() {
                "gentle" => {
                    "Rewrite this text to maximize agent-correctness. Docs ARE code — ensure every statement is actionable and unambiguous. Remove any stale references or outdated information."
                }
                "schriver" => {
                    "Rewrite this text for maximum findability. Use scannable headings, descriptive hyperlinks, and front-load key concepts. A reader must find their answer within 30 seconds."
                }
                "hopper" => {
                    "Rewrite this text for maximum accessibility. Make it comprehensible on first reading with zero prior context. Use plain language, active voice, and short sentences."
                }
                "lovelace" => {
                    "Rewrite this text for maximum precision. Make every specification independently verifiable — a reader must be able to write a test from this text alone."
                }
                _ => {
                    "Rewrite this text for all four dimensions of documentation excellence: agent-correctness (Gentle), findability (Schriver), accessibility (Hopper), and precision (Lovelace)."
                }
            };

            let prompt = format!("{dimension_guidance}\n\n=== TEXT TO REWRITE ===\n\n{}", params.content);

            let centroid_ref = if params.dimension.to_lowercase() == "composite" {
                format!("style:{}:centroid", params.author)
            } else {
                format!(
                    "style:{}:{}-centroid",
                    params.author,
                    params.dimension.to_lowercase()
                )
            };

            let model = embedding_model();
            let gen_model = generation_model();
            let inf_cfg = inference_config();
            let config = hkask_services_compose::CognitionConfig {
                author: params.author.clone(),
                jinja2_template: None,
                embedding: hkask_services_compose::EmbeddingSection {
                    model: model.clone(),
                    dim: 1024,
                    centroid_entity_ref: centroid_ref,
                    retrieval: Default::default(),
                },
                validation: hkask_services_compose::ValidationSection {
                    centroid_distance_max: 0.40,
                },
            };

            let inference_ctx = InferenceContext::from_parts(None, &gen_model, inf_cfg);

            let request = hkask_services_compose::ComposeRequest {
                prompt,
                db_path: PathBuf::from(&params.db_path),
                db_passphrase: params.passphrase,
                cognition: config,
                inference_ctx,
                no_validate: params.no_validate,
            };

            let result = hkask_services_compose::ComposeService::compose(request)
                .await
                .map_err(|e| McpToolError::internal(e.to_string()))?;

            let json_str = serde_json::to_string(&serde_json::json!({
                "rewritten": result.generated_prose,
                "dimension": params.dimension,
                "author": params.author,
                "exemplar_count": result.exemplar_count,
                "centroid_distance": result.validation.as_ref().map(|v| v.distance),
                "style_passed": result.validation.map(|v| v.passed),
            }))
            .map_err(|e| McpToolError::internal(e.to_string()))?;

            let parsed: Value = serde_json::from_str(&json_str).unwrap_or(json!({"error": "serialization failed"}));
            Ok(parsed)
        })
        .await
    }

    #[tool(
        description = "Compare all built author replicas, or evaluate a document against a persona's centroids."
    )]
    pub async fn replica_compare(&self, Parameters(params): Parameters<CompareRequest>) -> String {
        let persona = params.persona.clone();
        let document_content = params.document_content.clone();

        execute_tool(self, "replica_compare", async {
            let db = Database::open(&params.db_path, &params.passphrase)
                .map_err(|e| McpToolError::internal(e.to_string()))?;
            let pool = db
                .sqlite_pool()
                .map_err(|e| McpToolError::internal(format!("pool: {e}")))?;
            let store = EmbeddingStore::from_driver(Arc::new(SqliteDriver::new(pool)), 1024);

            // ── Document comparison path ──────────────────────────────
            if let Some(ref doc_text) = document_content {
                let started = Instant::now();

                // Embed the document
                let emb_model = embedding_model();
                let inf_cfg = inference_config();
                let embedder = EmbeddingRouter::new(inf_cfg);
                let vectors = embedder
                    .embed_sentences(&emb_model, &[doc_text.as_str()])
                    .await
                    .map_err(|e| {
                        McpToolError::internal(format!("Failed to embed document: {e}"))
                    })?;
                let doc_vec = vectors
                    .first()
                    .ok_or_else(|| McpToolError::internal("Embedding returned empty result"))?;

                // Query centroids for this persona
                let prefix = format!("style:{}:", persona.as_deref().unwrap_or(""));
                let all_refs = store
                    .query_by_prefix(&prefix)
                    .map_err(|e| McpToolError::internal(e.to_string()))?;

                // Count non-centroid passages for passage_count on each centroid
                let total_passages = all_refs.iter().filter(|r| !is_centroid_entity(r)).count();

                let mut dimension_scores: Vec<DimensionScore> = Vec::new();
                let mut composite_score: Option<DimensionScore> = None;

                for entity_ref in &all_refs {
                    if !is_centroid_entity(entity_ref) {
                        continue;
                    }

                    let emb = store
                        .get(entity_ref)
                        .map_err(|e| McpToolError::internal(e.to_string()))?;
                    let dist = cosine_distance(doc_vec, &emb.vector);

                    // Derive dimension name from entity_ref.
                    // "style:{persona}:{dimension}-centroid" → dimension name
                    // "style:{persona}:centroid" → composite
                    let last_segment = entity_ref.rsplit(':').next().unwrap_or(entity_ref);

                    let (dimension_name, is_composite) = if last_segment == "centroid" {
                        ("composite".to_string(), true)
                    } else if let Some(dim) = last_segment.strip_suffix("-centroid") {
                        let mut chars = dim.chars();
                        let capitalized = match chars.next() {
                            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                            None => dim.to_string(),
                        };
                        (capitalized, false)
                    } else {
                        continue;
                    };

                    let dim_lower = dimension_name.to_lowercase();
                    let dim_passage_count = all_refs
                        .iter()
                        .filter(|r| !is_centroid_entity(r) && r.to_lowercase().contains(&dim_lower))
                        .count();

                    let score = DimensionScore {
                        centroid_ref: entity_ref.clone(),
                        cosine_distance: dist,
                        qualitative: qualitative_label(dist),
                        passage_count: if is_composite {
                            total_passages
                        } else {
                            dim_passage_count
                        },
                        dimension_name: dimension_name.clone(),
                        description: String::new(),
                    };

                    if is_composite {
                        composite_score = Some(score);
                    } else {
                        dimension_scores.push(score);
                    }
                }

                let result = PersonaCompareResult {
                    persona: persona.unwrap_or_default(),
                    compare_mode: params.compare_mode.clone(),
                    embedding_model: emb_model,
                    composite_score,
                    dimension_scores: if params.compare_mode == "composite" {
                        Vec::new()
                    } else {
                        dimension_scores
                    },
                    elapsed_ms: started.elapsed().as_millis() as u64,
                };

                return serde_json::to_value(&result)
                    .map_err(|e| McpToolError::internal(e.to_string()));
            }

            // ── Pairwise author comparison path (backward compat) ─────
            let centroids = store
                .query_by_prefix("style:")
                .map_err(|e| McpToolError::internal(e.to_string()))?;

            let mut author_names: Vec<String> = Vec::new();
            let mut author_info: Vec<AuthorInfo> = Vec::new();

            for entity_ref in &centroids {
                if entity_ref.ends_with(":centroid") {
                    let parts: Vec<&str> = entity_ref.split(':').collect();
                    if parts.len() >= 3 {
                        let name = parts[1].to_string();
                        if name.contains(':') {
                            continue;
                        }
                        let prefix = format!("style:{}:", name);
                        let refs = store
                            .query_by_prefix(&prefix)
                            .map_err(|e| McpToolError::internal(e.to_string()))?;
                        let passage_count =
                            refs.iter().filter(|r| !r.ends_with(":centroid")).count();
                        author_names.push(name.clone());
                        author_info.push(AuthorInfo {
                            name,
                            centroid_ref: entity_ref.clone(),
                            passage_count,
                        });
                    }
                }
            }

            let mut distances: Vec<AuthorDistance> = Vec::new();
            for i in 0..author_names.len() {
                for j in (i + 1)..author_names.len() {
                    let ca = format!("style:{}:centroid", author_names[i]);
                    let cb = format!("style:{}:centroid", author_names[j]);
                    if let (Ok(a), Ok(b)) = (store.get(&ca), store.get(&cb)) {
                        let dist = cosine_distance(&a.vector, &b.vector);
                        distances.push(AuthorDistance {
                            author_a: author_names[i].clone(),
                            author_b: author_names[j].clone(),
                            cosine_distance: dist,
                            compatible: dist < 0.30,
                        });
                    }
                }
            }

            serde_json::to_value(&CompareResult {
                authors: author_info,
                distances,
            })
            .map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(description = "Generate prose blending two authors' styles.")]
    pub async fn replica_mashup(&self, Parameters(params): Parameters<MashupRequest>) -> String {
        execute_tool(self, "replica_mashup", async {
            let blend = params.blend.clamp(0.0, 1.0);
            let centroid_a_ref = format!("style:{}:centroid", params.author_a);
            let centroid_b_ref = format!("style:{}:centroid", params.author_b);
            let blended_ref = format!(
                "style:mashup:{}:{}:centroid",
                params.author_a, params.author_b
            );

            let db = Database::open(&params.db_path, &params.passphrase)
                .map_err(|e| McpToolError::internal(e.to_string()))?;
            let pool = db
                .sqlite_pool()
                .map_err(|e| McpToolError::internal(format!("pool: {e}")))?;
            let store = EmbeddingStore::from_driver(Arc::new(SqliteDriver::new(pool)), 1024);

            let emb_a = store.get(&centroid_a_ref).map_err(|_| {
                McpToolError::invalid_argument(format!(
                    "Author '{}' not found. Run replica_build first.",
                    params.author_a
                ))
            })?;
            let emb_b = store.get(&centroid_b_ref).map_err(|_| {
                McpToolError::invalid_argument(format!(
                    "Author '{}' not found. Run replica_build first.",
                    params.author_b
                ))
            })?;

            let blended: Vec<f32> = emb_a
                .vector
                .iter()
                .zip(emb_b.vector.iter())
                .map(|(a, b)| a * (1.0 - blend as f32) + b * blend as f32)
                .collect();

            let dist_a = hkask_services_compose::cosine_distance(&blended, &emb_a.vector);
            let dist_b = hkask_services_compose::cosine_distance(&blended, &emb_b.vector);

            let model = embedding_model();
            let gen_model = generation_model();
            store
                .store(&blended_ref, &blended, &model)
                .map_err(|e| McpToolError::internal(e.to_string()))?;

            let inf_cfg = inference_config();
            let config = hkask_services_compose::CognitionConfig {
                author: format!("mashup:{}:{}", params.author_a, params.author_b),
                jinja2_template: None,
                embedding: hkask_services_compose::EmbeddingSection {
                    model: model.clone(),
                    dim: 1024,
                    centroid_entity_ref: blended_ref.clone(),
                    retrieval: Default::default(),
                },
                validation: hkask_services_compose::ValidationSection {
                    centroid_distance_max: 0.25,
                },
            };

            let inference_ctx = InferenceContext::from_parts(None, &gen_model, inf_cfg);

            let request = hkask_services_compose::ComposeRequest {
                prompt: params.prompt,
                db_path: PathBuf::from(&params.db_path),
                db_passphrase: params.passphrase,
                cognition: config,
                inference_ctx,
                no_validate: false,
            };

            let result = hkask_services_compose::ComposeService::compose(request)
                .await
                .map_err(|e| McpToolError::internal(e.to_string()))?;

            let json_str = serde_json::to_string(&MashupResult {
                prose: result.generated_prose,
                exemplar_count: result.exemplar_count,
                blend_ratio: blend,
                blended_centroid_ref: blended_ref,
                centroid_distance: result.validation.as_ref().map(|v| v.distance),
                distance_a: dist_a,
                distance_b: dist_b,
            })
            .map_err(|e| McpToolError::internal(e.to_string()))?;

            let parsed: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
            Ok(parsed)
        })
        .await
    }

    #[tool(description = "Manage the registry of built author replicas.")]
    pub async fn replica_registry(
        &self,
        Parameters(params): Parameters<RegistryRequest>,
    ) -> String {
        execute_tool(self, "replica_registry", async {
            let db = Database::open(&params.db_path, &params.passphrase)
                .map_err(|e| McpToolError::internal(e.to_string()))?;
            let pool = db
                .sqlite_pool()
                .map_err(|e| McpToolError::internal(format!("pool: {e}")))?;
            let store = EmbeddingStore::from_driver(Arc::new(SqliteDriver::new(pool)), 1024);

            let json_str = match params.action {
                RegistryAction::List => {
                    let centroids = store
                        .query_by_prefix("style:")
                        .map_err(|e| McpToolError::internal(e.to_string()))?;
                    let mut entries: Vec<RegistryEntry> = Vec::new();
                    for entity_ref in &centroids {
                        if entity_ref.ends_with(":centroid") {
                            let parts: Vec<&str> = entity_ref.split(':').collect();
                            if parts.len() >= 3 {
                                let name = parts[1].to_string();
                                let prefix = format!("style:{}:", name);
                                let refs = store
                                    .query_by_prefix(&prefix)
                                    .map_err(|e| McpToolError::internal(e.to_string()))?;
                                let passage_count =
                                    refs.iter().filter(|r| !r.ends_with(":centroid")).count();
                                entries.push(RegistryEntry {
                                    name,
                                    centroid_ref: entity_ref.clone(),
                                    passage_count,
                                });
                            }
                        }
                    }
                    serde_json::to_string(&RegistryResult {
                        message: format!("{} author replicas registered", entries.len()),
                        entries,
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?
                }
                RegistryAction::Remove { author } => {
                    let prefix = format!("style:{}:", author);
                    // Remove embeddings
                    let refs = store
                        .query_by_prefix(&prefix)
                        .map_err(|e| McpToolError::internal(e.to_string()))?;
                    let emb_count = refs.len();
                    for entity_ref in &refs {
                        let _ = store.delete(entity_ref);
                    }
                    // Remove h_mems
                    let pool = db
                        .sqlite_pool()
                        .map_err(|e| McpToolError::internal(e.to_string()))?;
                    let driver = Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));
                    let h_mem_store = hkask_storage::HMemStore::from_driver(driver);
                    let mut triple_count = 0usize;
                    for entity_ref in refs {
                        if let Ok(h_mems) = h_mem_store.query_by_entity(&entity_ref) {
                            for t in &h_mems {
                                let _ = h_mem_store.close_by_id(&t.id);
                                triple_count += 1;
                            }
                        }
                    }
                    serde_json::to_string(&RegistryResult {
                        message: format!(
                            "Removed {} embeddings and {} h_mems for author '{}'",
                            emb_count, triple_count, author
                        ),
                        entries: vec![],
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?
                }
            };

            let parsed: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
            Ok(parsed)
        })
        .await
    }

    #[tool(description = "Explain what style centroids are and how the metadata layer works.")]
    pub async fn replica_explain(&self) -> String {
        execute_tool(self, "replica_explain", async {
            Ok(json!({
            "what_is_a_centroid": "A style centroid is the average of all embedded passage vectors for an author. Each passage (50-200 words) is converted to a 1024-dimensional vector via DeepInfra's Qwen3-Embedding-0.6B. The centroid is the 'average passage' — prose that matches the author's style will have a low cosine distance to it.",
            "metadata_layer": {
                "description": "Each embedded passage is enriched with metadata h_mems (entity-attribute-value) stored alongside embeddings. This enables parametric retrieval beyond pure vector similarity.",
                "structural": ["author", "work_title", "work_slug", "position", "word_count", "avg_sentence_length"],
                "entities_5w1h": {
                    "who": "mentions_character — characters appearing in the passage",
                    "where": "mentions_place — locations/settings",
                    "what": "mentions_event — events/actions",
                    "why": "mentions_concept — themes/ideas",
                    "how": "exhibits_method — stylistic techniques (iceberg_theory, parataxis, etc.)"
                },
                "method_signals": ["parataxis_ratio", "adjective_density", "adverb_density", "dialogue_ratio", "passive_voice_ratio", "sentence_length_variance", "hedge_density", "intensifier_density", "concrete_noun_ratio", "sensory_word_ratio"],
                "salience": "Graph centrality score = (one_hop + two_hop/2) / 2, where one_hop is the fraction of passages sharing ≥1 entity, and two_hop is the fraction reachable within 2 hops. Higher salience = more connected in the entity graph.",
                "budget": "HMem storage is budget-gated per corpus (default: 3,750 h_mems per 100 pages). Passages are sorted by salience; only the top-N earn metadata h_mems. Others get embeddings only."
            },
            "how_blending_works": "Style blending interpolates between two centroids: blended[i] = centroid_a[i] * (1 - blend) + centroid_b[i] * blend. blend=0.0 is pure author A, 1.0 is pure B, 0.5 is equal mix. The blended vector retrieves exemplars from both corpora.",
            "style_space_topology": "Authors cluster in different regions of embedding space. Similar styles have close centroids; opposite styles are far apart. The distance matrix from replica_compare shows which authors can be blended. Hemingway (paratactic) and Woolf (hypotactic) are maximally distant — blending produces noise. Similar authors like Hemingway/Crane or Woolf/Proust would blend well.",
            "distance_thresholds": {
                "identical": "0.000 — same text",
                "very_similar": "0.000-0.030 — nearly identical style",
                "compatible": "0.030-0.300 — blendable",
                "distinct": "0.300-1.000 — clearly different",
                "opposite": "1.000-2.000 — maximally different"
            },
            "retrieval_parameters": {
                "k_min": "Minimum exemplar passages (default: 3)",
                "k_max": "Maximum exemplar passages (default: 7)",
                "distance_threshold": "Maximum cosine distance for exemplar inclusion (default: 0.50)",
                "salience_min": "Only passages with salience >= this value are considered (default: 0.0)",
                "salience_top_k": "Limit to top K most salient matching passages"
            },
            "exemplar_types": {
                "public_domain_author": {
                    "status": "Implemented",
                    "description": "Static YAML corpus config pointing to Gutenberg URLs. Works are declared in corpus.yaml, downloaded, chunked, embedded.",
                    "examples": ["hemingway", "woolf", "austen", "wilde", "twain", "grant", "christie", "eliot"]
                },
                "mashup_persona": {
                    "status": "Implemented",
                    "description": "Two-author centroid interpolation. Exemplars drawn from both source corpora via the blended centroid vector.",
                    "examples": ["jane-wilde (Austen × Wilde)", "ulysses-s-twain (Grant × Twain)", "agatha-eliot (Christie × Eliot)"]
                },
                "academic_author": {
                    "status": "Implemented",
                    "description": "Dynamic corpus discovery via CLI command. Given a name (e.g., 'David Dunning'), searches Semantic Scholar, arXiv, web (SerpAPI), and YouTube transcripts, caches content, and generates a corpus.yaml ready for replica_build. Curated by default — web and YouTube results presented for user confirmation.",
                    "cli_command": "kask style discover \"David Dunning\" [--serpapi-key KEY] [--no-curate] [--no-transcripts] [--no-web]",
                    "pipeline": [
                        "1. Semantic Scholar — free academic paper search with abstracts and open-access PDF links",
                        "2. arXiv — free preprint search with PDF links",
                        "3. Web search (SerpAPI Google) — institutional pages, interviews, faculty profiles",
                        "4. YouTube transcript search (SerpAPI) — talks, lectures, interviews with full transcripts",
                        "5. Interactive curation — user selects which web/YouTube results to include",
                        "6. Content download + cache — PDF→text, HTML→text, stored in .cache/{slug}.txt",
                        "7. Corpus YAML generation — ready for kask style embed-corpus"
                    ],
                    "build_command": "kask style embed-corpus --config <author>/corpus.yaml --db <path>",
                    "implementation": "DiscoveryService in hkask-services (CLI → service, same pattern as EmbedService). MCP tools (replica_discover, replica_cache_work) available for server-mode use. Manifest (replica-discovery.yaml) serves as specification."
                },
                "human_exemplar_principle": "All exemplar types model a named human individual whose body of work constitutes a representational corpus. The logical validity of the replica derives from the relationship between the human and their work — the corpus IS the evidence of their voice, style, and intellectual framework."
            }
        }))
    })
        .await
    }

    #[tool(
        description = "Discover an academic author's body of work and generate a corpus.yaml for replica_build. Delegates to the replica-discovery skill manifest which orchestrates multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts), content extraction, and corpus generation. Supports agentic (fully automated) and curated (human-in-the-loop) modes."
    )]
    pub async fn replica_discover(
        &self,
        Parameters(params): Parameters<DiscoverRequest>,
    ) -> String {
        execute_tool(self, "replica_discover", async {
        let author_name = params.author_name.clone();

        // Validate mode
        let mode = match params.mode.as_str() {
            "agentic" | "curated" => params.mode.clone(),
            other => {
                return Err(McpToolError::invalid_argument(format!(
                    "Invalid mode '{}'. Use 'agentic' or 'curated'.",
                    other
                )));
            }
        };

        let author_name_lower = author_name.to_lowercase();

        // Build parameters for the manifest
        let manifest_params = serde_json::json!({
            "author_name": author_name,
            "author_name_lower": author_name_lower,
            "mode": mode,
            "max_works": params.max_works,
            "include_transcripts": params.include_transcripts,
            "include_web": params.include_web,
            "output_path": params.output_path,
        });

        // Build phase descriptions for the response
        let phases = vec![
            DiscoverPhase {
                ordinal: 1,
                name: "Name Disambiguation".into(),
                description: "Search across multiple sources to confirm author identity".into(),
                sources: vec!["web_search (deep)".into()],
            },
            DiscoverPhase {
                ordinal: 2,
                name: "Academic Paper Search".into(),
                description: "Enumerate papers via Semantic Scholar and arXiv".into(),
                sources: vec!["semantic_scholar".into(), "arxiv".into()],
            },
            DiscoverPhase {
                ordinal: 3,
                name: "Web + Institutional Content".into(),
                description: "Find faculty pages, interviews, and open web content".into(),
                sources: vec!["web_search (web)".into()],
            },
            DiscoverPhase {
                ordinal: 4,
                name: "YouTube Transcript Discovery".into(),
                description: "Search for talks, interviews, lectures on YouTube".into(),
                sources: vec![
                    "web_search (youtube.com)".into(),
                    "serpapi_transcript".into(),
                ],
            },
            DiscoverPhase {
                ordinal: 5,
                name: "Content Extraction".into(),
                description: "Extract full text from all discovered works".into(),
                sources: vec!["web_extract".into(), "docproc (PDF/OCR)".into()],
            },
            DiscoverPhase {
                ordinal: 6,
                name: "Corpus YAML Generation".into(),
                description: "Generate corpus.yaml from discovered works".into(),
                sources: vec!["minijinja template".into()],
            },
        ];

        let summary = format!(
            "Discovering corpus for '{}' in {} mode. Will search Semantic Scholar, arXiv, web{}, and generate a corpus.yaml with up to {} works.",
            params.author_name,
            mode,
            if params.include_transcripts {
                ", YouTube transcripts"
            } else {
                ""
            },
            params.max_works,
        );

        let result = DiscoverResult {
            manifest_id: "mcp/replica-discovery".into(),
            parameters: manifest_params,
            summary,
            phases,
        };

        let output = serde_json::to_value(&result)
            .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));

        Ok(output)
        })
        .await
    }

    #[tool(
        description = "Cache an extracted work's content to disk for reuse by replica_build. Writes content to {cache_dir}/{slug}.txt so the embedding pipeline can skip re-downloading."
    )]
    pub async fn replica_cache_work(
        &self,
        Parameters(params): Parameters<CacheWorkRequest>,
    ) -> String {
        execute_tool(self, "replica_cache_work", async {
            // Validate slug: alphanumeric + hyphens only, no path traversal
            if params.slug.is_empty()
                || !params
                    .slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Invalid slug '{}': must be alphanumeric with hyphens/underscores only",
                    params.slug
                )));
            }

            let cache_dir = PathBuf::from(&params.cache_dir);
            let cache_path = cache_dir.join(format!("{}.txt", params.slug));

            // Create cache directory if it doesn't exist
            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                return Err(McpToolError::internal(format!(
                    "Failed to create cache directory '{}': {}",
                    cache_dir.display(),
                    e
                )));
            }

            let bytes = params.content.as_bytes();
            match std::fs::write(&cache_path, bytes) {
                Ok(()) => {
                    let result = CacheWorkResult {
                        slug: params.slug.clone(),
                        path: cache_path.to_string_lossy().to_string(),
                        bytes_written: bytes.len() as u64,
                    };
                    let output = serde_json::to_value(&result)
                        .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));
                    Ok(output)
                }
                Err(e) => Err(McpToolError::internal(format!(
                    "Failed to write cache file '{}': {}",
                    cache_path.display(),
                    e
                ))),
            }
        })
        .await
    }

    // ── Pipeline orchestration ─────────────────────────────────────────

    #[tool(
        description = "Execute a corpus pipeline manifest with checkpoint/resume. Reads a PipelineManifest YAML, runs each step, checkpoints after each success. MCP steps (docproc_*, replica_*, training_*) are reported as needing external execution via 'kask mcp invoke'. Returns pipeline status with completed/skipped/failed counts."
    )]
    pub async fn replica_pipeline_run(
        &self,
        Parameters(params): Parameters<PipelineRunRequest>,
    ) -> String {
        execute_tool(self, "replica_pipeline_run", async {
            use hkask_types::pipeline_manifest::PipelineManifest;
            use hkask_types::pipeline_runner::{PipelineRunner, StepExecutor};

            let content = std::fs::read_to_string(&params.manifest_path)
                .map_err(|e| McpToolError::invalid_argument(format!("Cannot read manifest: {e}")))?;
            let manifest: PipelineManifest = serde_yaml_neo::from_str(&content)
                .map_err(|e| McpToolError::invalid_argument(format!("Invalid manifest: {e}")))?;

            let mut runner = PipelineRunner::new(manifest)
                .map_err(|e| McpToolError::internal(e.to_string()))?;

            // Step executor — routes pipeline steps to the correct handler.
            // All tools (docproc_*, replica_*) require external MCP execution.
            struct ReplicaStepExecutor;
            impl StepExecutor for ReplicaStepExecutor {
                fn execute(&self, step: &hkask_types::pipeline_manifest::PipelineStep) -> Result<serde_json::Value, hkask_types::pipeline_runner::PipelineError> {
                    if step.params.is_none() {
                        return Err(hkask_types::pipeline_runner::PipelineError::StepFailed {
                            step_id: step.id.clone(),
                            message: format!("Step '{}' requires parameters for tool '{}'", step.id, step.tool),
                        });
                    }
                    Err(hkask_types::pipeline_runner::PipelineError::StepFailed {
                        step_id: step.id.clone(),
                        message: format!(
                            "Step '{}' uses tool '{}' — external MCP execution required. Run via kask mcp invoke --server <tool-server> --tool {}.",
                            step.id, step.tool, step.tool
                        ),
                    })
                }
            }

            let executor = ReplicaStepExecutor;
            let result = runner.run_all(&executor);

            Ok(serde_json::to_value(&result).unwrap_or(json!({"error": "serialization failed"})))
        }).await
    }
}

/// Run the replica MCP server (used by binary target).
pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-replica",
        env!("CARGO_PKG_VERSION"),
        |ctx| {
            Ok(ReplicaServer::new(
                ctx.webid,
                userpod.clone(),
                daemon_client.clone(),
            ))
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_EMBEDDING_MODEL",
                "Embedding model for corpus vectorization (default: Qwen/Qwen3-Embedding-0.6B)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DEFAULT_MODEL",
                "Default generation model for all inference (also used for prose composition). Set via HKASK_DEFAULT_MODEL env var.",
            ),
        ],
    )
    .await
}
