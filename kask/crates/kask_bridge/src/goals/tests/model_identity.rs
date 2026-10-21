use super::*;

struct Producer {
    actual: Option<&'static str>,
    wrong_requested: bool,
    axis: usize,
    calls: AtomicUsize,
}
impl Producer {
    fn new(actual: Option<&'static str>, axis: usize) -> Arc<Self> {
        Arc::new(Self {
            actual,
            wrong_requested: false,
            axis,
            calls: AtomicUsize::new(0),
        })
    }
    fn vector(&self) -> Vec<f32> {
        let mut vector = vec![0.0; hkask_storage::embedding_dim()];
        vector[self.axis] = 1.0;
        vector
    }
}
impl InferencePort for Producer {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> InferenceFuture<'_> {
        Box::pin(async {
            Err(InferenceError::Generation(
                "no evidence: assessor must not be called".into(),
            ))
        })
    }
    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        dimensions: Option<u32>,
    ) -> hkask_types::EmbedFuture<'a> {
        assert_eq!(dimensions, Some(hkask_storage::embedding_dim() as u32));
        self.calls.fetch_add(1, Ordering::SeqCst);
        let requested = if self.wrong_requested {
            "another-request".to_string()
        } else {
            model.to_string()
        };
        let actual = self.actual.map(str::to_string);
        let vector = self.vector();
        let count = texts.len();
        Box::pin(async move {
            Ok(EmbeddingBatch {
                vectors: vec![vector; count],
                requested_model: requested,
                actual_model: actual,
                usage: InferenceUsage::default(),
                cost_usd: None,
            })
        })
    }
}
async fn interrupt_after_publish(host: Arc<GoalHost>, cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let task = runtime.spawn(async move {
        let mut work = Box::pin(host.drain_once());
        assert!(
            futures::poll!(work.as_mut()).is_pending(),
            "actual drainer publication/receipt boundary"
        );
        drop(work);
    });
    cx.background_spawn(async move {
        task.await.expect("interrupted production drain");
    })
    .await;
}
async fn resume_startup(
    host: Arc<GoalHost>,
    id: hkask_types::id::GoalID,
    owner: WebID,
    cx: &mut gpui::TestAppContext,
) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let task = runtime.spawn(async move {
        let runner = tokio::spawn(host.clone().run());
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let goal = host
                .service
                .goal_get(id, owner)
                .expect("get")
                .expect("goal");
            if goal
                .projections
                .iter()
                .all(|p| p.delivered_receipt.is_some())
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "startup drainer did not finish"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        runner.abort();
        assert!(runner.await.expect_err("aborted").is_cancelled());
    });
    cx.background_spawn(async move {
        task.await.expect("startup recovery");
    })
    .await;
}

#[gpui::test]
async fn restart_model_switch_repairs_same_width_vectors_before_ack_and_active_knn(
    cx: &mut gpui::TestAppContext,
) {
    for (same_alias, prior_confirmed, current_echoes_alias) in [
        (false, true, false),
        (true, true, false),
        (true, false, false),
        (true, false, true),
    ] {
        let dir = tempfile::tempdir().expect("directory");
        let owner = WebID::new();
        let a = Producer::new(
            if prior_confirmed {
                Some("producer/a-v1")
            } else {
                None
            },
            0,
        );
        let first_request = if same_alias {
            "alias/current"
        } else {
            "alias/a"
        };
        let active_request = if same_alias {
            "alias/current"
        } else {
            "alias/b"
        };
        let host = open_with_provider(dir.path(), a.clone(), first_request).expect("first host");
        let goal = declare(&host, owner).expect("goal");
        decision(host.clone(), &goal, owner, "stop_unknown", false, cx).await;
        interrupt_after_publish(host.clone(), cx).await;
        let entity = format!("curator:goal:{}", goal.id);
        let old = host
            .memory
            .as_ref()
            .expect("memory")
            .h_mems_by_entity_prefix(&entity)
            .expect("first publication");
        assert_eq!(old.len(), 1);
        let old_passage = hkask_memory::semantic_passage_for_h_mem(&old[0]).expect("passage");
        let old_producer = if prior_confirmed {
            "producer/a-v1"
        } else {
            first_request
        };
        assert!(
            host.memory
                .as_ref()
                .expect("memory")
                .has_embedding_for_passage_with_model(
                    &entity,
                    &old_passage,
                    old_producer,
                    &a.vector()
                )
                .expect("positive model/index existence")
        );
        assert!(
            !host
                .memory
                .as_ref()
                .expect("memory")
                .has_embedding_for_passage_with_model(
                    &entity,
                    &old_passage,
                    "producer/b-v2",
                    &a.vector()
                )
                .expect("negative model existence")
        );
        let old_id = old[0].id;
        let first_attribute = old[0].attribute.clone();
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
        let active_producer = if current_echoes_alias {
            "alias/current"
        } else {
            "producer/b-v2"
        };
        let b = Producer::new(Some(active_producer), 1);
        let host = open_with_provider(dir.path(), b.clone(), active_request)
            .expect("reopened with active producer");
        resume_startup(host.clone(), goal.id, owner, cx).await;
        let memory = host.memory.as_ref().expect("memory");
        let rows = memory
            .h_mems_by_entity_prefix(&entity)
            .expect("exact recall");
        let repaired = rows
            .iter()
            .find(|row| row.attribute == first_attribute)
            .expect("same payload identity");
        assert_ne!(
            repaired.id, old_id,
            "incompatible producer is repaired, never merely acknowledged or relabelled"
        );
        assert_eq!(repaired.value, old[0].value);
        let query = b
            .embed_with_dimensions(
                active_request,
                &["original frozen requirement".into()],
                Some(hkask_storage::embedding_dim() as u32),
            )
            .await
            .expect("active producer query");
        let hits = memory
            .search_similar(
                &query.vectors[0],
                100,
                &query.requested_model,
                query.actual_model.as_deref(),
            )
            .expect("active KNN");
        assert!(!hits.results.is_empty());
        assert!(
            hits.results.iter().any(|hit| hit.embedding.passage_text
                == hkask_memory::semantic_passage_for_h_mem(repaired)),
            "exact and semantic recall join after model change"
        );
        for hit in &hits.results {
            assert_eq!(
                hit.embedding.model, active_producer,
                "record the freshly reported producer identity"
            );
            assert_eq!(
                hit.embedding.vector,
                b.vector(),
                "the new producer actually generated the vector"
            );
            assert!(hit.distance.abs() < 1e-6);
        }
        let current = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert_eq!(rows.len(), current.projections.len());
        assert_eq!(
            current.projections[0].delivered_receipt,
            Some(repaired.id.to_string())
        );
    }
}

#[gpui::test]
async fn projection_rejects_wrong_requested_identity_and_blank_actual_identity(
    cx: &mut gpui::TestAppContext,
) {
    for (actual, wrong_requested) in [(Some("producer/b-v2"), true), (Some(""), false)] {
        let dir = tempfile::tempdir().expect("directory");
        let producer = Arc::new(Producer {
            actual,
            wrong_requested,
            axis: 1,
            calls: AtomicUsize::new(0),
        });
        let host = open_with_provider(dir.path(), producer, "alias/b").expect("host");
        let owner = WebID::new();
        let goal = declare(&host, owner).expect("goal");
        decision(host.clone(), &goal, owner, "stop_unknown", false, cx).await;
        super::artifacts::bounded_tick(host.clone(), cx).await;
        let current = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert!(
            current
                .projections
                .iter()
                .all(|p| p.delivered_receipt.is_none()),
            "inconsistent source identity never acknowledges delivery"
        );
        assert!(
            host.memory
                .as_ref()
                .expect("memory")
                .h_mems_by_entity_prefix(&format!("curator:goal:{}", goal.id))
                .expect("exact")
                .is_empty(),
            "bad batch publishes no projection row"
        );
    }
}

#[gpui::test]
async fn unconfirmed_producer_alias_is_regenerated_not_assumed_compatible(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().expect("directory");
    let owner = WebID::new();
    let a = Producer::new(None, 0);
    let host = open_with_provider(dir.path(), a, "unconfirmed-alias").expect("host");
    let goal = declare(&host, owner).expect("goal");
    decision(host.clone(), &goal, owner, "stop_unknown", false, cx).await;
    interrupt_after_publish(host.clone(), cx).await;
    let entity = format!("curator:goal:{}", goal.id);
    let old = host
        .memory
        .as_ref()
        .expect("memory")
        .h_mems_by_entity_prefix(&entity)
        .expect("published");
    assert_eq!(old.len(), 1);
    let old_id = old[0].id;
    let attribute = old[0].attribute.clone();
    drop(host);
    let b = Producer::new(None, 1);
    let host = open_with_provider(dir.path(), b.clone(), "unconfirmed-alias").expect("restart");
    resume_startup(host.clone(), goal.id, owner, cx).await;
    let memory = host.memory.as_ref().expect("memory");
    let rows = memory.h_mems_by_entity_prefix(&entity).expect("exact");
    let repaired = rows
        .iter()
        .find(|row| row.attribute == attribute)
        .expect("repaired");
    assert_ne!(repaired.id, old_id);
    let query = b
        .embed_with_dimensions(
            "unconfirmed-alias",
            &["frozen requirement".into()],
            Some(hkask_storage::embedding_dim() as u32),
        )
        .await
        .expect("active unconfirmed producer query");
    let hits = memory
        .search_similar(
            &query.vectors[0],
            100,
            &query.requested_model,
            query.actual_model.as_deref(),
        )
        .expect("active KNN");
    assert!(!hits.results.is_empty());
    for hit in hits.results {
        assert_eq!(hit.embedding.model, "unconfirmed-alias");
        assert_eq!(hit.embedding.vector, b.vector());
        assert!(hit.distance.abs() < 1e-6);
    }
}
