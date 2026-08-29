use anyhow::Result;
use gpui::SharedString;
use handlebars::Handlebars;
use rust_embed::RustEmbed;
use serde::Serialize;
use std::sync::Arc;

#[derive(RustEmbed)]
#[folder = "src/templates"]
#[include = "*.hbs"]
struct Assets;

pub struct Templates(Handlebars<'static>);

impl Templates {
    pub fn new() -> Arc<Self> {
        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(true);
        handlebars.register_helper("contains", Box::new(contains));
        handlebars.register_embed_templates::<Assets>().unwrap();
        Arc::new(Self(handlebars))
    }
}

pub trait Template: Sized {
    const TEMPLATE_NAME: &'static str;

    fn render(&self, templates: &Templates) -> Result<String>
    where
        Self: Serialize + Sized,
    {
        Ok(templates.0.render(Self::TEMPLATE_NAME, self)?)
    }
}

#[derive(Serialize)]
pub struct SystemPromptTemplate<'a> {
    #[serde(flatten)]
    pub project: &'a prompt_store::ProjectContext,
    pub available_tools: Vec<SharedString>,
    pub model_name: Option<String>,
    pub date: String,
    /// Contents of the user-global `~/.config/zed/AGENTS.md` file (or the
    /// platform equivalent), if present and non-empty.
    pub user_agents_md: Option<SharedString>,
    /// Agent static context (e.g., Curator overlay, Steer panel overlay).
    /// Rendered in the system prompt's `## Session Context` section.
    /// `None` when no agent overlay is set.
    pub static_context: Option<SharedString>,
    /// Whether agent-run terminal commands are wrapped in an OS-level
    /// sandbox for this thread. When `true` — and the `terminal` tool is
    /// in `available_tools` — the rendered prompt describes the sandbox's
    /// read/write/network rules and the per-command flags the model can
    /// request to relax them. Otherwise the prompt omits the sandbox
    /// section entirely.
    pub sandboxing: bool,
    /// Whether the host is Linux. The writable-temp story differs by
    /// platform (Linux exposes an ephemeral `tmpfs` over `/tmp`; other
    /// platforms provide a persistent per-thread `$TMPDIR`), so the sandbox
    /// section describes the right one rather than advertising a `$TMPDIR`
    /// that doesn't behave as stated.
    pub is_linux: bool,
    /// Whether sandboxed terminal commands run through WSL on Windows.
    pub is_windows: bool,
}

impl Template for SystemPromptTemplate<'_> {
    const TEMPLATE_NAME: &'static str = "system_prompt.hbs";
}

/// Handlebars helper for checking if an item is in a list
fn contains(
    h: &handlebars::Helper,
    _: &handlebars::Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> handlebars::HelperResult {
    let list = h
        .param(0)
        .and_then(|v| v.value().as_array())
        .ok_or_else(|| {
            handlebars::RenderError::new("contains: missing or invalid list parameter")
        })?;
    let query = h.param(1).map(|v| v.value()).ok_or_else(|| {
        handlebars::RenderError::new("contains: missing or invalid query parameter")
    })?;

    if list.contains(query) {
        out.write("true")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_template() {
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();
        assert!(rendered.contains("You are the Zed coding agent"));
        assert!(rendered.contains("Today's Date: 2026-01-01"));
        assert!(rendered.contains("## Fixing Diagnostics"));
        assert!(rendered.contains("test-model"));
    }

    #[test]
    fn test_system_prompt_renders_session_context_without_rules_or_agents_md() {
        // Regression: the `static_context` (Session Context) block was nested
        // inside `{{#if (or user_agents_md has_rules)}}`, so it was silently
        // dropped for projects with no `.rules` and no personal `AGENTS.md` —
        // which dropped the Curator overlay's `CURATOR_STATIC_CONTEXT`. It must
        // render independently of that guard.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: Some("CURATOR-CTX".to_string().into()),
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();
        assert!(
            rendered.contains("## Session Context"),
            "Session Context heading must render even without rules/AGENTS.md"
        );
        assert!(
            rendered.contains("CURATOR-CTX"),
            "static_context body must render even without rules/AGENTS.md"
        );
    }

    #[test]
    fn test_system_prompt_contains_tool_failure_mode_warnings() {
        // The tool warnings were moved from `inject_static_context` (runtime
        // injection) into the template itself. They must render unconditionally
        // — no guard, no injector dependency.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();
        assert!(
            rendered.contains("## Tool failure-mode warnings (kask)"),
            "tool warnings heading must render unconditionally in the template"
        );
        assert!(
            rendered.contains("read_file"),
            "read_file failure-mode guidance must be present"
        );
        assert!(
            rendered.contains("edit_file"),
            "edit_file failure-mode guidance must be present"
        );
        assert!(
            rendered.contains("terminal"),
            "terminal failure-mode guidance must be present"
        );
    }

    #[test]
    fn test_system_prompt_contains_loop_termination_guardrail() {
        // Pins the zed-kask-only loop-budget guardrail added to `## Task
        // Execution` (a divergence in a shared upstream section) so an upstream
        // merge that drops it is caught.
        //
        // The threshold must stay a concrete count, not a vague quantifier:
        // "several iterations" left the stop point to model discretion, so
        // different models bounded the loop at different depths.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: None,
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let rendered = template.render(&Templates::new()).unwrap();
        assert!(
            rendered.contains("If a tool loop repeats without measurable progress"),
            "loop-termination guardrail must be present in the rendered prompt"
        );
        assert!(
            rendered.contains("three times"),
            "the loop guardrail must state a concrete iteration count, not a \
             vague quantifier the model has to interpret"
        );
    }

    #[test]
    fn test_system_prompt_contains_division_of_responsibilities() {
        // Pins the four-moves interaction loop (functional-interaction-spec.md,
        // Phase A) so an upstream merge that drops it is caught. The section
        // must carry the four moves and the authority boundary: the user
        // decides functional questions, the agent interprets — never revises —
        // the functional requirement.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: None,
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let rendered = template.render(&Templates::new()).unwrap();
        assert!(
            rendered.contains("## Division of Responsibilities (kask)"),
            "the division section must be present in the rendered prompt"
        );
        for move_phrase in [
            "Point at the same target",
            "Bring choices to the user as experiences",
            "Report outcomes, not artifacts",
            "Bank the learning",
        ] {
            assert!(
                rendered.contains(move_phrase),
                "all four moves must be present; missing: {move_phrase}"
            );
        }
        assert!(
            rendered.contains("The user decides; you implement"),
            "the authority boundary must be explicit: functional decisions are the user's"
        );
        assert!(
            rendered.contains("to interpret it, not to revise it"),
            "the agent interprets the functional requirement; it never revises it"
        );
    }

    #[test]
    fn test_system_prompt_autonomy_bullet_includes_experience_trigger() {
        // Pins the amended autonomy bullet in `## Task Execution`: asking is
        // triggered not only by missing information or risk but by choices
        // that change what the user will experience. Without this carve-out
        // the bullet licenses unilateral functional decisions (the agent has
        // the information; the question is authority, not information).
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: None,
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let rendered = template.render(&Templates::new()).unwrap();
        assert!(
            rendered.contains("a choice changes what the user will experience"),
            "the autonomy bullet must name the experience-changing-choice ask trigger"
        );
    }

    #[test]
    fn test_system_prompt_final_message_leads_with_functional_outcome() {
        // Pins the functional-first Final Message bullet: reports lead with
        // what the user can now do (or no longer experiences as broken)
        // before technical detail. The prior bullet alone rehearsed a
        // file-list summary format every turn.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: None,
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let rendered = template.render(&Templates::new()).unwrap();
        assert!(
            rendered.contains("lead with the functional outcome"),
            "the Final Message section must require the functional outcome first"
        );
        assert!(
            !rendered.contains("briefly summarize what changed, reference the relevant files, and state what validation you ran"),
            "the old file-list-first bullet must not coexist with the functional-first \
             bullet — two \"what comes first\" instructions resolve unpredictably (§7.3)"
        );
    }

    #[test]
    fn test_system_prompt_vagueness_resolved_with_user() {
        // Pins the amended `## Ambition vs. Precision` bullet: scope vagueness
        // is resolved WITH the user, not filled with initiative. The upstream
        // text ("creative touches when scope is vague") directly contradicted
        // the Division of Responsibilities — scope-vague is the four-moves'
        // ask-trigger, so the contradiction is asserted absent as well.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: None,
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let rendered = template.render(&Templates::new()).unwrap();
        assert!(
            rendered.contains("resolve the vagueness with the user"),
            "vague scope must route to the user, not to initiative"
        );
        assert!(
            !rendered.contains("creative touches when scope is vague"),
            "the upstream phrase licenses unilateral product decisions in \
             exactly the situation where the functional requirement matters most"
        );
    }

    #[test]
    fn test_system_prompt_autonomy_bullet_reframes_prematurely() {
        // Pins the amended autonomy bullet: the anti-pattern is asking for
        // findable information — not asking per se. "Prematurely" framed
        // user-contact as cost and biased against interpretation rounds and
        // choice-surfacing.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: None,
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let rendered = template.render(&Templates::new()).unwrap();
        assert!(
            rendered.contains("for information you can find yourself"),
            "the autonomy bullet must name findable-information as the anti-pattern, not asking per se"
        );
        assert!(
            !rendered.contains("coming back to the user prematurely"),
            "\"prematurely\" biases against the Division's ask-triggers"
        );
    }

    #[test]
    fn test_system_prompt_mermaid_list_uses_renderer_directives() {
        // Supersedes an earlier test that asserted `kanban` was NOT a mermaid
        // type. That was wrong: `kanban` is in the renderer's allowlist
        // (`markdown::mermaid::SUPPORTED_PREFIXES`) and
        // `test_beta_suffixed_diagram_types_are_extracted` proves merman
        // extracts it. `kanban` is BOTH a mermaid directive and, separately, a
        // D18 fenced-block widget tag — the prompt must distinguish the two
        // rather than deny the mermaid form.
        //
        // The prompt must also name the `-beta` directives merman actually
        // requires; advertising bare `sankey`/`xychart` produced diagrams the
        // renderer silently dropped. The exhaustive prompt-vs-allowlist check
        // lives in `markdown`, next to the constant
        // (`test_system_prompt_advertises_every_supported_diagram_type`).
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: None,
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let rendered = template.render(&Templates::new()).unwrap();
        for directive in [
            "sankey-beta",
            "xychart-beta",
            "architecture-beta",
            "radar-beta",
        ] {
            assert!(
                rendered.contains(directive),
                "prompt must advertise the `{directive}` directive merman requires"
            );
        }
        assert!(
            rendered.contains("kanban"),
            "`kanban` is a supported mermaid directive and must be advertised"
        );
        assert!(
            rendered.contains("kask viz widgets, not mermaid"),
            "D18 widget blocks must be distinguished from mermaid diagrams"
        );
    }

    #[test]
    fn test_system_prompt_renders_user_agents_md_before_project_rules() {
        use prompt_store::{ProjectContext, RulesFileContext, WorktreeContext};
        use util::rel_path::RelPath;

        let worktrees = vec![WorktreeContext {
            root_name: "my-project".to_string(),
            abs_path: std::path::Path::new("/tmp/my-project").into(),
            rules_file: Some(RulesFileContext {
                path_in_worktree: RelPath::from_unix_str("AGENTS.md").unwrap().into(),
                text: "project-specific guidance".to_string(),
                frontmatter: None,
                project_entry_id: 1,
            }),
        }];
        let project = ProjectContext::new(worktrees);
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: Some("always be concise".into()),
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(rendered.contains("### Personal `AGENTS.md`"));
        assert!(rendered.contains("always be concise"));
        assert!(rendered.contains("### Project Rules"));
        assert!(rendered.contains("project-specific guidance"));

        let personal_idx = rendered.find("### Personal `AGENTS.md`").unwrap();
        let project_idx = rendered.find("### Project Rules").unwrap();
        assert!(
            personal_idx < project_idx,
            "personal AGENTS.md should render before project rules so project rules can override it"
        );
    }

    #[test]
    fn test_system_prompt_omits_sandbox_section_when_sandboxing_disabled() {
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();
        assert!(!rendered.contains("## Terminal sandbox"));
        assert!(!rendered.contains("allow_hosts"));
    }

    #[test]
    fn test_system_prompt_renders_sandbox_section_with_worktrees_when_enabled() {
        use prompt_store::{ProjectContext, WorktreeContext};

        let worktrees = vec![
            WorktreeContext {
                root_name: "alpha".to_string(),
                abs_path: std::path::Path::new("/tmp/alpha").into(),
                rules_file: None,
            },
            WorktreeContext {
                root_name: "beta".to_string(),
                abs_path: std::path::Path::new("/tmp/beta").into(),
                rules_file: None,
            },
        ];
        let project = ProjectContext::new(worktrees);
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into(), "terminal".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: true,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(rendered.contains("## Terminal sandbox"));
        assert!(rendered.contains("`/tmp/alpha`"));
        assert!(rendered.contains("`/tmp/beta`"));
        assert!(rendered.contains("allow_hosts"));
        assert!(rendered.contains("allow_all_hosts: true"));
        assert!(rendered.contains("fs_write_paths"));
        assert!(rendered.contains("allow_fs_write_all: true"));
        assert!(rendered.contains("unsandboxed: true"));
        assert!(rendered.contains("`.git` directories remain protected"));
        assert!(rendered.contains("Git metadata writes are never grantable inside the sandbox"));
        assert!(rendered.contains("request `unsandboxed: true` with a reason"));
        assert!(rendered.contains("git --no-optional-locks status"));
        assert!(rendered.contains("for the rest of the thread"));
        // macOS tolerates granting a not-yet-existing path, so the
        // existing-directory requirement must not be stated there; the
        // `create_directory` flow is the preferred guidance instead.
        assert!(!rendered.contains("Each path must be an existing directory"));
        assert!(rendered.contains("first create it with the `create_directory` tool"));
    }

    #[test]
    fn test_system_prompt_linux_sandbox_section_omits_tmpdir() {
        use prompt_store::{ProjectContext, WorktreeContext};

        let worktrees = vec![WorktreeContext {
            root_name: "alpha".to_string(),
            abs_path: std::path::Path::new("/tmp/alpha").into(),
            rules_file: None,
        }];
        let project = ProjectContext::new(worktrees);
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into(), "terminal".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: true,
            is_linux: true,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(rendered.contains("## Terminal sandbox"));
        // On Linux we must not advertise the special persistent `$TMPDIR`.
        assert!(!rendered.contains("$TMPDIR"));
        assert!(rendered.contains("`/tmp` is writable"));
        assert!(rendered.contains("`/tmp/alpha`"));
        // Linux write grants must already exist (bwrap binds existing paths).
        assert!(rendered.contains("Each path must be an existing directory"));
        assert!(rendered.contains("first create it with the `create_directory` tool"));
    }

    #[test]
    fn test_system_prompt_windows_sandbox_section_rejects_host_specific_network() {
        use prompt_store::{ProjectContext, WorktreeContext};

        let worktrees = vec![WorktreeContext {
            root_name: "alpha".to_string(),
            abs_path: std::path::Path::new("C:/Users/me/project").into(),
            rules_file: None,
        }];
        let project = ProjectContext::new(worktrees);
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into(), "terminal".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: true,
            is_linux: false,
            is_windows: true,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(rendered.contains("commands run inside WSL under Bubblewrap"));
        assert!(rendered.contains("Protected Git metadata remains read-only"));
        assert!(rendered.contains("do not use this on Windows"));
        assert!(rendered.contains("such requests are rejected"));
        assert!(rendered.contains("allow_all_hosts: true"));
        assert!(rendered.contains("git --no-optional-locks status"));
        // Out-of-project `create_directory` grants aren't supported on Windows,
        // so the prompt must not recommend that flow; it suggests granting the
        // nearest existing parent instead.
        assert!(rendered.contains("Each path must be an existing directory"));
        assert!(rendered.contains("nearest existing parent directory"));
        assert!(!rendered.contains("first create it with the `create_directory` tool"));
    }

    #[test]
    fn test_system_prompt_sandbox_section_handles_zero_worktrees() {
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into(), "terminal".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: true,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(rendered.contains("## Terminal sandbox"));
        assert!(rendered.contains("No project directories are currently writable"));
    }

    #[test]
    fn test_system_prompt_omits_sandbox_section_when_terminal_tool_unavailable() {
        // A profile can disable the terminal tool entirely; the prompt must not
        // describe a sandboxed `terminal` tool the model doesn't have.
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: true,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(!rendered.contains("## Terminal sandbox"));
        assert!(!rendered.contains("allow_hosts"));
    }

    #[test]
    fn test_system_prompt_omits_user_agents_md_section_when_absent() {
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();
        assert!(!rendered.contains("### Personal `AGENTS.md`"));
    }

    #[test]
    fn test_system_prompt_does_not_render_legacy_zed_rules_section() {
        let project = prompt_store::ProjectContext::default();
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["echo".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(!rendered.contains("The user has specified the following rules"));
        assert!(!rendered.contains("Rules title:"));
    }

    // The Agent Skills section follows upstream Zed: skills are markdown
    // bodies the model retrieves and follows. This test pins the upstream
    // wording so the D1 manifest-cascade text cannot silently regress.
    #[test]
    fn test_system_prompt_skills_section_describes_body_retrieval() {
        use agent_skills::SkillSummary;
        use prompt_store::ProjectContext;

        let summary = SkillSummary {
            name: "skill-maintenance".to_string(),
            description: "Skill lifecycle management.".to_string(),
            location: "/skills/skill-maintenance/SKILL.md".to_string(),
        };
        let project = ProjectContext::new(vec![]).with_skills(vec![summary]);
        let template = SystemPromptTemplate {
            project: &project,
            available_tools: vec!["skill".into()],
            model_name: Some("test-model".to_string()),
            date: "2026-01-01".to_string(),
            user_agents_md: None,
            static_context: None,
            sandboxing: false,
            is_linux: false,
            is_windows: false,
        };
        let templates = Templates::new();
        let rendered = template.render(&templates).unwrap();

        assert!(
            rendered.contains("use the `skill` tool to retrieve the full instructions"),
            "skills section must describe body retrieval"
        );
        assert!(
            rendered.contains(
                "If the Skill references additional files, use `read_file` to access them"
            ),
            "skills section must instruct reading additional files"
        );

        // The D1 manifest-cascade wording must be gone.
        assert!(
            !rendered.contains("PDCA (Plan-Do-Check-Act) cascade of Jinja2 templates"),
            "D1 manifest-cascade phrasing must be removed"
        );
        assert!(
            !rendered.contains("gas/rjoule budgets"),
            "D1 gas/rjoule mention must be removed"
        );
        assert!(
            !rendered.contains("discovery-only catalog entry"),
            "D1 discovery-only label must be removed"
        );

        // The catalog itself is still rendered.
        assert!(
            rendered.contains("<name>skill-maintenance</name>"),
            "available_skills catalog must still list the skill by name"
        );
    }

    use crate::curator_agent_server::CURATOR_STATIC_CONTEXT;

    // D2 standing obligation: an overlay that advertises tool names must pin
    // them against the tools' actual NAME constants — a rename would degrade
    // to "tool not found" at dispatch. Every `\`curator_*\` tool` mention in
    // CURATOR_STATIC_CONTEXT must be a registered Curator tool's NAME.
    #[test]
    fn test_curator_overlay_advertises_only_registered_tool_names() {
        use crate::thread::AgentTool;
        use crate::tools::{CuratorClearAlgedonicLogTool, CuratorDirectiveTool, CuratorStatusTool};
        const NAMES: &[&str] = &[
            <CuratorStatusTool as AgentTool>::NAME,
            <CuratorDirectiveTool as AgentTool>::NAME,
            <CuratorClearAlgedonicLogTool as AgentTool>::NAME,
        ];

        // Extract every backtick token from the overlay that looks like a
        // curator tool reference (`curator_...`).
        let mut advertised: Vec<&str> = Vec::new();
        for segment in CURATOR_STATIC_CONTEXT.split('`') {
            let token = segment.trim();
            if token.starts_with("curator_")
                && token.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            {
                advertised.push(token);
            }
        }
        assert!(
            !advertised.is_empty(),
            "CURATOR_STATIC_CONTEXT must advertise at least one curator tool"
        );

        for token in advertised {
            assert!(
                NAMES.contains(&token),
                "CURATOR_STATIC_CONTEXT advertises `{token}` but no Curator tool \
                 registers that NAME — update the overlay or the tool together"
            );
        }
    }
}
