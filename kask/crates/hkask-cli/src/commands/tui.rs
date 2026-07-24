//! `kask tui` — Launch the interactive ratatui workspace.
//!
//! The TUI embeds the REPL as its chat window via `ReplBridge`. If ratatui
//! cannot initialize (no terminal, broken `TERM`), falls back to the
//! line-based REPL automatically.
//!
//! Non-interactive mode: `kask tui -f <file>` or `kask tui -f -` reads from
//! a file or stdin, prints the agent's response, and exits — no TUI launched.

use hkask_templates::SqliteRegistry;
use std::path::PathBuf;
use std::sync::Arc;

use crate::repl_host::CliHost;

fn execute_one_shot(input_path: &PathBuf, execute: impl FnOnce(&str)) {
    let content = super::helpers::or_exit(
        std::fs::read_to_string(input_path),
        "Failed to read input file",
    );
    execute(content.trim());
}

/// Launch the TUI workspace or non-interactive chat.
///
/// pre:  rt is a valid tokio Runtime; registry is initialized
/// post: launches TUI (interactive) or prints one chat response (non-interactive via -f)
#[allow(clippy::too_many_arguments)]
pub fn run_tui(
    _rt: &tokio::runtime::Runtime,
    registry: &mut SqliteRegistry,
    handle: &tokio::runtime::Handle,
    template: Option<String>,
    input: Option<PathBuf>,
    mcp_servers: Vec<String>,
    agent: String,
    model: Option<String>,
) {
    if let Some(input_path) = input {
        execute_one_shot(&input_path, |content| {
            hkask_repl::run_once(
                registry,
                model.as_deref(),
                content,
                &mcp_servers,
                handle.clone(),
                Arc::new(CliHost),
            );
        });
    } else {
        // Launch the TUI workspace. The TUI hosts the REPL as its chat window.
        // If the TUI feature is not built, fall back to the line-based REPL.
        #[cfg(feature = "tui")]
        {
            hkask_repl::run_tui(
                registry,
                template.as_deref(),
                &agent,
                model.as_deref(),
                handle.clone(),
                Arc::new(CliHost),
            );
        }
        #[cfg(not(feature = "tui"))]
        {
            eprintln!("TUI not built — rebuild with `cargo build --features tui`");
            hkask_repl::run(
                registry,
                template.as_deref(),
                &agent,
                model.as_deref(),
                handle.clone(),
                Arc::new(CliHost),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_input_is_trimmed_and_dispatched() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let input_path = dir.path().join("request.txt");
        std::fs::write(&input_path, "  use training_submit  \n").expect("write input");

        let mut received = None;
        execute_one_shot(&input_path, |input| received = Some(input.to_string()));

        assert_eq!(received.as_deref(), Some("use training_submit"));
    }
}
