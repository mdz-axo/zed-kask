//! Socratic inquiry orchestration — structured dialogue using kanban tasks
//! and kata prompts as a 4-stage Socratic process.
//!
//! A Socratic inquiry is a kanban Task advancing through stages:
//!   Elicit (Backlog) → Structure (Ready) → Test (InProgress) → Summarize (Review)
//!
//! Each stage uses a kata prompt type. The Curator presents the prompt, the
//! user responds via comments, and the Curator advances the task. Regulation spans
//! carry PKO ontology anchors (pko:ChangeOfStatus, pko:UserFeedbackOccurrence).
//!
//! Zero new types. Zero new tools. Pure orchestration of existing infrastructure.

use super::types::{Task, TaskSpec, TaskStatus};
use super::{KanbanError, KanbanService};
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::id::{BoardId, TaskId};

/// Return the Socratic stage name for a given TaskStatus.
#[must_use]
pub fn stage_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Backlog => "Elicit",
        TaskStatus::Ready => "Structure",
        TaskStatus::InProgress => "Test",
        TaskStatus::Review => "Summarize",
        TaskStatus::Done => "Complete",
    }
}

/// Create a new Socratic inquiry task on a board.
///
/// The task is initialized with a generous gas budget for multi-turn dialogue
/// and a description of the 4-stage process.
#[must_use = "result must be used"]
pub fn create_inquiry(
    service: &KanbanService,
    board_id: BoardId,
    topic: &str,
    owner: WebID,
) -> Result<Task, KanbanError> {
    let spec = TaskSpec::new(format!("Inquiry: {topic}"))
        .with_gas_budget(50000)
        .with_description(format!(
            "Socratic inquiry into: {topic}\n\
             Stages: Elicit → Structure → Test → Summarize\n\
             Each stage presents a kata prompt. Respond in the comment thread."
        ));
    let task = service.task_create(board_id, spec, owner)?;

    tracing::info!(
        target: "reg.kata",
        operation = "socratic_inquiry_created",
        task_id = %task.id,
        topic = %topic,
        "REG"
    );

    Ok(task)
}

/// Generate the kata prompt for the current Socratic stage.
///
/// Returns (prompt_text, stage_name). The caller presents this to the user.
#[must_use = "result must be used"]
pub fn prompt(service: &KanbanService, task_id: TaskId) -> Result<(String, String), KanbanError> {
    let task = service.task_get(task_id)?.ok_or_else(|| {
        KanbanError::NotFound(NotFound {
            entity_type: "task".to_string(),
            id: task_id.to_string(),
        })
    })?;

    let name = stage_name(task.status);
    let body = match task.status {
        TaskStatus::Backlog => service.task_coaching_prompt(task_id)?,
        TaskStatus::Ready => service.task_improvement_prompt(task_id)?,
        TaskStatus::InProgress => service.task_practice_prompt(task_id, "the current obstacle")?,
        TaskStatus::Review => {
            return Ok((
                format!(
                    "╔══ {} — Synthesize ═══════════════════════════════════════╗\n\
                     ║  What did you learn through this inquiry?\n\
                     ║  What evidence supports your conclusions?\n\
                     ║  What remains uncertain?\n\
                     ║  Submit your summary to complete the inquiry.\n\
                     ╚══════════════════════════════════════════════════════════╝",
                    name
                ),
                name.to_string(),
            ));
        }
        TaskStatus::Done => return Ok(("Inquiry complete.".into(), name.to_string())),
    };

    let framed = format!(
        "╔══ {} ═══════════════════════════════════════════════╗\n\
         ║  Topic: {}\n\
         ╠══════════════════════════════════════════════════════════╣\n\
         {}\n\
         ╚══════════════════════════════════════════════════════════╝",
        name, task.title, body
    );
    Ok((framed, name.to_string()))
}

/// Advance the inquiry to the next stage.
///
/// Moves the task status forward. Returns a description of the transition.
/// If the task is in Review, the caller should use `task_verify` to complete.
#[must_use = "result must be used"]
pub fn advance(
    service: &KanbanService,
    task_id: TaskId,
    user: WebID,
) -> Result<String, KanbanError> {
    let task = service.task_get(task_id)?.ok_or_else(|| {
        KanbanError::NotFound(NotFound {
            entity_type: "task".to_string(),
            id: task_id.to_string(),
        })
    })?;

    let from = stage_name(task.status);
    let (next, to) = match task.status {
        TaskStatus::Backlog => (TaskStatus::Ready, "Structure"),
        TaskStatus::Ready => (TaskStatus::InProgress, "Test"),
        TaskStatus::InProgress => (TaskStatus::Review, "Summarize"),
        TaskStatus::Review => return Ok("Summarize — use task_verify to complete".into()),
        TaskStatus::Done => return Ok("Complete".into()),
    };

    service.task_move(task_id, next, user)?;
    let result = format!("{from} → {to}");

    tracing::info!(
        target: "reg.kata",
        operation = "socratic_stage_advanced",
        task_id = %task_id,
        from = %from,
        to = %to,
        "REG"
    );

    Ok(result)
}

/// Check whether the user has responded since the last prompt.
///
/// Returns true if at least one new comment exists since `last_count`.
/// This is the readiness gate — presence of a response, not its quality.
#[must_use = "result must be used"]
pub fn has_response(
    service: &KanbanService,
    task_id: TaskId,
    last_count: usize,
) -> Result<bool, KanbanError> {
    let comments = service.task_comments_since(task_id, last_count)?;
    Ok(!comments.is_empty())
}

/// Quality gate result — evaluates whether a user response meets the
/// structural expectations for the current Socratic stage.
#[derive(Debug, Clone)]
pub struct QualityGate {
    pub passed: bool,
    pub stage: String,
    pub feedback: String,
}

/// Evaluate the quality of a user response for the current Socratic stage.
///
/// Each stage has structural expectations:
/// - Elicit: response must have substance (≥30 chars, contains a claim or observation)
/// - Structure: must identify direction AND current condition (contains "direction" or "target" and "current" or "now")
/// - Test: must distinguish facts from interpretations (contains "observation" or "fact" and "interpretation" or "assume")
/// - Review: delegated to task_verify (always passes readiness, caller handles verification)
///
/// Returns QualityGate with pass/fail and specific feedback for the user.
#[must_use = "result must be used"]
pub fn quality_check(
    service: &KanbanService,
    task_id: TaskId,
    response: &str,
) -> Result<QualityGate, KanbanError> {
    let task = service.task_get(task_id)?.ok_or_else(|| {
        KanbanError::NotFound(NotFound {
            entity_type: "task".to_string(),
            id: task_id.to_string(),
        })
    })?;

    let stage = stage_name(task.status);
    let resp_lower = response.to_lowercase();
    let resp_len = response.trim().len();

    let (passed, feedback) = match task.status {
        TaskStatus::Backlog => {
            if resp_len < 30 {
                (
                    false,
                    "Your response is brief. The Elicit stage asks you to explore the\n\
                     target condition, actual condition, obstacles, and next experiment.\n\
                     Please elaborate — what do you observe? What questions arise?"
                        .into(),
                )
            } else {
                (true, String::new())
            }
        }
        TaskStatus::Ready => {
            let has_direction = resp_lower.contains("direction")
                || resp_lower.contains("target")
                || resp_lower.contains("goal");
            let has_current = resp_lower.contains("current")
                || resp_lower.contains("now")
                || resp_lower.contains("actual");
            if !has_direction || !has_current {
                (
                    false,
                    "The Structure stage asks you to identify:\n\
                     • A direction or target condition (what you're aiming for)\n\
                     • The current or actual condition (where you are now)\n\
                     Please address both in your response."
                        .into(),
                )
            } else {
                (true, String::new())
            }
        }
        TaskStatus::InProgress => {
            let has_fact = resp_lower.contains("observation")
                || resp_lower.contains("fact")
                || resp_lower.contains("observe")
                || resp_lower.contains("data");
            let has_interpretation = resp_lower.contains("interpret")
                || resp_lower.contains("assume")
                || resp_lower.contains("hypothesis")
                || resp_lower.contains("theory");
            if !has_fact || !has_interpretation {
                (
                    false,
                    "The Test stage asks you to distinguish:\n\
                     • What you OBSERVE (facts, data, evidence)\n\
                     • What you INTERPRET (assumptions, guesses, theories)\n\
                     Please separate these in your response."
                        .into(),
                )
            } else {
                (true, String::new())
            }
        }
        TaskStatus::Review | TaskStatus::Done => (true, String::new()),
    };

    Ok(QualityGate {
        passed,
        stage: stage.to_string(),
        feedback,
    })
}

/// Socratic role — the four inquiry perspectives for multi-inquiry coordination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocraticRole {
    Planner,
    Diagnoser,
    Tutor,
    Assessor,
}

impl SocraticRole {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            SocraticRole::Planner => "Planner",
            SocraticRole::Diagnoser => "Diagnoser",
            SocraticRole::Tutor => "Tutor",
            SocraticRole::Assessor => "Assessor",
        }
    }

    /// The kata prompt seed for each role — what perspective they bring.
    #[must_use]
    pub fn seed_prompt(&self, topic: &str) -> String {
        match self {
            SocraticRole::Planner => format!(
                "You are the Planner. Decompose this topic into a structured learning path.\n\
                 Topic: {topic}\n\
                 Identify: key questions, prerequisite knowledge, logical sequence, and milestones.\n\
                 Output a decomposition plan with ordered questions."
            ),
            SocraticRole::Diagnoser => format!(
                "You are the Diagnoser. Assess the current understanding of this topic.\n\
                 Topic: {topic}\n\
                 Identify: what is known with confidence, what is assumed, what gaps exist,\n\
                 what contradictions or inconsistencies are present."
            ),
            SocraticRole::Tutor => format!(
                "You are the Tutor. Guide exploration of this topic through Socratic dialogue.\n\
                 Topic: {topic}\n\
                 Use the Coaching Kata: target condition → actual condition → obstacles →\n\
                 experiment → learning velocity. Ask questions, don't give answers."
            ),
            SocraticRole::Assessor => format!(
                "You are the Assessor. Evaluate the evidence and conclusions from this inquiry.\n\
                 Topic: {topic}\n\
                 Check: are claims supported by evidence? Are uncertainties acknowledged?\n\
                 What remains to be explored? What is the confidence level in the conclusions?"
            ),
        }
    }
}

/// Spawn four role-based inquiry tasks on a board for coordinated exploration.
///
/// Creates Planner, Diagnoser, Tutor, and Assessor tasks — each a Socratic
/// inquiry scoped to a specific perspective. The Curator reads all four
/// comment threads and synthesizes the results.
///
/// Returns the created task IDs in role order.
#[must_use = "result must be used"]
pub fn spawn_role_inquiries(
    service: &KanbanService,
    board_id: BoardId,
    topic: &str,
    owner: WebID,
) -> Result<Vec<Task>, KanbanError> {
    let roles = [
        SocraticRole::Planner,
        SocraticRole::Diagnoser,
        SocraticRole::Tutor,
        SocraticRole::Assessor,
    ];
    let mut tasks = Vec::new();
    for role in &roles {
        let spec = TaskSpec::new(format!("{}: {topic}", role.as_str()))
            .with_gas_budget(50000)
            .with_description(role.seed_prompt(topic));
        let task = service.task_create(board_id, spec, owner)?;
        tasks.push(task);
    }
    Ok(tasks)
}

/// Read all comments from a set of role tasks and synthesize a summary.
///
/// Returns a formatted report showing each role's latest contribution.
#[must_use = "result must be used"]
pub fn synthesize_roles(service: &KanbanService, tasks: &[Task]) -> Result<String, KanbanError> {
    let mut report =
        String::from("╔══ Multi-Role Inquiry Synthesis ═══════════════════════════╗\n");
    for task in tasks {
        let comments = service.task_comments(task.id)?;
        let role = task.title.split(':').next().unwrap_or("Unknown").trim();
        report.push_str(&format!("║  {} ({})\n", role, task.id));
        report.push_str("║  Status: ");
        report.push_str(stage_name(task.status));
        report.push('\n');
        if let Some(last) = comments.last() {
            let preview: String = last.body.chars().take(120).collect();
            report.push_str(&format!("║  Latest: {}\n", preview));
        } else {
            report.push_str("║  (no responses yet)\n");
        }
        report.push_str("╠══════════════════════════════════════════════════════════╣\n");
    }
    report.push_str("║  Use /kanban socratic continue <task-id> <response>\n");
    report.push_str("║  to advance individual role inquiries.\n");
    report.push_str("╚══════════════════════════════════════════════════════════╝");
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::types::ColumnDef;
    use super::*;
    use hkask_storage::HMemStore;

    fn make_svc() -> (KanbanService, WebID, BoardId) {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = HMemStore::from_driver(driver);
        let service = KanbanService::new(store);
        let owner = hkask_types::WebID::new();
        let cols = vec![
            ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 0),
            ColumnDef::new("Ready".into(), TaskStatus::Ready, 1),
            ColumnDef::new("In Progress".into(), TaskStatus::InProgress, 2),
            ColumnDef::new("Review".into(), TaskStatus::Review, 3),
            ColumnDef::new("Done".into(), TaskStatus::Done, 4),
        ];
        let board = service
            .board_create(owner, "Socratic", &cols)
            .expect("board_create");
        (service, owner, board.id)
    }

    #[test]
    fn full_cycle_progresses_through_all_stages() {
        let (svc, owner, board_id) = make_svc();
        let task =
            create_inquiry(&svc, board_id, "What is knowledge?", owner).expect("create_inquiry");
        assert_eq!(task.status, TaskStatus::Backlog);

        // Elicit
        let (prompt_text, stage) = prompt(&svc, task.id).expect("prompt");
        assert_eq!(stage, "Elicit");
        assert!(prompt_text.contains("Coaching Kata"));
        svc.task_comment(task.id, owner, "Knowledge is justified true belief.")
            .expect("comment");
        assert!(has_response(&svc, task.id, 0).expect("check"));
        let t = advance(&svc, task.id, owner).expect("advance");
        assert_eq!(t, "Elicit → Structure");

        // Structure
        let (prompt_text, stage) = prompt(&svc, task.id).expect("prompt");
        assert_eq!(stage, "Structure");
        assert!(prompt_text.contains("Improvement Kata"));
        svc.task_comment(task.id, owner, "Direction: define knowledge precisely.")
            .expect("comment");
        advance(&svc, task.id, owner).expect("advance");

        // Test
        let (prompt_text, stage) = prompt(&svc, task.id).expect("prompt");
        assert_eq!(stage, "Test");
        assert!(prompt_text.contains("Starter Kata"));
        svc.task_comment(task.id, owner, "Observation: Gettier cases exist.")
            .expect("comment");
        advance(&svc, task.id, owner).expect("advance");

        // Summarize
        let (prompt_text, stage) = prompt(&svc, task.id).expect("prompt");
        assert_eq!(stage, "Summarize");
        assert!(prompt_text.contains("Synthesize"));

        // Verify
        let (verified, _v) = svc
            .task_verify(task.id, "JTB is insufficient; Gettier shows gap.", owner)
            .expect("verify");
        assert_eq!(verified.status, TaskStatus::Done);
    }

    #[test]
    fn has_response_detects_new_comments() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        assert!(!has_response(&svc, task.id, 0).expect("check"));
        svc.task_comment(task.id, owner, "Hello").expect("comment");
        assert!(has_response(&svc, task.id, 0).expect("check"));
        assert!(!has_response(&svc, task.id, 1).expect("check"));
    }

    #[test]
    fn done_stage_returns_complete() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        advance(&svc, task.id, owner).expect("advance");
        advance(&svc, task.id, owner).expect("advance");
        advance(&svc, task.id, owner).expect("advance");
        svc.task_verify(task.id, "Done.", owner).expect("verify");
        let (prompt_text, stage) = prompt(&svc, task.id).expect("prompt");
        assert_eq!(stage, "Complete");
        assert_eq!(prompt_text, "Inquiry complete.");
    }

    #[test]
    fn quality_gate_rejects_short_elicit_response() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        svc.task_comment(task.id, owner, "ok").expect("comment");
        let gate = quality_check(&svc, task.id, "ok").expect("check");
        assert!(!gate.passed);
        assert!(gate.feedback.contains("brief"));
    }

    #[test]
    fn quality_gate_passes_substantial_elicit_response() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        svc.task_comment(task.id, owner, "I observe that knowledge requires both justification and truth, but Gettier cases challenge this.")
            .expect("comment");
        let gate = quality_check(
            &svc,
            task.id,
            "I observe that knowledge requires justification",
        )
        .expect("check");
        assert!(gate.passed);
    }

    #[test]
    fn quality_gate_checks_structure_stage() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        advance(&svc, task.id, owner).expect("advance"); // → Structure
        svc.task_comment(task.id, owner, "Just some thoughts")
            .expect("comment");
        let gate = quality_check(&svc, task.id, "Just some thoughts").expect("check");
        assert!(!gate.passed, "missing direction and current");
        assert!(gate.feedback.contains("direction"));
    }

    #[test]
    fn quality_gate_passes_structure_with_direction_and_current() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        advance(&svc, task.id, owner).expect("advance");
        svc.task_comment(task.id, owner, "My target direction is to define knowledge. Currently, JTB is accepted but insufficient.")
            .expect("comment");
        let gate = quality_check(
            &svc,
            task.id,
            "target direction is to define knowledge. Currently, JTB is accepted",
        )
        .expect("check");
        assert!(gate.passed);
    }

    #[test]
    fn quality_gate_checks_test_stage_facts_vs_interpretations() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        advance(&svc, task.id, owner).expect("advance");
        advance(&svc, task.id, owner).expect("advance"); // → Test
        svc.task_comment(task.id, owner, "I think Gettier cases are important")
            .expect("comment");
        let gate =
            quality_check(&svc, task.id, "I think Gettier cases are important").expect("check");
        assert!(!gate.passed, "missing observation and interpretation");
        assert!(gate.feedback.contains("OBSERVE"));
    }

    #[test]
    fn quality_gate_passes_test_with_facts_and_interpretations() {
        let (svc, owner, board_id) = make_svc();
        let task = create_inquiry(&svc, board_id, "Test", owner).expect("create");
        advance(&svc, task.id, owner).expect("advance");
        advance(&svc, task.id, owner).expect("advance");
        svc.task_comment(task.id, owner, "Observation: Gettier 1963 shows counterexamples. My interpretation: JTB is necessary but not sufficient.")
            .expect("comment");
        let gate = quality_check(
            &svc,
            task.id,
            "Observation: Gettier shows counterexamples. My interpretation: JTB is not sufficient.",
        )
        .expect("check");
        assert!(gate.passed);
    }

    #[test]
    fn spawn_role_inquiries_creates_four_tasks() {
        let (svc, owner, board_id) = make_svc();
        let tasks =
            spawn_role_inquiries(&svc, board_id, "What is knowledge?", owner).expect("spawn");
        assert_eq!(tasks.len(), 4);
        assert!(tasks[0].title.contains("Planner"));
        assert!(tasks[1].title.contains("Diagnoser"));
        assert!(tasks[2].title.contains("Tutor"));
        assert!(tasks[3].title.contains("Assessor"));
    }

    #[test]
    fn synthesize_roles_reports_all_roles() {
        let (svc, owner, board_id) = make_svc();
        let tasks =
            spawn_role_inquiries(&svc, board_id, "What is knowledge?", owner).expect("spawn");
        svc.task_comment(
            tasks[0].id,
            owner,
            "Decomposition: 1. Define terms. 2. Examine JTB. 3. Evaluate Gettier.",
        )
        .expect("comment");
        let report = synthesize_roles(&svc, &tasks).expect("synthesize");
        assert!(report.contains("Planner"));
        assert!(report.contains("Diagnoser"));
        assert!(report.contains("Tutor"));
        assert!(report.contains("Assessor"));
        assert!(report.contains("Decomposition"));
    }
}
