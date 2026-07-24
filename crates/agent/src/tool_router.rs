//! Context-aware tool router.
//!
//! Selects and activates tools based on the current context (open files,
//! user message content, available tools). Replaces static mode-based tool
//! scoping (Roo Code modes) with a dynamic, context-driven approach.
//!
//! The router is a trait with a heuristic default implementation. When no
//! router is wired (upstream Zed), all enabled tools pass through (I2).

use gpui::SharedString;

/// Context for tool selection. Built from the current turn's state: the
/// user's latest message, open file paths, and the set of available tools.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct ToolSelectionContext {
    /// The latest user message text, if any.
    pub user_message: Option<String>,
    /// Paths of files currently open in the editor.
    pub open_file_paths: Vec<String>,
    /// The set of tool names available before routing (after profile and
    /// feature-flag filtering). The router returns a subset of these.
    pub available_tools: Vec<SharedString>,
}

/// A tool router selects which tools to activate based on context.
///
/// The default implementation (`HeuristicToolRouter`) scores each tool 0.0–1.0
/// on relevance to the context and returns only those scoring ≥ 0.30
/// (matching `skill-router`'s cutoff). When no router is wired, all tools
/// pass through unchanged (I2 — upstream Zed compatibility).
pub trait ToolRouter: Send + Sync {
    /// Select tools based on the given context. Returns the names of tools
    /// that should be activated. An empty return value means "no filtering"
    /// (fail-open) — the caller should pass all tools through.
    #[allow(dead_code)]
    fn select_tools(&self, context: &ToolSelectionContext) -> Vec<SharedString>;
}

/// Heuristic tool router. Scores tools based on simple signals:
/// - `.rs`/`.ts`/`.py` file open ⇒ boost `grep`, `read_file`, `edit_file`,
///   `write_file`, `diagnostics`, `find_path`, `list_directory`
/// - URL in message ⇒ boost `fetch`, `web_search`
/// - "terminal"/"run"/"execute" in message ⇒ boost `terminal`
/// - Otherwise: baseline 0.1 (below threshold, dropped)
///
/// Returns tools scoring ≥ 0.30.
pub struct HeuristicToolRouter;

impl ToolRouter for HeuristicToolRouter {
    fn select_tools(&self, context: &ToolSelectionContext) -> Vec<SharedString> {
        let has_code_file = context
            .open_file_paths
            .iter()
            .any(|path| is_code_file(path));

        let has_url = context
            .user_message
            .as_deref()
            .is_some_and(|msg| msg.contains("http://") || msg.contains("https://"));

        let has_terminal_keyword = context.user_message.as_deref().is_some_and(|msg| {
            let lower = msg.to_lowercase();
            lower.contains("terminal")
                || lower.contains("run ")
                || lower.contains("execute")
                || lower.contains("command")
                || lower.contains("shell")
        });

        let has_search_keyword = context.user_message.as_deref().is_some_and(|msg| {
            let lower = msg.to_lowercase();
            lower.contains("search") || lower.contains("find ") || lower.contains("grep")
        });

        context
            .available_tools
            .iter()
            .filter_map(|tool_name| {
                let score = score_tool(
                    tool_name.as_ref(),
                    has_code_file,
                    has_url,
                    has_terminal_keyword,
                    has_search_keyword,
                );
                if score >= 0.30 {
                    Some(tool_name.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Score a tool 0.0–1.0 based on context signals.
#[allow(dead_code)]
fn score_tool(
    tool_name: &str,
    has_code_file: bool,
    has_url: bool,
    has_terminal_keyword: bool,
    has_search_keyword: bool,
) -> f64 {
    match tool_name {
        // Code-editing tools: boosted when a code file is open
        "read_file" | "edit_file" | "write_file" | "grep" | "find_path" | "list_directory"
        | "diagnostics" | "copy_path" | "move_path" | "delete_path" | "create_directory" => {
            if has_code_file {
                0.7
            } else if has_search_keyword {
                0.5
            } else {
                0.1
            }
        }
        // LSP tools: boosted when a code file is open
        "find_references" | "get_code_actions" | "apply_code_action" | "go_to_definition"
        | "rename" => {
            if has_code_file {
                0.6
            } else {
                0.1
            }
        }
        // Web tools: boosted when a URL is in the message
        "fetch" | "web_search" => {
            if has_url {
                0.7
            } else {
                0.1
            }
        }
        // Terminal: boosted when terminal keywords are present
        "terminal" => {
            if has_terminal_keyword {
                0.7
            } else {
                0.1
            }
        }
        // Subagent and thread tools: always available (baseline)
        "spawn_agent" | "create_thread" | "list_agents_and_models" => 0.5,
        // Skill tool: always available
        "skill" => 0.5,
        // Unknown tools (e.g. MCP): pass through
        _ => 0.5,
    }
}

/// Check if a file path looks like a code file.
#[allow(dead_code)]
fn is_code_file(path: &str) -> bool {
    let extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
        ".rb", ".php", ".swift", ".kt", ".scala", ".lua", ".vim", ".elisp", ".clj", ".hs", ".ml",
        ".fs", ".ex", ".exs", ".erl", ".zig", ".nim", ".v", ".sv", ".d", ".dart",
    ];
    extensions.iter().any(|ext| path.ends_with(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_router_returns_code_tools_for_rs_file() {
        let context = ToolSelectionContext {
            user_message: Some("Fix the bug".to_string()),
            open_file_paths: vec!["/project/src/main.rs".to_string()],
            available_tools: vec![
                "grep".into(),
                "read_file".into(),
                "fetch".into(),
                "web_search".into(),
                "terminal".into(),
            ],
        };
        let router = HeuristicToolRouter;
        let selected = router.select_tools(&context);
        assert!(selected.contains(&"grep".into()));
        assert!(selected.contains(&"read_file".into()));
        assert!(!selected.contains(&"fetch".into()));
        assert!(!selected.contains(&"web_search".into()));
        assert!(!selected.contains(&"terminal".into()));
    }

    #[test]
    fn test_heuristic_router_returns_web_tools_for_url() {
        let context = ToolSelectionContext {
            user_message: Some("Check https://example.com".to_string()),
            open_file_paths: vec![],
            available_tools: vec![
                "grep".into(),
                "read_file".into(),
                "fetch".into(),
                "web_search".into(),
            ],
        };
        let router = HeuristicToolRouter;
        let selected = router.select_tools(&context);
        assert!(selected.contains(&"fetch".into()));
        assert!(selected.contains(&"web_search".into()));
        assert!(!selected.contains(&"grep".into()));
        assert!(!selected.contains(&"read_file".into()));
    }

    #[test]
    fn test_heuristic_router_returns_terminal_for_terminal_keyword() {
        let context = ToolSelectionContext {
            user_message: Some("Run the tests in the terminal".to_string()),
            open_file_paths: vec![],
            available_tools: vec!["terminal".into(), "grep".into()],
        };
        let router = HeuristicToolRouter;
        let selected = router.select_tools(&context);
        assert!(selected.contains(&"terminal".into()));
        assert!(!selected.contains(&"grep".into()));
    }

    #[test]
    fn test_heuristic_router_passes_through_unknown_tools() {
        let context = ToolSelectionContext {
            user_message: None,
            open_file_paths: vec![],
            available_tools: vec!["mcp_custom_tool".into()],
        };
        let router = HeuristicToolRouter;
        let selected = router.select_tools(&context);
        assert!(selected.contains(&"mcp_custom_tool".into()));
    }

    #[test]
    fn test_heuristic_router_returns_subagent_always() {
        let context = ToolSelectionContext {
            user_message: None,
            open_file_paths: vec![],
            available_tools: vec!["spawn_agent".into(), "grep".into()],
        };
        let router = HeuristicToolRouter;
        let selected = router.select_tools(&context);
        assert!(selected.contains(&"spawn_agent".into()));
        assert!(!selected.contains(&"grep".into()));
    }

    #[test]
    fn test_is_code_file_detects_extensions() {
        assert!(is_code_file("/project/src/main.rs"));
        assert!(is_code_file("app.tsx"));
        assert!(is_code_file("script.py"));
        assert!(!is_code_file("README.md"));
        assert!(!is_code_file("config.json"));
    }
}
