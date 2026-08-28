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
}
