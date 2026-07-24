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

// ── Mapping helpers ───────────────────────────────────────────────────────

/// Map a kanban task status to PKO execution status.
pub fn kanban_status_to_pko_execution(status: &str) -> Option<PkoConcept> {
    match status.to_lowercase().as_str() {
        "todo" | "backlog" => Some("pko:ProcedureExecutionStatus/queued"),
        "in_progress" | "doing" => Some("pko:ProcedureExecutionStatus/inProgress"),
        "review" | "verify" => Some("pko:ProcedureExecutionStatus/verifying"),
        "done" | "complete" => Some("pko:ProcedureExecutionStatus/completed"),
        "blocked" => Some("pko:ProcedureExecutionStatus/blocked"),
        _ => None,
    }
}

/// Map a document processing stage to a PKO Step concept.
pub fn docproc_stage_to_pko_step(stage: &str) -> Option<PkoConcept> {
    match stage.to_lowercase().as_str() {
        "convert" | "extract" => Some(STEP),
        "ocr" => Some(FUNCTION),
        "chunk" | "split" => Some(FUNCTION),
        "embed" | "vectorize" => Some(FUNCTION),
        "generate_qa" | "qa" => Some(ACTION),
        "extract_triples" | "h_mems" => Some(ACTION),
        "query" | "search" => Some(ACTION),
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

/// Map a task-breakdown output field to a PKO concept.
///
/// task-breakdown emits a plan (`Procedure`) of `Step`s with `StepVerification`,
/// `IssueOccurrence` risks, and `UserQuestionOccurrence` open questions — the
/// procedural-knowledge *specification* axis. The sibling mapper
/// `kanban_status_to_pko_execution` covers the *execution* axis, so a plan produced
/// here is consumed downstream as a `ProcedureExecution` (status) by kanban, and as
/// `StepExecution` + `StepVerification` by tdd. Together they span a full PKO
/// lifecycle: specification -> execution -> verification.
pub fn task_breakdown_field_to_pko(field: &str) -> Option<PkoConcept> {
    match field.to_lowercase().as_str() {
        "plan" | "procedure" => Some(PROCEDURE),
        "spec_or_intent" | "procedure_target" | "target" => Some(PROCEDURE_TARGET),
        "task" | "step" => Some(STEP),
        "phase" | "multi_step" => Some(MULTI_STEP),
        "dependencies" | "next_step" => Some(NEXT_STEP),
        "has_step" | "steps" => Some(HAS_STEP),
        "acceptance_criteria" | "requires_action" => Some(REQUIRES_ACTION),
        "action" => Some(ACTION),
        "verification" | "step_verification" => Some(STEP_VERIFICATION),
        "tests" | "build" | "requires_function" => Some(REQUIRES_FUNCTION),
        "function" => Some(FUNCTION),
        "files_likely_touched" | "requires_tool" => Some(REQUIRES_TOOL),
        "risks" | "issue" => Some(ISSUE_OCCURRENCE),
        "error" => Some(ERROR),
        "open_questions" | "question" => Some(USER_QUESTION_OCCURRENCE),
        "checkpoints" | "human_review" | "feedback" => Some(USER_FEEDBACK_OCCURRENCE),
        "agent" | "who" => Some(AGENT),
        "role" => Some(ROLE),
        "expertise" => Some(EXPERTISE_LEVEL),
        "iteration" | "version" => Some(HAS_VERSION),
        "next_version" => Some(NEXT_VERSION),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kanban_status_mapping() {
        assert_eq!(
            kanban_status_to_pko_execution("in_progress"),
            Some("pko:ProcedureExecutionStatus/inProgress")
        );
        assert_eq!(
            kanban_status_to_pko_execution("done"),
            Some("pko:ProcedureExecutionStatus/completed")
        );
        assert_eq!(kanban_status_to_pko_execution("nonexistent"), None);
    }

    #[test]
    fn docproc_stage_mapping() {
        assert_eq!(docproc_stage_to_pko_step("chunk"), Some(FUNCTION));
        assert_eq!(docproc_stage_to_pko_step("generate_qa"), Some(ACTION));
        assert_eq!(docproc_stage_to_pko_step("unknown"), None);
    }

    #[test]
    fn research_stage_mapping() {
        assert_eq!(
            research_stage_to_pko("hypothesis"),
            Some(USER_QUESTION_OCCURRENCE)
        );
        assert_eq!(research_stage_to_pko("evaluate"), Some(STEP_VERIFICATION));
        assert_eq!(research_stage_to_pko("bogus"), None);
    }

    #[test]
    fn task_breakdown_field_mapping() {
        // Specification axis: the plan as a Procedure targeting a ProcedureTarget
        assert_eq!(task_breakdown_field_to_pko("plan"), Some(PROCEDURE));
        assert_eq!(
            task_breakdown_field_to_pko("spec_or_intent"),
            Some(PROCEDURE_TARGET)
        );
        assert_eq!(task_breakdown_field_to_pko("task"), Some(STEP));
        assert_eq!(task_breakdown_field_to_pko("phase"), Some(MULTI_STEP));
        assert_eq!(task_breakdown_field_to_pko("dependencies"), Some(NEXT_STEP));
        // Verification axis: ACs as required actions, verification as StepVerification
        assert_eq!(
            task_breakdown_field_to_pko("acceptance_criteria"),
            Some(REQUIRES_ACTION)
        );
        assert_eq!(
            task_breakdown_field_to_pko("verification"),
            Some(STEP_VERIFICATION)
        );
        assert_eq!(
            task_breakdown_field_to_pko("tests"),
            Some(REQUIRES_FUNCTION)
        );
        assert_eq!(
            task_breakdown_field_to_pko("files_likely_touched"),
            Some(REQUIRES_TOOL)
        );
        // Feedback axis: risks, questions, checkpoints
        assert_eq!(task_breakdown_field_to_pko("risks"), Some(ISSUE_OCCURRENCE));
        assert_eq!(
            task_breakdown_field_to_pko("open_questions"),
            Some(USER_QUESTION_OCCURRENCE)
        );
        assert_eq!(
            task_breakdown_field_to_pko("checkpoints"),
            Some(USER_FEEDBACK_OCCURRENCE)
        );
        // Versioning: convergence iterations
        assert_eq!(task_breakdown_field_to_pko("iteration"), Some(HAS_VERSION));
        assert_eq!(task_breakdown_field_to_pko("nonexistent"), None);
    }
}
