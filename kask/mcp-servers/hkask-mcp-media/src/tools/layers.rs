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
    LayersGetRequest, LayersListRequest, LayersStoreItemRequest, LayersStoreRequest,
};

use crate::*;

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
}
