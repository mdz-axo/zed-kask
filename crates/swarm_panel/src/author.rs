//! The agent-authoring surface: form state, editor construction, and the
//! `render_author` renderer. Extracted from `swarm_panel.rs` — the renderer
//! stays a method on `SwarmPanel` (it dispatches via `cx.listener` into panel
//! methods); this module owns the form struct and the view construction.

use editor::Editor;
use gpui::{Context, Entity, SharedString, Window};
use ui::{
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip,
    prelude::*,
};

use crate::SwarmPanel;

/// State for the agent-authoring surface.
pub(crate) struct AuthorForm {
    pub(crate) name: Entity<Editor>,
    pub(crate) description: Entity<Editor>,
    pub(crate) system_prompt: Entity<Editor>,
    pub(crate) agent_type: String,
    /// When `Some`, the form is editing an existing agent (loaded via the
    /// drill-down from the browse card). The name field is read-only (renaming
    /// would change the agent id). When `None`, the form is creating a new
    /// agent. The submit button label and the save path branch on this.
    pub(crate) editing_id: Option<String>,
    /// Comma-separated tags for catalogue discovery.
    pub(crate) tags: Entity<Editor>,
    /// Visibility level: "public", "private", or "unlisted".
    pub(crate) visibility: String,
    /// Valence arousal (0.0–1.0). Parsed from the editor text at create time.
    pub(crate) valence_arousal: Entity<Editor>,
    /// Valence polarity (0.0–1.0). Parsed from the editor text at create time.
    pub(crate) valence_valence: Entity<Editor>,
    /// One-word primary affect label (e.g. "curiosity", "precision").
    pub(crate) valence_primary_affect: Entity<Editor>,
    /// Comma-separated personality trait descriptors.
    pub(crate) valence_personality_traits: Entity<Editor>,
    /// Result of the last create attempt (success id or error).
    pub(crate) status: Option<SharedString>,
    pub(crate) busy: bool,
}

impl AuthorForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<SwarmPanel>) -> Self {
        Self {
            name: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("agent_name (lowercase_with_underscores)", window, cx);
                e
            }),
            description: cx.new(|cx| {
                // Multi-line auto-height: descriptions can be a full
                // paragraph. Grows 2–6 lines with content.
                let mut e = Editor::auto_height(2, 6, window, cx);
                e.set_placeholder_text(
                    "One-sentence description shown on the agent card",
                    window,
                    cx,
                );
                e
            }),
            system_prompt: cx.new(|cx| {
                // Multi-line auto-height: system prompts are multi-paragraph
                // by nature (the L3 finding). Grows 8–40 lines with content —
                // enlarged from the prior 4–16 to give operators more space.
                let mut e = Editor::auto_height(8, 40, window, cx);
                e.set_placeholder_text(
                    "System prompt — the agent's instructions (multiple lines supported)",
                    window,
                    cx,
                );
                e
            }),
            agent_type: "research".to_string(),
            editing_id: None,
            tags: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("tag1, tag2, tag3 (comma-separated)", window, cx);
                e
            }),
            visibility: "private".to_string(),
            valence_arousal: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("0.0–1.0 (e.g. 0.6)", window, cx);
                e
            }),
            valence_valence: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("0.0–1.0 (e.g. 0.8)", window, cx);
                e
            }),
            valence_primary_affect: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("curiosity, precision, vigilance…", window, cx);
                e
            }),
            valence_personality_traits: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text(
                    "analytical, cautious, pragmatic (comma-separated)",
                    window,
                    cx,
                );
                e
            }),
            status: None,
            busy: false,
        }
    }
}

impl SwarmPanel {
    /// The agent-authoring surface: name, agent type, description, system
    /// prompt, create. Every field carries a tooltip so the operator always
    /// has a nudge for what to enter and which backend it targets.
    pub(crate) fn render_author(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border;
        let is_local = Self::current_swarm_mode(cx) == kask_bridge::SwarmModeConfig::Local;
        let is_editing = self.author.editing_id.is_some();
        let create_label = if self.author.busy {
            if is_editing { "Updating…" } else { "Creating…" }
        } else if is_editing {
            if is_local { "Update Local Agent" } else { "Update Agent" }
        } else if is_local {
            "Create Local Agent"
        } else {
            "Create Agent"
        };
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .child(
                Headline::new(if self.author.editing_id.is_some() {
                    "Edit Agent"
                } else {
                    "Author an Agent"
                })
                .size(HeadlineSize::Small),
            )
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
                            .id("author-name")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "Agent identifier (lowercase_with_underscores). Becomes the \
                                 system id used to hire, delegate, and reference the agent.",
                            ))
                            .child(self.author.name.clone()),
                    )
                    .when(self.author.editing_id.is_some(), |this| {
                        this.child(
                            Label::new("Name is read-only when editing (renaming would \
                                        create a new agent).")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            )
            // Agent type selector — previously the type was hardcoded to
            // "research" with no UI control, so the operator could never
            // choose it. Wired to `self.author.agent_type`.
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Agent type")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-agent-type-group")
                            .tooltip(Tooltip::text(
                                "The agent's role category. Drives how the catalogue \
                                 groups the agent and which delegation filters match it.",
                            ))
                            .child(
                                ToggleButtonGroup::single_row(
                                    "author-agent-type",
                                    [
                                        ToggleButtonSimple::new(
                                            "research",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.agent_type = "research".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "creative",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.agent_type = "creative".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "meta",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.agent_type = "meta".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                    ],
                                )
                                .style(ToggleButtonGroupStyle::Outlined)
                                .size(ToggleButtonGroupSize::Custom(rems_from_px(28.)))
                                .label_size(LabelSize::Small)
                                .auto_width()
                                .selected_index(match self.author.agent_type.as_str() {
                                    "creative" => 1,
                                    "meta" => 2,
                                    _ => 0,
                                })
                                .into_any_element(),
                            ),
                    ),
            )
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
                            .id("author-description")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "One-sentence summary shown on the agent card in the \
                                 browse list. Optional but recommended for discovery.",
                            ))
                            .child(self.author.description.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("System prompt")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-system-prompt")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "The agent's instructions — what it should do, how \
                                 it should behave, and any constraints. Multiple lines \
                                 supported. Required.",
                            ))
                            .child(self.author.system_prompt.clone()),
                    ),
            )
            // Tags — comma-separated discovery tags.
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Tags")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-tags")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "Comma-separated tags for catalogue discovery \
                                 (e.g. \"research, analysis, forecasting\"). Optional.",
                            ))
                            .child(self.author.tags.clone()),
                    ),
            )
            // Visibility — public / private / unlisted toggle.
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Visibility")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-visibility-group")
                            .tooltip(Tooltip::text(
                                "Public agents appear in the catalogue browse list. \
                                 Private agents are visible only to the owner. Unlisted \
                                 agents are accessible by URL but not listed.",
                            ))
                            .child(
                                ToggleButtonGroup::single_row(
                                    "author-visibility",
                                    [
                                        ToggleButtonSimple::new(
                                            "private",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.visibility = "private".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "unlisted",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.visibility = "unlisted".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "public",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.visibility = "public".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                    ],
                                )
                                .style(ToggleButtonGroupStyle::Outlined)
                                .size(ToggleButtonGroupSize::Custom(rems_from_px(28.)))
                                .label_size(LabelSize::Small)
                                .auto_width()
                                .selected_index(match self.author.visibility.as_str() {
                                    "unlisted" => 1,
                                    "public" => 2,
                                    _ => 0,
                                })
                                .into_any_element(),
                            ),
                    ),
            )
            // Valence — personality encoding fields.
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Valence — personality encoding")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .flex_1()
                                    .child(
                                        Label::new("Arousal (0–1)")
                                            .size(LabelSize::XSmall)
                                            .color(Color::Muted),
                                    )
                                    .child(
                                        div()
                                            .id("author-valence-arousal")
                                            .border_1()
                                            .border_color(border)
                                            .rounded_sm()
                                            .tooltip(Tooltip::text(
                                                "Arousal level: 0.0 = calm, 1.0 = highly activated.",
                                            ))
                                            .child(self.author.valence_arousal.clone()),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_1()
                                    .flex_1()
                                    .child(
                                        Label::new("Valence (0–1)")
                                            .size(LabelSize::XSmall)
                                            .color(Color::Muted),
                                    )
                                    .child(
                                        div()
                                            .id("author-valence-valence")
                                            .border_1()
                                            .border_color(border)
                                            .rounded_sm()
                                            .tooltip(Tooltip::text(
                                                "Valence polarity: 0.0 = serious, 1.0 = enthusiastic.",
                                            ))
                                            .child(self.author.valence_valence.clone()),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                Label::new("Primary affect")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                div()
                                    .id("author-valence-affect")
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .tooltip(Tooltip::text(
                                        "One-word affect label (e.g. curiosity, precision, vigilance). Optional.",
                                    ))
                                    .child(self.author.valence_primary_affect.clone()),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                Label::new("Personality traits")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                div()
                                    .id("author-valence-traits")
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .tooltip(Tooltip::text(
                                        "Comma-separated trait descriptors \
                                         (e.g. analytical, cautious, pragmatic). Optional.",
                                    ))
                                    .child(self.author.valence_personality_traits.clone()),
                            ),
                    ),
            )
            // AI Assist + validation — model-backed suggestions and a
            // well-formedness check before create. Mirrors the publish banner
            // pattern: a bordered box with Apply/Dismiss (suggestions) or a
            // success/issues list (validation).
            .child(self.render_ai_assist_row("agent", self.author.busy, cx))
            .children(self.render_ai_suggestions_banner("agent", cx))
            .children(self.render_validation_banner("agent", cx))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("create-agent", create_label)
                            .style(ButtonStyle::Filled)
                            .disabled(self.author.busy)
                            .tooltip(Tooltip::text(if is_local {
                                "Creates the agent on the local substrate \
                                 (agents/local/curated). No cost, no consent."
                            } else {
                                "Creates the agent in the ABW catalogue. \
                                 The catalogue may apply its own validation."
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_agent(cx);
                            })),
                    )
                    .when_some(self.author.status.clone(), |this, status| {
                        this.child(
                            Label::new(status)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            )
    }
}
