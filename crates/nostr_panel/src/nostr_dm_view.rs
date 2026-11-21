//! Editor-only NIP-17 encrypted Direct Messages view (`NIP-44` + `NIP-59` Gift Wrap,
//! SQLCipher-backed via `kask_bridge::nostr_dm`).
//!
//! Never touches MCP servers, the loopback LMDB relay, or Steer LLM context.

use std::collections::BTreeMap;

use gpui::{
    AnyElement, Context, Entity, Focusable, IntoElement, Render, ScrollHandle, Task, Window,
};
use hkask_types::nostr_wire::NOSTR_CLIENT_SERVER;
use kask_bridge::KaskSettings;
use kask_bridge::nostr_dm::{self, DmMessage, DmThreadSummary};
use serde_json::json;
use settings::Settings as _;
use ui::{Label, LabelSize, WithScrollbar as _, prelude::*};
use util::ResultExt as _;

use crate::nostr_composer::{PUBLISH_LIST_TOOL, parse_publish_receipt, sanitize_untrusted_text};
use crate::nostr_viewer::{display_text, invoke, short_hex, utc_text};

/// Format a peer chip label using a resolved display name when known, falling
/// back to a truncated hex pubkey.
pub(crate) fn format_peer_label(
    peer_hex: &str,
    peer_names: &BTreeMap<String, String>,
    message_count: Option<usize>,
) -> String {
    let base = peer_names
        .get(peer_hex)
        .filter(|name| !name.trim().is_empty())
        .map(|name| format!("@{}", display_text(name)))
        .unwrap_or_else(|| short_hex(peer_hex));
    match message_count {
        Some(count) => format!("{base} ({count})"),
        None => base,
    }
}

/// Self-contained GPUI view owning all editor-only NIP-17 DM state.
pub(crate) struct NostrDmView {
    threads: Vec<DmThreadSummary>,
    selected_peer: Option<String>,
    messages: Vec<DmMessage>,
    status: Option<(bool, String)>,
    busy: bool,
    loaded_local: bool,
    peer_names: BTreeMap<String, String>,
    recipient_editor: Option<Entity<editor::Editor>>,
    input_editor: Option<Entity<editor::Editor>>,
    scroll_handle: ScrollHandle,
    _task: Option<Task<()>>,
}

impl NostrDmView {
    pub(crate) fn new() -> Self {
        Self {
            threads: Vec::new(),
            selected_peer: None,
            messages: Vec::new(),
            status: None,
            busy: false,
            loaded_local: false,
            peer_names: BTreeMap::new(),
            recipient_editor: None,
            input_editor: None,
            scroll_handle: ScrollHandle::new(),
            _task: None,
        }
    }

    /// Merge resolved author display names from the parent viewer's feeds/lookups
    /// so peer chips and message headers show names instead of raw hex.
    pub(crate) fn update_known_names(&mut self, names: BTreeMap<String, String>) {
        for (pubkey, name) in names {
            if !name.trim().is_empty() {
                self.peer_names.insert(pubkey, name);
            }
        }
    }

    /// Open (or start) an encrypted DM thread with `peer_hex`, caching an optional
    /// display name and focusing the message input editor.
    pub(crate) fn open_peer(
        &mut self,
        peer_hex: String,
        peer_name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(name) = peer_name.filter(|n| !n.trim().is_empty()) {
            self.peer_names.insert(peer_hex.clone(), name);
        }
        let (_, input_ed) = self.ensure_editors(window, cx);
        self.select_peer(peer_hex, cx);
        input_ed.update(cx, |ed, cx| {
            window.focus(&ed.focus_handle(cx), cx);
        });
        cx.notify();
    }

    fn ensure_editors(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<editor::Editor>, Entity<editor::Editor>) {
        let recipient = if let Some(existing) = &self.recipient_editor {
            existing.clone()
        } else {
            let ed = cx.new(|cx| {
                let mut e = editor::Editor::single_line(window, cx);
                e.set_placeholder_text("Recipient npub1… or 64-char hex", window, cx);
                e
            });
            self.recipient_editor = Some(ed.clone());
            ed
        };
        let input = if let Some(existing) = &self.input_editor {
            existing.clone()
        } else {
            let ed = cx.new(|cx| {
                let mut e = editor::Editor::multi_line(window, cx);
                e.disable_scrollbars_and_minimap(window, cx);
                e.set_show_gutter(false, cx);
                e.set_show_line_numbers(false, cx);
                e.set_show_git_diff_gutter(false, cx);
                e.set_show_code_actions(false, cx);
                e.set_show_runnables(false, cx);
                e.set_show_bookmarks(false, cx);
                e.set_show_breakpoints(false, cx);
                e.set_show_wrap_guides(false, cx);
                e.set_placeholder_text("Write an encrypted NIP-17 DM…", window, cx);
                e
            });
            self.input_editor = Some(ed.clone());
            ed
        };
        (recipient, input)
    }

    fn load_local_threads(&mut self, cx: &mut Context<Self>) {
        if self.loaded_local || self.busy {
            return;
        }
        self.loaded_local = true;
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::load_local_threads().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(threads) => {
                        this.threads = threads;
                        if this.selected_peer.is_none()
                            && let Some(first) = this.threads.first()
                        {
                            let peer = first.peer_pubkey_hex.clone();
                            this.select_peer(peer, cx);
                        }
                    }
                    Err(err) => {
                        this.status = Some((false, err));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn select_peer(&mut self, peer_hex: String, cx: &mut Context<Self>) {
        self.selected_peer = Some(peer_hex.clone());
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::load_local_messages(peer_hex).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(messages) => this.messages = messages,
                    Err(err) => this.status = Some((false, err)),
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    pub(crate) fn sync_dms(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = Some((true, "Syncing NIP-17 gift wraps from seed relays…".into()));
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::sync_dms(cx).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(report) => {
                        let summary = format!(
                            "Synced {} wrap(s), {} new DM(s) (NIP-17)",
                            report.fetched_wraps, report.newly_stored
                        );
                        this.threads = report.threads;
                        this.status = Some((true, summary));
                        if let Some(peer) = this.selected_peer.clone() {
                            this.select_peer(peer, cx);
                        } else if let Some(first) = this.threads.first() {
                            this.select_peer(first.peer_pubkey_hex.clone(), cx);
                        }
                    }
                    Err(err) => {
                        this.status = Some((false, err));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn publish_dm_relays(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let relays: Vec<String> = KaskSettings::get_global(cx)
            .nostr
            .seed_relays
            .iter()
            .map(|r| r.trim().to_string())
            .filter(|r| r.starts_with("wss://"))
            .collect();
        if relays.is_empty() {
            self.status = Some((
                false,
                "Configure at least one public wss:// seed relay in Settings > Kask > Nostr first."
                    .into(),
            ));
            cx.notify();
            return;
        }
        self.busy = true;
        self.status = Some((
            true,
            format!(
                "Awaiting approval to publish {} DM inbox relay(s) (NIP-17 kind:10050)…",
                relays.len()
            ),
        ));
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            PUBLISH_LIST_TOOL,
            json!({ "kind": 10_050, "relay_urls": relays }),
        ) {
            Ok(task) => task,
            Err(err) => {
                self.busy = false;
                self.status = Some((false, err));
                cx.notify();
                return;
            }
        };
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.status = match result {
                    Ok(raw) => match parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => Some((
                            true,
                            format!(
                                "Published NIP-17 DM relay list (kind:10050 · {} relay(s))",
                                outcome.receipt().accepted.len()
                            ),
                        )),
                        Ok(_) => Some((
                            false,
                            "Publishing DM relay list failed: no relay accepted.".into(),
                        )),
                        Err(err) => Some((false, err)),
                    },
                    Err(err) => Some((false, sanitize_untrusted_text(&err.to_string()))),
                };
                cx.notify();
            })
            .log_err();
        }));
    }

    fn send_dm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (recipient_ed, input_ed) = self.ensure_editors(window, cx);
        let recipient_raw = self
            .selected_peer
            .clone()
            .unwrap_or_else(|| recipient_ed.read(cx).text(cx));
        let content = input_ed.read(cx).text(cx);
        self.busy = true;
        self.status = Some((true, "Awaiting DM approval & NIP-59 gift wrap…".into()));
        cx.notify();

        let window_handle = window.window_handle();
        let input_handle = input_ed;
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::send_dm(recipient_raw, content, cx).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(report) => {
                        let peer = report.message.peer_pubkey_hex.clone();
                        this.selected_peer = Some(peer);
                        this.messages.push(report.message);
                        this.status = Some((
                            true,
                            format!(
                                "Sent encrypted DM (NIP-17 · {} relay(s) accepted)",
                                report.relays_accepted
                            ),
                        ));
                        window_handle
                            .update(cx, |_, window, cx| {
                                input_handle.update(cx, |ed, cx| ed.set_text("", window, cx));
                            })
                            .log_err();
                        this.loaded_local = false;
                        this.load_local_threads(cx);
                    }
                    Err(err) => {
                        this.status = Some((false, err));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn line(text: impl Into<SharedString>, color: Color) -> AnyElement {
        div()
            .w_full()
            .min_w_0()
            .child(Label::new(text).size(LabelSize::XSmall).color(color))
            .into_any_element()
    }
}

impl Render for NostrDmView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.load_local_threads(cx);
        let (recipient_ed, input_ed) = self.ensure_editors(window, cx);
        let busy = self.busy;
        let selected_peer = self.selected_peer.clone();
        let status = self.status.clone();
        let selected_peer_in_threads = selected_peer.as_deref().is_some_and(|peer| {
            self.threads
                .iter()
                .any(|t| t.peer_pubkey_hex.eq_ignore_ascii_case(peer))
        });

        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .p_2()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .min_w_0()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new("NIP-17 · NIP-44 + NIP-59 · SQLCipher (editor-only)")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_shrink_0()
                            .child(
                                ui::Button::new("nostr-dm-publish-relays", "DM relays")
                                    .label_size(LabelSize::XSmall)
                                    .disabled(busy)
                                    .tooltip(ui::Tooltip::text(
                                        "Publish configured seed relays as your NIP-17 kind:10050 DM inbox relay list (always prompts)",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.publish_dm_relays(cx);
                                    })),
                            )
                            .child(
                                ui::Button::new("nostr-dm-sync", if busy { "Syncing…" } else { "Sync DMs" })
                                    .label_size(LabelSize::XSmall)
                                    .disabled(busy)
                                    .tooltip(ui::Tooltip::text(
                                        "Fetch & unwrap NIP-17 kind:1059 gift wraps from seed relays (https://nostr-nips.com/nip-17)",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.sync_dms(cx))),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .flex_wrap()
                    .child(
                        ui::Button::new("nostr-dm-new", "+ New DM")
                            .label_size(LabelSize::XSmall)
                            .toggle_state(selected_peer.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected_peer = None;
                                this.messages.clear();
                                cx.notify();
                            })),
                    )
                    .when_some(
                        selected_peer
                            .as_ref()
                            .filter(|_| !selected_peer_in_threads)
                            .cloned(),
                        |row, peer_hex| {
                            let label = format_peer_label(&peer_hex, &self.peer_names, None);
                            row.child(
                                ui::Button::new("nostr-dm-active-new-peer", label)
                                    .label_size(LabelSize::XSmall)
                                    .toggle_state(true),
                            )
                        },
                    )
                    .children(self.threads.iter().enumerate().map(|(ix, thread)| {
                        let peer_hex = thread.peer_pubkey_hex.clone();
                        let active = selected_peer.as_deref() == Some(peer_hex.as_str());
                        let label = format_peer_label(
                            &thread.peer_pubkey_hex,
                            &self.peer_names,
                            Some(thread.message_count),
                        );
                        ui::Button::new(("nostr-dm-peer", ix), label)
                            .label_size(LabelSize::XSmall)
                            .toggle_state(active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_peer(peer_hex.clone(), cx);
                            }))
                    })),
            )
            .when(selected_peer.is_none(), |el| {
                el.child(
                    div()
                        .w_full()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(cx.theme().colors().border)
                        .bg(cx.theme().colors().editor_background)
                        .child(recipient_ed),
                )
            })
            .when_some(status, |el, (ok, msg)| {
                el.child(
                    Label::new(display_text(&msg))
                        .size(LabelSize::XSmall)
                        .color(if ok { Color::Muted } else { Color::Error }),
                )
            })
            .child(
                v_flex()
                    .id("nostr-dm-messages-list")
                    .w_full()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .custom_scrollbars(
                        ui::Scrollbars::always_visible(ui::ScrollAxes::Vertical)
                            .tracked_scroll_handle(&self.scroll_handle),
                        window,
                        cx,
                    )
                    .pr_3()
                    .gap_1()
                    .when(self.messages.is_empty(), |el| {
                        el.child(Self::line(
                            "No decrypted NIP-17 messages in this thread yet. Click Sync DMs or send a message below.",
                            Color::Muted,
                        ))
                    })
                    .children(self.messages.iter().map(|msg| {
                        let who = if msg.outgoing {
                            "You".to_string()
                        } else {
                            format_peer_label(&msg.peer_pubkey_hex, &self.peer_names, None)
                        };
                        v_flex()
                            .w_full()
                            .p_1()
                            .rounded_sm()
                            .bg(cx.theme().colors().surface_background)
                            .child(Self::line(
                                format!("{who} · {}", utc_text(msg.created_at)),
                                Color::Hint,
                            ))
                            .child(Self::line(display_text(&msg.content), Color::Default))
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .flex_shrink_0()
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "enter"
                            && (event.keystroke.modifiers.secondary(cx)
                                || event.keystroke.modifiers.control)
                        {
                            this.send_dm(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .w_full()
                            .min_h(px(48.))
                            .max_h(px(96.))
                            .overflow_hidden()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .bg(cx.theme().colors().editor_background)
                            .child(input_ed),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .child(
                                ui::Button::new(
                                    "nostr-dm-send",
                                    if busy { "Sending…" } else { "Send DM" },
                                )
                                .style(ui::ButtonStyle::Filled)
                                .label_size(LabelSize::XSmall)
                                .disabled(busy)
                                .tooltip(ui::Tooltip::text(
                                    "Send NIP-17 encrypted DM (Ctrl/Cmd+Enter). Always prompts for approval.",
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.send_dm(window, cx);
                                })),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_peer_label_prefers_resolved_name_and_falls_back_to_short_hex() {
        let hex = "f063ef1d19c85c7b02a52f00a436d4b20e2b16180fe5f35a34b7f7325782d618";
        let mut names = BTreeMap::new();
        assert_eq!(format_peer_label(hex, &names, Some(3)), "f063ef1d…d618 (3)");
        assert_eq!(format_peer_label(hex, &names, None), "f063ef1d…d618");

        names.insert(hex.to_string(), "Alice".to_string());
        assert_eq!(format_peer_label(hex, &names, Some(3)), "@Alice (3)");
        assert_eq!(format_peer_label(hex, &names, None), "@Alice");
    }

    #[test]
    fn update_known_names_ignores_blank_values() {
        let mut view = NostrDmView::new();
        let mut map = BTreeMap::new();
        map.insert("a".repeat(64), "   ".to_string());
        map.insert("b".repeat(64), "Bob".to_string());
        view.update_known_names(map);
        assert!(!view.peer_names.contains_key(&"a".repeat(64)));
        assert_eq!(
            view.peer_names.get(&"b".repeat(64)).map(String::as_str),
            Some("Bob")
        );
    }
}
