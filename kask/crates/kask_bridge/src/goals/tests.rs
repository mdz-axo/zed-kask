use super::*;
use agent_client_protocol::schema::v2 as acp;
use gpui::AppContext;
use hkask_types::{
    EmbeddingBatch, EmbeddingGenerationError, InferenceError, InferenceFuture, InferenceResult,
    InferenceUsage,
};
use hkask_types::{InferencePort, template::LLMParameters};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

mod drain;

#[derive(Default)]
struct Provider {
    fail: AtomicBool,
    block_assessor: AtomicBool,
    resume_assessor: Arc<tokio::sync::Notify>,
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
        let resume = self.resume_assessor.clone();
        let malformed = self.bad_findings.load(Ordering::SeqCst);
        let artifact_findings = self.artifact_findings.load(Ordering::SeqCst);
        let forged_evidence = self.forged_evidence.load(Ordering::SeqCst);
        Box::pin(async move {
            if blocked {
                resume.notified().await;
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
    open_for_owner(dir, provider, embedding_model, WebID::anonymous())
}
fn open_for_owner(
    dir: &std::path::Path,
    provider: Arc<dyn InferencePort>,
    embedding_model: &str,
    execution_owner: WebID,
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
        execution_owner,
        ledger,
        Some(memory),
        {
            let provider = provider.clone();
            move |messages, model| {
                let provider = provider.clone();
                Box::pin(async move {
                    provider
                        .generate_with_messages(
                            &messages,
                            &LLMParameters::default(),
                            Some(&model),
                            None,
                        )
                        .await
                })
            }
        },
        move |model, passage, dimensions| {
            let provider = provider.clone();
            Box::pin(async move {
                provider
                    .embed_with_dimensions(&model, &[passage], Some(dimensions))
                    .await
            })
        },
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
    agent::set_native_goal_host(Some(host.clone()));
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
    let (authorization, task) =
        match futures::future::select(task, Box::pin(receiver.expect_authorization())).await {
            futures::future::Either::Right((authorization, task)) => (authorization, task),
            futures::future::Either::Left((result, _)) => {
                panic!("native tool completed before ActionChoice: {result:?}")
            }
        };
    let before_selection = host
        .service
        .goal_get(goal.id, goal.owner)
        .expect("canonical read")
        .expect("goal");
    assert_eq!(
        serde_json::to_value(before_selection).expect("canonical state"),
        serde_json::to_value(goal).expect("state before prompt"),
        "presenting ActionChoice must not change canonical state"
    );
    assert_eq!(
        authorization.kind,
        acp_thread::AuthorizationKind::ActionChoice
    );
    let rendered = serde_json::to_string(&authorization.tool_call).expect("prompt JSON");
    assert!(rendered.contains(&goal.spec_digest));
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
        assert!(rendered.contains("Latest human outcome:"));
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
            let goals: Vec<_> = host
                .service
                .goal_scan_for_host()
                .expect("goals")
                .into_values()
                .flatten()
                .collect();
            let done = if provider.fail.load(Ordering::SeqCst) {
                provider.embedding_calls.load(Ordering::SeqCst) > before
            } else {
                goals.iter().all(|goal| {
                    !goal.needs_assessment(host.service.goal_now())
                        && goal.indexed_revision == goal.revision
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

mod artifacts;
mod model_identity;

#[gpui::test]
async fn native_stop_unknown_restart_recall_and_optional_human_forecast_corrections(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let provider = Arc::new(Provider::default());
    let mut host = open(dir.path(), provider.clone()).expect("host");
    let clock = Arc::new(std::sync::Mutex::new(Utc::now()));
    let host_mut = Arc::get_mut(&mut host).expect("sole host");
    host_mut.service = host_mut.service.clone().with_goal_clock({
        let clock = clock.clone();
        Arc::new(move || *clock.lock().expect("clock"))
    });
    let owner = host.execution_owner;
    let now = host.service.goal_now();
    let forecast = GoalForecast {
        proposition: "Human accepts original requirements".into(),
        probability: 0.8,
        horizon: now + chrono::Duration::seconds(1),
        resolution_deadline: now + chrono::Duration::seconds(10),
        execution_policy: "this task".into(),
        resources: "this session".into(),
        resolver: GoalForecastResolver::HumanAcceptance {},
        baseline_probability: 0.0,
        cohort: "features".into(),
        update_policy: ForecastUpdatePolicy::FixedAtDeclaration,
    };
    let goal = declare_with_forecast(&host, owner, Some(forecast)).expect("declare");
    *clock.lock().expect("clock") += chrono::Duration::seconds(2);
    decision(host.clone(), &goal, owner, "stop_unknown", false, cx).await;
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let closed = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("closed");
    assert_eq!(closed.forecast_status, ForecastStatus::Unknown);
    assert!(closed.scores.is_empty());
    assert_eq!(
        closed.closure.as_ref().expect("closure").input.choice,
        HumanClosureChoice::StopUnknown
    );
    assert!(provider.requests.lock().expect("capture").is_empty());
    *clock.lock().expect("clock") += chrono::Duration::seconds(20);
    decision(host.clone(), &closed, owner, "correct_achieved", true, cx).await;
    let corrected = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(corrected.scores.len(), 1);
    assert!(corrected.scores[0].outcome);
    decision(host.clone(), &corrected, owner, "correct_unknown", true, cx).await;
    scheduled_tick(host.clone(), provider.clone(), cx).await;
    let retained = host
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("goal");
    assert_eq!(retained.forecast_status, ForecastStatus::Unknown);
    assert_eq!(retained.scores.len(), 1);
    assert_eq!(retained.human_corrections.len(), 2);
    drop(host);
    let restarted = open(dir.path(), provider).expect("restart");
    let recovered = restarted
        .service
        .goal_get(goal.id, owner)
        .expect("get")
        .expect("canonical survives");
    assert_eq!(recovered, retained);
    let memory = restarted.memory.as_ref().expect("memory");
    let entity = format!("curator:goal:{}", goal.id);
    let rows = memory
        .h_mems_by_entity_prefix(&entity)
        .expect("exact recall");
    assert_eq!(rows.len(), 1, "one current recall record, no snapshot tail");
    assert_eq!(
        rows[0].value["human_corrections"]
            .as_array()
            .expect("history")
            .len(),
        2
    );
    let hits = memory
        .search_similar(
            &vec![0.5; hkask_storage::embedding_dim()],
            10,
            "fixture-embedding",
            Some("fixture-embedding"),
        )
        .expect("KNN");
    assert!(!hits.results.is_empty());
    for hit in hits.results {
        assert_eq!(hit.embedding.entity_ref, entity);
        assert!(
            rows.iter()
                .any(|row| hkask_memory::semantic_passage_for_h_mem(row)
                    == hit.embedding.passage_text),
            "KNN result joins the exact current memory passage"
        );
    }
}

#[gpui::test]
async fn native_invalid_choice_cannot_supply_human_authority(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider).expect("host");
    let owner = host.execution_owner;
    let goal = declare(&host, owner).expect("goal");
    let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(owner);
    let id = goal.id.to_string();
    let native_host = host.clone();
    let task = cx.spawn(async move |mut cx| {
        agent::decide(id, vec![], false, stream, native_host, &mut cx).await
    });
    receiver
        .expect_authorization()
        .await
        .response
        .send(acp_thread::SelectedPermissionOutcome::new(
            acp::PermissionOptionId::new("allow"),
            acp::PermissionOptionKind::AllowAlways,
        ))
        .expect("invalid generic grant");
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
async fn native_drainers_share_private_claim_and_late_result_cannot_replace_new_assessment(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("enabled.txt");
    std::fs::write(&path, "feature_status=enabled\n").expect("artifact");
    let provider = Arc::new(Provider::default());
    provider.block_assessor.store(true, Ordering::SeqCst);
    provider.artifact_findings.store(true, Ordering::SeqCst);
    let clock = Arc::new(std::sync::Mutex::new(Utc::now()));
    let mut first = open(dir.path(), provider.clone()).expect("first host");
    let first_mut = Arc::get_mut(&mut first).expect("sole host");
    first_mut.service = first_mut.service.clone().with_goal_clock({
        let clock = clock.clone();
        Arc::new(move || *clock.lock().expect("clock"))
    });
    let owner = first.execution_owner;
    let goal = declare(&first, owner).expect("goal");
    decision_with_manifest(
        first.clone(),
        &goal,
        owner,
        "accomplished_not_achieved",
        false,
        artifacts::manifest(&path, artifacts::ENABLED_DIGEST),
        cx,
    )
    .await;
    let second_provider = Arc::new(Provider::default());
    second_provider
        .artifact_findings
        .store(true, Ordering::SeqCst);
    let mut second = open(dir.path(), second_provider.clone()).expect("separate host connections");
    let second_mut = Arc::get_mut(&mut second).expect("sole host");
    second_mut.service = second_mut.service.clone().with_goal_clock({
        let clock = clock.clone();
        Arc::new(move || *clock.lock().expect("clock"))
    });
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let task = runtime.spawn(async move {
        let older = tokio::spawn({
            let first = first.clone();
            async move { first.drain_once().await }
        });
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while provider.requests.lock().expect("capture").is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "assessor did not start"
            );
            tokio::task::yield_now().await;
        }
        let frozen = first
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert_eq!(
            frozen.assessments[0].input.artifacts[0].outcome,
            serde_json::from_str::<GoalAssessmentInput>(
                &provider.requests.lock().expect("capture")[0][1].content
            )
            .expect("exact input")
            .artifacts[0]
                .outcome
        );
        second.drain_once().await.expect("live claim skipped");
        assert!(second_provider.requests.lock().expect("capture").is_empty());
        *clock.lock().expect("clock") += chrono::Duration::seconds(91);
        second.drain_once().await.expect("expiry retry");
        provider.resume_assessor.notify_one();
        older
            .await
            .expect("older drainer")
            .expect("late failure is surfaced, siblings continue");
        let retained = second
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert_eq!(retained.assessments.len(), 2);
        assert!(
            retained.assessments[0]
                .result
                .as_ref()
                .expect("expired")
                .is_err()
        );
        assert!(
            retained.assessments[1]
                .result
                .as_ref()
                .expect("current")
                .is_ok()
        );
        assert_eq!(
            retained
                .closure
                .expect("human label retained")
                .input
                .choice
                .outcome(),
            Some(false)
        );
    });
    cx.background_spawn(async move {
        task.await.expect("production concurrency path");
    })
    .await;
}

async fn mcp_declare(
    host: &GoalHost,
    owner: WebID,
    dir: &std::path::Path,
    source: GoalSource,
    cx: &mut gpui::TestAppContext,
) -> Goal {
    use hkask_mcp_kata_kanban::types::{GoalCreateRequest, GoalCriterionInput};
    use rmcp::handler::server::wrapper::Parameters;
    let idempotency = Arc::new(
        idempotency::IdempotencyStore::with_driver(host.ledger.driver().clone())
            .expect("idempotency schema"),
    );
    let server = KanbanServer::new(
        owner,
        KanbanService::new(host.ledger.clone()),
        Arc::new(hkask_mcp_swarm::LocalAgentRegistry::new(
            dir.to_str().expect("registry path"),
        )),
        Arc::new(test_support::UnavailableWorktreeSpawn),
        idempotency.clone(),
        idempotency,
    );
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let task = runtime.spawn(async move {
        let output = server
            .kanban_goal_create(Parameters(GoalCreateRequest {
                goal_text: "The user can close an MCP-created goal through native consent".into(),
                criteria: vec![GoalCriterionInput {
                    description: "Explicit native selection records closure".into(),
                }],
                source,
                forecast: None,
                supersedes: None,
                task_id: None,
                idempotency_key: None,
            }))
            .await
            .expect("public MCP create");
        test_support::parse(&output)
    });
    let response = cx
        .background_spawn(async move { task.await.expect("MCP create task") })
        .await;
    assert_eq!(response["owner"], owner.to_string());
    let id = response["id"]
        .as_str()
        .expect("MCP goal id")
        .parse()
        .expect("goal id");
    let goal = host
        .service
        .goal_get(id, owner)
        .expect("canonical read")
        .expect("MCP-created goal");
    assert_eq!(goal.owner, owner);
    goal
}

/// Crosses public MCP create and actual native tools with distinct identities.
#[gpui::test]
async fn mcp_created_goal_closes_from_distinct_native_session(cx: &mut gpui::TestAppContext) {
    let managed = crate::mcp_servers::BUILT_IN_MCP_SERVERS
        .iter()
        .find(|server| server.id == "kata-kanban")
        .expect("managed kanban");
    assert!(!managed.config_env.contains(&"HKASK_WEBID"));
    assert!(!managed.credentials.contains(&"HKASK_WEBID"));
    let owner = WebID::anonymous();
    let actor = WebID::for_agent_name("native-thread:goal-mcp-boundary-session");
    assert_ne!(owner, actor, "production identities must not be collapsed");
    let dir = tempfile::tempdir().expect("directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("file-backed host");
    let goal = mcp_declare(
        &host,
        owner,
        dir.path(),
        GoalSource {
            interaction_id: "worker-declared-turn".into(),
            session_id: "worker-declared-session".into(),
            source: "worker-declared-source".into(),
        },
        cx,
    )
    .await;
    decision(host.clone(), &goal, actor, "stop_unknown", false, cx).await;
    let closed = host
        .service
        .goal_get(goal.id, owner)
        .expect("read")
        .expect("goal");
    let closure = &closed
        .closure
        .as_ref()
        .expect("explicit selection closes")
        .input;
    assert_eq!(closure.choice, HumanClosureChoice::StopUnknown);
    assert_eq!(closure.source.session_id, format!("test-session:{actor}"));
    assert!(
        closure
            .source
            .interaction_id
            .starts_with(&format!("{actor}:"))
    );
    assert_eq!(closure.source.source, "native-action-choice");
    assert_ne!(
        closure.source, goal.source,
        "worker source cannot supply native provenance"
    );
    decision(host.clone(), &closed, actor, "correct_unknown", true, cx).await;
    scheduled_tick(host.clone(), provider, cx).await;
    let current = host
        .service
        .goal_get(goal.id, owner)
        .expect("read")
        .expect("goal");
    assert_eq!(current.human_corrections.len(), 1);
    assert_eq!(current.assessments.len(), 1);
    assert_eq!(current.indexed_revision, current.revision);
}

#[gpui::test]
async fn foreign_goal_denied_before_prompt_despite_forged_native_source(
    cx: &mut gpui::TestAppContext,
) {
    use futures::FutureExt;
    let dir = tempfile::tempdir().expect("directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("host");
    let foreign = WebID::for_agent_name("foreign-kanban-owner");
    let actor = WebID::for_agent_name("native-thread:goal-mcp-boundary-session");
    let (stream, mut receiver) = agent::ToolCallEventStream::test_for_actor(actor);
    let source = GoalSource {
        interaction_id: format!("{actor}:{}", stream.tool_call_id()),
        session_id: stream
            .calling_session_id()
            .expect("captured session")
            .into(),
        source: "native-action-choice".into(),
    };
    let goal = mcp_declare(&host, foreign, dir.path(), source.clone(), cx).await;
    let keepalive = stream.clone();
    let native_host = host.clone();
    let id = goal.id.to_string();
    let result = cx
        .spawn(async move |mut cx| {
            agent::decide(id, vec![], false, stream, native_host, &mut cx).await
        })
        .await;
    assert!(
        matches!(result, Err(agent::NativeGoalOutput::Error { error }) if error.contains("permission denied"))
    );
    assert!(
        receiver.expect_authorization().now_or_never().is_none(),
        "foreign goal must not prompt"
    );
    drop(keepalive);
    let foreign_host =
        open_for_owner(dir.path(), provider, "fixture-embedding", foreign).expect("foreign host");
    for correction in [false, true] {
        assert!(
            host.prepare(&goal.id.to_string(), foreign, &[], correction)
                .err()
                .expect("session actor cannot impersonate owner")
                .contains("permission denied")
        );
    }
    for decision in [Decision::StopUnknown, Decision::CorrectUnknown] {
        let mut prompt = foreign_host
            .prepare(&goal.id.to_string(), actor, &[], false)
            .expect("foreign host owns goal");
        prompt.supersedes_event_id = Some("forged-target".into());
        assert!(
            host.submit(NativeGoalSubmission {
                goal_id: goal.id.to_string(),
                actor: foreign,
                interaction_id: source.interaction_id.clone(),
                session_id: source.session_id.clone(),
                prompt,
                decision,
                evidence_manifest: vec![],
            })
            .expect_err("submit must independently enforce host owner")
            .contains("permission denied")
        );
    }
    let retained = host
        .service
        .goal_get(goal.id, foreign)
        .expect("foreign read")
        .expect("goal");
    assert_eq!(
        serde_json::to_value(retained).expect("retained state"),
        serde_json::to_value(goal).expect("original state"),
        "denial must not mutate the foreign goal"
    );
}

#[test]
fn native_goal_inputs_reject_worker_identity_and_source_fields() {
    let base = serde_json::json!({"goal_id": "fixture-goal"});
    assert!(serde_json::from_value::<agent::GoalCloseInput>(base.clone()).is_ok());
    assert!(serde_json::from_value::<agent::GoalCorrectInput>(base.clone()).is_ok());
    for field in ["actor", "owner", "source", "interaction_id", "session_id"] {
        let mut forged = base.clone();
        forged[field] = serde_json::json!("worker-forged");
        assert!(
            serde_json::from_value::<agent::GoalCloseInput>(forged.clone()).is_err(),
            "close rejects {field}"
        );
        assert!(
            serde_json::from_value::<agent::GoalCorrectInput>(forged).is_err(),
            "correction rejects {field}"
        );
    }
}

#[gpui::test]
async fn supported_goals_are_assessed_and_indexed_across_owners(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("file-backed host");
    let owner = host.execution_owner;
    let other_owner = WebID::new();
    let mut goals = Vec::new();
    for actor in [owner, other_owner] {
        let goal = declare(&host, actor).expect("supported declaration");
        let owner_host = open_for_owner(dir.path(), provider.clone(), "fixture-embedding", actor)
            .expect("host configured for this owner");
        decision(owner_host, &goal, actor, "stop_unknown", false, cx).await;
        goals.push((goal.id, actor));
    }
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let task = runtime.spawn(async move {
        host.drain_once().await.expect("production scan");
        for (id, actor) in goals {
            let retained = host
                .service
                .goal_get(id, actor)
                .expect("supported read")
                .expect("retained");
            assert_eq!(retained.assessments.len(), 1, "both owners assessed");
            assert!(
                retained.assessments[0]
                    .result
                    .as_ref()
                    .expect("finished")
                    .is_ok()
            );
            assert_eq!(
                retained.indexed_revision, retained.revision,
                "both owners indexed"
            );
            let rows = host
                .memory
                .as_ref()
                .expect("memory")
                .h_mems_by_entity_prefix(&format!("curator:goal:{id}"))
                .expect("recall");
            assert_eq!(rows.len(), 1);
        }
        assert!(
            provider.requests.lock().expect("capture").is_empty(),
            "unfetched evidence remains deterministic unknown"
        );
    });
    cx.background_spawn(async move {
        task.await.expect("production drainer test");
    })
    .await;
}
