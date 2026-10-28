//! Media layer tools — store and recall time-keyed annotation layers over
//! stored video/audio recordings.
//!
//! The media sibling of the educt transcript-store tools: transcripts anchor
//! layers to word indices, media layers anchor to source-PTS second ranges
//! over an immutable media file, with provenance following the same
//! interaction model. The store validates before persisting; a failing
//! layer is rejected with the named invariant and nothing is written.
//! Layers are database rows — these tools never modify source media bytes.

use crate::layer_store::{self, LayerProvenance, MediaLayerFilter, MediaLayerItem, NewMediaLayer};
use crate::tools::educt::parse_json_value;
use crate::types::{
    LayersGetRequest, LayersListRequest, LayersShotPassRequest, LayersStoreItemRequest,
    LayersStoreRequest,
};

use crate::*;

/// Default scene threshold — ffmpeg's conventional `gt(scene,0.3)`.
pub(crate) const DEFAULT_SHOT_THRESHOLD: f64 = 0.3;

/// Default minimum shot length in seconds — suppresses cut noise so a
/// single hard transition cannot fragment into micro-shots.
pub(crate) const DEFAULT_MIN_SHOT_LENGTH_SECS: f64 = 0.5;

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
