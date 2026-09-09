//! Workflow resolution, seam validation, and execution — the local
//! "run the declared workflow" pattern. Some agents' cards declare a
//! `workflow_template` (the stages they run and the member agents that fill
//! each slot); this module checks and executes those templates as pure
//! functions over injected closures, so `swarm_workflow_check_local` and
//! `swarm_run_workflow_local` share one implementation and both are
//! unit-testable without the runtime.
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
//! - **Execution** (`run_workflow`) — sequential stages, the artifact flows
//!   verbatim: stage 1 receives the caller's task, each later stage receives
//!   its predecessor's response (fermi's `coordination_graph` feeding rule —
//!   no per-stage prompt format is invented here; per-stage framing is
//!   `swarm_pipeline_local`'s `{prev_output}` templates). An open slot or a
//!   missing agent stops the run with a named outcome — the check report
//!   promises "a runner will ask the operator to fill it", and this is the
//!   runner keeping that promise. Each downstream dispatch that carries
//!   upstream output is reported through the `record_edge` closure so the
//!   observed topology accumulates at the point of truth (fermi's
//!   `OBSERVED_SEAMS_SQL` lesson: declared ports predict none of the real
//!   hand-offs — the observed edge is the fact, the declared port is the
//!   aspiration).
//!
//! Surfaced via `swarm_workflow_check_local` (inspect/validate) and
//! `swarm_run_workflow_local` (execute).

use crate::local_registry::{LocalWorkflowStage, LocalWorkflowTemplate};

use std::future::Future;

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

/// Compare one observed delegation edge against the two cards' declared
/// ports — the local analog of fermi's `port_trust::seam_agreement`.
/// Returns the agreement class and a note naming the ports.
///
/// The permissive-empty rule matches [`check_seams`]: a side that declares no
/// ports matches anything (absence is not contradiction). fermi measured
/// that declared ports predict NONE of the real hand-offs (3 of 3
/// production compositions had zero label overlap), so `undeclared` is the
/// norm, not a fault — the caller reports it as a note, never a block.
pub fn observed_seam_agreement(
    from: &str,
    to: &str,
    lookup: impl Fn(&str) -> Option<(Vec<String>, Vec<String>)>,
) -> (&'static str, String) {
    let (Some((_, from_produces)), Some((to_accepts, _))) = (lookup(from), lookup(to)) else {
        return (
            "unknown_agent",
            "a card has been removed since the edge was observed".to_string(),
        );
    };
    if from_produces.is_empty() || to_accepts.is_empty() {
        return (
            "declared",
            "permissive — one side declares no ports".to_string(),
        );
    }
    if from_produces.iter().any(|port| to_accepts.contains(port)) {
        return ("declared", "declared ports overlap".to_string());
    }
    (
        "undeclared",
        format!(
            "produces {} but '{}' accepts {} — no overlap",
            from_produces.join(", "),
            to,
            to_accepts.join(", ")
        ),
    )
}

/// The outcome of one executed workflow stage.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WorkflowStageOutcome {
    /// The stage's declared name.
    pub stage: String,
    /// The agent that filled the slot, when one was declared.
    pub agent: Option<String>,
    /// `true` when the stage produced a response.
    pub ok: bool,
    /// The stage's response — the artifact that flows to the next stage.
    pub response: Option<String>,
    /// The failure when the stage ran and failed, or the reason it could not
    /// run (missing agent).
    pub error: Option<String>,
    /// Per-stage notes (open slot, declared/actual drift).
    pub notes: Vec<String>,
}

/// Run a declared workflow sequentially. Stage 1 receives `task`; each later
/// stage receives its predecessor's response verbatim (fermi's
/// `coordination_graph` feeding rule). The run stops at the first stage that
/// cannot run (open slot, missing agent) or fails (delegate error); prior
/// outcomes are kept.
///
/// `lookup` resolves an agent id to its `(accepts, produces)` — the same
/// closure shape as [`check_workflow`]. `delegate` runs one agent
/// `(agent_id, task) -> Result<response, error>`. `record_edge` is called
/// once per downstream dispatch that carries upstream output — the observed
/// topology accumulates at the point of truth, and the caller decides where
/// it lands (the event store in production, a Vec in tests).
///
/// The edge is recorded at DISPATCH, not at success: a downstream agent that
/// ran and failed still received the upstream artifact, so the flow is real
/// (fermi's `parent_episode_id` semantics — execution caused by execution).
/// A stage that never dispatched (open slot, missing agent) records nothing.
pub async fn run_workflow<D, F, R>(
    template: &LocalWorkflowTemplate,
    task: &str,
    lookup: impl Fn(&str) -> Option<(Vec<String>, Vec<String>)>,
    delegate: D,
    mut record_edge: R,
) -> Vec<WorkflowStageOutcome>
where
    D: Fn(&str, &str) -> F,
    F: Future<Output = Result<String, String>>,
    R: FnMut(&str, &str),
{
    let mut outcomes = Vec::with_capacity(template.stages.len());
    let mut input = task.to_string();
    let mut upstream_agent: Option<String> = None;

    for stage in &template.stages {
        let declared_agent = match &stage.agent {
            None => {
                outcomes.push(WorkflowStageOutcome {
                    stage: stage.name.clone(),
                    agent: None,
                    ok: false,
                    response: None,
                    error: None,
                    notes: vec![
                        "open slot — no agent declared; fill the slot (swarm_update_local_agent or a new card) and re-run"
                            .to_string(),
                    ],
                });
                break;
            }
            Some(agent_id) => agent_id,
        };
        if lookup(declared_agent).is_none() {
            outcomes.push(WorkflowStageOutcome {
                stage: stage.name.clone(),
                agent: Some(declared_agent.clone()),
                ok: false,
                response: None,
                error: Some(format!(
                    "agent '{declared_agent}' not found in local registry"
                )),
                notes: Vec::new(),
            });
            break;
        }
        // The upstream artifact flows into this dispatch — record the
        // observed edge before the agent runs, so the edge exists even when
        // the dispatch fails (the flow happened).
        if let Some(from) = &upstream_agent {
            record_edge(from, declared_agent);
        }
        match delegate(declared_agent, &input).await {
            Ok(response) => {
                outcomes.push(WorkflowStageOutcome {
                    stage: stage.name.clone(),
                    agent: Some(declared_agent.clone()),
                    ok: true,
                    response: Some(response.clone()),
                    error: None,
                    notes: Vec::new(),
                });
                input = response;
                upstream_agent = Some(declared_agent.clone());
            }
            Err(error) => {
                outcomes.push(WorkflowStageOutcome {
                    stage: stage.name.clone(),
                    agent: Some(declared_agent.clone()),
                    ok: false,
                    response: None,
                    error: Some(error),
                    notes: Vec::new(),
                });
                break;
            }
        }
    }
    outcomes
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

    // ── run_workflow ──────────────────────────────────────────────────────

    /// The delegate closure each test builds: echoes its input, recording
    /// every (agent, task) call so tests can assert what each stage
    /// actually received. Inlined per test — the closure's future type is
    /// concrete and unnamed, so it cannot pass through a helper's return
    /// type (`impl Trait` is not allowed in `Fn` return position).
    macro_rules! echo_delegate {
        ($calls:expr) => {{
            // Clone BEFORE the move closure — a `.clone()` inside a `move`
            // closure captures the outer binding itself, not a copy.
            let captured = $calls.clone();
            move |agent: &str, task: &str| {
                let calls = captured.clone();
                let agent = agent.to_string();
                let task = task.to_string();
                async move {
                    calls.lock().unwrap().push((agent, task.clone()));
                    Ok(format!("{task} (handled)"))
                }
            }
        }};
    }

    #[tokio::test]
    async fn the_artifact_flows_verbatim_between_stages() {
        let workflow = template(vec![
            stage("gather", Some("research"), &["text"], &["analysis"]),
            stage("write", Some("writer"), &["analysis"], &["draft"]),
        ]);
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let delegate = echo_delegate!(calls.clone());
        let mut edges = Vec::new();
        let outcomes = run_workflow(&workflow, "the question", lookup, delegate, |from, to| {
            edges.push((from.to_string(), to.to_string()))
        })
        .await;

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|o| o.ok));
        // Stage 1 received the caller's task; stage 2 received stage 1's
        // response verbatim — no framing invented here.
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls[0],
            ("research".to_string(), "the question".to_string())
        );
        assert_eq!(
            calls[1],
            ("writer".to_string(), "the question (handled)".to_string())
        );
        // One observed edge: research → writer.
        assert_eq!(edges, vec![("research".to_string(), "writer".to_string())]);
    }

    #[tokio::test]
    async fn a_single_stage_run_records_no_edges() {
        // No downstream dispatch — no observed edge. The edge is not
        // "agents co-occur in a template", it is "an artifact flowed".
        let workflow = template(vec![stage(
            "gather",
            Some("research"),
            &["text"],
            &["analysis"],
        )]);
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let delegate = echo_delegate!(calls);
        let mut edges = Vec::new();
        let outcomes = run_workflow(&workflow, "the question", lookup, delegate, |from, to| {
            edges.push((from.to_string(), to.to_string()))
        })
        .await;
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].ok);
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn an_open_slot_stops_the_run_with_a_named_outcome() {
        let workflow = template(vec![
            stage("gather", Some("research"), &["text"], &["analysis"]),
            stage("write", None, &["analysis"], &["draft"]),
        ]);
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let delegate = echo_delegate!(calls.clone());
        let mut edges = Vec::new();
        let outcomes = run_workflow(&workflow, "the question", lookup, delegate, |from, to| {
            edges.push((from.to_string(), to.to_string()))
        })
        .await;

        // Stage 1 ran; stage 2 never dispatched (open slot) — so no edge,
        // and the outcome names the slot so the operator can fill it.
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes[0].ok);
        assert!(!outcomes[1].ok);
        assert!(outcomes[1].notes[0].contains("open slot"));
        assert!(edges.is_empty());
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_missing_agent_stops_the_run_without_dispatching() {
        let workflow = template(vec![
            stage("gather", Some("research"), &["text"], &["analysis"]),
            stage("write", Some("ghost"), &["analysis"], &["draft"]),
        ]);
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let delegate = echo_delegate!(calls.clone());
        let mut edges = Vec::new();
        let outcomes = run_workflow(&workflow, "the question", lookup, delegate, |from, to| {
            edges.push((from.to_string(), to.to_string()))
        })
        .await;

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes[0].ok);
        assert!(!outcomes[1].ok);
        assert!(outcomes[1].error.as_deref().unwrap().contains("ghost"));
        // The ghost never received the artifact — no edge.
        assert!(edges.is_empty());
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_failed_downstream_dispatch_still_records_the_edge() {
        // The artifact flowed — the downstream agent ran and failed. The
        // edge is real (fermi's parent_episode_id semantics: execution
        // caused by execution), so the observed topology keeps it.
        let workflow = template(vec![
            stage("gather", Some("research"), &["text"], &["analysis"]),
            stage("write", Some("writer"), &["analysis"], &["draft"]),
        ]);
        let fail_writer = |_agent: &str, task: &str| {
            let task = task.to_string();
            async move {
                if task.contains("(handled)") {
                    Err("writer exploded".to_string())
                } else {
                    Ok(format!("{task} (handled)"))
                }
            }
        };
        let mut edges = Vec::new();
        let outcomes = run_workflow(
            &workflow,
            "the question",
            lookup,
            fail_writer,
            |from, to| edges.push((from.to_string(), to.to_string())),
        )
        .await;

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes[0].ok);
        assert!(!outcomes[1].ok);
        assert_eq!(outcomes[1].error.as_deref(), Some("writer exploded"));
        assert_eq!(edges, vec![("research".to_string(), "writer".to_string())]);
    }

    // ── observed_seam_agreement ────────────────────────────────────────────

    #[test]
    fn an_observed_edge_with_port_overlap_is_declared() {
        let (agreement, note) = observed_seam_agreement("research", "writer", lookup);
        assert_eq!(agreement, "declared");
        assert!(note.contains("overlap"));
    }

    #[test]
    fn an_observed_edge_without_overlap_is_undeclared_and_that_is_the_norm() {
        // fermi measured 3 of 3 production hand-offs with zero declared
        // overlap — the classification must name the ports, not judge.
        let (agreement, note) = observed_seam_agreement("writer", "research", lookup);
        assert_eq!(agreement, "undeclared");
        assert!(
            note.contains("draft"),
            "the note names the produced port: {note}"
        );
        assert!(
            note.contains("text"),
            "the note names the accepted port: {note}"
        );
    }

    #[test]
    fn an_observed_edge_with_an_empty_port_side_is_permissive() {
        // Absence is not contradiction — the same rule as check_seams.
        let bare = |agent: &str| match agent {
            "research" => Some((vec![], vec!["analysis".to_string()])),
            "writer" => Some((vec!["analysis".to_string()], vec![])),
            _ => None,
        };
        let (agreement, _) = observed_seam_agreement("research", "writer", bare);
        assert_eq!(agreement, "declared");
    }

    #[test]
    fn an_observed_edge_with_a_removed_card_is_unknown_not_undeclared() {
        // A removed card is not evidence about the ports — judging it
        // "undeclared" would fabricate a mismatch from missing data.
        let (agreement, note) = observed_seam_agreement("research", "ghost", lookup);
        assert_eq!(agreement, "unknown_agent");
        assert!(note.contains("removed"));
    }
}
