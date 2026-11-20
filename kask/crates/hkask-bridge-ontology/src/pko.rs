//! PKO (Procedural Knowledge Ontology) bridge.
//!
//! Maps hKask concepts to PKO standard concepts for knowledge production
//! processes — procedures, steps, actions, executions, issues, feedback.
//! Shared by kanban, docproc, and research servers.
//!
//! The full PKO v2.0.0 ontology (Carriero et al., arXiv:2503.20634,
//! <https://w3id.org/pko>) and the vocabularies it specializes (P-Plan,
//! PROV-O, Dublin Core) are loaded from `sources/` and resolved through
//! `published`. This module names only the concepts hKask code emits;
//! `all_terms_are_official` fails the build unless each is published by the
//! vocabulary owning its namespace.
//!
//! Reused terms (`pplan:`, `prov:`, `dcterms:`) keep their canonical
//! namespace prefixes — never re-prefixed under `pko:`. Verification (2026-08-29)
//! corrected five such mis-prefixed terms and dropped five dead ones
//! (`ProcedureTarget` does not exist in PKO; `Role`/`RoleInTime` are SPAR
//! terms with no consumers here; versioning is DCAT's, not PKO's).
//!
//! Pattern: thin mapping layer — canonical URI constants, field mapping
//! functions, no dependencies, no reasoners, no overhead.

/// A PKO concept URI.
pub type PkoConcept = &'static str;

/// Defines the vocabulary constants and registers every one in `ALL_TERMS`,
/// so the publication guard covers each constant by construction.
macro_rules! pko_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: PkoConcept = $uri;)*

        /// Every term in this module, so the publication guard covers each
        /// constant by construction. New terms must go through this macro.
        #[cfg(test)]
        const ALL_TERMS: &[PkoConcept] = &[$($name),*];
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every term in this module is published by the
    /// loaded vocabulary owning its namespace. A plausible-looking invented
    /// or mis-prefixed URI fails here.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_TERMS {
            assert!(
                crate::published::contains(term),
                "{term} is not published by the vocabulary owning its namespace"
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
