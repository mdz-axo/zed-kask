//! Self-contained NIP-57 Lightning Zap & NIP-61 Cashu Nutzap strip view.
//!
//! Extracted from `nostr_viewer.rs` (Experiment 3c): owns the 5 zap fields
//! (`zap`, `zap_amount_editor`, `zap_comment_editor`, `_zap_wallet_task`,
//! `_zap_task`) and emits `ZapSettled` so `NostrViewer` can optimistically
//! bump zap totals and refresh reactions without owning zap internals.

use gpui::{AnyElement, Context, Entity, EventEmitter, IntoElement, Render, Task, Window};
use hkask_types::nostr_wire::NOSTR_CLIENT_SERVER;
use hkask_types::wallet_wire::WALLET_SERVER_ID;
use serde_json::{Value, json};
use ui::{Label, LabelSize, prelude::*};
use util::ResultExt as _;

use crate::nostr_composer::{
    PREPARE_ZAP_TOOL, PUBLISH_NUTZAP_TOOL, WALLET_PAY_INVOICE_TOOL, WALLET_SEND_ECASH_TOOL,
    WALLET_STATUS_TOOL, WalletBadge, ZAP_PRESETS_SATS, ZapDraftState, ZapRail, ZapStatus,
    parse_custom_sats, parse_wallet_status, sanitize_untrusted_text,
};
use crate::nostr_viewer::{PROFILE_TOOL, extract_lud16_from_profile_response, invoke};

/// Event emitted when a NIP-57 Lightning zap or NIP-61 Cashu nutzap settles.
pub(crate) struct ZapSettled {
    pub(crate) target_event_id: String,
    pub(crate) amount_sats: u64,
    pub(crate) optimistic_bump: bool,
}

/// Self-contained GPUI view for the Amethyst-style quick-zap strip.
pub(crate) struct NostrZapView {
    zap: Option<ZapDraftState>,
    zap_amount_editor: Option<Entity<editor::Editor>>,
    zap_comment_editor: Option<Entity<editor::Editor>>,
    _zap_wallet_task: Option<Task<()>>,
    _zap_task: Option<Task<()>>,
}

impl EventEmitter<ZapSettled> for NostrZapView {}

impl NostrZapView {
    pub(crate) fn new() -> Self {
        Self {
            zap: None,
            zap_amount_editor: None,
            zap_comment_editor: None,
            _zap_wallet_task: None,
            _zap_task: None,
        }
    }

    pub(crate) fn is_open_for(&self, event_id: &str) -> bool {
        self.zap
            .as_ref()
            .is_some_and(|z| z.target_event_id == event_id)
    }

    pub(crate) fn dismiss_if_idle(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.zap.is_some() && !self.zap.as_ref().is_some_and(ZapDraftState::is_busy) {
            self.zap = None;
            self.clear_zap_editors(window, cx);
            cx.notify();
            true
        } else {
            false
        }
    }

    fn ensure_zap_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.zap_amount_editor.is_none() {
            let ed = cx.new(|cx| {
                let mut ed = editor::Editor::single_line(window, cx);
                ed.set_placeholder_text("Custom sats…", window, cx);
                ed
            });
            self.zap_amount_editor = Some(ed);
        }
        if self.zap_comment_editor.is_none() {
            let ed = cx.new(|cx| {
                let mut ed = editor::Editor::single_line(window, cx);
                ed.set_placeholder_text("Optional zap comment…", window, cx);
                ed
            });
            self.zap_comment_editor = Some(ed);
        }
    }

    pub(crate) fn toggle(
        &mut self,
        event_id: String,
        author: String,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .zap
            .as_ref()
            .is_some_and(|z| z.target_event_id == event_id)
        {
            if !self.zap.as_ref().is_some_and(ZapDraftState::is_busy) {
                self.zap = None;
                self.clear_zap_editors(window, cx);
            }
        } else {
            self.zap = Some(ZapDraftState::new(event_id, author, label));
            self.clear_zap_editors(window, cx);
            self.fetch_zap_wallet_status(cx);
        }
        cx.notify();
    }

    fn clear_zap_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ed) = &self.zap_amount_editor {
            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
        if let Some(ed) = &self.zap_comment_editor {
            ed.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
    }

    fn fetch_zap_wallet_status(&mut self, cx: &mut Context<Self>) {
        let task = match invoke(WALLET_SERVER_ID, WALLET_STATUS_TOOL, json!({})) {
            Ok(task) => task,
            Err(err) => {
                if let Some(z) = self.zap.as_mut() {
                    z.wallet = WalletBadge::Unavailable(err);
                }
                return;
            }
        };
        self._zap_wallet_task = Some(cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                if let Some(z) = this.zap.as_mut() {
                    z.wallet = match res {
                        Ok(raw) => match parse_wallet_status(&raw) {
                            Ok(summary) => WalletBadge::Ready(summary),
                            Err(err) => WalletBadge::Unavailable(err),
                        },
                        Err(err) => {
                            WalletBadge::Unavailable(sanitize_untrusted_text(&err.to_string()))
                        }
                    };
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Execute a NIP-57 Lightning zap or NIP-61 Cashu nutzap.
    fn execute_zap(&mut self, cx: &mut Context<Self>) {
        let custom_amount_text = self
            .zap_amount_editor
            .as_ref()
            .map(|ed| ed.read(cx).text(cx))
            .unwrap_or_default();
        let comment_text = self
            .zap_comment_editor
            .as_ref()
            .map(|ed| ed.read(cx).text(cx).trim().to_string())
            .unwrap_or_default();

        let Some(zap) = self.zap.as_mut() else {
            return;
        };
        if zap.is_busy() {
            return;
        }
        if !custom_amount_text.trim().is_empty() {
            match parse_custom_sats(&custom_amount_text) {
                Ok(sats) => zap.amount_sats = sats,
                Err(err) => {
                    zap.status = ZapStatus::Failed(err);
                    cx.notify();
                    return;
                }
            }
        }
        let target_event_id = zap.target_event_id.clone();
        let target_pubkey = zap.target_pubkey.clone();
        let amount_sats = zap.amount_sats;
        let rail_mode = zap.rail;
        let cached_wallet = zap.wallet.clone();

        if rail_mode == ZapRail::Nutzap {
            let summary = match cached_wallet {
                WalletBadge::Ready(summary) => summary,
                WalletBadge::Unavailable(err) => {
                    zap.status = ZapStatus::Failed(format!("Wallet unavailable: {err}"));
                    cx.notify();
                    return;
                }
                WalletBadge::Loading => {
                    zap.status = ZapStatus::Failed(
                        "Still loading wallet status; wait a moment and retry.".to_string(),
                    );
                    cx.notify();
                    return;
                }
            };
            let Some(mint_url) = summary.default_mint.clone() else {
                zap.status = ZapStatus::Failed(
                    "No allowlisted Cashu mint configured for NIP-61 nutzaps.".to_string(),
                );
                cx.notify();
                return;
            };
            let allowed_mints = summary.allowed_mints;
            let p2pk_pubkey = format!("02{}", target_pubkey.to_ascii_lowercase());
            zap.status = ZapStatus::PayingInvoice;
            cx.notify();

            let idempotency_key = format!(
                "nostr-nutzap-{}-{}",
                target_event_id,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            let mut ecash_args = json!({
                "amount_sats": amount_sats,
                "mint_url": mint_url,
                "p2pk_pubkey": p2pk_pubkey,
                "idempotency_key": idempotency_key,
            });
            if !comment_text.is_empty() {
                ecash_args["memo"] = json!(comment_text);
            }
            let send_task = match invoke(WALLET_SERVER_ID, WALLET_SEND_ECASH_TOOL, ecash_args) {
                Ok(t) => t,
                Err(err) => {
                    if let Some(z) = self.zap.as_mut() {
                        z.status = ZapStatus::Failed(err);
                    }
                    cx.notify();
                    return;
                }
            };
            self._zap_task = Some(cx.spawn(async move |this, cx| {
                let ecash_raw = match send_task.await {
                    Ok(raw) => raw,
                    Err(err) => {
                        this.update(cx, |this, cx| {
                            if let Some(z) = this.zap.as_mut() {
                                z.status =
                                    ZapStatus::Failed(sanitize_untrusted_text(&err.to_string()));
                            }
                            cx.notify();
                        })
                        .log_err();
                        return;
                    }
                };
                let proofs_json: Vec<String> = serde_json::from_str::<Value>(&ecash_raw)
                    .ok()
                    .and_then(|v| {
                        let root = v.get("content").unwrap_or(&v);
                        root.get("proofs_json")
                            .or_else(|| root.get("proofs"))
                            .and_then(Value::as_array)
                            .map(|arr| {
                                arr.iter()
                                    .map(|item| {
                                        item.as_str()
                                            .map(str::to_string)
                                            .unwrap_or_else(|| item.to_string())
                                    })
                                    .collect()
                            })
                    })
                    .unwrap_or_default();
                if proofs_json.is_empty() {
                    this.update(cx, |this, cx| {
                        if let Some(z) = this.zap.as_mut() {
                            z.status = ZapStatus::Failed(
                                "wallet_send_ecash did not return P2PK-locked Cashu proofs."
                                    .to_string(),
                            );
                        }
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
                let mut nutzap_args = json!({
                    "recipient": target_pubkey,
                    "recipient_p2pk_pubkey": p2pk_pubkey,
                    "mint_url": mint_url,
                    "allowed_mints": allowed_mints,
                    "proofs_json": proofs_json,
                    "event_id": target_event_id,
                });
                if !comment_text.is_empty() {
                    nutzap_args["comment"] = json!(comment_text);
                }
                let pub_task = match invoke(NOSTR_CLIENT_SERVER, PUBLISH_NUTZAP_TOOL, nutzap_args) {
                    Ok(t) => t,
                    Err(err) => {
                        this.update(cx, |this, cx| {
                            if let Some(z) = this.zap.as_mut() {
                                z.status = ZapStatus::Failed(err);
                            }
                            cx.notify();
                        })
                        .log_err();
                        return;
                    }
                };
                let pub_res = pub_task.await;
                this.update(cx, |this, cx| {
                    let settled = pub_res.is_ok();
                    if let Some(z) = this.zap.as_mut() {
                        z.status = match pub_res {
                            Ok(_) => ZapStatus::Settled(format!(
                                "Nutzapped {amount_sats} sats via Cashu (NIP-61)"
                            )),
                            Err(err) => {
                                ZapStatus::Failed(sanitize_untrusted_text(&err.to_string()))
                            }
                        };
                    }
                    if settled {
                        cx.emit(ZapSettled {
                            target_event_id: target_event_id.clone(),
                            amount_sats,
                            optimistic_bump: false,
                        });
                    }
                    cx.notify();
                })
                .log_err();
            }));
            return;
        }

        // NIP-57 Lightning zap flow.
        zap.status = ZapStatus::ResolvingProfile;
        cx.notify();

        let profile_task = match invoke(
            NOSTR_CLIENT_SERVER,
            PROFILE_TOOL,
            json!({ "authors": [target_pubkey] }),
        ) {
            Ok(task) => task,
            Err(err) => {
                if let Some(z) = self.zap.as_mut() {
                    z.status = ZapStatus::Failed(err);
                }
                cx.notify();
                return;
            }
        };

        self._zap_task = Some(cx.spawn(async move |this, cx| {
            // 1. Resolve recipient Lightning Address (`lud16`) from kind:0 profile.
            let profile_raw = match profile_task.await {
                Ok(raw) => raw,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        if let Some(z) = this.zap.as_mut() {
                            z.status = ZapStatus::Failed(format!(
                                "Profile lookup failed: {}",
                                sanitize_untrusted_text(&err.to_string())
                            ));
                        }
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };
            let lud16 = extract_lud16_from_profile_response(&profile_raw);

            let Some(lud16) = lud16 else {
                this.update(cx, |this, cx| {
                    if let Some(z) = this.zap.as_mut() {
                        z.status = ZapStatus::Failed(
                            "Recipient profile has no Lightning Address (lud16). Switch to Cashu Nutzap (NIP-61) instead."
                                .to_string(),
                        );
                    }
                    cx.notify();
                })
                .log_err();
                return;
            };

            // 2. Prepare NIP-57 zap request + verified BOLT11 invoice.
            this.update(cx, |this, cx| {
                if let Some(z) = this.zap.as_mut() {
                    z.status = ZapStatus::PreparingInvoice;
                }
                cx.notify();
            })
            .log_err();
            let mut prep_args = json!({
                "recipient": target_pubkey,
                "lud16": lud16,
                "amount_sats": amount_sats,
                "event_id": target_event_id,
            });
            if !comment_text.is_empty() {
                prep_args["comment"] = json!(comment_text);
            }
            let prep_task = match invoke(NOSTR_CLIENT_SERVER, PREPARE_ZAP_TOOL, prep_args) {
                Ok(t) => t,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        if let Some(z) = this.zap.as_mut() {
                            z.status = ZapStatus::Failed(err);
                        }
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };
            let prep_raw = match prep_task.await {
                Ok(raw) => raw,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        if let Some(z) = this.zap.as_mut() {
                            z.status = ZapStatus::Failed(sanitize_untrusted_text(&err.to_string()));
                        }
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };
            let Some(bolt11) = serde_json::from_str::<Value>(&prep_raw)
                .ok()
                .and_then(|v| v.get("bolt11").and_then(Value::as_str).map(str::to_string))
            else {
                this.update(cx, |this, cx| {
                    if let Some(z) = this.zap.as_mut() {
                        z.status = ZapStatus::Failed(
                            "nostr_prepare_zap did not return a verified bolt11 invoice."
                                .to_string(),
                        );
                    }
                    cx.notify();
                })
                .log_err();
                return;
            };

            // 3. Verify active wallet rail via `wallet_status` — never silently default.
            let rail_result: Result<String, String> = match cached_wallet {
                WalletBadge::Ready(summary) => Ok(summary.active_rail),
                _ => match invoke(WALLET_SERVER_ID, WALLET_STATUS_TOOL, json!({})) {
                    Ok(status_task) => match status_task.await {
                        Ok(raw) => parse_wallet_status(&raw).map(|s| s.active_rail),
                        Err(err) => Err(sanitize_untrusted_text(&err.to_string())),
                    },
                    Err(err) => Err(err),
                },
            };
            let rail = match rail_result {
                Ok(rail) => rail,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        if let Some(z) = this.zap.as_mut() {
                            z.status = ZapStatus::Failed(format!("Wallet unavailable: {err}"));
                        }
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };

            this.update(cx, |this, cx| {
                if let Some(z) = this.zap.as_mut() {
                    z.status = ZapStatus::PayingInvoice;
                }
                cx.notify();
            })
            .log_err();
            let idempotency_key = format!(
                "nostr-zap-{}-{}",
                target_event_id,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            let pay_task = match invoke(
                WALLET_SERVER_ID,
                WALLET_PAY_INVOICE_TOOL,
                json!({
                    "rail": rail,
                    "bolt11": bolt11,
                    "idempotency_key": idempotency_key,
                }),
            ) {
                Ok(t) => t,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        if let Some(z) = this.zap.as_mut() {
                            z.status = ZapStatus::Failed(err);
                        }
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };
            let pay_result = pay_task.await;
            this.update(cx, |this, cx| {
                let settled = pay_result.is_ok();
                if let Some(z) = this.zap.as_mut() {
                    z.status = match pay_result {
                        Ok(_) => ZapStatus::Settled(format!(
                            "Zapped {amount_sats} sats via {rail} (NIP-57)"
                        )),
                        Err(err) => ZapStatus::Failed(sanitize_untrusted_text(&err.to_string())),
                    };
                }
                if settled {
                    cx.emit(ZapSettled {
                        target_event_id: target_event_id.clone(),
                        amount_sats,
                        optimistic_bump: true,
                    });
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn render_zap_strip(&self, zap: &ZapDraftState, cx: &Context<Self>) -> AnyElement {
        let busy = zap.is_busy();
        let status_line: Option<(String, Color)> = match &zap.status {
            ZapStatus::Ready => None,
            ZapStatus::ResolvingProfile => Some((
                "Resolving recipient Lightning Address…".into(),
                Color::Muted,
            )),
            ZapStatus::PreparingInvoice => Some((
                "Awaiting Zap approval & BOLT11 invoice…".into(),
                Color::Muted,
            )),
            ZapStatus::PayingInvoice => {
                Some(("Awaiting wallet spend approval…".into(), Color::Muted))
            }
            ZapStatus::Settled(msg) => Some((msg.clone(), Color::Success)),
            ZapStatus::Failed(msg) => Some((msg.clone(), Color::Error)),
        };
        let (wallet_badge, wallet_color) = match &zap.wallet {
            WalletBadge::Loading => ("Checking wallet status…".to_string(), Color::Muted),
            WalletBadge::Ready(summary) => (summary.badge_text(), Color::Hint),
            WalletBadge::Unavailable(err) => (format!("Wallet: {err}"), Color::Error),
        };

        v_flex()
            .w_full()
            .p_2()
            .gap_1()
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .bg(cx.theme().colors().elevated_surface_background)
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .flex_wrap()
                    .items_center()
                    .child(
                        Label::new(format!("Zap {}", zap.target_label))
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(wallet_badge)
                            .size(LabelSize::XSmall)
                            .color(wallet_color)
                            .truncate(),
                    )
                    .child(
                        h_flex()
                            .ml_auto()
                            .gap_1()
                            .child(
                                ui::Button::new("nostr-zap-rail-ln", "Lightning")
                                    .label_size(LabelSize::XSmall)
                                    .toggle_state(zap.rail == ZapRail::Lightning)
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(z) = this.zap.as_mut() {
                                            z.rail = ZapRail::Lightning;
                                            z.status = ZapStatus::Ready;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                ui::Button::new("nostr-zap-rail-nutzap", "Cashu Nutzap")
                                    .label_size(LabelSize::XSmall)
                                    .toggle_state(zap.rail == ZapRail::Nutzap)
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(z) = this.zap.as_mut() {
                                            z.rail = ZapRail::Nutzap;
                                            z.status = ZapStatus::Ready;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .flex_wrap()
                    .items_center()
                    .children(ZAP_PRESETS_SATS.iter().map(|&sats| {
                        let selected = zap.amount_sats == sats;
                        ui::Button::new(("nostr-zap-preset", sats as usize), format!("{sats}"))
                            .label_size(LabelSize::XSmall)
                            .toggle_state(selected)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(z) = this.zap.as_mut() {
                                    z.amount_sats = sats;
                                    z.status = ZapStatus::Ready;
                                }
                                if let Some(ed) = &this.zap_amount_editor {
                                    ed.update(cx, |ed, cx| ed.set_text("", window, cx));
                                }
                                cx.notify();
                            }))
                    }))
                    .when_some(self.zap_amount_editor.clone(), |row, ed| {
                        row.child(
                            div()
                                .w(px(88.))
                                .px_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(cx.theme().colors().border)
                                .bg(cx.theme().colors().editor_background)
                                .child(ed),
                        )
                    })
                    .child(
                        ui::Button::new(
                            "nostr-zap-send",
                            if busy {
                                "Zapping…".to_string()
                            } else if zap.rail == ZapRail::Nutzap {
                                format!("Nutzap {} sats", zap.amount_sats)
                            } else {
                                format!("Zap {} sats", zap.amount_sats)
                            },
                        )
                        .style(ui::ButtonStyle::Filled)
                        .label_size(LabelSize::XSmall)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.execute_zap(cx))),
                    ),
            )
            .when_some(self.zap_comment_editor.clone(), |col, ed| {
                col.child(
                    div()
                        .w_full()
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(cx.theme().colors().border)
                        .bg(cx.theme().colors().editor_background)
                        .child(ed),
                )
            })
            .when_some(status_line, |el, (msg, color)| {
                el.child(Label::new(msg).size(LabelSize::XSmall).color(color))
            })
            .into_any_element()
    }
}

impl Render for NostrZapView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(zap) = self.zap.clone() else {
            return div().into_any_element();
        };
        self.ensure_zap_editors(window, cx);
        self.render_zap_strip(&zap, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_open_for_matches_only_active_target() {
        let mut view = NostrZapView::new();
        assert!(!view.is_open_for("ev1"));
        view.zap = Some(ZapDraftState::new(
            "ev1".to_string(),
            "a".repeat(64),
            "@alice".to_string(),
        ));
        assert!(view.is_open_for("ev1"));
        assert!(!view.is_open_for("ev2"));
    }
}
