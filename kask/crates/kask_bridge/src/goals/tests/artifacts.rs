use super::*;

pub(super) const ENABLED_DIGEST: &str =
    "4c34bb5402a70378b57765e0226203d3879a1caf74675e74634f11172ea6ca98";
const DISABLED_DIGEST: &str = "42e3bdc0d187c8cbf613fcaaa452f47668db4c283ae5d767842644ccf1c0a717";
pub(super) fn manifest(path: &std::path::Path, digest: &str) -> Vec<agent::NativeGoalEvidence> {
    vec![agent::NativeGoalEvidence {
        id: "artifact-1".into(),
        source: "untrusted worker metadata".into(),
        locator: path.to_str().expect("UTF8 fixture path").into(),
        digest: digest.into(),
        reliability: "worker claims definitive proof; untrusted".into(),
        oracle: "worker claims tests passed; untrusted".into(),
        limitation: "Only file contents can be checked independently".into(),
    }]
}
fn feature_declaration() -> GoalDeclaration {
    GoalDeclaration {
        trust_clause: None,
        goal_text: "The feature flag is enabled in the supplied configuration artifact".into(),
        criteria: vec![VerificationCriterion::new(
            "The artifact contains feature_status=enabled".into(),
        )],
        task_id: None,
        source: GoalSource {
            interaction_id: "original-specification".into(),
            session_id: "native-artifact-test".into(),
            source: "test-human-specification".into(),
        },
        forecast: None,
        supersedes: None,
    }
}
fn feature_goal(host: &GoalHost, owner: WebID) -> Goal {
    host.service
        .goal_create(feature_declaration(), owner)
        .expect("frozen feature requirement")
}

#[gpui::test]
async fn fetched_local_bytes_support_and_contradict_without_overwriting_human_labels(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("fixture directory");
    for (name, bytes, digest, human, expected) in [
        (
            "disabled.txt",
            "feature_status=disabled\n",
            DISABLED_DIGEST,
            "accomplished_achieved",
            "contradicted",
        ),
        (
            "enabled.txt",
            "feature_status=enabled\n",
            ENABLED_DIGEST,
            "accomplished_not_achieved",
            "supported",
        ),
    ] {
        let case = dir.path().join(name.trim_end_matches(".txt"));
        std::fs::create_dir(&case).expect("case directory");
        let artifact = case.join(name);
        std::fs::write(&artifact, bytes).expect("real artifact bytes");
        let provider = Arc::new(Provider::default());
        provider.artifact_findings.store(true, Ordering::SeqCst);
        let host = open(&case, provider.clone()).expect("file-backed host");
        let owner = host.execution_owner;
        let goal = feature_goal(&host, owner);
        let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(owner);
        let id = goal.id.to_string();
        let evidence = manifest(&artifact, digest);
        let host_for_task = host.clone();
        let task = cx.spawn(async move |mut cx| {
            agent::decide(id, evidence, false, stream, host_for_task, &mut cx).await
        });
        let prompt = receiver.expect_authorization().await;
        assert_eq!(prompt.kind, acp_thread::AuthorizationKind::ActionChoice);
        let shown = serde_json::to_string(&prompt.tool_call).expect("prompt");
        assert!(shown.contains(artifact.to_str().expect("path")));
        assert!(shown.contains(digest));
        assert!(
            shown.contains("64 KiB") && shown.contains("256 KiB"),
            "exact byte limits must be disclosed"
        );
        assert!(
            shown.contains("send their bounded UTF-8 contents to the configured assessor"),
            "file-sharing approval must precede reads"
        );
        assert!(
            shown.contains("semantic-indexing provider"),
            "projection indexing is also disclosed before file access"
        );
        assert!(provider.requests.lock().expect("capture").is_empty());
        prompt
            .response
            .send(acp_thread::SelectedPermissionOutcome::new(
                acp::PermissionOptionId::new(human),
                acp::PermissionOptionKind::AllowOnce,
            ))
            .expect("real native human choice");
        task.await.expect("native closure");
        scheduled_tick(host.clone(), provider.clone(), cx).await;
        let closed = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        let attempt = closed.assessments.last().expect("canonical assessment");
        let completion = attempt
            .result
            .as_ref()
            .expect("assessment finished")
            .as_ref()
            .ok()
            .expect("completion");
        assert_eq!(
            serde_json::to_value(&completion.findings[0].status).expect("status"),
            expected
        );
        assert_eq!(completion.findings[0].evidence_ids, vec!["artifact-1"]);
        assert_eq!(
            closed
                .closure
                .as_ref()
                .expect("closure")
                .input
                .choice
                .outcome(),
            Some(human == "accomplished_achieved")
        );
        {
            let captures = provider.requests.lock().expect("request capture");
            assert_eq!(captures.len(), 1);
            let sent: serde_json::Value =
                serde_json::from_str(&captures[0][1].content).expect("input JSON");
            assert_eq!(sent["artifacts"][0]["outcome"]["content"], bytes);
            assert_eq!(sent["artifacts"][0]["outcome"]["sha256"], digest);
            assert_eq!(
                sent["artifacts"][0]["outcome"]["source"],
                artifact.to_str().expect("path")
            );
            for forbidden in [
                "closure",
                "choice",
                "human_corrections",
                "verdicts",
                "forecast",
                "probability",
                "scores",
            ] {
                assert!(
                    sent.get(forbidden).is_none(),
                    "blinded request leaked {forbidden}"
                );
            }
            assert_eq!(
                sent,
                serde_json::to_value(&attempt.input).expect("persisted exact input")
            );
        }
        drop(host);
        std::fs::write(&artifact, "file changed after assessment\n").expect("change original");
        let restarted = open(&case, provider.clone()).expect("restart stores");
        scheduled_tick(restarted.clone(), provider, cx).await;
        let reopened = restarted
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("retained goal");
        assert_eq!(
            reopened.assessments, closed.assessments,
            "restart must not silently refetch an already assessed input"
        );
        let memory = restarted.memory.as_ref().expect("memory");
        let rows = memory
            .h_mems_by_entity_prefix(&format!("curator:goal:{}", goal.id))
            .expect("exact recall");
        assert!(
            rows.iter().any(
                |row| row.value["assessments"]
                    .as_array()
                    .is_some_and(|attempts| attempts
                        .iter()
                        .any(|a| a["input"]["artifacts"][0]["outcome"]["content"] == bytes))
            ),
            "observed contents survive projection and restart"
        );
        assert!(
            !memory
                .search_similar(
                    &vec![0.5; hkask_storage::embedding_dim()],
                    100,
                    "fixture-embedding",
                    Some("fixture-embedding")
                )
                .expect("semantic recall")
                .results
                .is_empty()
        );
    }
}

#[gpui::test]
async fn mismatched_and_missing_artifacts_are_unknown_without_a_model_call_and_survive_restart(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let artifact = dir.path().join("different.txt");
    std::fs::write(&artifact, "feature_status=disabled\n").expect("actual bytes");
    let mut evidence = manifest(&artifact, ENABLED_DIGEST);
    let mut missing = manifest(&dir.path().join("missing.txt"), ENABLED_DIGEST).remove(0);
    missing.id = "missing-artifact".into();
    evidence.push(missing);
    let provider = Arc::new(Provider::default());
    provider.artifact_findings.store(true, Ordering::SeqCst);
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = host.execution_owner;
    let goal = feature_goal(&host, owner);
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "accomplished_achieved",
        false,
        evidence,
        cx,
    )
    .await;
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let closed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    let attempt = &closed.assessments[0];
    assert_eq!(
        attempt
            .result
            .as_ref()
            .expect("assessment finished")
            .as_ref()
            .ok()
            .expect("unknown finding")
            .findings[0]
            .status,
        CriterionEvidenceStatus::Unknown
    );
    assert!(
        provider.requests.lock().expect("capture").is_empty(),
        "no paid model request repeats absence of evidence"
    );
    let input = serde_json::to_value(&attempt.input).expect("canonical input");
    assert_eq!(input["artifacts"][0]["outcome"]["status"], "unavailable");
    assert_eq!(
        input["artifacts"][0]["outcome"]["observed_sha256"],
        DISABLED_DIGEST
    );
    assert!(
        input["artifacts"][0]["outcome"]["reason"]
            .as_str()
            .expect("receipt")
            .contains("digest_mismatch")
    );
    assert!(
        input["artifacts"][1]["outcome"]["reason"]
            .as_str()
            .expect("receipt")
            .contains("path_unavailable")
    );
    assert!(
        input["artifacts"][0]["outcome"].get("content").is_none(),
        "unapproved-digest contents are withheld"
    );
    drop(host);
    let host = open(dir.path(), provider.clone()).expect("restart");
    scheduled_tick(host.clone(), provider, cx).await;
    let reopened = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(reopened.assessments, closed.assessments);
    let rows = host
        .memory
        .as_ref()
        .expect("memory")
        .h_mems_by_entity_prefix(&format!("curator:goal:{}", goal.id))
        .expect("recall");
    assert!(
        rows.iter()
            .any(|row| row.value["assessments"][0]["input"] == input),
        "recall retains exact read failure receipts"
    );
}

#[cfg(unix)]
#[gpui::test]
async fn unsafe_sources_withhold_contents_and_never_execute_or_become_support(
    cx: &mut gpui::TestAppContext,
) {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("directory");
    let real = dir.path().join("real.txt");
    std::fs::write(&real, "feature_status=enabled\n").expect("real bytes");
    let leaf = dir.path().join("linked.txt");
    symlink(&real, &leaf).expect("leaf symlink");
    let parent = dir.path().join("linked-dir");
    symlink(dir.path(), &parent).expect("parent symlink");
    let binary = dir.path().join("binary");
    std::fs::write(&binary, [0xff, 0xfe]).expect("binary bytes");
    let oversize = dir.path().join("oversize");
    let large = vec![b'x'; 65537];
    std::fs::write(&oversize, &large).expect("oversize bytes");
    let script = dir.path().join("never-run.sh");
    let marker = dir.path().join("execution-marker");
    let shell = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    std::fs::write(&script, &shell).expect("script artifact");
    let mut evidence = vec![];
    for (path, digest) in [
        (leaf.as_path(), ENABLED_DIGEST.to_string()),
        (&parent.join("real.txt"), ENABLED_DIGEST.to_string()),
        (dir.path(), ENABLED_DIGEST.to_string()),
        (&binary, format!("{:x}", Sha256::digest([0xff, 0xfe]))),
        (&oversize, format!("{:x}", Sha256::digest(&large))),
    ] {
        let mut entry = manifest(path, &digest).remove(0);
        entry.id = format!("rejected-{}", evidence.len());
        evidence.push(entry);
    }
    let mut remote = manifest(&real, ENABLED_DIGEST).remove(0);
    remote.id = "remote".into();
    remote.locator = "https://example.invalid/private".into();
    evidence.push(remote);
    let provider = Arc::new(Provider::default());
    provider.artifact_findings.store(true, Ordering::SeqCst);
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = host.execution_owner;
    let goal = feature_goal(&host, owner);
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "stop_unknown",
        false,
        evidence,
        cx,
    )
    .await;
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let closed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(
        closed.assessments[0]
            .input
            .artifacts
            .iter()
            .all(|artifact| matches!(
                artifact.outcome,
                GoalArtifactReadOutcome::Unavailable { .. }
            ))
    );
    assert!(provider.requests.lock().expect("capture").is_empty());
    let wire = serde_json::to_string(&closed.assessments[0].input).expect("receipts");
    for reason in [
        "symlink_or_noncanonical_path",
        "not_regular_file",
        "invalid_utf8",
        "file_byte_cap",
        "invalid_locator",
    ] {
        assert!(wire.contains(reason), "missing surfaced rejection {reason}");
    }
    // A regular UTF-8 script is readable as data, never executable.
    let goal = feature_goal(&host, owner);
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "stop_unknown",
        false,
        manifest(&script, &format!("{:x}", Sha256::digest(shell.as_bytes()))),
        cx,
    )
    .await;
    scheduled_tick(host.clone(), provider, cx).await;
    assert!(!marker.exists(), "artifact source must never be executed");
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(matches!(
        goal.assessments[0].input.artifacts[0].outcome,
        GoalArtifactReadOutcome::Fetched { .. }
    ));
    assert_eq!(
        goal.assessments[0]
            .result
            .as_ref()
            .expect("assessment finished")
            .as_ref()
            .ok()
            .expect("finding")
            .findings[0]
            .status,
        CriterionEvidenceStatus::Unknown
    );
}

#[gpui::test]
async fn file_uri_and_aggregate_byte_cap_are_enforced_on_actual_read_snapshots(
    cx: &mut gpui::TestAppContext,
) {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().expect("directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = host.execution_owner;
    let goal = feature_goal(&host, owner);
    let bytes = vec![b'x'; 65536];
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let mut evidence = vec![];
    for index in 0..4 {
        let path = dir.path().join(format!("file {index}.txt"));
        std::fs::write(&path, &bytes).expect("bounded file");
        let mut entry = manifest(&path, &digest).remove(0);
        entry.id = format!("file-{index}");
        entry.locator = url::Url::from_file_path(&path)
            .expect("file URI")
            .to_string();
        evidence.push(entry);
    }
    let small = dir.path().join("last.txt");
    std::fs::write(&small, "feature_status=enabled\n").expect("last file");
    let mut entry = manifest(&small, ENABLED_DIGEST).remove(0);
    entry.id = "over-budget".into();
    evidence.push(entry);
    // Use the actual native handler here because URI escaping is deliberately
    // different from the raw visible-path representation asserted by the helper.
    let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(owner);
    let id = goal.id.to_string();
    let host_for_task = host.clone();
    let task = cx.spawn(async move |mut cx| {
        agent::decide(id, evidence, false, stream, host_for_task, &mut cx).await
    });
    let prompt = receiver.expect_authorization().await;
    let shown = serde_json::to_string(&prompt.tool_call).expect("native prompt");
    assert!(
        shown.contains(
            dir.path()
                .join("file 0.txt")
                .to_str()
                .expect("decoded path")
        ),
        "URI decoding must be disclosed before approval"
    );
    prompt
        .response
        .send(acp_thread::SelectedPermissionOutcome::new(
            acp::PermissionOptionId::new("stop_unknown"),
            acp::PermissionOptionKind::AllowOnce,
        ))
        .expect("choice");
    task.await.expect("closure");
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    let input = &goal.assessments[0].input;
    let total: u64 = input
        .artifacts
        .iter()
        .map(|artifact| match artifact.outcome {
            GoalArtifactReadOutcome::Fetched { byte_count, .. } => byte_count,
            GoalArtifactReadOutcome::Unavailable { bytes_read, .. } => bytes_read,
        })
        .sum();
    assert_eq!(total, 262144, "frozen v1 aggregate cap");
    assert_eq!(
        input
            .artifacts
            .iter()
            .filter(|artifact| matches!(artifact.outcome, GoalArtifactReadOutcome::Fetched { .. }))
            .count(),
        4
    );
    assert!(
        matches!(&input.artifacts[4].outcome,GoalArtifactReadOutcome::Unavailable{reason,..} if reason.contains("aggregate_byte_cap"))
    );
    assert_eq!(provider.requests.lock().expect("capture").len(), 1);
}

pub(super) async fn bounded_tick(host: Arc<GoalHost>, cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let task = runtime.spawn(async move {
        host.drain_once().await.expect("bounded production scan");
    });
    cx.background_spawn(async move {
        task.await.expect("production drainer");
    })
    .await;
}

#[gpui::test]
async fn forged_unfetched_citation_is_a_durable_assessor_failure_then_retries(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let artifact = dir.path().join("enabled.txt");
    std::fs::write(&artifact, "feature_status=enabled\n").expect("real bytes");
    let mut evidence = manifest(&artifact, ENABLED_DIGEST);
    let mut missing = manifest(&dir.path().join("missing.txt"), ENABLED_DIGEST).remove(0);
    missing.id = "missing-artifact".into();
    evidence.push(missing);
    let provider = Arc::new(Provider::default());
    provider.artifact_findings.store(true, Ordering::SeqCst);
    provider.forged_evidence.store(true, Ordering::SeqCst);
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = host.execution_owner;
    let goal = feature_goal(&host, owner);
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "accomplished_not_achieved",
        false,
        evidence,
        cx,
    )
    .await;
    bounded_tick(host.clone(), cx).await;
    let failed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(failed.needs_assessment(host.service.goal_now()));
    assert!(
        failed.assessments[0]
            .result
            .as_ref()
            .expect("finished")
            .as_ref()
            .expect_err("durable failure")
            .contains("actually fetched")
    );
    assert!(
        failed.assessments[0]
            .result
            .as_ref()
            .expect("finished")
            .is_err(),
        "reference-only finding never becomes support"
    );
    assert!(matches!(
        failed.assessments[0].input.artifacts[0].outcome,
        GoalArtifactReadOutcome::Fetched { .. }
    ));
    assert!(matches!(
        failed.assessments[0].input.artifacts[1].outcome,
        GoalArtifactReadOutcome::Unavailable { .. }
    ));
    drop(host);
    provider.forged_evidence.store(false, Ordering::SeqCst);
    let host = open(dir.path(), provider.clone()).expect("restart");
    scheduled_tick(host.clone(), provider, cx).await;
    let recovered = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(recovered.assessments.len(), 2);
    assert_eq!(
        recovered.assessments[1]
            .result
            .as_ref()
            .expect("assessment finished")
            .as_ref()
            .ok()
            .expect("retry")
            .findings[0]
            .status,
        CriterionEvidenceStatus::Supported
    );
    assert_eq!(
        recovered
            .closure
            .expect("human unchanged")
            .input
            .choice
            .outcome(),
        Some(false)
    );
}

#[gpui::test]
async fn closure_without_read_approval_never_gains_local_access(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("directory");
    let artifact = dir.path().join("enabled.txt");
    std::fs::write(&artifact, "feature_status=enabled\n").expect("real bytes");
    let provider = Arc::new(Provider::default());
    provider.artifact_findings.store(true, Ordering::SeqCst);
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = host.execution_owner;
    let goal = feature_goal(&host, owner);
    let native = manifest(&artifact, ENABLED_DIGEST).remove(0);
    let closure = NativeGoalClosure {
        event_id: "historical-fixture".into(),
        expected_spec_digest: goal.spec_digest.clone(),
        source: GoalSource {
            interaction_id: "historical-native-choice".into(),
            session_id: "historical-session".into(),
            source: "historical fixture without file-sharing consent".into(),
        },
        occurred_at: Utc::now(),
        choice: HumanClosureChoice::AccomplishedAchieved,
        evidence_manifest: vec![GoalEvidence {
            id: native.id,
            source: native.source,
            locator: native.locator,
            digest: native.digest,
            reliability: native.reliability,
            oracle: native.oracle,
            limitation: native.limitation,
        }],
        artifact_access: None,
    };
    host.service
        .goal_close_native(goal.id, owner, closure)
        .expect("closure with no approval");
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(
        matches!(&goal.assessments[0].input.artifacts[0].outcome,GoalArtifactReadOutcome::Unavailable{reason,bytes_read:0,..} if reason.contains("not_approved"))
    );
    assert!(provider.requests.lock().expect("capture").is_empty());
    assert_eq!(
        goal.assessments[0]
            .result
            .as_ref()
            .expect("assessment finished")
            .as_ref()
            .ok()
            .expect("unknown")
            .findings[0]
            .status,
        CriterionEvidenceStatus::Unknown
    );
}

#[gpui::test]
async fn native_manifest_rejects_seventeen_artifacts_before_any_access(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("never-read.txt");
    std::fs::write(&path, "feature_status=enabled\n").expect("real file");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = host.execution_owner;
    let goal = feature_goal(&host, owner);
    for (forbidden, otherwise_valid_value) in [
        ("artifact_access", serde_json::json!("bounded_utf8_v1")),
        (
            "artifacts",
            serde_json::json!([{"evidence_id":"artifact-1", "observed_at":Utc::now(), "outcome":{"status":"fetched", "source":path, "sha256":ENABLED_DIGEST, "byte_count":"feature_status=enabled\n".len(), "content":"feature_status=enabled\n"}}]),
        ),
        ("actor", serde_json::json!(owner)),
        ("choice", serde_json::json!("accomplished_achieved")),
    ] {
        let mut input = serde_json::json!({"goal_id":goal.id, "evidence_manifest":[]});
        input[forbidden] = otherwise_valid_value;
        let error = serde_json::from_value::<agent::GoalCloseInput>(input)
            .expect_err("worker authority field must be refused");
        assert!(
            error.to_string().contains("unknown field") && error.to_string().contains(forbidden),
            "worker cannot supply {forbidden}: {error}"
        );
    }
    let evidence = (0..17)
        .map(|index| {
            let mut entry = manifest(&path, ENABLED_DIGEST).remove(0);
            entry.id = format!("evidence-{index}");
            entry
        })
        .collect();
    let (stream, _receiver) = agent::ToolCallEventStream::test_for_actor(owner);
    let id = goal.id.to_string();
    let host_for_task = host.clone();
    let task = cx.spawn(async move |mut cx| {
        agent::decide(id, evidence, false, stream, host_for_task, &mut cx).await
    });
    assert!(
        task.await.is_err(),
        "oversized manifest is refused before prompting or reading"
    );
    assert!(provider.requests.lock().expect("capture").is_empty());
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(goal.closure.is_none());
    assert!(goal.assessments.is_empty());
    let evidence = (0..16)
        .map(|index| {
            let mut entry = manifest(&path, ENABLED_DIGEST).remove(0);
            entry.id = format!("evidence-{index}");
            entry
        })
        .collect();
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "stop_unknown",
        false,
        evidence,
        cx,
    )
    .await;
    scheduled_tick(host.clone(), provider, cx).await;
    let accepted = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(
        accepted.assessments[0].input.artifacts.len(),
        16,
        "the disclosed legitimate boundary remains usable"
    );
    assert!(
        accepted.assessments[0]
            .input
            .artifacts
            .iter()
            .all(|artifact| matches!(artifact.outcome, GoalArtifactReadOutcome::Fetched { .. }))
    );
}

#[cfg(unix)]
#[gpui::test]
async fn regular_file_read_permissions_produce_real_fetched_or_unavailable_receipts(
    cx: &mut gpui::TestAppContext,
) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("restricted.txt");
    std::fs::write(&path, "feature_status=enabled\n").expect("fixture bytes");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
        .expect("restrict fixture");
    // Independent OS oracle: root/capability-bearing runners may legitimately
    // open mode-000 files; do not encode a false denial on those runners.
    let permitted_by_os = std::fs::File::open(&path).is_ok();
    eprintln!("artifact read-permission control: OS open permitted={permitted_by_os}");
    let provider = Arc::new(Provider::default());
    provider.artifact_findings.store(true, Ordering::SeqCst);
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = host.execution_owner;
    let goal = feature_goal(&host, owner);
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "stop_unknown",
        false,
        manifest(&path, ENABLED_DIGEST),
        cx,
    )
    .await;
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    let receipt = &goal.assessments[0].input.artifacts[0];
    if permitted_by_os {
        assert!(
            matches!(receipt.outcome, GoalArtifactReadOutcome::Fetched { .. }),
            "OS-authorized regular files remain readable"
        );
        assert_eq!(provider.requests.lock().expect("capture").len(), 1);
    } else {
        assert!(
            matches!(&receipt.outcome,GoalArtifactReadOutcome::Unavailable{reason,bytes_read:0,..} if reason.contains("safe_open_failed")),
            "real permission failure must be surfaced, not disguised as empty evidence"
        );
        assert!(provider.requests.lock().expect("capture").is_empty());
        assert_eq!(
            goal.assessments[0]
                .result
                .as_ref()
                .expect("assessment finished")
                .as_ref()
                .ok()
                .expect("finding")
                .findings[0]
                .status,
            CriterionEvidenceStatus::Unknown
        );
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("restore fixture permission");
}
