//! REPL info handlers — /history, /pods, /templates, /tools

use hkask_services_chat::MemoryService;

pub fn handle_history(state: &super::super::ReplState) {
    println!("{}", history_display(state));
}

/// Render the session history (recent turns) as a display string (no printing).
/// Shared by the REPL `/history` handler and the TUI `SessionBridge`.
pub(crate) fn history_display(state: &super::super::ReplState) -> String {
    let token = state.service_context.governance().checker.grant_registry(
        hkask_capability::DelegationAction::Read,
        *state.service_context.webid(),
        state.agent_webid,
    );
    match MemoryService::recall_recent_turns(
        &state
            .service_context
            .per_agent_memory(&state.current_agent)
            .expect("per-agent memory")
            .episodic_storage,
        &state.agent_webid,
        &token,
        usize::MAX, // retrieve all turns for display
    ) {
        Some(history) => {
            let turn_count = history.lines().filter(|l| l.starts_with("User:")).count();
            let mut out = format!("Session history ({} turns):\n", turn_count);
            for line in history.lines() {
                if line.is_empty() {
                    continue;
                }
                let preview = if line.len() > 80 {
                    format!("{}…", &line[..80])
                } else {
                    line.to_string()
                };
                out.push_str(&format!("    {}\n", preview));
            }
            out
        }
        None => "No turns in this session yet.".to_string(),
    }
}

pub fn handle_pods(rt: &tokio::runtime::Handle, state: &super::super::ReplState) {
    let pods = state.service_context.infra().pods.clone();
    match rt.block_on(pods.list_pods()) {
        Ok(pods) => {
            if pods.is_empty() {
                println!("  No pods registered.");
            } else {
                println!("  \x1b[1mAgent pods ({}):\x1b[0m", pods.len());
                for pod in &pods {
                    println!("  \x1b[36m{}\x1b[0m ({})", pod.pod_id, pod.state);
                    println!("    WebID: {}", pod.webid);
                    if let Some(name) = &pod.name {
                        println!("    Name:  {}", name);
                    }
                }
            }
        }
        Err(e) => println!("  \x1b[31mPod listing unavailable:\x1b[0m {}", e),
    }
    println!();
}

pub fn handle_templates(state: &crate::ReplState, rt: &tokio::runtime::Handle) {
    let entries = rt.block_on(async { state.host.list_templates_local() });
    if entries.is_empty() {
        println!("  No templates registered.");
    } else {
        println!("  \x1b[1mTemplates ({}):\x1b[0m", entries.len());
        for entry in &entries {
            println!(
                "  \x1b[36m{}\x1b[0m ({})",
                entry.id,
                entry.template_type.as_str()
            );
        }
    }
    println!();
}

pub fn handle_tools(state: &mut super::super::ReplState, rt: &tokio::runtime::Handle) {
    let tools = rt.block_on(state.service_context.infra().mcp.discover_tools());
    if tools.is_empty() {
        println!("  No MCP tools available.");
        println!(
            "  Use \x1b[36m/mcp list\x1b[0m to see available servers and \x1b[36m/mcp start <server>\x1b[0m to load one."
        );
    } else {
        println!("  \x1b[1mMCP Tools ({}):\x1b[0m", tools.len());
        for tool_name in &tools {
            if let Some(info) =
                rt.block_on(state.service_context.infra().mcp.get_tool_info(tool_name))
            {
                println!("  \x1b[36m{}\x1b[0m — {}", info.name, info.description);
            } else {
                println!("  \x1b[36m{}\x1b[0m", tool_name);
            }
        }
        println!("  \x1b[2mAll tool calls route through McpRuntime (OCAP + gas)\x1b[0m");
    }
    println!();
}
