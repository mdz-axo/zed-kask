//! Read-only, swarm-scoped member history beside the Curator Steer conversation.
use gpui::{Context, IntoElement};
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
use serde::Deserialize;
use serde_json::json;
use ui::{Color, Label, LabelSize, prelude::*};

use crate::{PanelMode, SWARM_SERVER, SwarmPanel, shared_tool_invoker};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct MemberTurn {
    sequence: i64,
    agent_id: String,
    task: String,
    response: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct MemberThread {
    swarm_id: String,
    turns: Vec<MemberTurn>,
    archived: bool,
}

fn parse_member_thread(output: &str, expected_id: &str) -> Result<MemberThread, String> {
    if let Some(error) = parse_tool_error(output) {
        return Err(format!(
            "Member turns: {} ({:?})",
            error.message, error.kind
        ));
    }
    let content = parse_tool_response(output)
        .ok_or_else(|| "Member turns: invalid tool response".to_string())?;
    let thread: MemberThread = serde_json::from_value(content)
        .map_err(|error| format!("Member turns: invalid response: {error}"))?;
    if thread.swarm_id != expected_id {
        return Err(format!(
            "Member turns: returned swarm id does not match selected swarm ({expected_id})"
        ));
    }
    if thread
        .turns
        .windows(2)
        .any(|pair| pair[0].sequence >= pair[1].sequence)
    {
        return Err("Member turns: server returned out-of-order sequence".to_string());
    }
    Ok(thread)
}

#[derive(Default)]
pub(super) struct MemberTurnsState {
    selected_id: Option<String>,
    generation: u64,
    loading: bool,
    error: Option<String>,
    turns: Vec<MemberTurn>,
    archived: bool,
}

impl MemberTurnsState {
    pub(super) fn clear(&mut self) {
        self.selected_id = None;
        self.generation = self.generation.wrapping_add(1);
        self.loading = false;
        self.error = None;
        self.turns.clear();
        self.archived = false;
    }

    fn start(&mut self, swarm_id: String) -> u64 {
        self.clear();
        self.selected_id = Some(swarm_id);
        self.loading = true;
        self.generation
    }

    fn finish(&mut self, swarm_id: &str, generation: u64, result: Result<MemberThread, String>) {
        if self.selected_id.as_deref() != Some(swarm_id) || self.generation != generation {
            return;
        }
        self.loading = false;
        match result {
            Ok(thread) => {
                self.turns = thread.turns;
                self.archived = thread.archived;
                self.error = None;
            }
            Err(error) => {
                self.turns.clear();
                self.error = Some(error);
            }
        }
    }
}

fn selected_member_id<'a>(
    local_mode: bool,
    selected_workspace: Option<&'a str>,
    selected_local_swarm: Option<&'a str>,
) -> Option<&'a str> {
    local_mode
        .then_some(selected_local_swarm)
        .flatten()
        .filter(|id| selected_workspace == Some(*id))
}

impl SwarmPanel {
    pub(crate) fn select_swarm_for_steer(
        &mut self,
        id: String,
        local: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = self.selected_workspace.as_deref() != Some(&id)
            || self.selected_local_swarm.is_some() != local;
        self.selected_workspace = Some(id.clone());
        self.selected_local_swarm = local.then_some(id);
        if changed {
            self.steer.invalidate();
            self.member_turns.clear();
            if self.mode == PanelMode::Steer {
                self.refresh_member_turns(cx);
            }
        }
        cx.notify();
    }

    fn selected_member_swarm(&self) -> Option<&str> {
        selected_member_id(
            self.last_swarm_mode == Some(kask_bridge::SwarmModeConfig::Local),
            self.selected_workspace.as_deref(),
            self.selected_local_swarm.as_deref(),
        )
    }

    pub(super) fn refresh_member_turns(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_member_swarm().map(str::to_owned) else {
            self.member_turns.clear();
            cx.notify();
            return;
        };
        let generation = self.member_turns.start(id.clone());
        let Some(invoker) = shared_tool_invoker() else {
            self.member_turns.finish(
                &id,
                generation,
                Err("Member turns: tool invoker not wired".into()),
            );
            cx.notify();
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_thread_local",
                    json!({ "swarm_id": id }),
                )
                .await;
            let parsed = match result {
                Ok(output) => parse_member_thread(&output, &id),
                Err(error) => Err(format!("Member turns: fetch failed: {error}")),
            };
            if let Err(error) = this.update(cx, |this, cx| {
                if this.selected_member_swarm() == Some(id.as_str()) {
                    this.member_turns.finish(&id, generation, parsed);
                    cx.notify();
                }
            }) {
                log::debug!(
                    "swarm-panel: member turns result discarded after panel closed: {error}"
                );
            }
        })
        .detach();
    }

    pub(super) fn render_member_turns(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self.selected_member_swarm().map(str::to_owned);
        let selection_note = if selected.is_some() {
            selected.clone().unwrap_or_default()
        } else if self.selected_local_swarm.is_some() {
            "Switch to local mode to view the selected swarm".to_string()
        } else {
            "Select a local swarm via Browse → Edit".to_string()
        };
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .child(Label::new("Local member turns").size(LabelSize::Small))
                    .child(
                        Label::new(selection_note)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .truncate(),
                    ),
            )
            .when(selected.is_some(), |section| {
                section.child(
                    h_flex().w_full().py_1().child(
                        ui::Button::new("refresh-member-turns", "Refresh turns")
                            .style(ui::ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_member_turns(cx))),
                    ),
                )
            })
            .when(self.member_turns.loading && selected.is_some(), |section| {
                section.child(
                    Label::new("Loading member turns…")
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            .when_some(
                self.member_turns
                    .error
                    .clone()
                    .filter(|_| selected.is_some()),
                |section, error| {
                    section.child(
                        Label::new(error)
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    )
                },
            )
            .when(
                self.member_turns.archived && selected.is_some(),
                |section| {
                    section.child(
                        Label::new("Archived swarm")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                },
            )
            .when(
                selected.is_some()
                    && !self.member_turns.loading
                    && self.member_turns.error.is_none()
                    && self.member_turns.turns.is_empty(),
                |section| {
                    section.child(
                        Label::new("No member turns recorded.")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                },
            )
            .when(
                !self.member_turns.turns.is_empty() && selected.is_some(),
                |section| {
                    section.child(
                        v_flex()
                            .id("local-member-turn-scroll")
                            .w_full()
                            .min_w_0()
                            .max_h(ui::rems_from_px(180.0_f32))
                            .overflow_y_scroll()
                            .overflow_x_hidden()
                            .gap_2()
                            .children(self.member_turns.turns.iter().map(|turn| {
                                v_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        Label::new(format!(
                                            "#{} · {} · {}",
                                            turn.sequence, turn.agent_id, turn.created_at
                                        ))
                                        .size(LabelSize::XSmall)
                                        .truncate(),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .min_w_0()
                                            .text_xs()
                                            .whitespace_normal()
                                            .child(format!("Task: {}", turn.task)),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .min_w_0()
                                            .text_xs()
                                            .whitespace_normal()
                                            .child(turn.response.clone()),
                                    )
                            })),
                    )
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ordered_turns_and_surfaces_server_and_shape_errors() {
        let payload = r#"{"content":{"swarm_id":"a","turns":[{"sequence":1,"agent_id":"one","task":"plan","response":"done","created_at":"now"},{"sequence":2,"agent_id":"two","task":"review","response":"ok","created_at":"later"}],"archived":true}}"#;
        let thread = parse_member_thread(payload, "a").expect("valid member thread");
        assert_eq!(
            thread
                .turns
                .iter()
                .map(|t| t.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
        assert!(thread.archived);
        assert!(parse_member_thread(payload, "b").is_err());
        assert!(parse_member_thread(r#"{"content":{"swarm_id":"a","turns":[{"sequence":1,"agent_id":"one","task":"x","response":"y","created_at":"now"},{"sequence":1,"agent_id":"two","task":"x","response":"y","created_at":"now"}],"archived":false}}"#, "a").is_err());
        assert!(parse_member_thread(r#"{"content":{"swarm_id":"a","turns":[]}}"#, "a").is_err());
        let err = parse_member_thread(r#"{"error":"not found","kind":"not_found"}"#, "a")
            .expect_err("server error");
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn scope_switch_and_new_refresh_drop_stale_results() {
        let mut state = MemberTurnsState::default();
        let old = state.start("a".into());
        let new = state.start("b".into());
        state.finish(
            "a",
            old,
            parse_member_thread(
                r#"{"content":{"swarm_id":"a","turns":[],"archived":false}}"#,
                "a",
            ),
        );
        assert!(state.loading);
        assert_eq!(state.selected_id.as_deref(), Some("b"));
        let newer = state.start("b".into());
        state.finish("b", new, Err("stale error".into()));
        assert!(state.loading);
        state.finish(
            "b",
            newer,
            parse_member_thread(
                r#"{"content":{"swarm_id":"b","turns":[],"archived":false}}"#,
                "b",
            ),
        );
        assert!(!state.loading);
        assert!(state.error.is_none());
        state.clear();
        state.finish("b", newer, Err("late".into()));
        assert!(state.error.is_none());
    }

    #[test]
    fn only_the_selected_local_id_in_local_mode_can_be_read() {
        assert_eq!(selected_member_id(true, Some("a"), Some("a")), Some("a"));
        assert_eq!(selected_member_id(false, Some("a"), Some("a")), None);
        assert_eq!(selected_member_id(true, Some("cloud"), Some("a")), None);
        assert_eq!(selected_member_id(true, None, Some("a")), None);
    }

    #[test]
    fn member_thread_tool_is_advertised_by_server() {
        assert!(crate::parse::SWARM_TOOLS.contains(&"swarm_thread_local"));
    }
}
