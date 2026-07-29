//! Per-tab system-prompt rendering via Jinja2 templates.
//!
//! Replaces the v0 `build_system_prompt` Rust `format!` string with
//! proper Jinja2 templates matching the kask skill cascade pattern. The
//! templates live in `kask/registry/panel-prompts/` and are embedded at
//! build time via `include_str!` so they work regardless of CWD or
//! install location.
//!
//! ## Templates
//!
//! - `panel-tab-system.j2` — the per-tab framing (parameterized by server).
//! - `panel-curator-guidance.j2` — the shared curator guidance (appended
//!   to every tab's system prompt via the `curator_guidance` variable).
//!
//! ## Context
//!
//! The templates receive `{{ server }}`, `{{ server_description }}`,
//! `{{ tools }}` (list of `{name, description}`), `{{ task }}` (the
//! user's current request, per the `.rules` "Skill cascade context must
//! carry the user's task" trap), and `{{ curator_guidance }}` (the
//! rendered shared include).

use std::collections::HashMap;

use minijinja::Environment;
use serde_json::Value;

/// The per-tab system prompt template (embedded at build time).
const TAB_SYSTEM_TEMPLATE: &str =
    include_str!("../../../kask/registry/panel-prompts/panel-tab-system.j2");

/// The shared curator guidance template (embedded at build time).
const CURATOR_GUIDANCE_TEMPLATE: &str =
    include_str!("../../../kask/registry/panel-prompts/panel-curator-guidance.j2");

/// Render the shared curator guidance include.
///
/// This is rendered once and injected as `curator_guidance` into every
/// tab's system prompt. It frames the curator's cross-tab role.
pub fn render_curator_guidance() -> String {
    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
    env.render_str(CURATOR_GUIDANCE_TEMPLATE, ())
        .expect("curator guidance template must render with empty context")
        .trim()
        .to_string()
}

/// Render the per-tab system prompt.
///
/// `server` is the MCP server ID (e.g. "curator", "codegraph").
/// `server_description` is the human-readable description.
/// `tools` is a list of `ToolDescriptor` (name + description).
/// `task` is the user's current request (may be empty for the first turn).
pub fn render_tab_system_prompt(
    server: &str,
    server_description: &str,
    tools: &[crate::ToolDescriptor],
    task: &str,
) -> String {
    let curator_guidance = render_curator_guidance();

    let tools_json: Vec<Value> = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
            })
        })
        .collect();

    let mut context = HashMap::new();
    context.insert("server".to_string(), Value::String(server.to_string()));
    context.insert(
        "server_description".to_string(),
        Value::String(server_description.to_string()),
    );
    context.insert("tools".to_string(), Value::Array(tools_json));
    context.insert("task".to_string(), Value::String(task.to_string()));
    context.insert(
        "curator_guidance".to_string(),
        Value::String(curator_guidance),
    );

    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
    env.render_str(TAB_SYSTEM_TEMPLATE, &context)
        .expect("tab system prompt template must render")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tools() -> Vec<crate::ToolDescriptor> {
        vec![
            crate::ToolDescriptor {
                name: "regulation_status".to_string(),
                description: "Fetch regulation status".to_string(),
            },
            crate::ToolDescriptor {
                name: "raise_issue".to_string(),
                description: "Raise an algedonic issue".to_string(),
            },
        ]
    }

    #[test]
    fn renders_server_name_and_description() {
        let prompt = render_tab_system_prompt("curator", "regulation cascade", &[], "");
        assert!(prompt.contains("curator"));
        assert!(prompt.contains("regulation cascade"));
    }

    #[test]
    fn renders_tool_list() {
        let prompt = render_tab_system_prompt("curator", "test", &sample_tools(), "");
        assert!(prompt.contains("/regulation_status"));
        assert!(prompt.contains("Fetch regulation status"));
        assert!(prompt.contains("/raise_issue"));
        assert!(prompt.contains("Raise an algedonic issue"));
    }

    #[test]
    fn notes_no_tools_when_empty() {
        let prompt = render_tab_system_prompt("codegraph", "code query", &[], "");
        assert!(prompt.contains("no tools discovered"));
    }

    #[test]
    fn includes_curator_guidance() {
        let prompt = render_tab_system_prompt("curator", "test", &[], "");
        assert!(prompt.contains("Curator guidance"));
        assert!(prompt.contains("Remember"));
    }

    #[test]
    fn includes_task_when_provided() {
        let prompt = render_tab_system_prompt("curator", "test", &[], "fix the bug");
        assert!(prompt.contains("Current task"));
        assert!(prompt.contains("fix the bug"));
    }

    #[test]
    fn omits_task_section_when_empty() {
        let prompt = render_tab_system_prompt("curator", "test", &[], "");
        assert!(!prompt.contains("Current task"));
    }

    #[test]
    fn curator_guidance_renders() {
        let guidance = render_curator_guidance();
        assert!(guidance.contains("Curator guidance"));
        assert!(guidance.contains("episodic"));
        assert!(guidance.contains("semantic"));
    }
}
