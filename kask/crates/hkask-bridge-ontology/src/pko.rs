//! PKO (Procedural Knowledge Ontology) bridge.
//!
//! Maps hKask concepts to PKO standard concepts for knowledge production
//! processes — procedures, steps, actions, executions, issues, feedback.
//! Shared by kanban, docproc, and research servers.
//!
//! Every URI in this module is verified against the official PKO v2.0.0
//! OWL artifact (Carriero et al., arXiv:2503.20634,
//! <https://w3id.org/pko>, version 2.0.0, 2026-06-29) —
//! `fixtures/pko-2.0.0-terms.txt` pins the term list, and
//! `all_terms_are_official` fails the build if a term drifts from it. Do
//! not add a term that is not in that fixture.
//!
//! PKO reuses P-Plan, PROV-O, SPAR, Dublin Core, and DCAT terms; the
//! reused terms this module carries (`pplan:`, `prov:`, `dcterms:`) are
//! defined in the PKO artifact itself and keep their canonical namespace
//! prefixes — never re-prefixed under `pko:`. Verification (2026-08-29)
//! corrected five such mis-prefixed terms and dropped five dead ones
//! (`ProcedureTarget` does not exist in PKO; `Role`/`RoleInTime` are SPAR
//! terms with no consumers here; versioning is DCAT's, not PKO's).
//!
//! Pattern: thin mapping layer — canonical URI constants, field mapping
//! functions, no dependencies, no reasoners, no overhead.

/// A PKO concept URI.
pub type PkoConcept = &'static str;

/// Defines the vocabulary constants and registers every one in `ALL_TERMS`,
/// so the fixture test covers each constant by construction.
macro_rules! pko_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: PkoConcept = $uri;)*

        /// Every term in this module. The fixture test asserts each appears
        /// in the official PKO term list — a fabricated URI cannot pass.
        /// New terms must go through this macro.
        pub const ALL_TERMS: &[PkoConcept] = &[$($name),*];
    };
}

pko_terms! {
    // ── Procedure specification ───────────────────────────────────────────

    /// A sequence of actions to be executed to achieve an outcome.
    /// Subclass of both pplan:Plan and dcat:Resource.
    PROCEDURE = "pko:Procedure",
    /// The type of a Procedure (e.g. standard operating procedure).
    PROCEDURE_TYPE = "pko:ProcedureType",
    /// The status of a Procedure (Draft, Approval, Approved, ...).
    PROCEDURE_STATUS = "pko:ProcedureStatus",
    /// Links a Procedure to its Steps.
    HAS_STEP = "pko:hasStep",
    /// Sequential ordering between Steps.
    NEXT_STEP = "pko:nextStep",

    // ── Step structure ───────────────────────────────────────────────────
    // PKO reuses P-Plan's Step and MultiStep — they keep the pplan: prefix.

    /// A Step groups one or more Actions/Functions to execute a portion of
    /// a Procedure (P-Plan, reused by PKO).
    STEP = "pplan:Step",
    /// A Step composed of other Steps (P-Plan, reused by PKO).
    MULTI_STEP = "pplan:MultiStep",

    /// Human action required by a Step.
    REQUIRES_ACTION = "pko:requiresAction",
    /// A human action.
    ACTION = "pko:Action",
    /// Algorithmic function required by a Step.
    REQUIRES_FUNCTION = "pko:requiresFunction",
    /// An algorithmic function.
    FUNCTION = "pko:Function",
    /// Tool required by a Step.
    REQUIRES_TOOL = "pko:requiresTool",

    // ── Execution ────────────────────────────────────────────────────────

    /// Execution of a Procedure. Subclass of prov:Activity.
    PROCEDURE_EXECUTION = "pko:ProcedureExecution",
    /// Execution of a single Step. Subclass of prov:Activity.
    STEP_EXECUTION = "pko:StepExecution",
    /// The status of a Procedure Execution. Published individuals:
    /// InProgress, Completed, Paused, Cancelled.
    PROCEDURE_EXECUTION_STATUS = "pko:ProcedureExecutionStatus",

    // ── Issues, feedback, questions ───────────────────────────────────────

    /// An error encountered by an Agent during execution.
    ISSUE_OCCURRENCE = "pko:IssueOccurrence",
    /// Feedback left by an Agent on a procedure or execution.
    USER_FEEDBACK_OCCURRENCE = "pko:UserFeedbackOccurrence",
    /// A question asked by an Agent while performing a procedure.
    USER_QUESTION_OCCURRENCE = "pko:UserQuestionOccurrence",
    /// The Error that caused an IssueOccurrence.
    ERROR = "pko:Error",
    /// The code of an Error.
    ERROR_CODE = "pko:errorCode",

    // ── Verification ──────────────────────────────────────────────────────

    /// How a Step's execution can be verified.
    STEP_VERIFICATION = "pko:StepVerification",

    // ── Agents and expertise ──────────────────────────────────────────────

    /// An Agent involved in procedure creation or execution (PROV-O,
    /// reused by PKO).
    AGENT = "prov:Agent",
    /// Expertise level required for a Step. Published individuals:
    /// Junior, Senior, Master, Expert.
    EXPERTISE_LEVEL = "pko:ExpertiseLevel",

    // ── Resources ─────────────────────────────────────────────────────────

    /// A Resource referenced by a Procedure (document, image, video) —
    /// Dublin Core, reused by PKO.
    REFERENCES_RESOURCE = "dcterms:references",
    /// A Procedure was extracted from a Resource (e.g., PDF describing steps).
    WAS_EXTRACTED_FROM = "pko:wasExtractedFrom",

    // ── Versioning ────────────────────────────────────────────────────────

    /// The next version of a Procedure.
    NEXT_VERSION = "pko:nextVersion",

    // ── Execution lifecycle ───────────────────────────────────────────────
    // Consumed by the kata-kanban server's type mapping (the execution axis
    // of the task lifecycle: status transitions and step-execution
    // provenance).

    /// A transition between two statuses.
    CHANGE_OF_STATUS = "pko:ChangeOfStatus",
    /// Expected duration of a step/procedure.
    HAS_EXPECTED_DURATION = "pko:hasExpectedDuration",

    // ── PROV-O reuse ─────────────────────────────────────────────────────
    // PKO extends P-Plan and PROV-O via soft reuse; these are the PROV-O
    // provenance properties the execution axis needs.

    /// Agent associated with an activity — PROV-O.
    WAS_ASSOCIATED_WITH = "prov:wasAssociatedWith",
    /// Entity generated by an activity — PROV-O.
    WAS_GENERATED_BY = "prov:wasGeneratedBy",
    /// Entity used by an activity — PROV-O.
    USED = "prov:used",

    // ── Published status individuals ─────────────────────────────────────
    // PKO v2.0.0 publishes exactly four ProcedureExecutionStatus individuals.

    /// Execution is in progress (PKO published individual).
    STATUS_IN_PROGRESS = "pko:InProgress",
    /// Execution completed (PKO published individual).
    STATUS_COMPLETED = "pko:Completed",
    /// Execution halted, resumable (PKO published individual). The honest
    /// cover for a blocked task: the execution is paused pending the
    /// impediment's resolution.
    STATUS_PAUSED = "pko:Paused",
}

// ── Mapping helpers ───────────────────────────────────────────────

/// Map a kanban task status to its PKO execution-status individual.
///
/// Covers the standard kanban `TaskStatus` wire strings. Only statuses
/// PKO actually publishes individuals for are mapped: `in_progress` →
/// InProgress, `done` → Completed, `blocked` → Paused (a blocked
/// execution is a paused one). PKO v2.0.0 publishes no queued,
/// not-started, or reviewing execution status — `todo`/`backlog`/`ready`
/// and `review` return `None` rather than forcing a nonexistent
/// individual (the response field is optional, so `None` degrades
/// gracefully).
pub fn kanban_status_to_pko_execution(status: &str) -> Option<PkoConcept> {
    match status.to_lowercase().as_str() {
        "in_progress" | "doing" => Some(STATUS_IN_PROGRESS),
        "done" | "complete" => Some(STATUS_COMPLETED),
        "blocked" => Some(STATUS_PAUSED),
        _ => None,
    }
}

/// Map a corpus pipeline operation to its PKO process concept.
///
/// Takes the bare operation name — the corpus tool name minus its `corpus_`
/// prefix (`corpus_convert` → `convert`). This is the canonical source of
/// truth for the corpus server's `ontology_anchor`: the reg.tool span tags a
/// tool *execution*, so pipeline operations anchor on the process axis (PKO),
/// not the state axis of the artifact they produce. Storage/registry
/// operations (cache, clear_index, purge) are deliberately unmapped here —
/// they anchor on the state axis (Dublin Core Dataset) via the anchor's
/// default arm.
pub fn corpus_stage_to_pko_step(stage: &str) -> Option<PkoConcept> {
    match stage.to_lowercase().as_str() {
        // Ingest: the entry step of the pipeline.
        "convert" | "extract" => Some(STEP),
        // Text-processing functions.
        "ocr" | "chunk" | "split" | "embed" | "vectorize" | "dedup" | "dedup_chunks"
        | "consolidate" | "consolidate_chunks" => Some(FUNCTION),
        // Triage before parse — verifies whether OCR is needed.
        "is_complex" => Some(STEP_VERIFICATION),
        // Knowledge-extraction and retrieval actions.
        "tag"
        | "tag_chunks"
        | "build_prompts"
        | "generate_qa"
        | "generate_qa_batch"
        | "qa"
        | "ingest_qa"
        | "extract_assertions"
        | "h_mems"
        | "query"
        | "search"
        | "discover"
        | "discover_company"
        | "prepare_training_dataset" => Some(ACTION),
        _ => None,
    }
}

/// Map a research workflow stage to a PKO concept.
pub fn research_stage_to_pko(stage: &str) -> Option<PkoConcept> {
    match stage.to_lowercase().as_str() {
        "hypothesis" | "question" => Some(USER_QUESTION_OCCURRENCE),
        "search" | "discover" => Some(ACTION),
        "extract" | "read" => Some(ACTION),
        "evaluate" | "assess" => Some(STEP_VERIFICATION),
        "synthesize" | "summarize" => Some(PROCEDURE_EXECUTION),
        "curate" | "organize" => Some(PROCEDURE),
        "cite" | "reference" => Some(REFERENCES_RESOURCE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every term in this module must appear in the
    /// official PKO term list checked in as a fixture (source URL and
    /// fetch date in the fixture header). A term that is not in the
    /// published ontology fails here — pin tests on the constants alone
    /// cannot catch a plausible-looking invented URI.
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/pko-2.0.0-terms.txt");
        let fixture = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {fixture_path}: {e}"));
        let official: std::collections::HashSet<&str> = fixture
            .lines()
            .map(|line| line.split('\t').next().unwrap_or("").trim())
            .filter(|term| !term.is_empty() && !term.starts_with('#'))
            .collect();
        assert!(
            !official.is_empty(),
            "fixture {fixture_path} contains no terms"
        );
        for term in ALL_TERMS {
            assert!(
                official.contains(term),
                "{term} is not in the official PKO v2.0.0 term list ({fixture_path}) — \
                 it must be verified against https://w3id.org/pko before use"
            );
        }
    }

    #[test]
    fn kanban_status_maps_only_published_individuals() {
        // PKO v2.0.0 publishes exactly four ProcedureExecutionStatus
        // individuals (InProgress, Completed, Paused, Cancelled). Only
        // wire statuses with a real individual map; the rest return None
        // rather than forcing a nonexistent status.
        assert_eq!(
            kanban_status_to_pko_execution("in_progress"),
            Some("pko:InProgress")
        );
        assert_eq!(
            kanban_status_to_pko_execution("done"),
            Some("pko:Completed")
        );
        assert_eq!(
            kanban_status_to_pko_execution("blocked"),
            Some("pko:Paused")
        );
        // No queued / not-started / reviewing individual exists in PKO.
        assert_eq!(kanban_status_to_pko_execution("todo"), None);
        assert_eq!(kanban_status_to_pko_execution("backlog"), None);
        assert_eq!(kanban_status_to_pko_execution("ready"), None);
        assert_eq!(kanban_status_to_pko_execution("review"), None);
        assert_eq!(kanban_status_to_pko_execution("archived"), None);
    }
}

#[test]
fn corpus_stage_mapper_covers_every_pipeline_operation() {
    // Every corpus pipeline tool's bare operation name (tool minus the
    // corpus_ prefix) must map — the corpus server's ontology_anchor
    // delegates here, so an unmapped pipeline op would silently fall to
    // the Dataset default (wrong axis for a process).
    for op in [
        "convert",
        "ocr",
        "is_complex",
        "chunk",
        "embed",
        "tag_chunks",
        "dedup_chunks",
        "consolidate_chunks",
        "build_prompts",
        "generate_qa",
        "generate_qa_batch",
        "ingest_qa",
        "extract_assertions",
        "query",
        "discover",
        "discover_company",
        "prepare_training_dataset",
    ] {
        assert!(
            corpus_stage_to_pko_step(op).is_some(),
            "pipeline operation {op} must map to a PKO process concept"
        );
    }
    // Storage/registry operations are deliberately unmapped (state axis).
    for op in ["cache", "cache_work", "clear_index", "purge_qa"] {
        assert!(
            corpus_stage_to_pko_step(op).is_none(),
            "storage operation {op} must NOT map to the process axis"
        );
    }
}
