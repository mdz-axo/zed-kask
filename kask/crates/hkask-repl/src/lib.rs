#![deny(unsafe_code)]
//! Interactive REPL for hKask — discoverable, self-documenting, alive.
//!
//! Design principles:
//! - Every capability is reachable from `/help`
//! - Tab completion for slash commands and agent names
//! - Fuzzy matching on slash commands (e.g. `/model`)
//! - Welcome banner with the Kask amphora logo
//! - Categorized help so the menu is scannable

mod builtin_servers;

mod commands;
pub mod deps;
pub mod display;
mod energy;
pub mod handlers;
mod helper;
pub mod host;
#[cfg(feature = "tui")]
mod reg_display;
#[cfg(feature = "tui")]
pub use host::{OnboardingError, OnboardingOutcome, ReplHost};
mod init;
mod threads;
mod turn;

#[cfg(feature = "tui")]
pub mod tui;

use hkask_services_context::AgentService;
use hkask_services_kata_kanban::KanbanService;
use hkask_templates::{BundleManifest, ManifestExecutor, SqliteRegistry};
use hkask_types::WebID;
use hkask_types::secret::ZeroizingSecret;
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config as ReadlineConfig, Editor};
use std::sync::Arc;

// Dependencies used via #[derive] or in submodules not directly importable.
use async_trait as _;
use hkask_memory as _;

use commands::handle_slash_command;
use handlers::ReplSettings;
#[cfg(feature = "tui")]
use handlers::repl_settings::DEFAULT_CONTEXT_WINDOW;
use helper::KaskHelper;

/// Talk configuration — paired voice design and enabled state.
///
/// Set together via `/talk on|off|voice`. Checked in the turn pipeline
/// to decide whether to summarize and speak responses aloud.
#[derive(Debug, Clone)]
pub struct TalkConfig {
    /// Whether spoken summaries are emitted. Uses an enum (not `bool`)
    /// so the invalid state "off but has voice_design" is representable
    /// but the on/off decision is explicit at the type level.
    pub mode: TalkMode,
    /// Voice design JSON for TTS (None = default "Rachel" voice).
    pub voice_design: Option<String>,
}

/// Talk mode — whether the REPL speaks responses aloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkMode {
    /// Spoken summaries are emitted after each turn.
    On,
    /// No spoken output.
    Off,
}

/// Manifest cascade — process manifest paired with its executor.
///
/// The two fields are always present together: the manifest defines the
/// steps, the executor runs them. Wrapping in a single `Option` enforces
/// the "both Some or both None" invariant at the type level — the invalid
/// state `Some(manifest) + None(executor)` is unrepresentable.
///
/// Cannot derive `Debug` because `ManifestExecutor` wraps non-Debug types
/// (trait objects, secrets).
#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct ManifestCascade {
    pub manifest: BundleManifest,
    pub executor: ManifestExecutor,
}

/// Manifest state — `Some` when the agent has a process manifest cascade
/// defined, `None` otherwise.
pub type ManifestState = Option<ManifestCascade>;

/// REPL state — surface-specific presentation fields.
/// All infrastructure (inference, memory, tool dispatch, gas tracking)
/// is accessed through `service_context: Arc<AgentService>`.
pub struct ReplState {
    pub agent_webid: WebID,
    pub current_model: String,
    pub current_agent: String,
    pub active_session: Option<String>,
    pub resolved_secrets: Option<hkask_services_onboarding::ResolvedSecrets>,
    /// Tool definitions for native function calling. Discovered from McpRuntime
    /// at init and refreshed when servers start/stop.
    pub tool_definitions: Vec<hkask_types::ChatToolDefinition>,
    pub manifest_state: ManifestState,
    pub service_context: Arc<AgentService>,
    pub repl_settings: ReplSettings,
    pub is_first_run: bool,
    /// Talk configuration — voice design and enabled state.
    /// Set via /talk command; checked in the turn pipeline.
    pub talk_config: TalkConfig,
    /// Active improv mode — set via /improv command.
    /// None means no improv posture is active (default agent behavior).
    pub improv_mode: Option<hkask_services_chat::ImprovMode>,
    /// Kanban service — lazily initialized for /kanban commands.
    pub kanban_service: Option<KanbanService>,
    /// MCP servers that failed to auto-start (server_id → error message).
    /// Populated during REPL init; surfaced in the session banner so the user
    /// knows which capabilities are degraded before their first prompt.
    pub degraded_servers: Vec<(String, String)>,
    /// Chat thread registry — persists conversation threads across sessions.
    /// Loaded from `agents/{name}/threads.json` on REPL init. Supports
    /// thread listing, switching, creation, and archival with configurable
    /// short-term memory lifespan.
    pub thread_registry: threads::ThreadRegistry,
    /// Host trait — provides WebID resolution, onboarding, template
    /// listing, and transcript viewing to the REPL subsystem.
    pub host: Arc<dyn host::ReplHost>,
}

impl std::fmt::Debug for ReplState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplState")
            .field("agent_webid", &self.agent_webid)
            .field("current_model", &self.current_model)
            .field("current_agent", &self.current_agent)
            .field("active_session", &self.active_session)
            // Redacted: carries master key and DB passphrase.
            .field("resolved_secrets", &"<redacted>")
            .field("tool_definitions", &self.tool_definitions)
            // ManifestCascade wraps non-Debug trait objects.
            .field(
                "manifest_state",
                &if self.manifest_state.is_some() {
                    "Some(<ManifestCascade>)"
                } else {
                    "None"
                },
            )
            .field("service_context", &"<AgentService>")
            .field("repl_settings", &self.repl_settings)
            .field("is_first_run", &self.is_first_run)
            .field("talk_config", &self.talk_config)
            .field("improv_mode", &self.improv_mode)
            .field("kanban_service", &self.kanban_service.is_some())
            .field("degraded_servers", &self.degraded_servers)
            .field("thread_registry", &self.thread_registry)
            .field("host", &"<ReplHost>")
            .finish()
    }
}

pub fn run(
    _registry: &mut SqliteRegistry,
    template_id: Option<&str>,
    _userpod_name: &str,
    initial_model: Option<&str>,
    rt_handle: tokio::runtime::Handle,
    host: Arc<dyn host::ReplHost>,
) {
    let Some(state) = init::init_repl_state(_registry, initial_model, &rt_handle, host) else {
        return;
    };
    run_with_state(state, template_id, rt_handle);
}

/// Execute one input through the standard governed REPL turn loop.
///
/// Used by non-interactive surfaces that must retain the REPL's MCP tool
/// dispatch, OCAP delegation, gas accounting, and Regulation updates.
pub fn run_once(
    registry: &mut SqliteRegistry,
    initial_model: Option<&str>,
    input: &str,
    mcp_servers: &[String],
    rt_handle: tokio::runtime::Handle,
    host: Arc<dyn host::ReplHost>,
) {
    let Some(mut state) = init::init_repl_state(registry, initial_model, &rt_handle, host) else {
        return;
    };
    let runtime = state.service_context.infra().mcp.clone();
    for server_id in mcp_servers {
        if !rt_handle.block_on(builtin_servers::start_single_server(
            runtime.as_ref(),
            server_id,
        )) {
            eprintln!("Failed to load MCP server: {server_id}");
            return;
        }
    }
    if !mcp_servers.is_empty() {
        state.tool_definitions = init::discover_tools(&runtime, &rt_handle);
    }
    let Some(secrets) = &state.resolved_secrets else {
        eprintln!("Error: No A2A secret resolved. Complete onboarding before invoking tools.");
        return;
    };
    let a2a_secret = ZeroizingSecret::new(secrets.a2a_secret.as_bytes().to_vec());
    turn::single_agent_turn(input, &mut state, &rt_handle, &a2a_secret, None);
}

fn run_with_state(
    mut state: ReplState,
    template_id: Option<&str>,
    rt_handle: tokio::runtime::Handle,
) {
    let helper = KaskHelper::new(state.thread_registry.clone());

    let rl_config = ReadlineConfig::builder()
        .history_ignore_space(true)
        .history_ignore_dups(true)
        .expect("invalid readline config")
        .completion_type(CompletionType::List)
        .build();

    let mut rl = match Editor::with_config(rl_config) {
        Ok(editor) => editor,
        Err(e) => {
            eprintln!("Failed to initialize readline: {}", e);
            return;
        }
    };
    rl.set_helper(Some(helper));

    if rl.load_history(&history_path()).is_err() {
        // No history file yet — that's fine
    }

    display::print_banner(
        &state.current_agent,
        template_id,
        &state.current_model,
        state.is_first_run,
        &state.degraded_servers,
    );

    loop {
        let prompt = format!("[1m{}[0m>> ", display::model_abbrev(&state.current_model));
        match rl.readline(&prompt) {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(input.to_owned());

                if input.starts_with('/') {
                    if handle_slash_command(input, template_id, &rt_handle, &mut state) {
                        let _ = rl.save_history(&history_path());
                        break;
                    }
                    continue;
                }

                if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
                    println!("Goodbye!");
                    let _ = rl.save_history(&history_path());
                    break;
                }

                let rt = rt_handle.clone();

                // A2A secret for signing capability tokens in tool invocations.
                // Derived from onboarding — same secret that governs OCAP authority.
                // Wrapped in ZeroizingSecret for defense-in-depth: the secret bytes
                // are scrubbed from memory on drop.
                let a2a_secret = match &state.resolved_secrets {
                    Some(secrets) => ZeroizingSecret::new(secrets.a2a_secret.as_bytes().to_vec()),
                    None => {
                        eprintln!(
                            "Error: No A2A secret resolved. Run `kask chat` to complete onboarding or set HKASK_MASTER_KEY."
                        );
                        continue;
                    }
                };

                if let Some(ref _session) = state.active_session.clone() {
                    // Dual-presence active. Fall back to single-agent mode.
                    state.active_session = None;
                    turn::single_agent_turn(input, &mut state, &rt, &a2a_secret, None);
                } else {
                    turn::single_agent_turn(input, &mut state, &rt, &a2a_secret, None);
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("(Ctrl+C — type /quit to exit)");
            }
            Err(ReadlineError::Eof) => {
                println!("Goodbye!");
                let _ = rl.save_history(&history_path());
                break;
            }
            Err(err) => {
                eprintln!("Readline error: {}", err);
                let _ = rl.save_history(&history_path());
                break;
            }
        }
    }
}

fn history_path() -> std::path::PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("hkask");
    let _ = std::fs::create_dir_all(&path);
    path.push("kask_history.txt");
    path
}

/// Launch the TUI workspace instead of the line-based REPL.
#[cfg(feature = "tui")]
pub fn run_tui(
    registry: &mut SqliteRegistry,
    template_id: Option<&str>,
    _userpod_name: &str,
    initial_model: Option<&str>,
    rt_handle: tokio::runtime::Handle,
    host: Arc<dyn host::ReplHost>,
) {
    let Some(state) = init::init_repl_state(registry, initial_model, &rt_handle, host) else {
        return;
    };

    let userpod_name = state.current_agent.clone();
    let model = state.current_model.clone();

    // Resolve A2A secret for capability tokens. Wrapped in ZeroizingSecret
    // so the bytes are scrubbed from memory when the bridge is dropped.
    let a2a_secret = state
        .resolved_secrets
        .as_ref()
        .map(|s| hkask_types::secret::ZeroizingSecret::new(s.a2a_secret.as_bytes().to_vec()))
        .unwrap_or_else(|| hkask_types::secret::ZeroizingSecret::new(Vec::new()));

    // Compute layout path before userpod_name is moved into the bridge
    let layout_path = crate::tui::layout::layout_path(&userpod_name);

    // Keep ReplState alive inside the bridge for full inference
    let bridge = Arc::new(TuiReplBridge {
        state: Arc::new(std::sync::Mutex::new(state)),
        rt_handle: rt_handle.clone(),
        a2a_secret,
        userpod_name,
        model,
        pending: std::sync::Mutex::new(std::collections::HashMap::new()),
        pending_invokes: std::sync::Mutex::new(std::collections::HashMap::new()),
        alert_count: std::sync::atomic::AtomicU32::new(0),
        context_window: std::sync::atomic::AtomicU32::new(DEFAULT_CONTEXT_WINDOW),
        context_used: std::sync::atomic::AtomicU32::new(0),
    });

    match crate::tui::TuiSession::new(
        bridge.clone() as Arc<dyn crate::tui::SystemBridge>,
        bridge.clone(),
    ) {
        Ok(session) => {
            let session = session
                .with_layout_path(layout_path)
                .with_settings_bridge(bridge.clone())
                .with_session_bridge(bridge.clone())
                .with_tool_invoke_bridge(bridge.clone());
            let mut session = session;
            if let Err(e) = session.run() {
                eprintln!("TUI error: {}", e);
            }
        }
        Err(e) => {
            eprintln!("Failed to initialize TUI: {e}");
            eprintln!("Falling back to line-based REPL.");
            let Ok(bridge) = Arc::try_unwrap(bridge) else {
                eprintln!("Cannot recover initialized REPL state after TUI failure.");
                return;
            };
            let Ok(state) = Arc::try_unwrap(bridge.state) else {
                eprintln!("Cannot recover shared REPL state after TUI failure.");
                return;
            };
            run_with_state(
                state
                    .into_inner()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
                template_id,
                rt_handle,
            );
        }
    }
}

/// Map a ToolPortError to a TUI-facing McpInvokeError.
#[cfg(feature = "tui")]
fn map_tool_port_error(
    err: hkask_capability::ToolPortError,
    tool: &str,
) -> crate::tui::McpInvokeError {
    match err {
        hkask_capability::ToolPortError::NotFound(_) => {
            crate::tui::McpInvokeError::ToolNotFound(tool.to_string())
        }
        hkask_capability::ToolPortError::CapabilityDenied(msg) => {
            crate::tui::McpInvokeError::Server(msg)
        }
        hkask_capability::ToolPortError::EnergyBudgetExceeded(msg) => {
            crate::tui::McpInvokeError::Server(msg)
        }
        hkask_capability::ToolPortError::InvocationFailed(msg) => {
            crate::tui::McpInvokeError::Server(msg)
        }
    }
}

/// Receiver and partial output owned by one TUI inference request.
#[cfg(feature = "tui")]
struct PendingTuiInference {
    receiver: std::sync::mpsc::Receiver<crate::tui::TuiTurnResult>,
    streaming_text: Arc<std::sync::Mutex<String>>,
}

/// Receiver for one TUI MCP tool invocation.
#[cfg(feature = "tui")]
struct PendingTuiInvoke {
    receiver: std::sync::mpsc::Receiver<Result<serde_json::Value, crate::tui::McpInvokeError>>,
}

/// Bridge implementation connecting the TUI to hKask's full inference engine.
#[cfg(feature = "tui")]
struct TuiReplBridge {
    state: Arc<std::sync::Mutex<ReplState>>,
    rt_handle: tokio::runtime::Handle,
    a2a_secret: hkask_types::secret::ZeroizingSecret,
    userpod_name: String,
    model: String,
    pending: std::sync::Mutex<
        std::collections::HashMap<crate::tui::InferenceRequestId, PendingTuiInference>,
    >,
    /// Pending MCP tool invocations (start/poll pattern).
    pending_invokes: std::sync::Mutex<
        std::collections::HashMap<crate::tui::McpInvokeRequestId, PendingTuiInvoke>,
    >,
    alert_count: std::sync::atomic::AtomicU32,
    /// Context window size from model metadata
    context_window: std::sync::atomic::AtomicU32,
    /// Approximate current context usage in tokens
    context_used: std::sync::atomic::AtomicU32,
}

#[cfg(feature = "tui")]
impl TuiReplBridge {
    fn build_result(capture: &turn::TurnCapture) -> crate::tui::TuiTurnResult {
        if capture.budget_exhausted {
            return crate::tui::TuiTurnResult {
                text: String::new(),
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                gas_cost: 0,
                iterations: 0,
                budget_exhausted: true,
            };
        }
        let mut text = capture.response_text.clone();
        if !capture.tool_output.is_empty() {
            text.push_str("\n\n── Tool Results ──\n");
            text.push_str(&capture.tool_output);
        }
        crate::tui::TuiTurnResult {
            text,
            prompt_tokens: capture.prompt_tokens,
            completion_tokens: capture.completion_tokens,
            total_tokens: capture.total_tokens,
            gas_cost: 1u64
                .max(capture.prompt_tokens as u64 / 100 + capture.completion_tokens as u64 / 25),
            iterations: capture.iterations,
            budget_exhausted: false,
        }
    }
}

#[cfg(feature = "tui")]
impl crate::tui::ReplBridge for TuiReplBridge {
    fn start_inference(&self, input: String) -> crate::tui::InferenceRequestId {
        let state = self.state.clone();
        let rt = self.rt_handle.clone();
        let a2a = self.a2a_secret.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let streaming = Arc::new(std::sync::Mutex::new(String::new()));
        let request = crate::tui::InferenceRequestId::new();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                request,
                PendingTuiInference {
                    receiver: rx,
                    streaming_text: streaming.clone(),
                },
            );

        std::thread::spawn(move || {
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            let capture = turn::single_agent_turn_captured(&input, &mut s, &rt, a2a.as_bytes());
            let result = Self::build_result(&capture);

            // Publish the full response text to the streaming buffer so the
            // TUI can render it immediately on the next poll. The previous
            // implementation faked streaming by chunking the text 3 chars at
            // a time with an 8ms sleep — this added latency (a 1000-char
            // response took ~2.7s of artificial delay on top of the actual
            // inference time) and blocked a thread doing nothing. Real
            // streaming should be wired through InferencePort::generate_stream
            // if progressive display is desired.
            if let Ok(mut buf) = streaming.lock() {
                buf.push_str(&result.text);
            }

            let _ = tx.send(result);
        });
        request
    }

    fn start_scoped_inference(
        &self,
        input: String,
        mcp_server: &str,
    ) -> crate::tui::InferenceRequestId {
        let state = self.state.clone();
        let rt = self.rt_handle.clone();
        let a2a = self.a2a_secret.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let streaming = Arc::new(std::sync::Mutex::new(String::new()));
        let scope = mcp_server.to_string();
        let request = crate::tui::InferenceRequestId::new();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                request,
                PendingTuiInference {
                    receiver: rx,
                    streaming_text: streaming.clone(),
                },
            );

        std::thread::spawn(move || {
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            let runtime = s.service_context.infra().mcp.clone();
            if rt.block_on(builtin_servers::start_single_server(
                runtime.as_ref(),
                &scope,
            )) {
                s.tool_definitions = init::discover_tools(&runtime, &rt);
            }

            // Scope tool definitions to the requested MCP server. We save
            // and restore the originals around the turn. If the turn panics,
            // the definitions would be left scoped — but the thread is
            // isolated and the next /mcp start refreshes tool_prompt anyway.
            // A panic-safe guard would require passing scoped tools as a
            // parameter to single_agent_turn_captured (a larger refactor).
            let original_tools = std::mem::take(&mut s.tool_definitions);
            let prefix = format!("{}/", scope);
            s.tool_definitions = original_tools
                .iter()
                .filter(|td| td.function.name.starts_with(&prefix))
                .cloned()
                .collect();

            let capture = turn::single_agent_turn_captured(&input, &mut s, &rt, a2a.as_bytes());

            // Restore original tool definitions.
            s.tool_definitions = original_tools;

            let result = Self::build_result(&capture);

            // Publish the full response text to the streaming buffer immediately.
            // See start_inference for why we don't fake-stream chunk by chunk.
            if let Ok(mut buf) = streaming.lock() {
                buf.push_str(&result.text);
            }

            let _ = tx.send(result);
        });
        request
    }

    fn poll_inference(
        &self,
        request: crate::tui::InferenceRequestId,
    ) -> crate::tui::InferenceState {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let Some(operation) = pending.get(&request) else {
            return crate::tui::InferenceState::Idle;
        };
        match operation.receiver.try_recv() {
            Ok(result) => {
                pending.remove(&request);
                // Update status bar metrics — previously only in the now-deleted
                // send_message_blocking (which had zero callers). This is the
                // actual hot path: the TUI polls on every tick (16ms).
                self.context_used
                    .fetch_add(result.total_tokens, std::sync::atomic::Ordering::Relaxed);
                if result.budget_exhausted {
                    self.alert_count
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                // Refresh context window from model metadata.
                if let Ok(state) = self.state.lock() {
                    let ctx_len = state
                        .repl_settings
                        .model_meta
                        .as_ref()
                        .map(|m| m.context_length)
                        .unwrap_or(DEFAULT_CONTEXT_WINDOW);
                    self.context_window
                        .store(ctx_len, std::sync::atomic::Ordering::Relaxed);
                }
                crate::tui::InferenceState::Done(result)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => crate::tui::InferenceState::Thinking,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                pending.remove(&request);
                crate::tui::InferenceState::Idle
            }
        }
    }

    fn streaming_text(&self, request: crate::tui::InferenceRequestId) -> String {
        let streaming = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&request)
            .map(|operation| operation.streaming_text.clone());
        streaming
            .and_then(|buffer| buffer.lock().ok().map(|text| text.clone()))
            .unwrap_or_default()
    }

    fn send_curator_message(&self, input: &str) -> String {
        // Run the turn through the real inference pipeline as the Curator
        // agent — NOT a canned stub. The operator's message reaches a live
        // agent (memory recall, tool dispatch, inference) and the captured
        // response is returned for the CuratorWindow to render.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let capture = turn::single_agent_turn_captured_with_agent(
            input,
            &mut state,
            &self.rt_handle,
            self.a2a_secret.as_bytes(),
            "Curator",
        );
        if capture.response_text.is_empty() {
            "(Curator produced no response — check agent registration and provider reachability.)"
                .to_string()
        } else {
            capture.response_text
        }
    }

    fn handle_command(&self, cmd: &str) -> crate::tui::CommandResult {
        let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
        let primary = parts.first().copied().unwrap_or("");
        match primary {
            // /fusion — show fusion status from inference config
            "fusion" => {
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(ref fusion) = state.service_context.config().inference_config.fusion {
                    crate::tui::CommandResult {
                        text: format!("Fusion: {:?} — {}", fusion.mode, fusion.description()),
                        should_quit: false,
                    }
                } else {
                    crate::tui::CommandResult {
                        text: "Fusion not configured. Set HKASK_FUSION_MODE + HKASK_FUSION_PANEL_MODELS env vars.".into(),
                        should_quit: false,
                    }
                }
            }
            // /ask <agent> <message> — blocking turn with a specific agent
            "ask" => {
                let agent = parts.get(1).copied().unwrap_or("");
                let message = parts.get(2).copied().unwrap_or("");
                if agent.is_empty() || message.is_empty() {
                    return crate::tui::CommandResult {
                        text: "Usage: /ask <agent> <message>".into(),
                        should_quit: false,
                    };
                }
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                let capture = turn::single_agent_turn_captured_with_agent(
                    message,
                    &mut state,
                    &self.rt_handle,
                    self.a2a_secret.as_bytes(),
                    agent,
                );
                crate::tui::CommandResult {
                    text: if capture.response_text.is_empty() {
                        format!(
                            "({} produced no response — check agent registration and provider reachability.)",
                            agent
                        )
                    } else {
                        capture.response_text
                    },
                    should_quit: false,
                }
            }
            // /thread — show thread info from registry
            "thread" | "th" => {
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                let reg = &state.thread_registry;
                let active = reg
                    .active_thread_id
                    .clone()
                    .unwrap_or_else(|| "none".into());
                crate::tui::CommandResult {
                    text: format!(
                        "Active thread: {}\nThreads: {}\nSeeded: {}\nUse `kask thread` CLI for full management.",
                        active,
                        reg.threads.len(),
                        reg.seeded
                    ),
                    should_quit: false,
                }
            }
            _ => crate::tui::CommandResult {
                text: format!(
                    "Command /{} not available in TUI. Use `kask {}` in the CLI.",
                    primary, primary
                ),
                should_quit: false,
            },
        }
    }
}

#[cfg(feature = "tui")]
impl crate::tui::SystemBridge for TuiReplBridge {
    fn userpod_name(&self) -> &str {
        &self.userpod_name
    }
    fn model_name(&self) -> &str {
        &self.model
    }
    fn gas_remaining(&self) -> u64 {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.service_context.gas_remaining())
            .unwrap_or(0)
    }
    fn gas_cap(&self) -> u64 {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.service_context.gas_cap())
            .unwrap_or(0)
    }
    fn reg_alert_count(&self) -> u32 {
        self.alert_count.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn context_pressure(&self) -> f64 {
        let used = self.context_used.load(std::sync::atomic::Ordering::Relaxed);
        let window = self
            .context_window
            .load(std::sync::atomic::Ordering::Relaxed);
        if window == 0 {
            0.0
        } else {
            (used as f64 / window as f64).min(1.0)
        }
    }
    fn mcp_status(&self) -> (usize, usize) {
        if let Ok(s) = self.state.lock() {
            let runtime = s.service_context.infra().mcp.clone().clone();
            let loaded = self.rt_handle.block_on(runtime.list_servers()).len();
            let total = hkask_mcp_server::BUILTIN_SERVERS.len();
            (loaded, total)
        } else {
            (0, hkask_mcp_server::BUILTIN_SERVERS.len())
        }
    }
    fn pod_counts(&self) -> Option<(usize, usize)> {
        if self.state.lock().is_ok() {
            let data_dir = dirs::data_local_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("hkask");
            let registry = hkask_pods::PodRegistry::new(&data_dir);
            match registry.scan_by_kind() {
                Ok(pods) => {
                    let mut curator = 0;
                    let mut userpod = 0;
                    for (kind, _, _) in &pods {
                        match kind {
                            hkask_pods::PodKind::Curator => curator += 1,
                            hkask_pods::PodKind::UserPod => userpod += 1,
                        }
                    }
                    Some((curator, userpod))
                }
                Err(_) => None,
            }
        } else {
            None
        }
    }
    fn reg_domains(&self) -> Vec<(String, bool)> {
        let alerts = self.alert_count.load(std::sync::atomic::Ordering::Relaxed);
        vec![
            ("reg.tool".into(), alerts < 5),
            ("reg.inference".into(), alerts < 3),
            ("reg.mcp".into(), true),
            ("reg.mcp.media.face".into(), true),
            ("reg.storage".into(), true),
            ("reg.keystore".into(), true),
            ("reg.tui".into(), true),
        ]
    }
}

#[cfg(feature = "tui")]
impl crate::tui::SettingsBridge for TuiReplBridge {
    fn set_model(&self, name: &str) -> crate::tui::ModelSwitchResult {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let r = handlers::model::resolve_and_set_model(&mut state, &self.rt_handle, name);
        crate::tui::ModelSwitchResult {
            resolved_name: r.resolved_name,
            detail: r.detail,
        }
    }

    fn list_models(&self) -> anyhow::Result<Vec<crate::tui::TuiModelInfo>> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let ctx = hkask_services_inference::InferenceContext::from(state.service_context.as_ref());
        let models =
            self.rt_handle
                .block_on(hkask_services_inference::InferenceService::search_models(
                    &ctx, "",
                ))?;
        Ok(models
            .into_iter()
            .map(|m| crate::tui::TuiModelInfo {
                name: m.name,
                family: m.family,
                parameter_size: m.parameter_size,
                quantization_level: m.quantization_level,
                size_bytes: m.size_bytes,
            })
            .collect())
    }

    fn settings_display(&self) -> String {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        handlers::repl_settings::render_settings(&state)
    }

    fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        handlers::repl_settings::apply_setting(&mut state, key, value)
    }
}

#[cfg(feature = "tui")]
impl crate::tui::SessionBridge for TuiReplBridge {
    fn current_agent(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current_agent
            .clone()
    }

    fn list_agents_display(&self) -> String {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        handlers::agent::list_agents_display(&state)
    }

    fn history_display(&self) -> String {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        handlers::info::history_display(&state)
    }
}

#[cfg(feature = "tui")]
impl crate::tui::ToolInvokeBridge for TuiReplBridge {
    fn start_mcp_tool_invoke(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> crate::tui::McpInvokeRequestId {
        let state = self.state.clone();
        let rt = self.rt_handle.clone();
        let a2a = self.a2a_secret.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let request = crate::tui::McpInvokeRequestId::new();
        let server = server.to_string();
        let tool = tool.to_string();

        self.pending_invokes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(request, PendingTuiInvoke { receiver: rx });

        std::thread::spawn(move || {
            use hkask_capability::{
                DelegationAction, DelegationResource, DelegationToken, derive_signing_key,
            };

            // Extract everything we need under the lock, then release it.
            let (runtime, principal, agent) = {
                let s = state.lock().unwrap_or_else(|e| e.into_inner());
                (
                    s.service_context.infra().mcp.clone(),
                    s.host.resolve_user_webid(),
                    s.agent_webid,
                )
            };

            // Now the state lock is released. Build the token and invoke.
            let signing_key = derive_signing_key(a2a.as_bytes());
            let token = DelegationToken::new(
                DelegationResource::Tool,
                tool.clone(),
                DelegationAction::Execute,
                principal,
                agent,
                &signing_key,
            );
            let result = rt.block_on(async {
                use hkask_capability::ToolPort;
                runtime
                    .invoke(&server, &tool, args, &token)
                    .await
                    .map_err(|e| map_tool_port_error(e, &tool))
            });
            let _ = tx.send(result);
        });
        request
    }

    fn poll_mcp_tool_invoke(
        &self,
        request: crate::tui::McpInvokeRequestId,
    ) -> crate::tui::McpInvokeState {
        let mut pending = self
            .pending_invokes
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some(invoke) = pending.get(&request) else {
            return crate::tui::McpInvokeState::Idle;
        };
        match invoke.receiver.try_recv() {
            Ok(Ok(value)) => {
                pending.remove(&request);
                crate::tui::McpInvokeState::Done(value)
            }
            Ok(Err(err)) => {
                pending.remove(&request);
                crate::tui::McpInvokeState::Error(err)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => crate::tui::McpInvokeState::Invoking,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                pending.remove(&request);
                crate::tui::McpInvokeState::Idle
            }
        }
    }
}

#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::*;

    #[test]
    fn map_tool_port_error_not_found() {
        let err = hkask_capability::ToolPortError::NotFound(hkask_types::NotFound {
            entity_type: "tool".to_string(),
            id: "test_tool".to_string(),
        });
        let mapped = map_tool_port_error(err, "my_tool");
        match mapped {
            crate::tui::McpInvokeError::ToolNotFound(t) => assert_eq!(t, "my_tool"),
            other => panic!("expected ToolNotFound, got {:?}", other),
        }
    }

    #[test]
    fn map_tool_port_error_capability_denied() {
        let err = hkask_capability::ToolPortError::CapabilityDenied("denied".into());
        let mapped = map_tool_port_error(err, "tool_x");
        match mapped {
            crate::tui::McpInvokeError::Server(msg) => assert_eq!(msg, "denied"),
            other => panic!("expected Server, got {:?}", other),
        }
    }

    #[test]
    fn map_tool_port_error_energy_exceeded() {
        let err = hkask_capability::ToolPortError::EnergyBudgetExceeded("no gas".into());
        let mapped = map_tool_port_error(err, "tool_x");
        match mapped {
            crate::tui::McpInvokeError::Server(msg) => assert_eq!(msg, "no gas"),
            other => panic!("expected Server, got {:?}", other),
        }
    }

    #[test]
    fn map_tool_port_error_invocation_failed() {
        let err = hkask_capability::ToolPortError::InvocationFailed("crashed".into());
        let mapped = map_tool_port_error(err, "tool_x");
        match mapped {
            crate::tui::McpInvokeError::Server(msg) => assert_eq!(msg, "crashed"),
            other => panic!("expected Server, got {:?}", other),
        }
    }
}
