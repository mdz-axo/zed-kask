use super::*;
use agent_client_protocol::schema::v2 as acp;
use gpui::AppContext;
use hkask_types::{
    EmbeddingBatch, EmbeddingGenerationError, InferenceError, InferenceFuture, InferenceResult,
    InferenceUsage,
};
use hkask_types::{InferencePort, template::LLMParameters};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
            let goals = host
                .service
                .goal_list(&host.owners().expect("owners")[0])
                .expect("goals")
                .goals;
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
    let owner = WebID::new();
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
    let owner = WebID::new();
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
    let owner = WebID::new();
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

// Observe actual drainer warnings without adding another logging dependency.
struct RejectionWarnings(Arc<std::sync::Mutex<Vec<String>>>);
impl tracing::Subscriber for RejectionWarnings {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = String::new();
        event.record(
            &mut |field: &tracing::field::Field, value: &dyn std::fmt::Debug| {
                use std::fmt::Write;
                write!(&mut fields, "{field}={value:?};").expect("string formatting");
            },
        );
        self.0.lock().expect("warnings").push(fields);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[gpui::test]
async fn unsupported_goal_warns_without_blocking_same_or_other_owner_draining(
    cx: &mut gpui::TestAppContext,
) {
    use tracing::instrument::WithSubscriber;
    let dir = tempfile::tempdir().expect("directory");
    let provider = Arc::new(Provider::default());
    let host = open(dir.path(), provider.clone()).expect("file-backed host");
    let owner = WebID::new();
    let other_owner = WebID::new();
    let unsupported_id = hkask_types::GoalID::new();
    let raw = format!(
        " {{ \"schema_version\": 2, \"id\": \"{unsupported_id}\", \"history\": \"retain bytes\" }}\n"
    );
    let row = HMem::new(
        "kanban:goal",
        &unsupported_id.to_string(),
        serde_json::from_str(&raw).expect("unsupported JSON"),
        owner,
    );
    host.ledger.insert(&row).expect("unsupported row");
    host.ledger
        .driver()
        .sqlite_pool()
        .expect("pool")
        .get()
        .expect("connection")
        .execute(
            "UPDATE hmems SET value = ?1 WHERE id = ?2",
            [&raw, &row.id.to_string()],
        )
        .expect("retain noncanonical JSON whitespace");
    let mut goals = Vec::new();
    for actor in [owner, other_owner] {
        let goal = declare(&host, actor).expect("supported declaration");
        decision(host.clone(), &goal, actor, "stop_unknown", false, cx).await;
        goals.push((goal.id, actor));
    }
    let warnings = Arc::new(std::sync::Mutex::new(Vec::new()));
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let captured = warnings.clone();
    let task = runtime.spawn(async move {
        host.drain_once()
            .with_subscriber(RejectionWarnings(captured))
            .await
            .expect("unsupported row cannot abort the production scan");
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
            host.service
                .goal_get(unsupported_id, owner)
                .expect_err("direct unsupported read")
                .to_string()
                .contains("expected version 3")
        );
        let retained: String = host
            .ledger
            .driver()
            .sqlite_pool()
            .expect("pool")
            .get()
            .expect("connection")
            .query_row(
                "SELECT value FROM hmems WHERE id = ?1",
                [row.id.to_string()],
                |row| row.get(0),
            )
            .expect("raw bytes");
        assert_eq!(retained, raw, "unsupported row stays byte-identical");
        assert!(
            provider.requests.lock().expect("capture").is_empty(),
            "unfetched evidence remains deterministic unknown"
        );
    });
    cx.background_spawn(async move {
        task.await.expect("production drainer test");
    })
    .await;
    let warnings = warnings.lock().expect("warnings");
    assert!(
        warnings
            .iter()
            .any(|message| message.contains(&unsupported_id.to_string())
                && message.contains("expected version 3")),
        "rejection identity and reason must be logged: {warnings:?}"
    );
}
