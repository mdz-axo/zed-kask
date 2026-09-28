mod apply_code_action_tool;
mod ask_user_tool;
mod context_server_registry;
mod copy_path_tool;
mod create_directory_tool;
mod create_thread_tool;
mod curator_tools;
mod delete_path_tool;
mod diagnostics_tool;
mod edit_file_tool;
mod edit_session;
#[cfg(all(test, feature = "unit-eval"))]
mod evals;
mod fetch_tool;
mod find_path_tool;
mod find_references_tool;
mod get_code_actions_tool;
mod go_to_definition_tool;
mod grep_tool;
mod lean_check_tool;
#[cfg(test)]
mod lisp_eval_conformance;
mod lisp_eval_emission_matrix;
mod lisp_eval_tool;
mod list_agents_and_models_tool;
mod list_directory_tool;
mod list_mcp_tools_tool;
mod move_path_tool;
mod onto_anchor_tool;
mod read_file_tool;
mod record_skill_feedback_tool;
mod rename_tool;
mod render_template_tool;
mod skill_tool;
mod spawn_agent_tool;
mod symbol_locator;
mod terminal_tool;
mod tool_permissions;
mod web_search_tool;
mod write_file_tool;

use crate::AgentTool;
use feature_flags::{
    CreateThreadToolFeatureFlag, FeatureFlagAppExt as _, LspToolFeatureFlag, RenameToolFeatureFlag,
};
use gpui::App;
use language_model::LanguageModelRequestTool;
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, Error as _},
};

/// Deserialize a tool-input field, teaching on rejection: the error names
/// the field, the received shape, and the accepted shape. An error that does
/// not teach produces identical retries — the anatomy of the 2026-09-28
/// audit lockout (5 identical malformed-env emissions hard-refused by the
/// per-input retry tracker). `tolerate_stringified` additionally accepts
/// the field as a stringified JSON string (models occasionally stringify
/// nested arguments — the `lisp_eval.env` / `edit_file.edits` /
/// `render_template.context` pattern). The teaching message replaces the
/// inner serde error entirely, so no position noise ("at line 1 column N")
/// reaches the model.
pub(crate) fn deserialize_teaching_field<'de, T, D>(
    deserializer: D,
    field: &'static str,
    accepted: &'static str,
    tolerate_stringified: bool,
) -> Result<T, D::Error>
where
    T: DeserializeOwned,
    D: Deserializer<'de>,
{
    let raw = serde_json::Value::deserialize(deserializer)
        .map_err(|error| D::Error::custom(format!("invalid JSON: {error}")))?;

    let parsed = match T::deserialize(&raw) {
        Ok(value) => Ok(value),
        Err(direct_error) => {
            if !tolerate_stringified {
                Err(direct_error)
            } else {
                // A stringified emission that parses as JSON but fails T is
                // MORE specific than the direct attempt's type error (the
                // string was JSON — its error names the inside problem), so
                // the stringified error wins.
                match raw.as_str().map(serde_json::from_str::<T>) {
                    Some(Ok(value)) => Ok(value),
                    Some(Err(stringified_error)) => Err(stringified_error),
                    None => Err(direct_error),
                }
            }
        }
    };

    parsed.map_err(|inner| {
        // Field-level errors (missing/unknown field inside a container the
        // model DID send) carry detail the shape description cannot — surface
        // them alongside the teaching message. Type errors are redundant with
        // the shape description. Position noise ("at line 1 column N") never
        // reaches the model.
        let inner_msg = inner.to_string();
        let inner_msg = inner_msg.split(" at line ").next().unwrap_or(&inner_msg);
        let detail =
            if inner_msg.starts_with("missing field") || inner_msg.starts_with("unknown field") {
                format!(" — {inner_msg}")
            } else {
                String::new()
            };
        D::Error::custom(format!(
            "{field}: received {}, expected {accepted}{detail}",
            describe_json_shape(&raw)
        ))
    })
}

/// One-line shape description for teaching errors: "null", "boolean true",
/// "number 5", "string \"abc\"", "an array", "an object".
fn describe_json_shape(raw: &serde_json::Value) -> String {
    match raw {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => format!("boolean {b}"),
        serde_json::Value::Number(n) => format!("number {n}"),
        serde_json::Value::String(s) => format!("string {s:?}"),
        serde_json::Value::Array(_) => "an array".to_string(),
        serde_json::Value::Object(_) => "an object".to_string(),
    }
}

/// Detect a path-filter pattern that provably matches no file, so the
/// "No matches found" it produces can be told apart from a genuine empty
/// search. Both the `grep` tool's `include_pattern` and `find_path`'s glob
/// are matched against full paths that start with a project root
/// directory, so a pattern whose first segment is a literal that names no
/// project root can only ever match zero files (observed live 3x
/// 2026-09-27/28: a docs grep, a call-site grep, and a registry-template
/// glob each returned "No matches found" while the targeted files existed,
/// and each was read as absence). Patterns with a wildcard in the first
/// segment are left to glob semantics; a correctly rooted pattern that
/// simply finds nothing stays clean — this must never fire on correct
/// output.
pub(crate) fn orphaned_path_pattern_note(pattern: &str, root_names: &[String]) -> Option<String> {
    let trimmed = pattern.trim();
    let first_segment = trimmed.split(['/', '\\']).next().unwrap_or(trimmed);
    if trimmed.is_empty()
        || first_segment.is_empty()
        || first_segment.contains(['*', '?', '[', ']'])
        || root_names.iter().any(|name| name == first_segment)
    {
        return None;
    }
    Some(format!(
        "the path pattern `{trimmed}` cannot match any file here: patterns are \
         matched against full paths that start with a project root directory \
         ({}), so a pattern starting with `{first_segment}` matches nothing. \
         Re-run with `**/{trimmed}`.",
        root_names.join(", "),
    ))
}

#[cfg(test)]
mod orphaned_path_pattern_tests {
    use super::orphaned_path_pattern_note;

    #[test]
    fn wrong_root_literal_prefix_is_flagged_with_the_corrected_form() {
        let note = orphaned_path_pattern_note("kask/docs/**", &["zed-kask".to_string()])
            .expect("wrong-root pattern is flagged");
        assert!(note.contains("cannot match any file"), "got: {note}");
        assert!(note.contains("`**/kask/docs/**`"), "got: {note}");
        assert!(note.contains("zed-kask"), "names the root: {note}");
    }

    #[test]
    fn root_name_prefix_is_not_flagged() {
        assert!(
            orphaned_path_pattern_note("zed-kask/kask/docs/**", &["zed-kask".to_string()])
                .is_none()
        );
    }

    #[test]
    fn wildcard_first_segment_is_never_flagged() {
        // Glob semantics for wildcard-leading patterns are the matcher's
        // business; the flag only covers the provable literal case.
        assert!(orphaned_path_pattern_note("**/*.rs", &["zed-kask".to_string()]).is_none());
        assert!(orphaned_path_pattern_note("*.rs", &["zed-kask".to_string()]).is_none());
    }

    #[test]
    fn any_matching_root_name_suppresses_the_flag() {
        // Multi-root projects: the pattern is fine under one of the roots.
        assert!(orphaned_path_pattern_note("b/**", &["a".to_string(), "b".to_string()]).is_none());
    }

    #[test]
    fn empty_pattern_is_never_flagged() {
        assert!(orphaned_path_pattern_note("", &["zed-kask".to_string()]).is_none());
        assert!(orphaned_path_pattern_note(" /x", &["zed-kask".to_string()]).is_none());
    }
}

pub use apply_code_action_tool::*;
pub use ask_user_tool::*;
pub use context_server_registry::*;
pub use copy_path_tool::*;
pub use create_directory_tool::*;
pub use create_thread_tool::*;
pub use curator_tools::*;
pub use delete_path_tool::*;
pub use diagnostics_tool::*;
pub use edit_file_tool::*;
pub use fetch_tool::*;
pub use find_path_tool::*;
pub use find_references_tool::*;
pub use get_code_actions_tool::*;
pub use go_to_definition_tool::*;
pub use grep_tool::*;
pub use lean_check_tool::*;
pub use lisp_eval_tool::*;
pub use list_agents_and_models_tool::*;
pub use list_directory_tool::*;
pub use list_mcp_tools_tool::*;
pub use move_path_tool::*;
pub use onto_anchor_tool::*;
pub use read_file_tool::*;
pub use record_skill_feedback_tool::*;
pub use rename_tool::*;
pub use render_template_tool::*;
pub use skill_tool::*;
pub use spawn_agent_tool::*;
pub use symbol_locator::*;

pub use terminal_tool::*;
pub use tool_permissions::*;
pub use web_search_tool::*;
pub use write_file_tool::*;

macro_rules! tools {
    ($($tool:ty),* $(,)?) => {
        /// Every built-in tool name, determined at compile time.
        pub const ALL_TOOL_NAMES: &[&str] = &[
            $(<$tool>::NAME,)*
        ];

        const _: () = {
            const fn str_eq(a: &str, b: &str) -> bool {
                let a = a.as_bytes();
                let b = b.as_bytes();
                if a.len() != b.len() {
                    return false;
                }
                let mut i = 0;
                while i < a.len() {
                    if a[i] != b[i] {
                        return false;
                    }
                    i += 1;
                }
                true
            }

            const NAMES: &[&str] = ALL_TOOL_NAMES;
            let mut i = 0;
            while i < NAMES.len() {
                let mut j = i + 1;
                while j < NAMES.len() {
                    if str_eq(NAMES[i], NAMES[j]) {
                        panic!("Duplicate tool name in tools! macro");
                    }
                    j += 1;
                }
                i += 1;
            }
        };

        /// Returns whether the tool with the given name supports the given provider.
        pub fn tool_supports_provider(name: &str, provider: &language_model::LanguageModelProviderId) -> bool {
            $(
                if name == <$tool>::NAME {
                    return <$tool>::supports_provider(provider);
                }
            )*
            false
        }

        /// Returns whether the tool with the given name may be provided to an
        /// agent in a restricted workspace. Unknown tools (e.g. MCP tools) are
        /// considered allowed.
        pub fn tool_allowed_in_restricted_mode(name: &str) -> bool {
            $(
                if name == <$tool>::NAME {
                    return <$tool>::allow_in_restricted_mode();
                }
            )*
            true
        }

        /// A list of all built-in tools
        pub fn built_in_tools() -> impl Iterator<Item = LanguageModelRequestTool> {
            fn language_model_tool<T: AgentTool>() -> LanguageModelRequestTool {
                let mut input_schema = T::input_schema().to_value();
                language_model::tool_schema::normalize_tool_schema(&mut input_schema);
                LanguageModelRequestTool::function(
                    T::NAME.to_string(),
                    T::description().to_string(),
                    input_schema,
                    T::supports_input_streaming(),
                )
            }
            [
                $(
                    language_model_tool::<$tool>(),
                )*
            ]
            .into_iter()
        }
    };
}

// Adding a tool here (and constructing it in `Thread::add_default_tools`) is
// not enough to make the model actually receive it. Three further gates will
// silently drop the tool rather than fail to compile:
//
// 1. `assets/settings/default.json`: the `write` and `ask` agent profiles each
//    carry an explicit `tools` allowlist. `Thread::enabled_tools` filters out
//    any tool not present there with value `true`, so it never reaches the
//    model.
// 2. `test_all_tools_are_in_tool_info_or_excluded` in
//    `crates/settings_ui/src/pages/tool_permissions_setup.rs`: every tool must
//    be in the permission-UI `TOOLS` list (if it calls
//    `decide_permission_from_settings`) or in `EXCLUDED_TOOLS`.
// 3. `tool_feature_flag_enabled`: some tools are gated behind a feature flag and
//    are dropped unless it is active. The agent-profile UI uses the same gate so
//    it never offers a tool the agent can't actually use.
tools! {
    ApplyCodeActionTool,
    AskUserTool,
    CopyPathTool,
    CreateDirectoryTool,
    CreateThreadTool,
    CuratorClearAlgedonicLogTool,
    CuratorDirectiveTool,
    CuratorStatusTool,
    DeletePathTool,
    DiagnosticsTool,
    EditFileTool,
    FetchTool,
    FindPathTool,
    FindReferencesTool,
    GetCodeActionsTool,
    GoToDefinitionTool,
    GrepTool,
    LeanCheckTool,
    ListAgentsAndModelsTool,
    ListDirectoryTool,
    ListMcpToolsTool,
    LispEvalTool,
    MovePathTool,
    OntoAnchorTool,
    ReadFileTool,
    RecordSkillFeedbackTool,
    RenameTool,
    SkillTool,
    SpawnAgentTool,
    RenderTemplateTool,
    TerminalTool,
    WebSearchTool,
    WriteFileTool,
}

/// Some built-in tools are gated behind a feature flag and only become usable
/// once that flag is active. Tools without a flag are always available.
///
/// This is the single source of truth for that gating: `Thread::enabled_tools`
/// uses it to decide what the model receives, and the agent-profile
/// configuration UI uses it to decide what to offer — so the UI can never list
/// a tool the agent would silently drop (see #56778).
pub fn tool_feature_flag_enabled(tool_name: &str, cx: &App) -> bool {
    match tool_name {
        RenameTool::NAME => cx.has_flag::<RenameToolFeatureFlag>(),
        FindReferencesTool::NAME
        | GetCodeActionsTool::NAME
        | ApplyCodeActionTool::NAME
        | GoToDefinitionTool::NAME => cx.has_flag::<LspToolFeatureFlag>(),
        CreateThreadTool::NAME => cx.has_flag::<CreateThreadToolFeatureFlag>(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_tool_schemas_are_normalized() {
        let tools = built_in_tools().collect::<Vec<_>>();

        assert_eq!(tools.len(), ALL_TOOL_NAMES.len());
        for tool in tools {
            let language_model::LanguageModelRequestToolInput::Function { input_schema, .. } =
                tool.input
            else {
                panic!("built-in tool `{}` should use a JSON schema", tool.name);
            };
            assert_eq!(input_schema.get("$schema"), None, "tool `{}`", tool.name);
            assert_eq!(input_schema.get("title"), None, "tool `{}`", tool.name);
            assert_eq!(
                input_schema.get("description"),
                None,
                "tool `{}`",
                tool.name
            );
            assert!(
                input_schema["properties"].is_object(),
                "tool `{}` should have object properties",
                tool.name
            );
        }
    }

    #[test]
    fn fetch_and_terminal_are_forbidden_in_restricted_mode() {
        assert!(!tool_allowed_in_restricted_mode(FetchTool::NAME));
        assert!(!tool_allowed_in_restricted_mode(TerminalTool::NAME));
        assert!(!tool_allowed_in_restricted_mode(LeanCheckTool::NAME));

        // Every other built-in tool, and unknown (e.g. MCP) tools, are allowed.
        for name in ALL_TOOL_NAMES {
            let expected = *name != FetchTool::NAME
                && *name != TerminalTool::NAME
                && *name != LeanCheckTool::NAME;
            assert_eq!(
                tool_allowed_in_restricted_mode(name),
                expected,
                "unexpected restricted-mode policy for tool `{name}`"
            );
        }
        assert!(tool_allowed_in_restricted_mode("some_mcp_tool"));
    }

    /// Gate 1 of the three silent-drop gates (see the comment above the
    /// `tools!` invocation): the shipped `write` profile — the default
    /// profile — carries an explicit `tools` allowlist, and
    /// `Thread::enabled_tools` drops any tool absent from it. A registered
    /// tool missing from the list never reaches the model, with no compile
    /// error (the defect: `record_skill_feedback` shipped registered
    /// but unlisted, so the operator's direct rating channel was invisible
    /// in every profile). Presence is the invariant — a deliberate disable
    /// must be listed as `false` (the `ask_user` pattern), never omitted.
    #[test]
    fn every_built_in_tool_is_listed_in_the_default_write_profile() {
        let default_json = include_str!("../../../assets/settings/default.json");
        let value: serde_json_lenient::Value =
            serde_json_lenient::from_str(default_json).expect("default.json must parse");
        let write_tools = value
            .get("agent")
            .expect("default.json should have 'agent' key")
            .get("profiles")
            .expect("agent should have 'profiles'")
            .get("write")
            .expect("profiles should have 'write'")
            .get("tools")
            .expect("the write profile should have 'tools'");
        for name in ALL_TOOL_NAMES {
            assert!(
                write_tools.get(*name).is_some(),
                "built-in tool `{name}` is missing from the `write` profile's tools \
                 allowlist in assets/settings/default.json — `Thread::enabled_tools` \
                 silently drops it, so the model never sees it. Add it (true, or \
                 false for a deliberate disable)."
            );
        }
    }

    #[test]
    fn curator_status_is_enabled_in_both_shipped_agent_profiles() {
        let default_json = include_str!("../../../assets/settings/default.json");
        let value: serde_json_lenient::Value =
            serde_json_lenient::from_str(default_json).expect("default.json must parse");
        let profiles = &value["agent"]["profiles"];
        for name in ["write", "ask"] {
            assert_eq!(
                profiles[name]["tools"]["curator_status"].as_bool(),
                Some(true),
                "{name} must expose the same read-only curator_status tool"
            );
        }
    }

    #[test]
    fn test_terminal_tool_input_accepts_string_timeout_ms() {
        // Models (especially GLM-class) sometimes emit `timeout_ms` as a string.
        // The deserialize_with attribute must coerce it so both live run and
        // replay succeed without wasting a model turn on a retry.
        let input = serde_json::json!({
            "command": "echo hi",
            "cd": ".",
            "timeout_ms": "300000"
        });
        let parsed: TerminalToolInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.timeout_ms, Some(300000));

        // Integer form still works.
        let input = serde_json::json!({
            "command": "echo hi",
            "cd": ".",
            "timeout_ms": 300000
        });
        let parsed: TerminalToolInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.timeout_ms, Some(300000));

        // Omitted timeout_ms defaults to None.
        let input = serde_json::json!({
            "command": "echo hi",
            "cd": "."
        });
        let parsed: TerminalToolInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.timeout_ms, None);
    }
}
