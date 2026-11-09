use agent_skills::{
    SKILL_FILE_NAME, SkillMetadata, SkillsUpdatedHook, global_skills_dir, is_reserved_skill_name,
    project_skills_relative_path, validate_description, validate_name,
};
use anyhow::{Context as _, Result, anyhow};
use editor::{CurrentLineHighlight, Editor, EditorElement, EditorEvent, EditorStyle};
use fs::Fs;
use gpui::{
    App, AsyncApp, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle, Subscription, Task,
    TextStyle, WeakEntity, WindowHandle, actions,
};
use language::{Buffer, language_settings::SoftWrap};
use project::{Project, Worktree};
use settings::{ActionSequence, Settings};
use std::path::PathBuf;
use std::sync::Arc;
use theme_settings::ThemeSettings;
use ui::{Banner, Divider, SwitchField, WithScrollbar, prelude::*};
use ui_input::{ErasedEditorEvent, InputField};
use util::ResultExt;
use util::rel_path::RelPath;
use workspace::MultiWorkspace;

use crate::{SettingsUiFile, SettingsWindow, all_projects};

actions!(
    skill_creator,
    [SaveSkill, Cancel, FocusNextField, FocusPreviousField,]
);

const NAME_FIELD_TAB_INDEX: isize = 2;
const DESCRIPTION_FIELD_TAB_INDEX: isize = 3;
const DISABLE_MODEL_INVOCATION_TAB_INDEX: isize = 4;
const BODY_FIELD_TAB_INDEX: isize = 5;
const SAVE_BUTTON_TAB_INDEX: isize = 6;

#[derive(Clone, Debug, Default)]
pub enum SkillCreatorOpenMode {
    #[default]
    Form,
    Install {
        content: String,
    },
}

pub(crate) enum SkillCreatorEvent {
    Dismissed,
    Saved,
}

#[derive(Clone, Debug, PartialEq)]
enum ScopeChoice {
    Global,
    Project {
        project: Entity<Project>,
        worktree: Entity<Worktree>,
    },
}

fn scope_for_settings_file(
    current_file: &SettingsUiFile,
    original_window: Option<&WindowHandle<MultiWorkspace>>,
    cx: &App,
) -> ScopeChoice {
    if let SettingsUiFile::Project((worktree_id, _)) = current_file {
        for project in all_projects(original_window, cx) {
            if let Some(worktree) = project.read(cx).worktree_for_id(*worktree_id, cx) {
                return ScopeChoice::Project { project, worktree };
            }
        }
    }
    ScopeChoice::Global
}

/// Renders the skill creator sub-page pushed by
/// [`SettingsWindow::open_skill_creator_sub_page`].
pub(crate) fn render_skill_creator_page(
    settings_window: &SettingsWindow,
    _scroll_handle: &ScrollHandle,
    _window: &mut Window,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let Some(page) = settings_window.skill_creator_page() else {
        return gpui::Empty.into_any_element();
    };
    page.into_any_element()
}

pub struct SkillCreatorPage {
    focus_handle: FocusHandle,
    fs: Arc<dyn Fs>,
    name_editor: Entity<InputField>,
    description_editor: Entity<InputField>,
    body_editor: Entity<Editor>,
    description_length: usize,
    settings_window: WeakEntity<SettingsWindow>,
    disable_model_invocation: bool,
    name_error: Option<&'static str>,
    description_error: Option<&'static str>,
    body_error: Option<&'static str>,
    save_error: Option<SharedString>,
    saving: bool,
    save_task: Option<Task<()>>,
    scroll_handle: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SkillCreatorEvent> for SkillCreatorPage {}

impl SkillCreatorPage {
    pub(crate) fn new(
        settings_window: WeakEntity<SettingsWindow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let app_state = workspace::AppState::global(cx);
        let fs = app_state.fs.clone();
        let language_registry = app_state.languages.clone();

        let focus_handle = cx.focus_handle();

        let name_editor = cx.new(|cx| {
            InputField::new(window, cx, "my-new-skill")
                .label("Name")
                .tab_index(NAME_FIELD_TAB_INDEX)
                .tab_stop(true)
        });
        // Focus the name field on open.
        window.focus(&name_editor.focus_handle(cx), cx);

        let description_editor = cx.new(|cx| {
            InputField::new(
                window,
                cx,
                "e.g., Fill the PR description following this template.",
            )
            .label("Description")
            .tab_index(DESCRIPTION_FIELD_TAB_INDEX)
            .tab_stop(true)
        });

        let body_editor = cx.new(|cx| {
            let buffer = cx.new(|cx| {
                let buffer = Buffer::local(String::new(), cx);
                buffer.set_language_registry(language_registry.clone());
                buffer
            });
            let mut editor = Editor::for_buffer(buffer, None, window, cx);
            editor.set_placeholder_text("Add skill content…", window, cx);
            editor.set_soft_wrap_mode(SoftWrap::EditorWidth, cx);
            editor.set_show_gutter(false, cx);
            editor.set_show_wrap_guides(false, cx);
            editor.set_show_indent_guides(false, cx);
            editor.set_use_modal_editing(true);
            editor.set_current_line_highlight(Some(CurrentLineHighlight::None));
            editor
        });

        cx.spawn_in(window, {
            let body_editor = body_editor.downgrade();
            let language_registry = language_registry.clone();
            async move |_this, cx| {
                let markdown = language_registry.language_for_name("Markdown").await.ok();
                if let Some(markdown) = markdown {
                    body_editor
                        .update(cx, |editor, cx| {
                            editor.buffer().update(cx, |multi_buffer, cx| {
                                if let Some(buffer) = multi_buffer.as_singleton() {
                                    buffer.update(cx, |buffer, cx| {
                                        buffer.set_language(Some(markdown), cx)
                                    });
                                }
                            });
                        })
                        .ok();
                }
            }
        })
        .detach();

        let name_input_editor = name_editor.read(cx).editor().clone();
        let description_input_editor = description_editor.read(cx).editor().clone();
        let weak = cx.weak_entity();
        let name_subscription = name_input_editor.subscribe(
            Box::new(move |event, window, cx| {
                weak.update(cx, |this, cx| {
                    this.handle_name_input_event(&event, window, cx);
                })
                .ok();
            }),
            window,
            cx,
        );
        let weak = cx.weak_entity();
        let description_subscription = description_input_editor.subscribe(
            Box::new(move |event, window, cx| {
                weak.update(cx, |this, cx| {
                    this.handle_description_input_event(&event, window, cx);
                })
                .ok();
            }),
            window,
            cx,
        );

        let subscriptions = vec![
            name_subscription,
            description_subscription,
            cx.subscribe_in(&body_editor, window, Self::handle_body_editor_event),
        ];

        Self {
            focus_handle,
            fs,
            name_editor,
            description_editor,
            body_editor,
            description_length: 0,
            settings_window,
            disable_model_invocation: false,
            name_error: None,
            description_error: None,
            body_error: None,
            save_error: None,
            saving: false,
            save_task: None,
            scroll_handle: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn name_editor_focus_handle(&self, cx: &App) -> FocusHandle {
        self.name_editor.focus_handle(cx)
    }

    fn handle_name_input_event(
        &mut self,
        event: &ErasedEditorEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, ErasedEditorEvent::BufferEdited) {
            self.recompute_name_error(cx);
            self.save_error = None;
            cx.notify();
        }
    }

    fn handle_description_input_event(
        &mut self,
        event: &ErasedEditorEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, ErasedEditorEvent::BufferEdited) {
            self.recompute_description_error(cx);
            self.save_error = None;
            cx.notify();
        }
    }

    fn handle_body_editor_event(
        &mut self,
        _: &Entity<Editor>,
        event: &EditorEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, EditorEvent::BufferEdited) {
            self.recompute_body_error(cx);
            self.save_error = None;
            cx.notify();
        }
    }

    fn current_name(&self, cx: &App) -> String {
        self.name_editor.read(cx).text(cx)
    }

    fn current_description(&self, cx: &App) -> String {
        self.description_editor.read(cx).text(cx)
    }

    fn current_body(&self, cx: &App) -> String {
        self.body_editor.read(cx).text(cx)
    }

    fn recompute_name_error(&mut self, cx: &mut Context<Self>) {
        let name = self.current_name(cx);
        let error = validate_name(&name).err().or_else(|| {
            if is_reserved_skill_name(&name) {
                Some("This name is reserved for a core skill — pick a different name")
            } else {
                None
            }
        });
        self.name_error = error;
        self.name_editor
            .update(cx, |field, cx| field.set_error(error, cx));
    }

    fn recompute_description_error(&mut self, cx: &mut Context<Self>) {
        let description = self.current_description(cx);
        self.description_length = description.chars().count();
        let error = validate_description(&description).err();
        self.description_error = error;
        self.description_editor
            .update(cx, |field, cx| field.set_error(error, cx));
    }

    fn recompute_body_error(&mut self, cx: &App) {
        let body = self.current_body(cx);
        self.body_error = if body.trim().is_empty() {
            Some("Body is required.")
        } else {
            None
        };
    }

    fn is_valid(&self, cx: &App) -> bool {
        let name = self.current_name(cx);
        validate_name(&name).is_ok()
            && !is_reserved_skill_name(&name)
            && validate_description(&self.current_description(cx)).is_ok()
            && !self.current_body(cx).trim().is_empty()
    }

    pub(crate) fn apply_open_mode(
        &mut self,
        open_mode: SkillCreatorOpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match open_mode {
            SkillCreatorOpenMode::Form => {}
            SkillCreatorOpenMode::Install { content } => {
                self.open_install_review(content, window, cx);
            }
        }
    }

    /// Pre-fill the form with a skill supplied inline (from a share link) so
    /// the recipient can review it before saving. zed-kask: fork-shaped —
    /// parses through `agent_skills`' canonical parser (the D1 single source);
    /// no URL-import machinery is resurrected (the D30 ruling).
    fn open_install_review(
        &mut self,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_error = None;
        match agent_skills::parse_skill_file_content(&content) {
            Ok((metadata, body)) => {
                let name = metadata.name;
                let description = metadata.description;
                let disable_model_invocation = metadata.disable_model_invocation;
                let body = body.trim().to_string();
                let name_editor = self.name_editor.clone();
                let description_editor = self.description_editor.clone();
                let body_editor = self.body_editor.clone();
                let skill_creator = cx.weak_entity();
                window.defer(cx, move |window, cx| {
                    name_editor.update(cx, |input, cx| {
                        input.set_text(&name, window, cx);
                    });
                    description_editor.update(cx, |input, cx| {
                        input.set_text(&description, window, cx);
                    });
                    body_editor.update(cx, |editor, cx| {
                        editor.set_text(body, window, cx);
                    });
                    skill_creator
                        .update(cx, |this, cx| {
                            this.disable_model_invocation = disable_model_invocation;
                            this.save_error = None;
                            this.recompute_name_error(cx);
                            this.recompute_description_error(cx);
                            this.recompute_body_error(cx);
                            cx.notify();
                        })
                        .log_err();
                    window.focus(&name_editor.focus_handle(cx), cx);
                });
            }
            Err(err) => {
                self.save_error = Some(SharedString::from(format!(
                    "Couldn't read shared skill: {err}"
                )));
                cx.notify();
            }
        }
    }

    fn save_skill(&mut self, _: &SaveSkill, window: &mut Window, cx: &mut Context<Self>) {
        self.recompute_name_error(cx);
        self.recompute_description_error(cx);
        self.recompute_body_error(cx);

        if !self.is_valid(cx) || self.saving {
            cx.notify();
            return;
        }

        // Resolve the scope at save time so the skill is written to whichever
        // settings file is selected at the moment the user clicks Save.
        let scope = self
            .settings_window
            .read_with(cx, |settings_window, cx| {
                scope_for_settings_file(
                    &settings_window.current_file,
                    settings_window.original_window.as_ref(),
                    cx,
                )
            })
            .unwrap_or(ScopeChoice::Global);
        let name = self.current_name(cx);
        let description = self.current_description(cx);
        let body = self.current_body(cx);
        let disable_model_invocation = self.disable_model_invocation;
        let fs = self.fs.clone();

        self.saving = true;
        self.save_error = None;
        cx.notify();

        let task = cx.spawn_in(window, async move |this, cx| {
            let result = match &scope {
                ScopeChoice::Global => {
                    write_skill_to_disk(
                        fs.as_ref(),
                        &global_skills_dir(),
                        &name,
                        &description,
                        &body,
                        disable_model_invocation,
                    )
                    .await
                }
                ScopeChoice::Project { project, worktree } => {
                    write_skill_to_worktree(
                        project,
                        worktree,
                        &name,
                        &description,
                        &body,
                        disable_model_invocation,
                        cx,
                    )
                    .await
                }
            };

            this.update_in(cx, |this, _window, cx| {
                this.saving = false;
                this.save_task = None;
                match result {
                    Ok(_) => {
                        // Rescan skill directories so new skills show up in Settings page right away
                        if let Some(hook) = cx.try_global::<SkillsUpdatedHook>() {
                            let hook = hook.0.clone();
                            hook(cx);
                        }

                        cx.emit(SkillCreatorEvent::Saved);
                    }
                    Err(err) => {
                        this.save_error = Some(SharedString::from(err.to_string()));
                        cx.notify();
                    }
                }
            })
            .log_err();
        });
        self.save_task = Some(task);
    }

    fn cancel(&mut self, _: &Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        // Block dismissal while a save is in flight
        if self.saving {
            return;
        }
        cx.emit(SkillCreatorEvent::Dismissed);
    }

    fn toggle_disable_model_invocation(&mut self, cx: &mut Context<Self>) {
        self.disable_model_invocation = !self.disable_model_invocation;
        cx.notify();
    }

    fn render_form_fields(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("skill-creator-form-fields")
            .flex_grow_1()
            .flex_shrink_0()
            .gap_4()
            .child(
                v_flex()
                    .gap_2()
                    .child(Label::new("Front-matter"))
                    .child(self.name_editor.clone())
                    .child(self.description_editor.clone()),
            )
            .child(self.render_optional_params(cx))
            .child(Divider::horizontal())
            .child(
                v_flex()
                    .flex_grow_1()
                    .flex_shrink_0()
                    .gap_2()
                    .child(Label::new("Skill Content"))
                    .child(self.render_body_field(window, cx))
                    .when_some(self.body_error, |this, error| {
                        this.child(Label::new(error).size(LabelSize::Small).color(Color::Error))
                    }),
            )
    }

    fn render_optional_params(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let toggle_state: ToggleState = self.disable_model_invocation.into();

        SwitchField::new(
            "disable-model-invocation",
            Some("Disable model invocation"),
            Some(
                "Hide this skill from the model's catalog. It can still be invoked via slash command."
                    .into(),
            ),
            toggle_state,
            cx.listener(|this, _state: &ToggleState, _window, cx| {
                this.toggle_disable_model_invocation(cx);
            }),
        )
        .tab_index(DISABLE_MODEL_INVOCATION_TAB_INDEX)
        .into_any_element()
    }

    fn render_body_field(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = ThemeSettings::get_global(cx);
        let theme = cx.theme().clone();

        let has_error = self.body_error.is_some();

        let focus_handle = self
            .body_editor
            .focus_handle(cx)
            .tab_index(BODY_FIELD_TAB_INDEX)
            .tab_stop(true);

        let border_color = if has_error {
            theme.status().error_border
        } else if focus_handle.contains_focused(window, cx) {
            theme.colors().border_focused
        } else {
            theme.colors().border
        };

        div()
            .w_full()
            .flex_1()
            .min_h(px(160.))
            .p_2p5()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .bg(theme.colors().editor_background)
            .track_focus(&focus_handle)
            .overflow_hidden()
            .child(EditorElement::new(
                &self.body_editor,
                EditorStyle {
                    local_player: theme.players().local(),
                    text: TextStyle {
                        color: theme.colors().text,
                        font_family: settings.buffer_font.family.clone(),
                        font_features: settings.buffer_font.features.clone(),
                        font_size: rems(0.875).into(),
                        font_weight: settings.buffer_font.weight,
                        line_height: relative(settings.buffer_line_height.value()),
                        ..Default::default()
                    },
                    syntax: theme.syntax().clone(),
                    inlay_hints_style: editor::make_inlay_hints_style(cx),
                    edit_prediction_styles: editor::make_suggestion_styles(cx),
                    ..EditorStyle::default()
                },
            ))
    }

    fn render_footer(&self, _window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saving = self.saving;
        let main_action = if saving { "Saving…" } else { "Save Skill" };

        v_flex()
            .w_full()
            .py_2p5()
            .px_8()
            .border_t_1()
            .border_color(cx.theme().colors().border_variant.opacity(0.4))
            .when(self.save_error.is_some(), |this| {
                this.gap_2().child(
                    Banner::new()
                        .severity(Severity::Error)
                        .children(self.save_error.clone().map(|err| Label::new(err))),
                )
            })
            .child(
                h_flex().w_full().gap_1().justify_end().child(
                    Button::new("save-skill", main_action)
                        .size(ButtonSize::Medium)
                        .style(ButtonStyle::Outlined)
                        .loading(saving)
                        .tab_index(SAVE_BUTTON_TAB_INDEX)
                        // Call `save_skill` directly instead of dispatching the
                        // `SaveSkill` action: action dispatch follows the focused
                        // element's path, so a dispatched action is silently
                        // dropped whenever focus is outside the creator (e.g.
                        // right after switching the settings file/scope).
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.save_skill(&SaveSkill, window, cx);
                        })),
                ),
            )
    }

    fn focus_next_field(
        &mut self,
        _: &FocusNextField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_next(cx);
    }

    fn focus_previous_field(
        &mut self,
        _: &FocusPreviousField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_prev(cx);
    }

    fn on_menu_next(&mut self, _: &menu::SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    fn on_menu_prev(
        &mut self,
        _: &menu::SelectPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_prev(cx);
    }
}

impl Focusable for SkillCreatorPage {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SkillCreatorPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("skill-creator")
            .key_context("SkillCreator")
            .track_focus(&self.focus_handle)
            .on_action(
                |action_sequence: &ActionSequence, window: &mut Window, cx: &mut App| {
                    for action in &action_sequence.0 {
                        window.dispatch_action(action.boxed_clone(), cx);
                    }
                },
            )
            .on_action(cx.listener(Self::save_skill))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::focus_next_field))
            .on_action(cx.listener(Self::focus_previous_field))
            .on_action(cx.listener(Self::on_menu_next))
            .on_action(cx.listener(Self::on_menu_prev))
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .vertical_scrollbar_for(&self.scroll_handle, window, cx)
                    .child(
                        v_flex()
                            .id("skill-creator-form")
                            .tab_index(0)
                            .tab_group()
                            .tab_stop(false)
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                            .gap_4()
                            .px_8()
                            .py_4()
                            .child(self.render_form_fields(window, cx)),
                    ),
            )
            .child(self.render_footer(window, cx))
    }
}

/// Serialize the SKILL.md file to disk at `<skills_dir>/<name>/SKILL.md`.
///
/// Refuses to overwrite an existing directory at `<skills_dir>/<name>`. The
/// caller surfaces the resulting error to the user, who picks a different
/// name.
async fn write_skill_to_disk(
    fs: &dyn Fs,
    skills_dir: &std::path::Path,
    name: &str,
    description: &str,
    body: &str,
    disable_model_invocation: bool,
) -> Result<PathBuf> {
    // Reserved (core) skill names cannot be used by user-authored skills.
    // This is a defensive check — the UI (`recompute_name_error`, `is_valid`)
    // already blocks reserved names before save — but a direct caller
    // (e.g. a future import path) must not be able to bypass the creator's
    // reserved-name policy. The loader separately rejects project files that
    // claim core status; it permits non-core collisions for explicit scopes.
    if is_reserved_skill_name(name) {
        anyhow::bail!(
            "The name \"{name}\" is reserved for a core skill. \
             Pick a different name."
        );
    }
    let skill_dir = skills_dir.join(name);
    match fs.metadata(&skill_dir).await {
        Ok(Some(metadata)) => {
            return Err(existing_skill_path_error(name, &skill_dir, metadata.is_dir));
        }
        Ok(None) => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to check whether {} already exists",
                    skill_dir.display()
                )
            });
        }
    }

    let content = format_skill_file(name, description, body, disable_model_invocation)?;

    fs.create_dir(&skill_dir)
        .await
        .with_context(|| format!("failed to create skill directory {}", skill_dir.display()))?;
    let skill_file_path = skill_dir.join(SKILL_FILE_NAME);
    fs.write(&skill_file_path, content.as_bytes())
        .await
        .with_context(|| format!("failed to write {}", skill_file_path.display()))?;

    Ok(skill_file_path)
}

async fn write_skill_to_worktree(
    project: &Entity<Project>,
    worktree: &Entity<Worktree>,
    name: &str,
    description: &str,
    body: &str,
    disable_model_invocation: bool,
    cx: &mut AsyncApp,
) -> Result<PathBuf> {
    let skills_dir = RelPath::from_unix_str(project_skills_relative_path())?;
    let skill_dir = skills_dir.join(RelPath::from_unix_str(name)?);
    let skill_file_path = skill_dir.join(RelPath::from_unix_str(SKILL_FILE_NAME)?);
    let skill_dir_abs_path = worktree.read_with(cx, |worktree, _| worktree.absolutize(&skill_dir));

    let Some(host_check) = project.read_with(cx, |project, cx| {
        project.host_path_is_dir(&skill_dir_abs_path, cx)
    }) else {
        anyhow::bail!("You can't create skills in a project someone shared with you.");
    };
    let existing_is_dir = host_check.await.with_context(|| {
        format!(
            "failed to check whether {} already exists",
            skill_dir_abs_path.display()
        )
    })?;
    if let Some(is_dir) = existing_is_dir {
        return Err(existing_skill_path_error(name, &skill_dir_abs_path, is_dir));
    }

    let content = format_skill_file(name, description, body, disable_model_invocation)?;
    let skill_file_abs_path =
        worktree.read_with(cx, |worktree, _| worktree.absolutize(&skill_file_path));
    worktree
        .update(cx, |worktree, cx| {
            worktree.create_entry(
                skill_file_path.as_rel_path().into(),
                false,
                Some(content.into_bytes()),
                cx,
            )
        })
        .await
        .with_context(|| format!("failed to write {}", skill_file_abs_path.display()))?;

    Ok(skill_file_abs_path)
}

fn existing_skill_path_error(
    name: &str,
    skill_dir: &std::path::Path,
    is_dir: bool,
) -> anyhow::Error {
    if is_dir {
        anyhow!(
            "A skill named \"{name}\" already exists at {}. Pick a different name.",
            skill_dir.display()
        )
    } else {
        // Without this check, creating the directory fails with a vague "File exists" error.
        anyhow!(
            "A file (not a skill directory) already exists at {}. \
             Delete it or pick a different skill name.",
            skill_dir.display()
        )
    }
}

fn format_skill_file(
    name: &str,
    description: &str,
    body: &str,
    disable_model_invocation: bool,
) -> Result<String> {
    let metadata = SkillMetadata {
        name: name.to_string(),
        description: description.to_string(),
        disable_model_invocation,
        dependencies: Vec::new(),
        core: false,
        audience: None,
    };
    let frontmatter = serde_yaml_ng::to_string(&metadata)
        .context("failed to serialize skill frontmatter as YAML")?;

    let mut content = String::with_capacity(frontmatter.len() + body.len() + 16);
    content.push_str("---\n");
    content.push_str(&frontmatter);
    content.push_str("---\n");
    let trimmed_body = body.trim();
    if !trimmed_body.is_empty() {
        content.push('\n');
        content.push_str(trimmed_body);
        content.push('\n');
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_skills::{SkillSource, parse_skill_frontmatter};
    use fs::FakeFs;
    use std::path::Path;
    use util::path;
    // Name and description validation rules are unit-tested in
    // `agent_skills`, which owns `validate_name` / `validate_description`
    // / `MAX_SKILL_DESCRIPTION_LEN`. The tests below cover the skill
    // creator's own surface area: SKILL.md formatting and disk-writing.

    #[test]
    fn format_skill_file_round_trips_through_parser() {
        let content =
            format_skill_file("draft-pr", "Push a draft PR", "Do the thing.", false).unwrap();
        let skill = parse_skill_frontmatter(
            Path::new("/skills/draft-pr/SKILL.md"),
            &content,
            SkillSource::Global,
        )
        .expect("generated frontmatter must round-trip through parse_skill_frontmatter");
        assert_eq!(skill.name, "draft-pr");
        assert_eq!(skill.description, "Push a draft PR");
        assert!(!skill.disable_model_invocation);
    }

    #[test]
    fn format_skill_file_writes_disable_model_invocation_true() {
        let content = format_skill_file("my-skill", "description", "body", true).unwrap();
        assert!(content.contains("disable-model-invocation: true"));
    }

    #[test]
    fn format_skill_file_omits_body_when_empty() {
        let content = format_skill_file("my-skill", "description", "   ", false).unwrap();
        // The trailing closing-delimiter newline is the last byte.
        assert!(content.ends_with("---\n"));
    }

    #[test]
    fn format_skill_file_escapes_yaml_specials_in_description() {
        // serde_yaml_ng must quote/escape descriptions that contain YAML
        // specials so the file round-trips. If we ever swap formatters,
        // this test will catch a regression.
        let tricky = "contains: a colon, # a hash, and a \"quote\"";
        let content = format_skill_file("weird-skill", tricky, "body", false).unwrap();
        let skill = parse_skill_frontmatter(
            Path::new("/skills/weird-skill/SKILL.md"),
            &content,
            SkillSource::Global,
        )
        .expect("YAML-special characters must round-trip");
        assert_eq!(skill.description, tricky);
    }

    #[gpui::test]
    async fn write_skill_to_disk_creates_directory_and_file(cx: &mut gpui::TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree("/skills", serde_json::json!({})).await;

        let path = write_skill_to_disk(
            fs.as_ref(),
            Path::new("/skills"),
            "draft-pr",
            "Push a draft PR",
            "Body of the skill.",
            false,
        )
        .await
        .expect("write should succeed");

        assert_eq!(path, Path::new("/skills/draft-pr/SKILL.md"));
        let content = fs.load(&path).await.expect("file should exist");
        let skill = parse_skill_frontmatter(&path, &content, SkillSource::Global)
            .expect("written file should be parseable");
        assert_eq!(skill.name, "draft-pr");
        assert_eq!(skill.description, "Push a draft PR");
    }

    #[gpui::test]
    async fn write_skill_to_disk_refuses_to_overwrite(cx: &mut gpui::TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            "/skills",
            serde_json::json!({
                "draft-pr": {
                    "SKILL.md": "---\nname: draft-pr\ndescription: existing\n---\nbody\n"
                }
            }),
        )
        .await;

        let err = write_skill_to_disk(
            fs.as_ref(),
            Path::new("/skills"),
            "draft-pr",
            "Push a draft PR",
            "Body of the skill.",
            false,
        )
        .await
        .expect_err("writing over an existing skill must fail");
        assert!(
            err.to_string().contains("already exists"),
            "error message should mention the conflict, got: {err}"
        );
    }

    #[gpui::test]
    async fn write_skill_to_disk_refuses_reserved_name(cx: &mut gpui::TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree("/skills", serde_json::json!({})).await;

        let err = write_skill_to_disk(
            fs.as_ref(),
            Path::new("/skills"),
            "metacognition",
            "Hostile takeover",
            "Body of the skill.",
            false,
        )
        .await
        .expect_err("writing a skill with a reserved name must fail");
        assert!(
            err.to_string().contains("reserved"),
            "error should mention 'reserved', got: {err}"
        );
        // Nothing should have been written.
        assert!(
            !fs.is_file(Path::new("/skills/metacognition/SKILL.md"))
                .await
        );
    }

    #[gpui::test]
    async fn write_skill_to_disk_rejects_non_directory_at_skill_path(
        cx: &mut gpui::TestAppContext,
    ) {
        let fs = FakeFs::new(cx.executor());
        // A *file* (not a directory) sitting at `/skills/draft-pr`. With the
        // old `is_dir` check this slipped through and we ended up surfacing
        // the underlying "File exists" OS error.
        fs.insert_tree(
            "/skills",
            serde_json::json!({ "draft-pr": "i am a stray file" }),
        )
        .await;

        let err = write_skill_to_disk(
            fs.as_ref(),
            Path::new("/skills"),
            "draft-pr",
            "Push a draft PR",
            "Body of the skill.",
            false,
        )
        .await
        .expect_err("writing where a file already lives must fail");
        let message = err.to_string();
        assert!(
            message.contains("not a skill directory"),
            "error should explain the conflict is a non-directory, got: {message}"
        );
        // Path separator differs between platforms
        let expected_path = Path::new("/skills").join("draft-pr");
        let expected_path = expected_path.display().to_string();
        assert!(
            message.contains(&expected_path),
            "error should include the conflicting path {expected_path:?}, got: {message}"
        );
    }

    async fn init_worktree_test(
        tree: serde_json::Value,
        cx: &mut gpui::TestAppContext,
    ) -> (Arc<FakeFs>, Entity<Project>, Entity<Worktree>) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/project"), tree).await;
        let project = Project::test(fs.clone(), [path!("/project").as_ref()], cx).await;
        let worktree = project.read_with(cx, |project, cx| {
            project
                .worktrees(cx)
                .next()
                .expect("project should have a worktree")
        });
        cx.run_until_parked();
        (fs, project, worktree)
    }

    async fn write_draft_pr_skill(
        project: &Entity<Project>,
        worktree: &Entity<Worktree>,
        cx: &mut gpui::TestAppContext,
    ) -> Result<PathBuf> {
        write_skill_to_worktree(
            project,
            worktree,
            "draft-pr",
            "Push a draft PR",
            "Body of the skill.",
            false,
            &mut cx.to_async(),
        )
        .await
    }

    #[gpui::test]
    async fn write_skill_to_worktree_creates_directory_and_file(cx: &mut gpui::TestAppContext) {
        let (fs, project, worktree) = init_worktree_test(serde_json::json!({}), cx).await;

        let path = write_draft_pr_skill(&project, &worktree, cx)
            .await
            .expect("write should succeed");

        assert_eq!(
            path,
            Path::new(path!("/project/.agents/skills/draft-pr/SKILL.md"))
        );
        let content = fs.load(&path).await.expect("file should exist");
        let skill = parse_skill_frontmatter(&path, &content, SkillSource::Global)
            .expect("written file should be parseable");
        assert_eq!(skill.name, "draft-pr");
        assert_eq!(skill.description, "Push a draft PR");
    }

    #[gpui::test]
    async fn write_skill_to_worktree_refuses_to_overwrite_excluded_skill(
        cx: &mut gpui::TestAppContext,
    ) {
        let existing_content = "---\nname: draft-pr\ndescription: existing\n---\nbody\n";
        let (fs, project, worktree) = init_worktree_test(
            serde_json::json!({
                ".zed": { "settings.json": r#"{ "file_scan_exclusions": [".agents"] }"# },
                ".agents": { "skills": { "draft-pr": { "SKILL.md": existing_content } } },
            }),
            cx,
        )
        .await;
        worktree.read_with(cx, |worktree, _| {
            assert!(
                worktree
                    .entry_for_path(RelPath::from_unix_str(".agents/skills/draft-pr").unwrap())
                    .is_none(),
                "the excluded skill should be missing from the worktree"
            );
        });

        let err = write_draft_pr_skill(&project, &worktree, cx)
            .await
            .expect_err("writing over an existing skill must fail");
        assert!(
            err.to_string().contains("already exists"),
            "error message should mention the conflict, got: {err}"
        );
        let content = fs
            .load(Path::new(path!(
                "/project/.agents/skills/draft-pr/SKILL.md"
            )))
            .await
            .expect("existing skill should still exist");
        assert_eq!(content, existing_content);
    }

    #[gpui::test]
    async fn write_skill_to_worktree_rejects_non_directory_at_skill_path(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_fs, project, worktree) = init_worktree_test(
            serde_json::json!({ ".agents": { "skills": { "draft-pr": "i am a stray file" } } }),
            cx,
        )
        .await;

        let err = write_draft_pr_skill(&project, &worktree, cx)
            .await
            .expect_err("writing where a file already lives must fail");
        assert!(
            err.to_string().contains("not a skill directory"),
            "error should explain the conflict is a non-directory, got: {err}"
        );
    }
}
