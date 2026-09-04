//! Workflow resolution and seam validation — the scaffold for the local
//! "run the declared workflow" pattern. Some agents' cards declare a
//! `workflow_template` (the stages they run and the member agents that fill
//! each slot); this module is everything a runner needs BEFORE executing
//! one, built as pure functions so the future `swarm_run_workflow_local`
//! reuses them verbatim.
//!
//! The pieces:
//! - **Slot resolution** — each stage names the agent that fills it (or is an
//!   open slot); resolution reports whether that agent exists in the local
//!   registry and whether its ACTUAL `accepts`/`produces` match the stage's
//!   DECLARED ports (a declared/actual mismatch is a note — the agent may
//!   have drifted since the workflow was authored).
//! - **Seam validation** — the `pipeline_strategist` pattern: each stage's
//!   declared `produces` must overlap the next stage's declared `accepts`.
//!   An empty port list on either side is permissive (matches anything —
//!   the same permissive-empty rule as `a2a::derive_modes`). Violations are
//!   reported, never blocking — advisory, like the contract checks.
//!
//! Surfaced today via `swarm_workflow_check_local`, so a cloned agent's
//! declared workflow is inspectable and validatable before a runner exists.

use crate::local_registry::{LocalWorkflowStage, LocalWorkflowTemplate};

/// The resolution report for one stage.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WorkflowStageReport {
    /// The stage's declared name.
    pub stage: String,
    /// The declared filling agent id, or `None` for an open slot.
    pub declared_agent: Option<String>,
    /// `Some(true)` — the agent exists in the registry; `Some(false)` — it
    /// does not; `None` — open slot (nothing to resolve).
    pub agent_found: Option<bool>,
    /// The stage's declared input ports.
    pub declared_accepts: Vec<String>,
    /// The stage's declared output ports.
    pub declared_produces: Vec<String>,
    /// The resolved agent's ACTUAL ports, when the agent was found.
    pub actual_accepts: Option<Vec<String>>,
    pub actual_produces: Option<Vec<String>>,
    /// Per-stage notes: declared/actual port mismatches, open slots.
    pub notes: Vec<String>,
}

/// The full workflow check report.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WorkflowReport {
    /// The agent whose declared workflow was checked.
    pub agent_id: String,
    /// Per-stage resolution, in pipeline order.
    pub stages: Vec<WorkflowStageReport>,
    /// Seam violations between consecutive stages (empty = all seams hold).
    pub seam_violations: Vec<String>,
    /// `true` when every slot resolves and every seam holds. Advisory —
    /// a `false` report describes what a runner would trip over, it does
    /// not block anything.
    pub valid: bool,
}

/// Resolve and validate one workflow. `lookup` maps an agent id to its
/// `(accepts, produces)` when it exists in the registry — passed as a
/// closure so this stays a pure function (no registry dependency, fully
/// unit-testable).
pub fn check_workflow(
    agent_id: &str,
    template: &LocalWorkflowTemplate,
    lookup: impl Fn(&str) -> Option<(Vec<String>, Vec<String>)>,
) -> WorkflowReport {
    let mut stages = Vec::with_capacity(template.stages.len());
    let mut unresolved_slots = false;

    for stage in &template.stages {
        stages.push(check_stage(stage, &lookup));
        if stages
            .last()
            .is_some_and(|report| report.agent_found == Some(false))
        {
            unresolved_slots = true;
        }
    }

    let seam_violations = check_seams(&template.stages);
    let valid = !unresolved_slots && seam_violations.is_empty();
    WorkflowReport {
        agent_id: agent_id.to_string(),
        stages,
        seam_violations,
        valid,
    }
}

/// Resolve one stage against the registry lookup.
fn check_stage(
    stage: &LocalWorkflowStage,
    lookup: &impl Fn(&str) -> Option<(Vec<String>, Vec<String>)>,
) -> WorkflowStageReport {
    let mut notes = Vec::new();
    let (declared_agent, agent_found, actual_accepts, actual_produces) = match &stage.agent {
        None => {
            notes.push(
                "open slot — no agent declared; a runner will ask the operator to fill it"
                    .to_string(),
            );
            (None, None, None, None)
        }
        Some(agent_id) => match lookup(agent_id) {
            None => (Some(agent_id.clone()), Some(false), None, None),
            Some((accepts, produces)) => {
                // Declared/actual drift is a note, not a violation — the
                // stage declaration is the workflow's contract; the agent
                // card may have changed since it was authored.
                for port in &stage.accepts {
                    if !accepts.contains(port) {
                        notes.push(format!(
                            "declared accepts port '{port}' is not on agent '{agent_id}''s card"
                        ));
                    }
                }
                for port in &stage.produces {
                    if !produces.contains(port) {
                        notes.push(format!(
                            "declared produces port '{port}' is not on agent '{agent_id}''s card"
                        ));
                    }
                }
                (
                    Some(agent_id.clone()),
                    Some(true),
                    Some(accepts),
                    Some(produces),
                )
            }
        },
    };
    WorkflowStageReport {
        stage: stage.name.clone(),
        declared_agent,
        agent_found,
        declared_accepts: stage.accepts.clone(),
        declared_produces: stage.produces.clone(),
        actual_accepts,
        actual_produces,
        notes,
    }
}

/// Seam validation — the `pipeline_strategist` pattern: each stage's declared
/// `produces` must overlap the next stage's declared `accepts`. An empty
/// port list on either side is permissive (matches anything).
fn check_seams(stages: &[LocalWorkflowStage]) -> Vec<String> {
    let mut violations = Vec::new();
    for pair in stages.windows(2) {
        let (upstream, downstream) = (&pair[0], &pair[1]);
        // Permissive-empty: an undeclared port side matches anything (the
        // same rule as `a2a::derive_modes` — absence is not contradiction).
        if upstream.produces.is_empty() || downstream.accepts.is_empty() {
            continue;
        }
        let overlaps = upstream
            .produces
            .iter()
            .any(|port| downstream.accepts.contains(port));
        if !overlaps {
            violations.push(format!(
                "seam '{}': produces {} but '{}' accepts {} — no overlap",
                upstream.name,
                upstream.produces.join(", "),
                downstream.name,
                downstream.accepts.join(", ")
            ));
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(
        name: &str,
        agent: Option<&str>,
        accepts: &[&str],
        produces: &[&str],
    ) -> LocalWorkflowStage {
        LocalWorkflowStage {
            name: name.to_string(),
            agent: agent.map(str::to_string),
            accepts: accepts.iter().map(|s| s.to_string()).collect(),
            produces: produces.iter().map(|s| s.to_string()).collect(),
            description: None,
        }
    }

    fn template(stages: Vec<LocalWorkflowStage>) -> LocalWorkflowTemplate {
        LocalWorkflowTemplate {
            mermaid: String::new(),
            stages,
            description: None,
        }
    }

    /// A registry with two agents: research produces "analysis", writer
    /// accepts "analysis" and produces "draft".
    fn lookup(agent: &str) -> Option<(Vec<String>, Vec<String>)> {
        match agent {
            "research" => Some((vec!["text".to_string()], vec!["analysis".to_string()])),
            "writer" => Some((vec!["analysis".to_string()], vec!["draft".to_string()])),
            _ => None,
        }
    }

    #[test]
    fn matching_seams_and_resolved_slots_are_valid() {
        let workflow = template(vec![
            stage("gather", Some("research"), &["text"], &["analysis"]),
            stage("write", Some("writer"), &["analysis"], &["draft"]),
        ]);
        let report = check_workflow("orchestrator", &workflow, lookup);
        assert!(report.valid, "seams hold, slots resolve");
        assert!(report.seam_violations.is_empty());
        assert_eq!(report.stages.len(), 2);
        assert_eq!(report.stages[0].agent_found, Some(true));
        assert_eq!(
            report.stages[0].actual_produces.as_deref(),
            Some(&["analysis".to_string()][..])
        );
    }

    #[test]
    fn disjoint_ports_report_a_seam_violation() {
        let workflow = template(vec![
            stage("gather", Some("research"), &["text"], &["analysis"]),
            // writer accepts "analysis" — but this stage declares it accepts
            // something else, so the DECLARED seam is disjoint even though
            // the agents would actually compose.
            stage("write", Some("writer"), &["unrelated"], &["draft"]),
        ]);
        let report = check_workflow("orchestrator", &workflow, lookup);
        assert!(!report.valid);
        assert_eq!(report.seam_violations.len(), 1);
        assert!(report.seam_violations[0].contains("no overlap"));
    }

    #[test]
    fn empty_ports_are_permissive_not_violations() {
        // Absence is not contradiction — a stage with no declared ports
        // matches anything (the derive_modes permissive-empty rule).
        let workflow = template(vec![
            stage("gather", Some("research"), &[], &[]),
            stage("write", Some("writer"), &["analysis"], &["draft"]),
        ]);
        let report = check_workflow("orchestrator", &workflow, lookup);
        assert!(report.seam_violations.is_empty());
    }

    #[test]
    fn missing_agent_is_reported_not_panicked() {
        let workflow = template(vec![stage(
            "gather",
            Some("ghost"),
            &["text"],
            &["analysis"],
        )]);
        let report = check_workflow("orchestrator", &workflow, lookup);
        assert!(!report.valid);
        assert_eq!(report.stages[0].agent_found, Some(false));
        assert!(
            report.seam_violations.is_empty(),
            "one stage — no seams to check"
        );
    }

    #[test]
    fn open_slot_is_a_note_not_a_failure_to_resolve() {
        let workflow = template(vec![
            stage("gather", None, &["text"], &["analysis"]),
            stage("write", Some("writer"), &["analysis"], &["draft"]),
        ]);
        let report = check_workflow("orchestrator", &workflow, lookup);
        // The open slot resolves to nothing (agent_found: None) — the seam
        // still holds on the DECLARED ports, so the workflow is valid; the
        // runner will ask the operator to fill the slot.
        assert!(report.valid);
        assert!(report.stages[0].notes[0].contains("open slot"));
    }

    #[test]
    fn declared_actual_port_drift_is_a_note() {
        // The stage declares a port the agent's card does not carry — a
        // drift note, not a seam violation (the declaration is the
        // workflow's contract; the card may have changed since).
        let workflow = template(vec![stage(
            "gather",
            Some("research"),
            &["text"],
            &["analysis", "phantom"],
        )]);
        let report = check_workflow("orchestrator", &workflow, lookup);
        assert!(report.valid, "no seam violations, slot resolves");
        assert!(
            report.stages[0]
                .notes
                .iter()
                .any(|note| note.contains("phantom"))
        );
    }
}
