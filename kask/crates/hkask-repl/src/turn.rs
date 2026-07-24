//! Per-turn processing for the REPL.
//!
//! Handles single-agent inference turns, including gas governance,
//! tool-augmented followup, Regulation updates, and persona filtering.
//!
//! Both CLI (stdout) and TUI (capture buffer) surfaces share a single
//! `run_turn_loop` via the `TurnSink` trait. Behavioral dependencies
//! (inference, gas, tools, threads) are injected via `TurnDeps`.

use hkask_services_chat::{TokenUsage, TurnResult};

use super::ReplState;
use super::TalkMode;
use super::deps::{TurnConfig, TurnDeps, TurnInput};
use super::handlers::speak_response;
#[cfg(feature = "tui")]
use super::reg_display;

// ── TurnSink: output abstraction ─────────────────────────────────────

trait TurnSink {
    fn agent_text(&mut self, agent: &str, text: &str);
    /// Render a reasoning/thinking delta in a visually distinct section so
    /// the chain-of-thought never mixes into the answer (Cline #8636).
    fn thinking(&mut self, agent: &str, text: &str);
    fn tool_log(&mut self, line: &str);
    fn status(&mut self, line: &str);
}

struct StdoutSink;

impl TurnSink for StdoutSink {
    fn agent_text(&mut self, agent: &str, text: &str) {
        println!("{}: {}", agent, text);
    }
    fn thinking(&mut self, agent: &str, text: &str) {
        // Dim, prefixed "↳ thinking" so reasoning is visually separated from
        // the answer and collapsible in a future TUI render.
        println!("  \x1b[2m↳ {} thinking:\x1b[0m {}", agent, text);
    }
    fn tool_log(&mut self, line: &str) {
        println!("{}", line);
    }
    fn status(&mut self, line: &str) {
        println!("{}", line);
    }
}

#[cfg(feature = "tui")]
struct CaptureSink {
    response_text: String,
    /// Reasoning trace, kept separate from `response_text` so the TUI can
    /// render a collapsible "Thinking" block (Zed/Cline pattern) instead of
    /// interleaving chain-of-thought into the answer.
    reasoning_text: String,
    tool_output: String,
}

#[cfg(feature = "tui")]
impl CaptureSink {
    fn new() -> Self {
        Self {
            response_text: String::new(),
            reasoning_text: String::new(),
            tool_output: String::new(),
        }
    }
}

#[cfg(feature = "tui")]
impl TurnSink for CaptureSink {
    fn agent_text(&mut self, _agent: &str, text: &str) {
        use std::fmt::Write;
        let _ = writeln!(self.response_text, "{}", text);
    }
    fn thinking(&mut self, _agent: &str, text: &str) {
        use std::fmt::Write;
        // Accumulate reasoning deltas into a dedicated buffer, separate
        // from the answer so the TUI renders a distinct Thinking block.
        let _ = write!(self.reasoning_text, "{}", text);
    }
    fn tool_log(&mut self, line: &str) {
        use std::fmt::Write;
        let _ = writeln!(self.tool_output, "{}", line);
    }
    fn status(&mut self, line: &str) {
        if line.contains("tokens (") {
            return;
        }
        use std::fmt::Write;
        let _ = writeln!(self.response_text, "{}", line);
    }
}

// ── TurnOutcome ──────────────────────────────────────────────────────

#[cfg_attr(not(feature = "tui"), allow(dead_code))]
struct TurnOutcome {
    success: bool,
    final_response: Option<String>,
    usage: TokenUsage,
    iterations: usize,
    budget_exhausted: bool,
}

fn zero_usage() -> TokenUsage {
    TokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    }
}

// ── Post-loop status display ─────────────────────────────────────────

fn emit_turn_status(
    sink: &mut impl TurnSink,
    usage: Option<&TokenUsage>,
    iteration: usize,
    gas_remaining: u64,
    gas_cap: u64,
) {
    if let Some(usage) = usage {
        if iteration > 1 {
            sink.status(&format!(
                "  \x1b[2m{} tokens ({} prompt + {} completion) across {} iterations\x1b[0m",
                usage.total_tokens, usage.prompt_tokens, usage.completion_tokens, iteration
            ));
        } else {
            sink.status(&format!(
                "  \x1b[2m{} tokens ({} prompt + {} completion)\x1b[0m",
                usage.total_tokens, usage.prompt_tokens, usage.completion_tokens
            ));
        }
    }
    if gas_cap > 0 && gas_remaining > 0 && (gas_remaining as f64 / gas_cap as f64) < 0.2 {
        sink.status(&format!(
            "  \x1b[33m\u{26a0} Gas budget low: {}/{} ({:.0}%)\x1b[0m",
            gas_remaining,
            gas_cap,
            (gas_remaining as f64 / gas_cap as f64) * 100.0
        ));
    } else if gas_cap > 0 && gas_remaining == 0 {
        sink.status("  \x1b[31m\u{2717} Gas budget exhausted \u{2014} some operations may be throttled\x1b[0m");
    }
}

// ── Unified turn loop ────────────────────────────────────────────────

fn run_turn_loop(
    input: &str,
    deps: TurnDeps,
    config: &TurnConfig,
    rt: &tokio::runtime::Handle,
    sink: &mut impl TurnSink,
    agent_override: Option<&str>,
) -> TurnOutcome {
    let display_name = crate::display::model_abbrev(&config.model_name);
    let mut iteration: usize = 0;
    let mut total_usage: Option<TokenUsage> = None;
    let mut final_response: Option<String> = None;
    let mut inference_error = false;
    // Growing message array — maintained across iterations with proper role tags.
    // Iteration 1: built by execute_turn (system + memory + thread + user).
    // Iteration 2+: we append assistant(response) + user(tool_results) and pass
    // the array back via prebuilt_messages, skipping prepare_chat entirely.
    let mut messages: Option<Vec<hkask_types::ChatMessage>> = None;

    tracing::info!(target: "reg", reg_domain = "reg.chat.turn", operation = "started", agent = %display_name, input_len = input.len(), "REG");

    loop {
        iteration += 1;
        if iteration > config.max_loops {
            sink.status(&format!("  \x1b[33m\u{26a0} Tool-use loop max iterations ({}) reached \u{2014} yielding current response\x1b[0m", config.max_loops));
            break;
        }

        let Some(mut gas_guard) = deps.gas.try_reserve(config.gas_heuristic) else {
            sink.status("  \x1b[31m\u{2717} Gas budget exhausted (hard limit) \u{2014} turn blocked by cybernetic regulator\x1b[0m");
            sink.status(
                "  \x1b[2mUse /status to see budget details, or wait for replenishment.\x1b[0m",
            );
            return TurnOutcome {
                success: false,
                final_response: None,
                usage: total_usage.unwrap_or_else(zero_usage),
                iterations: 0,
                budget_exhausted: true,
            };
        };

        // Iteration 1: pass thread_messages + input, let execute_turn build the array.
        // Iteration 2+: pass the growing array via prebuilt_messages.
        let thread_messages = if iteration == 1 && !deps.threads.is_seeded() {
            deps.threads.thread_history_messages(config.saliency_window)
        } else {
            None
        };
        let turn_input = TurnInput {
            input,
            iteration,
            agent_override,
            thread_messages,
            messages: messages.take(),
        };

        // Stream inference tokens to the sink as they arrive.
        let mut stream = deps.executor.execute_turn_streaming(&turn_input);
        let mut chat_response: Option<TurnResult> = None;
        let mut stream_error: Option<String> = None;
        use futures_util::StreamExt;
        loop {
            match rt.block_on(stream.next()) {
                Some(Ok(super::deps::TurnStreamChunk::Delta(delta))) => {
                    sink.agent_text(&display_name, &delta);
                }
                Some(Ok(super::deps::TurnStreamChunk::Thinking(thinking))) => {
                    sink.thinking(&display_name, &thinking);
                }
                Some(Ok(super::deps::TurnStreamChunk::Done(result))) => {
                    chat_response = Some(result);
                    break;
                }
                Some(Err(e)) => {
                    stream_error = Some(e.to_string());
                    break;
                }
                None => break,
            }
        }
        let chat_response = match (chat_response, stream_error) {
            (Some(r), _) => r,
            (None, Some(e)) => {
                sink.status(&format!("  \x1b[31mInference error:\x1b[0m {}", e));
                gas_guard.release();
                inference_error = true;
                break;
            }
            (None, None) => {
                sink.status("  \x1b[31mInference error:\x1b[0m stream ended unexpectedly");
                gas_guard.release();
                inference_error = true;
                break;
            }
        };

        // Capture the message array from the result — this is our growing array.
        let mut current_messages = chat_response.messages;

        let usage = chat_response.usage;
        if let Some(ref mut total) = total_usage {
            total.prompt_tokens += usage.prompt_tokens;
            total.completion_tokens += usage.completion_tokens;
            total.total_tokens += usage.total_tokens;
        } else {
            total_usage = Some(usage);
        }

        let actual_cost = total_usage
            .as_ref()
            .map(|u| u.gas_cost())
            .unwrap_or(gas_guard.heuristic());
        gas_guard.settle(actual_cost);

        let response = chat_response.text;
        let structured_calls = chat_response.structured_tool_calls;
        let parsed = extract_tool_calls(
            &response,
            if structured_calls.is_empty() {
                None
            } else {
                Some(&structured_calls)
            },
        );

        if parsed.tool_calls.is_empty() {
            sink.agent_text(&display_name, &parsed.text);
            final_response = Some(parsed.text.clone());
            break;
        }

        if !parsed.text.trim().is_empty() {
            sink.agent_text(&display_name, parsed.text.trim());
        }
        sink.tool_log(&format!(
            "  \x1b[2m\u{2750} {} tool call(s) from {}\x1b[0m",
            parsed.tool_calls.len(),
            display_name
        ));

        // Append the assistant's response to the growing message array
        // with the correct role tag — NOT as user input (N2 fix).
        current_messages.push(hkask_types::ChatMessage::assistant(&response));

        let mut tool_results_vec = Vec::new();
        for call in &parsed.tool_calls {
            let mut line = format!("  \x1b[2m  Invoking {}\x1b[0m", call.tool);
            if !call.server.is_empty() {
                line.push_str(&format!(" on \x1b[36m{}\x1b[0m", call.server));
            }
            line.push_str("...");
            sink.tool_log(&line);

            let result = rt.block_on(async {
                use hkask_capability::{
                    DelegationAction, DelegationResource, DelegationToken, derive_signing_key,
                };
                let token = DelegationToken::new(
                    DelegationResource::Tool,
                    call.tool.clone(),
                    DelegationAction::Execute,
                    config.principal_webid,
                    config.agent_webid,
                    &derive_signing_key(config.a2a_secret.as_bytes()),
                );
                deps.tools
                    .invoke(&call.server, &call.tool, call.args.clone(), &token)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}: {}", call.tool, e))
            });
            match &result {
                Ok(value) => {
                    sink.tool_log(&format!("  \x1b[32m  \u{2713}\x1b[0m {}", call.tool));
                    if let Ok(formatted) = serde_json::to_string_pretty(value) {
                        for line in formatted.lines().take(5) {
                            sink.tool_log(&format!("    {}", line));
                        }
                        if formatted.lines().count() > 5 {
                            sink.tool_log("    ...");
                        }
                    }
                }
                Err(err) => sink.tool_log(&format!(
                    "  \x1b[31m  \u{2717}\x1b[0m {} \u{2014} {}",
                    call.tool, err
                )),
            }
            tool_results_vec.push((call.clone(), result));
        }

        // Append tool results as a user message — the model sees:
        // [system, user, assistant(tool_calls), user(tool_results)]
        // and generates the next response with proper role context.
        let tool_results_text = format_tool_results(&tool_results_vec);
        current_messages.push(hkask_types::ChatMessage::user(&tool_results_text));
        messages = Some(current_messages);
    }

    let (gas_remaining, gas_cap) = deps.gas.gas_status();
    emit_turn_status(
        sink,
        total_usage.as_ref(),
        iteration,
        gas_remaining,
        gas_cap,
    );

    if let Some(ref resp) = final_response {
        deps.threads.append_turn(&config.default_agent, input, resp);
    }
    if !inference_error {
        deps.threads.mark_seeded();
    }

    if let Some(ref resp) = final_response {
        tracing::info!(target: "reg", reg_domain = "reg.chat.turn", operation = "completed", agent = %display_name, response_len = resp.len(), iterations = iteration, "REG");
    }

    (deps.on_reg_update)();

    TurnOutcome {
        success: !inference_error,
        final_response,
        usage: total_usage.unwrap_or_else(zero_usage),
        iterations: iteration,
        budget_exhausted: false,
    }
}

// ── Public wrappers ──────────────────────────────────────────────────

/// Shared turn execution: run the loop, speak if TTS is on, return the outcome.
/// All public entry points are thin wrappers around this.
fn run_turn_generic(
    input: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
    a2a_secret: &[u8],
    agent_override: Option<&str>,
    sink: &mut impl TurnSink,
) -> TurnOutcome {
    let outcome = run_turn_with_state(input, state, rt, a2a_secret, agent_override, sink);
    if let Some(ref resp) = outcome.final_response
        && state.talk_config.mode == TalkMode::On
    {
        speak_response(resp, state, rt);
    }
    outcome
}

pub(super) fn single_agent_turn(
    input: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
    a2a_secret: &[u8],
    agent_override: Option<&str>,
) -> bool {
    let outcome = run_turn_generic(
        input,
        state,
        rt,
        a2a_secret,
        agent_override,
        &mut StdoutSink,
    );
    outcome.success
}

#[cfg(feature = "tui")]
pub struct TurnCapture {
    pub response_text: String,
    pub tool_output: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub iterations: usize,
    pub budget_exhausted: bool,
}

/// Build a TurnCapture from a CaptureSink + TurnOutcome.
#[cfg(feature = "tui")]
fn capture_from_outcome(sink: CaptureSink, outcome: TurnOutcome) -> TurnCapture {
    TurnCapture {
        response_text: sink.response_text.trim().to_string(),
        tool_output: sink.tool_output,
        prompt_tokens: outcome.usage.prompt_tokens,
        completion_tokens: outcome.usage.completion_tokens,
        total_tokens: outcome.usage.total_tokens,
        iterations: outcome.iterations,
        budget_exhausted: outcome.budget_exhausted,
    }
}

#[cfg(feature = "tui")]
pub fn single_agent_turn_captured(
    input: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
    a2a_secret: &[u8],
) -> TurnCapture {
    let mut sink = CaptureSink::new();
    let outcome = run_turn_generic(input, state, rt, a2a_secret, None, &mut sink);
    capture_from_outcome(sink, outcome)
}

/// Like `single_agent_turn_captured` but forces a specific agent (e.g. the
/// Curator) to handle the turn. Used by the TUI's CuratorWindow so its
/// messages run through the real inference pipeline instead of a stub.
#[cfg(feature = "tui")]
pub fn single_agent_turn_captured_with_agent(
    input: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
    a2a_secret: &[u8],
    agent: &str,
) -> TurnCapture {
    let mut sink = CaptureSink::new();
    let outcome = run_turn_generic(input, state, rt, a2a_secret, Some(agent), &mut sink);
    capture_from_outcome(sink, outcome)
}

/// Build TurnDeps from ReplState and run the turn loop.
fn run_turn_with_state(
    input: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
    a2a_secret: &[u8],
    agent_override: Option<&str>,
    sink: &mut impl TurnSink,
) -> TurnOutcome {
    let governed_runtime = state.service_context.infra().mcp.clone();
    let config = TurnConfig {
        max_loops: state.repl_settings.tool_loop_limit,
        gas_heuristic: state.repl_settings.gas_heuristic,
        saliency_window: state.repl_settings.condense_saliency_window,
        default_agent: state.current_agent.clone(),
        model_name: state.current_model.clone(),
        a2a_secret: hkask_types::secret::ZeroizingSecret::new(a2a_secret.to_vec()),
        principal_webid: state.host.resolve_user_webid(),
        agent_webid: state.agent_webid,
    };
    // Run manifest cascade once before the turn loop (not per-iteration).
    let effective_input = if let (Some(exec), Some(manif)) = (
        state.manifest_state.as_ref().map(|c| &c.executor),
        state.manifest_state.as_ref().map(|c| &c.manifest),
    ) {
        let ctx = rt.block_on(hkask_services_chat::ChatService::execute_manifest_cascade(
            exec,
            manif,
            input,
            &state.current_agent,
        ));
        match ctx {
            Some(c) => hkask_services_chat::ChatService::wrap_manifest_input(input, &c),
            None => input.to_string(),
        }
    } else {
        input.to_string()
    };

    let executor = super::deps::ReplTurnExecutor::from_state(state);
    let gas = super::deps::ReplGasGovernor::from_state(state, rt);
    let _svc_ctx = &state.service_context;
    #[cfg(feature = "tui")]
    let on_reg_update = || reg_display::update_reg_and_display(_svc_ctx, rt);
    #[cfg(not(feature = "tui"))]
    let on_reg_update = || {};
    let mut threads = super::deps::ReplThreadMemory::new(&mut state.thread_registry);
    let deps = TurnDeps {
        executor: &executor,
        gas: &gas,
        tools: governed_runtime.as_ref(),
        threads: &mut threads,
        on_reg_update: &on_reg_update,
    };
    run_turn_loop(
        effective_input.as_str(),
        deps,
        &config,
        rt,
        sink,
        agent_override,
    )
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::*;
    use hkask_services_chat::{TokenUsage, TurnResult};
    use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    // ── Compaction threshold tests ───────────────────────────────────
    #[test]
    fn compaction_triggers_above_87_5_percent() {
        let w: u32 = 4096;
        let threshold = crate::handlers::repl_settings::DEFAULT_CONDENSE_THRESHOLD as f64;
        let t = (w as f64 * threshold) as u64;
        let c = (w as f64 * 0.90 * 4.0) as usize;
        assert!((c as u64) / 4 > t);
    }
    #[test]
    fn compaction_skips_below_87_5_percent() {
        let w: u32 = 4096;
        let threshold = crate::handlers::repl_settings::DEFAULT_CONDENSE_THRESHOLD as f64;
        let t = (w as f64 * threshold) as u64;
        let c = (w as f64 * 0.80 * 4.0) as usize;
        assert!((c as u64) / 4 <= t);
    }
    #[test]
    fn compaction_threshold_matches_formula() {
        let threshold = crate::handlers::repl_settings::DEFAULT_CONDENSE_THRESHOLD as f64;
        for (w, e) in [(2048, 1792), (4096, 3584), (8192, 7168), (32768, 28672)] {
            assert_eq!((w as f64 * threshold) as u64, e);
        }
    }

    // ── emit_turn_status tests ───────────────────────────────────────
    struct MockSink {
        lines: Vec<String>,
    }
    impl MockSink {
        fn new() -> Self {
            Self { lines: vec![] }
        }
    }
    impl TurnSink for MockSink {
        fn agent_text(&mut self, a: &str, t: &str) {
            self.lines.push(format!("{}: {}", a, t));
        }
        fn thinking(&mut self, a: &str, t: &str) {
            self.lines.push(format!("{} thinking: {}", a, t));
        }
        fn tool_log(&mut self, l: &str) {
            self.lines.push(l.to_string());
        }
        fn status(&mut self, l: &str) {
            self.lines.push(l.to_string());
        }
    }

    #[test]
    fn emit_status_single_iteration_omits_across() {
        let mut s = MockSink::new();
        let u = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
        };
        emit_turn_status(&mut s, Some(&u), 1, 5000, 10000);
        assert!(
            s.lines
                .iter()
                .any(|l| l.contains("120 tokens") && !l.contains("across"))
        );
    }
    #[test]
    fn emit_status_multi_iteration_includes_across() {
        let mut s = MockSink::new();
        let u = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
        };
        emit_turn_status(&mut s, Some(&u), 3, 5000, 10000);
        assert!(s.lines.iter().any(|l| l.contains("across 3 iterations")));
    }
    #[test]
    fn emit_status_no_usage_nothing() {
        let mut s = MockSink::new();
        emit_turn_status(&mut s, None, 1, 5000, 10000);
        assert!(s.lines.is_empty());
    }
    #[test]
    fn emit_status_gas_low_warns() {
        let mut s = MockSink::new();
        emit_turn_status(&mut s, None, 1, 100, 10000);
        assert!(s.lines.iter().any(|l| l.contains("Gas budget low")));
    }
    #[test]
    fn emit_status_gas_exhausted_warns() {
        let mut s = MockSink::new();
        emit_turn_status(&mut s, None, 1, 0, 10000);
        assert!(s.lines.iter().any(|l| l.contains("Gas budget exhausted")));
    }
    #[test]
    fn emit_status_gas_healthy_no_warning() {
        let mut s = MockSink::new();
        emit_turn_status(&mut s, None, 1, 5000, 10000);
        assert!(s.lines.is_empty());
    }
    #[test]
    fn emit_status_gas_cap_zero_no_warning() {
        let mut s = MockSink::new();
        emit_turn_status(&mut s, None, 1, 0, 0);
        assert!(s.lines.is_empty());
    }

    // ── Mock implementations for loop tests ──────────────────────────

    struct MockExecutor {
        responses: Mutex<VecDeque<Result<TurnResult, ServiceError>>>,
    }
    impl MockExecutor {
        fn new() -> Self {
            Self {
                responses: Mutex::new(VecDeque::new()),
            }
        }
        fn then(mut self, r: TurnResult) -> Self {
            self.responses.get_mut().unwrap().push_back(Ok(r));
            self
        }
        fn then_error(mut self, msg: &str) -> Self {
            self.responses
                .get_mut()
                .unwrap()
                .push_back(Err(ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Inference,
                    source: None,
                    message: msg.to_string(),
                }));
            self
        }
    }
    #[async_trait::async_trait]
    impl TurnExecutor for MockExecutor {
        async fn execute_turn(&self, _input: &TurnInput<'_>) -> Result<TurnResult, ServiceError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Inference,
                    source: None,
                    message: "exhausted".to_string(),
                }))
        }
    }

    struct MockGas {
        remaining: u64,
        cap: u64,
    }
    impl MockGas {
        fn new(r: u64, c: u64) -> Self {
            Self {
                remaining: r,
                cap: c,
            }
        }
    }
    struct MockRes {
        h: u64,
        settled: bool,
        released: bool,
    }
    impl GasReservation for MockRes {
        fn heuristic(&self) -> u64 {
            self.h
        }
        fn settle(&mut self, _: u64) {
            self.settled = true;
        }
        fn release(&mut self) {
            self.released = true;
        }
    }
    impl GasGovernor for MockGas {
        fn try_reserve(&self, h: u64) -> Option<Box<dyn GasReservation>> {
            if self.remaining == 0 {
                None
            } else {
                Some(Box::new(MockRes {
                    h,
                    settled: false,
                    released: false,
                }))
            }
        }
        fn gas_status(&self) -> (u64, u64) {
            (self.remaining, self.cap)
        }
    }

    struct MockTools {
        results: std::collections::HashMap<String, serde_json::Value>,
    }
    impl MockTools {
        fn new() -> Self {
            Self {
                results: std::collections::HashMap::new(),
            }
        }
        fn returning(mut self, t: &str, v: serde_json::Value) -> Self {
            self.results.insert(t.to_string(), v);
            self
        }
    }
    impl hkask_capability::ToolPort for MockTools {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            tool: &'a str,
            _args: serde_json::Value,
            _token: &'a hkask_capability::DelegationToken,
        ) -> hkask_capability::ToolFuture<
            'a,
            Result<serde_json::Value, hkask_capability::ToolPortError>,
        > {
            Box::pin(async move {
                self.results.get(tool).cloned().ok_or_else(|| {
                    hkask_capability::ToolPortError::InvocationFailed(format!(
                        "no mock for {}",
                        tool
                    ))
                })
            })
        }
        fn discover_tools(&self) -> hkask_capability::ToolFuture<'_, Vec<String>> {
            Box::pin(async move { vec![] })
        }
        fn get_tool_info(
            &self,
            _: &str,
        ) -> hkask_capability::ToolFuture<'_, Option<hkask_capability::ToolInfo>> {
            Box::pin(async move { None })
        }
    }

    struct MockThreads {
        seeded: bool,
        mark_seeded_count: usize,
    }
    impl MockThreads {
        fn new() -> Self {
            Self {
                seeded: false,
                mark_seeded_count: 0,
            }
        }
        fn mark_seeded_count(&self) -> usize {
            self.mark_seeded_count
        }
    }
    impl ThreadMemory for MockThreads {
        fn is_seeded(&self) -> bool {
            self.seeded
        }
        fn thread_history_messages(&self, _: usize) -> Option<Vec<hkask_types::ChatMessage>> {
            None
        }
        fn append_turn(&mut self, _: &str, _: &str, _: &str) {}
        fn mark_seeded(&mut self) {
            self.seeded = true;
            self.mark_seeded_count += 1;
        }
    }

    fn turn_result(text: &str, tools: Vec<ToolCall>) -> TurnResult {
        use hkask_types::StructuredToolCall;
        TurnResult {
            text: text.to_string(),
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
            structured_tool_calls: tools
                .into_iter()
                .map(|tc| StructuredToolCall {
                    server: tc.server,
                    tool: tc.tool,
                    args: tc.args,
                    call_id: None,
                })
                .collect(),
            reasoning: None,
            messages: vec![],
        }
    }

    fn turn_result_with_reasoning(text: &str, reasoning: &str) -> TurnResult {
        let mut r = turn_result(text, vec![]);
        r.reasoning = Some(reasoning.to_string());
        r
    }
    fn tool_call(name: &str) -> ToolCall {
        ToolCall {
            server: "mock".into(),
            tool: name.into(),
            args: serde_json::json!({}),
        }
    }
    fn mock_config() -> TurnConfig {
        TurnConfig {
            max_loops: 21,
            gas_heuristic: 500,
            saliency_window: 5,
            default_agent: "TestAgent".into(),
            model_name: "test-model".into(),
            a2a_secret: hkask_types::secret::ZeroizingSecret::new(vec![]),
            principal_webid: hkask_types::WebID::from_persona_with_namespace(b"test", "userpod"),
            agent_webid: hkask_types::WebID::from_persona_with_namespace(b"test", "userpod"),
        }
    }
    fn noop() {}
    fn mock_deps<'a>(
        ex: &'a MockExecutor,
        gas: &'a MockGas,
        tools: &'a MockTools,
        threads: &'a mut MockThreads,
    ) -> TurnDeps<'a> {
        TurnDeps {
            executor: ex,
            gas,
            tools,
            threads,
            on_reg_update: &noop,
        }
    }

    // ── Loop regression tests ────────────────────────────────────────

    #[test]
    fn loop_displays_final_response_after_tool_calls() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ex = MockExecutor::new()
            .then(turn_result("Let me search.", vec![tool_call("search")]))
            .then(turn_result("The answer is 42.", vec![]));
        let gas = MockGas::new(10000, 10000);
        let tools = MockTools::new().returning("search", json!({"result": "42"}));
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let deps = TurnDeps {
            executor: &ex,
            gas: &gas,
            tools: &tools,
            threads: &mut threads,
            on_reg_update: &noop,
        };
        let outcome = run_turn_loop("q", deps, &mock_config(), rt.handle(), &mut sink, None);
        assert!(outcome.success);
        assert!(
            sink.lines.iter().any(|l| l.contains("The answer is 42.")),
            "final response must display after tool calls"
        );
    }

    /// Anti-regression for Cline #8636: reasoning must never be silently dropped
    /// and must render in a distinct "thinking" section, never mixed into the
    /// answer. A reasoning model's chain-of-thought is surfaced via the default
    /// `execute_turn_streaming` `Thinking` chunk → `sink.thinking`, separate
    /// from the `Delta`/answer path.
    #[test]
    fn thinking_emitted_to_sink_separately() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Executor returns a final answer WITH a reasoning trace and no tool
        // calls — the turn loop must terminate after one iteration.
        let ex = MockExecutor::new().then(turn_result_with_reasoning(
            "Paris",
            "The user asked for the capital of France. France's capital is Paris.",
        ));
        let gas = MockGas::new(10000, 10000);
        let tools = MockTools::new();
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let deps = mock_deps(&ex, &gas, &tools, &mut threads);
        let outcome = run_turn_loop(
            "capital of France?",
            deps,
            &mock_config(),
            rt.handle(),
            &mut sink,
            None,
        );
        assert!(outcome.success);

        // The reasoning trace must reach the sink under the "thinking" channel.
        let thinking_line = sink
            .lines
            .iter()
            .find(|l| l.contains("thinking:"))
            .expect("reasoning must be emitted to the thinking sink, not dropped");
        assert!(
            thinking_line.contains("capital of France"),
            "thinking line must carry the chain-of-thought: {thinking_line}"
        );

        // The answer line must contain the final answer and NOT the deliberation —
        // chain-of-thought must never mix into the answer (the Cline #8636 bug).
        let answer_lines: Vec<&String> = sink
            .lines
            .iter()
            .filter(|l| !l.contains("thinking:") && !l.starts_with(' '))
            .collect();
        let combined = answer_lines
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(combined.contains("Paris"), "answer must contain Paris");
        assert!(
            !combined.contains("capital of France. France's capital is Paris"),
            "chain-of-thought must not be mixed into the answer"
        );
    }

    #[test]
    fn loop_shows_inference_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ex = MockExecutor::new().then_error("connection refused");
        let gas = MockGas::new(10000, 10000);
        let tools = MockTools::new();
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let deps = mock_deps(&ex, &gas, &tools, &mut threads);
        let outcome = run_turn_loop("q", deps, &mock_config(), rt.handle(), &mut sink, None);
        assert!(!outcome.success);
        assert!(sink.lines.iter().any(|l| l.contains("Inference error")));
    }

    #[test]
    fn loop_no_mark_seeded_on_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ex = MockExecutor::new().then_error("fail");
        let gas = MockGas::new(10000, 10000);
        let tools = MockTools::new();
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let deps = mock_deps(&ex, &gas, &tools, &mut threads);
        let _ = run_turn_loop("q", deps, &mock_config(), rt.handle(), &mut sink, None);
        assert_eq!(
            threads.mark_seeded_count(),
            0,
            "mark_seeded must not be called on inference error"
        );
    }

    #[test]
    fn loop_marks_seeded_on_success() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ex = MockExecutor::new().then(turn_result("Hello!", vec![]));
        let gas = MockGas::new(10000, 10000);
        let tools = MockTools::new();
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let deps = mock_deps(&ex, &gas, &tools, &mut threads);
        let _ = run_turn_loop("q", deps, &mock_config(), rt.handle(), &mut sink, None);
        assert_eq!(
            threads.mark_seeded_count(),
            1,
            "mark_seeded must be called on success"
        );
    }

    #[test]
    fn loop_displays_preamble() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ex = MockExecutor::new()
            .then(turn_result("Let me check.", vec![tool_call("check")]))
            .then(turn_result("Done!", vec![]));
        let gas = MockGas::new(10000, 10000);
        let tools = MockTools::new().returning("check", json!({"ok": true}));
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let deps = mock_deps(&ex, &gas, &tools, &mut threads);
        let _ = run_turn_loop("q", deps, &mock_config(), rt.handle(), &mut sink, None);
        assert!(
            sink.lines.iter().any(|l| l.contains("Let me check.")),
            "preamble must display before tool calls"
        );
    }

    #[test]
    fn loop_warns_on_max_iterations() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ex = MockExecutor::new()
            .then(turn_result("x", vec![tool_call("loop")]))
            .then(turn_result("x", vec![tool_call("loop")]))
            .then(turn_result("x", vec![tool_call("loop")]));
        let gas = MockGas::new(100000, 100000);
        let tools = MockTools::new().returning("loop", json!({}));
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let mut cfg = mock_config();
        cfg.max_loops = 2;
        let deps = mock_deps(&ex, &gas, &tools, &mut threads);
        let _ = run_turn_loop("q", deps, &cfg, rt.handle(), &mut sink, None);
        assert!(sink.lines.iter().any(|l| l.contains("max iterations")));
    }

    #[test]
    fn loop_grows_message_array_with_correct_roles() {
        use std::sync::{Arc, Mutex};

        let rt = tokio::runtime::Runtime::new().unwrap();

        // Mock executor that captures TurnInput.messages on each call.
        let captured: Arc<Mutex<Vec<Option<Vec<hkask_types::ChatMessage>>>>> =
            Arc::new(Mutex::new(vec![]));

        struct CapturingExecutor {
            captured: Arc<Mutex<Vec<Option<Vec<hkask_types::ChatMessage>>>>>,
        }

        #[async_trait::async_trait]
        impl TurnExecutor for CapturingExecutor {
            async fn execute_turn(
                &self,
                input: &TurnInput<'_>,
            ) -> Result<TurnResult, ServiceError> {
                self.captured.lock().unwrap().push(input.messages.clone());
                let captured = self.captured.lock().unwrap();
                if captured.len() == 1 {
                    // Iteration 1: return tool calls + initial messages
                    Ok(TurnResult {
                        text: "Let me search for that.".to_string(),
                        usage: TokenUsage {
                            prompt_tokens: 10,
                            completion_tokens: 5,
                            total_tokens: 15,
                        },
                        structured_tool_calls: vec![hkask_types::StructuredToolCall {
                            server: "".to_string(),
                            tool: "search".to_string(),
                            args: json!({"q": "test"}),
                            call_id: None,
                        }],
                        reasoning: None,
                        messages: vec![
                            hkask_types::ChatMessage::system("You are a test agent."),
                            hkask_types::ChatMessage::user("What is the capital of France?"),
                        ],
                    })
                } else {
                    // Iteration 2: return final response, no tool calls
                    Ok(TurnResult {
                        text: "The capital of France is Paris.".to_string(),
                        usage: TokenUsage {
                            prompt_tokens: 20,
                            completion_tokens: 10,
                            total_tokens: 30,
                        },
                        structured_tool_calls: vec![],
                        reasoning: None,
                        messages: vec![],
                    })
                }
            }
        }

        let ex = CapturingExecutor {
            captured: captured.clone(),
        };
        let gas = MockGas::new(100000, 100000);
        let tools = MockTools::new().returning("search", json!({"result": "Paris"}));
        let mut threads = MockThreads::new();
        let mut sink = MockSink::new();
        let cfg = mock_config();
        let deps = TurnDeps {
            executor: &ex,
            gas: &gas,
            tools: &tools,
            threads: &mut threads,
            on_reg_update: &noop,
        };
        let _ = run_turn_loop(
            "What is the capital of France?",
            deps,
            &cfg,
            rt.handle(),
            &mut sink,
            None,
        );

        // Verify: 2 calls were made
        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 2, "should have 2 executor calls");

        // Iteration 1: messages should be None (first iteration, no prebuilt messages)
        assert!(
            captured[0].is_none(),
            "iteration 1 should have no prebuilt messages"
        );

        // Iteration 2: messages should contain the growing array with correct roles
        let iter2_messages = captured[1]
            .as_ref()
            .expect("iteration 2 should have prebuilt messages");
        assert!(
            iter2_messages.len() >= 4,
            "iteration 2 should have system + user + assistant + tool_results"
        );

        // Find the assistant message — it should have role "assistant", NOT "user"
        let has_assistant = iter2_messages
            .iter()
            .any(|m| m.role == "assistant" && m.content.contains("Let me search"));
        assert!(
            has_assistant,
            "iteration 2 messages must contain the assistant response with role=assistant"
        );

        // Verify no role inversion: the assistant response must NOT appear as role="user"
        let no_role_inversion = !iter2_messages
            .iter()
            .any(|m| m.role == "user" && m.content.contains("Let me search"));
        assert!(
            no_role_inversion,
            "assistant response must NOT be tagged as role=user (role inversion bug)"
        );

        // Find the tool results message — it should have role "user"
        let has_tool_results = iter2_messages
            .iter()
            .any(|m| m.role == "user" && m.content.contains("search"));
        assert!(
            has_tool_results,
            "iteration 2 messages must contain tool results with role=user"
        );
    }
}

#[cfg(all(test, feature = "tui"))]
mod capture_sink_tests {
    use super::*;

    #[test]
    fn agent_text_to_response() {
        let mut s = CaptureSink::new();
        s.agent_text("A", "hi");
        assert!(s.response_text.contains("hi"));
        assert!(s.tool_output.is_empty());
    }
    #[test]
    fn tool_log_to_output() {
        let mut s = CaptureSink::new();
        s.tool_log("invoking");
        assert!(s.tool_output.contains("invoking"));
        assert!(s.response_text.is_empty());
    }
    #[test]
    fn status_tokens_filtered() {
        let mut s = CaptureSink::new();
        s.status("  120 tokens (100 prompt + 20 completion)");
        assert!(s.response_text.is_empty());
    }
    #[test]
    fn status_error_captured() {
        let mut s = CaptureSink::new();
        s.status("  Inference error: fail");
        assert!(s.response_text.contains("Inference error"));
    }
    #[test]
    fn status_gas_warning_captured() {
        let mut s = CaptureSink::new();
        s.status("  Gas budget low: 100/10000 (1%)");
        assert!(s.response_text.contains("Gas budget low"));
    }
    #[test]
    fn status_max_iter_captured() {
        let mut s = CaptureSink::new();
        s.status("  max iterations reached");
        assert!(s.response_text.contains("max iterations"));
    }
}

// ── Tool call parsing (inlined from deleted tool_augmented.rs) ──────────

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub server: String,
    pub tool: String,
    pub args: serde_json::Value,
}

impl From<hkask_types::StructuredToolCall> for ToolCall {
    fn from(stc: hkask_types::StructuredToolCall) -> Self {
        Self {
            server: stc.server,
            tool: stc.tool,
            args: stc.args,
        }
    }
}

pub struct ParsedResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
}

pub fn extract_tool_calls(
    response_text: &str,
    structured_tool_calls: Option<&[hkask_types::StructuredToolCall]>,
) -> ParsedResponse {
    let tool_calls = structured_tool_calls
        .unwrap_or(&[])
        .iter()
        .cloned()
        .map(ToolCall::from)
        .collect();
    ParsedResponse {
        text: response_text.to_string(),
        tool_calls,
    }
}

pub fn format_tool_results(calls: &[(ToolCall, anyhow::Result<serde_json::Value>)]) -> String {
    if calls.is_empty() {
        return String::new();
    }
    let mut parts = vec!["Tool results:".to_string(), String::new()];
    for (call, result) in calls {
        match result {
            Ok(value) => {
                let formatted =
                    serde_json::to_string_pretty(value).unwrap_or_else(|_| format!("{:?}", value));
                parts.push(format!("✓ {} → {}", call.tool, formatted));
            }
            Err(err) => parts.push(format!("✗ {} → ERROR: {}", call.tool, err)),
        }
    }
    parts.join("\n")
}
