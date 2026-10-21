use super::*;
use agent_client_protocol::schema::v2 as acp;
use gpui::AppContext;
use hkask_types::{
    EmbeddingBatch, EmbeddingGenerationError, InferenceError, InferenceFuture, InferenceResult,
    InferenceUsage,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct Provider {
    fail: AtomicBool,
    block_assessor: AtomicBool,
    bad_findings: AtomicBool,
    artifact_findings: AtomicBool,
    forged_evidence: AtomicBool,
    embedding_calls: AtomicUsize,
    requests: std::sync::Mutex<Vec<Vec<ChatMessage>>>,
}
impl InferencePort for Provider {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> InferenceFuture<'_> {
        Box::pin(async { Err(InferenceError::Generation("fresh messages required".into())) })
    }
    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        _: &LLMParameters,
        model: Option<&str>,
        tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> InferenceFuture<'_> {
        assert!(
            tools.is_none(),
            "assessor receives bounded snapshots, never executable tools"
        );
        assert_eq!(model, Some("fixture-assessor"));
        self.requests
            .lock()
            .expect("request capture")
            .push(messages.to_vec());
        let messages = messages.to_vec();
        let fail = self.fail.load(Ordering::SeqCst);
        let blocked = self.block_assessor.load(Ordering::SeqCst);
        let malformed = self.bad_findings.load(Ordering::SeqCst);
        let artifact_findings = self.artifact_findings.load(Ordering::SeqCst);
        let forged_evidence = self.forged_evidence.load(Ordering::SeqCst);
        Box::pin(async move {
            if blocked {
                return futures::future::pending().await;
            }
            if fail {
                return Err(InferenceError::Generation(
                    "injected assessor outage".into(),
                ));
            }
            let input: GoalAssessmentInput =
                serde_json::from_str(&messages[1].content).expect("blinded input");
            let data: serde_json::Value =
                serde_json::from_str(&messages[1].content).expect("request JSON");
            let observed = data
                .get("artifacts")
                .and_then(|v| v.as_array())
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row["outcome"]["status"] == "fetched")
                });
            let content = observed.and_then(|row| row["outcome"]["content"].as_str());
            let (status, ids) = if forged_evidence {
                ("supported", vec![serde_json::json!("missing-artifact")])
            } else if artifact_findings {
                match content {
                    Some("feature_status=enabled\n") => (
                        "supported",
                        vec![observed.expect("observed")["evidence_id"].clone()],
                    ),
                    Some("feature_status=disabled\n") => (
                        "contradicted",
                        vec![observed.expect("observed")["evidence_id"].clone()],
                    ),
                    _ => ("unknown", vec![]),
                }
            } else {
                ("unknown", vec![])
            };
            let findings: Vec<_> = input.criteria.iter().map(|criterion| serde_json::json!({"criterion_id":criterion.id,"status":status,"evidence_ids":ids,"note":"Finding about the fetched file's feature_status field only; no claim that a test ran"})).collect();
            let text = if malformed {
                serde_json::json!({"findings":[],"limitations":["Only supplied file contents are observed; execution provenance is not established"]})
            } else {
                serde_json::json!({"findings":findings,"limitations":["Only supplied file contents are observed; execution provenance is not established"]})
            };
            Ok(InferenceResult {
                text: text.to_string(),
                model: "fixture-assessor".into(),
                usage: InferenceUsage::default(),
                finish_reason: "stop".into(),
                tool_calls: vec![],
                reasoning: None,
                cost_usd: None,
            })
        })
    }
    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        dimensions: Option<u32>,
    ) -> hkask_types::EmbedFuture<'a> {
        self.embedding_calls.fetch_add(1, Ordering::SeqCst);
        let model = model.to_string();
        let count = texts.len();
        let fail = self.fail.load(Ordering::SeqCst);
        Box::pin(async move {
            if fail {
                return Err(EmbeddingGenerationError::Connection(
                    "injected semantic outage".into(),
                ));
            }
            Ok(EmbeddingBatch {
                vectors: vec![
                    vec![0.5; dimensions.expect("production supplies width") as usize];
                    count
                ],
                requested_model: model.clone(),
                actual_model: Some(model),
                usage: InferenceUsage::default(),
                cost_usd: None,
            })
        })
    }
}
fn open(dir: &std::path::Path, provider: Arc<Provider>) -> anyhow::Result<Arc<GoalHost>> {
    open_with_provider(dir, provider, "fixture-embedding")
}
fn open_with_provider(
    dir: &std::path::Path,
    provider: Arc<dyn InferencePort>,
    embedding_model: &str,
) -> anyhow::Result<Arc<GoalHost>> {
    let ledger_path = dir.join("kanban.db");
    let pool = hkask_storage::SqliteDriver::file_pool_with_schema(
        ledger_path.to_str().expect("test path"),
    )?;
    let ledger = HMemStore::from_driver(Arc::new(hkask_storage::SqliteDriver::new(pool)))?;
    let memory = Arc::new(hkask_memory::MemoryStore::open(
        dir.join("curator.db").to_str().expect("test path"),
        "goal-host-fixture",
        hkask_storage::embedding_dim(),
    )?);
    Ok(Arc::new(GoalHost::new(
        ledger,
        Some(memory),
        provider,
        "fixture-assessor".into(),
        embedding_model.into(),
        hkask_storage::embedding_dim() as u32,
    )))
}
fn declare(host: &GoalHost, owner: WebID) -> anyhow::Result<Goal> {
    declare_with_forecast(host, owner, None)
}
fn declare_with_forecast(
    host: &GoalHost,
    owner: WebID,
    forecast: Option<GoalForecast>,
) -> anyhow::Result<Goal> {
    Ok(host.service.goal_create(
        GoalDeclaration {
            goal_text: "The user can retrieve the original frozen requirements after restart"
                .into(),
            criteria: vec![
                VerificationCriterion::new(
                    "Exact recall retains the complete frozen criterion".into(),
                ),
                VerificationCriterion::new(
                    "Semantic recall returns the same canonical goal identity".into(),
                ),
            ],
            task_id: None,
            source: GoalSource {
                interaction_id: "original-user-turn".into(),
                session_id: "native-test".into(),
                source: "test-human-specification".into(),
            },
            forecast,
            supersedes: None,
        },
        owner,
    )?)
}
fn supplied_artifact_manifest(dir: &std::path::Path) -> Vec<agent::NativeGoalEvidence> {
    let path = dir.join("assessor-evidence.txt");
    std::fs::write(&path, "feature_status=enabled\n").expect("real supplied bytes");
    artifacts::manifest(&path, artifacts::ENABLED_DIGEST)
}
async fn decision(
    host: Arc<GoalHost>,
    goal: &Goal,
    owner: WebID,
    selection: &str,
    correction: bool,
    cx: &mut gpui::TestAppContext,
) -> agent::NativeGoalOutput {
    let evidence = if correction {
        vec![]
    } else {
        vec![agent::NativeGoalEvidence {
            id: "worker-evidence".into(),
            source: "worker".into(),
            locator: "unfetched://test-result".into(),
            digest: "claimed-digest".into(),
            reliability: "worker claim only".into(),
            oracle: "claimed command output".into(),
            limitation: "not independently fetched".into(),
        }]
    };
    decision_with_manifest(host, goal, owner, selection, correction, evidence, cx).await
}
async fn decision_with_manifest(
    host: Arc<GoalHost>,
    goal: &Goal,
    owner: WebID,
    selection: &str,
    correction: bool,
    evidence: Vec<agent::NativeGoalEvidence>,
    cx: &mut gpui::TestAppContext,
) -> agent::NativeGoalOutput {
    let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(owner);
    let goal_id = goal.id.to_string();
    let expected_locators: Vec<_> = if correction {
        goal.closure
            .as_ref()
            .expect("closed goal")
            .input
            .evidence_manifest
            .iter()
            .map(|e| e.locator.clone())
            .collect()
    } else {
        evidence.iter().map(|e| e.locator.clone()).collect()
    };
    use agent::AgentTool;
    static NATIVE_HOOK: futures::lock::Mutex<()> = futures::lock::Mutex::new(());
    cx.executor().allow_parking();
    let _serial = NATIVE_HOOK.lock().await;
    struct HookGuard;
    impl Drop for HookGuard {
        fn drop(&mut self) {
            agent::set_native_goal_host(None);
        }
    }
    agent::set_native_goal_host(Some(host));
    let _hook = HookGuard;
    let task = cx.update(|cx| {
        if correction {
            Arc::new(agent::GoalCorrectTool).run(
                agent::ToolInput::resolved(agent::GoalCorrectInput { goal_id }),
                stream,
                cx,
            )
        } else {
            Arc::new(agent::GoalCloseTool).run(
                agent::ToolInput::resolved(agent::GoalCloseInput {
                    goal_id,
                    evidence_manifest: evidence,
                }),
                stream,
                cx,
            )
        }
    });
    let authorization = receiver.expect_authorization().await;
    assert_eq!(
        authorization.kind,
        acp_thread::AuthorizationKind::ActionChoice
    );
    let rendered = serde_json::to_string(&authorization.tool_call).expect("prompt JSON");
    assert!(rendered.contains(&goal.spec_digest));
    if let Some(GoalForecast {
        resolver:
            GoalForecastResolver::MachineArtifactDigest {
                locator,
                expected_sha256,
            },
        ..
    }) = &goal.forecast
    {
        assert!(
            rendered.contains(locator) && rendered.contains(expected_sha256),
            "native approval discloses the exact resolver source and oracle even without a manifest entry"
        );
        assert!(rendered.contains("separate from assessment manifest"));
        if correction {
            assert!(rendered.contains("This correction grants no scheduled source access"));
        } else {
            assert!(rendered.contains("ONE scheduled bounded local UTF-8 regular-file read"));
        }
        assert!(rendered.contains("File bytes not execution authenticity"));
    }
    for criterion in &goal.criteria {
        assert!(
            rendered.contains(&criterion.description),
            "human sees each original frozen criterion"
        );
    }
    for locator in expected_locators {
        assert!(
            rendered.contains(&locator),
            "native decision must show the reviewed evidence locator"
        );
    }
    if correction {
        assert!(rendered.contains("Latest human outcome: unknown"));
    }
    let acp_thread::PermissionOptions::Flat(options) = &authorization.options else {
        panic!("explicit flat choices required")
    };
    assert_eq!(options.len(), if correction { 3 } else { 6 });
    assert!(options.iter().any(|option| option.option_id.0.as_ref()
        == if correction {
            "correct_unknown"
        } else {
            "stop_unknown"
        }));
    authorization
        .response
        .send(acp_thread::SelectedPermissionOutcome::new(
            acp::PermissionOptionId::new(selection.to_string()),
            acp::PermissionOptionKind::AllowAlways,
        ))
        .expect("actual host interaction callback");
    task.await.expect("native selection recorded")
}
async fn scheduled_tick(
    host: Arc<GoalHost>,
    provider: Arc<Provider>,
    cx: &mut gpui::TestAppContext,
) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let task = runtime.spawn(async move {
        let before = provider.embedding_calls.load(Ordering::SeqCst);
        let run = tokio::spawn(host.clone().run());
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let goals = host
                .service
                .goal_list(&host.owners().expect("owners")[0])
                .expect("goals");
            let done = if provider.fail.load(Ordering::SeqCst) {
                provider.embedding_calls.load(Ordering::SeqCst) > before
            } else {
                goals.iter().all(|goal| {
                    !goal.assessment_pending
                        && goal
                            .projections
                            .iter()
                            .all(|p| p.delivered_receipt.is_some())
                })
            };
            if done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "scheduled drainer did not reach expected state"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        run.abort();
        assert!(
            run.await
                .expect_err("drainer aborted after tick")
                .is_cancelled()
        );
    });
    cx.background_spawn(async move { task.await.expect("production scheduled tick") })
        .await;
}

#[gpui::test]
async fn native_goal_restart_drains_blinded_assessment_and_exact_semantic_projection(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    provider.fail.store(true, Ordering::SeqCst);
    let owner = WebID::new();
    let host = open(dir.path(), provider.clone()).expect("file-backed host");
    let goal = declare(&host, owner).expect("declare original requirement");
    assert!(
        serde_json::from_value::<agent::GoalCloseInput>(
            serde_json::json!({"goal_id":goal.id,"choice":"accomplished_achieved","actor":"human"})
        )
        .is_err(),
        "worker cannot submit a human label"
    );
    let _recorded = decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "stop_unknown",
        false,
        supplied_artifact_manifest(dir.path()),
        cx,
    )
    .await;
    let closed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(
        closed.closure.as_ref().expect("closure").input.choice,
        HumanClosureChoice::StopUnknown
    );
    assert!(closed.assessment_pending);
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let failed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(
        failed.assessments[0]
            .failure
            .as_ref()
            .expect("durable failure")
            .reason
            .contains("injected assessor outage")
    );
    assert!(
        failed
            .projections
            .iter()
            .all(|p| p.delivered_receipt.is_none())
    );
    drop(host);
    provider.fail.store(false, Ordering::SeqCst);
    let host = open(dir.path(), provider.clone()).expect("restart host and both stores");
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("canonical record never pruned");
    assert_eq!(goal.assessments.len(), 2);
    assert!(!goal.assessment_pending);
    let completion = goal.assessments[1]
        .completion
        .as_ref()
        .expect("assessment complete");
    assert_eq!(completion.findings.len(), 2);
    assert!(
        completion
            .findings
            .iter()
            .all(|f| f.status == CriterionEvidenceStatus::Unknown)
    );
    assert_eq!(
        goal.closure.as_ref().expect("closure").input.choice,
        HumanClosureChoice::StopUnknown
    );
    for messages in provider.requests.lock().expect("captures").iter() {
        assert_eq!(messages.len(), 2, "fresh context");
        let data: serde_json::Value = serde_json::from_str(&messages[1].content).expect("JSON");
        assert_eq!(data.as_object().expect("object").len(), 6);
        for key in [
            "closure",
            "choice",
            "verdicts",
            "forecast",
            "probability",
            "scores",
            "human_corrections",
        ] {
            assert!(data.get(key).is_none(), "blinded request leaked {key}");
        }
    }
    let memory = host.memory.as_ref().expect("projection store");
    let entity = format!("curator:goal:{}", goal.id);
    let rows = memory
        .h_mems_by_entity_prefix(&entity)
        .expect("exact recall");
    assert_eq!(
        rows.len(),
        goal.projections.len(),
        "no hand-inserted memory or duplicate retries"
    );
    for projection in &goal.projections {
        let row = rows
            .iter()
            .find(|r| r.attribute == format!("projection:{}", projection.event_id))
            .expect("event recall");
        assert_eq!(
            row.value, projection.payload,
            "complete original payload retained"
        );
        assert_eq!(
            projection.delivered_receipt.as_deref(),
            Some(row.id.to_string().as_str())
        );
        assert_eq!(
            row.confidence.value(),
            0.5,
            "outcomes never mutate memory confidence"
        );
    }
    let hits = memory
        .search_similar(
            &vec![0.5; hkask_storage::embedding_dim()],
            100,
            "fixture-embedding",
            Some("fixture-embedding"),
        )
        .expect("semantic recall");
    assert!(!hits.results.is_empty());
    for hit in hits.results {
        assert_eq!(hit.embedding.entity_ref, entity);
        assert!(
            rows.iter()
                .any(|row| hkask_memory::semantic_passage_for_h_mem(row)
                    == hit.embedding.passage_text),
            "semantic result joins the exact h_mem"
        );
    }
    let count = rows.len();
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    assert_eq!(
        memory
            .h_mems_by_entity_prefix(&entity)
            .expect("replay recall")
            .len(),
        count
    );
    let _corrected = decision(host.clone(), &goal, owner, "correct_achieved", true, cx).await;
    let corrected = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(corrected.human_corrections.len(), 1);
    assert_eq!(corrected.human_corrections[0].outcome, Some(true));
    assert_eq!(
        corrected.closure, goal.closure,
        "correction never rewrites original closure"
    );
}

#[gpui::test]
async fn native_goal_invalid_choice_cannot_authorize_a_closure(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("test directory");
    let host = open(dir.path(), Arc::new(Provider::default())).expect("host");
    let owner = WebID::new();
    let goal = declare(&host, owner).expect("goal");
    let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(owner);
    let id = goal.id.to_string();
    let host_for_task = host.clone();
    let task = cx.spawn(async move |mut cx| {
        agent::decide(id, vec![], false, stream, host_for_task, &mut cx).await
    });
    let prompt = receiver.expect_authorization().await;
    assert_eq!(prompt.kind, acp_thread::AuthorizationKind::ActionChoice);
    prompt
        .response
        .send(acp_thread::SelectedPermissionOutcome::new(
            acp::PermissionOptionId::new("allow"),
            acp::PermissionOptionKind::AllowAlways,
        ))
        .expect("callback");
    assert!(task.await.is_err());
    assert!(
        host.service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal")
            .closure
            .is_none()
    );
}

#[gpui::test]
async fn native_goal_restart_after_publication_before_ack_reuses_exact_receipt(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    let owner = WebID::new();
    let host = open(dir.path(), provider.clone()).expect("host");
    let goal = declare(&host, owner).expect("goal");
    decision(
        host.clone(),
        &goal,
        owner,
        "accomplished_achieved",
        false,
        cx,
    )
    .await;
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let host_for_task = host.clone();
    let task = runtime.spawn(async move {
        let mut work = Box::pin(host_for_task.drain_once());
        assert!(
            futures::poll!(work.as_mut()).is_pending(),
            "production drainer yields after durable publication before receipt"
        );
        drop(work); // Model a host interruption at the actual publication/ack boundary.
    });
    cx.background_spawn(async move { task.await.expect("interrupt production drainer") })
        .await;
    let entity = format!("curator:goal:{}", goal.id);
    let rows = host
        .memory
        .as_ref()
        .expect("memory")
        .h_mems_by_entity_prefix(&entity)
        .expect("published row");
    assert_eq!(rows.len(), 1);
    let original_id = rows[0].id;
    assert!(
        host.service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal")
            .projections
            .iter()
            .all(|p| p.delivered_receipt.is_none())
    );
    drop(host);
    let host = open(dir.path(), provider.clone()).expect("reopen after interruption");
    scheduled_tick(host.clone(), provider, cx).await;
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("canonical goal retained");
    let rows = host
        .memory
        .as_ref()
        .expect("memory")
        .h_mems_by_entity_prefix(&entity)
        .expect("recall");
    assert_eq!(rows.len(), goal.projections.len());
    assert_eq!(
        rows.iter().filter(|r| r.id == original_id).count(),
        1,
        "replay does not replace or duplicate the already-published event"
    );
    assert_eq!(
        goal.projections[0].delivered_receipt,
        Some(original_id.to_string())
    );
}

#[gpui::test]
async fn native_goal_missing_criterion_is_failed_retryable_not_passed(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    provider.bad_findings.store(true, Ordering::SeqCst);
    let owner = WebID::new();
    let host = open(dir.path(), provider.clone()).expect("host");
    let goal = declare(&host, owner).expect("goal");
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "accomplished_not_achieved",
        false,
        supplied_artifact_manifest(dir.path()),
        cx,
    )
    .await;
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let host_for_task = host.clone();
    let task = runtime.spawn(async move {
        host_for_task.drain_once().await.expect("scan");
    });
    cx.background_spawn(async move { task.await.expect("bounded drain") })
        .await;
    let failed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(failed.assessment_pending);
    assert!(failed.assessments[0].completion.is_none());
    assert!(
        failed.assessments[0].failure.is_some(),
        "missing exact criterion coverage is a durable failure"
    );
    provider.bad_findings.store(false, Ordering::SeqCst);
    drop(host);
    let host = open(dir.path(), provider.clone()).expect("restart");
    scheduled_tick(host.clone(), provider, cx).await;
    let recovered = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(recovered.assessments.len(), 2);
    assert!(recovered.assessments[1].completion.is_some());
    assert_eq!(
        recovered.closure.expect("closure").input.choice,
        HumanClosureChoice::AccomplishedNotAchieved
    );
}

#[gpui::test]
async fn native_goal_empty_evidence_keeps_assessment_unknown_but_declared_human_forecast_scores(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = WebID::new();
    let now = Utc::now();
    let goal = declare_with_forecast(
        &host,
        owner,
        Some(GoalForecast {
            proposition: "The human accepts the original goal under the declared execution policy"
                .into(),
            probability: 0.8,
            horizon: now + Duration::milliseconds(50),
            resolution_deadline: now + Duration::minutes(5),
            execution_policy: "Native host integration test".into(),
            resources: "Disposable file-backed fixture".into(),
            resolver: GoalForecastResolver::HumanAcceptance,
            baseline_probability: 0.5,
            cohort: "host-integration-fixture".into(),
            update_policy: ForecastUpdatePolicy::FixedAtDeclaration,
        }),
    )
    .expect("prospective forecast");
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let wait = runtime.spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    });
    cx.background_spawn(async move { wait.await.expect("horizon elapsed") })
        .await;
    let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(owner);
    let host_for_task = host.clone();
    let id = goal.id.to_string();
    let task = cx.spawn(async move |mut cx| {
        agent::decide(id, vec![], false, stream, host_for_task, &mut cx).await
    });
    let prompt = receiver.expect_authorization().await;
    assert_eq!(prompt.kind, acp_thread::AuthorizationKind::ActionChoice);
    prompt
        .response
        .send(acp_thread::SelectedPermissionOutcome::new(
            acp::PermissionOptionId::new("accomplished_achieved"),
            acp::PermissionOptionKind::AllowOnce,
        ))
        .expect("human selection");
    task.await.expect("closure");
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    assert!(
        provider.requests.lock().expect("capture").is_empty(),
        "empty evidence is decided as unknown without an assessor model request"
    );
    let goal = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert!(
        goal.closure
            .as_ref()
            .expect("closure")
            .input
            .evidence_manifest
            .is_empty()
    );
    assert!(
        goal.assessments[0]
            .completion
            .as_ref()
            .expect("assessment")
            .findings
            .iter()
            .all(|f| f.status == CriterionEvidenceStatus::Unknown)
    );
    assert_eq!(goal.forecast_status, ForecastStatus::Resolved);
    assert_eq!(goal.scores.len(), 1);
    assert!((goal.scores[0].brier - 0.04).abs() < 1e-12);
    assert_eq!(goal.scores[0].baseline_brier, 0.25);
}

#[gpui::test]
async fn native_goal_expired_assessment_lease_resumes_without_reopening_work(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("host");
    let owner = WebID::new();
    let goal = declare(&host, owner).expect("goal");
    decision(host.clone(), &goal, owner, "stop_not_achieved", false, cx).await;
    host.service
        .goal_claim_assessment(
            goal.id,
            owner,
            AssessmentLeaseRequest {
                attempt_id: "interrupted-host".into(),
                assessor_id: "host-blinded-assessor".into(),
                model_version: "fixture-assessor".into(),
                tool_version: "fixture".into(),
                lease_until: Utc::now() + Duration::seconds(2),
            },
            Vec::new(),
        )
        .expect("persist in-flight lease before simulated restart");
    drop(host);
    let host = open(dir.path(), provider.clone()).expect("restart");
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let host_for_task = host.clone();
    let provider_for_task = provider.clone();
    let task = runtime.spawn(async move {
        host_for_task
            .drain_once()
            .await
            .expect("startup scan with live lease");
        assert!(
            provider_for_task
                .requests
                .lock()
                .expect("capture")
                .is_empty(),
            "startup must not steal a live lease"
        );
        tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
        host_for_task
            .drain_once()
            .await
            .expect("next scheduled work scan after expiration");
    });
    cx.background_spawn(async move { task.await.expect("lease recovery") })
        .await;
    let recovered = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(recovered.assessments.len(), 2);
    assert!(
        recovered.assessments[0].failure.is_some(),
        "expired attempt remains failure history"
    );
    assert!(recovered.assessments[1].completion.is_some());
    assert_eq!(
        recovered.closure.expect("closure retained").input.choice,
        HumanClosureChoice::StopNotAchieved
    );
}

#[gpui::test]
async fn native_goal_missing_assessor_and_projection_store_retains_a_durable_failure(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    let configured = open(dir.path(), provider.clone()).expect("canonical ledger");
    let host = Arc::new(GoalHost::new(
        configured.ledger.clone(),
        None,
        provider.clone(),
        String::new(),
        "fixture-embedding".into(),
        hkask_storage::embedding_dim() as u32,
    ));
    let owner = WebID::new();
    let goal = declare(&host, owner).expect("goal");
    decision_with_manifest(
        host.clone(),
        &goal,
        owner,
        "stop_unknown",
        false,
        supplied_artifact_manifest(dir.path()),
        cx,
    )
    .await;
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let host_for_task = host.clone();
    let task = runtime.spawn(async move {
        host_for_task.drain_once().await.expect("scan");
    });
    cx.background_spawn(async move { task.await.expect("bounded drain") })
        .await;
    let pending = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("canonical goal retained");
    assert!(
        pending.closure.is_some(),
        "missing background capabilities never block human closure"
    );
    assert!(pending.assessment_pending);
    assert_eq!(pending.assessments.len(), 1);
    assert!(
        pending.assessments[0]
            .failure
            .as_ref()
            .expect("persisted configuration failure")
            .reason
            .contains("kask.models.classifier_model")
    );
    assert!(
        pending
            .projections
            .iter()
            .all(|projection| projection.delivered_receipt.is_none())
    );
    assert!(
        provider.requests.lock().expect("capture").is_empty(),
        "an unconfigured marker is never sent as a provider model"
    );
}

mod artifacts;

mod model_identity;

mod machine_digest;
