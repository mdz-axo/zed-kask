//! MovieLabs Ontology for Media Creation (OMC) bridge for hkask-mcp-media.
//!
//! Maps hKask media server concepts to the MovieLabs OMC v2.8 standard.
//! OMC is built by the Hollywood studios consortium (Disney, Warner Bros,
//! Paramount, Universal, Sony) for media production workflows.
//!
//! Reference: <https://mc.movielabs.com/docs/ontology/>
//! Reference: <https://github.com/MovieLabs/OMC>
//!
//! Pattern: thin mapping layer — canonical URI constants, mapping functions,
//! no dependencies, no reasoners, no overhead ≤120 lines.
//!
//! # Shared Bridge Integration
//!
//! Uses [`hkask_bridge_dublincore`] for media type classification
//! (e.g., `dctypes:StillImage`, `dctypes:Sound`) and [`hkask_bridge_dublincore`]
//! for production step classification.

/// An OMC concept URI.
pub type OmcConcept = &'static str;

// ── Assets ────────────────────────────────────────────────────────────────

/// An image asset — still visual content.
/// hKask mapping: generate_image, gallery images, transform_image
pub const IMAGE_ASSET: OmcConcept = "omc:Image";

/// An audio asset — sound content, tracks, channels, compositions.
/// hKask mapping: generate_speech, transcribe, audio_capture
pub const AUDIO_ASSET: OmcConcept = "omc:Audio";

/// A CG (computer graphics) asset — 3D models, rendered content.
pub const CG_ASSET: OmcConcept = "omc:CG";

// ── Asset metadata ────────────────────────────────────────────────────────

/// Camera metadata — exposure, aperture, focal length, sensor info.
/// hKask mapping: gallery EXIF data, gallery_timeline
pub const CAMERA_METADATA: OmcConcept = "omc:CameraMetadata";

/// A version of an asset — iteration or variant.
/// hKask mapping: upscale_image (new version), transform_image (variant)
pub const VERSION: OmcConcept = "omc:Version";

// ── Participants ──────────────────────────────────────────────────────────

/// A participant — person or entity involved in media creation.
/// hKask mapping: face_register, face_list, gallery_name_face
pub const PARTICIPANT: OmcConcept = "omc:Participant";

// ── Tasks ─────────────────────────────────────────────────────────────────

/// A task — a unit of work in the media creation workflow.
/// hKask mapping: media generation pipeline steps
pub const TASK: OmcConcept = "omc:Task";

// ── Creative Works ────────────────────────────────────────────────────────

/// A creative work — the final or intermediate media product.
/// hKask mapping: all generated media (images, videos, audio)
pub const CREATIVE_WORK: OmcConcept = "omc:CreativeWork";

// ── Context / narrative structure ─────────────────────────────────────────

/// A scene — a continuous action in a single location.
/// hKask mapping: video_clip, video_from_images
pub const SCENE: OmcConcept = "omc:Scene";

/// A shot — a continuous camera recording.
/// hKask mapping: video segment, video_to_gif segment
pub const SHOT: OmcConcept = "omc:Shot";

/// A sequence — a series of scenes forming a narrative unit.
/// hKask mapping: video_concat, video_remix
pub const SEQUENCE: OmcConcept = "omc:Sequence";

/// A set — the physical or virtual environment.
/// hKask mapping: image background context, video setting
pub const SET: OmcConcept = "omc:Set";

// ── Mapping helpers ───────────────────────────────────────────────────────

/// Map a media server operation to its OMC concept.
pub fn media_op_to_omc(op: &str) -> Option<OmcConcept> {
    match op {
        "generate_image" | "transform_image" | "upscale_image" => Some(IMAGE_ASSET),
        "generate_speech" | "transcribe" | "audio_capture" => Some(AUDIO_ASSET),
        "generate_video" | "video_clip" | "video_to_gif" | "video_remix" => Some(SHOT),
        "video_from_images" | "video_concat" => Some(SEQUENCE),
        "video_meme" => Some(CREATIVE_WORK),
        "image_create_collage" => Some(CREATIVE_WORK),
        "face_register" | "face_list" | "gallery_name_face" | "face_validate" => Some(PARTICIPANT),
        "gallery_timeline" => Some(CAMERA_METADATA),
        "gallery_analyze" | "describe_image" | "video_caption" => Some(TASK),
        "image_apply_style" | "image_remove_background" => Some(TASK),
        _ => None,
    }
}

/// Map a media format / MIME type to its OMC asset type.
pub fn media_type_to_omc(media_type: &str) -> Option<OmcConcept> {
    match media_type.to_lowercase().as_str() {
        "image" | "photo" | "picture" | "still" => Some(IMAGE_ASSET),
        "audio" | "sound" | "voice" | "speech" | "music" => Some(AUDIO_ASSET),
        "video" | "film" | "clip" | "movie" => Some(SHOT),
        "3d" | "model" | "cg" | "render" => Some(CG_ASSET),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_ops_map_to_omc() {
        assert_eq!(media_op_to_omc("generate_image"), Some(IMAGE_ASSET));
        assert_eq!(media_op_to_omc("generate_speech"), Some(AUDIO_ASSET));
        assert_eq!(media_op_to_omc("video_clip"), Some(SHOT));
        assert_eq!(media_op_to_omc("video_concat"), Some(SEQUENCE));
        assert_eq!(media_op_to_omc("face_register"), Some(PARTICIPANT));
        assert_eq!(media_op_to_omc("gallery_timeline"), Some(CAMERA_METADATA));
        assert_eq!(media_op_to_omc("unknown"), None);
    }

    #[test]
    fn media_types_map_to_omc() {
        assert_eq!(media_type_to_omc("image"), Some(IMAGE_ASSET));
        assert_eq!(media_type_to_omc("audio"), Some(AUDIO_ASSET));
        assert_eq!(media_type_to_omc("video"), Some(SHOT));
        assert_eq!(media_type_to_omc("text"), None);
    }
}
