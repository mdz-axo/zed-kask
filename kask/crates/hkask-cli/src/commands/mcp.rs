//! MCP server inventory — read-only listing of registered servers and tools.
//!
//! Tool invocation is runtime-only. Use the TUI REPL's `/invoke` slash command
//! or the agent's autonomous tool dispatch. The CLI does not expose a
//! side-door to MCP tool invocation.

use crate::cli::McpAction;
use hkask_mcp_server::BUILTIN_SERVERS;

fn build_agent_service(rt: &tokio::runtime::Runtime) -> hkask_services_context::AgentService {
    let ctx = super::helpers::build_agent_service();
    let userpod_name = ctx.config().user_name.clone();
    super::helpers::start_mcp_servers_with_env(rt, &ctx, BUILTIN_SERVERS, &userpod_name);
    ctx
}

/// Run an MCP inventory command (list-servers, list-tools, get-tool).
pub fn run(rt: &tokio::runtime::Runtime, action: McpAction) {
    match action {
        McpAction::ListServers => {
            let ctx = build_agent_service(rt);
            let servers = rt.block_on(ctx.infra().mcp.clone().list_servers());
            println!("MCP servers:");
            if servers.is_empty() {
                println!("  (no servers registered)");
            } else {
                for server in &servers {
                    println!("  {} ({} tools)", server.id, server.tools.len());
                }
            }
        }
        McpAction::ListTools => {
            let ctx = build_agent_service(rt);
            let tools = rt.block_on(ctx.infra().mcp.clone().discover_tools());
            println!("Available tools:");
            if tools.is_empty() {
                println!("  (no tools registered)");
            } else {
                for tool_name in &tools {
                    println!("  {}", tool_name);
                }
            }
        }
        McpAction::GetTool { name } => {
            let ctx = build_agent_service(rt);
            match rt.block_on(ctx.infra().mcp.clone().get_tool_info(&name)) {
                Some(info) => {
                    println!("Tool: {}", info.name);
                    println!("  Description: {}", info.description);
                    println!("  Server: {}", info.server_id);
                    if let Some(cap) = &info.required_capability {
                        println!("  Required capability: {}", cap);
                    }
                    println!(
                        "  Input schema: {}",
                        serde_json::to_string_pretty(&info.input_schema)
                            .unwrap_or_else(|_| info.input_schema.to_string())
                    );
                }
                None => {
                    eprintln!("Tool '{}' not found", name);
                    std::process::exit(1);
                }
            }
        }
    }
}
