//! REPL /start handler — guided tour for new users.
//!
//! Walks through key capabilities one step at a time. Each step is a single
//! line of explanation; the user presses Enter to continue. Designed for
//! progressive disclosure: session basics → model switching → system status
//! → tools → settings → advanced features.

use crate::ReplState;

/// Run the guided tour. Each step prints a short explanation and waits for
/// the user to press Enter. The user can type "skip" or "quit" at any prompt
/// to exit the tour early.
pub fn handle_start(state: &ReplState) {
    println!();
    println!("  \x1b[1;36m━━━ hKask Guided Tour ━━━\x1b[0m");
    println!("  Press Enter to continue, or type \x1b[2mskip\x1b[0m/\x1b[2mquit\x1b[0m to exit.");
    println!();

    let steps: &[(&str, &str)] = &[
        (
            "Chat",
            &format!(
                "  You're chatting with \x1b[36m{}\x1b[0m using model \x1b[36m{}\x1b[0m.\n  Just type anything to talk — your userpod responds with tools and knowledge.",
                state.current_agent, state.current_model
            ),
        ),
        (
            "Commands",
            "  Slash commands start with \x1b[36m/\x1b[0m. Try these:\n  \x1b[36m/help\x1b[0m    — full command list\n  \x1b[36m/agent\x1b[0m   — switch agents\n  \x1b[36m/clear\x1b[0m   — clear the screen\n  \x1b[36m/quit\x1b[0m   — exit hKask",
        ),
        (
            "Models",
            "  \x1b[36m/model\x1b[0m       — show current model\n  \x1b[36m/model list\x1b[0m  — browse available models\n  \x1b[36m/model qwen\x1b[0m  — fuzzy search for models\n  Different models have different strengths — experiment!",
        ),
        (
            "Status",
            "  \x1b[36m/status\x1b[0m — see system health:\n  • Regulation (Cybernetic Nervous System) status\n  • Energy budget (gas remaining)\n  • Active pods and loops\n  • Circuit breaker state",
        ),
        (
            "Tools",
            "  \x1b[36m/tools\x1b[0m — discover MCP tools your userpod can use:\n  • Web search, document parsing, memory storage\n  • Research, specifications, code analysis\n  • Your userpod auto-selects tools based on your query",
        ),
        (
            "Settings",
            "  \x1b[36m/repl\x1b[0m           — show all inference settings\n  \x1b[36m/repl temp 0.5\x1b[0m — lower temperature for focused answers\n  \x1b[36m/repl temp 1.0\x1b[0m — higher temperature for creative responses\n  Settings persist across sessions.",
        ),
        (
            "Memory",
            "  Your userpod remembers conversations via episodic memory.\n  \x1b[36m/consolidate\x1b[0m — promote important knowledge to long-term memory.\n  \x1b[36m/history\x1b[0m     — review recent conversation turns.",
        ),
        (
            "Done!",
            "  \x1b[1;32mYou're ready to use hKask!\x1b[0m\n  \x1b[36m/help\x1b[0m anytime for the full command reference.\n  \x1b[36m/start\x1b[0m anytime to replay this tour.\n  Happy building!",
        ),
    ];

    for (i, (title, body)) in steps.iter().enumerate() {
        println!(
            "  \x1b[1;33mStep {}/{}: {}\x1b[0m",
            i + 1,
            steps.len(),
            title
        );
        println!("{}", body);
        println!();

        if i == steps.len() - 1 {
            // Last step — no prompt needed
            break;
        }

        print!("  \x1b[2mPress Enter to continue...\x1b[0m");
        use std::io::Write;
        let _ = std::io::stdout().flush();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            break;
        }
        let trimmed = input.trim().to_lowercase();
        if trimmed == "skip" || trimmed == "quit" || trimmed == "exit" {
            println!("  Tour ended. Type \x1b[36m/help\x1b[0m for commands.");
            println!();
            return;
        }
        // Move cursor up, carriage-return to col 0, then clear the line.
        // Without \r the cursor sits at end-of-line after \x1b[2K and the
        // next step title would render mid-line instead of column 0.
        print!("\x1b[1A\r\x1b[2K");
    }

    println!();
}
