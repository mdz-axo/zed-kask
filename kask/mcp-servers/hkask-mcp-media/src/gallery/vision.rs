//! Vision LLM wrappers for gallery image analysis.
//!
//! Uses the hKask inference router to call vision-capable LLMs
//! (Llama 3.2 Vision, Qwen2-VL, Gemma 4, etc.) for:
//! - Face detection and description
//! - Object detection
//! - Color palette analysis
//! - Composition analysis
//! - Scene captioning
//! - Face reference validation
//! - Face matching (same person?)
//!
//! All prompts are backed by Jinja2 templates embedded in templates.rs.

use hkask_inference::InferenceRouter;
use hkask_types::template::LLMParameters;
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Result of face reference validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceValidationResult {
    /// Whether the image passes all criteria for use as a face reference.
    pub valid: bool,
    /// Number of faces detected.
    pub face_count: u32,
    /// Estimated percentage of image occupied by the face.
    pub face_coverage_pct: u32,
    /// Pose assessment.
    pub pose: String,
    /// Lighting quality.
    pub lighting: String,
    /// Occlusion assessment.
    pub occlusion: String,
    /// Image clarity / focus.
    pub clarity: String,
    /// List of failing criteria with explanations (empty if valid).
    pub issues: Vec<String>,
}

/// Result of comparing two face images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceMatchResult {
    /// Whether the two faces are the same person.
    #[serde(rename = "match")]
    pub is_match: bool,
    /// Confidence score (0.0–1.0).
    pub confidence: f64,
    /// Human-readable reasoning for the decision.
    pub reasoning: String,
}

/// Detect and describe all faces in an image using a vision LLM.
///
/// Returns a list of face descriptions (JSON objects with face_index,
/// age_range, gender_presentation, features, position, size).
/// Falls back to raw text if JSON parsing fails.
///
pub async fn detect_faces(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<Vec<serde_json::Value>, crate::MediaError> {
    let mut vars = HashMap::new();
    vars.insert("detail_level", "detailed");
    let prompt = crate::templates::render(template_env, "tag_faces", &vars)?;

    let params = LLMParameters::default();

    let model_label = vision_model.unwrap_or("default");
    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "detect",
        provider = model_label,
        "Vision LLM face detection"
    );

    let result = inference
        .generate_vision(&prompt, &[image_url.to_string()], &params, vision_model)
        .await
        .map_err(|e| {
            tracing::warn!(
                target: "reg.mcp.media.face",
                operation = "detect",
                provider = model_label,
                error = %e,
                "Vision LLM face detection failed"
            );
            crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e))
        })?;

    // Try parsing as JSON array first
    let faces = if let Ok(faces) = serde_json::from_str::<Vec<serde_json::Value>>(&result.text) {
        faces
    } else {
        // Fallback: wrap raw text as a single face entry
        vec![serde_json::json!({"raw": result.text.trim()})]
    };

    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "detect",
        provider = model_label,
        face_count = faces.len(),
        "Vision LLM face detection complete"
    );

    Ok(faces)
}

/// Validate a reference image for use in facial recognition.
///
/// Sends the image to a vision LLM with the `validate_face_ref` template.
/// Returns structured pass/fail with specific reasons for rejection.
///
pub async fn validate_face_reference(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<FaceValidationResult, crate::MediaError> {
    let prompt = crate::templates::render(
        template_env,
        "validate_face_ref",
        &std::collections::HashMap::new(),
    )?;

    let params = LLMParameters {
        temperature: 0.1, // Low temperature for consistent, objective assessment
        max_tokens: 512,
        ..Default::default()
    };

    let model_label = vision_model.unwrap_or("default");
    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "validate",
        provider = model_label,
        "Vision LLM face reference validation"
    );

    let result = inference
        .generate_vision(&prompt, &[image_url.to_string()], &params, vision_model)
        .await
        .map_err(|e| {
            tracing::warn!(
                target: "reg.mcp.media.face",
                operation = "validate",
                provider = model_label,
                error = %e,
                "Vision LLM face validation failed"
            );
            crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e))
        })?;

    let parsed: FaceValidationResult = serde_json::from_str(&result.text).map_err(|e| {
        crate::MediaError::VisionParse(format!(
            "Failed to parse validation result: {} — raw: {}",
            e,
            &result.text[..200.min(result.text.len())]
        ))
    })?;

    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "validate",
        provider = model_label,
        valid = parsed.valid,
        face_count = parsed.face_count,
        "Vision LLM face validation complete"
    );

    Ok(parsed)
}

/// Compare two face images to determine if they show the same person.
///
/// Sends both images to a vision LLM with the `match_faces` template.
/// Image 1 is the reference portrait, Image 2 is the query face.
///
pub async fn match_faces(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    reference_url: &str,
    query_url: &str,
    vision_model: Option<&str>,
) -> Result<FaceMatchResult, crate::MediaError> {
    let prompt = crate::templates::render(
        template_env,
        "match_faces",
        &std::collections::HashMap::new(),
    )?;

    let params = LLMParameters {
        temperature: 0.1,
        max_tokens: 512,
        ..Default::default()
    };

    let model_label = vision_model.unwrap_or("default");
    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "match",
        provider = model_label,
        "Vision LLM face match"
    );

    let result = inference
        .generate_vision(
            &prompt,
            &[reference_url.to_string(), query_url.to_string()],
            &params,
            vision_model,
        )
        .await
        .map_err(|e| {
            tracing::warn!(
                target: "reg.mcp.media.face",
                operation = "match",
                provider = model_label,
                error = %e,
                "Vision LLM face match failed"
            );
            crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e))
        })?;

    let parsed: FaceMatchResult = serde_json::from_str(&result.text).map_err(|e| {
        crate::MediaError::VisionParse(format!(
            "Failed to parse match result: {} — raw: {}",
            e,
            &result.text[..200.min(result.text.len())]
        ))
    })?;

    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "match",
        provider = model_label,
        is_match = parsed.is_match,
        confidence = parsed.confidence,
        "Vision LLM face match complete"
    );

    Ok(parsed)
}

/// Result of face embedding extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceEmbeddingResult {
    /// The 512-dim embedding vector.
    pub embedding: Vec<f32>,
    /// Dimension of the embedding (always 512 with the current template).
    pub dim: u32,
}

/// Produce a 512-dim face embedding vector for an image using a vision LLM.
///
/// Sends the image to a vision LLM with the `embed_face` template, which
/// instructs the model to output a JSON object `{"embedding": [512 floats], "dim": 512}`.
/// The vector is designed to be cosine-similar across images of the same person.
///
/// Returns the embedding as a `Vec<f32>` for storage as raw bytes.
///
pub async fn embed_face(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<FaceEmbeddingResult, crate::MediaError> {
    let prompt = crate::templates::render(
        template_env,
        "embed_face",
        &std::collections::HashMap::new(),
    )?;

    // Embeddings need more tokens than a match result — 512 floats + JSON overhead.
    let params = LLMParameters {
        temperature: 0.1,
        max_tokens: 4096,
        ..Default::default()
    };

    let model_label = vision_model.unwrap_or("default");
    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "embed",
        provider = model_label,
        "Vision LLM face embedding"
    );

    let result = inference
        .generate_vision(&prompt, &[image_url.to_string()], &params, vision_model)
        .await
        .map_err(|e| {
            tracing::warn!(
                target: "reg.mcp.media.face",
                operation = "embed",
                provider = model_label,
                error = %e,
                "Vision LLM face embedding failed"
            );
            crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e))
        })?;

    let parsed: FaceEmbeddingResult = serde_json::from_str(&result.text).map_err(|e| {
        crate::MediaError::VisionParse(format!(
            "Failed to parse embedding result: {} — raw: {}",
            e,
            &result.text[..200.min(result.text.len())]
        ))
    })?;

    if parsed.embedding.len() != parsed.dim as usize {
        return Err(crate::MediaError::VisionParse(format!(
            "Embedding dimension mismatch: declared {} but got {}",
            parsed.dim,
            parsed.embedding.len()
        )));
    }

    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "embed",
        provider = model_label,
        dim = parsed.dim,
        "Vision LLM face embedding complete"
    );

    Ok(parsed)
}

/// Detect and label all prominent objects in an image.
///
/// Returns a list of object descriptions (JSON objects with name,
/// location, confidence, description). Falls back to raw text.
///
pub async fn detect_objects(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<Vec<serde_json::Value>, crate::MediaError> {
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("detail_level", "detailed");
    vars.insert("max_objects", "20");
    let prompt = crate::templates::render(template_env, "tag_objects", &vars)?;

    let params = LLMParameters::default();

    let result = inference
        .generate_vision(&prompt, &[image_url.to_string()], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    if let Ok(objects) = serde_json::from_str::<Vec<serde_json::Value>>(&result.text) {
        Ok(objects)
    } else {
        Ok(vec![serde_json::json!({"raw": result.text.trim()})])
    }
}

/// Analyze the dominant color palette of an image.
///
/// Returns a JSON object with colors array, palette_style, temperature,
/// and saturation. Falls back to raw text.
///
pub async fn analyze_colors(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<serde_json::Value, crate::MediaError> {
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("max_colors", "8");
    let prompt = crate::templates::render(template_env, "tag_colors", &vars)?;

    let params = LLMParameters::default();

    let result = inference
        .generate_vision(&prompt, &[image_url.to_string()], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result.text) {
        Ok(parsed)
    } else {
        Ok(serde_json::json!({"raw": result.text.trim()}))
    }
}

/// Analyze the photographic composition of an image.
///
/// Returns a JSON object with focal_point, rule_of_thirds, leading_lines,
/// depth_of_field, perspective, framing, symmetry, negative_space.
/// Falls back to raw text.
///
pub async fn analyze_composition(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<serde_json::Value, crate::MediaError> {
    let prompt = crate::templates::render(template_env, "tag_composition", &HashMap::new())?;

    let params = LLMParameters::default();

    let result = inference
        .generate_vision(&prompt, &[image_url.to_string()], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result.text) {
        Ok(parsed)
    } else {
        Ok(serde_json::json!({"raw": result.text.trim()}))
    }
}

/// Generate a descriptive caption for an image.
///
/// Returns plain text describing the scene (subject, setting, lighting,
/// colors, composition, mood).
///
pub async fn caption_scene(
    inference: &Arc<InferenceRouter>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<String, crate::MediaError> {
    let mut vars = HashMap::new();
    vars.insert("style", "descriptive");
    let prompt = crate::templates::render(template_env, "caption", &vars)?;

    let params = LLMParameters::default();

    let result = inference
        .generate_vision(&prompt, &[image_url.to_string()], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    Ok(result.text.trim().to_string())
}
