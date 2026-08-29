use crate::{AgentToolOutput, AnyAgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use collections::{BTreeMap, HashMap};
use context_server::{ContextServerId, client::NotificationSubscription};
use futures::FutureExt as _;
use gpui::{App, AppContext, AsyncApp, Context, Entity, EventEmitter, SharedString, Task};
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
        this
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
    }
}

struct ContextServerTool {
    store: Entity<ContextServerStore>,
    server_id: ContextServerId,
    tool: context_server::types::Tool,
}

/// Map a completed `ContextServerTool` run to the regulation outcome tuple
/// `(success, error_kind)`. `error_kind` is the typed kind (e.g.
/// `"unavailable"`) when the error text carries the `[kind] ` prefix set by
/// the in-band envelope detection; otherwise the full text is the
/// classification hint, mirroring the McpRuntime path.
fn mcp_run_outcome(result: &Result<AgentToolOutput, AgentToolOutput>) -> (bool, Option<String>) {
    match result {
        Ok(_) => (true, None),
        Err(output) => {
            let text = mcp_error_text(output);
            (
                false,
                Some(hkask_types::tool_response::error_kind_from_display(&text)),
            )
        }
    }
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
                        // death), where a restart can actually help. The timeout bail
                        // message ("Context server request timeout") comes from
                        // `client.rs:483`; matching on the string avoids adding a new
                        // error variant to the context_server crate's public API.
                        let is_timeout = e.to_string().contains("Context server request timeout");
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
                return Err(anyhow::anyhow!(error_message).into());
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
            // zed-kask: D-seam — in-band error envelope detection. kask MCP
            // servers return tool errors as a `{"error": ..., "kind": ...}`
            // content envelope with `is_error` unset (the rmcp String-return
            // convention), so without this check every tool-logical error
            // flows downstream as a success — miscounted by the retry tracker
            // and the regulation ledger, and rendered as a successful tool
            // card. The detection requires a known `McpErrorKind`, so a data
            // payload that happens to carry `error`/`kind` keys can't
            // false-positive. The `[kind] message` text matches
            // `McpToolError`'s Display so consumers can classify by prefix.
            if let Some(envelope) =
                hkask_types::tool_response::parse_tool_error(&concatenated_text)
                && let Some(kind) = envelope.kind
            {
                return Err(
                    anyhow::anyhow!(format!("[{kind}] {}", envelope.message)).into(),
                );
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
        // kask tool errors carry a `[kind] ` prefix (set by the in-band
        // envelope detection in `run_inner`); the ledger must receive the
        // typed kind so config-gap classification (unavailable /
        // permission_denied) works instead of string-matching full messages.
        let output = AgentToolOutput {
            raw_output: serde_json::Value::String(String::new()),
            llm_output: vec![LanguageModelToolResultContent::Text(
                "[unavailable] yt-dlp not found on system PATH".into(),
            )],
        };
        let (success, error_kind) = mcp_run_outcome(&Err(output));
        assert!(!success);
        assert_eq!(error_kind.as_deref(), Some("unavailable"));
    }
}
