//! REPL /ask handler — force a specific agent to respond.
//!
//! Routes through the same `single_agent_turn` pipeline as direct REPL input,
//! with the agent name overridden. This gives /ask full access to tool calls,
//! governed tool invocation, and the tool-use loop (up to tool_loop_limit).

pub fn handle_ask(
    arg1: &str,
    arg2: &str,
    rt: &tokio::runtime::Handle,
    state: &mut super::super::ReplState,
) {
    if arg1.is_empty() || arg2.is_empty() {
        println!("  Usage: \x1b[36m/ask <agent> <message>\x1b[0m");
        return;
    }

    // Resolve A2A secret for tool invocation signing.
    let a2a_secret = match &state.resolved_secrets {
        Some(secrets) => {
            hkask_types::secret::ZeroizingSecret::new(secrets.a2a_secret.as_bytes().to_vec())
        }
        None => {
            eprintln!("Error: No A2A secret resolved. Run `kask chat` to complete onboarding.");
            return;
        }
    };

    // Route through the single-agent turn pipeline with the specified agent
    // as override. This uses the full tool-augmented inference path: tool
    // definitions in the request, <<tool:...>> parsing, GovernedTool invocation,
    // and the tool-use loop for followup corrections.
    if state.active_session.is_some() {
        state.active_session = None;
    }
    super::super::turn::single_agent_turn(arg2, state, rt, &a2a_secret, Some(arg1));
}
