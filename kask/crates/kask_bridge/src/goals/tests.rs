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
    bad_findings: AtomicBool,
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
            "no independent evidence-fetch capability is present"
        );
        assert_eq!(model, Some("fixture-assessor"));
        self.requests
            .lock()
            .expect("request capture")
            .push(messages.to_vec());
        let messages = messages.to_vec();
        let fail = self.fail.load(Ordering::SeqCst);
        let malformed = self.bad_findings.load(Ordering::SeqCst);
        Box::pin(async move {
            if fail {
                return Err(InferenceError::Generation(
                    "injected assessor outage".into(),
                ));
            }
            let input: GoalAssessmentInput =
                serde_json::from_str(&messages[1].content).expect("blinded input");
            let findings: Vec<_> = input.criteria.iter().map(|criterion| serde_json::json!({"criterion_id":criterion.id,"status":"unknown","evidence_ids":[],"note":"No independent evidence access"})).collect();
            let text = if malformed {
                serde_json::json!({"findings":[],"limitations":["no evidence access"]})
            } else {
                serde_json::json!({"findings":findings,"limitations":["no evidence access"]})
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
        "fixture-embedding".into(),
        hkask_storage::embedding_dim() as u32,
    )))
}
fn declare(host: &GoalHost, owner: WebID) -> anyhow::Result<Goal> {
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
            forecast: None,
            supersedes: None,
        },
        owner,
    )?)
}
async fn decision(
    host: Arc<GoalHost>,
    goal: &Goal,
    owner: WebID,
    selection: &str,
    correction: bool,
    cx: &mut gpui::TestAppContext,
) -> agent::NativeGoalOutput {
    let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(owner);
    let goal_id = goal.id.to_string();
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
    let task = cx.spawn(async move |mut cx| {
        agent::decide(goal_id, evidence, correction, stream, host, &mut cx).await
    });
    let authorization = receiver.expect_authorization().await;
    assert_eq!(
        authorization.kind,
        acp_thread::AuthorizationKind::ActionChoice
    );
    let rendered = serde_json::to_string(&authorization.tool_call).expect("prompt JSON");
    assert!(rendered.contains(&goal.spec_digest));
    assert!(rendered.contains("Exact recall retains the complete frozen criterion"));
    if !correction {
        assert!(rendered.contains("unfetched://test-result"));
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
    let _recorded = decision(host.clone(), &goal, owner, "stop_unknown", false, cx).await;
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
        assert_eq!(data.as_object().expect("object").len(), 5);
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
async fn native_goal_restart_after_publication_before_ack_reuses_exact_receipt(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    let owner = WebID::new();
    let host = open(dir.path(), provider.clone()).expect("host");
    let goal = declare(&host, owner).expect("goal");
    decision(host.clone(), &goal, owner, "accomplished_achieved", false, cx).await;
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let host_for_task = host.clone();
    let task = runtime.spawn(async move {
        let mut work = Box::pin(host_for_task.drain_once());
        assert!(futures::poll!(work.as_mut()).is_pending(), "production drainer yields after durable publication before receipt");
        drop(work); // Model a host interruption at the actual publication/ack boundary.
    });
    cx.background_spawn(async move { task.await.expect("interrupt production drainer") }).await;
    let entity = format!("curator:goal:{}", goal.id);
    let rows = host.memory.as_ref().expect("memory").h_mems_by_entity_prefix(&entity).expect("published row");
    assert_eq!(rows.len(), 1);
    let original_id = rows[0].id;
    assert!(host.service.goal_get(goal.id, owner).expect("get").expect("goal").projections.iter().all(|p|p.delivered_receipt.is_none()));
    drop(host);
    let host = open(dir.path(), provider.clone()).expect("reopen after interruption");
    scheduled_tick(host.clone(), provider, cx).await;
    let goal = host.service.goal_get(goal.id, owner).expect("get").expect("canonical goal retained");
    let rows = host.memory.as_ref().expect("memory").h_mems_by_entity_prefix(&entity).expect("recall");
    assert_eq!(rows.len(), goal.projections.len());
    assert_eq!(rows.iter().filter(|r|r.id==original_id).count(),1,"replay does not replace or duplicate the already-published event");
    assert_eq!(goal.projections[0].delivered_receipt, Some(original_id.to_string()));
}

#[gpui::test]
async fn native_goal_missing_criterion_is_failed_retryable_not_passed(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("test directory");
    let provider = Arc::new(Provider::default());
    provider.bad_findings.store(true, Ordering::SeqCst);
    let owner = WebID::new();
    let host = open(dir.path(), provider.clone()).expect("host");
    let goal = declare(&host, owner).expect("goal");
    decision(host.clone(), &goal, owner, "accomplished_not_achieved", false, cx).await;
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let host_for_task = host.clone();
    let task = runtime.spawn(async move { host_for_task.drain_once().await.expect("scan"); });
    cx.background_spawn(async move { task.await.expect("bounded drain") }).await;
    let failed = host.service.goal_get(goal.id, owner).expect("get").expect("goal");
    assert!(failed.assessment_pending);
    assert!(failed.assessments[0].completion.is_none());
    assert!(failed.assessments[0].failure.is_some(), "missing exact criterion coverage is a durable failure");
    provider.bad_findings.store(false, Ordering::SeqCst);
    drop(host);
    let host = open(dir.path(), provider.clone()).expect("restart");
    scheduled_tick(host.clone(), provider, cx).await;
    let recovered = host.service.goal_get(goal.id, owner).expect("get").expect("goal");
    assert_eq!(recovered.assessments.len(), 2);
    assert!(recovered.assessments[1].completion.is_some());
    assert_eq!(recovered.closure.expect("closure").input.choice, HumanClosureChoice::AccomplishedNotAchieved);
}
