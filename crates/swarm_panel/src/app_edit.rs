//! The App authoring + detail surface: form state, editor construction, and
//! the `render_app_author` renderer. Extracted from `swarm_panel.rs` — the
//! renderer stays a method on `SwarmPanel` (it dispatches via `cx.listener`
//! into panel methods); this module owns the form struct and the view
//! construction. See `author.rs` / `compose.rs` for the same extraction
//! pattern.
//!
//! The App form serves two roles:
//! 1. **Detail view** — loaded when the operator clicks an App card. Shows
//!    the App's manifest fields (slug, name, tagline, description, visibility,
//!    workspace_template) in editable editors. The slug is read-only when
//!    editing (renaming would change the App's identity).
//! 2. **Authoring form** — when the operator clicks "New App" (the Author
//!    mode toggle with the Apps filter active), the form is blank and the
//!    operator fills in the manifest fields to create a new App.

use editor::Editor;
use gpui::{Context, Entity, SharedString, Window};
use ui::{
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple,
    prelude::*,
};

use crate::SwarmPanel;
use crate::status_is_warning;

/// State for the App authoring/detail surface.
pub(crate) struct AppForm {
    /// App slug (3–64 chars, lowercase letters, digits, underscores).
    /// Read-only when editing.
    pub(crate) slug: Entity<Editor>,
    /// Human-readable name.
    pub(crate) name: Entity<Editor>,
    /// One-line tagline for catalogue surfacing.
    pub(crate) tagline: Entity<Editor>,
    /// Longer description.
    pub(crate) description: Entity<Editor>,
    /// Optional homepage URL.
    pub(crate) homepage_url: Entity<Editor>,
    /// Optional icon URL.
    pub(crate) icon_url: Entity<Editor>,
    /// Workspace template as JSON (initial_budget, auto_hire, initial_files).
    /// Multi-line editor — the template is a JSON object.
    pub(crate) workspace_template: Entity<Editor>,
    /// Visibility level: "private", "unlisted", or "public".
    pub(crate) visibility: String,
    /// When `Some`, the form is editing an existing App (loaded via a card
    /// click). The slug field is read-only. When `None`, the form is creating
    /// a new App.
    pub(crate) editing_slug: Option<String>,
    /// Result of the last create/update attempt (success message or error).
    pub(crate) status: Option<SharedString>,
    pub(crate) busy: bool,
}

impl AppForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<SwarmPanel>) -> Self {
        Self {
            slug: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text(
                    "app_slug (lowercase_with_underscores, 3-64 chars)",
                    window,
                    cx,
                );
                e
            }),
            name: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text(
                    "Human-readable name (defaults to slug-derived name)",
                    window,
                    cx,
                );
                e
            }),
            tagline: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("One-line tagline for the catalogue", window, cx);
                e
            }),
            description: cx.new(|cx| {
                let mut e = Editor::auto_height(2, 6, window, cx);
                e.set_placeholder_text("Longer description of what this App does", window, cx);
                e
            }),
            homepage_url: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("https://example.com (optional)", window, cx);
                e
            }),
            icon_url: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("https://example.com/icon.png (optional)", window, cx);
                e
            }),
            workspace_template: cx.new(|cx| {
                let mut e = Editor::auto_height(6, 30, window, cx);
                e.set_placeholder_text(
                    "{\"initial_budget\": 100, \"auto_hire\": [], \"initial_files\": [...]}",
                    window,
                    cx,
                );
                e
            }),
            visibility: "private".to_string(),
            editing_slug: None,
            status: None,
            busy: false,
        }
    }
}

impl SwarmPanel {
    /// The App authoring/detail surface: slug, name, tagline, description,
    /// workspace_template, visibility, create/update. Every field carries a
    /// tooltip so the operator always has a nudge for what to enter.
    pub(crate) fn render_app_author(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border;
        let is_editing = self.app_form.editing_slug.is_some();
        let create_label: SharedString = if self.app_form.busy {
            if is_editing {
                "Updating…"
            } else {
                "Creating…"
            }
        } else if is_editing {
            "Update App"
        } else {
            "Create App"
        }
        .into();

        v_flex()
            .gap_3()
            .pb_4()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Headline::new(if is_editing { "Edit App" } else { "New App" })
                            .size(HeadlineSize::Small),
                    )
                    .child(div().flex_1())
                    .when_some(self.app_form.status.clone(), |this, status| {
                        this.child(Label::new(&status).size(LabelSize::XSmall).color(
                            if status_is_warning(&status) {
                                Color::Warning
                            } else {
                                Color::Accent
                            },
                        ))
                    }),
            )
            // ── Slug ──────────────────────────────────────────────────────
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Slug")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("app-slug")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.app_form.slug.clone()),
                    ),
            )
            // ── Name ──────────────────────────────────────────────────────
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Name")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("app-name")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.app_form.name.clone()),
                    ),
            )
            // ── Tagline ───────────────────────────────────────────────────
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Tagline")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("app-tagline")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.app_form.tagline.clone()),
                    ),
            )
            // ── Description ───────────────────────────────────────────────
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Description")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("app-description")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.app_form.description.clone()),
                    ),
            )
            // ── Homepage URL + Icon URL (side by side) ────────────────────
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        v_flex()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(
                                Label::new("Homepage URL")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                div()
                                    .id("app-homepage")
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .child(self.app_form.homepage_url.clone()),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(
                                Label::new("Icon URL")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                div()
                                    .id("app-icon")
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .child(self.app_form.icon_url.clone()),
                            ),
                    ),
            )
            // ── Workspace template ────────────────────────────────────────
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Workspace Template (JSON)")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("app-workspace-template")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.app_form.workspace_template.clone()),
                    ),
            )
            // ── Visibility ────────────────────────────────────────────────
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Visibility")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div().child(
                            ToggleButtonGroup::single_row(
                                "app-visibility-buttons",
                                [
                                    ToggleButtonSimple::new(
                                        "Private",
                                        cx.listener(|this, _, _, cx| {
                                            this.app_form.visibility = "private".to_string();
                                            cx.notify();
                                        }),
                                    ),
                                    ToggleButtonSimple::new(
                                        "Unlisted",
                                        cx.listener(|this, _, _, cx| {
                                            this.app_form.visibility = "unlisted".to_string();
                                            cx.notify();
                                        }),
                                    ),
                                    ToggleButtonSimple::new(
                                        "Public",
                                        cx.listener(|this, _, _, cx| {
                                            this.app_form.visibility = "public".to_string();
                                            cx.notify();
                                        }),
                                    ),
                                ],
                            )
                            .style(ToggleButtonGroupStyle::Outlined)
                            .size(ToggleButtonGroupSize::Custom(rems_from_px(28.0_f32)))
                            .label_size(LabelSize::Default)
                            .auto_width()
                            .selected_index(match self.app_form.visibility.as_str() {
                                "unlisted" => 1,
                                "public" => 2,
                                _ => 0,
                            })
                            .into_any_element(),
                        ),
                    ),
            )
            // ── Action row ────────────────────────────────────────────────
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("app-submit", create_label)
                            .style(ButtonStyle::Filled)
                            .disabled(self.app_form.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.create_or_update_app(cx);
                            })),
                    )
                    .when(is_editing, |this| {
                        this.child(
                            Button::new("app-cancel-edit", "Done")
                                .style(ButtonStyle::Subtle)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reset_app_form_for_create(window, cx);
                                    this.set_mode(crate::PanelMode::Browse, window, cx);
                                })),
                        )
                    }),
            )
    }

    // ── App primitive — authoring (load + create/update) ────────────────────

    /// Load an existing App's manifest into the App form for editing.
    /// Fetches the App via `swarm_get_app`, stores the result in
    /// `pending_app_load`, and switches to `AppAuthor` mode. The form
    /// fields are populated in `render` via `apply_pending_app_load` (which
    /// has `&mut Window` for `Editor::set_text`). The slug field is set
    /// read-only immediately (renaming would change the App's identity).
    pub(crate) fn load_app_into_form(
        &mut self,
        slug: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.app_form.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.app_form.editing_slug = Some(slug.clone());
        self.app_form.status = Some("Loading App…".into());
        self.app_form.busy = false;
        self.app_form.slug.update(cx, |e, _| e.set_read_only(true));
        self.app_form
            .slug
            .update(cx, |e, cx| e.set_text(slug.clone(), window, cx));
        self.pending_app_load = None;
        self.set_mode(crate::PanelMode::AppAuthor, window, cx);
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_get_app", json!({ "slug": slug }))
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output);
                            match parsed {
                                Some(app) => {
                                    let get_str = |key: &str| {
                                        app.get(key)
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string()
                                    };
                                    let template_str = app
                                        .get("workspace_template")
                                        .map(|v| {
                                            serde_json::to_string_pretty(v).unwrap_or_default()
                                        })
                                        .unwrap_or_default();
                                    let visibility = {
                                        let v = get_str("visibility");
                                        if v.is_empty() {
                                            "private".to_string()
                                        } else {
                                            v
                                        }
                                    };
                                    this.pending_app_load = Some(crate::AppDetailLoad {
                                        name: get_str("name"),
                                        tagline: get_str("tagline"),
                                        description: get_str("description"),
                                        homepage_url: get_str("homepage_url"),
                                        icon_url: get_str("icon_url"),
                                        workspace_template: template_str,
                                        visibility,
                                    });
                                    this.app_form.status = None;
                                }
                                None => {
                                    this.app_form.status = Some(
                                        format!("Failed to parse App response: {output}").into(),
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            this.app_form.status =
                                Some(format!("Failed to load App: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Apply a pending App detail load to the form. Called from `render`
    /// because `Editor::set_text` requires `&mut Window`, which the spawn
    /// closure does not have. Mirrors `apply_pending_author_load`.
    pub(crate) fn apply_pending_app_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(load) = self.pending_app_load.take() else {
            return;
        };
        self.app_form
            .name
            .update(cx, |e, cx| e.set_text(load.name, window, cx));
        self.app_form
            .tagline
            .update(cx, |e, cx| e.set_text(load.tagline, window, cx));
        self.app_form
            .description
            .update(cx, |e, cx| e.set_text(load.description, window, cx));
        self.app_form
            .homepage_url
            .update(cx, |e, cx| e.set_text(load.homepage_url, window, cx));
        self.app_form
            .icon_url
            .update(cx, |e, cx| e.set_text(load.icon_url, window, cx));
        self.app_form
            .workspace_template
            .update(cx, |e, cx| e.set_text(load.workspace_template, window, cx));
        self.app_form.visibility = load.visibility;
    }

    /// Create a new App or update an existing one. Branches on
    /// `editing_slug`: when `Some`, calls `swarm_update_app`; when `None`,
    /// calls `swarm_create_app_direct`. Validates the slug and workspace
    /// template JSON before dispatching.
    pub(crate) fn create_or_update_app(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.app_form.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let slug = self.app_form.slug.read(cx).text(cx);
        let name = self.app_form.name.read(cx).text(cx);
        let tagline = self.app_form.tagline.read(cx).text(cx);
        let description = self.app_form.description.read(cx).text(cx);
        let homepage_url = self.app_form.homepage_url.read(cx).text(cx);
        let icon_url = self.app_form.icon_url.read(cx).text(cx);
        let template_raw = self.app_form.workspace_template.read(cx).text(cx);
        let visibility = self.app_form.visibility.clone();
        let editing_slug = self.app_form.editing_slug.clone();

        if slug.trim().is_empty() {
            self.app_form.status = Some("Slug is required.".into());
            cx.notify();
            return;
        }
        // Validate slug for new Apps (editing keeps the original slug).
        if editing_slug.is_none() {
            let len = slug.trim().chars().count();
            let valid = (3..=64).contains(&len)
                && slug
                    .trim()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid {
                self.app_form.status = Some(
                    "Slug must be 3-64 chars: lowercase letters, digits, underscores only.".into(),
                );
                cx.notify();
                return;
            }
        }
        // Parse the workspace template JSON if non-empty.
        let workspace_template: Option<serde_json::Value> = if template_raw.trim().is_empty() {
            None
        } else {
            match serde_json::from_str::<serde_json::Value>(&template_raw) {
                Ok(v) if v.is_object() => Some(v),
                Ok(_) => {
                    self.app_form.status = Some("Workspace template must be a JSON object.".into());
                    cx.notify();
                    return;
                }
                Err(e) => {
                    self.app_form.status =
                        Some(format!("Invalid JSON in workspace template: {e}").into());
                    cx.notify();
                    return;
                }
            }
        };

        self.app_form.busy = true;
        self.app_form.status = Some(
            if editing_slug.is_some() {
                "Updating…"
            } else {
                "Creating…"
            }
            .into(),
        );
        cx.notify();

        let is_editing = editing_slug.is_some();
        let tool_name = if is_editing {
            "swarm_update_app"
        } else {
            "swarm_create_app_direct"
        };
        // Build the request payload. For update, the slug is the editing
        // slug (immutable). For create, it's the form's slug field.
        let mut payload = serde_json::json!({});
        let obj = payload.as_object_mut().expect("just constructed object");
        if is_editing {
            obj.insert(
                "slug".into(),
                serde_json::json!(editing_slug.unwrap_or_default()),
            );
        } else {
            obj.insert("slug".into(), serde_json::json!(slug));
        }
        if !name.trim().is_empty() {
            obj.insert("name".into(), serde_json::json!(name));
        }
        if !tagline.trim().is_empty() {
            obj.insert("tagline".into(), serde_json::json!(tagline));
        }
        if !description.trim().is_empty() {
            obj.insert("description".into(), serde_json::json!(description));
        }
        if !homepage_url.trim().is_empty() {
            obj.insert("homepage_url".into(), serde_json::json!(homepage_url));
        }
        if !icon_url.trim().is_empty() {
            obj.insert("icon_url".into(), serde_json::json!(icon_url));
        }
        if let Some(v) = workspace_template {
            obj.insert("workspace_template".into(), v);
        }
        obj.insert("visibility".into(), serde_json::json!(visibility));

        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker.invoke_tool(SWARM_SERVER, tool_name, payload).await;
                this.update(cx, |this, cx| {
                    this.app_form.busy = false;
                    match result {
                        Ok(_) => {
                            this.app_form.status = Some(
                                if is_editing {
                                    "App updated."
                                } else {
                                    "App created."
                                }
                                .into(),
                            );
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.app_form.status = Some(
                                format!(
                                    "Failed to {} App: {err}",
                                    if is_editing { "update" } else { "create" }
                                )
                                .into(),
                            );
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── App primitive — spawn / publish / archive (fermi v0.10.15+) ──────────
    //
    // Card actions for App entries in the browse list. These are the
    // operator-facing App lifecycle actions: spawn a workspace from an App,
    // promote an App to public, and archive an App.

    /// Spawn a new workspace from an App. Calls `swarm_spawn_app_workspace`
    /// and refreshes the swarm list on success so the new workspace appears.
    pub(crate) fn spawn_app_workspace(&mut self, slug: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("spawn-app-{slug}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_spawn_app_workspace",
                        json!({ "slug": slug }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(output) => {
                            let ws_id = parse_tool_response(&output).and_then(|c| {
                                c.get("id")
                                    .or_else(|| c.get("workspace_id"))
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                            });
                            this.spend.hire_error = Some(
                                format!(
                                    "Spawned workspace from App '{slug}'{}.",
                                    ws_id.map(|id| format!(": {id}")).unwrap_or_default()
                                )
                                .into(),
                            );
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error = Some(
                                format!("Failed to spawn workspace from App '{slug}': {err}")
                                    .into(),
                            );
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Publish an App (promote visibility to public). Calls
    /// `swarm_publish_app` and refreshes the app list on success.
    pub(crate) fn publish_app(&mut self, slug: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("publish-app-{slug}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_publish_app", json!({ "slug": slug }))
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            this.spend.hire_error = Some(format!("Published App '{slug}'.").into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to publish App '{slug}': {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Archive an App. Calls `swarm_archive_app` and refreshes the app list
    /// on success. No confirmation modal — the card button is the
    /// confirmation (archived apps are visible with an "archived" badge and
    /// the Spawn button is disabled, so the action is reversible in effect:
    /// the App stays visible, just can't spawn).
    pub(crate) fn archive_app(&mut self, slug: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("archive-app-{slug}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_archive_app", json!({ "slug": slug }))
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            this.spend.hire_error = Some(format!("Archived App '{slug}'.").into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to archive App '{slug}': {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── Workspace action protocol (fermi v0.10.15+) ──────────────────────────
    //
    // The action protocol is the human-in-the-loop confirmation surface:
    // agents propose mutations (mutate_document, fork_state), the panel
    // surfaces them as pending actions, and the operator accepts or rejects.
    // These three methods wire the panel's review queue to the MCP tools.

    /// Accept a pending workspace action. Calls `swarm_workspace_accept_action`
    /// and refreshes the pending-actions list on success.
    pub(crate) fn accept_pending_action(
        &mut self,
        workspace_id: String,
        action_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        // Mark loading so the UI shows a spinner on the accept button.
        if let Some(pa) = self.detail.pending_actions.as_mut() {
            pa.loading = true;
            pa.error = None;
        }
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_workspace_accept_action",
                        json!({
                            "workspace_id": workspace_id,
                            "action_id": action_id,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(_) => {
                            // Refresh the pending-actions list. A failed
                            // refresh is non-fatal — the accept succeeded.
                            this.refresh_pending_actions(cx);
                        }
                        Err(err) => {
                            if let Some(pa) = this.detail.pending_actions.as_mut() {
                                pa.loading = false;
                                pa.error = Some(format!("Failed to accept action: {err}").into());
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Reject a pending workspace action. Calls `swarm_workspace_reject_action`
    /// and refreshes the pending-actions list on success.
    pub(crate) fn reject_pending_action(
        &mut self,
        workspace_id: String,
        action_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        if let Some(pa) = self.detail.pending_actions.as_mut() {
            pa.loading = true;
            pa.error = None;
        }
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_workspace_reject_action",
                        json!({
                            "workspace_id": workspace_id,
                            "action_id": action_id,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(_) => {
                            this.refresh_pending_actions(cx);
                        }
                        Err(err) => {
                            if let Some(pa) = this.detail.pending_actions.as_mut() {
                                pa.loading = false;
                                pa.error = Some(format!("Failed to reject action: {err}").into());
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Refresh the pending-actions list for the currently-open cloud swarm
    /// detail. Called after accept/reject and available as a manual refresh.
    /// No-op when no cloud swarm detail is open.
    pub(crate) fn refresh_pending_actions(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            return;
        };
        let Some(detail) = self.detail.swarm_detail.as_ref() else {
            return;
        };
        // Only cloud swarms have the action protocol.
        if detail.source == AgentSource::Local {
            return;
        }
        let workspace_id = detail.workspace_id.clone();
        if let Some(pa) = self.detail.pending_actions.as_mut() {
            pa.loading = true;
            pa.error = None;
        }
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_workspace_pending_actions",
                        json!({ "workspace_id": workspace_id }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => {
                            let actions = parse_tool_response(&output)
                                .map(crate::parse::parse_pending_actions)
                                .unwrap_or_default();
                            this.detail.pending_actions = Some(PendingActionsView {
                                workspace_id,
                                loading: false,
                                error: None,
                                actions,
                            });
                        }
                        Err(err) => {
                            if let Some(pa) = this.detail.pending_actions.as_mut() {
                                pa.loading = false;
                                pa.error =
                                    Some(format!("Failed to load pending actions: {err}").into());
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}
