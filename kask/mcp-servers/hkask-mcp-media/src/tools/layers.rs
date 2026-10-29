//! Media layer tools — store and recall time-keyed annotation layers over
//! stored video/audio recordings.
//!
//! The media sibling of the educt transcript-store tools: transcripts anchor
//! layers to word indices, media layers anchor to source-PTS second ranges
//! over an immutable media file, with provenance following the same
//! interaction model. The store validates before persisting; a failing
//! layer is rejected with the named invariant and nothing is written.
//! Layers are database rows — these tools never modify source media bytes.

use crate::layer_pass::{
    LayerAudioChunk, LayerPassError, LayerVisionChunk, run_layer_pass_audio, run_layer_pass_vision,
};
use crate::layer_store::{self, LayerProvenance, MediaLayerFilter, MediaLayerItem, NewMediaLayer};
use crate::tools::educt::parse_json_value;
use crate::tools::processing::SourceSnapshot;
use crate::transcript_pass::PassMode;
use crate::types::{
    LayersAudioPassRequest, LayersGetRequest, LayersListRequest, LayersSemanticPassRequest,
    LayersShotPassRequest, LayersStoreItemRequest, LayersStoreRequest,
};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::*;

/// Default scene threshold — ffmpeg's conventional `gt(scene,0.3)`.
pub(crate) const DEFAULT_SHOT_THRESHOLD: f64 = 0.3;

/// Default minimum shot length in seconds — suppresses cut noise so a
/// single hard transition cannot fragment into micro-shots.
pub(crate) const DEFAULT_MIN_SHOT_LENGTH_SECS: f64 = 0.5;

/// Default frames sampled per vision chunk.
pub(crate) const DEFAULT_FRAMES_PER_CHUNK: u32 = 4;
/// Upper bound on frames per vision chunk (admission limit).
pub(crate) const MAX_FRAMES_PER_CHUNK: u32 = 12;
/// Default chunk duration in time-range mode.
pub(crate) const DEFAULT_CHUNK_DURATION_SECS: f64 = 5.0;
/// Default chunks processed per call; larger plans return a resume cursor.
pub(crate) const DEFAULT_MAX_CHUNKS: u32 = 8;
/// Upper bound on chunks per call (admission limit).
pub(crate) const MAX_CHUNKS_CAP: u32 = 64;
/// Upper bound on a full chunk plan — a finer chunking than this is a
/// caller-size error, not a plan to iterate.
const MAX_PLANNED_CHUNKS: usize = 10_000;

/// The semantic vision pass's attempts and rejections — the per-pass
/// measurement (the transcript-pass counter pattern).
static SEMANTIC_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static SEMANTIC_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);

/// The audio pass's attempts and rejections — the same per-pass
/// measurement.
static AUDIO_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static AUDIO_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);

/// Map store errors to MCP wire-level kinds per-variant (never a blanket
/// internal): a validation failure is InvalidArgument carrying the named
/// invariant; a missing gallery asset is NotFound; a DB failure is Internal.
pub(crate) fn map_layer_store_error(error: layer_store::LayerStoreError) -> McpToolError {
    match error {
        layer_store::LayerStoreError::Validation(validation) => {
            McpToolError::invalid_argument(format!("layer rejected: {validation}"))
        }
        layer_store::LayerStoreError::GalleryAssetNotFound { gallery_asset_id } => {
            McpToolError::not_found(format!("gallery asset {gallery_asset_id} not found"))
        }
        layer_store::LayerStoreError::Serialization(message) => McpToolError::internal(message),
        layer_store::LayerStoreError::Db(db) => {
            McpToolError::internal(format!("media layer store: {db}"))
        }
    }
}

/// Build the ordered, disjoint shot items from strictly increasing
/// boundary PTS (frame 0 first, then every detected cut). Half-open
/// ranges: shot i runs `[starts[i], starts[i+1])` and the final shot ends
/// at the probed duration. Cut boundaries at or past the duration cannot
/// start a shot and are dropped. `None` means no cuts — a single scene,
/// which is the caller's named no-shots degradation.
fn shot_items(boundaries: &[f64], duration_secs: f64) -> Option<Vec<MediaLayerItem>> {
    let Some((first, cuts)) = boundaries.split_first() else {
        return None;
    };
    let first = first.max(0.0);
    let cuts: Vec<f64> = cuts
        .iter()
        .copied()
        .filter(|cut| cut.is_finite() && *cut > first && *cut <= duration_secs)
        .collect();
    if cuts.is_empty() {
        return None;
    }
    let mut starts = Vec::with_capacity(cuts.len() + 1);
    starts.push(first);
    starts.extend(cuts);
    Some(
        starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let end = if index + 1 < starts.len() {
                    starts[index + 1]
                } else {
                    duration_secs
                };
                MediaLayerItem {
                    start_sec: *start,
                    end_sec: Some(end),
                    searchable_text: format!("shot {}: {:.3}s - {:.3}s", index + 1, start, end),
                    payload: serde_json::json!({
                        "shot_index": index,
                        "start_sec": start,
                        "end_sec": end,
                    }),
                    frame_asset_ids: None,
                }
            })
            .collect(),
    )
}

/// Decode the wire item form into the store's domain item: the payload
/// crosses the boundary as `AnyJsonValue` JSON text and is decoded here
/// (the transcript-store precedent).
fn decode_item(
    item: &LayersStoreItemRequest,
    index: usize,
) -> Result<MediaLayerItem, McpToolError> {
    let payload = parse_json_value(item.payload.0.clone(), &format!("items[{index}].payload"))?;
    Ok(MediaLayerItem {
        start_sec: item.start_sec,
        end_sec: item.end_sec,
        searchable_text: item.searchable_text.clone(),
        payload,
        frame_asset_ids: item.frame_asset_ids.clone(),
    })
}

/// The semantic vision pass's typed chunk output — the schema the model is
/// held to (the template embeds it; the deterministic layer validates it).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SemanticPassOutput {
    pub caption: SemanticCaption,
    #[serde(default)]
    pub events: Vec<SemanticEvent>,
    #[serde(default)]
    pub objects: Vec<DetectedObject>,
    #[serde(default)]
    pub ocr: Vec<OcrItem>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SemanticCaption {
    pub text: String,
    /// Confidence in the reading, 0..1.
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SemanticEvent {
    pub label: String,
    /// Confidence in the observation, 0..1.
    pub confidence: f64,
}

/// A normalized bounding box: x,y is the top-left corner in 0..1 relative
/// to the frame image, w,h its extent — the box lies inside the frame.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct NormalizedBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DetectedObject {
    pub label: String,
    /// Detection confidence, 0..1.
    pub confidence: f64,
    pub r#box: NormalizedBox,
    /// Index into the chunk's attached frame order.
    pub frame_index: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct OcrItem {
    pub text: String,
    pub confidence: f64,
    pub r#box: NormalizedBox,
    pub frame_index: u32,
}

/// The audio pass's typed chunk output — the schema the model is held
/// to (the template embeds it; the deterministic layer validates it).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AudioPassOutput {
    #[serde(default)]
    pub events: Vec<AudioEventSpan>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AudioEventSpan {
    pub label: String,
    /// Chunk-relative span start, seconds.
    pub start_sec: f64,
    /// Chunk-relative span end, seconds.
    pub end_sec: f64,
    /// Confidence in the observation, 0..1.
    pub confidence: f64,
}

/// Map pass-core errors to MCP wire kinds per-variant (the educt
/// `map_pass_error` kinds: unparseable model output is `unavailable` — a
/// model failure, not a caller fault).
pub(crate) fn map_layer_pass_error(error: LayerPassError) -> McpToolError {
    match error {
        LayerPassError::UnparseableOutput { expected, raw } => {
            McpToolError::unavailable(format!("model output failed to parse as {expected}: {raw}"))
        }
        LayerPassError::Validation(validation) => {
            McpToolError::invalid_argument(format!("layer rejected: {validation}"))
        }
        LayerPassError::Prompt(message) => {
            McpToolError::internal(format!("prompt construction: {message}"))
        }
        LayerPassError::Inference(error) => {
            classify_inference_error("media layer pass failed", error)
        }
        LayerPassError::NotConfigured { kind, env } => McpToolError::permission_denied(format!(
            "no {kind} model configured — set {env} or pass an explicit model; \
             kask never falls back to a hidden code constant"
        )),
        LayerPassError::ChunkPlan(message) => {
            McpToolError::invalid_argument(format!("invalid chunk plan: {message}"))
        }
    }
}

/// Validate a model-produced normalized box — the deterministic gate on
/// the vision pass's hallucination risk: a box outside the frame is a
/// named rejection of the whole pass, never a silent clamp.
fn validate_box(chunk_index: usize, what: &str, r#box: &NormalizedBox) -> Result<(), McpToolError> {
    for (name, value) in [
        ("x", r#box.x),
        ("y", r#box.y),
        ("w", r#box.w),
        ("h", r#box.h),
    ] {
        if !value.is_finite() || value < 0.0 || value > 1.0 {
            return Err(McpToolError::invalid_argument(format!(
                "chunk {chunk_index}: {what} box {name}={value} is outside 0..1 — boxes must lie within the frame"
            )));
        }
    }
    if r#box.x + r#box.w > 1.0 + BOX_EPSILON || r#box.y + r#box.h > 1.0 + BOX_EPSILON {
        return Err(McpToolError::invalid_argument(format!(
            "chunk {chunk_index}: {what} box (x={}, y={}, w={}, h={}) extends past the frame — the box must lie inside 0..1",
            r#box.x, r#box.y, r#box.w, r#box.h
        )));
    }
    Ok(())
}

/// Tolerance for the box-inside-frame sums — floating arithmetic, not a
/// semantic allowance.
const BOX_EPSILON: f64 = 1e-9;

fn validate_confidence(
    chunk_index: usize,
    what: &str,
    confidence: f64,
) -> Result<(), McpToolError> {
    if !confidence.is_finite() || confidence < 0.0 || confidence > 1.0 {
        return Err(McpToolError::invalid_argument(format!(
            "chunk {chunk_index}: {what} confidence {confidence} is outside 0..1"
        )));
    }
    Ok(())
}

fn validate_frame_index(
    chunk_index: usize,
    what: &str,
    frame_index: u32,
    frames_count: usize,
) -> Result<(), McpToolError> {
    if frame_index as usize >= frames_count {
        return Err(McpToolError::invalid_argument(format!(
            "chunk {chunk_index}: {what} frame_index {frame_index} is outside the attached frames (0..{frames_count})"
        )));
    }
    Ok(())
}

/// One chunk's sampled-and-published frames: the gallery asset ids items
/// reference, their source PTS, the base64 PNG images for inference, and
/// the per-frame publication failures (surfaced, never swallowed).
struct ChunkFrames {
    asset_ids: Vec<String>,
    pts_sec: Vec<f64>,
    images: Vec<String>,
    publish_failures: Vec<serde_json::Value>,
}

/// Sample one chunk's frames through the extraction machinery and publish
/// them as durable gallery assets — the video_extract_frames stamping
/// contract: source identity (sha256) + source_pts_sec + frame_index +
/// sampling interval + timestamp basis, with the observed showinfo PTS.
/// Frames whose PTS falls outside the chunk range are not attached.
async fn sample_chunk_frames(
    gallery_store: &std::sync::Arc<GalleryStore>,
    ffmpeg: &crate::video::ffmpeg::FfmpegRunner,
    source: &SourceSnapshot,
    range: (f64, f64),
    frames_per_chunk: u32,
) -> Result<ChunkFrames, McpToolError> {
    let span = (range.1 - range.0).max(0.0);
    let sampling_interval = (span / f64::from(frames_per_chunk)).max(0.1);
    let frames = ffmpeg
        .extract_keyframes_in_range(
            &source.path(),
            range.0,
            sampling_interval as f32,
            frames_per_chunk,
        )
        .await
        .map_err(map_media_error)?;
    let mut chunk = ChunkFrames {
        asset_ids: Vec::new(),
        pts_sec: Vec::new(),
        images: Vec::new(),
        publish_failures: Vec::new(),
    };
    for (frame_path, pts) in (&frames).into_iter().zip(&frames.timestamps_sec) {
        if *pts < range.0 || *pts > range.1 + 1e-6 {
            continue;
        }
        let mut params = source.identity.clone();
        params["source_pts_sec"] = serde_json::json!(pts);
        params["frame_index"] = serde_json::json!(chunk.asset_ids.len());
        params["sampling_interval_sec"] = serde_json::json!(sampling_interval);
        params["timestamp_basis"] = serde_json::json!("selected_original_frame_showinfo_pts");
        let frame_index = chunk.asset_ids.len();
        let publication = std::fs::read(frame_path)
            .map_err(|error| {
                McpToolError::internal(format!(
                    "read extracted frame {}: {error}",
                    frame_path.display()
                ))
            })
            .and_then(|data| {
                let data_uri = format!(
                    "data:image/jpeg;base64,{}",
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data)
                );
                // Publication and image encoding both must succeed for a
                // frame to be attachable; either failure is collected.
                crate::assets::publish_local_media(
                    gallery_store,
                    frame_path,
                    "layers_semantic_pass",
                    "extracted",
                    crate::assets::LocalMediaFormat::Jpg,
                    &params,
                )
                .map(|record| (record, data_uri))
            });
        let publication = match publication {
            Ok(pair) => pair,
            Err(error) => {
                chunk.publish_failures.push(serde_json::json!({
                    "source_pts_sec": pts,
                    "frame_index": frame_index,
                    "cause": error.to_string(),
                }));
                continue;
            }
        };
        let asset_id = publication
            .0
            .get("gallery_asset_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                McpToolError::internal("frame publication produced no gallery asset identity")
            })?;
        match crate::gallery::vision::load_image_as_png_base64(&publication.1).await {
            Ok(image) => {
                chunk.asset_ids.push(asset_id);
                chunk.pts_sec.push(*pts);
                chunk.images.push(image);
            }
            Err(error) => {
                chunk.publish_failures.push(serde_json::json!({
                    "source_pts_sec": pts,
                    "frame_index": frame_index,
                    "cause": error.to_string(),
                }));
            }
        }
    }
    Ok(chunk)
}

#[tool_router(router = layers_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Store one media layer with time-keyed items over a local or URL video/audio file. Item ranges are source-PTS seconds validated against the ffprobe-anchored media duration; reversed or out-of-bounds ranges, blank searchable_text, frame asset ids absent from the gallery, and unknown gallery asset ids are rejected with named errors and nothing is persisted. Provenance (model, prompt_template) is recorded with the layer; created_at is stamped server-side. Layers are database rows only — source media bytes are never modified."
    )]
    pub async fn layers_store(
        &self,
        Parameters(request): Parameters<LayersStoreRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_store", async {
            ensure_local_input_exists(&request.media_path)?;
            if !crate::is_local_media_path(&request.media_path) {
                validate_tool_url_with_dns(&request.media_path).await?;
            }
            let ffmpeg = self.require_ffmpeg()?;
            let probe = ffmpeg
                .probe(&request.media_path)
                .await
                .map_err(map_media_error)?;
            let provenance = LayerProvenance {
                model: request.provenance.model,
                prompt_template: request.provenance.prompt_template,
                created_at: hkask_types::time::now_rfc3339(),
                parameters: None,
            };
            let items = request
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| decode_item(item, index))
                .collect::<Result<Vec<_>, _>>()?;
            let new_layer = NewMediaLayer {
                media_path: request.media_path.clone(),
                gallery_asset_id: request.gallery_asset_id.clone(),
                kind: request.kind,
                label: request.label.clone(),
                provenance,
                duration_secs: probe.duration_secs as f64,
                items,
            };
            let driver = &**self.gallery_store.driver();
            let summary =
                layer_store::store_layer(driver, &new_layer).map_err(map_layer_store_error)?;
            let mut value = serde_json::to_value(&summary)
                .map_err(|e| McpToolError::internal(format!("encode layer summary: {e}")))?;
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "probed_duration_secs".to_string(),
                    serde_json::json!(probe.duration_secs),
                );
                object.insert(
                    "time_basis".to_string(),
                    serde_json::json!("source_pts_seconds"),
                );
            }
            Ok(value)
        })
        .await
    }

    #[tool(
        description = "List stored media layers, newest first, optionally filtered by media_path and/or kind. Returns summaries with provenance and item counts; load full items with layers_get."
    )]
    pub async fn layers_list(
        &self,
        Parameters(request): Parameters<LayersListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_list", async {
            let driver = &**self.gallery_store.driver();
            let filter = MediaLayerFilter {
                media_path: request.media_path,
                kind: request.kind,
                limit: request.limit.unwrap_or(50).min(500) as usize,
            };
            let layers =
                layer_store::list_layers(driver, &filter).map_err(map_layer_store_error)?;
            Ok(serde_json::json!({
                "layers": layers,
                "count": layers.len(),
            }))
        })
        .await
    }

    #[tool(
        description = "Load one stored media layer with its full items: stable item ids, source-PTS time ranges, searchable text, payload, and frame asset ids. Unknown layer ids fail as not_found naming the id."
    )]
    pub async fn layers_get(
        &self,
        Parameters(request): Parameters<LayersGetRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_get", async {
            let driver = &**self.gallery_store.driver();
            let Some(record) =
                layer_store::get_layer(driver, &request.layer_id).map_err(map_layer_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "layer {} not found",
                    request.layer_id
                )));
            };
            Ok(serde_json::json!({
                "layer": record,
                "time_basis": "source_pts_seconds",
            }))
        })
        .await
    }

    #[tool(
        description = "Detect shot boundaries in a local or URL video through ffmpeg scene detection (select filter over gt(scene,threshold) with showinfo source-PTS parsing) and store one kind=shot layer of ordered, disjoint time ranges covering the detected boundaries. Defaults: threshold 0.3 (scene score, (0.0,1.0]), min_shot_length 0.5s — the exact filter and both parameters are recorded in the layer provenance. A video with no detected cuts returns status no_shots with the exact filter parameters (a named degradation, not an error; nothing is stored); media without a video stream fails as failed_precondition naming the requirement; a missing file fails as not_found naming the path. Decodes the whole video once; source media bytes are never modified."
    )]
    pub async fn layers_shot_pass(
        &self,
        Parameters(request): Parameters<LayersShotPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_shot_pass", async {
            let _admission = self.admit_heavy_operation()?;
            ensure_local_input_exists(&request.media_path)?;
            if !crate::is_local_media_path(&request.media_path) {
                validate_tool_url_with_dns(&request.media_path).await?;
            }
            let ffmpeg = self.require_ffmpeg()?;
            let threshold = request.threshold.unwrap_or(DEFAULT_SHOT_THRESHOLD);
            if !threshold.is_finite() || threshold <= 0.0 || threshold > 1.0 {
                return Err(McpToolError::invalid_argument(format!(
                    "threshold must be within (0.0, 1.0], got {threshold}"
                )));
            }
            let min_shot_length = request
                .min_shot_length
                .unwrap_or(DEFAULT_MIN_SHOT_LENGTH_SECS);
            if !min_shot_length.is_finite() || min_shot_length < 0.0 {
                return Err(McpToolError::invalid_argument(format!(
                    "min_shot_length must be >= 0.0, got {min_shot_length}"
                )));
            }
            let probe = ffmpeg
                .probe(&request.media_path)
                .await
                .map_err(map_media_error)?;
            if !probe.has_video_stream {
                return Err(McpToolError::failed_precondition(format!(
                    "shot detection requires a video stream; {} has none \
                     (probed codec {:?}, {}x{}) — no layer stored",
                    request.media_path, probe.codec, probe.width, probe.height
                )));
            }
            let detection = ffmpeg
                .detect_scene_changes(&request.media_path, threshold, min_shot_length)
                .await
                .map_err(map_media_error)?;
            let Some(items) = shot_items(&detection.boundaries_sec, probe.duration_secs as f64)
            else {
                // Named degradation: the analysis completed and found a
                // single scene — the note carries the exact parameters, and
                // nothing is stored (never an error, never silent success).
                return Ok(serde_json::json!({
                    "status": "no_shots",
                    "note": "no scene changes above the threshold were detected; no layer stored",
                    "filter": detection.filter,
                    "threshold": threshold,
                    "min_shot_length": min_shot_length,
                    "probed_duration_secs": probe.duration_secs,
                }));
            };
            let provenance = LayerProvenance {
                model: "ffmpeg".to_string(),
                prompt_template: "ffmpeg:scdet".to_string(),
                created_at: hkask_types::time::now_rfc3339(),
                parameters: Some(serde_json::json!({
                    "filter": detection.filter,
                    "threshold": threshold,
                    "min_shot_length": min_shot_length,
                })),
            };
            let new_layer = NewMediaLayer {
                media_path: request.media_path.clone(),
                gallery_asset_id: request.gallery_asset_id.clone(),
                kind: layer_store::MediaLayerKind::Shot,
                label: Some("shot boundaries".to_string()),
                provenance,
                duration_secs: probe.duration_secs as f64,
                items,
            };
            let driver = &**self.gallery_store.driver();
            let summary =
                layer_store::store_layer(driver, &new_layer).map_err(map_layer_store_error)?;
            Ok(serde_json::json!({
                "status": "completed",
                "layer": summary,
                "boundaries_sec": detection.boundaries_sec,
                "filter": detection.filter,
                "threshold": threshold,
                "min_shot_length": min_shot_length,
                "probed_duration_secs": probe.duration_secs,
                "time_basis": "source_pts_seconds",
            }))
        })
        .await
    }

    #[tool(
        description = "Generate semantic, object and OCR layers over a video's sampled frames via the bounded vision pass — one inference call per chunk. Bound by a source-PTS time_range or by the shots of one stored shot layer (exactly one required); chunks beyond max_chunks return a resume cursor, and the coverage block reports plan-vs-observed (chunks planned vs processed, frames published, frameless chunks, per-kind item counts). Frames are sampled per chunk and published as durable gallery assets (source_pts_sec + source_sha256); item frame references resolve to those assets. Model-produced boxes are validated to lie within 0..1, confidences within 0..1, and frame indices within the attached frames — violations reject the whole pass with named errors and nothing is stored. structured=true routes through the structured-output counters with the schema in the prompt (image inputs have no provider-enforced strict-schema path — the response names the mode used); structured=false (default) is the v1 schema-in-prompt mode. Media without a video stream fails as failed_precondition naming the requirement; a missing file as not_found naming the path; a shot layer with no shots returns the no_shots note. Sampled frames only — audio and unobserved intervals are not assessed; source media bytes are never modified."
    )]
    pub async fn layers_semantic_pass(
        &self,
        Parameters(request): Parameters<LayersSemanticPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_semantic_pass", async {
            let _admission = self.admit_heavy_operation()?;
            ensure_local_input_exists(&request.media_path)?;
            if !crate::is_local_media_path(&request.media_path) {
                validate_tool_url_with_dns(&request.media_path).await?;
            }
            let ffmpeg = self.require_ffmpeg()?;
            let frames_per_chunk = request.frames_per_chunk.unwrap_or(DEFAULT_FRAMES_PER_CHUNK);
            crate::validate_item_count(
                "frames_per_chunk",
                frames_per_chunk as usize,
                1,
                MAX_FRAMES_PER_CHUNK as usize,
            )?;
            let max_chunks = request.max_chunks.unwrap_or(DEFAULT_MAX_CHUNKS);
            crate::validate_item_count(
                "max_chunks",
                max_chunks as usize,
                1,
                MAX_CHUNKS_CAP as usize,
            )?;
            let chunk_duration = request
                .chunk_duration_secs
                .unwrap_or(DEFAULT_CHUNK_DURATION_SECS);
            if !chunk_duration.is_finite() || chunk_duration <= 0.0 {
                return Err(McpToolError::invalid_argument(format!(
                    "chunk_duration_secs must be finite and > 0, got {chunk_duration}"
                )));
            }
            let structured = request.structured.unwrap_or(false);
            let mode = if structured {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };

            let probe = ffmpeg
                .probe(&request.media_path)
                .await
                .map_err(map_media_error)?;
            if !probe.has_video_stream {
                return Err(McpToolError::failed_precondition(format!(
                    "the vision pass requires a video stream; {} has none \
                     (probed codec {:?}, {}x{}) — no layer stored",
                    request.media_path, probe.codec, probe.width, probe.height
                )));
            }
            let duration_secs = probe.duration_secs as f64;

            let driver = &**self.gallery_store.driver();
            let plan: Vec<(f64, f64)> = match (&request.time_range, &request.shot_layer_id) {
                (Some(_), Some(_)) => {
                    return Err(McpToolError::invalid_argument(
                        "exactly one of time_range or shot_layer_id is required — both were provided",
                    ));
                }
                (Some(range), None) => {
                    let end = range.end_sec.unwrap_or(duration_secs);
                    if !range.start_sec.is_finite() || range.start_sec < 0.0 {
                        return Err(McpToolError::invalid_argument(format!(
                            "time_range start_sec must be >= 0, got {}",
                            range.start_sec
                        )));
                    }
                    if end <= range.start_sec {
                        return Err(McpToolError::invalid_argument(format!(
                            "reversed time_range: start_sec {} >= end_sec {}",
                            range.start_sec, end
                        )));
                    }
                    if end > duration_secs + 1e-6 {
                        return Err(McpToolError::invalid_argument(format!(
                            "time_range end_sec {end} exceeds the probed duration {duration_secs}"
                        )));
                    }
                    let mut plan = Vec::new();
                    let mut start = range.start_sec;
                    while start < end - 1e-9 {
                        let chunk_end = (start + chunk_duration).min(end);
                        plan.push((start, chunk_end));
                        start = chunk_end;
                    }
                    if plan.len() > MAX_PLANNED_CHUNKS {
                        return Err(McpToolError::invalid_argument(format!(
                            "time_range with chunk_duration_secs {chunk_duration} yields {} chunks \
                             — raise chunk_duration_secs",
                            plan.len()
                        )));
                    }
                    plan
                }
                (None, Some(layer_id)) => {
                    let Some(record) = layer_store::get_layer(driver, layer_id)
                        .map_err(map_layer_store_error)?
                    else {
                        return Err(McpToolError::not_found(format!(
                            "layer {layer_id} not found"
                        )));
                    };
                    if record.kind != layer_store::MediaLayerKind::Shot {
                        return Err(McpToolError::invalid_argument(format!(
                            "shot_layer_id must reference a kind=shot layer \
                             (layers_shot_pass output), got kind {}",
                            record.kind.as_str()
                        )));
                    }
                    if record.items.is_empty() {
                        return Ok(serde_json::json!({
                            "status": "no_shots",
                            "note": "the referenced shot layer has no shots; no layer stored",
                            "shot_layer_id": layer_id,
                            "frames_per_chunk": frames_per_chunk,
                            "max_chunks": max_chunks,
                        }));
                    }
                    record
                        .items
                        .iter()
                        .map(|item| {
                            (
                                item.item.start_sec,
                                item.item.end_sec.unwrap_or(duration_secs),
                            )
                        })
                        .collect()
                }
                (None, None) => {
                    return Err(McpToolError::invalid_argument(
                        "exactly one of time_range or shot_layer_id is required",
                    ));
                }
            };
            let chunking_mode = if request.shot_layer_id.is_some() {
                "shot_layer"
            } else {
                "time_range"
            };
            let resume_from = request.resume_from.unwrap_or(0) as usize;
            if resume_from > plan.len() {
                return Err(McpToolError::invalid_argument(format!(
                    "resume_from {resume_from} is past the chunk plan ({} chunks)",
                    plan.len()
                )));
            }
            let slice_end = (resume_from + max_chunks as usize).min(plan.len());

            // Sample and publish the bounded slice's frames chunk by chunk —
            // the extraction machinery's stamping, once per chunk.
            let source =
                SourceSnapshot::capture(&self.gallery_store, &request.media_path).await?;
            let schema = serde_json::to_string(&schemars::schema_for!(SemanticPassOutput))
                .map_err(|e| McpToolError::internal(format!("schema serialization: {e}")))?;
            let mut vision_chunks: Vec<LayerVisionChunk> = Vec::new();
            let mut sampled: Vec<(usize, (f64, f64), ChunkFrames)> = Vec::new();
            let mut chunks_without_frames = Vec::new();
            let mut frames_published = 0usize;
            let mut frame_publication_failures = Vec::new();
            for chunk_index in resume_from..slice_end {
                let range = plan[chunk_index];
                let mut frames = sample_chunk_frames(
                    &self.gallery_store,
                    ffmpeg,
                    &source,
                    range,
                    frames_per_chunk,
                )
                .await?;
                if frames.images.is_empty() {
                    chunks_without_frames.push(serde_json::json!({
                        "chunk_index": chunk_index,
                        "start_sec": range.0,
                        "end_sec": range.1,
                        "publication_failures": frames.publish_failures,
                    }));
                    continue;
                }
                let mut vars: std::collections::HashMap<&str, &str> =
                    std::collections::HashMap::new();
                let start_text = format!("{:.3}", range.0);
                let end_text = format!("{:.3}", range.1);
                let pts_text = format!("{:?}", frames.pts_sec);
                vars.insert("schema", &schema);
                vars.insert("start_sec", &start_text);
                vars.insert("end_sec", &end_text);
                vars.insert("frame_pts", &pts_text);
                let prompt = crate::templates::render(
                    &self.template_env,
                    "layers_semantic_pass",
                    &vars,
                )
                .map_err(|e| McpToolError::internal(format!("Template render failed: {e}")))?;
                frames_published += frames.asset_ids.len();
                frame_publication_failures.extend(frames.publish_failures.clone());
                vision_chunks.push(LayerVisionChunk {
                    prompt,
                    images: std::mem::take(&mut frames.images),
                });
                sampled.push((chunk_index, range, frames));
            }
            if vision_chunks.is_empty() {
                // A completed bounded slice with no attachable frames — a
                // named degradation, never an error and never silent.
                return Ok(serde_json::json!({
                    "status": "no_frames",
                    "note": "no sampled frames published in the bounded slice; no inference ran; no layer stored",
                    "coverage": {
                        "chunking_mode": chunking_mode,
                        "planned_chunks": plan.len(),
                        "chunks_without_frames": chunks_without_frames,
                        "frame_publication_failures": frame_publication_failures,
                    },
                    "probed_duration_secs": probe.duration_secs,
                }));
            }

            let expected = "SemanticPassOutput {\"caption\": {\"text\", \"confidence\"}, \"events\": [...], \"objects\": [...], \"ocr\": [...]}";
            let result = run_layer_pass_vision::<SemanticPassOutput>(
                &self.vision_port,
                mode,
                request.model.as_deref(),
                &vision_chunks,
                0,
                vision_chunks.len(),
                &SEMANTIC_PASS_ATTEMPTS,
                &SEMANTIC_PASS_REJECTIONS,
                expected,
            )
            .await
            .map_err(map_layer_pass_error)?;

            // Normalize the typed outputs into per-kind items — the
            // deterministic validation gates run here (boxes, confidences,
            // frame indices, nonblank text); a violation rejects the whole
            // pass and nothing is stored.
            let mut semantic_items: Vec<MediaLayerItem> = Vec::new();
            let mut object_items: Vec<MediaLayerItem> = Vec::new();
            let mut ocr_items: Vec<MediaLayerItem> = Vec::new();
            for (output, (chunk_index, range, frames)) in
                result.outputs.iter().zip(&sampled)
            {
                let chunk_index = *chunk_index;
                let caption_text = output.caption.text.trim();
                if caption_text.is_empty() {
                    return Err(McpToolError::invalid_argument(format!(
                        "chunk {chunk_index}: caption text is blank"
                    )));
                }
                validate_confidence(chunk_index, "caption", output.caption.confidence)?;
                semantic_items.push(MediaLayerItem {
                    start_sec: range.0,
                    end_sec: Some(range.1),
                    searchable_text: caption_text.to_string(),
                    payload: serde_json::json!({
                        "kind": "caption",
                        "confidence": output.caption.confidence,
                    }),
                    frame_asset_ids: Some(frames.asset_ids.clone()),
                });
                for event in &output.events {
                    let label = event.label.trim();
                    if label.is_empty() {
                        return Err(McpToolError::invalid_argument(format!(
                            "chunk {chunk_index}: event label is blank"
                        )));
                    }
                    validate_confidence(chunk_index, "event", event.confidence)?;
                    semantic_items.push(MediaLayerItem {
                        start_sec: range.0,
                        end_sec: Some(range.1),
                        searchable_text: label.to_string(),
                        payload: serde_json::json!({
                            "kind": "event",
                            "label": label,
                            "confidence": event.confidence,
                        }),
                        frame_asset_ids: Some(frames.asset_ids.clone()),
                    });
                }
                for object in &output.objects {
                    let label = object.label.trim();
                    if label.is_empty() {
                        return Err(McpToolError::invalid_argument(format!(
                            "chunk {chunk_index}: object label is blank"
                        )));
                    }
                    validate_confidence(chunk_index, "object", object.confidence)?;
                    validate_box(chunk_index, "object", &object.r#box)?;
                    validate_frame_index(
                        chunk_index,
                        "object",
                        object.frame_index,
                        frames.asset_ids.len(),
                    )?;
                    let frame_asset_id = frames.asset_ids[object.frame_index as usize].clone();
                    object_items.push(MediaLayerItem {
                        start_sec: range.0,
                        end_sec: Some(range.1),
                        searchable_text: format!("object: {label}"),
                        payload: serde_json::json!({
                            "label": label,
                            "confidence": object.confidence,
                            "box": {
                                "x": object.r#box.x,
                                "y": object.r#box.y,
                                "w": object.r#box.w,
                                "h": object.r#box.h,
                            },
                            "frame_index": object.frame_index,
                        }),
                        frame_asset_ids: Some(vec![frame_asset_id]),
                    });
                }
                for ocr in &output.ocr {
                    let text = ocr.text.trim();
                    if text.is_empty() {
                        return Err(McpToolError::invalid_argument(format!(
                            "chunk {chunk_index}: ocr text is blank"
                        )));
                    }
                    validate_confidence(chunk_index, "ocr", ocr.confidence)?;
                    validate_box(chunk_index, "ocr", &ocr.r#box)?;
                    validate_frame_index(
                        chunk_index,
                        "ocr",
                        ocr.frame_index,
                        frames.asset_ids.len(),
                    )?;
                    let frame_asset_id = frames.asset_ids[ocr.frame_index as usize].clone();
                    ocr_items.push(MediaLayerItem {
                        start_sec: range.0,
                        end_sec: Some(range.1),
                        searchable_text: text.to_string(),
                        payload: serde_json::json!({
                            "text": text,
                            "confidence": ocr.confidence,
                            "box": {
                                "x": ocr.r#box.x,
                                "y": ocr.r#box.y,
                                "w": ocr.r#box.w,
                                "h": ocr.r#box.h,
                            },
                            "frame_index": ocr.frame_index,
                        }),
                        frame_asset_ids: Some(vec![frame_asset_id]),
                    });
                }
            }

            // Store one layer per kind that produced items. The counts are
            // captured before the items move into their layers.
            let items_counts = serde_json::json!({
                "semantic": semantic_items.len(),
                "object": object_items.len(),
                "ocr": ocr_items.len(),
            });
            // The published frame inventory — item frame references resolve
            // against these assets, and their source PTS is the in-range
            // evidence the item ranges must contain.
            let published_frames: Vec<serde_json::Value> = sampled
                .iter()
                .flat_map(|(_, _, frames)| {
                    frames
                        .asset_ids
                        .iter()
                        .zip(&frames.pts_sec)
                        .map(|(asset_id, pts)| {
                            serde_json::json!({
                                "gallery_asset_id": asset_id,
                                "source_pts_sec": pts,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let provenance = LayerProvenance {
                model: result.model,
                prompt_template: "layers_semantic_pass".to_string(),
                created_at: hkask_types::time::now_rfc3339(),
                parameters: Some(serde_json::json!({
                    "sampling": {
                        "frames_per_chunk": frames_per_chunk,
                    },
                    "chunking": {
                        "mode": chunking_mode,
                        "chunk_duration_secs": if chunking_mode == "time_range" {
                            serde_json::json!(chunk_duration)
                        } else {
                            serde_json::Value::Null
                        },
                        "max_chunks": max_chunks,
                        "resume_from": resume_from,
                        "planned_chunks": plan.len(),
                        "processed_chunks": sampled.len(),
                    },
                    "structured": structured,
                })),
            };
            let mut kinds = Vec::new();
            let mut kinds_skipped = Vec::new();
            for (kind, label, items, count_key) in [
                (
                    layer_store::MediaLayerKind::Semantic,
                    "semantic captions and events",
                    &mut semantic_items,
                    "semantic",
                ),
                (
                    layer_store::MediaLayerKind::Object,
                    "detected objects",
                    &mut object_items,
                    "object",
                ),
                (
                    layer_store::MediaLayerKind::Ocr,
                    "ocr text",
                    &mut ocr_items,
                    "ocr",
                ),
            ] {
                if items.is_empty() {
                    kinds_skipped.push(count_key);
                    continue;
                }
                let summary = layer_store::store_layer(
                    driver,
                    &NewMediaLayer {
                        media_path: request.media_path.clone(),
                        gallery_asset_id: request.gallery_asset_id.clone(),
                        kind,
                        label: Some(label.to_string()),
                        provenance: provenance.clone(),
                        duration_secs,
                        items: std::mem::take(items),
                    },
                )
                .map_err(map_layer_store_error)?;
                kinds.push(serde_json::json!({
                    "id": summary.id,
                    "kind": summary.kind.as_str(),
                    "label": summary.label,
                    "items_count": summary.items_count,
                    "provenance": summary.provenance,
                }));
            }

            let resume_cursor = if slice_end < plan.len() {
                Some(slice_end)
            } else {
                None
            };
            Ok(serde_json::json!({
                "status": "completed",
                "layers": kinds,
                "resume_from": resume_cursor,
                "coverage": {
                    "chunking_mode": chunking_mode,
                    "planned_chunks": plan.len(),
                    "processed_chunks": sampled.len(),
                    "chunks_without_frames": chunks_without_frames,
                    "frames_published": frames_published,
                    "frames": published_frames,
                    "frame_publication_failures": frame_publication_failures,
                    "items": items_counts,
                    "kinds_skipped": kinds_skipped,
                    "limitation": "Sampled frames only — intervening and later events, audio, speech, and whole-recording coverage are not assessed.",
                },
                "mode": if structured {
                    serde_json::json!({
                        "requested": "structured",
                        "note": "image inputs have no provider-enforced strict-schema path — the schema rode in the prompt and the structured-output counters were used",
                    })
                } else {
                    serde_json::json!({"requested": "prompt_schema"})
                },
                "pass_stats": {
                    "attempts": SEMANTIC_PASS_ATTEMPTS.load(Ordering::Relaxed),
                    "rejections": SEMANTIC_PASS_REJECTIONS.load(Ordering::Relaxed),
                },
                "probed_duration_secs": probe.duration_secs,
                "time_basis": "source_pts_seconds",
            }))
        })
        .await
    }

    #[tool(
        description = "Generate labeled audio_event items over a recording's audio via the bounded audio pass — one chat_audio inference call per chunk. Bound by a source-PTS time_range (default: the full probed duration); chunks beyond max_chunks return a resume cursor. Model-produced event spans are validated (finite, inside the chunk, confidences within 0..1, nonblank labels) before storing one kind=audio_event layer whose provenance records the model and template — a violation rejects the whole pass and nothing is stored. Speech is not transcribed (that is the transcript pipeline's job); media without an audio stream fails as failed_precondition naming the requirement; a pass that finds no events returns a named no_events note and stores nothing. Source media bytes are never modified."
    )]
    pub async fn layers_audio_pass(
        &self,
        Parameters(request): Parameters<LayersAudioPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_audio_pass", async {
            let _admission = self.admit_heavy_operation()?;
            ensure_local_input_exists(&request.media_path)?;
            if !crate::is_local_media_path(&request.media_path) {
                validate_tool_url_with_dns(&request.media_path).await?;
            }
            let ffmpeg = self.require_ffmpeg()?;
            let max_chunks = request.max_chunks.unwrap_or(DEFAULT_MAX_CHUNKS);
            crate::validate_item_count(
                "max_chunks",
                max_chunks as usize,
                1,
                MAX_CHUNKS_CAP as usize,
            )?;
            let chunk_duration = request
                .chunk_duration_secs
                .unwrap_or(DEFAULT_CHUNK_DURATION_SECS);
            if !chunk_duration.is_finite() || chunk_duration <= 0.0 {
                return Err(McpToolError::invalid_argument(format!(
                    "chunk_duration_secs must be finite and > 0, got {chunk_duration}"
                )));
            }
            let structured = request.structured.unwrap_or(false);
            let mode = if structured {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };

            let probe = ffmpeg
                .probe(&request.media_path)
                .await
                .map_err(map_media_error)?;
            if !probe.has_audio_stream {
                return Err(McpToolError::failed_precondition(format!(
                    "the audio pass requires an audio stream; {} has none \
                     (probed codec {:?}, {}x{}) — no layer stored",
                    request.media_path, probe.codec, probe.width, probe.height
                )));
            }
            let duration_secs = probe.duration_secs as f64;

            let (range_start, range_end) = match &request.time_range {
                Some(range) => {
                    let end = range.end_sec.unwrap_or(duration_secs);
                    if !range.start_sec.is_finite() || range.start_sec < 0.0 {
                        return Err(McpToolError::invalid_argument(format!(
                            "time_range start_sec must be >= 0, got {}",
                            range.start_sec
                        )));
                    }
                    if end <= range.start_sec {
                        return Err(McpToolError::invalid_argument(format!(
                            "reversed time_range: start_sec {} >= end_sec {}",
                            range.start_sec, end
                        )));
                    }
                    if end > duration_secs + 1e-6 {
                        return Err(McpToolError::invalid_argument(format!(
                            "time_range end_sec {end} exceeds the probed duration {duration_secs}"
                        )));
                    }
                    (range.start_sec, end)
                }
                None => (0.0, duration_secs),
            };
            let mut plan: Vec<(f64, f64)> = Vec::new();
            let mut start = range_start;
            while start < range_end - 1e-9 {
                let chunk_end = (start + chunk_duration).min(range_end);
                plan.push((start, chunk_end));
                start = chunk_end;
            }
            if plan.len() > MAX_PLANNED_CHUNKS {
                return Err(McpToolError::invalid_argument(format!(
                    "the range with chunk_duration_secs {chunk_duration} yields {} chunks \
                     — raise chunk_duration_secs",
                    plan.len()
                )));
            }
            let resume_from = request.resume_from.unwrap_or(0) as usize;
            if resume_from > plan.len() {
                return Err(McpToolError::invalid_argument(format!(
                    "resume_from {resume_from} is past the chunk plan ({} chunks)",
                    plan.len()
                )));
            }
            let slice_end = (resume_from + max_chunks as usize).min(plan.len());

            // Extract each chunk's audio span to a transient wav (the
            // runner's temp dir — tracked for removal) and render its
            // prompt: the schema rides in the prompt (v1 — audio inputs
            // have no provider-enforced strict-schema path).
            let schema = serde_json::to_string(&schemars::schema_for!(AudioPassOutput))
                .map_err(|e| McpToolError::internal(format!("schema serialization: {e}")))?;
            let mut intermediates = crate::assets::ScratchIntermediates::new("layers_audio_pass");
            let mut chunks: Vec<LayerAudioChunk> = Vec::new();
            for chunk_index in resume_from..slice_end {
                let (chunk_start, chunk_end) = plan[chunk_index];
                let span_path = ffmpeg
                    .audio_trim(&request.media_path, chunk_start as f32, chunk_end as f32)
                    .await
                    .map_err(map_media_error)?;
                intermediates.track(span_path.clone());
                let start_text = format!("{:.3}", chunk_start);
                let end_text = format!("{:.3}", chunk_end);
                let mut vars: std::collections::HashMap<&str, &str> =
                    std::collections::HashMap::new();
                vars.insert("schema", &schema);
                vars.insert("start_sec", &start_text);
                vars.insert("end_sec", &end_text);
                let prompt = crate::templates::render(
                    &self.template_env,
                    "layers_audio_pass",
                    &vars,
                )
                .map_err(|e| McpToolError::internal(format!("Template render failed: {e}")))?;
                chunks.push(LayerAudioChunk {
                    prompt,
                    audio_path: span_path.to_string_lossy().into_owned(),
                });
            }

            let expected =
                "AudioPassOutput {\"events\": [{\"label\", \"start_sec\", \"end_sec\", \"confidence\"}]}";
            let result = run_layer_pass_audio::<AudioPassOutput>(
                &self.vision_port,
                mode,
                request.model.as_deref(),
                &chunks,
                0,
                chunks.len(),
                &AUDIO_PASS_ATTEMPTS,
                &AUDIO_PASS_REJECTIONS,
                expected,
            )
            .await
            .map_err(map_layer_pass_error)?;
            intermediates.cleanup().map_err(map_media_error)?;

            // Normalize chunk-relative spans onto the source timeline —
            // the deterministic gates (finite, inside the chunk, ordered,
            // confidence, nonblank) reject the whole pass; nothing is
            // stored on a violation.
            let mut items: Vec<MediaLayerItem> = Vec::new();
            for (index, output) in result.outputs.iter().enumerate() {
                let chunk_index = resume_from + index;
                let (chunk_start, chunk_end) = plan[chunk_index];
                let chunk_span = chunk_end - chunk_start;
                for event in &output.events {
                    let label = event.label.trim();
                    if label.is_empty() {
                        return Err(McpToolError::invalid_argument(format!(
                            "chunk {chunk_index}: event label is blank"
                        )));
                    }
                    validate_confidence(chunk_index, "event", event.confidence)?;
                    for (which, value) in
                        [("start_sec", event.start_sec), ("end_sec", event.end_sec)]
                    {
                        if !value.is_finite() || value < 0.0 || value > chunk_span + 1e-6 {
                            return Err(McpToolError::invalid_argument(format!(
                                "chunk {chunk_index}: event {which} {value} is outside the chunk \
                                 span 0..={chunk_span} (chunk-relative seconds)"
                            )));
                        }
                    }
                    if event.end_sec < event.start_sec {
                        return Err(McpToolError::invalid_argument(format!(
                            "chunk {chunk_index}: reversed event span: start_sec {} > end_sec {}",
                            event.start_sec, event.end_sec
                        )));
                    }
                    items.push(MediaLayerItem {
                        start_sec: chunk_start + event.start_sec,
                        end_sec: Some(chunk_start + event.end_sec),
                        searchable_text: label.to_string(),
                        payload: serde_json::json!({
                            "label": label,
                            "confidence": event.confidence,
                            "chunk_index": chunk_index,
                        }),
                        frame_asset_ids: None,
                    });
                }
            }

            let resume_cursor = if slice_end < plan.len() {
                Some(slice_end)
            } else {
                None
            };
            if items.is_empty() {
                return Ok(serde_json::json!({
                    "status": "no_events",
                    "note": "no audio events detected in the bounded slice; no layer stored",
                    "resume_from": resume_cursor,
                    "coverage": {
                        "chunking_mode": "time_range",
                        "planned_chunks": plan.len(),
                        "processed_chunks": result.outputs.len(),
                    },
                    "mode": if structured {
                        serde_json::json!({
                            "requested": "structured",
                            "note": "audio inputs have no provider-enforced strict-schema path — the schema rode in the prompt and the structured-output counters were used",
                        })
                    } else {
                        serde_json::json!({"requested": "prompt_schema"})
                    },
                    "probed_duration_secs": probe.duration_secs,
                }));
            }
            let events_count = items.len();
            let provenance = LayerProvenance {
                model: result.model,
                prompt_template: "layers_audio_pass".to_string(),
                created_at: hkask_types::time::now_rfc3339(),
                parameters: Some(serde_json::json!({
                    "chunking": {
                        "chunk_duration_secs": chunk_duration,
                        "max_chunks": max_chunks,
                        "resume_from": resume_from,
                        "planned_chunks": plan.len(),
                        "processed_chunks": result.outputs.len(),
                    },
                    "structured": structured,
                })),
            };
            let driver = &**self.gallery_store.driver();
            let summary = layer_store::store_layer(
                driver,
                &NewMediaLayer {
                    media_path: request.media_path.clone(),
                    gallery_asset_id: request.gallery_asset_id.clone(),
                    kind: layer_store::MediaLayerKind::AudioEvent,
                    label: Some("audio events".to_string()),
                    provenance,
                    duration_secs,
                    items,
                },
            )
            .map_err(map_layer_store_error)?;
            Ok(serde_json::json!({
                "status": "completed",
                "layer": summary,
                "resume_from": resume_cursor,
                "coverage": {
                    "chunking_mode": "time_range",
                    "planned_chunks": plan.len(),
                    "processed_chunks": result.outputs.len(),
                    "events_count": events_count,
                    "limitation": "The analyzed audio span only — unanalyzed intervals are not assessed.",
                },
                "mode": if structured {
                    serde_json::json!({
                        "requested": "structured",
                        "note": "audio inputs have no provider-enforced strict-schema path — the schema rode in the prompt and the structured-output counters were used",
                    })
                } else {
                    serde_json::json!({"requested": "prompt_schema"})
                },
                "pass_stats": {
                    "attempts": AUDIO_PASS_ATTEMPTS.load(Ordering::Relaxed),
                    "rejections": AUDIO_PASS_REJECTIONS.load(Ordering::Relaxed),
                },
                "probed_duration_secs": probe.duration_secs,
                "time_basis": "source_pts_seconds",
            }))
        })
        .await
    }
}

#[cfg(test)]
mod shot_tests {
    use super::*;

    fn assert_ordered_disjoint_within_duration(items: &[MediaLayerItem], duration_secs: f64) {
        for pair in items.windows(2) {
            assert!(
                pair[0].end_sec.is_some_and(|end| end <= pair[1].start_sec),
                "shots must be ordered and disjoint: {items:?}"
            );
        }
        for item in items {
            assert!(item.start_sec >= 0.0 && item.start_sec <= duration_secs);
            assert!(item.end_sec.is_some_and(|end| end <= duration_secs));
            assert!(!item.searchable_text.trim().is_empty());
        }
    }

    #[test]
    fn one_cut_yields_two_ordered_disjoint_shots() {
        let items = shot_items(&[0.0, 1.0], 2.0).expect("one cut -> two shots");
        assert_eq!(items.len(), 2);
        assert_ordered_disjoint_within_duration(&items, 2.0);
        assert_eq!(items[0].start_sec, 0.0);
        assert_eq!(items[0].end_sec, Some(1.0));
        assert_eq!(items[1].start_sec, 1.0);
        assert_eq!(items[1].end_sec, Some(2.0));
        assert_eq!(
            items[0].payload,
            serde_json::json!({"shot_index": 0, "start_sec": 0.0, "end_sec": 1.0})
        );
        assert_eq!(items[1].payload["shot_index"], serde_json::json!(1));
    }

    #[test]
    fn many_cuts_tile_the_duration_in_order() {
        let items = shot_items(&[0.0, 0.5, 1.5], 2.0).expect("two cuts -> three shots");
        assert_eq!(items.len(), 3);
        assert_ordered_disjoint_within_duration(&items, 2.0);
        let starts: Vec<f64> = items.iter().map(|item| item.start_sec).collect();
        assert_eq!(starts, vec![0.0, 0.5, 1.5]);
    }

    #[test]
    fn no_cuts_is_none_the_named_degradation_path() {
        assert!(shot_items(&[], 2.0).is_none(), "no boundaries at all");
        assert!(
            shot_items(&[0.0], 2.0).is_none(),
            "frame 0 alone is a single scene"
        );
    }

    #[test]
    fn cuts_at_or_past_the_duration_cannot_start_a_shot() {
        // A boundary past the probe duration (rounding at the container
        // edge) is dropped rather than producing an out-of-bounds shot.
        let items = shot_items(&[0.0, 1.0, 2.5], 2.0).expect("the in-bounds cut survives");
        assert_eq!(items.len(), 2);
        assert_ordered_disjoint_within_duration(&items, 2.0);
        assert_eq!(items[1].end_sec, Some(2.0));
    }
}
