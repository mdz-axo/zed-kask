//! PKO (Procedural Knowledge Ontology) bridge.
//!
//! Maps hKask concepts to PKO standard concepts for knowledge production
//! processes — procedures, steps, actions, executions, issues, feedback.
//! Shared by kanban, docproc, and research servers.
//!
//! Reference: Carriero et al. (2025, arXiv:2503.20634)
//! PKO reuses: PROV-O (Activity, Agent), P-Plan (Step, Plan), DCAT (Resource), DCMI
//! Canonical namespace: <https://w3id.org/pko>
//!
//! Pattern: thin mapping layer — canonical URI constants, field mapping
//! functions, no dependencies, no reasoners, no overhead ≤150 lines.

/// A PKO concept URI.
pub type PkoConcept = &'static str;

// ── Procedure specification ───────────────────────────────────────────────

/// A sequence of actions to be executed to achieve an outcome.
/// Subclass of both pplan:Plan and dcat:Resource.
pub const PROCEDURE: PkoConcept = "pko:Procedure";
pub const PROCEDURE_TYPE: PkoConcept = "pko:ProcedureType";
pub const PROCEDURE_STATUS: PkoConcept = "pko:ProcedureStatus";
pub const PROCEDURE_TARGET: PkoConcept = "pko:ProcedureTarget";

/// Links a Procedure to its Steps (pplan:Step instances).
pub const HAS_STEP: PkoConcept = "pko:hasStep";
/// Sequential ordering between Steps.
pub const NEXT_STEP: PkoConcept = "pko:nextStep";

// ── Step structure ────────────────────────────────────────────────────────

/// A Step groups one or more Actions/Functions to execute a portion of a Procedure.
/// PKO reuses pplan:Step; MultiStep is a PKO extension.
pub const STEP: PkoConcept = "pko:Step";
pub const MULTI_STEP: PkoConcept = "pko:MultiStep";

/// Human action required by a Step.
pub const REQUIRES_ACTION: PkoConcept = "pko:requiresAction";
pub const ACTION: PkoConcept = "pko:Action";

/// Algorithmic function required by a Step.
pub const REQUIRES_FUNCTION: PkoConcept = "pko:requiresFunction";
pub const FUNCTION: PkoConcept = "pko:Function";

/// Tool required by a Step.
pub const REQUIRES_TOOL: PkoConcept = "pko:requiresTool";

// ── Execution ─────────────────────────────────────────────────────────────

/// Execution of a Procedure. Subclass of prov:Activity.
pub const PROCEDURE_EXECUTION: PkoConcept = "pko:ProcedureExecution";
/// Execution of a single Step. Subclass of prov:Activity.
pub const STEP_EXECUTION: PkoConcept = "pko:StepExecution";
pub const PROCEDURE_EXECUTION_STATUS: PkoConcept = "pko:ProcedureExecutionStatus";

// ── Issues, feedback, questions ───────────────────────────────────────────

/// An error encountered by an Agent during execution.
pub const ISSUE_OCCURRENCE: PkoConcept = "pko:IssueOccurrence";
/// Feedback left by an Agent on a procedure or execution.
pub const USER_FEEDBACK_OCCURRENCE: PkoConcept = "pko:UserFeedbackOccurrence";
/// A question asked by an Agent while performing a procedure.
pub const USER_QUESTION_OCCURRENCE: PkoConcept = "pko:UserQuestionOccurrence";

/// The Error that caused an IssueOccurrence.
pub const ERROR: PkoConcept = "pko:Error";
pub const ERROR_CODE: PkoConcept = "pko:errorCode";

// ── Verification ──────────────────────────────────────────────────────────

/// How a Step's execution can be verified.
pub const STEP_VERIFICATION: PkoConcept = "pko:StepVerification";

// ── Agents and roles ──────────────────────────────────────────────────────

/// An Agent involved in procedure creation or execution.
pub const AGENT: PkoConcept = "pko:Agent";
/// A Role an Agent plays (e.g., editor, supervisor, user).
pub const ROLE: PkoConcept = "pko:Role";
/// A role restricted to a PeriodOfTime.
pub const ROLE_IN_TIME: PkoConcept = "pko:RoleInTime";
/// Expertise level required for a Step.
pub const EXPERTISE_LEVEL: PkoConcept = "pko:ExpertiseLevel";

// ── Resources ─────────────────────────────────────────────────────────────

/// A Resource referenced by a Procedure (document, image, video).
pub const REFERENCES_RESOURCE: PkoConcept = "pko:references";
/// A Procedure was extracted from a Resource (e.g., PDF describing steps).
pub const WAS_EXTRACTED_FROM: PkoConcept = "pko:wasExtractedFrom";

// ── Versioning ────────────────────────────────────────────────────────────

pub const HAS_VERSION: PkoConcept = "pko:hasVersion";
pub const NEXT_VERSION: PkoConcept = "pko:nextVersion";
pub const PREVIOUS_VERSION: PkoConcept = "pko:previousVersion";

// ── Execution lifecycle ───────────────────────────────────────────────────
// Consumed by the kata-kanban server's type mapping (the execution axis of the
// task lifecycle: status transitions and step-execution provenance).

/// A transition between two statuses.
pub const CHANGE_OF_STATUS: PkoConcept = "pko:ChangeOfStatus";
/// Expected duration of a step/procedure.
pub const HAS_EXPECTED_DURATION: PkoConcept = "pko:hasExpectedDuration";

// ── PROV-O reuse ─────────────────────────────────────────────────────────
// PKO extends P-Plan and PROV-O via soft reuse; these are the PROV-O
// provenance properties the execution axis needs.

/// Agent associated with an activity — PROV-O.
pub const WAS_ASSOCIATED_WITH: PkoConcept = "prov:wasAssociatedWith";
/// Entity generated by an activity — PROV-O.
pub const WAS_GENERATED_BY: PkoConcept = "prov:wasGeneratedBy";
/// Entity used by an activity — PROV-O.
pub const USED: PkoConcept = "prov:used";

// ── Mapping helpers ───────────────────────────────────────────────────────

/// Map a kanban task status to PKO execution status.
///
/// Covers the standard kanban `TaskStatus` wire strings (backlog, ready,
/// in_progress, review, done) plus their common aliases. `ready` maps to
/// queued — a ready task is pulled but not yet started.
pub fn kanban_status_to_pko_execution(status: &str) -> Option<PkoConcept> {
    match status.to_lowercase().as_str() {
        "todo" | "backlog" | "ready" => Some("pko:ProcedureExecutionStatus/queued"),
        "in_progress" | "doing" => Some("pko:ProcedureExecutionStatus/inProgress"),
        "review" | "verify" => Some("pko:ProcedureExecutionStatus/verifying"),
        "done" | "complete" => Some("pko:ProcedureExecutionStatus/completed"),
        "blocked" => Some("pko:ProcedureExecutionStatus/blocked"),
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

    #[test]
    fn kanban_status_maps_all_standard_wire_statuses() {
        // The kanban TaskStatus wire strings (hkask-types, rename_all =
        // lowercase) must all map — an unmapped standard status would leave
        // the execution-axis annotation silently absent.
        assert_eq!(
            kanban_status_to_pko_execution("backlog"),
            Some("pko:ProcedureExecutionStatus/queued")
        );
        assert_eq!(
            kanban_status_to_pko_execution("ready"),
            Some("pko:ProcedureExecutionStatus/queued")
        );
        assert_eq!(
            kanban_status_to_pko_execution("in_progress"),
            Some("pko:ProcedureExecutionStatus/inProgress")
        );
        assert_eq!(
            kanban_status_to_pko_execution("review"),
            Some("pko:ProcedureExecutionStatus/verifying")
        );
        assert_eq!(
            kanban_status_to_pko_execution("done"),
            Some("pko:ProcedureExecutionStatus/completed")
        );
    }

    #[test]
    fn kanban_status_mapping_is_case_insensitive_and_rejects_unknown() {
        assert_eq!(
            kanban_status_to_pko_execution("Done"),
            Some("pko:ProcedureExecutionStatus/completed")
        );
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
