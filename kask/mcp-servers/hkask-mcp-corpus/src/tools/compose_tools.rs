//! Compose and rewrite tools — LLM-based prose generation.
//!
//! These tools generate prose in a specified style using exemplar retrieval
//! and centroid validation. When a `config_path` is provided, the cognition
//! config (Jinja2 system prompt template, embedding model, retrieval
//! parameters, validation thresholds) is loaded from a YAML file — this is
//! how mashup and style-synthesizer configs are used. When no `config_path`
//! is provided, a generic inline config is constructed from the tool
//! parameters.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::helpers::{default_corpus_passphrase, map_service_error};
use crate::inference_svc::InferenceContext;
use crate::{McpToolError, Parameters, execute_tool, tool, tool_router};

/// Resolve the embedding model from HkaskSettings.
fn embedding_model() -> String {
    hkask_services_core::standalone_settings::HkaskSettings::load().embedding_model()
}

/// Resolve the generation model from InferenceConfig.
fn generation_model() -> String {
    hkask_inference::InferenceConfig::from_env().default_model
}

/// Load a `CognitionConfig` from a YAML file, applying path containment.
fn load_cognition_config(
    config_path: &str,
    author: &str,
) -> Result<crate::compose::CognitionConfig, McpToolError> {
    // config_path is LLM-reachable, so apply path containment (CWE-22/200)
    // to prevent traversal outside the project root or kask data dir.
    let resolved_path = hkask_mcp_server::server::contain_for_read(config_path)?;
    let yaml_str = std::fs::read_to_string(&resolved_path).map_err(|e| {
        McpToolError::invalid_argument(format!(
            "Failed to read cognition config at {}: {e}",
            resolved_path.display()
        ))
    })?;

    let mut config: crate::compose::CognitionConfig =
        serde_yaml_neo::from_str(&yaml_str).map_err(|e| {
            McpToolError::invalid_argument(format!(
                "Failed to parse cognition config at {}: {e}",
                resolved_path.display()
            ))
        })?;

    // The author from the tool call takes precedence over the YAML's author
    // field — the YAML declares the style, the tool call selects which
    // exemplar's corpus to compose against.
    if !author.is_empty() {
        config.author = author.to_string();
    }

    Ok(config)
}

/// Build a `CognitionConfig` inline with the caller-validated embedding
/// model (see `resolve_cognition_config` — the fail-visible gate).
fn inline_cognition_config_with(
    author: &str,
    embed_model: String,
) -> crate::compose::CognitionConfig {
    crate::compose::CognitionConfig {
        author: author.to_string(),
        jinja2_template: None,
        embedding: crate::compose::EmbeddingSection {
            model: embed_model,
            dim: crate::embedding_dim(),
            centroid_entity_ref: crate::compose::style_centroid_ref(author, None),
            retrieval: Default::default(),
        },
        validation: crate::compose::ValidationSection {
            centroid_distance_max: 0.25,
        },
    }
}

/// Resolve cognition config: load from YAML if config_path is provided,
/// otherwise construct inline. Fail-visible: an inline config REQUIRES a
/// configured embedding model — empty is a typed error naming the setting,
/// never a hidden constant (the operator's no-hidden-models spec).
fn resolve_cognition_config(
    config_path: Option<&str>,
    author: &str,
) -> Result<crate::compose::CognitionConfig, McpToolError> {
    match config_path {
        Some(path) if !path.trim().is_empty() => load_cognition_config(path, author),
        _ => {
            let embed_model = embedding_model();
            if embed_model.trim().is_empty() {
                return Err(McpToolError::permission_denied(
                    "no embedding model configured — set \
                     kask.models.embedding_model (injected as \
                     HKASK_EMBEDDING_MODEL); kask never falls back to a \
                     hidden code constant",
                ));
            }
            Ok(inline_cognition_config_with(author, embed_model))
        }
    }
}

/// Guard for the canonically-resolved passphrase: an empty resolution is
/// an authorization failure naming the setting, never a silent fallback to
/// an unencrypted DB (the `.rules` missing-credential pattern).
fn require_passphrase(passphrase: &str) -> Result<(), McpToolError> {
    if passphrase.trim().is_empty() {
        return Err(McpToolError::permission_denied(
            "no database passphrase resolved — set HKASK_DB_PASSPHRASE \
             (canonical resolution: launch-injected credentials → env → \
             keychain); kask never falls back to an unencrypted DB",
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ComposeRequest {
    pub prompt: String,
    pub author: String,
    pub db_path: String,
    #[serde(default = "default_corpus_passphrase")]
    pub passphrase: String,
    /// Optional path to a cognition config YAML (e.g. a mashup or style
    /// synthesizer config). When provided, the Jinja2 template, embedding
    /// model, retrieval parameters, and validation thresholds are loaded
    /// from the file. When omitted, a generic inline config is used.
    #[serde(default)]
    pub config_path: Option<String>,
    #[serde(default)]
    pub no_validate: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RewriteRequest {
    pub content: String,
    pub author: String,
    pub db_path: String,
    #[serde(default = "default_corpus_passphrase")]
    pub passphrase: String,
    #[serde(default = "default_composite")]
    pub dimension: String,
    /// Optional path to a cognition config YAML. When provided, the Jinja2
    /// template and validation thresholds are loaded from the file.
    #[serde(default)]
    pub config_path: Option<String>,
}

fn default_composite() -> String {
    "composite".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CentroidRequest {
    pub author: String,
    pub db_path: String,
    #[serde(default = "default_corpus_passphrase")]
    pub passphrase: String,
    /// Optional contained UTF-8 file: one existing entity ref per nonblank line.
    /// Repeated refs count once; missing eligible refs fail without storing a centroid.
    #[serde(default)]
    pub refs_file: Option<String>,
    /// Optional quality dimension, stored at style:{author}:{dimension}:centroid.
    /// Omit to retain style:{author}:centroid. This is not the vector size.
    #[serde(default)]
    pub dimension: Option<String>,
}

fn normalize_dimension(dimension: &str) -> Result<String, McpToolError> {
    let dimension = dimension.trim().to_lowercase();
    if dimension.is_empty() || dimension.contains(':') {
        return Err(McpToolError::invalid_argument(
            "dimension must be nonblank and contain no ':' namespace separator",
        ));
    }
    Ok(dimension)
}

fn load_centroid_refs(refs_file: &str) -> Result<Vec<String>, McpToolError> {
    let path = hkask_mcp_server::server::contain_for_read(refs_file)?;
    let text = std::fs::read_to_string(&path).map_err(|error| {
        McpToolError::invalid_argument(format!(
            "Failed to read refs_file {}: {error}",
            path.display()
        ))
    })?;
    let refs: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect();
    if refs.is_empty() {
        return Err(McpToolError::invalid_argument(
            "refs_file contains no entity refs",
        ));
    }
    Ok(refs)
}

#[tool_router(router = compose_router, vis = "pub")]
impl crate::CorpusServer {
    #[tool(
        description = "Generate prose in an author's style using exemplar retrieval and centroid validation. When config_path is provided, loads a cognition config YAML (mashup or style synthesizer) for the Jinja2 system prompt and validation thresholds. The db_path and passphrase connect to the corpus memory DB for exemplar retrieval."
    )]
    pub async fn corpus_compose(
        &self,
        Parameters(params): Parameters<ComposeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_compose", async {
            require_passphrase(&params.passphrase)?;
            let gen_model = generation_model();
            let config = resolve_cognition_config(params.config_path.as_deref(), &params.author)?;

            let inference_ctx =
                InferenceContext::from_parts(Some(self.inference_router.clone()), &gen_model);

            let request = crate::compose::ComposeRequest {
                prompt: params.prompt,
                db_path: PathBuf::from(&params.db_path),
                db_passphrase: params.passphrase,
                cognition: config,
                inference_ctx,
                no_validate: params.no_validate,
            };

            let result = crate::compose::ComposeService::compose(request)
                .await
                .map_err(|e| map_service_error(e, "Compose failed"))?;

            Ok(json!({
                "prose": result.generated_prose,
                "centroid_missing": result.centroid_missing,
                "exemplar_count": result.exemplar_count,
                "method_signals_missing": result.method_signals_missing,
                "centroid_distance": result.validation.as_ref().map(|v| v.distance),
                "style_passed": result.validation.map(|v| v.passed),
            }))
        })
        .await
    }

    #[tool(
        description = "Compute and store a style centroid from existing embeddings. By default averages the style:{author}: prefix and stores style:{author}:centroid. Optional path-contained refs_file selects newline-delimited entity refs without copying embeddings; duplicate refs count once and missing eligible refs fail. Optional quality dimension stores style:{author}:{dimension}:centroid for corpus_rewrite. Derived :centroid and :rule: refs are excluded. Build before compose/rewrite: absent validation centroids surface as centroid_missing. Embedding model: kask.models.embedding_model; vector size: HKASK_EMBEDDING_DIM (default 1024), independent of quality dimension."
    )]
    pub async fn corpus_centroid(
        &self,
        Parameters(params): Parameters<CentroidRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_centroid", async {
            require_passphrase(&params.passphrase)?;
            let embed_model = embedding_model();
            if embed_model.trim().is_empty() {
                return Err(McpToolError::permission_denied(
                    "no embedding model configured — set \
                     kask.models.embedding_model (injected as \
                     HKASK_EMBEDDING_MODEL); kask never falls back to a \
                     hidden code constant",
                ));
            }

            let dimension = params
                .dimension
                .as_deref()
                .map(normalize_dimension)
                .transpose()?;
            let entity_refs = params
                .refs_file
                .as_deref()
                .map(load_centroid_refs)
                .transpose()?;
            let result = crate::compose::ComposeService::style_centroid(
                crate::compose::CentroidComputeRequest {
                    db_path: PathBuf::from(&params.db_path),
                    db_passphrase: params.passphrase,
                    author: params.author.clone(),
                    dimension,
                    entity_refs,
                    model: embed_model,
                    dim: crate::embedding_dim(),
                },
            )
            .map_err(|e| map_service_error(e, "Centroid computation failed"))?;

            Ok(json!({
                "author": params.author,
                "centroid_entity_ref": result.centroid_entity_ref,
                "passage_count": result.passage_count,
                "stored": result.stored,
            }))
        })
        .await
    }

    #[tool(
        description = "Rewrite a passage or code snippet in an author's style, optimized for a specific quality dimension (gentle/schriver/hopper/lovelace/composite). Validates against style:{author}:{dimension}:centroid; an absent centroid surfaces as centroid_missing with null distance/pass, never a successful validation. When config_path is provided, uses its Jinja2 prompt and validation thresholds but selects the requested dimension centroid."
    )]
    pub async fn corpus_rewrite(
        &self,
        Parameters(params): Parameters<RewriteRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "corpus_rewrite",
            async {
                require_passphrase(&params.passphrase)?;
                let dimension = normalize_dimension(&params.dimension)?;
                let dimension_guidance = match dimension.as_str() {
                    "gentle" => "Rewrite this text to maximize agent-correctness. Docs ARE code — ensure every statement is actionable and unambiguous. Remove any stale references or outdated information.",
                    "schriver" => "Rewrite this text for maximum findability. Use scannable headings, descriptive hyperlinks, and front-load key concepts. A reader must find their answer within 30 seconds.",
                    "hopper" => "Rewrite this text for maximum accessibility. Make it comprehensible on first reading with zero prior context. Use plain language, active voice, and short sentences.",
                    "lovelace" => "Rewrite this text for maximum precision. Make every specification independently verifiable — a reader must be able to write a test from this text alone.",
                    _ => "Rewrite this text for all four dimensions of documentation excellence: agent-correctness (Gentle), findability (Schriver), accessibility (Hopper), and precision (Lovelace).",
                };

                let prompt = format!(
                    "{dimension_guidance}\n\nText to rewrite:\n\n{}",
                    params.content
                );

                let gen_model = generation_model();
                let mut config = resolve_cognition_config(
                    params.config_path.as_deref(),
                    &params.author,
                )?;
                config.embedding.centroid_entity_ref =
                    crate::compose::style_centroid_ref(&params.author, Some(&dimension));
                let centroid_entity_ref = config.embedding.centroid_entity_ref.clone();

                let inference_ctx = InferenceContext::from_parts(
                    Some(self.inference_router.clone()),
                    &gen_model,
                );

                let request = crate::compose::ComposeRequest {
                    prompt,
                    db_path: PathBuf::from(&params.db_path),
                    db_passphrase: params.passphrase,
                    cognition: config,
                    inference_ctx,
                    no_validate: false,
                };

                let result = crate::compose::ComposeService::compose(request)
                    .await
                    .map_err(|e| map_service_error(e, "Rewrite failed"))?;

                Ok(json!({
                    "rewritten": result.generated_prose,
                    "dimension": dimension,
                    "centroid_entity_ref": centroid_entity_ref,
                    "centroid_missing": result.centroid_missing,
                    "author": params.author,
                    "exemplar_count": result.exemplar_count,
                    "method_signals_missing": result.method_signals_missing,
                    "centroid_distance": result.validation.as_ref().map(|v| v.distance),
                    "style_passed": result.validation.map(|v| v.passed),
                }))
            },
        )
        .await
    }
}
