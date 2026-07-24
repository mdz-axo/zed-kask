//! LLM concept extraction and method inference for the discovery pipeline.

use crate::embed::{Entity, EntityConfig};
use hkask_inference::{InferenceConfig, InferenceRouter};
use hkask_memory::salience::{DeclaredMethod, MethodThresholds};
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::InferencePort;
use std::path::{Path, PathBuf};

use super::types::DiscoveredWork;

/// Default template base path relative to project root.
const TEMPLATE_BASE: &str = "registry/templates/replica";

/// Parse a model override directive from a Jinja2 template's first line.
/// Format: `{# model: OM/qwen3:14b #}`
/// Returns the model name (with provider prefix) if found, None otherwise.
pub(crate) fn parse_template_model(template_src: &str) -> Option<String> {
    let first_line = template_src.lines().next()?;
    let trimmed = first_line.trim();
    if trimmed.starts_with("{# model:") && trimmed.ends_with("#}") {
        let model = trimmed
            .strip_prefix("{# model:")?
            .strip_suffix("#}")?
            .trim();
        if model.is_empty() {
            None
        } else {
            Some(model.to_string())
        }
    } else {
        None
    }
}

/// Extract key concepts, places, and events from academic paper titles
/// and abstracts using LLM semantic deduplication via the extract-concepts.j2 template.
pub(crate) async fn extract_concepts(
    author_name: &str,
    works: &[DiscoveredWork],
) -> Result<EntityConfig, ServiceError> {
    // Build paper list for template with titles and abstracts
    let papers: Vec<serde_json::Value> = works
        .iter()
        .map(|w| {
            serde_json::json!({
                "title": w.title,
                "abstract": w.abstract_text.as_deref().unwrap_or(""),
                "year": w.year.map(|y| y.to_string()).unwrap_or_else(|| "unknown".to_string()),
            })
        })
        .collect();

    // Render template
    let template_path = PathBuf::from(TEMPLATE_BASE).join("extract-concepts.j2");
    let template_src = std::fs::read_to_string(&template_path).map_err(|e| {
        let msg = format!("Failed to read extract-concepts template: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let model_override = parse_template_model(&template_src);

    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template_owned("extract-concepts", template_src)
        .map_err(|e| {
            let msg = format!("Failed to parse template: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    let tmpl = env.get_template("extract-concepts").map_err(|e| {
        let msg = format!("Failed to load template: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let prompt = tmpl
        .render(minijinja::context! {
            author_name,
            papers,
            max_concepts => 15,
        })
        .map_err(|e| {
            let msg = format!("Failed to render template: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    // Call inference
    let inf_cfg = InferenceConfig::from_env();
    let router = InferenceRouter::new(inf_cfg);
    let params = hkask_types::template::LLMParameters {
        temperature: 0.3,
        max_tokens: 1024,
        ..Default::default()
    };

    let result = router
        .generate_with_model(&prompt, &params, model_override.as_deref(), None)
        .await
        .map_err(|e| {
            let msg = format!("Concept extraction inference failed: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    // Parse JSON response
    let parsed: serde_json::Value = serde_json::from_str(&result.text).map_err(|e| {
        let msg = format!("Failed to parse concept extraction response as JSON: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let concepts = parse_entity_list(&parsed, "concepts");
    let places = parse_entity_list(&parsed, "places");
    let events = parse_entity_list(&parsed, "events");

    Ok(EntityConfig {
        characters: vec![],
        places,
        events,
        concepts,
        co_authors: vec![],
        venues: vec![],
        topics: vec![],
        paradigms: vec![],
    })
}

/// Infer methodological and stylistic patterns from cached work content
/// using LLM analysis via the infer-methods.j2 template.
pub(crate) async fn infer_methods(
    author_name: &str,
    works: &[DiscoveredWork],
    cache_dir: &Path,
) -> Result<Vec<DeclaredMethod>, ServiceError> {
    // Sample up to 5 passages from cached content (first ~800 chars of each)
    let mut sample_passages: Vec<serde_json::Value> = Vec::new();
    for work in works.iter().take(5) {
        let cache_path = cache_dir.join(format!("{}.txt", work.slug));
        if let Ok(content) = std::fs::read_to_string(&cache_path) {
            let excerpt: String = content.chars().take(800).collect();
            if excerpt.split_whitespace().count() >= 20 {
                sample_passages.push(serde_json::json!({
                    "text": excerpt,
                    "work_slug": work.slug,
                }));
            }
        }
    }

    if sample_passages.is_empty() {
        return Ok(vec![]);
    }

    // Render template
    let template_path = PathBuf::from(TEMPLATE_BASE).join("infer-methods.j2");
    let template_src = std::fs::read_to_string(&template_path).map_err(|e| {
        let msg = format!("Failed to read infer-methods template: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let model_override = parse_template_model(&template_src);

    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template_owned("infer-methods", template_src)
        .map_err(|e| {
            let msg = format!("Failed to parse template: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    let tmpl = env.get_template("infer-methods").map_err(|e| {
        let msg = format!("Failed to load template: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let prompt = tmpl
        .render(minijinja::context! {
            author_name,
            author_domain => "academic",
            sample_passages,
        })
        .map_err(|e| {
            let msg = format!("Failed to render template: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    // Call inference
    let inf_cfg = InferenceConfig::from_env();
    let router = InferenceRouter::new(inf_cfg);
    let params = hkask_types::template::LLMParameters {
        temperature: 0.3,
        max_tokens: 1024,
        ..Default::default()
    };

    let result = router
        .generate_with_model(&prompt, &params, model_override.as_deref(), None)
        .await
        .map_err(|e| {
            let msg = format!("Method inference failed: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    // Parse JSON response
    let parsed: serde_json::Value = serde_json::from_str(&result.text).map_err(|e| {
        let msg = format!("Failed to parse method inference response as JSON: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let methods: Vec<DeclaredMethod> = parsed["methods"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m["name"].as_str()?.to_string();
                    let description = m["description"].as_str().unwrap_or("").to_string();
                    let signal: MethodThresholds =
                        serde_json::from_value(m["signal"].clone()).unwrap_or_default();
                    Some(DeclaredMethod {
                        name,
                        description,
                        signal,
                        threshold: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(methods)
}

/// Parse an entity list from a JSON field (e.g., "concepts", "places", "events").
fn parse_entity_list(parsed: &serde_json::Value, field: &str) -> Vec<Entity> {
    parsed[field]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let name = v["name"].as_str()?.to_string();
                    let appears_in: Vec<String> = v["appears_in"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| s.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(Entity { name, appears_in })
                })
                .collect()
        })
        .unwrap_or_default()
}
