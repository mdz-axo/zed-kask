//! Approved P1–P6 lifecycle obligations at the real file-backed service seam.
use chrono::Utc;
use hkask_mcp_kata_kanban::*;
use hkask_storage::DatabaseDriverTrait;
use hkask_storage::{HMemStore, SqliteDriver};
use hkask_types::WebID;
use std::sync::Arc;

fn open(path: &std::path::Path) -> anyhow::Result<KanbanService> {
    let pool = SqliteDriver::file_pool_with_schema(path.to_str().expect("utf8 test path"))?;
    Ok(KanbanService::new(HMemStore::from_driver(Arc::new(
        SqliteDriver::new(pool),
    ))?))
}
fn source() -> GoalSource {
    GoalSource {
        interaction_id: "user-turn-1".into(),
        session_id: "session-1".into(),
        source: "native-confirmation".into(),
    }
}
fn declaration() -> GoalDeclaration {
    GoalDeclaration {
        goal_text: "The original requirement".into(),
        criteria: vec![VerificationCriterion::new(
            "The original explicit criterion".into(),
        )],
        task_id: None,
        source: source(),
        forecast: None,
        supersedes: None,
    }
}
fn closure(goal: &Goal) -> NativeGoalClosure {
    NativeGoalClosure {
        event_id: "closure-1".into(),
        expected_spec_digest: goal.spec_digest.clone(),
        source: source(),
        occurred_at: Utc::now(),
        choice: HumanClosureChoice::StopUnknown,
        evidence_manifest: vec![],
    }
}
#[test]
fn stop_unknown_survives_reopen_and_delivery_never_deletes_the_goal() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("goals.sqlite");
    let owner = WebID::new();
    let service = open(&path)?;
    let goal = service.goal_create(declaration(), owner)?;
    let closed = service.goal_close_native(goal.id, owner, closure(&goal))?;
    assert!(closed.assessment_pending);
    assert!(closed.scores.is_empty());
    drop(service);
    let reopened = open(&path)?;
    let retained = reopened
        .goal_get(goal.id, owner)?
        .expect("canonical goal survives");
    let projection = retained.projections.last().expect("durable outbox");
    let receipt = GoalProjectionReceipt {
        event_id: projection.event_id.clone(),
        revision: projection.revision,
        payload_digest: projection.payload_digest.clone(),
        receipt_id: "memory-receipt-1".into(),
    };
    reopened.goal_mark_projection_delivered(goal.id, owner, receipt.clone())?;
    reopened.goal_mark_projection_delivered(goal.id, owner, receipt)?;
    drop(reopened);
    let retained = open(&path)?
        .goal_get(goal.id, owner)?
        .expect("receipt cannot erase history");
    assert_eq!(retained.criteria, goal.criteria);
    assert_eq!(retained.spec_digest, goal.spec_digest);
    assert_eq!(
        retained.closure.expect("closure preserved").input.choice,
        HumanClosureChoice::StopUnknown
    );
    assert!(retained.assessment_pending);
    assert!(retained.scores.is_empty());
    Ok(())
}

#[test]
fn unresolved_forecasts_remain_pending_after_assessment_and_delivery() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let mut declaration = declaration();
    declaration.forecast = Some(GoalForecast {
        proposition: "Declared machine condition at horizon".into(),
        probability: 0.8,
        horizon: Utc::now() + chrono::Duration::hours(1),
        resolution_deadline: Utc::now() + chrono::Duration::hours(2),
        execution_policy: "fixed procedure".into(),
        resources: "declared budget".into(),
        resolver: GoalForecastResolver::MachineEvidence {
            source: "machine".into(),
            oracle: "check".into(),
        },
        baseline_probability: 0.5,
        cohort: "same-policy".into(),
        update_policy: ForecastUpdatePolicy::FixedAtDeclaration,
    });
    let goal = service.goal_create(declaration, owner)?;
    service.goal_close_native(goal.id, owner, closure(&goal))?;
    service.goal_claim_assessment(goal.id, owner, lease("attempt-1"))?;
    let goal = service.goal_get(goal.id, owner)?.expect("goal");
    let attempt = goal.assessments.last().expect("attempt");
    let goal = service.goal_complete_assessment(
        goal.id,
        owner,
        AssessmentCompletion {
            attempt_id: "attempt-1".into(),
            input_digest: attempt.input_digest.clone(),
            findings: vec![CriterionFinding {
                criterion_id: "criterion-1".into(),
                status: CriterionEvidenceStatus::Unknown,
                evidence_ids: vec![],
                note: "no evidence yet".into(),
            }],
            limitations: vec!["no observation".into()],
        },
    )?;
    for projection in goal.projections {
        service.goal_mark_projection_delivered(
            goal.id,
            owner,
            GoalProjectionReceipt {
                event_id: projection.event_id,
                revision: projection.revision,
                payload_digest: projection.payload_digest,
                receipt_id: "receipt".into(),
            },
        )?;
    }
    assert!(
        service
            .goal_pending_work(&owner)?
            .iter()
            .any(|g| g.id == goal.id),
        "unresolved forecast must remain discoverable by the restart drainer"
    );
    Ok(())
}
fn lease(id: &str) -> AssessmentLeaseRequest {
    AssessmentLeaseRequest {
        attempt_id: id.into(),
        assessor_id: "independent-assessor".into(),
        model_version: "recorded-provider-version".into(),
        tool_version: "recorded-tool-version".into(),
        lease_until: Utc::now() + chrono::Duration::minutes(5),
    }
}

#[test]
fn human_corrections_append_without_overwriting_original_closure() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    let original = closure(&goal);
    service.goal_close_native(goal.id, owner, original.clone())?;
    let correction = NativeGoalOutcomeCorrection {
        event_id: "correction-1".into(),
        expected_spec_digest: goal.spec_digest.clone(),
        source: source(),
        occurred_at: Utc::now(),
        supersedes_event_id: original.event_id.clone(),
        outcome: Some(false),
        reason: "Human corrects the outcome; stop remains stop".into(),
    };
    service.goal_correct_human_outcome(goal.id, owner, correction.clone())?;
    service.goal_correct_human_outcome(goal.id, owner, correction.clone())?;
    let mut conflict = correction;
    conflict.outcome = Some(true);
    assert!(
        service
            .goal_correct_human_outcome(goal.id, owner, conflict)
            .is_err()
    );
    let retained = service.goal_get(goal.id, owner)?.expect("goal");
    assert_eq!(retained.closure.expect("original").input, original);
    assert_eq!(retained.human_corrections.len(), 1);
    assert!(
        retained.scores.is_empty(),
        "correction does not create a forecast"
    );
    Ok(())
}

fn evidence() -> GoalEvidence {
    GoalEvidence {
        id: "evidence-1".into(),
        source: "machine".into(),
        locator: "file:///retained/check-report".into(),
        digest: "sha256:actual-report-digest".into(),
        reliability: "captured machine output".into(),
        oracle: "check".into(),
        limitation: "Only the stated criterion was checked".into(),
    }
}
fn completion(goal: &Goal, status: CriterionEvidenceStatus) -> AssessmentCompletion {
    let attempt = goal.assessments.last().expect("claimed attempt");
    AssessmentCompletion {
        attempt_id: attempt.lease.attempt_id.clone(),
        input_digest: attempt.input_digest.clone(),
        findings: vec![CriterionFinding {
            criterion_id: "criterion-1".into(),
            status,
            evidence_ids: vec!["evidence-1".into()],
            note: "Original criterion checked by retained oracle".into(),
        }],
        limitations: vec!["No independent ground truth beyond this oracle".into()],
    }
}
fn close_with_evidence(
    service: &KanbanService,
    goal: &Goal,
    owner: WebID,
    choice: HumanClosureChoice,
) -> anyhow::Result<Goal> {
    let mut input = closure(goal);
    input.choice = choice;
    input.evidence_manifest = vec![evidence()];
    Ok(service.goal_close_native(goal.id, owner, input)?)
}
fn assessed(
    service: &KanbanService,
    id: hkask_types::id::GoalID,
    owner: WebID,
    status: CriterionEvidenceStatus,
) -> anyhow::Result<Goal> {
    service.goal_claim_assessment(id, owner, lease("attempt-1"))?;
    let goal = service.goal_get(id, owner)?.expect("goal");
    Ok(service.goal_complete_assessment(id, owner, completion(&goal, status))?)
}
fn forecast(resolver: GoalForecastResolver) -> GoalForecast {
    GoalForecast {
        proposition: "Frozen condition at the declared horizon".into(),
        probability: 0.8,
        horizon: Utc::now() + chrono::Duration::milliseconds(100),
        resolution_deadline: Utc::now() + chrono::Duration::hours(1),
        execution_policy: "fixed procedure".into(),
        resources: "fixed budget".into(),
        resolver,
        baseline_probability: 0.5,
        cohort: "same-case-policy-horizon".into(),
        update_policy: ForecastUpdatePolicy::FixedAtDeclaration,
    }
}
fn wait_until(time: chrono::DateTime<Utc>) {
    if let Ok(duration) = (time - Utc::now()).to_std() {
        std::thread::sleep(duration);
    }
}
#[test]
fn six_human_choices_preserve_stop_and_acceptance_without_implicit_forecasts() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    for choice in [
        HumanClosureChoice::AccomplishedAchieved,
        HumanClosureChoice::AccomplishedNotAchieved,
        HumanClosureChoice::AccomplishedUnknown,
        HumanClosureChoice::StopAchieved,
        HumanClosureChoice::StopNotAchieved,
        HumanClosureChoice::StopUnknown,
    ] {
        let goal = service.goal_create(declaration(), owner)?;
        let closed = close_with_evidence(&service, &goal, owner, choice)?;
        let assessed = assessed(
            &service,
            goal.id,
            owner,
            CriterionEvidenceStatus::Contradicted,
        )?;
        assert_eq!(
            closed.closure, assessed.closure,
            "assessor cannot relabel human closure"
        );
        assert_eq!(assessed.closure.expect("closure").input.choice, choice);
        assert!(assessed.scores.is_empty());
        assert_eq!(assessed.forecast_status, ForecastStatus::NotDeclared);
    }
    Ok(())
}
#[test]
fn closure_replay_conflicts_spec_drift_and_foreign_owners_leave_history_unchanged()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    let input = closure(&goal);
    assert!(
        service
            .goal_close_native(goal.id, WebID::new(), input.clone())
            .is_err()
    );
    let mut wrong = input.clone();
    wrong.expected_spec_digest = "rewritten-spec".into();
    assert!(service.goal_close_native(goal.id, owner, wrong).is_err());
    let mut wrong = input.clone();
    wrong.source.interaction_id.clear();
    assert!(service.goal_close_native(goal.id, owner, wrong).is_err());
    assert_eq!(service.goal_get(goal.id, owner)?.expect("goal"), goal);
    let closed = service.goal_close_native(goal.id, owner, input.clone())?;
    assert_eq!(
        service.goal_close_native(goal.id, owner, input.clone())?,
        closed
    );
    let mut conflict = input;
    conflict.choice = HumanClosureChoice::AccomplishedAchieved;
    assert!(service.goal_close_native(goal.id, owner, conflict).is_err());
    assert_eq!(service.goal_get(goal.id, owner)?.expect("goal"), closed);
    Ok(())
}
#[test]
fn progress_requires_exact_coverage_and_done_cannot_contradict_failed_criteria()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    let mut verdict = GoalVerdict {
        verdict: GoalVerdictValue::Done,
        confidence: 0.9,
        criterion_results: vec![CriterionJudgment {
            index: 0,
            passed: false,
            note: "check failed".into(),
        }],
        reasoning: "worker claim, not human closure".into(),
        judged_at: Utc::now(),
    };
    assert!(service.goal_judge(goal.id, verdict.clone(), owner).is_err());
    verdict.criterion_results[0].passed = true;
    let mut missing = verdict.clone();
    missing.criterion_results.clear();
    assert!(service.goal_judge(goal.id, missing, owner).is_err());
    let mut duplicate = verdict.clone();
    duplicate
        .criterion_results
        .push(duplicate.criterion_results[0].clone());
    assert!(service.goal_judge(goal.id, duplicate, owner).is_err());
    let mut range = verdict.clone();
    range.criterion_results[0].index = 1;
    assert!(service.goal_judge(goal.id, range, owner).is_err());
    assert!(
        service
            .goal_judge(goal.id, verdict.clone(), WebID::new())
            .is_err()
    );
    let judged = service.goal_judge(goal.id, verdict, owner)?;
    assert!(
        judged.closure.is_none(),
        "worker done is never human closure"
    );
    assert_eq!(judged.verdicts.len(), 1);
    Ok(())
}
#[test]
fn assessor_input_excludes_labels_and_completion_requires_the_frozen_manifest() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    let verdict = GoalVerdict {
        verdict: GoalVerdictValue::Done,
        confidence: 0.88,
        criterion_results: vec![CriterionJudgment {
            index: 0,
            passed: true,
            note: "WORKER_LABEL_SECRET".into(),
        }],
        reasoning: "WORKER_LABEL_SECRET".into(),
        judged_at: Utc::now(),
    };
    service.goal_judge(goal.id, verdict, owner)?;
    close_with_evidence(
        &service,
        &goal,
        owner,
        HumanClosureChoice::AccomplishedAchieved,
    )?;
    let claim = lease("attempt-1");
    let input = service.goal_claim_assessment(goal.id, owner, claim.clone())?;
    assert_eq!(service.goal_claim_assessment(goal.id, owner, claim)?, input);
    let wire = serde_json::to_value(&input)?;
    for forbidden in [
        "verdicts",
        "closure",
        "forecast",
        "scores",
        "owner",
        "human_corrections",
    ] {
        assert!(wire.get(forbidden).is_none());
    }
    assert!(!wire.to_string().contains("WORKER_LABEL_SECRET"));
    assert_eq!(input.evidence_manifest, vec![evidence()]);
    assert!(
        service
            .goal_claim_assessment(goal.id, owner, lease("attempt-2"))
            .is_err()
    );
    let goal = service.goal_get(goal.id, owner)?.expect("goal");
    let good = completion(&goal, CriterionEvidenceStatus::Contradicted);
    let mut missing = good.clone();
    missing.findings.clear();
    assert!(
        service
            .goal_complete_assessment(goal.id, owner, missing)
            .is_err()
    );
    let mut duplicate = good.clone();
    duplicate.findings.push(duplicate.findings[0].clone());
    assert!(
        service
            .goal_complete_assessment(goal.id, owner, duplicate)
            .is_err()
    );
    let mut wrong = good.clone();
    wrong.input_digest = "different-input".into();
    assert!(
        service
            .goal_complete_assessment(goal.id, owner, wrong)
            .is_err()
    );
    let mut wrong = good.clone();
    wrong.findings[0].evidence_ids = vec!["invented-evidence".into()];
    assert!(
        service
            .goal_complete_assessment(goal.id, owner, wrong)
            .is_err()
    );
    let mut unsupported = good.clone();
    unsupported.findings[0].evidence_ids.clear();
    assert!(
        service
            .goal_complete_assessment(goal.id, owner, unsupported)
            .is_err()
    );
    let completed = service.goal_complete_assessment(goal.id, owner, good.clone())?;
    assert_eq!(
        service.goal_complete_assessment(goal.id, owner, good)?,
        completed
    );
    assert!(!completed.assessment_pending);
    assert_eq!(
        completed.closure.expect("human retained").input.choice,
        HumanClosureChoice::AccomplishedAchieved
    );
    Ok(())
}
#[test]
fn failed_assessment_survives_reopen_and_can_be_retried_without_reopening_work()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("goals.sqlite");
    let owner = WebID::new();
    let service = open(&path)?;
    let goal = service.goal_create(declaration(), owner)?;
    close_with_evidence(&service, &goal, owner, HumanClosureChoice::StopUnknown)?;
    service.goal_claim_assessment(goal.id, owner, lease("failed-attempt"))?;
    let retained = service.goal_get(goal.id, owner)?.expect("goal");
    let failure = AssessmentFailure {
        attempt_id: "failed-attempt".into(),
        input_digest: retained.assessments[0].input_digest.clone(),
        reason: "provider unavailable".into(),
    };
    let failed = service.goal_fail_assessment(goal.id, owner, failure.clone())?;
    assert_eq!(
        service.goal_fail_assessment(goal.id, owner, failure)?,
        failed
    );
    drop(service);
    let service = open(&path)?;
    assert!(
        service
            .goal_pending_work(&owner)?
            .iter()
            .any(|g| g.id == goal.id)
    );
    service.goal_claim_assessment(goal.id, owner, lease("retry"))?;
    let retained = service.goal_get(goal.id, owner)?.expect("goal");
    assert_eq!(
        retained.assessments[0]
            .failure
            .as_ref()
            .expect("failure retained")
            .reason,
        "provider unavailable"
    );
    service.goal_complete_assessment(
        goal.id,
        owner,
        completion(&retained, CriterionEvidenceStatus::Supported),
    )?;
    let retained = service.goal_get(goal.id, owner)?.expect("goal");
    assert!(!retained.assessment_pending);
    assert_eq!(
        retained.closure.expect("closed").input.choice,
        HumanClosureChoice::StopUnknown
    );
    Ok(())
}
#[test]
fn expired_assessor_lease_is_preserved_as_failure_and_old_completion_is_rejected()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    close_with_evidence(&service, &goal, owner, HumanClosureChoice::StopUnknown)?;
    let mut short = lease("expired");
    short.lease_until = Utc::now() + chrono::Duration::milliseconds(100);
    service.goal_claim_assessment(goal.id, owner, short.clone())?;
    let old = completion(
        &service.goal_get(goal.id, owner)?.expect("goal"),
        CriterionEvidenceStatus::Supported,
    );
    wait_until(short.lease_until);
    service.goal_claim_assessment(goal.id, owner, lease("replacement"))?;
    assert!(
        service
            .goal_complete_assessment(goal.id, owner, old)
            .is_err()
    );
    let retained = service.goal_get(goal.id, owner)?.expect("goal");
    assert!(
        retained.assessments[0]
            .failure
            .as_ref()
            .expect("expired failure")
            .reason
            .contains("expired")
    );
    assert_eq!(retained.assessments.len(), 2);
    Ok(())
}
#[test]
fn forged_projection_receipt_is_rejected_and_digest_binds_original_payload() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    let projection = &goal.projections[0];
    assert_eq!(
        projection.payload_digest,
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&projection.payload)?)
        )
    );
    assert_eq!(
        projection.payload["criteria"][0]["description"],
        "The original explicit criterion"
    );
    let mut receipt = GoalProjectionReceipt {
        event_id: projection.event_id.clone(),
        revision: projection.revision,
        payload_digest: "forged".into(),
        receipt_id: "receipt-1".into(),
    };
    assert!(
        service
            .goal_mark_projection_delivered(goal.id, owner, receipt.clone())
            .is_err()
    );
    receipt.payload_digest = projection.payload_digest.clone();
    receipt.revision += 1;
    assert!(
        service
            .goal_mark_projection_delivered(goal.id, owner, receipt.clone())
            .is_err()
    );
    receipt.revision = projection.revision;
    service.goal_mark_projection_delivered(goal.id, owner, receipt.clone())?;
    receipt.receipt_id = "conflicting-receipt".into();
    assert!(
        service
            .goal_mark_projection_delivered(goal.id, owner, receipt)
            .is_err()
    );
    assert_eq!(
        service
            .goal_get(goal.id, owner)?
            .expect("canonical history")
            .projections
            .len(),
        1,
        "ack must not publish another ack"
    );
    Ok(())
}

#[test]
fn human_forecast_scores_only_after_assessor_attempt_and_keeps_contradiction() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let mut declared = declaration();
    declared.forecast = Some(forecast(GoalForecastResolver::HumanAcceptance));
    let goal = service.goal_create(declared, owner)?;
    wait_until(goal.forecast.as_ref().expect("forecast").horizon);
    let closed = close_with_evidence(
        &service,
        &goal,
        owner,
        HumanClosureChoice::AccomplishedAchieved,
    )?;
    assert!(closed.scores.is_empty());
    let goal = assessed(
        &service,
        goal.id,
        owner,
        CriterionEvidenceStatus::Contradicted,
    )?;
    let score = goal.scores.last().expect("declared human resolver");
    assert!((score.brier - 0.04).abs() < 1e-12);
    assert_eq!(score.baseline_brier, 0.25);
    assert_eq!(goal.forecast_status, ForecastStatus::Resolved);
    assert_eq!(
        goal.assessments[0]
            .completion
            .as_ref()
            .expect("assessment")
            .findings[0]
            .status,
        CriterionEvidenceStatus::Contradicted
    );
    let correction = NativeGoalOutcomeCorrection {
        event_id: "human-correction".into(),
        expected_spec_digest: goal.spec_digest.clone(),
        source: source(),
        occurred_at: Utc::now(),
        supersedes_event_id: "closure-1".into(),
        outcome: Some(false),
        reason: "Human corrects actual acceptance".into(),
    };
    let corrected = service.goal_correct_human_outcome(goal.id, owner, correction.clone())?;
    assert_eq!(corrected.scores.len(), 2);
    assert!((corrected.scores[1].brier - 0.64).abs() < 1e-12);
    assert_eq!(corrected.scores[1].supersedes_version, Some(1));
    assert_eq!(
        service
            .goal_correct_human_outcome(goal.id, owner, correction)?
            .scores
            .len(),
        2
    );
    Ok(())
}
fn observation(
    id: &str,
    outcome: Option<bool>,
    correction_of: Option<&str>,
) -> ForecastObservation {
    ForecastObservation {
        event_id: id.into(),
        source: "machine".into(),
        oracle: "check".into(),
        occurred_at: Utc::now(),
        outcome,
        evidence_ids: vec!["evidence-1".into()],
        correction_of: correction_of.map(str::to_string),
        reason: "Actual source observation retained by host".into(),
    }
}
#[test]
fn machine_resolver_rejects_source_substitution_and_retains_unknown_disputed_corrections()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("goals.sqlite");
    let service = open(&path)?;
    let owner = WebID::new();
    let mut declared = declaration();
    declared.forecast = Some(forecast(GoalForecastResolver::MachineEvidence {
        source: "machine".into(),
        oracle: "check".into(),
    }));
    let goal = service.goal_create(declared, owner)?;
    wait_until(goal.forecast.as_ref().expect("forecast").horizon);
    close_with_evidence(&service, &goal, owner, HumanClosureChoice::StopNotAchieved)?;
    let first = observation("actual-1", Some(true), None);
    let mut wrong = first.clone();
    wrong.source = "assessor-opinion".into();
    assert!(
        service
            .goal_record_forecast_observation(goal.id, owner, wrong)
            .is_err()
    );
    let mut wrong = first.clone();
    wrong.evidence_ids = vec!["not-retained".into()];
    assert!(
        service
            .goal_record_forecast_observation(goal.id, owner, wrong)
            .is_err()
    );
    let mut wrong = first.clone();
    wrong.occurred_at = goal.created_at;
    assert!(
        service
            .goal_record_forecast_observation(goal.id, owner, wrong)
            .is_err()
    );
    assert!(
        service
            .goal_record_forecast_observation(goal.id, owner, first.clone())?
            .scores
            .is_empty(),
        "source outcome alone does not bypass assessment attempt"
    );
    service.goal_claim_assessment(goal.id, owner, lease("attempt-1"))?;
    let retained = service.goal_get(goal.id, owner)?.expect("goal");
    let failed = service.goal_fail_assessment(
        goal.id,
        owner,
        AssessmentFailure {
            attempt_id: "attempt-1".into(),
            input_digest: retained.assessments[0].input_digest.clone(),
            reason: "provider failed; not a pass".into(),
        },
    )?;
    assert!((failed.scores[0].brier - 0.04).abs() < 1e-12);
    assert!(failed.assessment_pending);
    assert_eq!(
        service
            .goal_record_forecast_observation(goal.id, owner, first.clone())?
            .scores
            .len(),
        1
    );
    let mut conflict = first;
    conflict.outcome = Some(false);
    assert!(
        service
            .goal_record_forecast_observation(goal.id, owner, conflict)
            .is_err()
    );
    let disputed = service.goal_record_forecast_observation(
        goal.id,
        owner,
        observation("contradictory-source", Some(false), None),
    )?;
    assert_eq!(disputed.forecast_status, ForecastStatus::Disputed);
    assert_eq!(
        disputed.scores.len(),
        1,
        "disagreement does not invent a new score"
    );
    let corrected = service.goal_record_forecast_observation(
        goal.id,
        owner,
        observation("corrected-1", Some(false), Some("actual-1")),
    )?;
    assert_eq!(corrected.forecast_status, ForecastStatus::Resolved);
    assert!((corrected.scores.last().expect("score correction").brier - 0.64).abs() < 1e-12);
    let unknown = service.goal_record_forecast_observation(
        goal.id,
        owner,
        observation("unknown", None, Some("corrected-1")),
    )?;
    assert_eq!(unknown.forecast_status, ForecastStatus::Unknown);
    drop(service);
    let retained = open(&path)?
        .goal_get(goal.id, owner)?
        .expect("full history survives");
    assert_eq!(retained.observations.len(), 4);
    assert_eq!(retained.forecast_status, ForecastStatus::Unknown);
    assert_eq!(
        retained.closure.expect("human preserved").input.choice,
        HumanClosureChoice::StopNotAchieved
    );
    Ok(())
}
#[test]
fn forecast_admission_rejects_late_nonfinite_and_incomplete_declarations() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    for probability in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut declared = declaration();
        let mut f = forecast(GoalForecastResolver::HumanAcceptance);
        f.probability = probability;
        declared.forecast = Some(f);
        assert!(service.goal_create(declared, owner).is_err());
    }
    let mut declared = declaration();
    let mut f = forecast(GoalForecastResolver::HumanAcceptance);
    f.horizon = Utc::now() - chrono::Duration::hours(1);
    declared.forecast = Some(f);
    assert!(service.goal_create(declared, owner).is_err());
    let mut declared = declaration();
    let mut f = forecast(GoalForecastResolver::HumanAcceptance);
    f.execution_policy.clear();
    declared.forecast = Some(f);
    assert!(service.goal_create(declared, owner).is_err());
    let mut declared = declaration();
    declared.criteria[0].description = "  ".into();
    assert!(service.goal_create(declared, owner).is_err());
    assert!(service.goal_list(&owner)?.is_empty());
    Ok(())
}
#[test]
fn matched_zero_baseline_loss_has_no_synthetic_skill() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let mut declared = declaration();
    let mut f = forecast(GoalForecastResolver::HumanAcceptance);
    f.baseline_probability = 1.0;
    declared.forecast = Some(f);
    let goal = service.goal_create(declared, owner)?;
    wait_until(goal.forecast.as_ref().expect("forecast").horizon);
    close_with_evidence(&service, &goal, owner, HumanClosureChoice::StopAchieved)?;
    let goal = assessed(&service, goal.id, owner, CriterionEvidenceStatus::Unknown)?;
    let score = goal.scores.last().expect("score");
    assert_eq!(score.baseline_brier, 0.0);
    assert_eq!(score.skill, None);
    Ok(())
}
#[test]
fn legacy_archive_preserves_raw_data_but_never_reconstructs_eligibility() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("goals.sqlite");
    let owner = WebID::new();
    let id = hkask_types::id::GoalID::new();
    let pool = SqliteDriver::file_pool_with_schema(path.to_str().expect("path"))?;
    let store = HMemStore::from_driver(Arc::new(SqliteDriver::new(pool)))?;
    let raw = serde_json::json!({ "id": id, "goal_text": "historical original", "criteria": [{"description":"historical criterion", "llm_prompt":null}], "task_id": null, "prediction": 0.8, "verdicts": [], "resolution": {"achieved":true, "brier":0.04, "resolved_at":Utc::now()}, "owner":owner, "created_at":Utc::now(), "updated_at":Utc::now(), "unknown_historical_field": {"preserve":"verbatim"} });
    store.insert(&hkask_storage::HMem::new(
        "kanban:goal",
        "historically-drifted-key",
        raw.clone(),
        owner,
    ))?;
    let service = KanbanService::new(store);
    assert_eq!(service.goal_archive_legacy(owner)?, 1);
    assert_eq!(service.goal_archive_legacy(owner)?, 0);
    drop(service);
    let service = open(&path)?;
    let archived = service
        .goal_get(id, owner)?
        .expect("legacy remains visible");
    assert_eq!(archived.legacy.as_ref().expect("explicit archive").raw, raw);
    assert_eq!(archived.forecast_status, ForecastStatus::LegacyUnscored);
    assert!(archived.forecast.is_none());
    assert!(archived.scores.is_empty());
    assert!(
        service
            .goal_close_native(id, owner, closure(&archived))
            .is_err()
    );
    let mut successor = declaration();
    successor.supersedes = Some(id);
    let next = service.goal_create(successor, owner)?;
    assert_eq!(next.supersedes, Some(id));
    assert_eq!(service.goal_list(&owner)?.len(), 2);
    Ok(())
}
#[test]
fn concurrent_closures_and_progress_never_lose_successful_updates() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    let barrier = std::sync::Barrier::new(3);
    std::thread::scope(|scope| -> anyhow::Result<()> {
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let service = &service;
                let barrier = &barrier;
                scope.spawn(move || {
                    let verdict = GoalVerdict {
                        verdict: GoalVerdictValue::Continue,
                        confidence: 0.5,
                        criterion_results: vec![CriterionJudgment {
                            index: 0,
                            passed: false,
                            note: "not yet satisfied".into(),
                        }],
                        reasoning: format!("worker-{i}"),
                        judged_at: Utc::now(),
                    };
                    barrier.wait();
                    service.goal_judge(goal.id, verdict, owner)
                })
            })
            .collect();
        barrier.wait();
        for handle in handles {
            handle.join().expect("thread")?;
        }
        Ok(())
    })?;
    assert_eq!(
        service
            .goal_get(goal.id, owner)?
            .expect("goal")
            .verdicts
            .len(),
        2
    );
    let a = closure(&goal);
    let mut b = a.clone();
    b.event_id = "different-event".into();
    b.choice = HumanClosureChoice::AccomplishedAchieved;
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| service.goal_close_native(goal.id, owner, a));
        let b = scope.spawn(|| service.goal_close_native(goal.id, owner, b));
        [a.join().expect("thread"), b.join().expect("thread")]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let retained = service.goal_get(goal.id, owner)?.expect("goal");
    assert!(retained.assessment_pending);
    assert_eq!(retained.verdicts.len(), 2);
    assert_eq!(
        retained.projections.len(),
        4,
        "create, two judges, one closure"
    );
    Ok(())
}
#[test]
fn failed_atomic_closure_does_not_leave_half_an_assessment_or_outbox() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("goals.sqlite");
    let pool = SqliteDriver::file_pool_with_schema(path.to_str().expect("path"))?;
    let driver = Arc::new(SqliteDriver::new(pool));
    let service = KanbanService::new(HMemStore::from_driver(driver.clone())?);
    let owner = WebID::new();
    let goal = service.goal_create(declaration(), owner)?;
    driver.execute_batch("CREATE TRIGGER reject_closure BEFORE UPDATE ON hmems WHEN NEW.entity = 'kanban:goal' BEGIN SELECT RAISE(FAIL, 'forced closure failure'); END;")?;
    let input = closure(&goal);
    assert!(
        service
            .goal_close_native(goal.id, owner, input.clone())
            .is_err()
    );
    assert_eq!(
        service.goal_get(goal.id, owner)?.expect("unchanged goal"),
        goal
    );
    driver.execute_batch("DROP TRIGGER reject_closure;")?;
    assert!(
        service
            .goal_close_native(goal.id, owner, input)?
            .assessment_pending
    );
    Ok(())
}

#[test]
fn late_human_correction_rescores_the_original_in_window_outcome() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let mut declared = declaration();
    let mut f = forecast(GoalForecastResolver::HumanAcceptance);
    f.resolution_deadline = f.horizon + chrono::Duration::milliseconds(300);
    declared.forecast = Some(f);
    let goal = service.goal_create(declared, owner)?;
    let forecast = goal.forecast.as_ref().expect("forecast");
    wait_until(forecast.horizon);
    close_with_evidence(
        &service,
        &goal,
        owner,
        HumanClosureChoice::AccomplishedAchieved,
    )?;
    assessed(&service, goal.id, owner, CriterionEvidenceStatus::Supported)?;
    wait_until(forecast.resolution_deadline);
    let corrected = service.goal_correct_human_outcome(
        goal.id,
        owner,
        NativeGoalOutcomeCorrection {
            event_id: "late-correction".into(),
            expected_spec_digest: goal.spec_digest.clone(),
            source: source(),
            occurred_at: Utc::now(),
            supersedes_event_id: "closure-1".into(),
            outcome: Some(false),
            reason: "Correction of the original human outcome, not a new target".into(),
        },
    )?;
    assert_eq!(corrected.forecast_status, ForecastStatus::Resolved);
    assert!((corrected.scores.last().expect("corrected score").brier - 0.64).abs() < 1e-12);
    Ok(())
}

#[test]
fn forecast_deadline_refresh_records_expiry_without_fabricating_failure() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    let mut declared = declaration();
    let mut forecast = forecast(GoalForecastResolver::MachineEvidence {
        source: "machine".into(),
        oracle: "check".into(),
    });
    forecast.resolution_deadline = forecast.horizon;
    let deadline = forecast.resolution_deadline;
    declared.forecast = Some(forecast);
    let goal = service.goal_create(declared, owner)?;
    close_with_evidence(&service, &goal, owner, HumanClosureChoice::StopNotAchieved)?;
    assessed(&service, goal.id, owner, CriterionEvidenceStatus::Unknown)?;
    wait_until(deadline);
    let refreshed = service.goal_refresh_forecast(goal.id, owner)?;
    assert_eq!(refreshed.forecast_status, ForecastStatus::DeadlineElapsed);
    assert!(refreshed.scores.is_empty());
    assert!(refreshed.observations.is_empty());
    assert_eq!(
        service.goal_refresh_forecast(goal.id, owner)?,
        refreshed,
        "deadline reconciliation is idempotent"
    );
    Ok(())
}

#[test]
fn explicit_criteria_are_bounded_and_assessment_cannot_precede_closure() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let service = open(&dir.path().join("goals.sqlite"))?;
    let owner = WebID::new();
    for count in [0, 5] {
        let mut declared = declaration();
        declared.criteria = (0..count)
            .map(|i| VerificationCriterion::new(format!("explicit criterion {i}")))
            .collect();
        assert!(service.goal_create(declared, owner).is_err());
    }
    let mut declared = declaration();
    declared.criteria = (0..4)
        .map(|i| VerificationCriterion::new(format!("explicit criterion {i}")))
        .collect();
    let goal = service.goal_create(declared, owner)?;
    assert_eq!(
        goal.criterion_ids,
        ["criterion-1", "criterion-2", "criterion-3", "criterion-4"]
    );
    assert!(
        service
            .goal_claim_assessment(goal.id, owner, lease("premature"))
            .is_err()
    );
    assert_eq!(service.goal_get(goal.id, owner)?.expect("unchanged"), goal);
    let mut revised = declaration();
    revised.supersedes = Some(goal.id);
    let successor = service.goal_create(revised, owner)?;
    assert_ne!(successor.spec_digest, goal.spec_digest);
    assert_eq!(
        service.goal_get(goal.id, owner)?.expect("original remains"),
        goal
    );
    assert!(service.goal_get(goal.id, WebID::new())?.is_none());
    Ok(())
}
