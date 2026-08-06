//! MovieLabs Ontology for Media Creation (OMC) vocabulary bridge.
//!
//! Canonical concept URIs for media-production workflows (capture → post →
//! distribution). OMC is the MovieLabs standard ontology for media creation.
//! We anchor to OMC rather than inventing our own taxonomy.
//!
//! Reference: <https://movielabs.com/ontology-for-media-creation/>
//! Source: <https://github.com/MovieLabs/OMC>
//!
//! This module holds the OMC concept vocabulary and the shared concept→explain-tool
//! dispatch function. Server-specific tool-name→concept mapping lives in the media
//! server (that is the server's business), but the concept→explain-tool mapping is
//! shared: both the media MCP server and the media widget need it, and duplicating
//! it in two crates (kept in sync "by convention") is the `.rules` constant-
//! duplication drift class.

/// An OMC concept URI — the canonical identifier for a media-creation concept.
pub type OmcConcept = &'static str;

// ── OMC concept constants (STAR seed terms) ──────────────────────────────
//
// These are the top-level OMC concepts most directly entailed by media-tool
// outputs. OMC is large; we extract only the seed terms the tools actually
// produce, plus their direct logical entailments (a `Version` is a
// `CreativeWork`, a `Shot` is part of a `Scene`, etc.).

/// A distinct intellectual or artistic creation — the root creative artifact.
/// OMC: `omc:CreativeWork` (analogous to `dcterms:Work`).
pub const CREATIVE_WORK: OmcConcept = "omc:CreativeWork";
/// A continuous sequence of media — a single rendered image or video clip.
/// OMC: `omc:Scene` (a contiguous segment of a creative work).
pub const SCENE: OmcConcept = "omc:Scene";
/// A single camera capture — a frame or take within a scene.
/// OMC: `omc:Shot`.
pub const SHOT: OmcConcept = "omc:Shot";
/// An ordered series of scenes — a multi-step media workflow output.
/// OMC: `omc:Sequence`.
pub const SEQUENCE: OmcConcept = "omc:Sequence";
/// A person or system participating in media creation (model, artist, tool).
/// OMC: `omc:Participant`.
pub const PARTICIPANT: OmcConcept = "omc:Participant";
/// A source media asset — the raw input to a transform or generation.
/// OMC: `omc:MediaSource`.
pub const MEDIA_SOURCE: OmcConcept = "omc:MediaSource";
/// A managed media asset in the gallery — a stored, tagged, retrievable item.
/// OMC: `omc:Asset`.
pub const ASSET: OmcConcept = "omc:Asset";
/// A unit of production work — a workflow execution, a generation job.
/// OMC: `omc:Task`.
pub const TASK: OmcConcept = "omc:Task";
/// A derived or modified form of a creative work — an upscale, transform,
/// or remix output. OMC: `omc:Version` (a version is a creative work).
pub const VERSION: OmcConcept = "omc:Version";

/// All OMC concepts, for validation or iteration.
pub const ALL_CONCEPTS: &[OmcConcept] = &[
    CREATIVE_WORK,
    SCENE,
    SHOT,
    SEQUENCE,
    PARTICIPANT,
    MEDIA_SOURCE,
    ASSET,
    TASK,
    VERSION,
];

/// The OMC concept → explain tool mapping (the "I" pattern — ontology-bounded
/// affordances). Shared between the media MCP server and the media widget so
/// both sides agree on which explain tool a given OMC concept dispatches.
///
/// - `omc:Scene` / `omc:Asset` → `gallery_analyze` (scene/asset inspection)
/// - Others (CreativeWork, Version, MediaSource, Sequence, Shot, Participant,
///   Task) → `describe_image` (vision caption)
/// - Empty/unknown → `describe_image` (the general vision fallback)
pub fn explain_tool_for(omc: &str) -> &'static str {
    match omc {
        SCENE | ASSET => "gallery_analyze",
        _ => "describe_image",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omc_seed_concepts_are_omc_namespaced() {
        for concept in ALL_CONCEPTS {
            assert!(
                concept.starts_with("omc:"),
                "OMC concept must be omc-namespaced: {concept}"
            );
        }
    }

    #[test]
    fn omc_creative_work_is_root() {
        assert_eq!(CREATIVE_WORK, "omc:CreativeWork");
    }
}
