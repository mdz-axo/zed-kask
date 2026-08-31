//! Goal operations — the native goal-setting and verification system.
//!
//! Thin vertical slice per `kask/docs/architecture/functional-interaction-spec.md`
//! Phase B: the functional goal (kata target condition) as a first-class
//! object with verifiable criteria, recorded verdicts, and Brier-scored
//! intake predictions. Schema lifted from the validated `goal-analysis`
//! skill templates.
//!
//! **Ephemeral by design (operator ruling 2026-08-29):** the goal store is
//! in-memory and dies with the process — zed-agent goals leave no persistent
//! clutter. The curator's memory is the durable vehicle: every `kanban_goal_*`
//! tool result in a turn is extracted by the thread-side record builder and
//! written as a first-class goal h_mem by `kask_bridge/src/memory/ingest.rs`,
//! so therapy / algedonic-review find goal entities, not prose archaeology.
//!
//! HMem scheme (ephemeral in-memory store):
//!   kanban:goal → {goal_id} → JSON Goal

use hkask_storage::HMemStore;
use hkask_types::WebID;
use hkask_types::id::{GoalID, TaskId};

use super::service::KanbanService;
use super::types::KanbanError;
use crate::kanban::{Goal, GoalResolution, GoalVerdict, VerificationCriterion};

const GOAL_ENTITY: &str = "kanban:goal";

/// Bounds on goal criteria — lifted from `goal-analysis` (`create.j2`:
/// "2–4 observable semantic conditions"), relaxed to allow a single
/// criterion for trivially verifiable goals while keeping verification
/// tractable.
const MIN_CRITERIA: usize = 1;
const MAX_CRITERIA: usize = 4;

impl KanbanService {
    // ── Goal operations ───────────────────────────────────────────────────

    /// Create a functional goal with observable criteria.
    ///
    /// pre:  goal_text non-empty; 1–4 criteria; prediction in 0.0..=1.0 if
    ///       given; task_id refers to an existing task if given
    /// post: goal persisted as a h_mem; returns the created Goal
    #[must_use = "result must be used"]
    pub(crate) fn goal_create(
        &self,
        goal_text: String,
        criteria: Vec<VerificationCriterion>,
        prediction: Option<f64>,
        task_id: Option<TaskId>,
        owner: WebID,
    ) -> Result<Goal, KanbanError> {
        if goal_text.trim().is_empty() {
            return Err(KanbanError::InvalidInput(
                "goal_text must be non-empty — the functional goal in the user's words".into(),
            ));
        }
        if criteria.len() < MIN_CRITERIA || criteria.len() > MAX_CRITERIA {
            return Err(KanbanError::InvalidInput(format!(
                "goal requires {MIN_CRITERIA}–{MAX_CRITERIA} observable criteria (got {})",
                criteria.len()
            )));
        }
        if let Some(p) = prediction
            && !(0.0..=1.0).contains(&p)
        {
            return Err(KanbanError::InvalidInput(format!(
                "prediction must be in 0.0..=1.0 (got {p})"
            )));
        }
        if let Some(tid) = task_id
            && self.task_get(tid)?.is_none()
        {
            return Err(KanbanError::InvalidInput(format!(
                "task {tid} not found — cannot link goal to a nonexistent task"
            )));
        }

        let mut goal = Goal::new(GoalID::new(), goal_text, criteria, owner);
        goal.prediction = prediction;
        goal.task_id = task_id;

        // Process-family anchor: `pplan:Step` (P-Plan, soft-reused by PKO) —
        // the same term the goal responses emit via `kanban_type_to_pko`,
        // so the goal's record and its wire surface agree. Operator decision
        // 2026-08-30: goals anchor on the PKO family so the whole kanban
        // graph (boards = pko:Procedure, tasks and goals = pplan:Step,
        // verdicts = pko:StepVerification) is one linked dataset in a
        // published ontology — family coherence over concept-exactness.
        // The former `pko:Goal` was fabricated (PKO publishes no Goal
        // class); the interim IAO:0000005 anchor was rejected as opaque.
        let goal_ontology = hkask_types::HMemOntology {
            dimensions: vec![hkask_types::Dimension::Why.as_str().to_string()],
            dc_type: hkask_bridge_ontology::pko::STEP.to_string(),
            dc_source: "kanban".to_string(),
            pko_procedure: Some(goal.id.to_string()),
            ..Default::default()
        };
        let h_mem = hkask_storage::HMem::new(
            GOAL_ENTITY,
            &goal.id.to_string(),
            serde_json::to_value(&goal)
                .map_err(|e| KanbanError::Internal(format!("goal serialization failed: {e}")))?,
            owner,
        )
        .with_ontology(goal_ontology);
        self.goal_store()?
            .insert(&h_mem)
            .map_err(|e| KanbanError::Internal(format!("h_mem insert failed: {e}")))?;

        tracing::info!(
            target: "hkask.kanban",
            operation = "goal_created",
            goal_id = %goal.id,
            owner = %owner,
            "REG"
        );

        Ok(goal)
    }

    /// Get a goal by ID.
    ///
    /// pre:  goal_id is valid
    /// post: returns Some(Goal) if found, None otherwise
    #[must_use = "result must be used"]
    pub(crate) fn goal_get(&self, goal_id: GoalID) -> Result<Option<Goal>, KanbanError> {
        let h_mems = self
            .goal_store()?
            .query_by_entity_attribute(GOAL_ENTITY, &goal_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        if let Some(t) = h_mems.into_iter().next() {
            let goal = serde_json::from_value::<Goal>(t.value)
                .map_err(|e| KanbanError::Internal(format!("deserialization failed: {e}")))?;
            Ok(Some(goal))
        } else {
            Ok(None)
        }
    }

    /// List all goals for a given owner, newest first.
    ///
    /// pre:  owner is a valid WebID
    /// post: returns all goals owned by this agent
    #[must_use = "result must be used"]
    pub(crate) fn goal_list(&self, owner: &WebID) -> Result<Vec<Goal>, KanbanError> {
        let h_mems = self
            .goal_store()?
            .query_by_entity(GOAL_ENTITY)
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        let mut goals: Vec<Goal> = Vec::new();
        for t in &h_mems {
            if t.access.owner_webid == *owner
                && let Ok(goal) = serde_json::from_value::<Goal>(t.value.clone())
            {
                goals.push(goal);
            }
        }

        goals.sort_by_key(|g| std::cmp::Reverse(g.created_at));
        Ok(goals)
    }

    /// Record a judge verdict against the goal's criteria.
    ///
    /// Appends to the verdict history — the history IS the learning. Only
    /// the goal owner may judge (P12).
    ///
    /// pre:  goal exists and is owned by caller; confidence in 0.0..=1.0;
    ///       criterion results cover every criterion exactly once (indices
    ///       in range, no duplicates, none missing)
    /// post: verdict appended and persisted; returns the updated Goal
    #[must_use = "result must be used"]
    pub(crate) fn goal_judge(
        &self,
        goal_id: GoalID,
        verdict: GoalVerdict,
        judge: WebID,
    ) -> Result<Goal, KanbanError> {
        let mut goal = self.require_goal(goal_id)?;
        if goal.owner != judge {
            return Err(KanbanError::PermissionDenied(format!(
                "goal {goal_id} is not owned by caller — cannot judge"
            )));
        }
        if goal.resolution.is_some() {
            return Err(KanbanError::InvalidInput(format!(
                "goal {goal_id} is already resolved — open a new goal to continue work"
            )));
        }
        if !(0.0..=1.0).contains(&verdict.confidence) {
            return Err(KanbanError::InvalidInput(format!(
                "confidence must be in 0.0..=1.0 (got {})",
                verdict.confidence
            )));
        }
        // A verdict must judge EVERY criterion: the per-criterion results are
        // the explicit obligation the Brier score later discharges. A verdict
        // with missing or duplicated criterion results is an unanchored
        // claim — reject it with an error naming what is missing.
        let criterion_count = goal.criteria.len();
        if let Some(cj) = verdict
            .criterion_results
            .iter()
            .find(|cj| cj.index >= criterion_count)
        {
            return Err(KanbanError::InvalidInput(format!(
                "criterion index {} out of range (goal has {} criteria)",
                cj.index, criterion_count
            )));
        }
        let mut judged: Vec<usize> = verdict
            .criterion_results
            .iter()
            .map(|cj| cj.index)
            .collect();
        judged.sort_unstable();
        judged.dedup();
        if judged.len() != verdict.criterion_results.len() {
            return Err(KanbanError::InvalidInput(
                "criterion results contain duplicate indices — judge each criterion exactly once"
                    .to_string(),
            ));
        }
        let missing: Vec<usize> = (0..criterion_count)
            .filter(|index| !judged.contains(index))
            .collect();
        if !missing.is_empty() {
            return Err(KanbanError::InvalidInput(format!(
                "verdict must judge every criterion — missing indices {missing:?} (goal has {criterion_count} criteria)"
            )));
        }

        goal.verdicts.push(verdict);
        goal.updated_at = chrono::Utc::now();
        self.goal_persist(&goal)?;
        Ok(goal)
    }

    /// Resolve a goal: record the realized outcome and Brier-score the
    /// intake prediction.
    ///
    /// pre:  goal exists and is owned by caller; not already resolved
    /// post: resolution recorded; `brier` is `None` (surfaced, never faked)
    ///       when no intake prediction was recorded
    #[must_use = "result must be used"]
    pub(crate) fn goal_score(
        &self,
        goal_id: GoalID,
        achieved: bool,
        judge: WebID,
    ) -> Result<Goal, KanbanError> {
        let mut goal = self.require_goal(goal_id)?;
        if goal.owner != judge {
            return Err(KanbanError::PermissionDenied(format!(
                "goal {goal_id} is not owned by caller — cannot score"
            )));
        }
        if goal.resolution.is_some() {
            return Err(KanbanError::InvalidInput(format!(
                "goal {goal_id} is already resolved"
            )));
        }

        // Brier of the intake prediction against the realized outcome.
        // No prediction → None, surfaced by the caller — a synthetic 0
        // would read as "perfectly calibrated" (the calibration.rs lesson).
        let brier = goal
            .prediction
            .map(|p| hkask_forecast::brier_score(p, achieved));

        goal.resolution = Some(GoalResolution {
            achieved,
            brier,
            resolved_at: chrono::Utc::now(),
        });
        goal.updated_at = chrono::Utc::now();
        self.goal_persist(&goal)?;

        tracing::info!(
            target: "hkask.kanban",
            operation = "goal_resolved",
            goal_id = %goal.id,
            achieved,
            brier = ?brier,
            "REG"
        );

        Ok(goal)
    }

    /// Fetch a goal by id or return `KanbanError::NotFound`.
    /// Mirrors `require_task`.
    fn require_goal(&self, goal_id: GoalID) -> Result<Goal, KanbanError> {
        self.goal_get(goal_id)?.ok_or_else(|| {
            KanbanError::NotFound(hkask_types::NotFound {
                entity_type: "goal".to_string(),
                id: goal_id.to_string(),
            })
        })
    }

    /// The ephemeral in-memory goal store. Lazily initialized on first
    /// use; shared across service clones via the `Arc` in
    /// [`KanbanService`]. Init failure surfaces as a typed error naming the
    /// cause — never a silent fallback.
    fn goal_store(&self) -> Result<HMemStore, KanbanError> {
        let mut guard = self
            .goal_store
            .lock()
            .map_err(|_| KanbanError::Internal("goal store mutex poisoned".to_string()))?;
        if let Some(store) = &*guard {
            return Ok(store.clone());
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = HMemStore::from_driver(driver)
            .map_err(|e| KanbanError::Internal(format!("in-memory goal store init failed: {e}")))?;
        *guard = Some(store.clone());
        Ok(store)
    }

    /// Persist a goal to the ephemeral store (insert-or-replace by
    /// entity+attribute key).
    fn goal_persist(&self, goal: &Goal) -> Result<(), KanbanError> {
        let h_mem = hkask_storage::HMem::new(
            GOAL_ENTITY,
            &goal.id.to_string(),
            serde_json::to_value(goal)
                .map_err(|e| KanbanError::Internal(format!("goal serialization failed: {e}")))?,
            goal.owner,
        );
        self.goal_store()?
            .insert(&h_mem)
            .map_err(|e| KanbanError::Internal(format!("h_mem insert failed: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod goal_tests {
    use super::*;
    use crate::kanban::{CriterionJudgment, GoalVerdictValue};
    use hkask_storage::HMemStore;

    fn make_service() -> KanbanService {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        KanbanService::new(HMemStore::from_driver(driver).expect("hmem store init"))
    }

    fn criteria(n: usize) -> Vec<VerificationCriterion> {
        (0..n)
            .map(|i| VerificationCriterion::new(format!("criterion {i} is observable")))
            .collect()
    }

    #[test]
    fn goal_create_round_trip() {
        let svc = make_service();
        let owner = WebID::new();
        let goal = svc
            .goal_create(
                "The user can filter search results by date".into(),
                criteria(2),
                Some(0.8),
                None,
                owner,
            )
            .unwrap();
        assert_eq!(goal.prediction, Some(0.8));
        assert!(goal.verdicts.is_empty());

        let fetched = svc.goal_get(goal.id).unwrap().expect("goal persisted");
        assert_eq!(
            fetched.goal_text,
            "The user can filter search results by date"
        );
        assert_eq!(fetched.criteria.len(), 2);
    }

    #[test]
    fn goal_create_rejects_empty_text_and_bad_criteria_counts() {
        let svc = make_service();
        let owner = WebID::new();
        assert!(
            svc.goal_create("".into(), criteria(2), None, None, owner)
                .is_err()
        );
        assert!(
            svc.goal_create("goal".into(), vec![], None, None, owner)
                .is_err()
        );
        assert!(
            svc.goal_create("goal".into(), criteria(5), None, None, owner)
                .is_err()
        );
        assert!(
            svc.goal_create("goal".into(), criteria(2), Some(1.5), None, owner)
                .is_err()
        );
    }

    #[test]
    fn goal_judge_appends_history_and_enforces_ownership() {
        let svc = make_service();
        let owner = WebID::new();
        let intruder = WebID::new();
        let goal = svc
            .goal_create("goal".into(), criteria(2), None, None, owner)
            .unwrap();

        let verdict = GoalVerdict {
            verdict: GoalVerdictValue::Continue,
            confidence: 0.7,
            criterion_results: vec![
                CriterionJudgment {
                    index: 0,
                    passed: false,
                    note: "not yet observable".into(),
                },
                CriterionJudgment {
                    index: 1,
                    passed: false,
                    note: "not yet observable".into(),
                },
            ],
            reasoning: "work in progress".into(),
            judged_at: chrono::Utc::now(),
        };
        let updated = svc.goal_judge(goal.id, verdict, owner).unwrap();
        assert_eq!(updated.verdicts.len(), 1);

        // Non-owner cannot judge (P12). Full criterion coverage so the ONLY
        // rejection reason is ownership.
        let verdict2 = GoalVerdict {
            verdict: GoalVerdictValue::Done,
            confidence: 0.9,
            criterion_results: vec![
                CriterionJudgment {
                    index: 0,
                    passed: true,
                    note: "observable".into(),
                },
                CriterionJudgment {
                    index: 1,
                    passed: true,
                    note: "observable".into(),
                },
            ],
            reasoning: "done".into(),
            judged_at: chrono::Utc::now(),
        };
        assert!(svc.goal_judge(goal.id, verdict2, intruder).is_err());

        // Out-of-range criterion index is rejected.
        let bad = GoalVerdict {
            verdict: GoalVerdictValue::Done,
            confidence: 0.9,
            criterion_results: vec![CriterionJudgment {
                index: 5,
                passed: true,
                note: "out of range".into(),
            }],
            reasoning: "bad index".into(),
            judged_at: chrono::Utc::now(),
        };
        assert!(svc.goal_judge(goal.id, bad, owner).is_err());
    }

    #[test]
    fn goal_judge_requires_every_criterion_judged_exactly_once() {
        // A verdict with missing or duplicate criterion results is an
        // unanchored claim — the per-criterion results are the explicit
        // obligation the Brier score discharges, so they must cover every
        // criterion exactly once.
        let svc = make_service();
        let owner = WebID::new();
        let goal = svc
            .goal_create("goal".into(), criteria(3), None, None, owner)
            .unwrap();

        let missing = GoalVerdict {
            verdict: GoalVerdictValue::Done,
            confidence: 0.9,
            criterion_results: vec![
                CriterionJudgment {
                    index: 0,
                    passed: true,
                    note: "observable".into(),
                },
                CriterionJudgment {
                    index: 2,
                    passed: true,
                    note: "observable".into(),
                },
            ],
            reasoning: "skipped criterion 1".into(),
            judged_at: chrono::Utc::now(),
        };
        match svc.goal_judge(goal.id, missing, owner) {
            Err(KanbanError::InvalidInput(message)) => {
                assert!(message.contains("missing indices [1]"), "{message}");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        let duplicate = GoalVerdict {
            verdict: GoalVerdictValue::Done,
            confidence: 0.9,
            criterion_results: vec![
                CriterionJudgment {
                    index: 0,
                    passed: true,
                    note: "observable".into(),
                },
                CriterionJudgment {
                    index: 0,
                    passed: true,
                    note: "judged twice".into(),
                },
                CriterionJudgment {
                    index: 1,
                    passed: true,
                    note: "observable".into(),
                },
                CriterionJudgment {
                    index: 2,
                    passed: true,
                    note: "observable".into(),
                },
            ],
            reasoning: "double-judged criterion 0".into(),
            judged_at: chrono::Utc::now(),
        };
        assert!(svc.goal_judge(goal.id, duplicate, owner).is_err());
    }

    #[test]
    fn goal_score_brier_and_surfaced_missing_prediction() {
        let svc = make_service();
        let owner = WebID::new();

        // With a prediction: Brier = (p - o)^2.
        let with_pred = svc
            .goal_create("goal a".into(), criteria(2), Some(0.8), None, owner)
            .unwrap();
        let resolved = svc.goal_score(with_pred.id, true, owner).unwrap();
        let resolution = resolved.resolution.expect("resolution recorded");
        assert!(resolution.achieved);
        // (0.8 - 1.0)^2 = 0.04
        assert!((resolution.brier.expect("brier computed") - 0.04).abs() < 1e-9);

        // Without a prediction: brier is None — surfaced, never a fake 0.
        let no_pred = svc
            .goal_create("goal b".into(), criteria(1), None, None, owner)
            .unwrap();
        let resolved2 = svc.goal_score(no_pred.id, false, owner).unwrap();
        let resolution2 = resolved2.resolution.expect("resolution recorded");
        assert!(!resolution2.achieved);
        assert!(
            resolution2.brier.is_none(),
            "no intake prediction → brier must be None, not a synthetic 0"
        );

        // Double-resolution is rejected.
        assert!(svc.goal_score(no_pred.id, true, owner).is_err());
    }

    #[test]
    fn goals_are_ephemeral_not_written_to_the_persistent_store() {
        // Operator ruling 2026-08-29: zed-agent goals are ephemeral — the
        // in-memory goal store serves the session; the curator's memory (fed
        // by the turn-ingestion goal-event path) is the durable vehicle. This
        // pins that goal operations NEVER touch the service's persistent
        // store, so conversational goals leave no kanban-DB clutter.
        let svc = make_service();
        let owner = WebID::new();
        let goal = svc
            .goal_create("ephemeral goal".into(), criteria(2), None, None, owner)
            .unwrap();
        let _ = svc
            .goal_judge(
                goal.id,
                GoalVerdict {
                    verdict: GoalVerdictValue::Done,
                    confidence: 0.9,
                    criterion_results: vec![
                        CriterionJudgment {
                            index: 0,
                            passed: true,
                            note: "observable".into(),
                        },
                        CriterionJudgment {
                            index: 1,
                            passed: true,
                            note: "observable".into(),
                        },
                    ],
                    reasoning: "done".into(),
                    judged_at: chrono::Utc::now(),
                },
                owner,
            )
            .unwrap();

        // The ephemeral store serves the goal.
        assert!(svc.goal_get(goal.id).unwrap().is_some());

        // The persistent store has zero goal h_mems.
        let persistent = svc
            .store
            .query_by_entity("kanban:goal")
            .expect("persistent store query");
        assert!(
            persistent.is_empty(),
            "goals must not be written to the persistent kanban store — \
             conversational goals are ephemeral by design"
        );
    }

    #[test]
    fn goal_list_returns_owner_goals_newest_first() {
        let svc = make_service();
        let owner = WebID::new();
        let other = WebID::new();
        let g1 = svc
            .goal_create("first".into(), criteria(1), None, None, owner)
            .unwrap();
        let _g2 = svc
            .goal_create("second".into(), criteria(1), None, None, owner)
            .unwrap();
        let _g3 = svc
            .goal_create("someone else's".into(), criteria(1), None, None, other)
            .unwrap();

        let goals = svc.goal_list(&owner).unwrap();
        assert_eq!(goals.len(), 2);
        assert_eq!(goals[0].goal_text, "second");
        // Newest first: the second entry is the first-created goal.
        assert_eq!(goals[1].id, g1.id);
        assert!(goals.iter().all(|g| g.owner == owner));
    }
}
