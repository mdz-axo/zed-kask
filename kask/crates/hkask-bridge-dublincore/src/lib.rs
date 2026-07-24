#![forbid(unsafe_code)]
//! Ontology vocabulary bridge — Dublin Core + BIBO + CiTO + PKO.
//!
//! A shared pure-vocabulary crate — canonical URI constants and mapping helpers
//! for bibliographic metadata, resource typing, citation relationships, and
//! procedural knowledge. No dependencies, no reasoners, no overhead. Used by
//! research, docproc, media, training, and replica servers as a cross-cutting
//! vocabulary resource.
//!
//! Dublin Core reference: <https://www.dublincore.org/specifications/dublin-core/dcmi-terms/>
//! BIBO reference: <https://www.dublincore.org/specifications/bibo/>
//! CiTO reference: <https://sparontologies.github.io/cito/current/cito.html>
//! PKO reference: Carriero et al. (2025, arXiv:2503.20634)

pub mod pko;
// Re-export PKO items at crate root for backward-compatible access.
pub use pko::*;

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
pub fn mime_to_dc_type(mime: &str) -> Option<DcConcept> {
    match mime {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/tiff" => Some(STILL_IMAGE),
        "video/mp4" | "video/webm" | "video/quicktime" => Some(MOVING_IMAGE),
        "audio/mpeg" | "audio/wav" | "audio/ogg" | "audio/flac" => Some(SOUND),
        "text/plain" | "text/markdown" | "text/html" | "application/pdf" => Some(TEXT),
        "application/json" | "text/csv" => Some(DATASET),
        _ => None,
    }
}

/// Map a common resource kind string to its BIBO type.
/// Used when servers already classify resources with informal labels.
pub fn kind_to_bibo(kind: &str) -> Option<DcConcept> {
    match kind.to_lowercase().as_str() {
        "article" | "paper" => Some(ARTICLE),
        "academic_article" | "journal_article" => Some(ACADEMIC_ARTICLE),
        "journal" => Some(JOURNAL),
        "book" => Some(BOOK),
        "chapter" | "book_section" => Some(BOOK_SECTION),
        "thesis" | "dissertation" => Some(THESIS),
        "webpage" | "url" | "web" => Some(WEBPAGE),
        "document" => Some(DOCUMENT),
        "preprint" | "arxiv" => Some(PREPRINT),
        "proceedings" | "conference" => Some(PROCEEDINGS),
        "report" | "technical_report" => Some(REPORT),
        "manuscript" => Some(MANUSCRIPT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_to_dc_type_coverage() {
        assert_eq!(mime_to_dc_type("image/png"), Some(STILL_IMAGE));
        assert_eq!(mime_to_dc_type("video/mp4"), Some(MOVING_IMAGE));
        assert_eq!(mime_to_dc_type("audio/wav"), Some(SOUND));
        assert_eq!(mime_to_dc_type("application/pdf"), Some(TEXT));
        assert_eq!(mime_to_dc_type("application/json"), Some(DATASET));
        assert_eq!(mime_to_dc_type("application/octet-stream"), None);
    }

    #[test]
    fn kind_to_bibo_coverage() {
        assert_eq!(kind_to_bibo("article"), Some(ARTICLE));
        assert_eq!(kind_to_bibo("book"), Some(BOOK));
        assert_eq!(kind_to_bibo("preprint"), Some(PREPRINT));
        assert_eq!(kind_to_bibo("webpage"), Some(WEBPAGE));
        assert_eq!(kind_to_bibo("unknown_thing"), None);
    }
}
