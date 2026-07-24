//! Slash command registry and dispatch for the hKask REPL.

use super::ReplState;
use super::display::{print_command_help, print_help};
use super::handlers;

// ── Command table ──────────────────────────────────────────────────────

pub(super) struct SlashCommand {
    pub primary: &'static str,
    pub aliases: &'static [&'static str],
    pub args: &'static str,
    pub about: &'static str,
}

impl SlashCommand {
    pub(super) fn matches(&self, input: &str) -> bool {
        input == self.primary || self.aliases.contains(&input)
    }
}

pub(super) const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        primary: "help",
        aliases: &["h", "?"],
        args: "[COMMAND]",
        about: "Show help, or details for a specific command",
    },
    SlashCommand {
        primary: "quit",
        aliases: &["q", "exit"],
        args: "",
        about: "End the session",
    },
    SlashCommand {
        primary: "clear",
        aliases: &["cls"],
        args: "",
        about: "Clear the screen",
    },
    SlashCommand {
        primary: "status",
        aliases: &["st"],
        args: "",
        about: "System status (Regulation, agent, pod count)",
    },
    SlashCommand {
        primary: "agent",
        aliases: &["a"],
        args: "[NAME]",
        about: "Switch agent, or show current",
    },
    SlashCommand {
        primary: "agents",
        aliases: &["ls"],
        args: "",
        about: "List registered agents",
    },
    SlashCommand {
        primary: "pods",
        aliases: &[],
        args: "",
        about: "List agent pods",
    },
    SlashCommand {
        primary: "templates",
        aliases: &["tpl"],
        args: "",
        about: "List registered templates",
    },
    SlashCommand {
        primary: "tools",
        aliases: &[],
        args: "",
        about: "List MCP tools",
    },
    SlashCommand {
        primary: "mcp",
        aliases: &[],
        args: "list|start <server|all>",
        about: "Manage MCP server connections (P2: opt-in)",
    },
    SlashCommand {
        primary: "ask",
        aliases: &[],
        args: "<AGENT> <MESSAGE>",
        about: "Force a specific agent to respond",
    },
    SlashCommand {
        primary: "model",
        aliases: &["m"],
        args: "[NAME|QUERY|list|refresh]",
        about: "Switch model, fuzzy search, list, or refresh the catalog",
    },
    SlashCommand {
        primary: "fusion",
        aliases: &[],
        args: "[off|on|status]",
        about: "Show or toggle fusion mode (multi-model deliberation)",
    },
    SlashCommand {
        primary: "escalations",
        aliases: &["esc"],
        args: "",
        about: "List pending escalations",
    },
    SlashCommand {
        primary: "resolve",
        aliases: &[],
        args: "<ID>",
        about: "Resolve an escalation",
    },
    SlashCommand {
        primary: "dismiss",
        aliases: &[],
        args: "<ID>",
        about: "Dismiss an escalation",
    },
    SlashCommand {
        primary: "metacognition",
        aliases: &["meta"],
        args: "",
        about: "Run a metacognition cycle",
    },
    SlashCommand {
        primary: "invoke",
        aliases: &["inv"],
        args: "<server>/<tool> [args]",
        about: "Invoke an MCP tool",
    },
    SlashCommand {
        primary: "sovereignty",
        aliases: &["sov"],
        args: "",
        about: "Show sovereignty status",
    },
    SlashCommand {
        primary: "history",
        aliases: &["hist"],
        args: "",
        about: "Show session history",
    },
    SlashCommand {
        primary: "bundle",
        aliases: &["b"],
        args: "[SKILL1 SKILL2 ...] | list | show <id> | apply <id> | evolve <id> | skills | off",
        about: "Compose, apply, or manage skill bundles",
    },
    SlashCommand {
        primary: "repl",
        aliases: &[],
        args: "[SETTING] [VALUE]",
        about: "Show or set REPL inference settings",
    },
    SlashCommand {
        primary: "skill",
        aliases: &["sk"],
        args: "list [public|private] | status <name> | publish <name> | audit [--json] | derive <name>",
        about: "Skill discovery, status, publishing, and auditing",
    },
    SlashCommand {
        primary: "goal",
        aliases: &[],
        args: "create <text> | list [state] | set-state <id> <state>",
        about: "Goal coordination substrate",
    },
    SlashCommand {
        primary: "kata",
        aliases: &[],
        args: "list | show <name> | start <name> [--bot <name>] [--ctx key=val]",
        about: "Kata practice cycles (coaching, improvement, starter)",
    },
    SlashCommand {
        primary: "pod",
        aliases: &[],
        args: "list | status <id> | create <template> <persona> [name] | activate <id> | deactivate <id>",
        about: "Pod lifecycle management",
    },
    SlashCommand {
        primary: "adapter",
        aliases: &[],
        args: "list | status <job_id> | deploy <id> <provider> | teardown <id>",
        about: "Trained adapter lifecycle",
    },
    SlashCommand {
        primary: "consolidate",
        aliases: &["cons"],
        args: "[LIMIT] [--floor CONFIDENCE] [--max MAX_TRIPLES]",
        about: "Trigger episodic→semantic consolidation with optional semantic cleanup",
    },
    SlashCommand {
        primary: "start",
        aliases: &["tour", "onboarding"],
        args: "",
        about: "Take a guided tour of hKask's key capabilities",
    },
    SlashCommand {
        primary: "feedback",
        aliases: &[],
        args: "",
        about: "Submit onboarding or usability feedback (appended to local feedback.md)",
    },
    SlashCommand {
        primary: "listen",
        aliases: &["rec", "record"],
        args: "start [SECONDS] | stop | view [FILE]",
        about: "Record audio, transcribe, and play back with word-level sync",
    },
    SlashCommand {
        primary: "talk",
        aliases: &["speak"],
        args: "on | off | voice [DESCRIPTION]",
        about: "Enable spoken summaries of agent responses (TTS)",
    },
    SlashCommand {
        primary: "improv",
        aliases: &["imp"],
        args: "[plussing|yes-and|yes-but|freestyle|riff]",
        about: "Set or display the active improv interaction mode",
    },
    SlashCommand {
        primary: "matrix",
        aliases: &["mx"],
        args: "[ROOM_ID]",
        about: "List Matrix rooms, or show messages from a room",
    },
    SlashCommand {
        primary: "msg",
        aliases: &["dm"],
        args: "<ROOM_ID> <MESSAGE>",
        about: "Send a message to a Matrix room",
    },
    SlashCommand {
        primary: "kanban",
        aliases: &["kb"],
        args: "list|board|task|move|accept|submit|decompose|spawn",
        about: "Kanban board and task coordination",
    },
    SlashCommand {
        primary: "thread",
        aliases: &["th"],
        args: "list|switch <id>|new [title]|archive <id>",
        about: "Manage chat threads — short-term memory across sessions",
    },
];

// ── Lookup ─────────────────────────────────────────────────────────────

pub(super) fn find_command(input: &str) -> Option<&'static SlashCommand> {
    SLASH_COMMANDS.iter().find(|c| c.matches(input))
}

pub(super) fn fuzzy_match_command(input: &str) -> Vec<&'static SlashCommand> {
    let lower = input.to_lowercase();
    SLASH_COMMANDS
        .iter()
        .filter(|c| {
            c.primary.contains(&lower)
                || c.aliases.iter().any(|a| a.contains(&lower))
                || c.about.to_lowercase().contains(&lower)
        })
        .collect()
}

// ── Dispatch ───────────────────────────────────────────────────────────

pub(super) fn handle_slash_command(
    input: &str,
    template_id: Option<&str>,
    rt: &tokio::runtime::Handle,
    state: &mut ReplState,
) -> bool {
    let without_slash = &input[1..];
    let parts: Vec<&str> = without_slash.splitn(3, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let arg1 = parts.get(1).map(|s| s.trim()).unwrap_or("");
    let arg2 = parts.get(2).map(|s| s.trim()).unwrap_or("");

    match cmd.as_str() {
        // Trivial commands — inline is clearer than a function call
        "help" | "h" | "?" => {
            if arg1.is_empty() {
                print_help()
            } else {
                print_command_help(arg1)
            }
        }
        "quit" | "q" | "exit" => {
            println!("Goodbye!");
            return true;
        }
        "clear" | "cls" => {
            print!("\x1b[2J\x1b[H");
        }

        // Delegated to handler modules
        "status" | "st" => handlers::handle_status(state, template_id, rt),
        "agent" | "a" => handlers::handle_agent(arg1, arg2, state, rt),
        "agents" | "ls" => handlers::handle_agents(state),
        "history" | "hist" => handlers::handle_history(state),
        "pods" => handlers::handle_pods(rt, state),
        "templates" | "tpl" => handlers::handle_templates(state, rt),
        "tools" => handlers::handle_tools(state, rt),
        "mcp" => handlers::handle_mcp(state, arg1, arg2, rt),
        "escalations" | "esc" => handlers::handle_escalations(state),
        "resolve" => handlers::handle_resolve(arg1, state),
        "dismiss" => handlers::handle_dismiss(arg1, state),
        "metacognition" | "meta" => {
            rt.block_on(async {
                match hkask_services_chat::chat::service::ChatService::run_curator_metacognition(
                    &state.service_context,
                )
                .await
                {
                    Ok(summary) => println!("  {}", summary),
                    Err(e) => println!("  Error: {}", e),
                }
            });
            println!();
        }
        "sovereignty" | "sov" => {
            state.host.run_sovereignty_status();
        }
        "ask" => handlers::handle_ask(arg1, arg2, rt, state),
        "model" | "m" => handlers::handle_model(arg1, rt, state),
        "fusion" => handlers::handle_fusion(arg1, state),
        "consolidate" | "cons" => {
            let cons_arg = if arg2.is_empty() {
                arg1.to_string()
            } else {
                format!("{} {}", arg1, arg2)
            };
            handlers::handle_consolidate(&cons_arg, state, rt);
        }
        "bundle" | "b" => match arg1 {
            "list" | "show" | "apply" | "evolve" | "skills" | "off" | "" => {
                let rest = if arg2.is_empty() {
                    arg1.to_string()
                } else {
                    format!("{} {}", arg1, arg2)
                };
                handlers::handle_bundle(arg1, &rest, state, rt);
            }
            skills_arg => {
                // Compose: /bundle skill1 skill2 ...
                let rest = format!("{} {}", arg1, arg2);
                handlers::handle_bundle("", &rest, state, rt);
                let _ = skills_arg;
            }
        },
        "repl" => handlers::handle_repl_set(arg1, arg2, state),
        "skill" | "sk" => handlers::handle_skill(arg1, arg2, state),
        "goal" => handlers::handle_goal(arg1, arg2, state, rt),
        "kata" => handlers::handle_kata(arg1, arg2, state, rt),
        "pod" => handlers::handle_pod(arg1, arg2, state, rt),
        "adapter" => handlers::handle_adapter(arg1, arg2, state),
        "start" | "tour" | "onboarding" => handlers::handle_start(state),
        "feedback" => handlers::handle_feedback(state),
        "listen" | "rec" | "record" => handlers::handle_listen(arg1, arg2, state, rt),
        "talk" | "speak" => handlers::handle_talk(arg1, arg2, state, rt),
        "improv" | "imp" => handlers::handle_improv(arg1, arg2, state),
        #[cfg(feature = "communication")]
        "matrix" | "mx" => handlers::handle_matrix(arg1, rt),
        #[cfg(feature = "communication")]
        "msg" | "dm" => handlers::handle_msg(arg1, arg2, rt),
        #[cfg(not(feature = "communication"))]
        cmd @ ("matrix" | "mx" | "msg" | "dm") => {
            println!(
                "  Matrix communication not built — rebuild with `cargo build --features communication`"
            );
            let _ = cmd;
        }
        "kanban" | "kb" => handlers::handle_kanban(arg1, arg2, state, rt),
        "thread" | "th" => handlers::handle_thread(arg1, arg2, state),

        _ => {
            let fuzzy = fuzzy_match_command(&cmd);
            if fuzzy.is_empty() {
                println!("  Unknown command: \x1b[31m/{}\x1b[0m", cmd);
            } else {
                println!("  Unknown command: \x1b[31m/{}\x1b[0m — did you mean:", cmd);
                for c in &fuzzy {
                    println!("    \x1b[36m/{}\x1b[0m — {}", c.primary, c.about);
                }
            }
            println!("  Type \x1b[36m/help\x1b[0m for available commands.");
            println!();
        }
    }
    false
}
