//! The agent-authoring surface: form state, editor construction, the
//! `render_author` renderer, and the `create_agent` submit flow. Extracted
//! from `swarm_panel.rs` — the renderer and the create flow stay methods on
//! `SwarmPanel` (they mutate panel state via `cx.spawn` + `this.update`);
//! this module owns the form struct, the view construction, and the create
//! flow.

use editor::Editor;
use gpui::{Context, Entity, SharedString, Window};
use serde_json::json;
use ui::{
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip,
    prelude::*,
};

use crate::CreateTarget;
use crate::SWARM_SERVER;
use crate::SwarmPanel;
use crate::parse::{extract_unsupported_fields_note, extract_wallet_balance};
use crate::status_is_warning;

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
    /// The source of the agent being edited (`Cloud`, `Local`, or `Synced`).
    /// `None` when creating a new agent. Determines which delete tool the
    /// Delete button dispatches to: `swarm_remove_local` for local/synced
    /// (severs the local card), `swarm_delete_agent` for cloud (irreversible
    /// ABW delete). Set by `load_agent_into_author`.
    pub(crate) editing_source: Option<crate::parse::AgentSource>,
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
    /// Sample queries, one per line (fermi `has_sample_queries`: without
    /// one, nobody can tell what to ask this agent).
    pub(crate) sample_queries: Entity<Editor>,
    /// Comma-separated declared input types (fermi `declares_accepts`).
    pub(crate) accepts: Entity<Editor>,
    /// Comma-separated declared output types (fermi `declares_produces`).
    pub(crate) produces: Entity<Editor>,
    /// Which backend to create the agent on (Cloud = ABW, Local = local
    /// substrate). A per-form choice — not gated on `kask.swarm.mode`.
    /// When editing, this is derived from `editing_source`.
    pub(crate) create_target: super::CreateTarget,
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
            editing_source: None,
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
            sample_queries: cx.new(|cx| {
                let mut e = Editor::auto_height(2, 6, window, cx);
                e.set_placeholder_text(
                    "One sample query per line — what would someone ask this agent?",
                    window,
                    cx,
                );
                e
            }),
            accepts: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("text, market_report, … (comma-separated)", window, cx);
                e
            }),
            produces: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text(
                    "sentiment_report, risk_assessment, … (comma-separated)",
                    window,
                    cx,
                );
                e
            }),
            create_target: super::CreateTarget::Cloud,
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
        let is_local = self.author.create_target == super::CreateTarget::Local;
        let is_editing = self.author.editing_id.is_some();
        let editing_source = self.author.editing_source.clone();
        let delete_tooltip = match &editing_source {
            Some(crate::parse::AgentSource::Cloud) => {
                "Permanently deletes this ABW agent (irreversible). Removes it \
                 from your library and every workspace roster. A synced local \
                 card is NOT touched."
            }
            Some(crate::parse::AgentSource::Local) | Some(crate::parse::AgentSource::Synced) => {
                "Permanently deletes this local agent card. A synced card's \
                 ABW agent is NOT touched (delete that separately from the \
                 cloud card's More menu)."
            }
            None => "",
        };
        let create_label = if self.author.busy {
            if is_editing {
                "Updating…"
            } else {
                "Creating…"
            }
        } else if is_editing {
            if is_local {
                "Update Local Agent"
            } else {
                "Update Agent"
            }
        } else if is_local {
            "Create Local Agent"
        } else {
            "Create Agent"
        };
        v_flex()
            .w_full()
            .gap_3()
            // py — the top pad is the gap between the mode-toggle row and the
            // form headline (the content column has no top pad of its own);
            // the bottom pad gives the scroll end breathing room. Horizontal
            // inset comes from the column's px_4 — px here would double it vs
            // Browse.
            .py_4()
            .child(
                Headline::new(if self.author.editing_id.is_some() {
                    "Edit Agent"
                } else {
                    "Author an Agent"
                })
                .size(HeadlineSize::Small),
            )
            // Cloud/Local target toggle — a per-form choice, not a global
            // setting. Both backends are always available; this selects which
            // tool the create button dispatches (`swarm_create_agent` vs
            // `swarm_create_local_agent`). Disabled when editing (the target is
            // derived from the editing source).
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Label::new("Target:")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                div().child(
                                    ToggleButtonGroup::single_row(
                                        "author-create-target",
                                        [
                                            ToggleButtonSimple::new(
                                                "Cloud",
                                                cx.listener(|this, _, _, cx| {
                                                    this.author.create_target =
                                                        super::CreateTarget::Cloud;
                                                    this.active_backend =
                                                        super::CreateTarget::Cloud;
                                                    cx.notify();
                                                }),
                                            ),
                                            ToggleButtonSimple::new(
                                                "Local",
                                                cx.listener(|this, _, _, cx| {
                                                    this.author.create_target =
                                                        super::CreateTarget::Local;
                                                    this.active_backend =
                                                        super::CreateTarget::Local;
                                                    cx.notify();
                                                }),
                                            ),
                                        ],
                                    )
                                    .style(ToggleButtonGroupStyle::Outlined)
                                    .size(ToggleButtonGroupSize::Custom(rems_from_px(24.0_f32)))
                                    .label_size(LabelSize::XSmall)
                                    .auto_width()
                                    .selected_index(if is_local { 1 } else { 0 })
                                    .into_any_element(),
                                ),
                            ),
                    )
                    .when(self.author.editing_id.is_some(), |this| {
                        this.child(
                            Label::new("Target is fixed to the editing source.")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
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
                                // Same 24px/XSmall scale as the Target toggle
                                // above — form controls share one visual size;
                                // the 30px scale is reserved for header nav.
                                .size(ToggleButtonGroupSize::Custom(rems_from_px(24.0_f32)))
                                .label_size(LabelSize::XSmall)
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
                                .size(ToggleButtonGroupSize::Custom(rems_from_px(28.0_f32)))
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
            // Sample queries — one per line. fermi `has_sample_queries`:
            // without one, nobody can tell what to ask this agent.
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Sample queries")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-sample-queries")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "One example question per line. The ABW publish gate \
                                 requires at least one — without one, nobody can tell \
                                 what to ask this agent.",
                            ))
                            .child(self.author.sample_queries.clone()),
                    ),
            )
            // Accepts / produces — the composition contract. fermi
            // `declares_accepts` / `declares_produces`: composition planning
            // routes on inputs; downstream agents match on outputs.
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        v_flex()
                            .gap_1()
                            .flex_1()
                            .child(
                                Label::new("Accepts (input types)")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                div()
                                    .id("author-accepts")
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .tooltip(Tooltip::text(
                                        "Comma-separated input types this agent accepts \
                                         (e.g. text, market_report). The ABW publish gate \
                                         requires at least one — composition planning cannot \
                                         route work to an agent with no declared inputs.",
                                    ))
                                    .child(self.author.accepts.clone()),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .flex_1()
                            .child(
                                Label::new("Produces (output types)")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                div()
                                    .id("author-produces")
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .tooltip(Tooltip::text(
                                        "Comma-separated output types this agent produces \
                                         (e.g. sentiment_report). The ABW publish gate \
                                         requires at least one — downstream agents match \
                                         against it to build pipelines.",
                                    ))
                                    .child(self.author.produces.clone()),
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
                    .when_some(editing_source, |this, source| {
                        let delete_label = match source {
                            crate::parse::AgentSource::Cloud => "Delete Agent",
                            crate::parse::AgentSource::Local
                            | crate::parse::AgentSource::Synced => "Delete Local Card",
                        };
                        this.child(
                            Button::new("delete-agent", delete_label)
                                .style(ButtonStyle::Subtle)
                                .color(Color::Warning)
                                .disabled(self.author.busy)
                                .tooltip(Tooltip::text(delete_tooltip))
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.delete_edited_agent(cx);
                                })),
                        )
                    })
                    .when_some(self.author.status.clone(), |this, status| {
                        let is_warning = status_is_warning(&status);
                        this.child(
                            Label::new(status)
                                .size(LabelSize::Small)
                                .color(if is_warning {
                                    Color::Warning
                                } else {
                                    Color::Muted
                                }),
                        )
                    }),
            )
    }

    /// Reset the author form to a fresh create state (clear `editing_id`,
    /// make the name field editable again, clear the status). Called when the
    /// operator clicks the Author mode toggle in the header — distinct from
    /// `load_agent_into_author`, which sets `editing_id` and read-only before
    /// calling `set_mode`.
    pub(crate) fn reset_author_form_for_create(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.author.editing_id = None;
        self.author.editing_source = None;
        // Keep the panel's backend context rather than resetting to Cloud —
        // the operator's last cloud/local choice carries into the next
        // authoring session (the "doesn't carry over" finding).
        self.author.create_target = self.active_backend;
        self.author.status = None;
        self.author.name.update(cx, |e, _| e.set_read_only(false));
        // Clear the text fields so the operator starts fresh.
        self.author.name.update(cx, |e, cx| e.clear(window, cx));
        self.author
            .description
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .system_prompt
            .update(cx, |e, cx| e.clear(window, cx));
        self.author.tags.update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_arousal
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_valence
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_primary_affect
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_personality_traits
            .update(cx, |e, cx| e.clear(window, cx));
        self.author.agent_type = "research".to_string();
        self.author.visibility = "private".to_string();
    }

    /// Split a comma-separated form field into trimmed, non-empty entries.
    /// Shared by the create and update paths so both send identically-parsed
    /// lists (tags, accepts, produces).
    pub(crate) fn comma_list(raw: &str) -> Vec<String> {
        raw.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Gather the four valence editors into the valence JSON object, or
    /// `None` when every field is empty. Arousal and valence are optional
    /// floats; unparseable text parses to `None` (the server fills neutral
    /// defaults). Shared by the create and update paths.
    pub(crate) fn gather_valence(&self, cx: &mut Context<Self>) -> Option<serde_json::Value> {
        let arousal_raw = self.author.valence_arousal.read(cx).text(cx);
        let valence_raw = self.author.valence_valence.read(cx).text(cx);
        let primary_affect = self.author.valence_primary_affect.read(cx).text(cx);
        let personality_traits =
            Self::comma_list(&self.author.valence_personality_traits.read(cx).text(cx));
        if arousal_raw.trim().is_empty()
            && valence_raw.trim().is_empty()
            && primary_affect.trim().is_empty()
            && personality_traits.is_empty()
        {
            None
        } else {
            Some(json!({
                "arousal": arousal_raw.trim().parse::<f64>().ok(),
                "valence": valence_raw.trim().parse::<f64>().ok(),
                "primary_affect": if primary_affect.trim().is_empty() { None } else { Some(primary_affect.trim()) },
                "personality_traits": personality_traits,
            }))
        }
    }

    /// Create a new agent from the authoring form. Mode-aware: in Local mode
    /// the agent is created on the local substrate via `swarm_create_local_agent`
    /// (field `agent_id`, no cost, no consent); in ABW mode it is created in the
    /// ABW catalogue via `swarm_create_agent` (field `agent_name`).
    pub(crate) fn create_agent(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let name = self.author.name.read(cx).text(cx);
        let description = self.author.description.read(cx).text(cx);
        let system_prompt = self.author.system_prompt.read(cx).text(cx);
        if name.trim().is_empty() || system_prompt.trim().is_empty() {
            self.author.status = Some("Name and system prompt are required.".into());
            cx.notify();
            return;
        }
        let is_local = self.author.create_target == CreateTarget::Local;
        // Target-aware slug pre-validation. ABW requires `^[a-z0-9_]{3,64}$` —
        // a server-side rejection after the operator has filled every field
        // is a poor round-trip; validate up front so the error is
        // field-specific and immediate. Local mode allows alphanumeric plus
        // `-_.` (the local substrate sanitizes the id), but warn if the name
        // contains chars that would be stripped.
        let trimmed_name = name.trim();
        if is_local {
            if trimmed_name.is_empty() {
                self.author.status = Some("Name is required.".into());
                cx.notify();
                return;
            }
            let has_strippable = trimmed_name
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'));
            if has_strippable {
                self.author.status = Some(
                    "Name contains characters that will be stripped on the local \
                     substrate (allowed: letters, digits, -, _, .)."
                        .into(),
                );
                cx.notify();
                return;
            }
        } else {
            let len = trimmed_name.chars().count();
            let valid = (3..=64).contains(&len)
                && trimmed_name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid {
                self.author.status = Some(
                    "Name must be 3-64 chars: lowercase letters, digits, underscores only \
                     (ABW slug rule)."
                        .into(),
                );
                cx.notify();
                return;
            }
        }
        let agent_type = self.author.agent_type.clone();
        // The selector enforces the agent_type, but double-check — a stale
        // form state (e.g. a future refactor) must not silently send an
        // invalid type to the server.
        if !matches!(agent_type.as_str(), "research" | "creative" | "meta") {
            self.author.status =
                Some("Agent type must be one of: research, creative, meta.".into());
            cx.notify();
            return;
        }
        let tags = Self::comma_list(&self.author.tags.read(cx).text(cx));
        // fermi contract fields: sample queries (one per line — they contain
        // commas) and the accepts/produces composition ports (CSV).
        let sample_queries: Vec<String> = self
            .author
            .sample_queries
            .read(cx)
            .text(cx)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let accepts = Self::comma_list(&self.author.accepts.read(cx).text(cx));
        let produces = Self::comma_list(&self.author.produces.read(cx).text(cx));
        let visibility = self.author.visibility.clone();
        let valence = self.gather_valence(cx);
        self.author.busy = true;
        self.author.status = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = if is_local {
                invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_create_local_agent",
                        json!({
                            "agent_id": name.trim(),
                            "agent_type": agent_type,
                            "system_prompt": system_prompt.trim(),
                            "description": description.trim(),
                            "tags": tags,
                            "visibility": visibility,
                            "valence": valence,
                            "sample_queries": sample_queries,
                            "accepts": accepts,
                            "produces": produces,
                        }),
                    )
                    .await
            } else {
                invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_create_agent",
                        json!({
                            "agent_name": name.trim(),
                            "agent_type": agent_type,
                            "system_prompt": system_prompt.trim(),
                            "description": description.trim(),
                            "tags": tags,
                            "visibility": visibility,
                            "valence": valence,
                            "sample_queries": sample_queries,
                            "accepts": accepts,
                            "produces": produces,
                        }),
                    )
                    .await
            };
            this.update(cx, |this, cx| {
                this.author.busy = false;
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.spend.wallet_balance = Some(b);
                        }
                        // Surface the server's honest-drop note (cloud create
                        // with fields the ABW API cannot store) instead of a
                        // bare "created" that hides the loss.
                        let mut status = format!("Agent '{}' created.", name.trim());
                        if let Some(note) = extract_unsupported_fields_note(&output) {
                            status.push(' ');
                            status.push_str(&note);
                        }
                        this.author.status = Some(status.into());
                        // Refresh so the new agent appears in browse.
                        this.fetch_all(cx);
                    }
                    Err(err) => {
                        this.author.status = Some(format!("Create failed: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
