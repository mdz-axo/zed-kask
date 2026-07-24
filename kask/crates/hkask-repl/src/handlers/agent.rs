//! REPL /agent and /agents handlers — userpod info, listing, A2A registration.
//!
//! One user = one userpod. There is no switching — the current userpod is
//! set during REPL init and never changes. These commands show the current
//! userpod, list A2A-registered agents, and allow manual A2A registration.

use hkask_types::WebID;

/// Handle `/agent` — show current userpod, or register a new A2A agent.
pub fn handle_agent(
    arg1: &str,
    rest: &str,
    state: &mut super::super::ReplState,
    rt: &tokio::runtime::Handle,
) {
    match arg1 {
        "" => {
            println!("  Current userpod: \x1b[1m{}\x1b[0m", state.current_agent);
            println!(
                "  Use \x1b[36m/agent register <webid> [cap1,cap2,...]\x1b[0m to register in A2A"
            );
            println!();
        }

        "register" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                println!("  \x1b[31mError:\x1b[0m WebID required");
                println!("  Usage: \x1b[36m/agent register <webid> [cap1,cap2,...]\x1b[0m");
                println!();
                return;
            }
            let webid_str = parts[0];
            let capabilities: Vec<String> = parts
                .get(1)
                .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
                .unwrap_or_default();

            let webid = match webid_str.parse::<WebID>() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Invalid WebID '{}': {}", webid_str, e);
                    println!();
                    return;
                }
            };

            let a2a = state.service_context.governance().a2a.clone();
            match rt.block_on(a2a.register_agent(webid, capabilities.clone())) {
                Ok(_token) => {
                    println!("  \x1b[32m✓\x1b[0m Registered agent: {}", webid_str);
                    println!("    Capabilities: {}", capabilities.join(", "));
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Registration failed: {}", e);
                    println!();
                }
            }
        }

        _ => {
            println!("  \x1b[33mNo switching — one userpod per user.\x1b[0m");
            println!("  Current userpod: \x1b[1m{}\x1b[0m", state.current_agent);
            println!("  Use \x1b[36m/agent register <webid>\x1b[0m to register in A2A");
            println!();
        }
    }
}

/// Render the A2A-registered agent list as a display string (no printing).
pub(crate) fn list_agents_display(state: &super::super::ReplState) -> String {
    let a2a = state.service_context.governance().a2a.clone();
    let agents = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(a2a.list_agents())
    });
    if agents.is_empty() {
        return "No agents registered in A2A.".to_string();
    }
    let mut out = format!("A2A Agents ({})\n", agents.len());
    out.push_str(&format!("{:<60} {:<12} CAPABILITIES\n", "WEBID", "ACTIVE"));
    out.push_str(&"-".repeat(90));
    out.push('\n');
    for agent in &agents {
        out.push_str(&format!(
            "{:<60} {:<12} {}\n",
            agent.webid.to_string(),
            agent.active,
            agent.capabilities.join(", ")
        ));
    }
    out
}

/// Handle `/agents` — list all A2A-registered agents.
pub fn handle_agents(state: &super::super::ReplState) {
    println!("{}", list_agents_display(state));
}
