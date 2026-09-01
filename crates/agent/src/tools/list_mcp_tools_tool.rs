use agent_client_protocol::schema::v1 as acp;
use gpui::{App, Entity, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    AgentTool, ToolCallEventStream, ToolInput,
    tools::context_server_registry::ContextServerRegistry,
};

/// Enumerate every MCP server tool registered in this session, grouped by
/// server, with each tool's name and description.
///
/// Your visible tool list can be smaller than the registered surface — tools
/// are filtered by the active agent profile or a panel's server scope. Call
/// this before concluding a tool is unavailable,
/// and to find the exact tool name to ask the user to enable or to name in
/// their next message. Pass `filter` to narrow the listing with a
/// case-insensitive substring matched against server ids, tool names, and
/// descriptions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListMcpToolsToolInput {
    /// Optional case-insensitive substring matched against server ids, tool
    /// names, and descriptions. Omit to list the entire registered surface.
    #[serde(default)]
    pub filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMcpToolsToolOutput {
    pub servers: Vec<McpServerToolListing>,
    /// Total registered tools in the listing (after any filter).
    pub total_tools: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerToolListing {
    pub server_id: String,
    pub tools: Vec<McpToolListing>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolListing {
    pub name: String,
    pub description: String,
}

impl From<ListMcpToolsToolOutput> for LanguageModelToolResultContent {
    fn from(output: ListMcpToolsToolOutput) -> Self {
        serde_json::to_string(&output)
            .unwrap_or_else(|e| format!("Failed to serialize list_mcp_tools output: {e}"))
            .into()
    }
}

pub struct ListMcpToolsTool {
    registry: Entity<ContextServerRegistry>,
}

impl ListMcpToolsTool {
    pub fn new(registry: Entity<ContextServerRegistry>) -> Self {
        Self { registry }
    }
}

impl AgentTool for ListMcpToolsTool {
    type Input = ListMcpToolsToolInput;
    type Output = ListMcpToolsToolOutput;

    const NAME: &'static str = "list_mcp_tools";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(ListMcpToolsToolInput {
                filter: Some(filter),
            }) => format!("List MCP tools matching \"{filter}\"").into(),
            _ => "List MCP tools".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|_| ListMcpToolsToolOutput {
                servers: Vec::new(),
                total_tools: 0,
            })?;
            let filter = input.filter.as_deref().map(str::to_lowercase);
            Ok(cx.update(|cx| {
                self.registry
                    .read(cx)
                    .enumerate_tool_listing(filter.as_deref())
            }))
        })
    }
}

impl ContextServerRegistry {
    /// The registered MCP surface as a listing: every server with every tool's
    /// name and description, optionally narrowed by a case-insensitive
    /// substring over server id, tool name, or description. Read-only — this
    /// is the discovery half of D44: the model pulls the index on demand
    /// instead of a router pushing a per-turn guess. Pinned by
    /// `test_list_mcp_tools_enumerates_and_filters` (tests/mod.rs).
    pub fn enumerate_tool_listing(&self, filter: Option<&str>) -> ListMcpToolsToolOutput {
        let mut servers = Vec::new();
        let mut total_tools = 0;
        for (server_id, tools) in self.servers() {
            let mut tool_listings = Vec::new();
            for (name, tool) in tools {
                let description = tool.description();
                if let Some(filter) = filter {
                    let haystack =
                        format!("{} {} {}", server_id.0, name, description).to_lowercase();
                    if !haystack.contains(filter) {
                        continue;
                    }
                }
                tool_listings.push(McpToolListing {
                    name: name.to_string(),
                    description: description.to_string(),
                });
            }
            if tool_listings.is_empty() {
                continue;
            }
            total_tools += tool_listings.len();
            servers.push(McpServerToolListing {
                server_id: server_id.0.to_string(),
                tools: tool_listings,
            });
        }
        ListMcpToolsToolOutput {
            servers,
            total_tools,
        }
    }
}
