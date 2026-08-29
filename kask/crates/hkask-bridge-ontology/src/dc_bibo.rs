//! Dublin Core + BIBO + CiTO vocabulary — the state (entity) axis.
//!
//! Canonical URI constants and mapping helpers for bibliographic metadata,
//! resource typing, and citation relationships. This is the universal "what
//! is this" axis of the dual-axis framework (P5.4): every artifact carries a
//! state identity drawn from this vocabulary.
//!
//! Reference: <https://www.dublincore.org/specifications/dublin-core/dcmi-terms/>

/// A Dublin Core / BIBO / CiTO concept URI.
pub type DcConcept = &'static str;

// ── Dublin Core Terms ────────────────────────────────────────────────────

pub const TITLE: DcConcept = "dcterms:title";
pub const CREATOR: DcConcept = "dcterms:creator";
pub const CONTRIBUTOR: DcConcept = "dcterms:contributor";
pub const PUBLISHER: DcConcept = "dcterms:publisher";
pub const DATE: DcConcept = "dcterms:date";
pub const CREATED: DcConcept = "dcterms:created";
pub const MODIFIED: DcConcept = "dcterms:modified";
pub const DESCRIPTION: DcConcept = "dcterms:description";
pub const FORMAT: DcConcept = "dcterms:format";
pub const IDENTIFIER: DcConcept = "dcterms:identifier";
pub const SOURCE: DcConcept = "dcterms:source";
pub const LANGUAGE: DcConcept = "dcterms:language";
pub const RIGHTS: DcConcept = "dcterms:rights";
pub const SUBJECT: DcConcept = "dcterms:subject";
pub const TYPE: DcConcept = "dcterms:type";

// ── Dublin Core Type Vocabulary ───────────────────────────────────────────

pub const STILL_IMAGE: DcConcept = "dcterms:StillImage";
pub const MOVING_IMAGE: DcConcept = "dcterms:MovingImage";
pub const SOUND: DcConcept = "dcterms:Sound";
pub const TEXT: DcConcept = "dcterms:Text";
pub const DATASET: DcConcept = "dcterms:Dataset";
pub const SOFTWARE: DcConcept = "dcterms:Software";
pub const COLLECTION: DcConcept = "dcterms:Collection";
pub const BIBLIOGRAPHIC_RESOURCE: DcConcept = "dcterms:BibliographicResource";

// ── BIBO (Bibliographic Ontology) ─────────────────────────────────────────

pub const ARTICLE: DcConcept = "bibo:Article";
pub const ACADEMIC_ARTICLE: DcConcept = "bibo:AcademicArticle";
pub const JOURNAL: DcConcept = "bibo:Journal";
pub const BOOK: DcConcept = "bibo:Book";
pub const BOOK_SECTION: DcConcept = "bibo:BookSection";
pub const THESIS: DcConcept = "bibo:Thesis";
pub const WEBPAGE: DcConcept = "bibo:Webpage";
pub const DOCUMENT: DcConcept = "bibo:Document";
pub const PREPRINT: DcConcept = "bibo:Preprint";
pub const PROCEEDINGS: DcConcept = "bibo:Proceedings";
pub const REPORT: DcConcept = "bibo:Report";
pub const MANUSCRIPT: DcConcept = "bibo:Manuscript";

// ── CiTO (Citation Typing Ontology) ───────────────────────────────────────

pub const CITES: DcConcept = "cito:cites";
pub const IS_CITED_BY: DcConcept = "cito:isCitedBy";
pub const SUPPORTS: DcConcept = "cito:supports";
pub const REFUTES: DcConcept = "cito:refutes";
pub const DISCUSSES: DcConcept = "cito:discusses";
pub const REVIEWS: DcConcept = "cito:reviews";
pub const REPLIES_TO: DcConcept = "cito:repliesTo";
pub const USES_DATA_FROM: DcConcept = "cito:usesDataFrom";
pub const CITES_AS_DATA_SOURCE: DcConcept = "cito:citesAsDataSource";
pub const CITES_AS_EVIDENCE: DcConcept = "cito:citesAsEvidence";

// ── Mapping helpers ───────────────────────────────────────────────────────

/// Map a MIME type to its Dublin Core type.
///
/// Office document MIME types (docx/pptx → Text, xlsx → Dataset) are included
/// so the corpus ingest path — whose supported formats are exactly
/// pdf/markdown/html/plain/docx/pptx/xlsx/csv — can ground every converted
/// artifact's state identity (the crate-level "every artifact carries a state
/// identity" contract).
pub fn mime_to_dc_type(mime: &str) -> Option<DcConcept> {
    match mime {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/tiff" => Some(STILL_IMAGE),
        "video/mp4" | "video/webm" | "video/quicktime" => Some(MOVING_IMAGE),
        "audio/mpeg" | "audio/wav" | "audio/ogg" | "audio/flac" => Some(SOUND),
        "text/plain" | "text/markdown" | "text/html" | "application/pdf" => Some(TEXT),
        // Word and PowerPoint documents are text artifacts.
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Some(TEXT),
        // Spreadsheets are tabular data.
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some(DATASET),
        "application/json" | "text/csv" => Some(DATASET),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_to_dc_type_covers_corpus_ingest_formats() {
        // Every format `corpus_convert` supports must ground to a DC type —
        // the "every artifact carries a state identity" contract.
        assert_eq!(mime_to_dc_type("application/pdf"), Some(TEXT));
        assert_eq!(mime_to_dc_type("text/markdown"), Some(TEXT));
        assert_eq!(mime_to_dc_type("text/html"), Some(TEXT));
        assert_eq!(mime_to_dc_type("text/plain"), Some(TEXT));
        assert_eq!(mime_to_dc_type("text/csv"), Some(DATASET));
        assert_eq!(
            mime_to_dc_type(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            Some(TEXT)
        );
        assert_eq!(
            mime_to_dc_type(
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            ),
            Some(TEXT)
        );
        assert_eq!(
            mime_to_dc_type("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
            Some(DATASET)
        );
    }

    #[test]
    fn mime_to_dc_type_maps_media_families() {
        assert_eq!(mime_to_dc_type("image/png"), Some(STILL_IMAGE));
        assert_eq!(mime_to_dc_type("video/mp4"), Some(MOVING_IMAGE));
        assert_eq!(mime_to_dc_type("audio/wav"), Some(SOUND));
        assert_eq!(mime_to_dc_type("application/x-unknown"), None);
    }
}
