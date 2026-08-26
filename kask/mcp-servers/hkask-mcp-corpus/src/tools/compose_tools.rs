//! Compose and rewrite tools — LLM-based prose generation.
//!
//! These tools generate prose in a specified style by prompting the LLM.
//! They do NOT use style centroids or exemplar retrieval — the persona/
//! centroid system was removed as dead surface. The LLM generates style-
//! appropriate prose from the prompt alone.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::helpers::map_service_error;
use crate::inference_svc::InferenceContext;
use crate::{Parameters, execute_tool_semantic, tool, tool_router};

/// Resolve the embedding model from HkaskSettings.
fn embedding_model() -> String {
    hkask_services_core::settings::HkaskSettings::load().embedding_model()
}

/// Resolve the generation model from InferenceConfig.
fn generation_model() -> String {
    hkask_inference::InferenceConfig::from_env().default_model
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ComposeRequest {
    pub prompt: String,
    pub author: String,
    pub db_path: String,
    pub passphrase: String,
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
}

fn default_composite() -> String {
    "composite".to_string()
}

#[tool_router(router = compose_router, vis = "pub")]
impl crate::CorpusServer {
    #[tool(
        description = "Generate prose in an author's style. Uses the LLM to compose text matching the specified style. The db_path and passphrase connect to the corpus memory DB for optional context retrieval."
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
                let gen_model = embedding_model();
                let config = crate::compose::CognitionConfig {
                    author: params.author.clone(),
                    jinja2_template: None,
                    embedding: crate::compose::EmbeddingSection {
                        model: gen_model.clone(),
                        dim: crate::embedding_dim(),
                        centroid_entity_ref: format!("style:{}:centroid", params.author),
                        retrieval: Default::default(),
                    },
                    validation: crate::compose::ValidationSection {
                        centroid_distance_max: 0.25,
                    },
                };

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
        description = "Rewrite a passage or code snippet in an author's style, optimized for a specific quality dimension (gentle/schriver/hopper/lovelace/composite). Delegates to corpus_compose with dimension-specific guidance."
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

                let gen_model = embedding_model();
                let config = crate::compose::CognitionConfig {
                    author: params.author.clone(),
                    jinja2_template: None,
                    embedding: crate::compose::EmbeddingSection {
                        model: gen_model.clone(),
                        dim: crate::embedding_dim(),
                        centroid_entity_ref: format!("style:{}:centroid", params.author),
                        retrieval: Default::default(),
                    },
                    validation: crate::compose::ValidationSection {
                        centroid_distance_max: 0.25,
                    },
                };

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
