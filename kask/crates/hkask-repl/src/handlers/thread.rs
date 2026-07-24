//! REPL /thread handler — manage chat threads (short-term memory).
//!
//! Threads persist across sessions in `agents/{name}/threads.json`.
//! Auto-archival runs on session start based on `short_term_memory_life`.

use super::super::ReplState;
use super::super::threads::ThreadStatus;

pub fn handle_thread(arg1: &str, arg2: &str, state: &mut ReplState) {
    match arg1 {
        "" | "list" => {
            let threads = state.thread_registry.list();
            if threads.is_empty() {
                println!("  No chat threads yet.");
            } else {
                println!(
                    "  \x1b[1mChat Threads\x1b[0m — \x1b[36m/thread switch <id>\x1b[0m to resume"
                );
                println!();
                for t in &threads {
                    let status = match t.status {
                        ThreadStatus::Active => "\x1b[32m● active\x1b[0m",
                        ThreadStatus::Archived => "\x1b[2m○ archived\x1b[0m",
                    };
                    let marker = if state.thread_registry.active_thread_id.as_deref() == Some(&t.id)
                    {
                        " \x1b[1m← current\x1b[0m"
                    } else {
                        ""
                    };
                    println!(
                        "  {}  \x1b[36m{}\x1b[0m{}  \x1b[2m{} msgs, {}\x1b[0m",
                        status,
                        &t.id[..8.min(t.id.len())],
                        marker,
                        t.message_count,
                        t.title,
                    );
                }
                println!();
                let active = threads
                    .iter()
                    .filter(|t| t.status == ThreadStatus::Active)
                    .count();
                let archived = threads.len() - active;
                println!(
                    "  \x1b[2m{} active, {} archived — stm_life: {} days\x1b[0m",
                    active, archived, state.repl_settings.short_term_memory_life,
                );
            }
            println!();
        }

        "switch" => {
            if arg2.is_empty() {
                println!(
                    "  Usage: \x1b[36m/thread switch <id>\x1b[0m (use /thread list to see IDs)"
                );
                println!();
                return;
            }
            // Collect matching thread ID first (avoid simultaneous borrow).
            let matched_id = state
                .thread_registry
                .threads
                .keys()
                .find(|id| id.starts_with(arg2))
                .cloned();
            match matched_id {
                Some(id) => {
                    if state.thread_registry.switch_to(&id, &state.current_agent) {
                        let t = state.thread_registry.get(&id).unwrap();
                        println!(
                            "  \x1b[32mSwitched to thread:\x1b[0m {} (\x1b[2m{} msgs\x1b[0m)",
                            t.title, t.message_count
                        );
                        println!(
                            "  \x1b[2mPast conversation history is preserved in episodic memory.\x1b[0m"
                        );
                    }
                }
                None => {
                    println!("  \x1b[31mNo thread found with prefix '{}'\x1b[0m", arg2);
                    println!("  Use \x1b[36m/thread list\x1b[0m to see available threads.");
                }
            }
            println!();
        }

        "new" => {
            let title = if arg2.is_empty() { "New session" } else { arg2 };
            let t = state
                .thread_registry
                .create_thread(&state.current_agent, title);
            println!(
                "  \x1b[32mCreated thread:\x1b[0m {} (\x1b[2m{}\x1b[0m)",
                &t.id[..8.min(t.id.len())],
                t.title
            );
            println!();
        }

        "archive" => {
            if arg2.is_empty() {
                println!(
                    "  Usage: \x1b[36m/thread archive <id>\x1b[0m (use /thread list to see IDs)"
                );
                println!();
                return;
            }
            // Collect matching thread ID and current status first.
            let target: Option<(String, ThreadStatus)> = {
                let exact = state.thread_registry.threads.get(arg2);
                if let Some(t) = exact {
                    Some((arg2.to_string(), t.status.clone()))
                } else {
                    state
                        .thread_registry
                        .threads
                        .iter()
                        .find(|(id, _)| id.starts_with(arg2))
                        .map(|(id, t)| (id.clone(), t.status.clone()))
                }
            };
            match target {
                Some((id, current_status)) => {
                    let new_status = match current_status {
                        ThreadStatus::Active => ThreadStatus::Archived,
                        ThreadStatus::Archived => ThreadStatus::Active,
                    };
                    if state
                        .thread_registry
                        .set_status(&id, &state.current_agent, new_status)
                    {
                        let t = state.thread_registry.get(&id).unwrap();
                        let label = match t.status {
                            ThreadStatus::Active => "activated",
                            ThreadStatus::Archived => "archived",
                        };
                        println!("  \x1b[32mThread {}:\x1b[0m {}", label, t.title);
                    }
                }
                None => {
                    println!("  \x1b[31mNo thread found with prefix '{}'\x1b[0m", arg2);
                }
            }
            println!();
        }

        _ => {
            println!("  Unknown /thread subcommand: '{}'", arg1);
            println!(
                "  Usage: \x1b[36m/thread list\x1b[0m | \x1b[36m/thread switch <id>\x1b[0m | \x1b[36m/thread new [title]\x1b[0m | \x1b[36m/thread archive <id>\x1b[0m"
            );
            println!();
        }
    }
}
