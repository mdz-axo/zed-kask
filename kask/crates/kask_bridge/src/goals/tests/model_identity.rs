use super::*;

struct Producer {
    actual: Option<&'static str>,
    wrong_requested: bool,
    axis: usize,
    paused: AtomicBool,
    started: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}
impl Producer {
    fn new(actual: Option<&'static str>, axis: usize) -> Arc<Self> {
        Arc::new(Self {
            actual,
            wrong_requested: false,
            axis,
            paused: AtomicBool::new(false),
            started: Arc::new(tokio::sync::Notify::new()),
            resume: Arc::new(tokio::sync::Notify::new()),
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
        let requested = if self.wrong_requested {
            "wrong-request".into()
        } else {
            model.to_string()
        };
        let actual = self.actual.map(str::to_string);
        let vectors = vec![self.vector(); texts.len()];
        Box::pin(async move {
            if self.paused.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.resume.notified().await;
            }
            Ok(EmbeddingBatch {
                vectors,
                requested_model: requested,
                actual_model: actual,
                usage: InferenceUsage::default(),
                cost_usd: None,
            })
        })
    }
}
fn block_ack(host: &GoalHost) {
    host.ledger.driver().execute_batch("CREATE TRIGGER fail_goal_ack BEFORE UPDATE ON hmems WHEN NEW.entity = 'kanban:goal' AND json_extract(NEW.value, '$.indexed_revision') > json_extract(OLD.value, '$.indexed_revision') BEGIN SELECT RAISE(FAIL, 'interruption after memory publication'); END;").expect("install canonical commit fault");
}
fn unblock_ack(host: &GoalHost) {
    host.ledger
        .driver()
        .execute_batch("DROP TRIGGER fail_goal_ack;")
        .expect("restore canonical commits");
}
#[tokio::test]
async fn publication_before_mark_restart_reuses_one_current_record_and_repairs_active_model() {
    for (prior, actual) in [
        (Some("producer/a"), Some("producer/b")),
        (None, Some("alias/current")),
        (None, None),
    ] {
        let dir = tempfile::tempdir().expect("directory");
        let old = Producer::new(prior, 0);
        let host = open_with_provider(dir.path(), old.clone(), "alias/current").expect("host");
        let owner = WebID::new();
        let goal = declare(&host, owner).expect("goal");
        block_ack(&host);
        host.drain_once()
            .await
            .expect("failed goal retained, siblings continue");
        let entity = format!("curator:goal:{}", goal.id);
        let memory = host.memory.as_ref().expect("memory");
        let before = memory.h_mems_by_entity_prefix(&entity).expect("published");
        assert_eq!(before.len(), 1);
        assert_eq!(
            host.service
                .goal_get(goal.id, owner)
                .expect("get")
                .expect("goal")
                .indexed_revision,
            0
        );
        unblock_ack(&host);
        drop(host);
        let active = Producer::new(actual, 1);
        let host =
            open_with_provider(dir.path(), active.clone(), "alias/current").expect("restart");
        host.drain_once().await.expect("repair and mark");
        let retained = host
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert_eq!(retained.indexed_revision, retained.revision);
        let memory = host.memory.as_ref().expect("memory");
        let rows = memory.h_mems_by_entity_prefix(&entity).expect("recall");
        assert_eq!(
            rows.len(),
            1,
            "revision replaces, never appends snapshot history"
        );
        let passage = hkask_memory::semantic_passage_for_h_mem(&rows[0]).expect("passage");
        let producer = actual.unwrap_or("alias/current");
        assert!(
            memory
                .has_embedding_for_passage_with_model(&entity, &passage, producer, &active.vector())
                .expect("active row and index")
        );
        assert!(
            !memory
                .has_embedding_for_passage_with_model(
                    &entity,
                    &passage,
                    prior.unwrap_or("alias/current"),
                    &old.vector()
                )
                .expect("stale vector excluded")
        );
        assert_eq!(memory.embedding_count().expect("one vector"), 1);
        assert!(
            memory
                .search_similar(&active.vector(), 5, "alias/current", actual)
                .expect("active KNN")
                .results
                .iter()
                .any(|hit| hit.embedding.entity_ref == entity)
        );
        // Repeated scans do not mint another memory row or rotate its identity.
        let stable = rows[0].id;
        host.drain_once().await.expect("already indexed");
        assert_eq!(
            memory.h_mems_by_entity_prefix(&entity).expect("recall")[0].id,
            stable
        );
    }
}
#[tokio::test]
async fn same_producer_retry_after_publication_before_mark_reuses_exact_row() {
    let dir = tempfile::tempdir().expect("directory");
    let provider = Producer::new(Some("producer/current"), 0);
    let host = open_with_provider(dir.path(), provider.clone(), "alias/current").expect("host");
    let owner = WebID::new();
    let goal = declare(&host, owner).expect("goal");
    block_ack(&host);
    host.drain_once().await.expect("backlog retained");
    let entity = format!("curator:goal:{}", goal.id);
    let before = host
        .memory
        .as_ref()
        .expect("memory")
        .h_mems_by_entity_prefix(&entity)
        .expect("published");
    unblock_ack(&host);
    drop(host);
    let host = open_with_provider(dir.path(), provider, "alias/current").expect("restart");
    host.drain_once().await.expect("retry");
    let after = host
        .memory
        .as_ref()
        .expect("memory")
        .h_mems_by_entity_prefix(&entity)
        .expect("recalled");
    assert_eq!(before.len(), 1);
    assert_eq!(after.len(), 1);
    assert_eq!(
        before[0].id, after[0].id,
        "crash replay converges without another row"
    );
}
#[tokio::test]
async fn newer_revision_or_producer_wins_over_older_inflight_preparation() {
    for advance_revision in [false, true] {
        let dir = tempfile::tempdir().expect("directory");
        let old = Producer::new(Some("producer/old"), 0);
        old.paused.store(true, Ordering::SeqCst);
        let first = open_with_provider(dir.path(), old.clone(), "alias/current").expect("old host");
        let owner = WebID::new();
        let goal = declare(&first, owner).expect("goal");
        let started = old.started.notified();
        let task = tokio::spawn({
            let first = first.clone();
            async move { first.drain_once().await }
        });
        started.await;
        let active = Producer::new(Some("producer/current"), 1);
        let second = open_with_provider(dir.path(), active.clone(), "alias/current")
            .expect("second process-equivalent connection");
        if advance_revision {
            second
                .service
                .goal_judge(
                    goal.id,
                    GoalVerdict {
                        verdict: GoalVerdictValue::Continue,
                        confidence: 0.5,
                        criterion_results: goal
                            .criteria
                            .iter()
                            .enumerate()
                            .map(|(index, _)| CriterionJudgment {
                                index,
                                passed: false,
                                note: "Work remains".into(),
                            })
                            .collect(),
                        reasoning: "Newer worker observation".into(),
                        judged_at: Utc::now(),
                    },
                    owner,
                )
                .expect("new revision");
        }
        second.drain_once().await.expect("current revision indexed");
        old.resume.notify_one();
        task.await
            .expect("older task")
            .expect("stale goal retained, not acknowledged");
        let retained = second
            .service
            .goal_get(goal.id, owner)
            .expect("get")
            .expect("goal");
        assert_eq!(retained.indexed_revision, retained.revision);
        let entity = format!("curator:goal:{}", goal.id);
        let memory = second.memory.as_ref().expect("memory");
        let rows = memory.h_mems_by_entity_prefix(&entity).expect("recall");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value["revision"], retained.revision);
        assert_eq!(
            rows[0].value["verdicts"].as_array().expect("history").len(),
            usize::from(advance_revision)
        );
        let passage = hkask_memory::semantic_passage_for_h_mem(&rows[0]).expect("passage");
        assert!(
            memory
                .has_embedding_for_passage_with_model(
                    &entity,
                    &passage,
                    "producer/current",
                    &active.vector()
                )
                .expect("newer index intact")
        );
    }
}
#[tokio::test]
async fn wrong_requested_or_blank_actual_producer_never_marks_goal_indexed() {
    for (requested_wrong, actual) in [(true, Some("producer")), (false, Some(""))] {
        let dir = tempfile::tempdir().expect("directory");
        let mut provider = Producer::new(actual, 0);
        Arc::get_mut(&mut provider)
            .expect("sole provider")
            .wrong_requested = requested_wrong;
        let host = open_with_provider(dir.path(), provider, "alias/current").expect("host");
        let owner = WebID::new();
        let goal = declare(&host, owner).expect("goal");
        host.drain_once().await.expect("failed projection retained");
        assert_eq!(
            host.service
                .goal_get(goal.id, owner)
                .expect("get")
                .expect("goal")
                .indexed_revision,
            0
        );
        assert_eq!(
            host.memory
                .as_ref()
                .expect("memory")
                .h_mem_count()
                .expect("rows"),
            0
        );
    }
}
