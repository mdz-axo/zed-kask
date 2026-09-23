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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ThreadChoice {
    swarm_id: String,
    turn_count: usize,
    archived: bool,
}

#[derive(Deserialize)]
struct ThreadChoices {
    threads: Vec<ThreadChoice>,
}

fn parse_thread_choices(output: &str) -> Result<Vec<ThreadChoice>, String> {
    if let Some(error) = parse_tool_error(output) {
        return Err(format!(
            "History choices: {} ({:?})",
            error.message, error.kind
        ));
    }
    let content = parse_tool_response(output)
        .ok_or_else(|| "History choices: invalid tool response".to_string())?;
    let choices: ThreadChoices = serde_json::from_value(content)
        .map_err(|error| format!("History choices: invalid response: {error}"))?;
    if choices
        .threads
        .iter()
        .any(|choice| choice.swarm_id.trim().is_empty())
    {
        return Err("History choices: empty swarm id".into());
    }
    let mut ids = std::collections::HashSet::new();
    if choices
        .threads
        .iter()
        .any(|choice| !ids.insert(&choice.swarm_id))
    {
        return Err("History choices: duplicate swarm id".into());
    }
    Ok(choices
        .threads
        .into_iter()
        .filter(|choice| choice.archived)
        .collect())
}

#[derive(Default)]
pub(super) struct MemberTurnsState {
    archived_choice: Option<String>,
    choices: Vec<ThreadChoice>,
    choices_generation: u64,
    choices_loading: bool,
    choices_error: Option<String>,
    selected_id: Option<String>,
    generation: u64,
    loading: bool,
    error: Option<String>,
    turns: Vec<MemberTurn>,
    archived: bool,
}

impl MemberTurnsState {
    fn start_choices(&mut self) -> u64 {
        self.choices_generation = self.choices_generation.wrapping_add(1);
        self.choices_loading = true;
        self.choices_error = None;
        self.choices_generation
    }

    fn finish_choices(&mut self, generation: u64, result: Result<Vec<ThreadChoice>, String>) {
        if self.choices_generation != generation {
            return;
        }
        self.choices_loading = false;
        match result {
            Ok(choices) => {
                if self
                    .archived_choice
                    .as_ref()
                    .is_some_and(|id| !choices.iter().any(|choice| &choice.swarm_id == id))
                {
                    self.archived_choice = None;
                    self.clear();
                }
                self.choices = choices;
                self.choices_error = None;
            }
            Err(error) => self.choices_error = Some(error),
        }
    }

    fn choose_archive(&mut self, id: Option<String>) {
        if self.archived_choice != id {
            self.archived_choice = id;
            self.clear();
        }
    }

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

fn inspected_member_id<'a>(
    archived_id: Option<&'a str>,
    active_id: Option<&'a str>,
) -> Option<&'a str> {
    archived_id.or(active_id)
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
            self.member_turns.choose_archive(None);
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

    fn inspected_member_swarm(&self) -> Option<&str> {
        inspected_member_id(
            self.member_turns.archived_choice.as_deref(),
            self.selected_member_swarm(),
        )
    }

    pub(super) fn refresh_thread_choices(&mut self, cx: &mut Context<Self>) {
        let generation = self.member_turns.start_choices();
        let Some(invoker) = shared_tool_invoker() else {
            self.member_turns.finish_choices(
                generation,
                Err("History choices: tool invoker not wired".into()),
            );
            cx.notify();
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(SWARM_SERVER, "swarm_list_local_threads", json!({}))
                .await;
            let parsed = match result {
                Ok(output) => parse_thread_choices(&output),
                Err(error) => Err(format!("History choices: fetch failed: {error}")),
            };
            if let Err(error) = this.update(cx, |this, cx| {
                this.member_turns.finish_choices(generation, parsed);
                // A vanished archived choice falls back to the active swarm.
                if this.member_turns.selected_id.as_deref() != this.inspected_member_swarm() {
                    this.refresh_member_turns(cx);
                }
                cx.notify();
            }) {
                log::debug!("swarm-panel: history choices discarded after panel closed: {error}");
            }
        })
        .detach();
    }

    fn select_archived_thread(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.member_turns.choose_archive(id);
        self.refresh_member_turns(cx);
    }

    pub(super) fn refresh_member_turns(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.inspected_member_swarm().map(str::to_owned) else {
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
                if this.inspected_member_swarm() == Some(id.as_str()) {
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
        let selected = self.inspected_member_swarm().map(str::to_owned);
        let selection_note = if let Some(id) = self.member_turns.archived_choice.as_ref() {
            format!("Archived history · {id} (read only)")
        } else if let Some(id) = selected.as_ref() {
            id.clone()
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
            .child(
                h_flex().w_full().min_w_0().py_1().child(
                    ui::Button::new("refresh-history-choices", "Refresh history choices")
                        .style(ui::ButtonStyle::Subtle)
                        .label_size(LabelSize::XSmall)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_thread_choices(cx))),
                ),
            )
            .when(self.member_turns.choices_loading, |section| {
                section.child(
                    Label::new("Loading history choices…")
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            .when_some(self.member_turns.choices_error.clone(), |section, error| {
                section.child(
                    Label::new(error)
                        .size(LabelSize::XSmall)
                        .color(Color::Warning),
                )
            })
            .when(
                !self.member_turns.choices_loading
                    && self.member_turns.choices_error.is_none()
                    && self.member_turns.choices.is_empty(),
                |section| {
                    section.child(
                        Label::new("No archived threads recorded.")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                },
            )
            .when(!self.member_turns.choices.is_empty(), |section| {
                section.child(
                    v_flex()
                        .id("archived-thread-choices-scroll")
                        .w_full()
                        .min_w_0()
                        .max_h(ui::rems_from_px(110.0_f32))
                        .overflow_y_scroll()
                        .overflow_x_hidden()
                        .children(self.member_turns.choices.iter().map(|choice| {
                            let id = choice.swarm_id.clone();
                            let short_id: String = id.chars().take(24).collect();
                            let label = if id.chars().count() > 24 {
                                format!("{short_id}… · {} turns", choice.turn_count)
                            } else {
                                format!("{short_id} · {} turns", choice.turn_count)
                            };
                            h_flex().w_full().min_w_0().child(
                                ui::Button::new(
                                    gpui::SharedString::from(format!(
                                        "archive-choice-{}",
                                        choice.swarm_id
                                    )),
                                    label,
                                )
                                .style(ui::ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.select_archived_thread(Some(id.clone()), cx);
                                    },
                                )),
                            )
                        })),
                )
            })
            .when(self.member_turns.archived_choice.is_some(), |section| {
                section.child(
                    h_flex().w_full().min_w_0().child(
                        ui::Button::new("view-current-member-turns", "View current swarm")
                            .style(ui::ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.select_archived_thread(None, cx)),
                            ),
                    ),
                )
            })
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
    fn history_choices_parse_archived_only_and_reject_bad_responses() {
        let choices = parse_thread_choices(r#"{"content":{"threads":[{"swarm_id":"deleted","turn_count":2,"archived":true},{"swarm_id":"live","turn_count":3,"archived":false}]}}"#)
            .expect("valid list");
        assert_eq!(
            choices,
            vec![ThreadChoice {
                swarm_id: "deleted".into(),
                turn_count: 2,
                archived: true
            }]
        );
        for invalid in [
            r#"{"content":{"threads":[{"swarm_id":"missing-count","archived":true}]}}"#,
            r#"{"content":{"threads":[{"swarm_id":"x","turn_count":-1,"archived":true}]}}"#,
            r#"{"content":{"threads":[{"swarm_id":" ","turn_count":0,"archived":true}]}}"#,
            r#"{"content":{"threads":[{"swarm_id":"x","turn_count":0,"archived":true},{"swarm_id":"x","turn_count":1,"archived":false}]}}"#,
            r#"{"content":{"wrong":[]}}"#,
            "not json",
        ] {
            assert!(parse_thread_choices(invalid).is_err(), "{invalid}");
        }
        let error = parse_thread_choices(r#"{"error":"storage offline","kind":"unavailable"}"#)
            .expect_err("server error must surface");
        assert!(error.contains("storage offline"), "{error}");
    }

    #[test]
    fn archived_selection_survives_turn_refresh_but_not_missing_list_entry() {
        let mut state = MemberTurnsState::default();
        let first = state.start_choices();
        state.finish_choices(
            first,
            parse_thread_choices(
                r#"{"content":{"threads":[{"swarm_id":"gone","turn_count":1,"archived":true}]}}"#,
            ),
        );
        state.choose_archive(Some("gone".into()));
        let turn_request = state.start("gone".into());
        state.finish(
            "gone",
            turn_request,
            parse_member_thread(
                r#"{"content":{"swarm_id":"gone","turns":[],"archived":true}}"#,
                "gone",
            ),
        );
        assert!(state.archived);
        assert_eq!(state.archived_choice.as_deref(), Some("gone"));
        let old_list = state.start_choices();
        let new_list = state.start_choices();
        state.finish_choices(old_list, Err("stale".into()));
        assert!(state.choices_loading);
        assert!(state.choices_error.is_none());
        state.finish_choices(new_list, Err("storage offline".into()));
        assert_eq!(state.archived_choice.as_deref(), Some("gone"));
        assert_eq!(state.choices_error.as_deref(), Some("storage offline"));
        let refreshed = state.start_choices();
        state.finish_choices(refreshed, Ok(vec![]));
        assert!(state.archived_choice.is_none());
        assert!(state.selected_id.is_none());
        assert!(!state.archived);
        state.finish("gone", turn_request, Err("late turn".into()));
        assert!(state.error.is_none());
    }

    #[test]
    fn archived_choice_is_not_an_executable_workspace_selection() {
        let mut state = MemberTurnsState::default();
        state.choose_archive(Some("deleted".into()));
        assert_eq!(state.archived_choice.as_deref(), Some("deleted"));
        let active = selected_member_id(true, Some("live"), Some("live"));
        assert_eq!(
            inspected_member_id(state.archived_choice.as_deref(), active),
            Some("deleted")
        );
        assert_eq!(active, Some("live"));
        assert_eq!(
            inspected_member_id(state.archived_choice.as_deref(), None),
            Some("deleted")
        );
        assert_eq!(selected_member_id(true, None, None), None);
        state.choose_archive(None);
        assert!(state.archived_choice.is_none());
        assert!(crate::parse::SWARM_TOOLS.contains(&"swarm_list_local_threads"));
    }

    #[test]
    fn member_thread_tool_is_advertised_by_server() {
        assert!(crate::parse::SWARM_TOOLS.contains(&"swarm_thread_local"));
    }
}
