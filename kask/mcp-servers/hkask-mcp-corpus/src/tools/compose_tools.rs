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

use crate::helpers::map_service_error;
use crate::inference_svc::InferenceContext;
use crate::{McpToolError, Parameters, execute_tool_semantic, tool, tool_router};

/// Resolve the embedding model from HkaskSettings.
fn embedding_model() -> String {
    hkask_services_core::settings::HkaskSettings::load().embedding_model()
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

/// Build a `CognitionConfig` inline when no YAML config path is provided.
fn inline_cognition_config(author: &str) -> crate::compose::CognitionConfig {
    let embed_model = embedding_model();
    crate::compose::CognitionConfig {
        author: author.to_string(),
        jinja2_template: None,
        embedding: crate::compose::EmbeddingSection {
            model: embed_model,
            dim: crate::embedding_dim(),
            centroid_entity_ref: format!("style:{}:centroid", author),
            retrieval: Default::default(),
        },
        validation: crate::compose::ValidationSection {
            centroid_distance_max: 0.25,
        },
    }
}

/// Resolve cognition config: load from YAML if config_path is provided,
/// otherwise construct inline.
fn resolve_cognition_config(
    config_path: Option<&str>,
    author: &str,
) -> Result<crate::compose::CognitionConfig, McpToolError> {
    match config_path {
        Some(path) if !path.trim().is_empty() => load_cognition_config(path, author),
        _ => Ok(inline_cognition_config(author)),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ComposeRequest {
    pub prompt: String,
    pub author: String,
    pub db_path: String,
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

#[tool_router(router = compose_router, vis = "pub")]
impl crate::CorpusServer {
    #[tool(
        description = "Generate prose in an author's style using exemplar retrieval and centroid validation. When config_path is provided, loads a cognition config YAML (mashup or style synthesizer) for the Jinja2 system prompt and validation thresholds. The db_path and passphrase connect to the corpus memory DB for exemplar retrieval."
    )]
    pub async fn corpus_compose(
        &self,
        Parameters(params): Parameters<ComposeRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "corpus_compose",
            Self::ontology_anchor("corpus_compose"),
            async {
                let gen_model = generation_model();
                let config = resolve_cognition_config(
                    params.config_path.as_deref(),
                    &params.author,
                )?;

                let inference_ctx = InferenceContext::from_parts(
                    Some(self.inference_router.clone()),
                    &gen_model,
                );

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
                    "exemplar_count": result.exemplar_count,
                    "centroid_distance": result.validation.as_ref().map(|v| v.distance),
                    "style_passed": result.validation.map(|v| v.passed),
                }))
            },
        )
        .await
    }

    #[tool(
        description = "Rewrite a passage or code snippet in an author's style, optimized for a specific quality dimension (gentle/schriver/hopper/lovelace/composite). When config_path is provided, loads a cognition config YAML for the Jinja2 system prompt and validation thresholds. Delegates to corpus_compose with dimension-specific guidance."
    )]
    pub async fn corpus_rewrite(
        &self,
        Parameters(params): Parameters<RewriteRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "corpus_rewrite",
            Self::ontology_anchor("corpus_rewrite"),
            async {
                let dimension_guidance = match params.dimension.to_lowercase().as_str() {
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
                let config = resolve_cognition_config(
                    params.config_path.as_deref(),
                    &params.author,
                )?;

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
                    no_validate: true,
                };

                let result = crate::compose::ComposeService::compose(request)
                    .await
                    .map_err(|e| map_service_error(e, "Rewrite failed"))?;

                Ok(json!({
                    "rewritten": result.generated_prose,
                    "dimension": params.dimension,
                    "author": params.author,
                    "exemplar_count": result.exemplar_count,
                    "centroid_distance": result.validation.as_ref().map(|v| v.distance),
                    "style_passed": result.validation.map(|v| v.passed),
                }))
            },
        )
        .await
    }
}
