//! Lazy tool router.
//!
//! Narrows the tool set only when the user's request is complex enough to
//! benefit from filtering. For simple interactions, all tools pass through
//! unchanged — the model is good at selecting from a list when the request
//! is straightforward.
//!
//! The router activates in two cases:
//!
//! 1. **Explicit tool request** — the user message mentions a specific tool
//!    name or capability ("use grep", "run terminal", "fetch this URL").
//!    The router narrows to tools whose descriptions overlap with the request.
//!
//! 2. **Complex request** — the message is long (≥ 40 words) or signals
//!    decomposition ("plan", "break down", "multiple steps", "subagent",
//!    "delegate"). The router narrows to tools relevant to the task,
//!    reducing the tool list the model must reason about.
//!
//! For all other messages, the router returns empty (fail-open — no
//! filtering). This preserves the full tool set for simple interactions
//! and avoids starving the model on short prompts.
//!
//! When no router is wired (upstream Zed), all tools pass through (I2).

use gpui::SharedString;
use std::collections::HashSet;

/// A candidate tool for routing. The name + description pair is what the
/// router scores — descriptions are available for all tools including MCP
/// tools via `AnyAgentTool::description()`.
#[derive(Debug, Clone)]
pub struct ToolCandidate {
    pub name: SharedString,
    pub description: SharedString,
}

/// Context for tool selection, built from the current turn's state.
#[derive(Debug, Clone, Default)]
pub struct ToolSelectionContext {
    /// The latest user message text, if any.
    pub user_message: Option<String>,
    /// Absolute paths of files currently open in the editor.
    pub open_file_paths: Vec<String>,
    /// All candidate tools (name + description) after profile/feature-flag
    /// filtering. The router returns a subset of these names.
    pub candidates: Vec<ToolCandidate>,
}

/// A tool router selects which tools to activate based on context.
///
/// Returns `Some(names)` to filter to that set. Returns `None` to signal
/// "no filtering" (fail-open) — the caller passes all tools through.
/// This distinguishes "router did not activate" from "router activated
/// but found no matching tools."
pub trait ToolRouter: Send + Sync {
    fn select_tools(&self, context: &ToolSelectionContext) -> Option<Vec<SharedString>>;
}

/// Apply the tool router to a set of tools, bypassing built-in zed tools.
///
/// zed-kask: the router only filters context-server (MCP) tools. Built-in
/// zed tools (those in `built_in_names`) always pass through — the router
/// was introduced to tame MCP tool floods, not to second-guess the
/// agent-profile allowlist for core tools.
///
/// `tools` is an iterator of `(name, description)` pairs for all tools
/// currently enabled (after profile/feature-flag filtering). The function
/// returns the set of tool names to retain. Built-in tools are always in
/// the returned set. MCP tools are retained only if the router includes
/// them (or if the router returns `None` / fails open).
///
/// This is a free function so it can be unit-tested without the
/// process-global `TOOL_ROUTER` `OnceLock`.
pub fn apply_router_bypassing_built_ins<'a, I>(
    router: &dyn ToolRouter,
    tools: I,
    user_message: Option<&str>,
    open_file_paths: Vec<String>,
    built_in_names: &std::collections::HashSet<&str>,
) -> std::collections::HashSet<SharedString>
where
    I: IntoIterator<Item = (&'a SharedString, &'a SharedString)>,
{
    // Collect into a map for lookup.
    let tool_map: std::collections::HashMap<&SharedString, &SharedString> =
        tools.into_iter().collect();

    // Start with all built-in tools — they bypass the router.
    let mut retained: std::collections::HashSet<SharedString> = tool_map
        .keys()
        .filter(|name| built_in_names.contains(name.as_ref()))
        .map(|name| (*name).clone())
        .collect();

    // Build candidates from MCP tools only.
    let candidates: Vec<ToolCandidate> = tool_map
        .iter()
        .filter(|(name, _)| !built_in_names.contains(name.as_ref()))
        .map(|(name, description)| ToolCandidate {
            name: (*name).clone(),
            description: (*description).clone(),
        })
        .collect();

    // If there are no MCP candidates, skip the router entirely.
    if candidates.is_empty() {
        return retained;
    }

    let context = ToolSelectionContext {
        user_message: user_message.map(|s| s.to_string()),
        open_file_paths,
        candidates,
    };
    let selected = router.select_tools(&context);
    if let Some(selected) = selected {
        // Retain MCP tools that the router selected.
        for name in selected {
            if tool_map.contains_key(&name) {
                retained.insert(name);
            }
        }
    } else {
        // Router returned None (fail-open) — retain all MCP tools.
        for name in tool_map.keys() {
            if !built_in_names.contains(name.as_ref()) {
                retained.insert((*name).clone());
            }
        }
    }

    retained
}

/// Lazy keyword-overlap tool router. Only activates when the request is
/// complex or explicitly tool-directed. For simple messages, returns `None`
/// (fail-open).
///
/// When activated, scores each tool by keyword overlap between the context
/// and the tool's description. Tools scoring ≥ the threshold are included.
/// Always-on tools (spawn_agent, skill, etc.) bypass scoring.
pub struct LazyToolRouter {
    /// Tools that are always included when the router activates.
    always_on: HashSet<&'static str>,
    /// Score threshold for inclusion.
    threshold: f64,
    /// Minimum word count for a message to be considered "complex."
    complex_word_threshold: usize,
}

impl LazyToolRouter {
    pub fn new() -> Self {
        Self {
            always_on: [
                "spawn_agent",
                "skill",
                "create_thread",
                "list_agents_and_models",
            ]
            .into_iter()
            .collect(),
            threshold: 0.30,
            complex_word_threshold: 40,
        }
    }
}

impl Default for LazyToolRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRouter for LazyToolRouter {
    fn select_tools(&self, context: &ToolSelectionContext) -> Option<Vec<SharedString>> {
        let Some(message) = &context.user_message else {
            return None; // No message → fail-open.
        };

        // Decide whether to activate. The router is lazy — it only filters
        // when the request is complex or explicitly tool-directed.
        if !self.should_activate(message, &context.open_file_paths) {
            return None; // Not activated → fail-open.
        }

        let context_keywords = extract_context_keywords(context);
        let has_code_file = context
            .open_file_paths
            .iter()
            .any(|path| is_code_file(path));

        // Activated: return the filtered set (may be empty if no tools match).
        Some(
            context
                .candidates
                .iter()
                .filter_map(|candidate| {
                    let score = self.score_tool(candidate, &context_keywords, has_code_file);
                    if score >= self.threshold {
                        Some(candidate.name.clone())
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }
}

impl LazyToolRouter {
    /// Decide whether the router should activate for this message.
    ///
    /// Activates when:
    /// - The message mentions a specific tool name (e.g., "use grep", "run terminal")
    /// - The message is complex (long, or signals decomposition/planning)
    /// - A code file is open AND the message mentions file/edit/search keywords
    ///
    /// Does NOT activate for simple greetings, short questions, or messages
    /// with no tool-relevant signal.
    fn should_activate(&self, message: &str, open_file_paths: &[String]) -> bool {
        let lower = message.to_lowercase();
        let word_count = message.split_whitespace().count();

        // 1. Explicit tool name mention — always activate.
        let mentions_tool = TOOL_NAME_SIGNALS
            .iter()
            .any(|signal| lower.contains(signal));
        if mentions_tool {
            return true;
        }

        // 2. Complex request — long message or decomposition signals.
        if word_count >= self.complex_word_threshold {
            return true;
        }
        let complex_signals = [
            "plan",
            "break down",
            "decompose",
            "multiple steps",
            "subagent",
            "delegate",
            "parallel",
            "coordinate",
            "orchestrate",
        ];
        if complex_signals.iter().any(|signal| lower.contains(signal)) {
            return true;
        }

        // 3. Code file open + file/edit/search keywords — activate to
        // narrow to relevant code tools.
        let has_code_file = open_file_paths.iter().any(|path| is_code_file(path));
        if has_code_file {
            let code_signals = [
                "edit",
                "write",
                "read",
                "fix",
                "refactor",
                "search",
                "grep",
                "find",
                "delete",
                "create",
                "move",
                "rename",
                "diagnostic",
                "debug",
                "test",
                "build",
                "compile",
            ];
            if code_signals.iter().any(|signal| lower.contains(signal)) {
                return true;
            }
        }

        false
    }

    fn score_tool(
        &self,
        candidate: &ToolCandidate,
        context_keywords: &HashSet<String>,
        has_code_file: bool,
    ) -> f64 {
        // Always-on tools bypass scoring.
        if self.always_on.contains(candidate.name.as_ref()) {
            return 1.0;
        }

        let description_lower = candidate.description.to_lowercase();
        let description_words: HashSet<&str> = description_lower.split_whitespace().collect();

        // Count how many context keywords appear in the tool description.
        let overlapping = context_keywords
            .iter()
            .filter(|keyword| {
                let keyword_lower = keyword.to_lowercase();
                if keyword_lower.len() <= 4 {
                    description_words.contains(keyword_lower.as_str())
                } else {
                    description_lower.contains(&keyword_lower)
                }
            })
            .count();

        let overlap_ratio = overlapping as f64 / context_keywords.len().max(1) as f64;

        // Intent-signal keywords get a higher weight.
        let intent_overlap = context_keywords
            .iter()
            .filter(|keyword| {
                let kw = keyword.as_str();
                matches!(
                    kw,
                    "url" | "fetch" | "web" | "terminal" | "shell" | "command" | "search"
                )
            })
            .filter(|keyword| {
                let keyword_lower = keyword.to_lowercase();
                if keyword_lower.len() <= 4 {
                    description_words.contains(keyword_lower.as_str())
                } else {
                    description_lower.contains(&keyword_lower)
                }
            })
            .count();

        let mut score = 0.2 + 0.4 * overlap_ratio + 0.4 * (intent_overlap as f64).min(1.0);

        // File-type boost: if a code file is open OR the message mentions
        // code-editing actions, boost tools whose descriptions mention
        // file/edit/grep/directory/diagnostic.
        let code_action_signal = context_keywords.iter().any(|kw| {
            matches!(
                kw.as_str(),
                "edit"
                    | "write"
                    | "read"
                    | "fix"
                    | "refactor"
                    | "search"
                    | "grep"
                    | "find"
                    | "delete"
                    | "create"
                    | "move"
                    | "rename"
                    | "debug"
                    | "test"
                    | "build"
                    | "compile"
            )
        });
        if has_code_file || code_action_signal {
            let code_tool_keywords = [
                "file",
                "edit",
                "write",
                "read",
                "grep",
                "search",
                "directory",
                "path",
                "diagnostic",
                "definition",
                "reference",
                "rename",
                "code action",
                "symbol",
            ];
            if code_tool_keywords
                .iter()
                .any(|kw| description_lower.contains(kw))
            {
                score = score.max(0.5);
            }
        }

        score.min(1.0)
    }
}

/// Words in user messages that signal an explicit tool request. When any
/// of these appear, the router activates to narrow the tool set.
const TOOL_NAME_SIGNALS: &[&str] = &[
    "grep",
    "read file",
    "read_file",
    "edit file",
    "edit_file",
    "write file",
    "write_file",
    "terminal",
    "fetch",
    "web search",
    "web_search",
    "find path",
    "find_path",
    "list directory",
    "list_directory",
    "diagnostics",
    "find references",
    "find_references",
    "code action",
    "go to definition",
    "rename",
    "spawn agent",
    "spawn_agent",
    "create thread",
    "create_thread",
];

/// Extract keywords from the context: user message words (excluding
/// stopwords), file extensions from open file paths, and intent signals
/// (URL presence, terminal keywords) that map to tool capabilities.
fn extract_context_keywords(context: &ToolSelectionContext) -> HashSet<String> {
    let mut keywords = HashSet::new();

    if let Some(message) = &context.user_message {
        for word in message.split_whitespace() {
            let cleaned = word
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .to_lowercase();
            if cleaned.len() > 2 && !STOPWORDS.contains(&cleaned.as_str()) {
                keywords.insert(cleaned);
            }
        }

        // Intent signals: add synthetic keywords that map to tool
        // capabilities.
        let lower = message.to_lowercase();
        if lower.contains("http://") || lower.contains("https://") {
            keywords.insert("url".to_string());
            keywords.insert("fetch".to_string());
            keywords.insert("web".to_string());
        }
        if lower.contains("terminal")
            || lower.contains("run ")
            || lower.contains("execute")
            || lower.contains("command")
            || lower.contains("shell")
        {
            keywords.insert("terminal".to_string());
            keywords.insert("shell".to_string());
            keywords.insert("command".to_string());
        }
        if lower.contains("search") || lower.contains("find ") || lower.contains("grep") {
            keywords.insert("search".to_string());
        }
    }

    // Add file extensions as keywords.
    for path in &context.open_file_paths {
        if let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
        {
            keywords.insert(ext.to_lowercase());
        }
    }

    keywords
}

/// Common English stopwords to exclude from keyword extraction.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "her", "was", "one", "our",
    "out", "day", "get", "has", "him", "his", "how", "its", "may", "new", "now", "old", "see",
    "two", "way", "who", "boy", "did", "man", "men", "put", "say", "she", "too", "use", "this",
    "that", "with", "have", "from", "they", "will", "would", "there", "their", "what", "about",
    "which", "when", "your", "them", "then", "than", "been", "want", "into", "some", "like",
    "just", "also", "make", "more", "most", "such", "only", "does", "done", "very", "much", "need",
    "even", "here", "know", "think", "help", "please", "could", "should",
];

/// Check if a file path looks like a code file.
fn is_code_file(path: &str) -> bool {
    let extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
        ".rb", ".php", ".swift", ".kt", ".scala", ".lua", ".vim", ".clj", ".hs", ".ml", ".fs",
        ".ex", ".exs", ".erl", ".zig", ".nim", ".v", ".sv", ".d", ".dart",
    ];
    extensions.iter().any(|ext| path.ends_with(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(name: &str, description: &str) -> ToolCandidate {
        ToolCandidate {
            name: name.into(),
            description: description.into(),
        }
    }

    #[test]
    fn test_lazy_router_does_not_activate_for_simple_message() {
        let context = ToolSelectionContext {
            user_message: Some("hello".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("grep", "Search file contents using a regular expression"),
                candidate("read_file", "Read a file from the project filesystem"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router.select_tools(&context);
        assert!(
            selected.is_none(),
            "simple message should fail-open (no filtering)"
        );
    }

    #[test]
    fn test_lazy_router_activates_for_explicit_tool_request() {
        let context = ToolSelectionContext {
            user_message: Some("use grep to search for the function".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("grep", "Search file contents using a regular expression"),
                candidate("fetch", "Fetches a URL and returns content as Markdown"),
                candidate("spawn_agent", "Spawn a sub-agent for a task"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(selected.contains(&"grep".into()));
        assert!(selected.contains(&"spawn_agent".into()));
        assert!(
            !selected.contains(&"fetch".into()),
            "fetch should be filtered — no URL context"
        );
    }

    #[test]
    fn test_lazy_router_activates_for_complex_request() {
        let long_message = "I need to plan a multi-step refactoring of the authentication \
            module. We should break down the work into parallel subtasks and delegate \
            each one to a subagent. The first step is to search for all usages of the \
            old auth function, then edit each call site to use the new API, and finally \
            run the test suite to verify nothing broke.";
        let context = ToolSelectionContext {
            user_message: Some(long_message.to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("grep", "Search file contents using a regular expression"),
                candidate("read_file", "Read a file from the project filesystem"),
                candidate("edit_file", "Edit a file in the project"),
                candidate("terminal", "Execute a shell command"),
                candidate("fetch", "Fetches a URL and returns content"),
                candidate("spawn_agent", "Spawn a sub-agent for a task"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(selected.contains(&"grep".into()));
        assert!(selected.contains(&"read_file".into()));
        assert!(selected.contains(&"edit_file".into()));
        assert!(selected.contains(&"spawn_agent".into()));
        assert!(
            !selected.contains(&"fetch".into()),
            "fetch should be filtered — no URL in a refactoring task"
        );
    }

    #[test]
    fn test_lazy_router_activates_for_code_file_with_edit_signal() {
        let context = ToolSelectionContext {
            user_message: Some("fix the bug in main.rs".to_string()),
            open_file_paths: vec!["/project/src/main.rs".to_string()],
            candidates: vec![
                candidate("read_file", "Read a file from the filesystem"),
                candidate("grep", "Search file contents using a regular expression"),
                candidate("fetch", "Fetches a URL and returns content"),
                candidate("spawn_agent", "Spawn a sub-agent for a task"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(selected.contains(&"read_file".into()));
        assert!(selected.contains(&"grep".into()));
        assert!(selected.contains(&"spawn_agent".into()));
        assert!(!selected.contains(&"fetch".into()));
    }

    #[test]
    fn test_lazy_router_does_not_activate_for_short_question() {
        let context = ToolSelectionContext {
            user_message: Some("what does this function do?".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("grep", "Search file contents"),
                candidate("read_file", "Read a file"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router.select_tools(&context);
        assert!(
            selected.is_none(),
            "short question should fail-open — no tool signal"
        );
    }

    #[test]
    fn test_lazy_router_includes_fetch_when_url_in_explicit_request() {
        let context = ToolSelectionContext {
            user_message: Some("fetch the content from https://example.com".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("fetch", "Fetches a URL and returns content as Markdown"),
                candidate("grep", "Search file contents using a regular expression"),
                candidate("web_search", "Search the web for information"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(selected.contains(&"fetch".into()));
        assert!(selected.contains(&"web_search".into()));
    }

    #[test]
    fn test_lazy_router_scores_mcp_tools_by_description() {
        let context = ToolSelectionContext {
            user_message: Some(
                "use grep to search the corpus for investment strategies".to_string(),
            ),
            open_file_paths: vec![],
            candidates: vec![
                candidate(
                    "corpus_search",
                    "Search the embedded document corpus for relevant passages using semantic query",
                ),
                candidate(
                    "lora_train",
                    "Configure and launch a LoRA training run on the GPU",
                ),
                candidate("grep", "Search file contents using a regular expression"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(
            selected.contains(&"corpus_search".into()),
            "corpus_search should match via 'search' keyword overlap"
        );
        assert!(selected.contains(&"grep".into()));
        assert!(
            !selected.contains(&"lora_train".into()),
            "lora_train should be filtered — no keyword overlap"
        );
    }

    #[test]
    fn test_lazy_router_always_includes_spawn_agent_when_active() {
        let context = ToolSelectionContext {
            user_message: Some("use grep to find the function".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("spawn_agent", "Spawn a sub-agent for a task"),
                candidate("grep", "Search file contents using a regular expression"),
                candidate("fetch", "Fetches a URL"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(selected.contains(&"spawn_agent".into()));
        assert!(selected.contains(&"grep".into()));
        assert!(!selected.contains(&"fetch".into()));
    }

    #[test]
    fn test_is_code_file_detects_extensions() {
        assert!(is_code_file("/project/src/main.rs"));
        assert!(is_code_file("app.tsx"));
        assert!(is_code_file("script.py"));
        assert!(!is_code_file("README.md"));
        assert!(!is_code_file("config.json"));
    }

    // zed-kask: `Thread::enabled_tools` filters built-in tool names out of
    // the candidates list before calling `select_tools`, so the router only
    // ever sees MCP/context-server tools. This contract must hold — if a
    // future change passes built-in tool names to the router, the agent
    // silently loses access to `fetch`, `diagnostics`, `list_directory`,
    // etc. on ordinary coding requests. See the `.rules` entry
    // "LazyToolRouter filters MCP tools only".
    #[test]
    fn test_router_only_sees_mcp_candidates_when_built_ins_filtered() {
        // Simulate what `enabled_tools` now does: filter out built-in tool
        // names before constructing the candidates list. The router should
        // only filter MCP tools, and built-in tools (never passed in) are
        // unaffected by definition.
        let built_in_names: std::collections::HashSet<&str> =
            crate::tools::ALL_TOOL_NAMES.iter().copied().collect();

        // All tools the thread has registered.
        let all_tools = [
            candidate("grep", "Search file contents using a regular expression"),
            candidate("fetch", "Fetches a URL and returns content as Markdown"),
            candidate("diagnostics", "Get errors and warnings for the project"),
            candidate(
                "list_directory",
                "List files and directories in a given path",
            ),
            candidate("spawn_agent", "Spawn a sub-agent for a task"),
            // MCP tools (not in ALL_TOOL_NAMES):
            candidate(
                "corpus_search",
                "Search the embedded document corpus for relevant passages",
            ),
            candidate(
                "lora_train",
                "Configure and launch a LoRA training run on the GPU",
            ),
        ];

        // Filter out built-in tools, as `enabled_tools` does.
        let mcp_candidates: Vec<_> = all_tools
            .iter()
            .filter(|c| !built_in_names.contains(c.name.as_ref()))
            .cloned()
            .collect();

        // Only the MCP tools should remain as candidates.
        assert_eq!(mcp_candidates.len(), 2);
        assert!(mcp_candidates.iter().any(|c| c.name == "corpus_search"));
        assert!(mcp_candidates.iter().any(|c| c.name == "lora_train"));

        // The router activates and filters MCP tools by keyword overlap.
        let context = ToolSelectionContext {
            user_message: Some(
                "use grep to search the corpus for investment strategies".to_string(),
            ),
            open_file_paths: vec![],
            candidates: mcp_candidates,
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate for tool-directed message");

        // corpus_search matches via 'search' keyword overlap; lora_train does not.
        assert!(
            selected.contains(&"corpus_search".into()),
            "corpus_search should match via 'search' keyword overlap"
        );
        assert!(
            !selected.contains(&"lora_train".into()),
            "lora_train should be filtered — no keyword overlap"
        );

        // Built-in tools are never in `selected` because they were never
        // passed as candidates. `enabled_tools` retains them unconditionally
        // via the `built_in_names.contains(name) || selected.contains(name)`
        // check. This is the contract: built-in tools bypass the router.
        assert!(!selected.contains(&"grep".into()));
        assert!(!selected.contains(&"fetch".into()));
        assert!(!selected.contains(&"diagnostics".into()));
        assert!(!selected.contains(&"list_directory".into()));
        // spawn_agent is always_on in the router, but it was never passed as
        // a candidate, so it's not in `selected` either. `enabled_tools`
        // retains it via the built-in bypass.
        assert!(!selected.contains(&"spawn_agent".into()));
    }

    /// Stub router that returns a fixed selection, for testing
    /// `apply_router_bypassing_built_ins` without the `LazyToolRouter`'s
    /// keyword-scoring logic.
    struct StubRouter {
        selected: Option<Vec<SharedString>>,
    }

    impl ToolRouter for StubRouter {
        fn select_tools(&self, _context: &ToolSelectionContext) -> Option<Vec<SharedString>> {
            self.selected.clone()
        }
    }

    #[test]
    fn test_apply_router_retains_all_built_ins_unconditionally() {
        let built_in_names: std::collections::HashSet<&str> =
            ["grep", "fetch", "read_file"].into_iter().collect();

        // Tools: 3 built-in + 2 MCP.
        let tools: Vec<(SharedString, SharedString)> = vec![
            ("grep".into(), "Search file contents".into()),
            ("fetch".into(), "Fetch a URL".into()),
            ("read_file".into(), "Read a file".into()),
            ("corpus_search".into(), "Search the corpus".into()),
            ("lora_train".into(), "Train a LoRA".into()),
        ];

        // Router selects only corpus_search — lora_train is filtered out.
        let router = StubRouter {
            selected: Some(vec!["corpus_search".into()]),
        };

        let retained = apply_router_bypassing_built_ins(
            &router,
            tools.iter().map(|(n, d)| (n, d)),
            Some("search the corpus"),
            vec![],
            &built_in_names,
        );

        // All built-in tools are retained, regardless of router selection.
        assert!(retained.contains("grep"));
        assert!(retained.contains("fetch"));
        assert!(retained.contains("read_file"));
        // MCP tools: corpus_search is retained (router selected it),
        // lora_train is filtered out.
        assert!(retained.contains("corpus_search"));
        assert!(!retained.contains("lora_train"));
    }

    #[test]
    fn test_apply_router_fail_open_retains_all_mcp() {
        let built_in_names: std::collections::HashSet<&str> = ["grep"].into_iter().collect();

        let tools: Vec<(SharedString, SharedString)> = vec![
            ("grep".into(), "Search file contents".into()),
            ("corpus_search".into(), "Search the corpus".into()),
            ("lora_train".into(), "Train a LoRA".into()),
        ];

        // Router returns None (fail-open — did not activate).
        let router = StubRouter { selected: None };

        let retained = apply_router_bypassing_built_ins(
            &router,
            tools.iter().map(|(n, d)| (n, d)),
            Some("hello"),
            vec![],
            &built_in_names,
        );

        // Fail-open: all tools retained (built-in + MCP).
        assert!(retained.contains("grep"));
        assert!(retained.contains("corpus_search"));
        assert!(retained.contains("lora_train"));
    }

    #[test]
    fn test_apply_router_no_mcp_tools_skips_router() {
        let built_in_names: std::collections::HashSet<&str> =
            ["grep", "fetch"].into_iter().collect();

        // Only built-in tools, no MCP tools.
        let tools: Vec<(SharedString, SharedString)> = vec![
            ("grep".into(), "Search file contents".into()),
            ("fetch".into(), "Fetch a URL".into()),
        ];

        // Router would filter everything if called, but it shouldn't be
        // called at all when there are no MCP candidates.
        let router = StubRouter {
            selected: Some(vec![]), // empty selection = filter everything
        };

        let retained = apply_router_bypassing_built_ins(
            &router,
            tools.iter().map(|(n, d)| (n, d)),
            Some("use grep to search"),
            vec![],
            &built_in_names,
        );

        // All built-in tools retained — router was skipped.
        assert!(retained.contains("grep"));
        assert!(retained.contains("fetch"));
    }
}
