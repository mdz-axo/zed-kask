//! The AI Assist / validation surface: the cross-form `swarm_ai_assist`
//! integration shared by the Author and Compose forms. Extracted from
//! `swarm_panel.rs` — the methods and render helpers stay methods on
//! `SwarmPanel` (they mutate panel state via `cx.spawn` + `this.update`);
//! this module owns the mode-derivation policy, the state types, the tool
//! invocation, and the banner rendering. The Compose surface's Xaman Ek
//! consultant (`ask_xaman`) lives in `compose.rs` — it mutates ComposeForm
//! state, not this module's state.

use gpui::{Context, Window};
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;
use ui::{Button, Color, Tooltip, prelude::*};

use crate::CreateTarget;
use crate::PanelMode;
use crate::SWARM_SERVER;
use crate::SwarmPanel;

/// The backend mode string sent to `swarm_ai_assist` for a form surface.
/// Reads ONLY the named surface's own target toggle: a tuple-match that also
/// consulted the author form's target for the swarm surface would send ABW
/// guidance to a Local compose (and vice versa). Pinned by tests including
/// the exact hole (`swarm` + compose Cloud + author Local → `abw`).
fn ai_assist_mode(
    surface: &str,
    compose_target: CreateTarget,
    author_target: CreateTarget,
) -> &'static str {
    let target = if surface == "swarm" {
        compose_target
    } else {
        author_target
    };
    if target == CreateTarget::Local {
        "local"
    } else {
        "abw"
    }
}

/// R2: AI Assist / validation state — shared by the Author and Compose
/// surfaces (suggestions + validation verdict from `swarm_ai_assist`).
#[derive(Default)]
pub(crate) struct AiAssistState {
    busy: bool,
    action: Option<String>,
    suggestions: Option<AiSuggestions>,
    validation: Option<ValidationResult>,
}

/// AI Assist suggestion result (action: "suggest"). Each field is a suggested
/// completion for the corresponding form field; an empty string means the field
/// was already filled or the model had no suggestion. `surface` records which
/// form the suggestions target so the Author banner doesn't render in Compose
/// (and vice versa).
#[derive(Clone, Debug)]
struct AiSuggestions {
    surface: String,
    name: String,
    agent_type: String,
    description: String,
    system_prompt: String,
    mission: String,
    agents: String,
}

/// AI Assist validation verdict (action: "validate"). `valid` is the model's
/// well-formedness check; `issues` lists the problems when `valid` is false.
/// `surface` gates which form's banner renders.
#[derive(Clone, Debug)]
struct ValidationResult {
    surface: String,
    valid: bool,
    issues: Vec<String>,
    /// Advisory findings (fermi Warning severity) — reported, never
    /// blocking. Includes the typed-tier notice and the LLM quality review.
    warnings: Vec<String>,
    /// Carrier for out-of-band notes (e.g. "advisory layer unavailable").
    notes: String,
}

impl SwarmPanel {
    // ── AI Assist ─────────────────────────────────────────────────────────
    //
    // The Author and Compose surfaces call the `swarm_ai_assist` MCP tool for
    // two purposes: `action: "suggest"` asks the default model to propose
    // completions for empty or partial fields (offered as an Apply banner),
    // and `action: "validate"` runs a well-formedness check before create
    // (offered as a validation banner). The panel only reads editors here;
    // `apply_ai_suggestions` (which writes editors) takes `&mut Window`.

    /// Call `swarm_ai_assist` with the current form fields. `action` is either
    /// `"suggest"` or `"validate"`. The surface ("agent" / "swarm") is derived
    /// from `self.mode`; only Author and Compose are wired (Browse/Steer are
    /// ignored). Stores the result in `ai_assist_suggestions` or
    /// `validation_result` for the surface's banner to render.
    fn ai_assist(&mut self, action: &str, cx: &mut Context<Self>) {
        let surface = match self.mode {
            PanelMode::Author => "agent",
            PanelMode::Compose => "swarm",
            // AI Assist is only wired for Author and Compose — a call from
            // another mode is a no-op rather than a panic.
            _ => return,
        };
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        // The backend mode sent to `swarm_ai_assist` must match the surface's
        // own target toggle — reading the author form's target for the swarm
        // surface sent ABW guidance to a Local compose (and vice versa).
        let mode = ai_assist_mode(
            surface,
            self.compose.create_target,
            self.author.create_target,
        );

        let (name, agent_type, description, system_prompt, mission, agents) = if surface == "agent"
        {
            (
                self.author.name.read(cx).text(cx),
                self.author.agent_type.clone(),
                self.author.description.read(cx).text(cx),
                self.author.system_prompt.read(cx).text(cx),
                String::new(),
                String::new(),
            )
        } else {
            (
                self.compose.name.read(cx).text(cx),
                String::new(),
                String::new(),
                String::new(),
                self.compose.mission.read(cx).text(cx),
                self.compose.agents.read(cx).text(cx),
            )
        };
        // Agent-surface contract fields (fermi `agent_contract`): tags,
        // sample queries, accepts/produces, valence presence. Sent on both
        // actions so `suggest` can propose them and `validate` can check
        // them against the deterministic contract.
        let (tags, sample_queries, accepts, produces, has_valence) = if surface == "agent" {
            let arousal = self.author.valence_arousal.read(cx).text(cx);
            let valence = self.author.valence_valence.read(cx).text(cx);
            let affect = self.author.valence_primary_affect.read(cx).text(cx);
            let traits = self.author.valence_personality_traits.read(cx).text(cx);
            (
                self.author.tags.read(cx).text(cx),
                self.author.sample_queries.read(cx).text(cx),
                self.author.accepts.read(cx).text(cx),
                self.author.produces.read(cx).text(cx),
                !arousal.trim().is_empty()
                    || !valence.trim().is_empty()
                    || !affect.trim().is_empty()
                    || !traits.trim().is_empty(),
            )
        } else {
            (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                false,
            )
        };

        self.ai_assist.busy = true;
        self.ai_assist.action = Some(action.to_string());
        // Clear stale banners so the operator doesn't see the previous result
        // while a new call is in flight (mirrors the Xaman Ek stale-suggestion
        // fix, L5).
        self.ai_assist.suggestions = None;
        self.ai_assist.validation = None;
        cx.notify();

        let surface_owned = surface.to_string();
        let action_owned = action.to_string();
        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_ai_assist",
                    json!({
                        "action": action_owned,
                        "surface": surface_owned,
                        "mode": mode,
                        "name": name,
                        "agent_type": agent_type,
                        "description": description,
                        "system_prompt": system_prompt,
                        "mission": mission,
                        "agents": agents,
                        "tags": tags,
                        "sample_queries": sample_queries,
                        "accepts": accepts,
                        "produces": produces,
                        "has_valence": has_valence,
                    }),
                )
                .await;
            this.update(cx, |this, cx| {
                this.ai_assist.busy = false;
                this.ai_assist.action = None;
                match result {
                    Ok(output) => {
                        if let Some(content) = parse_tool_response(&output) {
                            if action_owned == "suggest" {
                                let s = content.get("suggestions");
                                let pick = |key: &str| {
                                    s.and_then(|s| s.get(key))
                                        .and_then(|v| v.as_str())
                                        .map(str::to_string)
                                        .unwrap_or_default()
                                };
                                this.ai_assist.suggestions = Some(AiSuggestions {
                                    surface: surface_owned.clone(),
                                    name: pick("name"),
                                    agent_type: pick("agent_type"),
                                    description: pick("description"),
                                    system_prompt: pick("system_prompt"),
                                    mission: pick("mission"),
                                    agents: pick("agents"),
                                });
                            } else {
                                let valid = content
                                    .get("valid")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let issues = content
                                    .get("issues")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(str::to_string))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                // Advisory tier (fermi Warning severity): reported
                                // but never blocking. Includes the deterministic
                                // typed-tier notice and the LLM's quality review.
                                let warnings = content
                                    .get("warnings")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(str::to_string))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let notes = content
                                    .get("notes")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                this.ai_assist.validation = Some(ValidationResult {
                                    surface: surface_owned.clone(),
                                    valid,
                                    issues,
                                    warnings,
                                    notes,
                                });
                            };
                        }
                    }
                    Err(err) => {
                        // Surface the error on the active form's status line so
                        // the operator gets feedback (mirrors create_agent).
                        let msg = format!("AI Assist unavailable: {err}");
                        if surface_owned == "agent" {
                            this.author.status = Some(msg.into());
                        } else {
                            this.compose.status = Some(msg.into());
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Apply the stored AI Assist suggestions to the form editors. Each
    /// non-empty suggestion overwrites the corresponding field. For the agent
    /// surface, `agent_type` is only applied when it is a valid selector value
    /// (research/creative/meta). Clears `ai_assist_suggestions` after applying.
    /// Requires `&mut Window` because `Editor::set_text` needs it.
    fn apply_ai_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.ai_assist.suggestions.clone() else {
            return;
        };
        if s.surface == "agent" {
            if !s.name.is_empty() {
                let editor = self.author.name.clone();
                editor.update(cx, |e, cx| e.set_text(s.name, window, cx));
            }
            if !s.description.is_empty() {
                let editor = self.author.description.clone();
                editor.update(cx, |e, cx| e.set_text(s.description, window, cx));
            }
            if !s.system_prompt.is_empty() {
                let editor = self.author.system_prompt.clone();
                editor.update(cx, |e, cx| e.set_text(s.system_prompt, window, cx));
            }
            if matches!(s.agent_type.as_str(), "research" | "creative" | "meta") {
                self.author.agent_type = s.agent_type;
            }
        } else if s.surface == "swarm" {
            if !s.name.is_empty() {
                let editor = self.compose.name.clone();
                editor.update(cx, |e, cx| e.set_text(s.name, window, cx));
            }
            if !s.mission.is_empty() {
                let editor = self.compose.mission.clone();
                editor.update(cx, |e, cx| e.set_text(s.mission, window, cx));
            }
            if !s.agents.is_empty() {
                let editor = self.compose.agents.clone();
                editor.update(cx, |e, cx| e.set_text(s.agents, window, cx));
            }
        }
        self.ai_assist.suggestions = None;
        cx.notify();
    }

    /// Dismiss the AI Assist suggestions banner without applying.
    fn dismiss_ai_suggestions(&mut self, cx: &mut Context<Self>) {
        self.ai_assist.suggestions = None;
        cx.notify();
    }

    /// Dismiss the validation banner.
    fn dismiss_validation(&mut self, cx: &mut Context<Self>) {
        self.ai_assist.validation = None;
        cx.notify();
    }

    // ── AI Assist render helpers ───────────────────────────────────────
    //
    // `render_ai_assist_row` is the two-button row (AI Assist / Validate)
    // shown on both the Author and Compose surfaces. `render_ai_suggestions_banner`
    // and `render_validation_banner` mirror `render_publish_banner`: a bordered
    // box with Apply/Dismiss (suggestions) or a success/issues list (validation).
    // Each banner is gated by its `surface` field so the Author banner does not
    // render in Compose and vice versa.

    /// The AI Assist button row for a form surface ("agent" or "swarm"). Shown
    /// below the fields and above the Create button. Both buttons are disabled
    /// while the form's create is in flight or an AI Assist call is in flight.
    pub(crate) fn render_ai_assist_row(
        &self,
        surface: &str,
        form_busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let disabled = form_busy || self.ai_assist.busy;
        let busy_label = match self.ai_assist.action.as_deref() {
            Some("validate") => "Validating…",
            Some("suggest") => "Assisting…",
            _ => "",
        };
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                Button::new(format!("ai-assist-{surface}"), "AI Assist")
                    .style(ButtonStyle::Subtle)
                    .label_size(LabelSize::XSmall)
                    .disabled(disabled)
                    .tooltip(Tooltip::text(
                        "Uses the default model to suggest completions for empty or \
                         partial fields based on ABW/Local composition guidance.",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ai_assist("suggest", cx);
                    })),
            )
            .child(
                Button::new(format!("ai-validate-{surface}"), "Validate")
                    .style(ButtonStyle::Subtle)
                    .label_size(LabelSize::XSmall)
                    .disabled(disabled)
                    .tooltip(Tooltip::text(
                        "Runs the inputs through the default model to check \
                         well-formedness and surface issues before creating.",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ai_assist("validate", cx);
                    })),
            )
            .when(!busy_label.is_empty(), |this| {
                this.child(
                    Label::new(busy_label)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
    }

    /// The AI Assist suggestions banner for a form surface. Returns `None`
    /// when there are no suggestions or they target a different surface.
    pub(crate) fn render_ai_suggestions_banner(
        &self,
        surface: &str,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let s = self.ai_assist.suggestions.clone()?;
        if s.surface != surface {
            return None;
        }
        let border = cx.theme().colors().border;
        // Collect (label, value) pairs for the non-empty suggestions so the
        // operator sees exactly which fields would change.
        let fields: Vec<(&'static str, String)> = [
            ("Name", s.name.clone()),
            ("Agent type", s.agent_type.clone()),
            ("Description", s.description.clone()),
            ("System prompt", s.system_prompt.clone()),
            ("Mission", s.mission.clone()),
            ("Agents", s.agents),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .collect();
        if fields.is_empty() {
            // No suggestions to apply — show a note instead of an empty banner.
            return Some(
                v_flex()
                    .w_full()
                    .gap_2()
                    .p_3()
                    .rounded_sm()
                    .border_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Label::new("AI Assist").color(Color::Accent))
                            .child(
                                Label::new("No suggestions — the fields look complete.")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        h_flex().gap_2().items_center().child(div().flex_1()).child(
                            Button::new(format!("dismiss-ai-sug-empty-{surface}"), "Dismiss")
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_ai_suggestions(cx);
                                })),
                        ),
                    ),
            );
        }
        Some(
            v_flex()
                .w_full()
                .gap_2()
                .p_3()
                .rounded_sm()
                .border_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Label::new("AI Assist").color(Color::Accent))
                        .child(
                            Label::new("Suggested completions:")
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                )
                .child(
                    v_flex()
                        .gap_0p5()
                        .children(fields.into_iter().map(|(label, value)| {
                            // Truncate long suggestion previews so a full
                            // system-prompt draft doesn't blow out the panel height.
                            let preview = if value.chars().count() > 120 {
                                let truncated: String = value.chars().take(120).collect();
                                format!("• {label}: {truncated}…")
                            } else {
                                format!("• {label}: {value}")
                            };
                            Label::new(preview)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                        })),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1())
                        .child(
                            Button::new(format!("apply-ai-sug-{surface}"), "Apply")
                                .style(ButtonStyle::Filled)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_ai_suggestions(window, cx);
                                })),
                        )
                        .child(
                            Button::new(format!("dismiss-ai-sug-{surface}"), "Dismiss")
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_ai_suggestions(cx);
                                })),
                        ),
                ),
        )
    }

    /// The validation banner for a form surface. Returns `None` when there is
    /// no result or it targets a different surface. Shows a success label when
    /// `valid`, or the issues list (Warning color) when not.
    pub(crate) fn render_validation_banner(
        &self,
        surface: &str,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let v = self.ai_assist.validation.clone()?;
        if v.surface != surface {
            return None;
        }
        let border = cx.theme().colors().border;
        let header = if v.valid {
            "Validation passed"
        } else {
            "Validation found issues"
        };
        Some(
            v_flex()
                .w_full()
                .gap_2()
                .p_3()
                .rounded_sm()
                .border_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Label::new(header).color(if v.valid {
                            Color::Accent
                        } else {
                            Color::Warning
                        }))
                        .when(v.valid, |this| {
                            this.child(
                                Label::new("Meets the ABW composition contract.")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                        }),
                )
                .when(!v.valid, |this| {
                    this.child(v_flex().gap_0p5().children(v.issues.iter().map(|issue| {
                        Label::new(format!("• {issue}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Warning)
                    })))
                })
                // Advisory tier — fermi Warning severity: worth fixing, never
                // blocking. Rendered muted so the visual weight stays on the
                // contract failures above.
                .when(!v.warnings.is_empty(), |this| {
                    this.child(
                        v_flex()
                            .gap_0p5()
                            .children(v.warnings.iter().map(|warning| {
                                Label::new(format!("◦ {warning}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                            })),
                    )
                })
                .when(!v.notes.is_empty(), |this| {
                    this.child(
                        Label::new(v.notes.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                })
                .child(
                    h_flex().gap_2().items_center().child(div().flex_1()).child(
                        Button::new(format!("dismiss-validation-{surface}"), "Dismiss")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismiss_validation(cx);
                            })),
                    ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AI Assist mode derivation ──────────────────────────────────────────
    //
    // `swarm_ai_assist` must be told the backend of the surface it is
    // advising. The original inline tuple-match had a hole: the arm
    // `(_, _, CreateTarget::Local)` let the AUTHOR form's target win for the
    // swarm surface, so a Local author form + Cloud compose sent "local"
    // guidance to a Cloud compose. These tests pin the surface-specific read.

    #[test]
    fn ai_assist_mode_reads_only_the_named_surface() {
        // The hole: swarm surface must ignore the author form's target.
        assert_eq!(
            ai_assist_mode("swarm", CreateTarget::Cloud, CreateTarget::Local),
            "abw"
        );
        // And symmetrically: agent surface must ignore the compose form's target.
        assert_eq!(
            ai_assist_mode("agent", CreateTarget::Local, CreateTarget::Cloud),
            "abw"
        );
        // Each surface reads its own target.
        assert_eq!(
            ai_assist_mode("swarm", CreateTarget::Local, CreateTarget::Cloud),
            "local"
        );
        assert_eq!(
            ai_assist_mode("agent", CreateTarget::Cloud, CreateTarget::Local),
            "local"
        );
        // Cloud on both surfaces.
        assert_eq!(
            ai_assist_mode("swarm", CreateTarget::Cloud, CreateTarget::Cloud),
            "abw"
        );
        assert_eq!(
            ai_assist_mode("agent", CreateTarget::Cloud, CreateTarget::Cloud),
            "abw"
        );
    }
}
