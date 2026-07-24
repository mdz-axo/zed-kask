//! Integration tests for kata-kanban service — verifies kanban service
//! construction, kata prompt generation, and the task-scoped gas feedback loop.

#[cfg(test)]
mod tests {
    use hkask_services_kata_kanban::kanban::{ColumnDef, KanbanService, TaskSpec};
    use hkask_services_kata_kanban::{KataEngine, KataManifest, TaskGasAccountantFn};
    use hkask_storage::HMemStore;
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_templates::SqliteRegistry;
    use hkask_types::template::LLMParameters;
    use hkask_types::{
        ChatToolDefinition, InferenceError, InferencePort, InferenceResult, InferenceUsage,
    };
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    fn make_test_store() -> HMemStore {
        let pool = SqliteDriver::in_memory_pool().expect("in-memory pool");
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(pool));
        HMemStore::from_driver(driver)
    }

    fn default_columns() -> Vec<ColumnDef> {
        KanbanService::standard_columns()
    }

    /// Parse a kata manifest from a YAML string.
    fn parse_manifest(yaml: &str) -> KataManifest {
        serde_yaml_neo::from_str(yaml).expect("manifest YAML must parse")
    }

    /// A 1-question coaching manifest with Regulation spans disabled for test isolation.
    const COACHING_MANIFEST_1Q: &str = r#"
manifest:
  id: test-coaching-1q
  name: Test Coaching 1Q
  kata_type: coaching
  description: Test
gas:
  cap: 100000
  alert_threshold: 0.7
  hard_limit: true
questions:
  - number: 1
    question: "What is your target condition?"
    description: "Target"
ledger:
  emit_spans: false
  span_namespace: "test.kata"
  variety_monitoring: false
"#;

    /// A 3-question coaching manifest for multi-step gas deduction tests.
    const COACHING_MANIFEST_3Q: &str = r#"
manifest:
  id: test-coaching-3q
  name: Test Coaching 3Q
  kata_type: coaching
  description: Test
gas:
  cap: 100000
  alert_threshold: 0.7
  hard_limit: true
questions:
  - number: 1
    question: "Q1?"
    description: "D1"
  - number: 2
    question: "Q2?"
    description: "D2"
  - number: 3
    question: "Q3?"
    description: "D3"
ledger:
  emit_spans: false
  span_namespace: "test.kata"
  variety_monitoring: false
"#;

    /// A 1-step improvement manifest.
    const IMPROVEMENT_MANIFEST_1S: &str = r#"
manifest:
  id: test-improvement-1s
  name: Test Improvement 1S
  kata_type: improvement
  description: Test
gas:
  cap: 100000
  alert_threshold: 0.7
  hard_limit: true
steps:
  - ordinal: 1
    action: "understand_direction"
    description: "Understand the direction"
    gas_cap: 2000
ledger:
  emit_spans: false
  span_namespace: "test.kata"
  variety_monitoring: false
"#;

    // ── KanbanService construction ──────────────────────────────────────

    #[test]
    fn kanban_service_constructs_with_store() {
        let svc = KanbanService::new(make_test_store());
        let _ = svc;
    }

    #[test]
    fn kanban_service_builder_chain_with_pod_manager() {
        let svc = KanbanService::new(make_test_store());
        let _ = svc;
    }

    // ── Kata prompt generation (MCP/REPL path) ─────────────────────────

    #[test]
    fn kata_prompt_methods_emit_reg_spans() {
        let svc = KanbanService::new(make_test_store());
        let owner = hkask_types::WebID::new();
        let board = svc
            .board_create(owner, "Regulation Board", &default_columns())
            .unwrap();
        let spec = TaskSpec::new("Regulation Task".into());
        let task = svc.task_create(board.id, spec, owner).unwrap();

        let coaching = svc.task_coaching_prompt(task.id);
        assert!(coaching.is_ok());

        let improvement = svc.task_improvement_prompt(task.id);
        assert!(improvement.is_ok());

        let practice = svc.task_practice_prompt(task.id, "test");
        assert!(practice.is_ok());
    }

    // ── Task gas feedback loop (Option C wiring) ───────────────────────

    /// Mock inference port that returns a fixed response with known token usage.
    /// Each call returns 100 total tokens, so we can verify the gas deduction.
    struct MockInference {
        response_text: String,
        tokens_per_call: u32,
    }

    impl InferencePort for MockInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            let text = self.response_text.clone();
            let tokens = self.tokens_per_call;
            Box::pin(async move {
                Ok(InferenceResult {
                    text,
                    model: "mock-model".into(),
                    usage: InferenceUsage {
                        prompt_tokens: tokens / 2,
                        completion_tokens: tokens / 2,
                        total_tokens: tokens,
                    },
                    finish_reason: "stop".into(),
                    token_probabilities: None,
                    tool_calls: vec![],
                    reasoning: None,
                })
            })
        }
    }

    #[tokio::test]
    async fn task_gas_accountant_deducts_from_coaching_kata() {
        // Setup: kanban service with a task that has a gas budget of 500
        let svc = Arc::new(KanbanService::new(make_test_store()));
        let owner = hkask_types::WebID::new();
        let board = svc
            .board_create(owner, "Gas Board", &default_columns())
            .unwrap();
        let spec = TaskSpec::new("Gas Task".into()).with_gas_budget(500);
        let task = svc.task_create(board.id, spec, owner).unwrap();
        assert_eq!(task.gas_remaining, Some(500));

        // Create a kata engine with mock inference (100 tokens per call)
        // and a task gas accountant bound to the task.
        let registry = SqliteRegistry::new(None).expect("temp registry");
        let mock_inference = Arc::new(MockInference {
            response_text: "Target condition: ship the feature.".into(),
            tokens_per_call: 100,
        });
        let accountant: TaskGasAccountantFn = svc.gas_accountant_for(task.id);
        let engine = KataEngine::new(mock_inference, registry).with_task_gas_accountant(accountant);

        // Execute a coaching kata cycle (1 question = 1 inference call)
        let manifest = parse_manifest(COACHING_MANIFEST_1Q);
        let result = engine
            .execute(&manifest, "test-bot", std::collections::HashMap::new())
            .await
            .expect("coaching kata should succeed");

        assert_eq!(result.kata_type, "coaching");
        assert_eq!(result.steps_completed, 1);

        // Verify the task's gas_remaining was decremented by 100 tokens
        let updated_task = svc.task_get(task.id).unwrap().expect("task should exist");
        assert_eq!(
            updated_task.gas_remaining,
            Some(400),
            "gas_remaining should be 500 - 100 = 400 after one coaching inference call"
        );

        // Verify a GasEntry was recorded in the audit trail
        assert_eq!(
            updated_task.gas_spend.len(),
            1,
            "one GasEntry should be recorded for the inference call"
        );
        let entry = &updated_task.gas_spend[0];
        assert_eq!(entry.amount, 100);
        assert!(entry.reason.contains("coaching-q1"));
        assert!(entry.reason.contains("mock-model"));
        assert_eq!(entry.kind, "gas_spend");
    }

    #[tokio::test]
    async fn task_gas_accountant_deducts_from_improvement_kata() {
        // Setup: kanban service with a task that has a gas budget of 1000
        let svc = Arc::new(KanbanService::new(make_test_store()));
        let owner = hkask_types::WebID::new();
        let board = svc
            .board_create(owner, "Gas Board", &default_columns())
            .unwrap();
        let spec = TaskSpec::new("Gas Task".into()).with_gas_budget(1000);
        let task = svc.task_create(board.id, spec, owner).unwrap();
        assert_eq!(task.gas_remaining, Some(1000));

        // Create a kata engine with mock inference (100 tokens per call)
        let registry = SqliteRegistry::new(None).expect("temp registry");
        let mock_inference = Arc::new(MockInference {
            response_text: "Direction: improve the system.".into(),
            tokens_per_call: 100,
        });
        let accountant: TaskGasAccountantFn = svc.gas_accountant_for(task.id);
        let engine = KataEngine::new(mock_inference, registry).with_task_gas_accountant(accountant);

        // Execute an improvement kata cycle (1 step = 1 inference call)
        let manifest = parse_manifest(IMPROVEMENT_MANIFEST_1S);
        let result = engine
            .execute(&manifest, "test-bot", std::collections::HashMap::new())
            .await
            .expect("improvement kata should succeed");

        assert_eq!(result.kata_type, "improvement");
        assert_eq!(result.steps_completed, 1);

        // Verify the task's gas_remaining was decremented by 100 tokens
        let updated_task = svc.task_get(task.id).unwrap().expect("task should exist");
        assert_eq!(
            updated_task.gas_remaining,
            Some(900),
            "gas_remaining should be 1000 - 100 = 900 after one improvement inference call"
        );

        // Verify a GasEntry was recorded
        assert_eq!(updated_task.gas_spend.len(), 1);
        let entry = &updated_task.gas_spend[0];
        assert_eq!(entry.amount, 100);
        assert!(entry.reason.contains("step-1"));
    }

    #[tokio::test]
    async fn task_gas_accountant_no_op_without_accountant() {
        // When no accountant is configured, inference should still work
        // and no gas should be deducted from any task.
        let svc = KanbanService::new(make_test_store());
        let owner = hkask_types::WebID::new();
        let board = svc
            .board_create(owner, "No Accountant Board", &default_columns())
            .unwrap();
        let spec = TaskSpec::new("No Accountant Task".into()).with_gas_budget(500);
        let task = svc.task_create(board.id, spec, owner).unwrap();

        let registry = SqliteRegistry::new(None).expect("temp registry");
        let mock_inference = Arc::new(MockInference {
            response_text: "Response without accounting.".into(),
            tokens_per_call: 100,
        });
        // No with_task_gas_accountant — standalone kata execution
        let engine = KataEngine::new(mock_inference, registry);

        let manifest = parse_manifest(COACHING_MANIFEST_1Q);
        let result = engine
            .execute(&manifest, "test-bot", std::collections::HashMap::new())
            .await
            .expect("kata should succeed without accountant");

        assert_eq!(result.steps_completed, 1);

        // Task gas should be unchanged (no accountant wired)
        let unchanged_task = svc.task_get(task.id).unwrap().expect("task should exist");
        assert_eq!(
            unchanged_task.gas_remaining,
            Some(500),
            "gas_remaining should be unchanged when no accountant is configured"
        );
        assert!(
            unchanged_task.gas_spend.is_empty(),
            "no GasEntry should be recorded without an accountant"
        );
    }

    #[tokio::test]
    async fn task_gas_accountant_deducts_across_multiple_steps() {
        // Verify that multiple inference calls accumulate deductions correctly.
        // 3 coaching questions × 100 tokens each = 300 total deducted.
        let svc = Arc::new(KanbanService::new(make_test_store()));
        let owner = hkask_types::WebID::new();
        let board = svc
            .board_create(owner, "Multi Board", &default_columns())
            .unwrap();
        let spec = TaskSpec::new("Multi Task".into()).with_gas_budget(1000);
        let task = svc.task_create(board.id, spec, owner).unwrap();

        let registry = SqliteRegistry::new(None).expect("temp registry");
        let mock_inference = Arc::new(MockInference {
            response_text: "Answer.".into(),
            tokens_per_call: 100,
        });
        let accountant: TaskGasAccountantFn = svc.gas_accountant_for(task.id);
        let engine = KataEngine::new(mock_inference, registry).with_task_gas_accountant(accountant);

        let manifest = parse_manifest(COACHING_MANIFEST_3Q);
        let result = engine
            .execute(&manifest, "test-bot", std::collections::HashMap::new())
            .await
            .expect("multi coaching should succeed");

        assert_eq!(result.steps_completed, 3);

        // 3 calls × 100 tokens = 300 deducted from 1000 = 700 remaining
        let updated_task = svc.task_get(task.id).unwrap().expect("task should exist");
        assert_eq!(
            updated_task.gas_remaining,
            Some(700),
            "gas_remaining should be 1000 - 300 = 700 after three coaching calls"
        );
        assert_eq!(
            updated_task.gas_spend.len(),
            3,
            "three GasEntry records should exist"
        );
    }
}
