use crate::{AgentToolOutput, AnyAgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use collections::{BTreeMap, HashMap};
use context_server::{ContextServerId, client::NotificationSubscription};
use futures::FutureExt as _;
use gpui::{App, AppContext, AsyncApp, Context, Entity, EventEmitter, SharedString, Task};
use gpui_tokio::Tokio;
use language_model::{LanguageModelImage, LanguageModelImageExt, LanguageModelToolResultContent};
use project::context_server_store::{ContextServerStatus, ContextServerStore};
use std::sync::Arc;
use util::{ResultExt, markdown::MarkdownEscaped};

/// Maximum number of characters to show from a tool argument in the
/// collapsed tool-call header. Longer values are truncated with an ellipsis.
const MAX_INLINE_ARG_LEN: usize = 120;

/// Generates a tool ID for an MCP tool that can be used in settings.
///
/// The format is `mcp:<server_id>:<tool_name>` to avoid collisions with built-in tools.
pub fn mcp_tool_id(server_id: &str, tool_name: &str) -> String {
    format!("mcp:{}:{}", server_id, tool_name)
}

// ── zed-kask: governed-runtime tool source (single spawn authority, I1) ────

/// A kask MCP server tool descriptor, surfaced from the governed
/// `McpRuntime` into the agent's tool list.
#[derive(Clone)]
pub struct KaskToolDescriptor {
    pub server_id: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Source for kask built-in MCP server tools — wired in `main.rs` to the
/// governed `McpRuntime`. Kask servers are NOT registered with zed's
/// per-project `ContextServerStore` (the single-spawn-authority invariant,
/// 2026-08-29): their processes are owned exclusively by the runtime, their
/// env composed exclusively by `build_mcp_server_env`, and their tools
/// surface to the agent through this source. Dispatch through `invoke` is
/// metered by the runtime's governance — the agent-side
/// `record_mcp_tool_outcome` wrapper is intentionally absent from
/// `KaskServerTool` to avoid double-recording.
pub trait KaskToolSource: Send + Sync {
    /// The current tool surface: one descriptor per tool across all
    /// registered kask servers. Cache-backed and synchronous — the impl
    /// owns the async refresh.
    fn tools(&self) -> Vec<KaskToolDescriptor>;
    /// Dispatch a tool call to the governed runtime. `Ok(value)` is the
    /// parsed tool result; `Err(text)` is the operator-facing error text
    /// (kask errors carry the typed kind as a `[kind] message` prefix).
    fn invoke(
        &self,
        server_id: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>,
    >;
}

static KASK_TOOL_SOURCE: std::sync::Mutex<Option<Arc<dyn KaskToolSource>>> =
    std::sync::Mutex::new(None);

/// Warn-once latch for the unwired kask tool source. Resets when the source
/// is present, so the normal startup window (main.rs wires the source in its
/// deferred task) produces at most one line, and a source that later goes
/// away warns again. Without this, an unwired source is silent: kask tools
/// just don't surface, and the operator cannot distinguish "not configured"
/// from "configured but broken".
static KASK_TOOL_SOURCE_UNWIRED_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Record that the kask tool source was consulted and found unwired.
/// Returns whether this call is the first since the last wired observation
/// (the caller warns only then, to avoid a line per store event).
fn note_kask_tool_source_unwired() -> bool {
    !KASK_TOOL_SOURCE_UNWIRED_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
}

/// Record that the kask tool source is wired, re-arming the unwired warn.
fn note_kask_tool_source_wired() {
    KASK_TOOL_SOURCE_UNWIRED_WARNED.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Wire the governed `McpRuntime` as the agent's kask tool source. Called
/// once from `main.rs` after the runtime is created.
pub fn set_kask_tool_source(source: Arc<dyn KaskToolSource>) {
    *KASK_TOOL_SOURCE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(source);
}

/// The wired kask tool source, if any (absent in tests and lightweight
/// embedders — kask tools then simply do not surface).
pub fn kask_tool_source() -> Option<Arc<dyn KaskToolSource>> {
    KASK_TOOL_SOURCE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub struct ContextServerPrompt {
    pub server_id: ContextServerId,
    pub prompt: context_server::types::Prompt,
}

pub enum ContextServerRegistryEvent {
    ToolsChanged,
    PromptsChanged,
}

impl EventEmitter<ContextServerRegistryEvent> for ContextServerRegistry {}

pub struct ContextServerRegistry {
    server_store: Entity<ContextServerStore>,
    registered_servers: HashMap<ContextServerId, RegisteredContextServer>,
    _subscription: gpui::Subscription,
}

struct RegisteredContextServer {
    tools: BTreeMap<SharedString, Arc<dyn AnyAgentTool>>,
    prompts: BTreeMap<SharedString, ContextServerPrompt>,
    load_tools: Task<Result<()>>,
    load_prompts: Task<Result<()>>,
    _tools_updated_subscription: Option<NotificationSubscription>,
}

/// How often the registry re-reads the kask tool source. Kask servers
/// register with the governed `McpRuntime`, not the per-project
/// `ContextServerStore`, so their registration never fires a store event —
/// polling is the only way a registry created before the deferred MCP
/// launch (window restore) can learn about them.
const KASK_TOOL_SOURCE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

impl ContextServerRegistry {
    pub fn new(server_store: Entity<ContextServerStore>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            server_store: server_store.clone(),
            registered_servers: HashMap::default(),
            _subscription: cx.subscribe(&server_store, Self::handle_context_server_store_event),
        };
        for server in server_store.read(cx).running_servers() {
            this.reload_tools_for_server(server.id(), cx);
            this.reload_prompts_for_server(server.id(), cx);
        }
        // zed-kask: kask tools surface from the governed runtime, not the
        // per-project store (single spawn authority, I1).
        this.reload_kask_tools(cx);
        // zed-kask: the startup race. This registry is created during window
        // restore, before the deferred MCP launch registers kask tools — and
        // because kask servers are not in the ContextServerStore, no store
        // event ever announces the late registration. Without this poll the
        // merge above is the last one for the whole app session and every
        // agent-panel conversation runs without kask tools (observed live
        // 2026-08-30: a fresh session had zero kask tools until a store
        // event was fired by hand). GPUI-native timer, not tokio — tokio
        // timers panic on the foreground thread (see the .rules GPUI traps).
        // The loop exits when this registry is dropped.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(KASK_TOOL_SOURCE_POLL_INTERVAL)
                    .await;
                let Ok(()) = this.update(cx, |this, cx| this.reload_kask_tools(cx)) else {
                    break;
                };
            }
        })
        .detach();
        this
    }

    /// Merge the kask tool surface (from the process-global
    /// [`KaskToolSource`], wired to the governed `McpRuntime` in `main.rs`)
    /// into the registry under each tool's server id. Cache-backed and
    /// synchronous, so it is cheap enough to re-run on every store event and
    /// on every poll tick. The poll is what catches kask servers that
    /// registered after this registry was created — store events cannot
    /// (kask servers are not in the ContextServerStore), so the
    /// store-event path only re-inserts entries that a colliding raw
    /// settings id removed.
    fn reload_kask_tools(&mut self, cx: &mut Context<Self>) {
        let Some(source) = kask_tool_source() else {
            if note_kask_tool_source_unwired() {
                log::warn!(
                    "kask tool source is unwired — kask MCP server tools are not surfacing in the agent tool list. main.rs wires set_kask_tool_source in its deferred startup task: a single line during the first seconds after launch is startup ordering; a persistent absence means the wiring is broken"
                );
            }
            return;
        };
        note_kask_tool_source_wired();
        let mut by_server: HashMap<String, BTreeMap<SharedString, Arc<dyn AnyAgentTool>>> =
            HashMap::default();
        for descriptor in source.tools() {
            let server_id = descriptor.server_id.clone();
            let name: SharedString = descriptor.name.clone().into();
            let tool: Arc<dyn AnyAgentTool> = Arc::new(KaskServerTool {
                source: source.clone(),
                descriptor,
            });
            by_server.entry(server_id).or_default().insert(name, tool);
        }
        // Re-merge every entry so the wrapped descriptors stay fresh across
        // kask server restarts (the source's cache is rebuilt on reconnect),
        // but emit `ToolsChanged` only when the visible surface changed: the
        // poll re-runs this every tick and store events re-run it on every
        // server status change, and notifying subscribers for a no-op would
        // churn every thread's tool list. A kask entry removed by a store
        // event for a colliding raw settings id counts as changed here and
        // is re-inserted on the next pass.
        let mut surface_changed = false;
        for (server_id, tools) in by_server {
            let server_id = ContextServerId(std::sync::Arc::from(server_id.as_str()));
            let tool_names_unchanged = self
                .registered_servers
                .get(&server_id)
                .is_some_and(|registered| registered.tools.keys().eq(tools.keys()));
            if !tool_names_unchanged {
                surface_changed = true;
            }
            self.registered_servers.insert(
                server_id,
                RegisteredContextServer {
                    tools,
                    prompts: BTreeMap::default(),
                    load_tools: Task::ready(Ok(())),
                    load_prompts: Task::ready(Ok(())),
                    _tools_updated_subscription: None,
                },
            );
        }
        if surface_changed {
            cx.emit(ContextServerRegistryEvent::ToolsChanged);
        }
    }

    pub fn tools_for_server(
        &self,
        server_id: &ContextServerId,
    ) -> impl Iterator<Item = &Arc<dyn AnyAgentTool>> {
        self.registered_servers
            .get(server_id)
            .map(|server| server.tools.values())
            .into_iter()
            .flatten()
    }

    pub fn servers(
        &self,
    ) -> impl Iterator<
        Item = (
            &ContextServerId,
            &BTreeMap<SharedString, Arc<dyn AnyAgentTool>>,
        ),
    > {
        self.registered_servers
            .iter()
            .map(|(id, server)| (id, &server.tools))
    }

    pub fn prompts(&self) -> impl Iterator<Item = &ContextServerPrompt> {
        self.registered_servers
            .values()
            .flat_map(|server| server.prompts.values())
    }

    pub fn find_prompt(
        &self,
        server_id: Option<&ContextServerId>,
        name: &str,
    ) -> Option<&ContextServerPrompt> {
        if let Some(server_id) = server_id {
            self.registered_servers
                .get(server_id)
                .and_then(|server| server.prompts.get(name))
        } else {
            self.registered_servers
                .values()
                .find_map(|server| server.prompts.get(name))
        }
    }

    pub fn server_store(&self) -> &Entity<ContextServerStore> {
        &self.server_store
    }

    fn get_or_register_server(
        &mut self,
        server_id: &ContextServerId,
        cx: &mut Context<Self>,
    ) -> &mut RegisteredContextServer {
        self.registered_servers
            .entry(server_id.clone())
            .or_insert_with(|| Self::init_registered_server(server_id, &self.server_store, cx))
    }

    fn init_registered_server(
        server_id: &ContextServerId,
        server_store: &Entity<ContextServerStore>,
        cx: &mut Context<Self>,
    ) -> RegisteredContextServer {
        let tools_updated_subscription = server_store
            .read(cx)
            .get_running_server(server_id)
            .and_then(|server| {
                let client = server.client()?;

                if !client.capable(context_server::protocol::ServerCapability::Tools) {
                    return None;
                }

                let server_id = server.id();
                let this = cx.entity().downgrade();

                Some(client.on_notification(
                    "notifications/tools/list_changed",
                    Box::new(move |_params, cx: AsyncApp| {
                        let server_id = server_id.clone();
                        let this = this.clone();
                        cx.spawn(async move |cx| {
                            this.update(cx, |this, cx| {
                                log::info!(
                                    "Received tools/list_changed notification for server {}",
                                    server_id
                                );
                                this.reload_tools_for_server(server_id, cx);
                            })
                        })
                        .detach();
                    }),
                ))
            });

        RegisteredContextServer {
            tools: BTreeMap::default(),
            prompts: BTreeMap::default(),
            load_tools: Task::ready(Ok(())),
            load_prompts: Task::ready(Ok(())),
            _tools_updated_subscription: tools_updated_subscription,
        }
    }

    fn reload_tools_for_server(&mut self, server_id: ContextServerId, cx: &mut Context<Self>) {
        let Some(server) = self.server_store.read(cx).get_running_server(&server_id) else {
            return;
        };
        let Some(client) = server.client() else {
            return;
        };

        if !client.capable(context_server::protocol::ServerCapability::Tools) {
            return;
        }

        let registered_server = self.get_or_register_server(&server_id, cx);
        registered_server.load_tools = cx.spawn(async move |this, cx| {
            let response = client
                .request::<context_server::types::requests::ListTools>(())
                .await;

            this.update(cx, |this, cx| {
                let Some(registered_server) = this.registered_servers.get_mut(&server_id) else {
                    return;
                };

                registered_server.tools.clear();
                if let Some(response) = response.log_err() {
                    for tool in response.tools {
                        let tool = Arc::new(ContextServerTool::new(
                            this.server_store.clone(),
                            server.id(),
                            tool,
                        ));
                        registered_server.tools.insert(tool.name(), tool);
                    }
                    cx.emit(ContextServerRegistryEvent::ToolsChanged);
                    cx.notify();
                }
            })
        });
    }

    fn reload_prompts_for_server(&mut self, server_id: ContextServerId, cx: &mut Context<Self>) {
        let Some(server) = self.server_store.read(cx).get_running_server(&server_id) else {
            return;
        };
        let Some(client) = server.client() else {
            return;
        };
        if !client.capable(context_server::protocol::ServerCapability::Prompts) {
            return;
        }

        let registered_server = self.get_or_register_server(&server_id, cx);

        registered_server.load_prompts = cx.spawn(async move |this, cx| {
            let response = client
                .request::<context_server::types::requests::PromptsList>(())
                .await;

            this.update(cx, |this, cx| {
                let Some(registered_server) = this.registered_servers.get_mut(&server_id) else {
                    return;
                };

                registered_server.prompts.clear();
                if let Some(response) = response.log_err() {
                    for prompt in response.prompts {
                        let name: SharedString = prompt.name.clone().into();
                        registered_server.prompts.insert(
                            name,
                            ContextServerPrompt {
                                server_id: server_id.clone(),
                                prompt,
                            },
                        );
                    }
                    cx.emit(ContextServerRegistryEvent::PromptsChanged);
                    cx.notify();
                }
            })
        });
    }

    fn handle_context_server_store_event(
        &mut self,
        _: Entity<ContextServerStore>,
        event: &project::context_server_store::ServerStatusChangedEvent,
        cx: &mut Context<Self>,
    ) {
        let project::context_server_store::ServerStatusChangedEvent { server_id, status } = event;

        match status {
            ContextServerStatus::Starting | ContextServerStatus::Authenticating => {}
            ContextServerStatus::Running => {
                self.reload_tools_for_server(server_id.clone(), cx);
                self.reload_prompts_for_server(server_id.clone(), cx);
            }
            ContextServerStatus::Stopped
            | ContextServerStatus::Error(_)
            | ContextServerStatus::AuthRequired
            | ContextServerStatus::ClientSecretRequired { .. } => {
                if let Some(registered_server) = self.registered_servers.remove(server_id) {
                    if !registered_server.tools.is_empty() {
                        cx.emit(ContextServerRegistryEvent::ToolsChanged);
                    }
                    if !registered_server.prompts.is_empty() {
                        cx.emit(ContextServerRegistryEvent::PromptsChanged);
                    }
                }
                cx.notify();
            }
        };
        // zed-kask: opportunistic kask-tool refresh. The governed runtime
        // registers kask tools asynchronously (deferred launch, restarts,
        // reconnects) with no store event to observe — re-reading the cache
        // here catches late registrations, and re-inserts kask entries if a
        // store event for a colliding raw settings id wrongly removed them.
        self.reload_kask_tools(cx);
    }
}

struct ContextServerTool {
    store: Entity<ContextServerStore>,
    server_id: ContextServerId,
    tool: context_server::types::Tool,
}

/// A kask built-in MCP server tool, dispatched through the governed
/// `McpRuntime` via the process-global [`KaskToolSource`] — not through the
/// per-project `ContextServerStore`. Same `AnyAgentTool` surface as
/// `ContextServerTool` so the agent's tool list assembly is unchanged; the
/// self-healing loop is absent because the runtime owns reconnection
/// (keeper reaping, on-demand reconnect, health supervisor, circuit
/// breaker), and outcome recording is absent because the runtime's
/// governance meters every `invoke`.
struct KaskServerTool {
    source: Arc<dyn KaskToolSource>,
    descriptor: KaskToolDescriptor,
}

impl AnyAgentTool for KaskServerTool {
    fn name(&self) -> SharedString {
        self.descriptor.name.clone().into()
    }

    fn description(&self) -> SharedString {
        self.descriptor.description.clone().into()
    }

    fn kind(&self) -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(&self, input: serde_json::Value, _cx: &mut App) -> SharedString {
        format_mcp_initial_title(&self.descriptor.name, &input).into()
    }

    fn input_schema(
        &self,
        format: language_model::LanguageModelToolSchemaFormat,
    ) -> Result<serde_json::Value> {
        let mut schema = self.descriptor.input_schema.clone();
        language_model::tool_schema::adapt_schema_to_format(&mut schema, format)?;
        Ok(match schema {
            serde_json::Value::Null => {
                serde_json::json!({ "type": "object", "properties": [] })
            }
            serde_json::Value::Object(map) if map.is_empty() => {
                serde_json::json!({ "type": "object", "properties": [] })
            }
            _ => schema,
        })
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<serde_json::Value>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<AgentToolOutput, AgentToolOutput>> {
        let tool_id = mcp_tool_id(&self.descriptor.server_id, &self.descriptor.name);
        let display_name = self.descriptor.name.clone();
        let initial_title = self.initial_title(serde_json::Value::Null, cx);
        let authorize =
            event_stream.authorize_third_party_tool(initial_title, tool_id, display_name, cx);
        let server_id = self.descriptor.server_id.clone();
        let tool_name = self.descriptor.name.clone();
        let source = self.source.clone();
        // The dispatch future MUST run on the Tokio runtime, not on the GPUI
        // foreground executor. `source.invoke` → `McpRuntime::invoke` →
        // `call_tool_inner` → `try_reconnect` → `start_server_with_env` spawns a
        // child process via `TokioChildProcess`, which requires a Tokio reactor
        // context. The GPUI foreground executor (`cx.spawn`) has no reactor, so
        // awaiting `source.invoke` directly here panics with "there is no
        // reactor running" on a transport-loss-triggered reconnect — the
        // `.rules` "background_spawn of tokio-dependent futures" / "tokio
        // primitives panic inside cx.spawn" trap. The swarm-panel
        // `PanelToolInvoker` (main.rs) fixes the same trap the same way. The
        // `JoinHandle` is a oneshot-backed future, so awaiting it on the
        // foreground executor is sound (it does not register a timer with the
        // Tokio reactor, unlike `tokio::time::Sleep`).
        let tokio_handle = Tokio::handle(cx);
        cx.spawn(async move |_| {
            let input = input
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            authorize
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let result = tokio_handle
                .spawn(source.invoke(&server_id, &tool_name, input))
                .await
                .map_err(|join_error| {
                    anyhow::anyhow!(format!("kask tool dispatch task failed: {join_error}"))
                })?;
            match result {
                Ok(value) => {
                    let text = match &value {
                        serde_json::Value::String(string) => string.clone(),
                        value => value.to_string(),
                    };
                    // Structural display hints (T-V2) — same as the store
                    // path: fenced media blocks render deterministically.
                    let mut tool_call_content = Vec::new();
                    for hint in hkask_types::tool_response::display_hints_from_output_text(&text) {
                        tool_call_content.push(acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(hint)),
                        )));
                    }
                    if !tool_call_content.is_empty() {
                        event_stream.update_fields(
                            acp::ToolCallUpdateFields::new().content(tool_call_content),
                        );
                    }
                    Ok(AgentToolOutput {
                        raw_output: value,
                        llm_output: vec![LanguageModelToolResultContent::Text(text.into())],
                    })
                }
                Err(error_text) => Err(AgentToolOutput {
                    raw_output: serde_json::Value::String(error_text.clone()),
                    llm_output: vec![LanguageModelToolResultContent::Text(error_text.into())],
                }),
            }
        })
    }

    fn replay(
        &self,
        _input: serde_json::Value,
        _output: serde_json::Value,
        _event_stream: ToolCallEventStream,
        _cx: &mut App,
    ) -> Result<()> {
        Ok(())
    }
}

/// Map a completed `ContextServerTool` run to the regulation outcome tuple
/// `(success, error_kind)`. The kind is read structurally from the error
/// output's `raw_output` (the server's `structured_content`, set by the
/// `is_error` branch in `run_inner`); when absent (non-kask servers, whose
/// errors are plain text) the full text is the classification hint.
fn mcp_run_outcome(result: &Result<AgentToolOutput, AgentToolOutput>) -> (bool, Option<String>) {
    match result {
        Ok(_) => (true, None),
        Err(output) => {
            let kind = hkask_types::tool_response::parse_tool_error_value(&output.raw_output)
                .and_then(|envelope| envelope.kind)
                .map(|kind| kind.to_string());
            (false, Some(kind.unwrap_or_else(|| mcp_error_text(output))))
        }
    }
}

/// zed-kask: D-seam — whether a failed context-server tool call is a
/// request timeout (server alive but slow) rather than a transport death
/// (connection reset, process death). Timeouts must NOT enter the
/// restart-and-retry loop: restarting a live-but-slow server wastes 30s
/// and does not fix the slowness. The timeout message is produced by
/// `context_server::client` (`"Context server request timeout"`); it is
/// matched on the string to avoid adding a public error variant to the
/// `context_server` crate.
fn is_context_server_timeout(error_text: &str) -> bool {
    error_text.contains("Context server request timeout")
}

/// Extract the error text from a failed MCP tool run. The error message
/// lives in the LLM-facing text parts of the output; an output with no text
/// at all reports "unknown error" rather than an empty classification.
fn mcp_error_text(output: &AgentToolOutput) -> String {
    let text = output
        .llm_output
        .iter()
        .filter_map(|part| match part {
            LanguageModelToolResultContent::Text(text) => Some(text.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    if text.trim().is_empty() {
        "unknown error".to_string()
    } else {
        text
    }
}

impl ContextServerTool {
    fn new(
        store: Entity<ContextServerStore>,
        server_id: ContextServerId,
        tool: context_server::types::Tool,
    ) -> Self {
        Self {
            store,
            server_id,
            tool,
        }
    }
}

impl AnyAgentTool for ContextServerTool {
    fn name(&self) -> SharedString {
        self.tool.name.clone().into()
    }

    fn description(&self) -> SharedString {
        self.tool.description.clone().unwrap_or_default().into()
    }

    fn kind(&self) -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(&self, input: serde_json::Value, _cx: &mut App) -> SharedString {
        format_mcp_initial_title(&self.tool.name, &input).into()
    }

    fn input_schema(
        &self,
        format: language_model::LanguageModelToolSchemaFormat,
    ) -> Result<serde_json::Value> {
        let mut schema = self.tool.input_schema.clone();
        language_model::tool_schema::adapt_schema_to_format(&mut schema, format)?;
        Ok(match schema {
            serde_json::Value::Null => {
                serde_json::json!({ "type": "object", "properties": [] })
            }
            serde_json::Value::Object(map) if map.is_empty() => {
                serde_json::json!({ "type": "object", "properties": [] })
            }
            _ => schema,
        })
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<serde_json::Value>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<AgentToolOutput, AgentToolOutput>> {
        // zed-kask: D-seam — T-V1 regulation wiring. Agent-path MCP tool
        // calls (zed's context-server client) were invisible to the
        // regulation system: the McpRuntime dispatch path (skills/panel/IPC)
        // records outcomes via `with_governance`, but this path had no
        // `record_outcome` call, so the ToolReliabilitySensor and the
        // curator never saw agent-initiated MCP failures. The wrapper
        // records every outcome — including the early not-running error
        // path inside `run_inner` — through the process-global hook wired
        // in `main.rs`.
        let server_name = self.server_id.0.clone();
        let tool_name = self.tool.name.clone();
        let inner = Self::run_inner(self, input, event_stream, cx);
        cx.spawn(async move |_| {
            let result = inner.await;
            let (success, error_kind) = mcp_run_outcome(&result);
            crate::record_mcp_tool_outcome(
                &server_name,
                &tool_name,
                success,
                error_kind.as_deref(),
            );
            result
        })
    }

    fn replay(
        &self,
        _input: serde_json::Value,
        _output: serde_json::Value,
        _event_stream: ToolCallEventStream,
        _cx: &mut App,
    ) -> Result<()> {
        Ok(())
    }
}

impl ContextServerTool {
    /// Execute the MCP tool call — the original `run` body, extracted so the
    /// `run` wrapper can record the outcome for regulation (T-V1).
    fn run_inner(
        self: Arc<Self>,
        input: ToolInput<serde_json::Value>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<AgentToolOutput, AgentToolOutput>> {
        // Check the server is running before spawning — gives an early error
        // rather than entering the self-healing loop for a server that was
        // never started. The closure re-fetches by id after auth.
        if self
            .store
            .read(cx)
            .get_running_server(&self.server_id)
            .is_none()
        {
            return Task::ready(Err(anyhow::anyhow!("Context server not found").into()));
        }
        let tool_name = self.tool.name.clone();
        let tool_id = mcp_tool_id(&self.server_id.0, &self.tool.name);
        let display_name = self.tool.name.clone();
        let initial_title = self.initial_title(serde_json::Value::Null, cx);
        let authorize =
            event_stream.authorize_third_party_tool(initial_title, tool_id, display_name, cx);
        // Capture the store and server_id for the self-healing path below —
        // the closure needs to re-fetch the server after a transport death.
        let store = self.store.clone();
        let server_id = self.server_id.clone();

        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            authorize
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            // zed-kask: D-seam — on-demand self-healing. When the server's
            // client is None (transport died, server moved to Stopped by
            // watch_transport_shutdown), trigger maintain_servers to restart
            // it before failing. This mirrors McpRuntime::call_tool_inner's
            // try_reconnect pattern.
            let server = cx.update(|cx| store.read(cx).get_running_server(&server_id));
            let server = match server {
                Some(s) => s,
                None => {
                    // Server is not running — trigger a restart via
                    // available_context_servers_changed, then wait for it.
                    log::warn!(
                        "Context server '{}' not running — triggering restart",
                        server_id.0
                    );
                    cx.update(|cx| {
                        store.update(cx, |store, cx| {
                            store.trigger_server_maintenance(cx);
                        });
                    });
                    // Wait for the server to come back (up to 30s)
                    let mut elapsed = 0u64;
                    let server = loop {
                        if elapsed >= 30_000 {
                            return Err(anyhow::anyhow!(
                                "Context server '{}' failed to restart within 30s",
                                server_id.0
                            ).into());
                        }
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(500))
                            .await;
                        elapsed += 500;
                        if let Some(s) = cx.update(|cx| store.read(cx).get_running_server(&server_id)) {
                            break s;
                        }
                    };
                    server
                }
            };

            let Some(protocol) = server.client() else {
                return Err(anyhow::anyhow!("Context server not initialized").into());
            };

            let arguments = if let serde_json::Value::Object(map) = input {
                Some(map.into_iter().collect())
            } else {
                None
            };

            log::trace!(
                "Running tool: {} with arguments: {:?}",
                tool_name,
                arguments
            );

            let request = protocol.request::<context_server::types::requests::CallTool>(
                context_server::types::CallToolParams {
                    name: tool_name.clone(),
                    arguments: arguments.clone(),
                    meta: None,
                },
            );

            let response = futures::select! {
                response = request.fuse() => match response {
                    Ok(r) => r,
                    Err(e) => {
                        // zed-kask: D-seam — distinguish timeout from transport death.
                        // The original code retried on *any* error, including timeouts.
                        // But a timeout means the server is alive but slow (or its upstream
                        // is slow) — restarting it wastes 30s and doesn't fix the slowness.
                        // Only retry on actual transport errors (connection reset, process
                        // death), where a restart can actually help.
                        let is_timeout = is_context_server_timeout(&e.to_string());
                        if is_timeout {
                            log::warn!(
                                "Context server '{}' tool '{}' timed out — not retrying (server is alive but slow)",
                                server_id.0, tool_name
                            );
                            return Err(e.into());
                        }
                        // Transport error — server may have died mid-call. Trigger a
                        // restart and retry once, mirroring McpRuntime::call_tool_inner.
                        log::warn!(
                            "Context server '{}' tool '{}' failed: {} — attempting restart and retry",
                            server_id.0, tool_name, e
                        );
                        cx.update(|cx| {
                            store.update(cx, |store, cx| {
                                store.trigger_server_maintenance(cx);
                            });
                        });
                        // Wait for restart (up to 30s)
                        let mut elapsed = 0u64;
                        let retried_server = loop {
                            if elapsed >= 30_000 {
                                return Err(anyhow::anyhow!(
                                    "Context server '{}' failed to restart after transport error: {}",
                                    server_id.0, e
                                ).into());
                            }
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(500))
                                .await;
                            elapsed += 500;
                            if let Some(s) = cx.update(|cx| store.read(cx).get_running_server(&server_id)) {
                                break s;
                            }
                        };
                        let Some(retried_protocol) = retried_server.client() else {
                            return Err(anyhow::anyhow!(
                                "Context server '{}' restarted but client not available",
                                server_id.0
                            ).into());
                        };
                        let retry_request = retried_protocol.request::<context_server::types::requests::CallTool>(
                            context_server::types::CallToolParams {
                                name: tool_name,
                                arguments,
                                meta: None,
                            },
                        );
                        futures::select! {
                            response = retry_request.fuse() => response?,
                            _ = event_stream.cancelled_by_user().fuse() => {
                                return Err(anyhow::anyhow!("MCP tool cancelled by user").into());
                            }
                        }
                    }
                },
                _ = event_stream.cancelled_by_user().fuse() => {
                    return Err(anyhow::anyhow!("MCP tool cancelled by user").into());
                }
            };

            if response.is_error == Some(true) {
                let error_message: String =
                    response.content.iter().filter_map(|c| c.text()).collect();
                // zed-kask: D-seam — carry the typed error kind structurally.
                // kask servers set `is_error` natively (rmcp's Result handling
                // + `McpToolError: IntoCallToolResult`) with the typed kind in
                // `structured_content`. Putting that value in the error
                // output's `raw_output` lets downstream consumers (regulation
                // outcome recording, retry tracker) classify from the typed
                // field instead of parsing error text.
                return Err(AgentToolOutput {
                    raw_output: response.structured_content.clone().unwrap_or_default(),
                    llm_output: vec![LanguageModelToolResultContent::Text(
                        error_message.into(),
                    )],
                });
            }

            let mut llm_output = Vec::new();
            let mut tool_call_content = Vec::new();
            let mut concatenated_text = String::new();
            for content in response.content {
                match content {
                    context_server::types::ToolResponseContent::Text { text } => {
                        concatenated_text.push_str(&text);
                        tool_call_content.push(acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(text.clone())),
                        )));
                        llm_output.push(LanguageModelToolResultContent::Text(text.into()));
                    }
                    context_server::types::ToolResponseContent::Image { data, mime_type } => {
                        tool_call_content.push(acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Image(acp::ImageContent::new(
                                data.clone(),
                                mime_type.clone(),
                            )),
                        )));
                        let language_model_image = cx
                            .background_spawn({
                                let mime_type = mime_type.clone();
                                async move {
                                    LanguageModelImage::from_base64_image(&data, &mime_type)
                                }
                            })
                            .await;
                        match language_model_image {
                            Ok(Some(image)) => {
                                llm_output.push(LanguageModelToolResultContent::Image(image));
                            }
                            Ok(None) => {
                                log::warn!(
                                    "Skipping MCP tool response image with MIME type `{}` because it cannot be converted for language model input",
                                    mime_type
                                );
                            }
                            Err(error) => {
                                log::warn!(
                                    "Failed to convert MCP tool response image with MIME type `{}` for language model input: {:#}",
                                    mime_type,
                                    error
                                );
                            }
                        }
                    }
                    context_server::types::ToolResponseContent::Audio { .. } => {
                        log::warn!("Ignoring audio content from tool response");
                    }
                    context_server::types::ToolResponseContent::Resource { .. } => {
                        log::warn!("Ignoring resource content from tool response");
                    }
                    context_server::types::ToolResponseContent::ResourceLink { .. } => {
                        log::warn!("Ignoring resource link content from tool response");
                    }
                }
            }
            // zed-kask: D-seam — structural display_hint rendering (T-V2).
            // Media tool results carry `display_hint` / `display_hints`
            // (fenced ```media blocks) as JSON fields inside the content
            // envelope; previously they rendered only if the model
            // voluntarily copied the fenced block into its reply — the
            // entire media pipeline depended on model cooperation. Pushing
            // the fenced blocks as additional text content makes the D18
            // media_block_renderer render them deterministically in the
            // tool card, in every conversation surface.
            for hint in
                hkask_types::tool_response::display_hints_from_output_text(&concatenated_text)
            {
                tool_call_content.push(acp::ToolCallContent::Content(acp::Content::new(
                    acp::ContentBlock::Text(acp::TextContent::new(hint)),
                )));
            }
            if !tool_call_content.is_empty() {
                event_stream
                    .update_fields(acp::ToolCallUpdateFields::new().content(tool_call_content));
            }
            let raw_output = serde_json::Value::String(concatenated_text);
            Ok(AgentToolOutput {
                raw_output,
                llm_output,
            })
        })
    }
}

/// Builds the header label shown for an MCP tool call. When the input is an
/// object with a single string-valued field, the value is inlined next to the
/// tool name so the primary argument (e.g. a URL, path, or query) is visible
/// without expanding the input block — matching the UX of built-in tools like
/// `Fetch`. All other shapes fall back to the tool name alone.
fn format_mcp_initial_title(tool_name: &str, input: &serde_json::Value) -> String {
    if let Some(value) = single_string_arg(input) {
        let preview = truncate_chars(value, MAX_INLINE_ARG_LEN);
        format!("Run MCP tool `{}` {}", tool_name, MarkdownEscaped(&preview))
    } else {
        format!("Run MCP tool `{}`", tool_name)
    }
}

fn single_string_arg(input: &serde_json::Value) -> Option<&str> {
    let obj = input.as_object()?;
    if obj.len() != 1 {
        return None;
    }
    obj.values().next()?.as_str()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

pub fn get_prompt(
    server_store: &Entity<ContextServerStore>,
    server_id: &ContextServerId,
    prompt_name: &str,
    arguments: HashMap<String, String>,
    cx: &mut AsyncApp,
) -> Task<Result<context_server::types::PromptsGetResponse>> {
    let server = cx.update(|cx| server_store.read(cx).get_running_server(server_id));
    let Some(server) = server else {
        return Task::ready(Err(anyhow::anyhow!("Context server not found")));
    };

    let Some(protocol) = server.client() else {
        return Task::ready(Err(anyhow::anyhow!("Context server not initialized")));
    };

    let prompt_name = prompt_name.to_string();

    cx.background_spawn(async move {
        let response = protocol
            .request::<context_server::types::requests::PromptsGet>(
                context_server::types::PromptsGetParams {
                    name: prompt_name,
                    arguments: (!arguments.is_empty()).then(|| arguments),
                    meta: None,
                },
            )
            .await?;

        Ok(response)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_id_format() {
        assert_eq!(
            mcp_tool_id("filesystem", "read_file"),
            "mcp:filesystem:read_file"
        );
        assert_eq!(
            mcp_tool_id("github", "create_issue"),
            "mcp:github:create_issue"
        );
        assert_eq!(
            mcp_tool_id("my-custom-server", "do_something"),
            "mcp:my-custom-server:do_something"
        );
        // Underscores in names
        assert_eq!(mcp_tool_id("my_server", "my_tool"), "mcp:my_server:my_tool");
    }

    // Note: Tests for MCP tool ID collision with built-in tools and permission
    // decisions are in crates/agent/src/tool_permissions.rs to avoid duplication.

    #[test]
    fn test_format_mcp_initial_title_inlines_single_string_arg() {
        let input = serde_json::json!({ "url": "https://example.com/page" });
        assert_eq!(
            format_mcp_initial_title("open_url_in_browser", &input),
            "Run MCP tool `open_url_in_browser` https://example.com/page"
        );
    }

    #[test]
    fn test_format_mcp_initial_title_no_args() {
        let input = serde_json::json!({});
        assert_eq!(
            format_mcp_initial_title("cleanup", &input),
            "Run MCP tool `cleanup`"
        );
    }

    #[test]
    fn test_format_mcp_initial_title_null_input() {
        assert_eq!(
            format_mcp_initial_title("cleanup", &serde_json::Value::Null),
            "Run MCP tool `cleanup`"
        );
    }

    #[test]
    fn test_format_mcp_initial_title_multiple_fields_falls_back() {
        let input = serde_json::json!({ "x": "a", "y": "b" });
        assert_eq!(
            format_mcp_initial_title("do_thing", &input),
            "Run MCP tool `do_thing`"
        );
    }

    #[test]
    fn test_format_mcp_initial_title_non_string_field_falls_back() {
        let input = serde_json::json!({ "count": 42 });
        assert_eq!(
            format_mcp_initial_title("tick", &input),
            "Run MCP tool `tick`"
        );
    }

    #[test]
    fn test_format_mcp_initial_title_truncates_long_values() {
        let long = "x".repeat(MAX_INLINE_ARG_LEN + 50);
        let input = serde_json::json!({ "q": long });
        let title = format_mcp_initial_title("search", &input);
        assert!(
            title.ends_with('…'),
            "expected truncation ellipsis, got: {title}"
        );
        // Prefix + backticked name + space + MAX chars + ellipsis — no full 170-char value.
        assert!(title.chars().count() < MAX_INLINE_ARG_LEN + 50);
    }

    #[test]
    fn test_format_mcp_initial_title_escapes_markdown_in_value() {
        let input = serde_json::json!({ "q": "**bold** _italic_" });
        let title = format_mcp_initial_title("search", &input);
        // Asterisks and underscores must be escaped so the header renders literally.
        assert!(title.contains("\\*"), "expected \\*, got: {title}");
        assert!(title.contains("\\_"), "expected \\_, got: {title}");
    }

    #[test]
    fn test_truncate_chars_boundary() {
        assert_eq!(truncate_chars("abc", 3), "abc");
        assert_eq!(truncate_chars("abcd", 3), "abc…");
    }

    #[test]
    fn test_truncate_chars_handles_multibyte() {
        // "café" is 4 chars but 5 bytes — byte-based truncation would panic.
        assert_eq!(truncate_chars("café", 4), "café");
        assert_eq!(truncate_chars("café", 3), "caf…");
    }

    #[test]
    fn test_single_string_arg_ignores_empty_string() {
        // An empty string is still a string — we inline it rather than fall back,
        // which lets callers tell "the server sent an empty arg" apart from
        // "no args at all".
        let input = serde_json::json!({ "q": "" });
        assert_eq!(single_string_arg(&input), Some(""));
    }

    #[test]
    fn test_mcp_run_outcome_maps_success() {
        let output = AgentToolOutput {
            raw_output: serde_json::Value::String("ok".into()),
            llm_output: vec![],
        };
        let (success, error_kind) = mcp_run_outcome(&Ok(output));
        assert!(success);
        assert_eq!(error_kind, None);
    }

    #[test]
    fn test_mcp_run_outcome_maps_error_text() {
        let output = AgentToolOutput {
            raw_output: serde_json::Value::String(String::new()),
            llm_output: vec![LanguageModelToolResultContent::Text(
                "Context server not found".into(),
            )],
        };
        let (success, error_kind) = mcp_run_outcome(&Err(output));
        assert!(!success);
        assert_eq!(error_kind.as_deref(), Some("Context server not found"));
    }

    #[test]
    fn test_mcp_run_outcome_empty_error_text_falls_back() {
        // An error output with no text parts must still carry a non-empty
        // classification — an empty error_kind would be indistinguishable
        // from "no error" on the wire.
        let output = AgentToolOutput {
            raw_output: serde_json::Value::Null,
            llm_output: vec![],
        };
        let (success, error_kind) = mcp_run_outcome(&Err(output));
        assert!(!success);
        assert_eq!(error_kind.as_deref(), Some("unknown error"));
    }

    #[test]
    fn test_mcp_run_outcome_extracts_typed_kind() {
        // The typed kind rides in the error output's `raw_output` (the
        // server's `structured_content`, set by the `is_error` branch in
        // `run_inner`) — the regulation ledger classifies structurally,
        // never by parsing error text.
        let output = AgentToolOutput {
            raw_output: serde_json::json!({
                "error": "yt-dlp not found on system PATH",
                "kind": "unavailable"
            }),
            llm_output: vec![LanguageModelToolResultContent::Text(
                "yt-dlp not found on system PATH".into(),
            )],
        };
        let (success, error_kind) = mcp_run_outcome(&Err(output));
        assert!(!success);
        assert_eq!(error_kind.as_deref(), Some("unavailable"));
    }

    // ── zed-kask pinning tests (KaskToolSource D-seam, I1) ──────────────

    /// A controllable `KaskToolSource` for pinning the hook surface and
    /// the `KaskServerTool` dispatch. `invocations` records every dispatch
    /// so tests can assert the agent's tool call reached the source with
    /// the right server id, tool name, and arguments.
    struct FakeKaskToolSource {
        descriptors: Vec<KaskToolDescriptor>,
        invocations: std::sync::Arc<std::sync::Mutex<Vec<(String, String, serde_json::Value)>>>,
    }

    impl FakeKaskToolSource {
        /// A source exposing zero tools — wiring it is observationally inert
        /// for any registry constructed in parallel tests.
        fn empty() -> std::sync::Arc<dyn KaskToolSource> {
            std::sync::Arc::new(Self {
                descriptors: Vec::new(),
                invocations: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            })
        }
    }

    impl KaskToolSource for FakeKaskToolSource {
        fn tools(&self) -> Vec<KaskToolDescriptor> {
            self.descriptors.clone()
        }

        fn invoke(
            &self,
            server_id: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>,
        > {
            let record = (server_id.to_string(), tool.to_string(), args);
            self.invocations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(record);
            let is_failing = tool == "failing_tool";
            Box::pin(async move {
                if is_failing {
                    Err("[unavailable] upstream is down".to_string())
                } else {
                    Ok(serde_json::json!({"status": "ok"}))
                }
            })
        }
    }

    /// Pin: the warn-once latch for the unwired kask tool source. The first
    /// unwired observation reports true (the caller warns), subsequent ones
    /// false (no storm); a wired observation re-arms it so a later regression
    /// warns again. Without the latch, `reload_kask_tools` would either warn
    /// on every store event or stay silent — neither lets the operator
    /// distinguish "not configured" from "configured but broken".
    #[test]
    fn kask_tool_source_unwired_warn_latch_fires_once_and_rearms() {
        // Reset to a known state, then simulate the startup window: the
        // source is unwired before main.rs's deferred task runs.
        note_kask_tool_source_wired();
        assert!(
            note_kask_tool_source_unwired(),
            "first unwired observation must report warn"
        );
        assert!(
            !note_kask_tool_source_unwired(),
            "subsequent unwired observations must not warn again"
        );
        // The deferred task lands the source: the latch re-arms.
        note_kask_tool_source_wired();
        assert!(
            note_kask_tool_source_unwired(),
            "a wired observation must re-arm the warn latch"
        );
        // Leave the latch armed-but-unwarned for other tests.
        note_kask_tool_source_wired();
    }

    /// Pin: the `KaskToolSource` process-global hook (wired in `main.rs` to
    /// the governed `McpRuntime`) is settable and replaceable (Mutex slot,
    /// same pattern as the mcp outcome recorder), and — the documented
    /// degradation — `kask_tool_source()` reads `None` while unwired, which
    /// makes `reload_kask_tools` a no-op so kask tools do not surface in
    /// tests and lightweight embedders. The fakes used for the
    /// set/replace assertions expose zero tools so parallel tests are
    /// unaffected; the slot is reset to `None` at the end.
    ///
    /// NOT covered: the degradation is silent by design (the doc on
    /// `kask_tool_source` says tools "simply do not surface" — there is no
    /// operator-visible note/status for the unwired state). The
    /// registry-level merge (`reload_kask_tools` inserting into
    /// `registered_servers`) IS pinned end-to-end now, by
    /// `agent::tests::test_kask_tools_surface_when_source_populates_after_registry_creation`
    /// (tests/mod.rs): the shared-slot leak hazard this comment once cited
    /// is handled there with a distinctive server id, a minimal populated
    /// window, and an empty-source reset.
    #[test]
    fn kask_tool_source_hook_is_settable_replaceable_and_absent_when_unwired() {
        // All assertions on the shared slot stay in ONE test (the recorder
        // pattern) so parallel tests cannot race it.
        let source_a = FakeKaskToolSource::empty();
        set_kask_tool_source(source_a.clone());
        let wired = kask_tool_source().expect("source must be wired after set");
        assert!(
            std::sync::Arc::ptr_eq(&wired, &source_a),
            "set_kask_tool_source must install the given source"
        );

        // Replaceable (Mutex, not OnceLock): a second wiring replaces the
        // first — the deferred re-wiring pattern depends on this.
        let source_b = FakeKaskToolSource::empty();
        set_kask_tool_source(source_b.clone());
        let wired = kask_tool_source().expect("source must remain wired after replace");
        assert!(
            std::sync::Arc::ptr_eq(&wired, &source_b),
            "the slot must be replaceable, not one-shot"
        );
        assert!(!std::sync::Arc::ptr_eq(&wired, &source_a));

        // Absent-source degradation: unset the slot (in-crate tests reach
        // the private static directly) and confirm the hook reads None.
        *KASK_TOOL_SOURCE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        assert!(
            kask_tool_source().is_none(),
            "unwired source must read None — kask tools do not surface"
        );
    }

    /// Pin: `KaskServerTool` — the agent-visible surface of a kask MCP
    /// server tool — passes the descriptor's name/description through and
    /// dispatches `run` through `KaskToolSource::invoke` (the
    /// single-spawn-authority path, I1: dispatch goes to the governed
    /// runtime, never the per-project `ContextServerStore`). `Ok(value)`
    /// surfaces as the tool's LLM text; `Err(text)` surfaces as the error
    /// text verbatim. The source is passed directly (no process-global
    /// wiring), so this test cannot perturb parallel tests.
    #[gpui::test]
    async fn kask_server_tool_dispatches_through_kask_tool_source(cx: &mut gpui::TestAppContext) {
        // `run` authorizes via `authorize_third_party_tool`, which consults
        // the tool-permission settings — default allow, like the MCP tests
        // in `tests/mod.rs`.
        cx.update(|cx| {
            use settings::Settings as _;
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            let mut agent_settings = agent_settings::AgentSettings::get_global(cx).clone();
            agent_settings.tool_permissions.default = settings::ToolPermissionMode::Allow;
            agent_settings::AgentSettings::override_global(agent_settings, cx);
        });

        let invocations = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let descriptors = vec![
            KaskToolDescriptor {
                server_id: "kask-test".to_string(),
                name: "hello_tool".to_string(),
                description: "A governed kask tool".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            KaskToolDescriptor {
                server_id: "kask-test".to_string(),
                name: "failing_tool".to_string(),
                description: "A governed kask tool that fails".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
        ];
        let source = std::sync::Arc::new(FakeKaskToolSource {
            descriptors: descriptors.clone(),
            invocations: invocations.clone(),
        });

        // Descriptor passthrough: the surfaced tool keeps the descriptor's
        // name and description (the merge in `reload_kask_tools` wraps
        // exactly this tool shape).
        let hello = std::sync::Arc::new(KaskServerTool {
            source: source.clone(),
            descriptor: descriptors[0].clone(),
        });
        assert_eq!(hello.name().as_ref(), "hello_tool");
        assert_eq!(hello.description().as_ref(), "A governed kask tool");

        // Success path: dispatch reaches the source with the right server
        // id, tool name, and arguments; the source's value surfaces as the
        // LLM-facing text.
        let (mut sender, input) = ToolInput::<serde_json::Value>::test();
        sender.send_full(serde_json::json!({"query": "hi"}));
        let (event_stream, _rx) = ToolCallEventStream::test();
        let task = cx.update(|cx| hello.run(input, event_stream, cx));
        let output = match task.await {
            Ok(output) => output,
            Err(_) => panic!("successful invoke should return Ok"),
        };
        assert_eq!(
            output.llm_output,
            vec![LanguageModelToolResultContent::Text(
                serde_json::json!({"status": "ok"}).to_string().into(),
            )],
            "the source's Ok(value) must surface as the tool's LLM text"
        );
        {
            let recorded = invocations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert_eq!(
                recorded.as_slice(),
                &[(
                    "kask-test".to_string(),
                    "hello_tool".to_string(),
                    serde_json::json!({"query": "hi"}),
                )],
                "the dispatch must carry the descriptor's server id and tool name"
            );
        }

        // Error path: the source's Err(text) surfaces verbatim as the
        // tool's error text (kask errors carry the typed kind as a
        // `[kind] message` prefix).
        let failing = std::sync::Arc::new(KaskServerTool {
            source,
            descriptor: descriptors[1].clone(),
        });
        let (mut sender, input) = ToolInput::<serde_json::Value>::test();
        sender.send_full(serde_json::json!({}));
        let (event_stream, _rx) = ToolCallEventStream::test();
        let task = cx.update(|cx| failing.run(input, event_stream, cx));
        let error = match task.await {
            Ok(_) => panic!("failing invoke should return Err"),
            Err(error) => error,
        };
        assert_eq!(
            error.llm_output,
            vec![LanguageModelToolResultContent::Text(
                "[unavailable] upstream is down".into(),
            )],
            "the source's Err(text) must surface verbatim as the error text"
        );
    }

    /// Pin (D-seam, `run_inner` retry classification): a request timeout
    /// ("server alive but slow") and a transport death produce DISTINCT
    /// retry verdicts. Only the timeout text classifies as a timeout — the
    /// verdict that makes `run_inner` bail immediately without the 30s
    /// restart-and-retry loop. Transport-death errors classify as
    /// retryable, entering the restart-then-retry-once path. The full-path
    /// timeout half is pinned in `tests/mod.rs`
    /// (`test_mcp_tool_timeout_does_not_retry`); the restart-then-retry
    /// half needs a killable-and-restartable mock server and is pinned
    /// only at this predicate.
    #[test]
    fn timeout_and_transport_death_classify_into_distinct_retry_verdicts() {
        assert!(is_context_server_timeout("Context server request timeout"));
        // The message may carry context after the canonical prefix.
        assert!(is_context_server_timeout(
            "error: Context server request timeout (after 60s)"
        ));

        // Transport-death errors must NOT classify as timeouts — they are
        // the retryable kind, where a restart can actually help.
        for transport_death in [
            "connection reset by peer",
            "broken pipe",
            "child process exited unexpectedly",
            "transport closed while awaiting response",
        ] {
            assert!(
                !is_context_server_timeout(transport_death),
                "{transport_death:?} is a transport death, not a timeout — it must classify as retryable"
            );
        }
    }
}
