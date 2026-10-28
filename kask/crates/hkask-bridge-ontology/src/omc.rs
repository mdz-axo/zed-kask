//! MovieLabs Ontology for Media Creation (OMC) vocabulary bridge.
//!
//! Canonical concept URIs for media-production workflows (capture → post →
//! distribution). OMC is the MovieLabs standard ontology for media creation.
//! We anchor to OMC rather than inventing our own taxonomy.
//!
//! Reference: <https://movielabs.com/ontology-for-media-creation/>
//! Source: <https://github.com/MovieLabs/OMC> — official RDF artifact
//! `OMC-RDF/OntologyMediaCreation-OMC/omc.ttl` (v2.8, namespace
//! `https://movielabs.com/omc/rdf/schema/v2.8#`).
//!
//! The full OMC v2.8 artifact (with its Creative Works companion) is loaded
//! from `sources/omc/` and resolved through `published`; this module names
//! only the concepts hKask code emits, and `all_terms_are_official` fails the
//! build unless each is published.
//!
//! This module holds the OMC concept vocabulary and the shared concept→explain-tool
//! dispatch function. Server-specific tool-name→concept mapping lives in the media
//! server (that is the server's business), but the concept→explain-tool mapping is
//! shared: both the media MCP server and the media widget need it, and duplicating
//! it in two crates (kept in sync "by convention") is the `.rules` constant-
//! duplication drift class.

/// An OMC concept URI — the canonical identifier for a media-creation concept.
pub type OmcConcept = &'static str;

// ── Named OMC constants used by media tools ───────────────────────────────
//
// The complete pinned OMC vocabulary resolves through `published`; these
// constants are consumer selections, not an extracted subset of the source.

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
/// OMC: `omc:Capture` (an AssetAsFunction for captured material; OMC v2.8
/// publishes no `MediaSource` class).
pub const CAPTURE: OmcConcept = "omc:Capture";
/// A managed media asset in the gallery — a stored, tagged, retrievable item.
/// OMC: `omc:Asset`.
pub const ASSET: OmcConcept = "omc:Asset";
/// A unit of production work — a workflow execution, a generation job.
/// OMC: `omc:Task`.
pub const TASK: OmcConcept = "omc:Task";
/// The origin and creation account attached to a media asset.
/// OMC: `omc:Provenance`, linked from `omc:Asset` by `omc:hasProvenance`.
pub const PROVENANCE: OmcConcept = "omc:Provenance";
/// A service participating in media creation.
pub const SERVICE: OmcConcept = "omc:Service";
/// A participant's production role, connecting one participant to one task.
pub const ROLE: OmcConcept = "omc:Role";
/// The current lifecycle state of a task.
pub const STATE: OmcConcept = "omc:State";
/// A controlled descriptor attached to a task state.
pub const STATE_DESCRIPTOR: OmcConcept = "omc:StateDescriptor";
/// Information defining the scope of constructing a creative work.
pub const MEDIA_CREATION_CONTEXT: OmcConcept = "omc:MediaCreationContext";

/// Asset → provenance.
pub const HAS_PROVENANCE: OmcConcept = "omc:hasProvenance";
/// Asset → task that created it.
pub const IS_CREATED_BY_TASK: OmcConcept = "omc:isCreatedByTask";
/// Task → current state.
pub const HAS_STATE: OmcConcept = "omc:hasState";
/// State → controlled descriptor.
pub const HAS_STATE_DESCRIPTOR: OmcConcept = "omc:hasStateDescriptor";
/// Provenance → responsible participant.
pub const IS_CREATED_BY: OmcConcept = "omc:isCreatedBy";
/// Role → task.
pub const HAS_TASK: OmcConcept = "omc:hasTask";
/// Role → participant.
pub const HAS_PARTICIPANT: OmcConcept = "omc:hasParticipant";
/// Provenance creation timestamp.
pub const CREATED_ON: OmcConcept = "omc:createdOn";
/// A derived or modified form of a creative work — an upscale, transform,
/// or remix output. OMC: `omc:VersionInfo` (a description of a version of
/// an asset; OMC v2.8 publishes no `Version` class — versioning is modeled
/// as VersionInfo plus the `hasVersion`/`isVersionOf` properties).
pub const VERSION_INFO: OmcConcept = "omc:VersionInfo";

/// Every concept this module names, for the publication guard.
#[cfg(test)]
const ALL_CONCEPTS: &[OmcConcept] = &[
    CREATIVE_WORK,
    SCENE,
    SHOT,
    SEQUENCE,
    PARTICIPANT,
    CAPTURE,
    ASSET,
    TASK,
    PROVENANCE,
    SERVICE,
    ROLE,
    STATE,
    STATE_DESCRIPTOR,
    MEDIA_CREATION_CONTEXT,
    HAS_PROVENANCE,
    IS_CREATED_BY_TASK,
    HAS_STATE,
    HAS_STATE_DESCRIPTOR,
    IS_CREATED_BY,
    HAS_TASK,
    HAS_PARTICIPANT,
    CREATED_ON,
    VERSION_INFO,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every concept in this module is published in the
    /// loaded OMC v2.8 artifact.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_CONCEPTS {
            assert!(
                crate::published::contains(term),
                "{term} is not published in the loaded OMC v2.8 artifact"
            );
        }
    }
}

/// The OMC concept → explain tool mapping (the "I" pattern — ontology-bounded
/// affordances). Shared between the media MCP server and the media widget so
/// both sides agree on which explain tool a given OMC concept dispatches.
///
/// - `omc:Scene` / `omc:Asset` → `gallery_analyze` (scene/asset inspection)
/// - Others (CreativeWork, VersionInfo, Capture, Sequence, Shot, Participant,
///   Task) → `describe_image` (vision caption)
/// - Empty/unknown → `describe_image` (the general vision fallback)
pub fn explain_tool_for(omc: &str) -> &'static str {
    match omc {
        SCENE | ASSET => "gallery_analyze",
        _ => "describe_image",
    }
}
