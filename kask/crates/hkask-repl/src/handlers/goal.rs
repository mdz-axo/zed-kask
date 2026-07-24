//! `/goal` REPL commands — goal coordination substrate.
//!
//! Calls `SqliteGoalRepository` directly via the service context's storage.

use crate::ReplState;
use hkask_types::visibility::Visibility;
use std::str::FromStr;

/// Handle `/goal` REPL commands.
pub fn handle_goal(
    subcommand: &str,
    rest: &str,
    state: &mut ReplState,
    _rt: &tokio::runtime::Handle,
) {
    let repo = &state.service_context.storage().goals;
    let webid = state.agent_webid;

    match subcommand {
        "" | "help" => {
            println!("  \x1b[1mGoal Commands\x1b[0m");
            println!("    \x1b[36m/goal create <text>\x1b[0m              Create a goal");
            println!("    \x1b[36m/goal list [state]\x1b[0m              List goals");
            println!("    \x1b[36m/goal set-state <id> <state>\x1b[0m     Transition a goal");
            println!();
            println!("  \x1b[2mStates: pending, active, completed, abandoned\x1b[0m");
            println!();
        }

        "create" => {
            let text = rest.trim();
            if text.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Goal text required");
                println!("  Usage: \x1b[36m/goal create <text>\x1b[0m");
                println!();
                return;
            }
            match repo.create_goal(&webid, text, Visibility::Private) {
                Ok(goal) => {
                    println!("  \x1b[32m✓\x1b[0m Created goal '{}'", goal.id);
                    println!("    State: {}", goal.state.as_str());
                    println!("    Text:  {}", goal.text);
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Create failed: {}", e);
                    println!();
                }
            }
        }

        "list" => {
            let state_filter = rest
                .split_whitespace()
                .next()
                .and_then(hkask_types::GoalState::parse_str);

            match repo.list_goals(&webid, state_filter) {
                Ok(goals) => {
                    if goals.is_empty() {
                        println!("  No goals found.");
                    } else {
                        println!("  \x1b[1mGoals\x1b[0m");
                        for g in &goals {
                            println!(
                                "    {} [{}] {}",
                                g.id,
                                g.state.as_str(),
                                g.text.chars().take(60).collect::<String>()
                            );
                        }
                    }
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m List failed: {}", e);
                    println!();
                }
            }
        }

        "set-state" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                println!("  \x1b[31mError:\x1b[0m Goal ID and state required");
                println!("  Usage: \x1b[36m/goal set-state <id> <state>\x1b[0m");
                println!();
                return;
            }
            let id_str = parts[0];
            let new_state_str = parts[1];

            let goal_id = match hkask_types::GoalID::from_str(id_str) {
                Ok(id) => id,
                Err(_) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Invalid goal ID: {}", id_str);
                    println!();
                    return;
                }
            };

            let new_state = match hkask_types::GoalState::parse_str(new_state_str) {
                Some(s) => s,
                None => {
                    eprintln!("  \x1b[31m✗\x1b[0m Invalid state: {}", new_state_str);
                    println!("  Valid states: pending, active, completed, abandoned");
                    println!();
                    return;
                }
            };

            match repo.update_goal_state(goal_id, new_state) {
                Ok(()) => {
                    println!(
                        "  \x1b[32m✓\x1b[0m Goal {} → {}",
                        id_str,
                        new_state.as_str()
                    );
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Transition failed: {}", e);
                    println!();
                }
            }
        }

        _ => {
            println!("  Unknown goal subcommand: \x1b[31m{}\x1b[0m", subcommand);
            println!("  Type \x1b[36m/goal help\x1b[0m for available commands.");
            println!();
        }
    }
}
