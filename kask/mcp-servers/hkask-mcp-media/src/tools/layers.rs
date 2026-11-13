//! Media layer tools — store and recall time-keyed annotation layers over
//! stored video/audio recordings.
//!
//! Tool index (family modules — the concern-family split, operator
//! decision 2026-11-01, executed over five strangler commits search ->
//! export -> store -> edl -> passes, with `layers.rs` as the module
//! root):
//!
//! | Tool | Family | Concern |
//! |------|--------|---------|
//! | `layers_store` | store | Store a layer with validated items |
//! | `layers_list` | store | List layers (filtered by media/kind) |
//! | `layers_get` | store | Get one layer by id |
//! | `layers_shot_pass` | passes | Deterministic shot boundaries (ffmpeg) |
//! | `layers_semantic_pass` | passes | Bounded vision pass (semantic/object/OCR) |
//! | `layers_audio_pass` | passes | Bounded audio pass (non-speech audio events) |
//! | `layers_stitch_tracks` | passes | Stitch detections into tracks (IoU-linked) |
//! | `layers_search` | search | Lexical search across layers + transcripts |
//! | `layers_edl_compose` | edl | Compose researcher selections into an EDL layer |
//! | `layers_render_edl` | edl | Render an EDL into a playable gallery asset |
//! | `layers_render_redactions` | edl | Render tier-1 redactions (blur/pixelate/black/delogo) |
//! | `layers_export` | export | Export layer items as corpus passages |
//! | `layers_transcript` | store | Read one recording's stored layers as one time-ordered event surface |
//!
//! The media sibling of the educt transcript-store tools: transcripts anchor
//! layers to word indices, media layers anchor to source-PTS second ranges
//! over an immutable media file, with provenance following the same
//! interaction model. The store validates before persisting; a failing
//! layer is rejected with the named invariant and nothing is written.
//! Layers are database rows — these tools never modify source media bytes.

pub(crate) use crate::layer_pass::{
    LayerAudioChunk, LayerPassError, LayerVisionChunk, run_layer_pass_audio, run_layer_pass_vision,
};
pub(crate) use crate::layer_store::{
    self, LayerProvenance, MediaLayerFilter, MediaLayerItem, MediaLayerKind, MediaLayerRecord,
    NewMediaLayer, StoredMediaLayerItem,
};
pub(crate) use crate::redaction::{self, RedactionDecision, RedactionDecisionError, SegmentMode};
pub(crate) use crate::tools::educt::{
    SEARCH_TEXT_BYTE_BUDGET, WorkingTranscript, lexical_citations, lexical_item_matches,
    map_store_error, validate_lexical_search, working_transcript,
};
pub(crate) use crate::tools::educt::{admit_media_source, parse_json_value, source_is_audio_only};
pub(crate) use crate::tools::processing::SourceSnapshot;
pub(crate) use crate::transcript_pass::PassMode;
pub(crate) use crate::transcript_store::{self, TranscriptFilter};
pub(crate) use crate::types::{
    LayersAudioPassRequest, LayersExportRequest, LayersGetRequest, LayersListRequest,
    LayersRenderRedactionsRequest, LayersShotPassRequest, LayersStitchTracksRequest,
    LayersStoreItemRequest, LayersStoreRequest, LayersTranscriptRequest,
};
pub(crate) use hkask_storage::database::driver::DatabaseDriver;
pub(crate) use std::sync::atomic::{AtomicU64, Ordering};

use crate::*;

mod edl;
mod export;
mod passes;
mod search;
mod store;

pub(crate) use passes::map_layer_pass_error;
pub use passes::{
    AudioEventSpan, AudioPassOutput, DetectedObject, NormalizedBox, OcrItem, SemanticCaption,
    SemanticEvent, SemanticPassOutput,
};

/// Upper bound on layers examined per recording in the unified search —
/// a recording with more kind-matching layers than this is a corpus-size
/// error the operator should see, not silently half-searched.
const MAX_LAYERS_PER_RECORDING: usize = 1000;

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
