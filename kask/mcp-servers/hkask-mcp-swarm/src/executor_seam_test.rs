// ── AgentExecutor seam: run returns raw output; scan_output is separate ──
//
// The debit-before-scan invariant (see `delegate`'s doc + the canary test
// above) depends on `AgentExecutor::run` NOT scanning the final output —
// it returns the raw text so the runtime can debit, then call
// `scan_output`. This pins that seam: a canary in the model output passes
// through `run` unredacted (Ok), and `scan_output` is what rejects it. If
// a future "simplification" moves `scan_output` into `run`, this test
// fails (run would reject the canary instead of returning it raw), and the
// debit-before-scan invariant would silently break.
#[tokio::test]
async fn executor_run_returns_raw_output_without_scanning() {
    let guard = hkask_guard::ContentGuard::mandatory(&hkask_guard::GuardConfig::default());
    let canary = guard.canary().as_str().to_string();
    let executor = crate::agent_executor::AgentExecutor::with_deps(
        std::sync::Arc::new(StubInferencePort::new(&canary, 100)),
        std::sync::Arc::new(StubToolDispatch::new(serde_json::json!({}))),
        std::sync::Arc::new(StubSkillExec::ok("stub skill output")),
        guard,
    );
    let agent = test_agent_card("You are a test agent.", "");
    // run returns the raw canary text — it does NOT scan the final output.
    let raw = executor
        .run(&agent, "do something")
        .await
        .expect("run must return raw output without scanning it");
    assert_eq!(
        raw.text, canary,
        "run must return the model's raw text, including the canary"
    );
    // scan_output is the separate step that rejects the canary. This is
    // what the runtime calls AFTER debit, preserving "compute was spent".
    let scan_err = executor
        .scan_output(&raw.text)
        .expect_err("scan_output must reject the canary");
    assert!(
        matches!(scan_err, SwarmError::Unavailable(ref m) if m.contains("canary token detected")),
        "scan_output must detect the canary that run let through, got {scan_err:?}"
    );
}
