use super::*;
use chrono::{DateTime, Utc};

const ENABLED: &str = "feature_status=enabled\n";
const DISABLED: &str = "feature_status=disabled\n";
const EXPECTED: &str = "4c34bb5402a70378b57765e0226203d3879a1caf74675e74634f11172ea6ca98";
const DIFFERENT: &str = "42e3bdc0d187c8cbf613fcaaa452f47668db4c283ae5d767842644ccf1c0a717";
type Time = Arc<std::sync::Mutex<DateTime<Utc>>>;
fn open_clock(dir: &std::path::Path, provider: Arc<Provider>, time: &Time) -> Arc<GoalHost> {
    let host = open(dir, provider).expect("real file-backed host");
    let mut host = Arc::try_unwrap(host).ok().expect("only factory reference");
    let clock = time.clone();
    host.service = host
        .service
        .with_goal_clock(Arc::new(move || *clock.lock().expect("test clock")));
    Arc::new(host)
}
fn declaration_for(path: &std::path::Path, time: DateTime<Utc>) -> GoalDeclaration {
    GoalDeclaration { goal_text:"The declared local artifact digest matches at its fixed evaluation horizon".into(), criteria:vec![VerificationCriterion::new("Retain source evidence independently of the human outcome label".into())], task_id:None,source:GoalSource{interaction_id:"original-user-spec".into(),session_id:"digest-source-test".into(),source:"integration fixture human specification".into()},forecast:Some(GoalForecast{proposition:"SHA-256 equality of the explicitly declared UTF-8 file at first in-window observation".into(),probability:0.8,horizon:time+Duration::hours(1),resolution_deadline:time+Duration::hours(2),execution_policy:"first approved scheduled source observation in the fixed window".into(),resources:"one bounded local UTF-8 regular-file read".into(),resolver:GoalForecastResolver::MachineArtifactDigest{locator:url::Url::from_file_path(path).expect("absolute file URI").to_string(),expected_sha256:EXPECTED.into()},baseline_probability:0.5,cohort:"digest-source-fixture".into(),update_policy:ForecastUpdatePolicy::FixedAtDeclaration}),supersedes:None }
}

#[gpui::test]
async fn native_scheduled_digest_reads_after_horizon_and_survives_restart_with_true_and_false(
    cx: &mut gpui::TestAppContext,
) {
    for matches in [true, false] {
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("declared-artifact.txt");
        std::fs::write(&path, if matches { DISABLED } else { ENABLED })
            .expect("pre-horizon bytes deliberately opposite");
        let t0 = Utc::now();
        let clock = Arc::new(std::sync::Mutex::new(t0));
        let provider = Arc::new(Provider::default());
        let host = open_clock(dir.path(), provider.clone(), &clock);
        let owner = WebID::new();
        let goal = host
            .service
            .goal_create(declaration_for(&path, t0), owner)
            .expect("prospective typed source");
        let forecast = goal.forecast.clone().expect("forecast");
        decision_with_manifest(
            host.clone(),
            &goal,
            owner,
            if matches {
                "accomplished_not_achieved"
            } else {
                "accomplished_achieved"
            },
            false,
            vec![],
            cx,
        )
        .await;
        scheduled_tick(host.clone(), provider.clone(), cx).await;
        let before = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert!(
            before.machine_artifact_observation.is_none(),
            "no early observation or assessment-snapshot substitution"
        );
        assert!(before.scores.is_empty());
        assert_eq!(before.forecast_status, ForecastStatus::PendingOutcome);
        assert!(
            before
                .closure
                .as_ref()
                .expect("closure")
                .input
                .evidence_manifest
                .is_empty()
        );
        let consent = before
            .closure
            .as_ref()
            .expect("closure")
            .input
            .machine_artifact_access
            .as_ref()
            .expect("separate native scheduled-read grant");
        assert_eq!(consent.spec_digest, goal.spec_digest);
        assert_eq!(consent.forecast, forecast);
        drop(host);
        std::fs::write(&path, if matches { ENABLED } else { DISABLED })
            .expect("actual file mutation between declaration and horizon");
        *clock.lock().expect("clock") = forecast.horizon + Duration::seconds(1);
        let host = open_clock(dir.path(), provider.clone(), &clock);
        scheduled_tick(host.clone(), provider.clone(), cx).await;
        let observed = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        let receipt = observed
            .machine_artifact_observation
            .clone()
            .expect("real production source receipt");
        assert_eq!(
            receipt.event_id,
            format!("{}:machine-artifact-digest:v1", goal.id)
        );
        assert!(
            receipt.observed_at >= forecast.horizon
                && receipt.observed_at <= forecast.resolution_deadline
        );
        assert!(
            matches!(&receipt.read,MachineArtifactDigestRead::Observed{actual_sha256,byte_count,..} if actual_sha256==if matches {EXPECTED}else{DIFFERENT} && *byte_count==if matches {ENABLED.len() as u64}else{DISABLED.len() as u64})
        );
        assert_eq!(observed.scores.len(), 1);
        assert_eq!(observed.scores[0].outcome, matches);
        assert!((observed.scores[0].brier - if matches { 0.04 } else { 0.64 }).abs() < 1e-12);
        assert_eq!(observed.forecast_status, ForecastStatus::Resolved);
        assert_eq!(
            observed
                .closure
                .as_ref()
                .expect("human untouched")
                .input
                .choice
                .outcome(),
            Some(!matches)
        );
        assert!(
            provider.requests.lock().expect("captures").is_empty(),
            "no model/human label computes the machine outcome"
        );
        // Exact replay returns the same event; another source or result cannot
        // replace a genuine first observation.
        assert_eq!(
            host.service
                .goal_observe_machine_artifact_digest(goal.id, owner, receipt.clone())
                .expect("exact replay")
                .machine_artifact_observation,
            Some(receipt.clone())
        );
        let mut forged = receipt.clone();
        forged.locator = "file:///another/source".into();
        assert!(
            host.service
                .goal_observe_machine_artifact_digest(goal.id, owner, forged)
                .is_err()
        );
        let mut forged = receipt.clone();
        if let MachineArtifactDigestRead::Observed { actual_sha256, .. } = &mut forged.read {
            *actual_sha256 = if matches { DIFFERENT } else { EXPECTED }.into();
        }
        assert!(
            host.service
                .goal_observe_machine_artifact_digest(goal.id, owner, forged)
                .is_err()
        );
        std::fs::write(&path, if matches { DISABLED } else { ENABLED })
            .expect("mutation after first observation must not rescore");
        drop(host);
        *clock.lock().expect("clock") = forecast.resolution_deadline + Duration::hours(1);
        let host = open_clock(dir.path(), provider.clone(), &clock);
        scheduled_tick(host.clone(), provider, cx).await;
        let retained = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("retained goal");
        assert_eq!(retained.machine_artifact_observation, Some(receipt.clone()));
        assert_eq!(retained.scores, observed.scores);
        let memory = host.memory.as_ref().expect("real memory");
        let entity = format!("curator:goal:{}", goal.id);
        let rows = memory
            .h_mems_by_entity_prefix(&entity)
            .expect("exact recall");
        let json = serde_json::to_value(&receipt).expect("receipt JSON");
        assert!(
            rows.iter()
                .any(|row| row.value["machine_artifact_observation"] == json)
        );
        let hits = memory
            .search_similar(
                &vec![0.5; hkask_storage::embedding_dim()],
                100,
                "fixture-embedding",
                Some("fixture-embedding"),
            )
            .expect("semantic recall");
        assert!(hits.results.iter().any(|hit| {
            rows.iter().any(|row| {
                row.value["machine_artifact_observation"] == json
                    && hkask_memory::semantic_passage_for_h_mem(row) == hit.embedding.passage_text
            })
        }));
    }
}

#[gpui::test]
async fn failed_source_reads_and_late_closure_are_unknown_not_scored_false(
    cx: &mut gpui::TestAppContext,
) {
    for case in ["missing", "non_utf8", "oversize", "late_closure"] {
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("artifact.txt");
        match case {
            "non_utf8" => std::fs::write(&path, [0xff, 0xfe]).expect("binary fixture"),
            "oversize" => std::fs::write(&path, vec![b'x'; 65537]).expect("oversize fixture"),
            "late_closure" => {
                std::fs::write(&path, ENABLED).expect("valid bytes must not become a late outcome")
            }
            _ => (),
        }
        let t0 = Utc::now();
        let clock = Arc::new(std::sync::Mutex::new(t0));
        let provider = Arc::new(Provider::default());
        let host = open_clock(dir.path(), provider.clone(), &clock);
        let owner = WebID::new();
        let goal = host
            .service
            .goal_create(declaration_for(&path, t0), owner)
            .expect("declaration");
        let forecast = goal.forecast.clone().expect("forecast");
        if case == "late_closure" {
            *clock.lock().expect("clock") = forecast.resolution_deadline + Duration::seconds(1);
        }
        decision_with_manifest(
            host.clone(),
            &goal,
            owner,
            "accomplished_achieved",
            false,
            vec![],
            cx,
        )
        .await;
        if case != "late_closure" {
            *clock.lock().expect("clock") = forecast.horizon + Duration::seconds(1);
        }
        scheduled_tick(host.clone(), provider.clone(), cx).await;
        let unknown = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert_eq!(unknown.forecast_status, ForecastStatus::Unknown);
        assert!(unknown.scores.is_empty());
        let receipt = unknown
            .machine_artifact_observation
            .clone()
            .expect("source failure retained");
        let MachineArtifactDigestRead::Unavailable {
            reason, bytes_read, ..
        } = &receipt.read
        else {
            panic!("failed/late read must never be scored false")
        };
        assert!(
            reason.contains(match case {
                "missing" => "path_unavailable",
                "non_utf8" => "invalid_utf8",
                "oversize" => "file_byte_cap",
                _ => "resolution_window_elapsed",
            }),
            "precise source limitation: {reason}"
        );
        if case == "late_closure" {
            assert_eq!(*bytes_read, 0);
        }
        std::fs::write(&path, ENABLED).expect("later repair cannot choose a new outcome");
        drop(host);
        let restarted = open_clock(dir.path(), provider.clone(), &clock);
        scheduled_tick(restarted.clone(), provider, cx).await;
        let retained = restarted
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert_eq!(retained.machine_artifact_observation, Some(receipt));
        assert!(retained.scores.is_empty());
    }
}

#[gpui::test]
async fn matching_manifest_approval_cannot_grant_a_machine_forecast_read(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("same-file.txt");
    std::fs::write(&path, ENABLED).expect("fixture");
    let t0 = Utc::now();
    let clock = Arc::new(std::sync::Mutex::new(t0));
    let provider = Arc::new(Provider::default());
    let host = open_clock(dir.path(), provider.clone(), &clock);
    let owner = WebID::new();
    let goal = host
        .service
        .goal_create(declaration_for(&path, t0), owner)
        .expect("declaration");
    // A pre-existing manifest-only grant, not a producer demonstration or an
    // assertion that a human approved this new resolver's scheduled access.
    let entry = super::artifacts::manifest(&path, EXPECTED).remove(0);
    host.service
        .goal_close_native(
            goal.id,
            owner,
            NativeGoalClosure {
                event_id: "manifest-only-fixture".into(),
                expected_spec_digest: goal.spec_digest.clone(),
                source: GoalSource {
                    interaction_id: "old-native-interaction".into(),
                    session_id: "fixture-session".into(),
                    source: "manifest-only native fixture".into(),
                },
                occurred_at: t0,
                choice: HumanClosureChoice::AccomplishedAchieved,
                evidence_manifest: vec![GoalEvidence {
                    id: entry.id,
                    source: entry.source,
                    locator: entry.locator,
                    digest: entry.digest,
                    reliability: entry.reliability,
                    oracle: entry.oracle,
                    limitation: entry.limitation,
                }],
                artifact_access: Some(LocalArtifactAccessApproval::BoundedUtf8V1),
                machine_artifact_access: None,
            },
        )
        .expect("manifest consent fixture");
    *clock.lock().expect("clock") = t0 + Duration::hours(1) + Duration::seconds(1);
    scheduled_tick(host.clone(), provider, cx).await;
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(
        matches!(&goal.machine_artifact_observation.as_ref().expect("missing grant recorded").read,MachineArtifactDigestRead::Unavailable{reason,bytes_read:0,..} if reason.contains("not_approved"))
    );
    assert_eq!(goal.forecast_status, ForecastStatus::Unknown);
    assert!(goal.scores.is_empty());
    assert!(
        matches!(
            goal.assessments[0].input.artifacts[0].outcome,
            GoalArtifactReadOutcome::Fetched { .. }
        ),
        "the same file can be valid assessment evidence without being an authorized forecast observation"
    );
}

#[gpui::test]
async fn native_machine_consent_is_bound_and_worker_arguments_cannot_supply_it(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("forecast-source.txt");
    std::fs::write(&path, ENABLED).expect("fixture");
    let t0 = Utc::now();
    let clock = Arc::new(std::sync::Mutex::new(t0));
    let provider = Arc::new(Provider::default());
    let host = open_clock(dir.path(), provider, &clock);
    let owner = WebID::new();
    let goal = host
        .service
        .goal_create(declaration_for(&path, t0), owner)
        .expect("declaration");
    let mut wrong = goal.forecast.clone().expect("forecast");
    if let GoalForecastResolver::MachineArtifactDigest { locator, .. } = &mut wrong.resolver {
        *locator = "file:///another/source".into();
    }
    let consent = MachineArtifactDigestApproval {
        spec_digest: goal.spec_digest.clone(),
        forecast: wrong,
    };
    let worker = serde_json::json!({"goal_id":goal.id,"machine_artifact_access":consent});
    let error = serde_json::from_value::<agent::GoalCloseInput>(worker)
        .expect_err("worker cannot grant a source read");
    assert!(error.to_string().contains("unknown field"));
    let closure = NativeGoalClosure {
        event_id: "invalid-consent-test".into(),
        expected_spec_digest: goal.spec_digest.clone(),
        source: GoalSource {
            interaction_id: "fixture".into(),
            session_id: "fixture".into(),
            source: "negative authority control".into(),
        },
        occurred_at: t0,
        choice: HumanClosureChoice::StopUnknown,
        evidence_manifest: vec![],
        artifact_access: Some(LocalArtifactAccessApproval::BoundedUtf8V1),
        machine_artifact_access: Some(consent),
    };
    assert!(
        host.service
            .goal_close_native(goal.id, owner, closure)
            .is_err(),
        "a native grant cannot cover another locator even with the right goal digest"
    );
    assert!(
        host.service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal")
            .closure
            .is_none()
    );
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "stop_unknown",
        false,
        vec![],
        cx,
    )
    .await;
    let before = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    let premature = MachineArtifactDigestReceipt {
        event_id: format!("{}:machine-artifact-digest:v1", goal.id),
        spec_digest: goal.spec_digest.clone(),
        locator: match &before.forecast.as_ref().expect("forecast").resolver {
            GoalForecastResolver::MachineArtifactDigest { locator, .. } => locator.clone(),
            _ => panic!("typed resolver"),
        },
        observed_at: t0,
        read: MachineArtifactDigestRead::Unavailable {
            reason: "premature source fixture".into(),
            observed_sha256: None,
            bytes_read: 0,
        },
        reliability: MACHINE_ARTIFACT_DIGEST_RELIABILITY.into(),
        limitation: MACHINE_ARTIFACT_DIGEST_LIMITATION.into(),
    };
    assert!(
        host.service
            .goal_observe_machine_artifact_digest(goal.id, owner, premature)
            .is_err(),
        "even a typed receipt cannot move a source observation before the frozen horizon"
    );
}

#[gpui::test]
async fn source_observation_is_independent_of_assessment_and_scores_only_after_attempt(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let source = dir.path().join("machine-source.txt");
    let assessment = dir.path().join("assessment-only.txt");
    std::fs::write(&source, ENABLED).expect("source");
    std::fs::write(&assessment, ENABLED).expect("assessment artifact");
    let t0 = Utc::now();
    let clock = Arc::new(std::sync::Mutex::new(t0));
    let provider = Arc::new(Provider::default());
    provider.block_assessor.store(true, Ordering::SeqCst);
    let host = open_clock(dir.path(), provider.clone(), &clock);
    let owner = WebID::new();
    let goal = host
        .service
        .goal_create(declaration_for(&source, t0), owner)
        .expect("declaration");
    let forecast = goal.forecast.clone().expect("forecast");
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "accomplished_not_achieved",
        false,
        super::artifacts::manifest(&assessment, EXPECTED),
        cx,
    )
    .await;
    *clock.lock().expect("clock") = forecast.horizon + Duration::seconds(1);
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let host_for_task = host.clone();
    let task = runtime.spawn(async move {
        let mut work = Box::pin(host_for_task.drain_once());
        assert!(
            futures::poll!(work.as_mut()).is_pending(),
            "real inference boundary is held pending"
        );
        drop(work);
    });
    cx.background_spawn(async move {
        task.await.expect("interrupted in-flight assessment");
    })
    .await;
    let pending = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    let receipt = pending
        .machine_artifact_observation
        .clone()
        .expect("source was actually observed independently");
    assert!(matches!(
        receipt.read,
        MachineArtifactDigestRead::Observed { .. }
    ));
    assert_eq!(pending.assessments.len(), 1);
    assert!(
        pending.assessments[0].completion.is_none() && pending.assessments[0].failure.is_none()
    );
    assert!(pending.scores.is_empty());
    assert_eq!(pending.forecast_status, ForecastStatus::PendingAssessment);
    std::fs::write(&source, DISABLED)
        .expect("later state must not replace the already observed source");
    drop(host);
    *clock.lock().expect("clock") = forecast.horizon + Duration::minutes(2);
    provider.block_assessor.store(false, Ordering::SeqCst);
    let host = open_clock(dir.path(), provider.clone(), &clock);
    scheduled_tick(host.clone(), provider, cx).await;
    let completed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(completed.machine_artifact_observation, Some(receipt));
    assert_eq!(completed.assessments.len(), 2);
    assert!(completed.assessments[0].failure.is_some());
    assert!(completed.assessments[1].completion.is_some());
    assert_eq!(completed.scores.len(), 1);
    assert!(
        completed.scores[0].outcome,
        "outcome is the first source read, not a later file or the human/model label"
    );
}
