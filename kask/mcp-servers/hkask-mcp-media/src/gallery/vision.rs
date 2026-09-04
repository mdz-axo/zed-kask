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

use base64::Engine;
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Load an image and return it as raw base64-encoded PNG — the exact
/// payload the inference bridge's `LanguageModelImage` contract expects
/// (`to_base64_url` prepends `data:image/png;base64,` itself). The former
/// conventions passed URLs and pre-built data URIs, which the bridge
/// wrapped into `data:image/png;base64,<garbage>` — the image never
/// reached the model.
///
/// Accepts local file paths (optional `file://` prefix), http(s) URLs, and
/// pre-built `data:<mime>;base64,<payload>` URIs (unwrapped, not passed
/// through — the bridge would double-wrap them). Images larger than 2048px
/// are downscaled to fit: a full-size photo re-encoded as PNG would exceed
/// the 16MB IPC line cap, and vision models tile internally at lower
/// resolutions anyway.
pub(crate) async fn load_image_as_png_base64(
    image_source: &str,
) -> Result<String, crate::MediaError> {
    const MAX_DIMENSION: u32 = 2048;

    let bytes: Vec<u8> = if let Some(payload) = image_source
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,").map(|(_, payload)| payload))
    {
        // Pre-built data URI: decode the payload — passing the URI whole
        // would make the bridge double-wrap it into undeclarable garbage.
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|e| {
                crate::MediaError::VisionApi(format!("Invalid base64 image payload: {e}"))
            })?
    } else if image_source.starts_with("http://") || image_source.starts_with("https://") {
        reqwest::get(image_source)
            .await
            .map_err(|e| {
                crate::MediaError::VisionApi(format!("Failed to fetch image '{image_source}': {e}"))
            })?
            .bytes()
            .await
            .map_err(|e| {
                crate::MediaError::VisionApi(format!(
                    "Failed to read image bytes from '{image_source}': {e}"
                ))
            })?
            .to_vec()
    } else {
        let path = image_source.strip_prefix("file://").unwrap_or(image_source);
        tokio::fs::read(path).await.map_err(|e| {
            crate::MediaError::VisionApi(format!("Failed to read image file '{path}': {e}"))
        })?
    };

    let mut img = image::load_from_memory(&bytes)
        .map_err(|e| crate::MediaError::VisionApi(format!("Failed to decode image: {e}")))?;
    if img.width() > MAX_DIMENSION || img.height() > MAX_DIMENSION {
        img = img.thumbnail(MAX_DIMENSION, MAX_DIMENSION);
    }
    let mut png_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .map_err(|e| crate::MediaError::VisionApi(format!("Failed to re-encode image as PNG: {e}")))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png_bytes))
}

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
    inference: &Arc<dyn InferencePort>,
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

    let image_b64 = load_image_as_png_base64(image_url).await?;
    let result = inference
        .generate_vision(&prompt, &[image_b64], &params, vision_model)
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

    // The template demands a JSON array — anything else is a model misbehavior
    // (refusal, prose preamble, truncation) and must error, not be fabricated
    // into a face entry that would be persisted as a 0.85-confidence tag.
    let faces: Vec<serde_json::Value> = serde_json::from_str(&result.text).map_err(|e| {
        crate::MediaError::VisionParse(format!(
            "Failed to parse face detection result: {} — raw: {}",
            e,
            &result.text[..200.min(result.text.len())]
        ))
    })?;

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
    inference: &Arc<dyn InferencePort>,
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
        ..Default::default()
    };

    let model_label = vision_model.unwrap_or("default");
    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "validate",
        provider = model_label,
        "Vision LLM face reference validation"
    );

    let image_b64 = load_image_as_png_base64(image_url).await?;
    let result = inference
        .generate_vision(&prompt, &[image_b64], &params, vision_model)
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
    inference: &Arc<dyn InferencePort>,
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
        ..Default::default()
    };

    let model_label = vision_model.unwrap_or("default");
    tracing::info!(
        target: "reg.mcp.media.face",
        operation = "match",
        provider = model_label,
        "Vision LLM face match"
    );

    let reference_b64 = load_image_as_png_base64(reference_url).await?;
    let query_b64 = load_image_as_png_base64(query_url).await?;
    let result = inference
        .generate_vision(&prompt, &[reference_b64, query_b64], &params, vision_model)
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

/// Detect and label all prominent objects in an image.
///
/// Returns a list of object descriptions (JSON objects with name,
/// location, confidence, description). Unparseable model output is an
/// error — it is never fabricated into an object entry.
///
pub async fn detect_objects(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<Vec<serde_json::Value>, crate::MediaError> {
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("detail_level", "detailed");
    vars.insert("max_objects", "20");
    let prompt = crate::templates::render(template_env, "tag_objects", &vars)?;

    let params = LLMParameters::default();

    let image_b64 = load_image_as_png_base64(image_url).await?;
    let result = inference
        .generate_vision(&prompt, &[image_b64], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    let objects: Vec<serde_json::Value> = serde_json::from_str(&result.text).map_err(|e| {
        crate::MediaError::VisionParse(format!(
            "Failed to parse object detection result: {} — raw: {}",
            e,
            &result.text[..200.min(result.text.len())]
        ))
    })?;
    Ok(objects)
}

/// Analyze the dominant color palette of an image.
///
/// Returns a JSON object with colors array, palette_style, temperature,
/// and saturation. Unparseable model output is an error — a `raw` wrapper
/// would silently yield zero color tags while reporting success.
///
pub async fn analyze_colors(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<serde_json::Value, crate::MediaError> {
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("max_colors", "8");
    let prompt = crate::templates::render(template_env, "tag_colors", &vars)?;

    let params = LLMParameters::default();

    let image_b64 = load_image_as_png_base64(image_url).await?;
    let result = inference
        .generate_vision(&prompt, &[image_b64], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    let parsed: serde_json::Value = serde_json::from_str(&result.text).map_err(|e| {
        crate::MediaError::VisionParse(format!(
            "Failed to parse color analysis result: {} — raw: {}",
            e,
            &result.text[..200.min(result.text.len())]
        ))
    })?;
    Ok(parsed)
}

/// Analyze the photographic composition of an image.
///
/// Returns a JSON object with focal_point, rule_of_thirds, leading_lines,
/// depth_of_field, perspective, framing, symmetry, negative_space.
/// Unparseable model output is an error.
///
pub async fn analyze_composition(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<serde_json::Value, crate::MediaError> {
    let prompt = crate::templates::render(template_env, "tag_composition", &HashMap::new())?;

    let params = LLMParameters::default();

    let image_b64 = load_image_as_png_base64(image_url).await?;
    let result = inference
        .generate_vision(&prompt, &[image_b64], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    let parsed: serde_json::Value = serde_json::from_str(&result.text).map_err(|e| {
        crate::MediaError::VisionParse(format!(
            "Failed to parse composition analysis result: {} — raw: {}",
            e,
            &result.text[..200.min(result.text.len())]
        ))
    })?;
    Ok(parsed)
}

/// Generate a descriptive caption for an image.
///
/// Returns plain text describing the scene (subject, setting, lighting,
/// colors, composition, mood).
///
pub async fn caption_scene(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    image_url: &str,
    vision_model: Option<&str>,
) -> Result<String, crate::MediaError> {
    let mut vars = HashMap::new();
    vars.insert("style", "descriptive");
    let prompt = crate::templates::render(template_env, "caption", &vars)?;

    let params = LLMParameters::default();

    let image_b64 = load_image_as_png_base64(image_url).await?;
    let result = inference
        .generate_vision(&prompt, &[image_b64], &params, vision_model)
        .await
        .map_err(|e| crate::MediaError::VisionApi(format!("Vision LLM call failed: {}", e)))?;

    Ok(result.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::load_image_as_png_base64;

    /// A 1×1 PNG fixture (the same byte pattern the corpus server's OCR
    /// guard tests use).
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// The payload must be RAW base64 PNG — no data: prefix (the bridge adds
    /// it) and decodable to a PNG (the bridge declares the MIME).
    fn assert_raw_png_base64(b64: &str) {
        assert!(!b64.starts_with("data:"), "raw base64, not a data URI");
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("valid base64");
        assert_eq!(
            &bytes[..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            "the payload must decode to a PNG"
        );
    }

    #[tokio::test]
    async fn loads_local_file_as_raw_png_base64() {
        let path = std::env::temp_dir().join(format!(
            "vision-load-test-{}-{}.png",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, TINY_PNG).expect("fixture write");
        let b64 = load_image_as_png_base64(path.to_str().unwrap())
            .await
            .expect("local file loads");
        assert_raw_png_base64(&b64);
        let _ = std::fs::remove_file(&path);
    }

    /// Pre-built data URIs must be UNWRAPPED — passing them whole would make
    /// the bridge double-wrap into `data:image/png;base64,data:...` garbage
    /// (the video_caption path's former fate).
    #[tokio::test]
    async fn prebuilt_data_uri_is_unwrapped_not_double_wrapped() {
        use base64::Engine;
        let data_uri = format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(TINY_PNG)
        );
        let b64 = load_image_as_png_base64(&data_uri)
            .await
            .expect("data URI loads");
        assert_raw_png_base64(&b64);
    }

    #[tokio::test]
    async fn missing_file_errors_not_silently_empty() {
        let result = load_image_as_png_base64("/nonexistent/no-such-image.png").await;
        assert!(
            result.is_err(),
            "a missing image must error — never silently pass an empty payload"
        );
    }
}
