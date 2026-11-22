//! Self-contained Relay, Blossom, Seed-Relay Probe, NIP-65/NIP-61 list
//! publishing, and NIP-01 `kind: 0` self-profile editor tab view.
//!
//! Extracted from `nostr_viewer.rs` (matching `nostr_dm_view.rs` and
//! `nostr_zap_view.rs`): owns the 16 relay/profile/blossom fields and emits
//! `RelaySynced` so `NostrViewer` can reload the local archive after a
//! `nostr_relay_sync` without owning relay-tab internals.

use gpui::{AnyElement, Context, Entity, EventEmitter, IntoElement, Render, ScrollHandle, Window};
use hkask_types::nostr_wire::{NOSTR_BLOSSOM_SERVER, NOSTR_CLIENT_SERVER, NOSTR_RELAY_SERVER};
use hkask_types::wallet_wire::WALLET_SERVER_ID;
use kask_bridge::{KaskSettings, nostr_dm};
use serde_json::{Value, json};
use settings::Settings as _;
use ui::{LabelSize, WithScrollbar as _, prelude::*};
use util::ResultExt as _;

use crate::nostr_composer::{
    PUBLISH_LIST_TOOL, PUBLISH_PROFILE_TOOL, WALLET_STATUS_TOOL, parse_publish_receipt,
    parse_wallet_status, sanitize_untrusted_text,
};
use crate::nostr_event_model::{
    BlossomStatusSummary, Decision, LOG_PAGE, RelayReading, RelayStatus, Request, SeedRelayProbe,
    host_of, parse_account_seed_relays, parse_blossom_status,
};
use crate::nostr_viewer::{
    ACCOUNT_TOOL, BLOSSOM_STATUS_TOOL, LOG_TOOL, NostrViewer, PROFILE_TOOL, STATUS_TOOL, SYNC_TOOL,
    content_of, display_text, invoke, short_hex, utc_text,
};

/// Emitted when `nostr_relay_sync` completes successfully so `NostrViewer`
/// reloads the local archive.
pub(crate) struct RelaySynced;

pub(crate) struct NostrRelayView {
    relay: Option<RelayReading>,
    relay_request: Request,
    blossom_status: Option<BlossomStatusSummary>,
    seed_relay_probes: Vec<SeedRelayProbe>,
    probe_request: Request,
    nutzap_info_status: Option<(bool, String)>,
    profile_editor_open: bool,
    profile_name_editor: Option<Entity<editor::Editor>>,
    profile_about_editor: Option<Entity<editor::Editor>>,
    profile_nip05_editor: Option<Entity<editor::Editor>>,
    profile_lud16_editor: Option<Entity<editor::Editor>>,
    /// Non-edited `kind: 0` fields (`display_name`, `picture`, `banner`, `website`, `lud06`)
    /// preserved from the operator's loaded profile so publishing never silently wipes them.
    loaded_profile_extras: [Option<String>; 5],
    profile_loading: bool,
    profile_publish_status: Option<(bool, String)>,
    nip65_publish_status: Option<(bool, String)>,
    relay_scroll_handle: ScrollHandle,
}

impl EventEmitter<RelaySynced> for NostrRelayView {}

impl NostrRelayView {
    pub(crate) fn new() -> Self {
        Self {
            relay: None,
            relay_request: Request::new(),
            blossom_status: None,
            seed_relay_probes: Vec::new(),
            probe_request: Request::new(),
            nutzap_info_status: None,
            profile_editor_open: false,
            profile_name_editor: None,
            profile_about_editor: None,
            profile_nip05_editor: None,
            profile_lud16_editor: None,
            loaded_profile_extras: [None, None, None, None, None],
            profile_loading: false,
            profile_publish_status: None,
            nip65_publish_status: None,
            relay_scroll_handle: ScrollHandle::new(),
        }
    }

    pub(crate) fn has_reading(&self) -> bool {
        self.relay.is_some()
    }

    fn my_pubkey_hex(&self, cx: &Context<Self>) -> Option<String> {
        nostr_dm::parse_recipient_pubkey(&KaskSettings::get_global(cx).nostr.npub)
            .ok()
            .map(|pk| pk.to_hex())
    }

    /// Read the local relay's state and its admission decisions, plus local
    /// Blossom sidecar status. Strictly local (contacts no public relay).
    pub(crate) fn load_relay(&mut self, cx: &mut Context<Self>) {
        let epoch = self.relay_request.begin();
        cx.notify();
        let blossom_task = invoke(NOSTR_BLOSSOM_SERVER, BLOSSOM_STATUS_TOOL, json!({})).ok();
        let tasks = invoke(NOSTR_RELAY_SERVER, STATUS_TOOL, json!({})).and_then(|status| {
            invoke(NOSTR_RELAY_SERVER, LOG_TOOL, json!({ "limit": LOG_PAGE }))
                .map(|log| (status, log))
        });
        let (status, log) = match tasks {
            Ok(tasks) => tasks,
            Err(error) => return self.apply_relay(epoch, Err(error), None, cx),
        };
        cx.spawn(async move |this, cx| {
            let blossom = match blossom_task {
                Some(task) => content_of(BLOSSOM_STATUS_TOOL, task.await)
                    .ok()
                    .and_then(|content| parse_blossom_status(&content)),
                None => None,
            };
            let result = async {
                let status = content_of(STATUS_TOOL, status.await)?;
                let log = content_of(LOG_TOOL, log.await)?;
                let status: RelayStatus = serde_json::from_value(status).map_err(|error| {
                    format!("{STATUS_TOOL} returned an unexpected shape: {error}")
                })?;
                let decisions: Vec<Decision> = serde_json::from_value(
                    log.get("decisions").cloned().unwrap_or_else(|| json!([])),
                )
                .map_err(|error| format!("{LOG_TOOL} returned an unexpected shape: {error}"))?;
                Ok(RelayReading { status, decisions })
            }
            .await;
            this.update(cx, |this, cx| this.apply_relay(epoch, result, blossom, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_relay(
        &mut self,
        epoch: u64,
        result: Result<RelayReading, String>,
        blossom: Option<BlossomStatusSummary>,
        cx: &mut Context<Self>,
    ) {
        let (reading, error) = match result {
            Ok(reading) => (Some(reading), None),
            Err(error) => (None, Some(error)),
        };
        if !self.relay_request.finish(epoch, error) {
            return;
        }
        if let Some(reading) = reading {
            self.relay = Some(reading);
        }
        if blossom.is_some() {
            self.blossom_status = blossom;
        }
        cx.notify();
    }

    fn sync_local_relay(&mut self, cx: &mut Context<Self>) {
        let epoch = self.relay_request.begin();
        cx.notify();
        let task = match invoke(NOSTR_RELAY_SERVER, SYNC_TOOL, json!({})) {
            Ok(task) => task,
            Err(error) => {
                self.relay_request.finish(epoch, Some(error));
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let result = content_of(SYNC_TOOL, task.await);
            this.update(cx, |this, cx| match result {
                Ok(_) => {
                    this.relay_request.finish(epoch, None);
                    this.load_relay(cx);
                    cx.emit(RelaySynced);
                }
                Err(err) => {
                    this.relay_request.finish(epoch, Some(err));
                    cx.notify();
                }
            })
            .log_err();
        })
        .detach();
    }

    /// Explicit opt-in seed-relay connectivity probe via `nostr_account`.
    fn probe_seed_relays(&mut self, cx: &mut Context<Self>) {
        let epoch = self.probe_request.begin();
        cx.notify();
        let task = match invoke(NOSTR_CLIENT_SERVER, ACCOUNT_TOOL, json!({})) {
            Ok(task) => task,
            Err(error) => {
                self.probe_request.finish(epoch, Some(error));
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let result = content_of(ACCOUNT_TOOL, task.await)
                .map(|content| parse_account_seed_relays(&content));
            this.update(cx, |this, cx| {
                let (probes, error) = match result {
                    Ok(probes) => (Some(probes), None),
                    Err(err) => (None, Some(err)),
                };
                if !this.probe_request.finish(epoch, error) {
                    return;
                }
                if let Some(probes) = probes {
                    this.seed_relay_probes = probes;
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn ensure_profile_editors(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        Entity<editor::Editor>,
        Entity<editor::Editor>,
        Entity<editor::Editor>,
        Entity<editor::Editor>,
    ) {
        let mk = |slot: &mut Option<Entity<editor::Editor>>,
                  placeholder: &'static str,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
            if let Some(existing) = slot.as_ref() {
                existing.clone()
            } else {
                let ed = cx.new(|cx| {
                    let mut e = editor::Editor::single_line(window, cx);
                    e.set_placeholder_text(placeholder, window, cx);
                    e
                });
                *slot = Some(ed.clone());
                ed
            }
        };
        let name_ed = mk(&mut self.profile_name_editor, "Name / handle…", window, cx);
        let about_ed = mk(
            &mut self.profile_about_editor,
            "About / short bio…",
            window,
            cx,
        );
        let nip05_ed = mk(
            &mut self.profile_nip05_editor,
            "NIP-05 address (user@domain)…",
            window,
            cx,
        );
        let lud16_ed = mk(
            &mut self.profile_lud16_editor,
            "Lightning Address (user@domain, lud16)…",
            window,
            cx,
        );
        (name_ed, about_ed, nip05_ed, lud16_ed)
    }

    fn load_own_profile_into_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mine) = self.my_pubkey_hex(cx) else {
            self.profile_publish_status = Some((
                false,
                "Configure your Nostr npub in Settings > Kask > Nostr first.".to_string(),
            ));
            cx.notify();
            return;
        };
        let (name_ed, about_ed, nip05_ed, lud16_ed) = self.ensure_profile_editors(window, cx);
        self.profile_loading = true;
        self.profile_publish_status = Some((true, "Loading current kind:0 profile…".to_string()));
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            PROFILE_TOOL,
            json!({ "authors": [mine] }),
        ) {
            Ok(t) => t,
            Err(err) => {
                self.profile_loading = false;
                self.profile_publish_status = Some((false, err));
                cx.notify();
                return;
            }
        };
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = content_of(PROFILE_TOOL, task.await);
            this.update(cx, |this, cx| {
                this.profile_loading = false;
                match result {
                    Ok(content) => {
                        let fields = content
                            .get("profiles")
                            .and_then(Value::as_array)
                            .and_then(|arr| arr.first())
                            .and_then(|p| p.get("fields"))
                            .and_then(Value::as_object);
                        if let Some(fields) = fields {
                            let get_str = |k: &str| {
                                fields
                                    .get(k)
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string()
                            };
                            let name_v = get_str("name");
                            let about_v = get_str("about");
                            let nip05_v = get_str("nip05");
                            let lud16_v = get_str("lud16");
                            let opt_str = |k: &str| {
                                fields
                                    .get(k)
                                    .and_then(Value::as_str)
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                            };
                            this.loaded_profile_extras = [
                                opt_str("display_name"),
                                opt_str("picture"),
                                opt_str("banner"),
                                opt_str("website"),
                                opt_str("lud06"),
                            ];
                            window_handle
                                .update(cx, |_, window, cx| {
                                    // Preserve any text the operator already typed while loading.
                                    if name_ed.read(cx).text(cx).trim().is_empty() {
                                        name_ed.update(cx, |e, cx| e.set_text(name_v, window, cx));
                                    }
                                    if about_ed.read(cx).text(cx).trim().is_empty() {
                                        about_ed
                                            .update(cx, |e, cx| e.set_text(about_v, window, cx));
                                    }
                                    if nip05_ed.read(cx).text(cx).trim().is_empty() {
                                        nip05_ed
                                            .update(cx, |e, cx| e.set_text(nip05_v, window, cx));
                                    }
                                    if lud16_ed.read(cx).text(cx).trim().is_empty() {
                                        lud16_ed
                                            .update(cx, |e, cx| e.set_text(lud16_v, window, cx));
                                    }
                                })
                                .log_err();
                            this.profile_publish_status =
                                Some((true, "Loaded current kind:0 profile fields.".to_string()));
                        } else {
                            this.profile_publish_status = Some((
                                true,
                                "No existing kind:0 profile found on configured relays."
                                    .to_string(),
                            ));
                        }
                    }
                    Err(err) => {
                        this.profile_publish_status = Some((false, err));
                    }
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn publish_own_profile(&mut self, cx: &mut Context<Self>) {
        if self.profile_loading {
            self.profile_publish_status = Some((
                false,
                "Wait for current kind:0 profile to finish loading before publishing.".to_string(),
            ));
            cx.notify();
            return;
        }
        let read_opt = |ed: &Option<Entity<editor::Editor>>| {
            ed.as_ref()
                .map(|e| e.read(cx).text(cx).trim().to_string())
                .filter(|s| !s.is_empty())
        };
        let mut args = serde_json::Map::new();
        for (k, v) in [
            ("name", read_opt(&self.profile_name_editor)),
            ("display_name", self.loaded_profile_extras[0].clone()),
            ("about", read_opt(&self.profile_about_editor)),
            ("picture", self.loaded_profile_extras[1].clone()),
            ("banner", self.loaded_profile_extras[2].clone()),
            ("website", self.loaded_profile_extras[3].clone()),
            ("nip05", read_opt(&self.profile_nip05_editor)),
            ("lud16", read_opt(&self.profile_lud16_editor)),
            ("lud06", self.loaded_profile_extras[4].clone()),
        ] {
            if let Some(val) = v {
                args.insert(k.to_string(), json!(val));
            }
        }
        if args.is_empty() {
            self.profile_publish_status = Some((
                false,
                "Enter at least one profile field before publishing kind:0.".to_string(),
            ));
            cx.notify();
            return;
        }
        self.profile_publish_status = Some((
            true,
            "Awaiting editor approval to publish NIP-01 kind:0 profile…".to_string(),
        ));
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            PUBLISH_PROFILE_TOOL,
            Value::Object(args),
        ) {
            Ok(t) => t,
            Err(err) => {
                self.profile_publish_status = Some((false, err));
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                this.profile_publish_status = match res {
                    Ok(raw) => match parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => Some((
                            true,
                            format!(
                                "Published NIP-01 profile (kind:0 · {} relay(s))",
                                outcome.receipt().accepted.len()
                            ),
                        )),
                        Ok(_) => Some((
                            false,
                            "Publishing kind:0 profile failed: no relay accepted.".to_string(),
                        )),
                        Err(err) => Some((false, err)),
                    },
                    Err(err) => Some((false, sanitize_untrusted_text(&err.to_string()))),
                };
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Publish the operator's configured `wss://` seed relays as their NIP-65
    /// `kind: 10002` relay list (`nostr_publish_list`). Always prompts in editor.
    fn publish_nip65_relays(&mut self, cx: &mut Context<Self>) {
        let relays: Vec<String> = KaskSettings::get_global(cx)
            .nostr
            .seed_relays
            .iter()
            .map(|r| r.trim().to_string())
            .filter(|r| r.starts_with("wss://"))
            .collect();
        if relays.is_empty() {
            self.nip65_publish_status = Some((
                false,
                "Configure at least one public wss:// seed relay in Settings > Kask > Nostr first."
                    .to_string(),
            ));
            cx.notify();
            return;
        }
        self.nip65_publish_status = Some((
            true,
            format!(
                "Awaiting editor approval to publish {} relay(s) as NIP-65 kind:10002…",
                relays.len()
            ),
        ));
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            PUBLISH_LIST_TOOL,
            json!({ "kind": 10_002, "relay_urls": relays }),
        ) {
            Ok(t) => t,
            Err(err) => {
                self.nip65_publish_status = Some((false, err));
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                this.nip65_publish_status = match res {
                    Ok(raw) => match parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => Some((
                            true,
                            format!(
                                "Published NIP-65 relay list (kind:10002 · {} relay(s))",
                                outcome.receipt().accepted.len()
                            ),
                        )),
                        Ok(_) => Some((
                            false,
                            "Publishing NIP-65 relay list failed: no relay accepted.".to_string(),
                        )),
                        Err(err) => Some((false, err)),
                    },
                    Err(err) => Some((false, sanitize_untrusted_text(&err.to_string()))),
                };
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Publish the operator's NIP-61 `kind: 10019` nutzap receiving info
    /// (`relay_urls`, `mint_urls`, and the wallet's seed-derived `p2pk_pubkey`).
    /// Always prompts in the editor.
    fn publish_nutzap_info(&mut self, cx: &mut Context<Self>) {
        let relays: Vec<String> = KaskSettings::get_global(cx)
            .nostr
            .seed_relays
            .iter()
            .map(|r| r.trim().to_string())
            .filter(|r| r.starts_with("wss://"))
            .collect();
        if relays.is_empty() {
            self.nutzap_info_status = Some((
                false,
                "Configure at least one public wss:// seed relay in Settings > Kask > Nostr first."
                    .to_string(),
            ));
            cx.notify();
            return;
        }
        self.nutzap_info_status = Some((true, "Reading wallet P2PK receive key & mints…".into()));
        cx.notify();
        let status_task = match invoke(WALLET_SERVER_ID, WALLET_STATUS_TOOL, json!({})) {
            Ok(t) => t,
            Err(err) => {
                self.nutzap_info_status = Some((false, err));
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let status_raw = match status_task.await {
                Ok(raw) => raw,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.nutzap_info_status =
                            Some((false, sanitize_untrusted_text(&err.to_string())));
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };
            let summary = match parse_wallet_status(&status_raw) {
                Ok(s) => s,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.nutzap_info_status = Some((false, err));
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };
            if summary.allowed_mints.is_empty() {
                this.update(cx, |this, cx| {
                    this.nutzap_info_status = Some((
                        false,
                        "Configure at least one allowlisted Cashu mint in Settings > Kask > Wallet first."
                            .to_string(),
                    ));
                    cx.notify();
                })
                .log_err();
                return;
            }
            let Some(p2pk_pubkey) = summary.p2pk_xonly_pubkey else {
                this.update(cx, |this, cx| {
                    this.nutzap_info_status = Some((
                        false,
                        "Wallet is locked or has no P2PK receive key; unlock wallet before publishing kind:10019."
                            .to_string(),
                    ));
                    cx.notify();
                })
                .log_err();
                return;
            };
            this.update(cx, |this, cx| {
                this.nutzap_info_status = Some((
                    true,
                    "Awaiting editor approval to publish NIP-61 kind:10019 nutzap info…".into(),
                ));
                cx.notify();
            })
            .log_err();
            let pub_task = match invoke(
                NOSTR_CLIENT_SERVER,
                PUBLISH_LIST_TOOL,
                json!({
                    "kind": 10_019,
                    "relay_urls": relays,
                    "mint_urls": summary.allowed_mints,
                    "p2pk_pubkey": p2pk_pubkey,
                }),
            ) {
                Ok(t) => t,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.nutzap_info_status = Some((false, err));
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };
            let pub_res = pub_task.await;
            this.update(cx, |this, cx| {
                this.nutzap_info_status = match pub_res {
                    Ok(raw) => match parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => Some((
                            true,
                            format!(
                                "Published NIP-61 nutzap info (kind:10019 · {} relay(s))",
                                outcome.receipt().accepted.len()
                            ),
                        )),
                        Ok(_) => Some((
                            false,
                            "Publishing kind:10019 failed: no relay accepted.".into(),
                        )),
                        Err(err) => Some((false, err)),
                    },
                    Err(err) => Some((false, sanitize_untrusted_text(&err.to_string()))),
                };
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let profile_editors = if self.profile_editor_open {
            Some(self.ensure_profile_editors(window, cx))
        } else {
            None
        };
        let profile_open = self.profile_editor_open;
        let request = NostrViewer::request_line(&self.relay_request, "Reading the local relay…");
        let Some(reading) = &self.relay else {
            return v_flex()
                .size_full()
                .children(request)
                .child(NostrViewer::empty(
                    "Relay not read",
                    "The local relay's state has not been read yet.",
                ))
                .into_any_element();
        };
        let status = &reading.status;
        let listener = match (&status.listener.address, &status.listener.reason) {
            (Some(address), _) => format!("Listener {}: {address}", status.listener.state),
            (None, Some(reason)) => format!("Listener {}: {reason}", status.listener.state),
            (None, None) => format!("Listener {}", status.listener.state),
        };
        let listening = status.listener.state == "listening";
        let inner = v_flex()
            .id("nostr-viewer-relay")
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.relay_scroll_handle)
            .p_2()
            .pr_3()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        ui::Button::new("nostr-relay-sync", "Sync local relay")
                            .style(ui::ButtonStyle::Filled)
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text(
                                "Contacts configured sync relays to pull allowlisted authors' public events into the local archive",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.sync_local_relay(cx))),
                    )
                    .child(
                        ui::Button::new("nostr-relay-probe", "Probe seed relays")
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text(
                                "Contacts configured seed relays via nostr_account to test connectivity",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.probe_seed_relays(cx))),
                    )
                    .child(
                        ui::Button::new("nostr-publish-10002", "Publish NIP-65 relays (10002)")
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text(
                                "Publish your configured wss:// seed relays as your NIP-65 kind:10002 relay list (always prompts)",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.publish_nip65_relays(cx))),
                    )
                    .child(
                        ui::Button::new("nostr-publish-10019", "Publish Nutzap info (10019)")
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text(
                                "Publish your wallet's NIP-61 P2PK key, allowlisted mints, and seed relays (kind:10019; always prompts)",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.publish_nutzap_info(cx))),
                    )
                    .child(
                        ui::Button::new(
                            "nostr-toggle-profile-editor",
                            if profile_open {
                                "▾ Hide Profile (kind:0)"
                            } else {
                                "▸ Edit Profile (kind:0)"
                            },
                        )
                        .label_size(LabelSize::XSmall)
                        .toggle_state(profile_open)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.profile_editor_open = !this.profile_editor_open;
                            if this.profile_editor_open && this.profile_publish_status.is_none() {
                                this.load_own_profile_into_editors(window, cx);
                            }
                            cx.notify();
                        })),
                    ),
            )
            .when_some(self.nip65_publish_status.as_ref(), |el, (ok, msg)| {
                el.child(NostrViewer::line(
                    display_text(msg),
                    if *ok { Color::Muted } else { Color::Error },
                ))
            })
            .when_some(
                profile_editors,
                |el, (name_ed, about_ed, nip05_ed, lud16_ed)| {
                    el.child(
                        v_flex()
                            .w_full()
                            .p_2()
                            .gap_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(cx.theme().colors().border_variant)
                            .bg(cx.theme().colors().elevated_surface_background)
                            .child(NostrViewer::line(
                                "NIP-01 kind:0 profile metadata (always prompts in editor)",
                                Color::Hint,
                            ))
                            .children([name_ed, about_ed, nip05_ed, lud16_ed].into_iter().map(
                                |ed| {
                                    div()
                                        .w_full()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(cx.theme().colors().border)
                                        .bg(cx.theme().colors().editor_background)
                                        .child(ed)
                                },
                            ))
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_1()
                                    .flex_wrap()
                                    .child(
                                        ui::Button::new("nostr-profile-load", "Load current")
                                            .label_size(LabelSize::XSmall)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.load_own_profile_into_editors(window, cx);
                                            })),
                                    )
                                    .child(
                                        ui::Button::new(
                                            "nostr-profile-publish",
                                            "Publish Profile (kind:0)",
                                        )
                                        .style(ui::ButtonStyle::Filled)
                                        .label_size(LabelSize::XSmall)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.publish_own_profile(cx);
                                        })),
                                    ),
                            )
                            .when_some(self.profile_publish_status.as_ref(), |col, (ok, msg)| {
                                col.child(NostrViewer::line(
                                    display_text(msg),
                                    if *ok { Color::Muted } else { Color::Error },
                                ))
                            }),
                    )
                },
            )
            .children(request)
            .when_some(self.nutzap_info_status.as_ref(), |el, (ok, msg)| {
                el.child(NostrViewer::line(
                    display_text(msg),
                    if *ok { Color::Muted } else { Color::Error },
                ))
            })
            .children(NostrViewer::request_line(
                &self.probe_request,
                "Probing seed relays via nostr_account…",
            ))
            .when(!self.seed_relay_probes.is_empty(), |el| {
                el.child(NostrViewer::line("Seed-relay probe results", Color::Hint))
                    .children(self.seed_relay_probes.iter().map(|probe| {
                        let color = if probe.state == "connected" {
                            Color::Success
                        } else {
                            Color::Warning
                        };
                        NostrViewer::line(
                            display_text(&format!("{} · {}", host_of(&probe.address), probe.state)),
                            color,
                        )
                    }))
            })
            .when_some(self.blossom_status.as_ref(), |el, blossom| {
                let mut block = el
                    .child(NostrViewer::line("Local Blossom sidecar", Color::Hint))
                    .child(NostrViewer::line(
                        display_text(&format!(
                            "Listener: {} · {} blobs · {} / {} bytes",
                            blossom.listener,
                            blossom.blob_count,
                            blossom.total_bytes,
                            blossom.quota_bytes
                        )),
                        Color::Default,
                    ));
                if let Some(url) = &blossom.public_base_url {
                    block = block.child(NostrViewer::line(
                        display_text(&format!("Public base URL: {url}")),
                        Color::Muted,
                    ));
                }
                block
            })
            .child(NostrViewer::line(
                display_text(&listener),
                if listening {
                    Color::Default
                } else {
                    Color::Warning
                },
            ))
            .child(NostrViewer::line(
                format!(
                    "{} events stored · {} allowlisted authors · kinds refused: {:?}",
                    status.archive.stored_events,
                    status.policy.allowlisted_authors.len(),
                    status.policy.refused_kinds
                ),
                Color::Default,
            ))
            .child(NostrViewer::line(
                format!(
                    "Since start: {} admitted, {} refused",
                    status.admission_since_start.admitted, status.admission_since_start.refused
                ),
                Color::Default,
            ))
            .child(NostrViewer::line(
                display_text(&format!(
                    "Sync reads from: {}",
                    if status.policy.sync_relays.is_empty() {
                        "no relay configured".to_string()
                    } else {
                        status.policy.sync_relays.join(", ")
                    }
                )),
                Color::Muted,
            ))
            .children(
                status
                    .configuration_warnings
                    .iter()
                    .map(|warning| NostrViewer::line(display_text(warning), Color::Warning)),
            )
            .child(NostrViewer::line("What the relay does not protect", Color::Hint))
            .children(
                status
                    .access_limits
                    .iter()
                    .map(|limit| NostrViewer::line(display_text(limit), Color::Muted)),
            )
            .child(NostrViewer::line(
                format!(
                    "Admission decisions, newest first ({})",
                    reading.decisions.len()
                ),
                Color::Hint,
            ))
            .when(reading.decisions.is_empty(), |el| {
                el.child(NostrViewer::line(
                    "None since the relay started. The log is kept in memory.",
                    Color::Muted,
                ))
            })
            .children(reading.decisions.iter().map(|decision| {
                NostrViewer::line(
                    display_text(&format!(
                        "{} · {} · kind {} · {} · {}",
                        decision
                            .at
                            .map(utc_text)
                            .unwrap_or_else(|| "time unknown".to_string()),
                        decision.path,
                        decision.kind,
                        short_hex(&decision.author),
                        decision.outcome
                    )),
                    if decision.outcome == "admitted" {
                        Color::Default
                    } else {
                        Color::Warning
                    },
                )
            }));
        div()
            .id("nostr-relay-scroll-frame")
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(inner)
            .custom_scrollbars(
                ui::Scrollbars::always_visible(ui::ScrollAxes::Vertical)
                    .id("nostr-relay-scrollbar")
                    .tracked_scroll_handle(&self.relay_scroll_handle),
                window,
                cx,
            )
            .into_any_element()
    }
}

impl Render for NostrRelayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_body(window, cx)
    }
}
