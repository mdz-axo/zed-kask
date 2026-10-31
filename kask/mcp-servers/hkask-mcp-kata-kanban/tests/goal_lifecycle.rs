//! Functional lifecycle tests over separate connections to the canonical store.
use chrono::{DateTime, Duration, Utc};
use hkask_mcp_kata_kanban::*;
use hkask_storage::{HMem, HMemStore, SqliteDriver};
use hkask_types::WebID;
use std::sync::{Arc, Mutex};

fn open(path: &std::path::Path, clock: &Arc<Mutex<DateTime<Utc>>>) -> KanbanService {
    let pool = SqliteDriver::file_pool_with_schema(path.to_str().expect("path")).expect("pool");
    let clock = clock.clone();
    KanbanService::new(HMemStore::from_driver(Arc::new(SqliteDriver::new(pool))).expect("store"))
        .with_goal_clock(Arc::new(move || *clock.lock().expect("clock")))
}
fn declaration() -> GoalDeclaration {
    GoalDeclaration {
        goal_text: "The feature is enabled".into(),
        criteria: vec![VerificationCriterion::new(
            "The file enables the feature".into(),
        )],
        task_id: None,
        source: GoalSource {
            interaction_id: "original-user-turn".into(),
            session_id: "session".into(),
            source: "human-specification".into(),
        },
        forecast: None,
        supersedes: None,
        trust_clause: None,
    }
}
fn close(goal: &Goal, now: DateTime<Utc>, choice: HumanClosureChoice) -> NativeGoalClosure {
    NativeGoalClosure {
        event_id: "native-choice".into(),
        expected_spec_digest: goal.spec_digest.clone(),
        source: declaration().source,
        occurred_at: now,
        choice,
        artifact_access: Some(LocalArtifactAccessApproval::BoundedUtf8V1),
        evidence_manifest: vec![GoalEvidence {
            id: "file".into(),
            source: "untrusted worker".into(),
            locator: "/evidence".into(),
            digest: "4c34bb5402a70378b57765e0226203d3879a1caf74675e74634f11172ea6ca98".into(),
            reliability: "claim".into(),
            oracle: "claim".into(),
            limitation: "file bytes only".into(),
        }],
    }
}
fn capture(closure: &NativeGoalClosure) -> Result<Vec<GoalArtifactSnapshot>, String> {
    Ok(vec![GoalArtifactSnapshot {
        evidence_id: "file".into(),
        observed_at: closure.occurred_at,
        outcome: GoalArtifactReadOutcome::Fetched {
            source: "/evidence".into(),
            sha256: closure.evidence_manifest[0].digest.clone(),
            byte_count: 23,
            content: "feature_status=enabled\n".into(),
        },
    }])
}
fn reply(input: GoalAssessmentInput, status: CriterionEvidenceStatus) -> AssessmentResult {
    AssessmentResult {
        findings: input
            .criteria
            .into_iter()
            .map(|c| CriterionFinding {
                criterion_id: c.id,
                status: status.clone(),
                evidence_ids: vec!["file".into()],
                note: "Only fetched file contents were checked".into(),
            })
            .collect(),
        limitations: vec!["No execution or producer authenticity established".into()],
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    clock: Arc<Mutex<DateTime<Utc>>>,
    owner: WebID,
    service: KanbanService,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("directory");
        let clock = Arc::new(Mutex::new(Utc::now()));
        let service = open(&dir.path().join("goals.db"), &clock);
        Self {
            dir,
            clock,
            owner: WebID::new(),
            service,
        }
    }
    fn now(&self) -> DateTime<Utc> {
        *self.clock.lock().expect("clock")
    }
    fn advance(&self, seconds: i64) {
        *self.clock.lock().expect("clock") += Duration::seconds(seconds);
    }
    fn reopen(&self) -> KanbanService {
        open(&self.dir.path().join("goals.db"), &self.clock)
    }
    fn store(&self) -> HMemStore {
        let pool = SqliteDriver::file_pool_with_schema(
            self.dir.path().join("goals.db").to_str().expect("path"),
        )
        .expect("pool");
        HMemStore::from_driver(Arc::new(SqliteDriver::new(pool))).expect("store")
    }
    fn goal(&self, id: hkask_types::GoalID) -> Goal {
        self.service
            .goal_get(id, self.owner)
            .expect("get")
            .expect("goal")
    }
}
#[tokio::test]
async fn unknown_stop_replay_and_restart_keep_history_without_forecast_or_model() {
    let f = Fixture::new();
    let goal = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("create");
    let mut input = close(&goal, f.now(), HumanClosureChoice::StopUnknown);
    input.evidence_manifest.clear();
    f.service
        .goal_close_native(goal.id, f.owner, input.clone())
        .expect("close");
    f.service
        .goal_close_native(goal.id, f.owner, input.clone())
        .expect("exact replay");
    input.choice = HumanClosureChoice::StopAchieved;
    assert!(
        f.service
            .goal_close_native(goal.id, f.owner, input)
            .is_err()
    );
    f.reopen()
        .goal_assess(
            goal.id,
            f.owner,
            |_| Ok(vec![]),
            |_| async { panic!("no evidence must not call inference") },
        )
        .await
        .expect("unknown assessment");
    let retained = f
        .reopen()
        .goal_get(goal.id, f.owner)
        .expect("get")
        .expect("retained");
    assert_eq!(
        retained.closure.expect("closure").input.choice,
        HumanClosureChoice::StopUnknown
    );
    assert_eq!(
        retained.assessments[0]
            .result
            .as_ref()
            .expect("result")
            .as_ref()
            .expect("unknown")
            .findings[0]
            .status,
        CriterionEvidenceStatus::Unknown
    );
    assert!(retained.forecast.is_none() && retained.scores.is_empty());
}
#[tokio::test]
async fn human_forecast_disagreement_corrections_and_original_time_are_separate() {
    for (choice, status, initial) in [
        (
            HumanClosureChoice::AccomplishedAchieved,
            CriterionEvidenceStatus::Contradicted,
            true,
        ),
        (
            HumanClosureChoice::AccomplishedNotAchieved,
            CriterionEvidenceStatus::Supported,
            false,
        ),
    ] {
        let f = Fixture::new();
        let mut spec = declaration();
        spec.forecast = Some(GoalForecast {
            proposition: "The human accepts the feature".into(),
            probability: 0.8,
            baseline_probability: 0.0,
            horizon: f.now() + Duration::seconds(1),
            resolution_deadline: f.now() + Duration::seconds(10),
            execution_policy: "this task".into(),
            resources: "this session".into(),
            cohort: "features".into(),
            resolver: GoalForecastResolver::HumanAcceptance {},
            update_policy: ForecastUpdatePolicy::FixedAtDeclaration,
        });
        let goal = f
            .service
            .goal_create(spec, f.owner)
            .expect("prospective forecast");
        f.advance(2);
        f.service
            .goal_close_native(goal.id, f.owner, close(&goal, f.now(), choice))
            .expect("close");
        f.service
            .goal_assess(goal.id, f.owner, capture, |input| async {
                Ok(reply(input, status))
            })
            .await
            .expect("assessment");
        let assessed = f.goal(goal.id);
        assert_eq!(
            assessed.scores[0].outcome, initial,
            "assessor is not the human resolver"
        );
        if !initial {
            assert!(
                assessed.scores[0].skill.is_none(),
                "zero baseline loss has undefined skill"
            );
        }
        f.advance(20);
        let correction = NativeGoalOutcomeCorrection {
            event_id: "corrected".into(),
            expected_spec_digest: goal.spec_digest.clone(),
            source: declaration().source,
            occurred_at: f.now(),
            supersedes_event_id: "native-choice".into(),
            outcome: Some(!initial),
            reason: "Human corrects original outcome".into(),
        };
        f.service
            .goal_correct_human_outcome(goal.id, f.owner, correction.clone())
            .expect("correction after deadline");
        f.service
            .goal_correct_human_outcome(goal.id, f.owner, correction)
            .expect("replay");
        let corrected = f.goal(goal.id);
        assert_eq!(corrected.scores.len(), 2);
        assert_eq!(corrected.scores[1].outcome, !initial);
        assert_eq!(corrected.assessments, assessed.assessments);
        let original = corrected.closure.expect("original closure");
        f.service
            .goal_correct_human_outcome(
                goal.id,
                f.owner,
                NativeGoalOutcomeCorrection {
                    event_id: "unknown".into(),
                    expected_spec_digest: goal.spec_digest,
                    source: declaration().source,
                    occurred_at: f.now(),
                    supersedes_event_id: "corrected".into(),
                    outcome: None,
                    reason: "Human cannot establish outcome".into(),
                },
            )
            .expect("unknown correction");
        let unknown = f.goal(goal.id);
        assert_eq!(unknown.forecast_status, ForecastStatus::Unknown);
        assert_eq!(unknown.scores.len(), 2, "no invented score for unknown");
        assert_eq!(unknown.closure.expect("original retained"), original);
    }
}
#[tokio::test]
async fn failed_assessment_retries_and_only_fetched_ids_can_support() {
    let f = Fixture::new();
    let goal = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("create");
    f.service
        .goal_close_native(
            goal.id,
            f.owner,
            close(&goal, f.now(), HumanClosureChoice::StopNotAchieved),
        )
        .expect("close");
    f.service
        .goal_assess(goal.id, f.owner, capture, |input| async {
            let mut result = reply(input, CriterionEvidenceStatus::Supported);
            result.findings[0].evidence_ids = vec!["invented".into()];
            Ok(result)
        })
        .await
        .expect("failure retained");
    assert!(
        f.goal(goal.id).assessments[0]
            .result
            .as_ref()
            .expect("failure")
            .is_err()
    );
    f.reopen()
        .goal_assess(goal.id, f.owner, capture, |input| async {
            Ok(reply(input, CriterionEvidenceStatus::Supported))
        })
        .await
        .expect("retry");
    assert_eq!(f.goal(goal.id).assessments.len(), 2);
    assert!(!f.goal(goal.id).needs_assessment(f.now()));
}
#[tokio::test]
async fn separate_hosts_cannot_steal_live_claim_or_finish_expired_work() {
    let f = Fixture::new();
    let goal = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("create");
    f.service
        .goal_close_native(
            goal.id,
            f.owner,
            close(&goal, f.now(), HumanClosureChoice::StopUnknown),
        )
        .expect("close");
    let (started, wait) = tokio::sync::oneshot::channel();
    let (finish, resume) = tokio::sync::oneshot::channel();
    let service = f.reopen();
    let owner = f.owner;
    let id = goal.id;
    let first = tokio::spawn(async move {
        service
            .goal_assess(id, owner, capture, |input| async move {
                started.send(()).expect("started");
                resume.await.expect("resume");
                Ok(reply(input, CriterionEvidenceStatus::Contradicted))
            })
            .await
    });
    wait.await.expect("persisted before inference");
    let frozen = f.goal(id);
    assert_eq!(frozen.assessments[0].input.artifacts.len(), 1);
    f.reopen()
        .goal_assess(
            id,
            owner,
            |_| panic!("live claim: no capture"),
            |_| async { panic!("live claim: no inference") },
        )
        .await
        .expect("other host skips live claim");
    f.advance(91);
    f.reopen()
        .goal_assess(id, owner, capture, |input| async {
            Ok(reply(input, CriterionEvidenceStatus::Supported))
        })
        .await
        .expect("expired claim replaced");
    finish.send(()).expect("late completion");
    assert!(first.await.expect("task").is_err());
    let retained = f.goal(id);
    assert_eq!(retained.assessments.len(), 2);
    assert!(
        retained.assessments[0]
            .result
            .as_ref()
            .expect("expiry")
            .is_err()
    );
    assert_eq!(
        retained.assessments[1]
            .result
            .as_ref()
            .expect("result")
            .as_ref()
            .expect("new completion")
            .findings[0]
            .status,
        CriterionEvidenceStatus::Supported
    );
}
#[tokio::test]
async fn stale_preparation_cannot_publish_or_acknowledge_newer_revision() {
    let f = Fixture::new();
    let goal = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("create");
    let other = f.reopen();
    let result = f
        .service
        .goal_index_current(goal.id, f.owner, |_| async {
            other
                .goal_close_native(
                    goal.id,
                    f.owner,
                    close(&goal, f.now(), HumanClosureChoice::StopUnknown),
                )
                .expect("new revision");
            other
                .goal_index_current(goal.id, f.owner, |_| async { Ok(|| Ok(())) })
                .await
                .expect("new revision published");
            Ok(|| -> Result<(), String> {
                panic!("older payload must not publish over current memory")
            })
        })
        .await;
    assert!(result.is_err());
    let current = f.goal(goal.id);
    assert_eq!(current.indexed_revision, current.revision);
}
#[test]
fn current_schema_listing_and_host_scan_include_closed_goals_newest_first() {
    let f = Fixture::new();
    let first = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("first");
    let closed = f
        .service
        .goal_close_native(
            first.id,
            f.owner,
            close(&first, f.now(), HumanClosureChoice::StopUnknown),
        )
        .expect("close");
    f.advance(1);
    let second = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("second");
    let foreign_owner = WebID::new();
    let foreign = f
        .service
        .goal_create(declaration(), foreign_owner)
        .expect("foreign");
    let listed = f.service.goal_list(&f.owner).expect("current goals");
    assert_eq!(listed, vec![second.clone(), closed.clone()]);
    let scanned = f.service.goal_scan_for_host().expect("host scan");
    assert_eq!(scanned.len(), 2);
    assert_eq!(scanned[&f.owner], vec![second, closed]);
    assert_eq!(scanned[&foreign_owner], vec![foreign]);
}

#[test]
fn unsupported_schema_is_rejected_by_get_list_and_host_scan() {
    let f = Fixture::new();
    f.service
        .goal_create(declaration(), f.owner)
        .expect("current goal");
    let id = hkask_types::GoalID::new();
    f.store()
        .insert(&HMem::new(
            "kanban:goal",
            &id.to_string(),
            serde_json::json!({"schema_version":2,"id":id}),
            f.owner,
        ))
        .expect("obsolete schema fixture");
    for error in [
        f.service
            .goal_get(id, f.owner)
            .expect_err("get rejects obsolete schema"),
        f.service
            .goal_list(&f.owner)
            .expect_err("list rejects obsolete schema"),
        f.service
            .goal_scan_for_host()
            .expect_err("host rejects obsolete schema"),
    ] {
        assert!(error.to_string().contains("expected version 3"));
    }
}

#[test]
fn invalid_current_records_fail_listing_and_host_scan_instead_of_being_omitted() {
    for case in [
        "missing field",
        "unknown field",
        "key",
        "owner",
        "duplicate",
    ] {
        let f = Fixture::new();
        let goal = f
            .service
            .goal_create(declaration(), f.owner)
            .expect("current goal");
        let mut value = serde_json::to_value(&goal).expect("goal JSON");
        let mut key = goal.id.to_string();
        match case {
            "missing field" => {
                value.as_object_mut().expect("object").remove("criteria");
            }
            "unknown field" => {
                value["removed_field"] = serde_json::json!(true);
            }
            "key" => {
                key = "drifted-key".into();
            }
            "owner" => {
                value["owner"] = serde_json::json!(WebID::new());
            }
            "duplicate" => {}
            _ => unreachable!("fixed test cases"),
        }
        f.store()
            .insert(&HMem::new("kanban:goal", &key, value, f.owner))
            .expect("invalid current fixture");
        let listed = f.service.goal_list(&f.owner).expect_err(case);
        let scanned = f.service.goal_scan_for_host().expect_err(case);
        assert_eq!(listed.to_string(), scanned.to_string(), "{case}");
        let expected = match case {
            "missing field" | "unknown field" => "goal decode",
            "key" | "duplicate" => "key mismatch or duplicate canonical identity",
            "owner" => "not owned by caller",
            _ => unreachable!("fixed test cases"),
        };
        assert!(listed.to_string().contains(expected), "{case}: {listed}");
    }
}

#[test]
fn caller_listing_filters_foreign_rows_before_decoding_or_owner_validation() {
    let f = Fixture::new();
    let goal = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("current goal");
    let foreign_owner = WebID::new();
    let store = f.store();
    for (key, value) in [
        ("malformed", serde_json::json!({"schema_version":3})),
        ("unsupported", serde_json::json!({"schema_version":2})),
        (
            "owner-mismatch",
            serde_json::to_value(&goal).expect("goal JSON"),
        ),
    ] {
        store
            .insert(&HMem::new("kanban:goal", key, value, foreign_owner))
            .expect("foreign fixture");
    }
    assert_eq!(
        f.service.goal_list(&f.owner).expect("isolated caller"),
        vec![goal]
    );
    assert!(
        f.service
            .goal_list(&WebID::new())
            .expect("no owned rows")
            .is_empty()
    );
    assert!(f.service.goal_list(&foreign_owner).is_err());
    assert!(f.service.goal_scan_for_host().is_err());
}
#[test]
fn strict_forecasts_and_findings_do_not_accept_removed_shapes_or_extra_fields() {
    for value in [
        serde_json::json!({"kind":"unknown"}),
        serde_json::json!({"kind":"human_acceptance","outcome":true}),
    ] {
        assert!(serde_json::from_value::<GoalForecastResolver>(value).is_err());
    }
    assert!(serde_json::from_value::<CriterionFinding>(serde_json::json!({"criterion_id":"c","status":"unknown","evidence_ids":[],"note":"none","outcome":true})).is_err());
    assert!(
        serde_json::from_value::<AssessmentResult>(
            serde_json::json!({"findings":[],"limitations":[],"score":1})
        )
        .is_err()
    );
}

#[tokio::test]
async fn rejected_authority_or_evidence_changes_leave_no_partial_attempt() {
    let f = Fixture::new();
    let goal = f
        .service
        .goal_create(declaration(), f.owner)
        .expect("create");
    assert!(f.service.goal_get(goal.id, WebID::new()).is_err());
    assert!(
        f.service
            .goal_close_native(
                goal.id,
                WebID::new(),
                close(&goal, f.now(), HumanClosureChoice::StopUnknown)
            )
            .is_err()
    );
    let mut closure = close(&goal, f.now(), HumanClosureChoice::StopUnknown);
    closure.artifact_access = None;
    f.service
        .goal_close_native(goal.id, f.owner, closure)
        .expect("closure without grant");
    let before = f.goal(goal.id);
    assert!(
        f.service
            .goal_assess(goal.id, f.owner, capture, |_| async {
                panic!("unapproved bytes must never reach inference")
            })
            .await
            .is_err()
    );
    assert_eq!(
        f.goal(goal.id),
        before,
        "failed claim is atomic, not a half attempt"
    );
}

#[tokio::test]
async fn closure_before_horizon_cannot_become_eligible_via_later_human_correction() {
    let f = Fixture::new();
    let mut spec = declaration();
    spec.forecast = Some(GoalForecast {
        proposition: "Human accepts".into(),
        probability: 0.8,
        baseline_probability: 0.5,
        horizon: f.now() + Duration::seconds(10),
        resolution_deadline: f.now() + Duration::seconds(20),
        execution_policy: "task".into(),
        resources: "session".into(),
        cohort: "features".into(),
        resolver: GoalForecastResolver::HumanAcceptance {},
        update_policy: ForecastUpdatePolicy::FixedAtDeclaration,
    });
    let goal = f
        .service
        .goal_create(spec.clone(), f.owner)
        .expect("prospective");
    let mut early = close(&goal, f.now(), HumanClosureChoice::AccomplishedAchieved);
    early.evidence_manifest.clear();
    f.service
        .goal_close_native(goal.id, f.owner, early)
        .expect("early closure");
    f.service
        .goal_assess(
            goal.id,
            f.owner,
            |_| Ok(vec![]),
            |_| async { panic!("no evidence") },
        )
        .await
        .expect("unknown assessment");
    f.advance(15);
    f.service
        .goal_correct_human_outcome(
            goal.id,
            f.owner,
            NativeGoalOutcomeCorrection {
                event_id: "later".into(),
                expected_spec_digest: goal.spec_digest,
                source: declaration().source,
                occurred_at: f.now(),
                supersedes_event_id: "native-choice".into(),
                outcome: Some(true),
                reason: "Correction inside window cannot move the original event".into(),
            },
        )
        .expect("correction");
    assert!(f.goal(goal.id).scores.is_empty());
    assert!(
        f.service.goal_create(spec.clone(), f.owner).is_err(),
        "cannot declare retrospectively"
    );
    let forecast = spec.forecast.as_mut().expect("forecast");
    forecast.horizon = f.now() + Duration::seconds(10);
    forecast.resolution_deadline = f.now() + Duration::seconds(5);
    assert!(
        f.service.goal_create(spec.clone(), f.owner).is_err(),
        "invalid window"
    );
    let forecast = spec.forecast.as_mut().expect("forecast");
    forecast.resolution_deadline = f.now() + Duration::seconds(20);
    forecast.probability = f64::NAN;
    assert!(
        f.service.goal_create(spec, f.owner).is_err(),
        "no nonfinite probability"
    );
}

#[test]
fn declaration_and_worker_progress_preserve_exact_criterion_obligations() {
    let f = Fixture::new();
    for count in [0, 5] {
        let mut invalid = declaration();
        invalid.criteria = vec![VerificationCriterion::new("criterion".into()); count];
        assert!(f.service.goal_create(invalid, f.owner).is_err());
    }
    let goal = f.service.goal_create(declaration(), f.owner).expect("goal");
    let mut report = GoalVerdict {
        verdict: GoalVerdictValue::Done,
        confidence: 0.5,
        criterion_results: vec![CriterionJudgment {
            index: 0,
            passed: false,
            note: "Not working".into(),
        }],
        reasoning: "Observed failure".into(),
        judged_at: f.now(),
    };
    assert!(
        f.service
            .goal_judge(goal.id, report.clone(), f.owner)
            .is_err(),
        "done cannot contradict failed criteria"
    );
    report.verdict = GoalVerdictValue::Continue;
    f.service
        .goal_judge(goal.id, report.clone(), f.owner)
        .expect("honest progress");
    report.criterion_results.clear();
    assert!(
        f.service.goal_judge(goal.id, report, f.owner).is_err(),
        "every criterion must be covered"
    );
    assert_eq!(f.goal(goal.id).verdicts.len(), 1);
}
