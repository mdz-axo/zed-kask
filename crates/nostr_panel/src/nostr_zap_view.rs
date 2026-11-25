//! Self-contained NIP-57 Lightning Zap & NIP-61 Cashu Nutzap strip view.
//!
//! Extracted from `nostr_viewer.rs` (Experiment 3c): owns the 5 zap fields
//! (`zap`, `zap_amount_editor`, `zap_comment_editor`, `_zap_wallet_task`,
//! `_zap_task`) and emits `ZapSettled` so `NostrViewer` can optimistically
//! bump zap totals and refresh reactions without owning zap internals.

use gpui::{AnyElement, Context, Entity, EventEmitter, IntoElement, Render, Task, Window};
use hkask_ipc_wire::nostr_wire::NOSTR_CLIENT_SERVER;
use hkask_ipc_wire::wallet_wire::WALLET_SERVER_ID;
use serde_json::{Value, json};
use ui::{Label, LabelSize, prelude::*};
use util::ResultExt as _;

use crate::nostr_composer::{
    PREPARE_ZAP_TOOL, PUBLISH_NUTZAP_TOOL, WALLET_PAY_INVOICE_TOOL, WALLET_SEND_ECASH_TOOL,
    WALLET_STATUS_TOOL, WalletBadge, ZAP_PRESETS_SATS, ZapDraftState, ZapRail, ZapStatus,
    parse_custom_sats, parse_wallet_status, sanitize_untrusted_text,
};
use crate::nostr_viewer::{FETCH_TOOL, PROFILE_TOOL, extract_lud16_from_profile_response, invoke};

/// Validated NIP-61 recipient nutzap target extracted from their `kind: 10019`
/// event (`https://nostr-nips.com/nip-61`). Outbound nutzaps fail closed unless
/// a matching mint and valid P2PK pubkey are confirmed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedNutzapTarget {
    pub(crate) mint_url: String,
    pub(crate) p2pk_pubkey: String,
}

fn normalize_mint_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Parse a `nostr_fetch_events` JSON response for a recipient's `kind: 10019`
/// NIP-61 nutzap info event and verify at least one mint overlaps with
/// `allowed_mints` plus a valid `pubkey` tag.
pub(crate) fn parse_nutzap_info_from_events(
    raw_json: &str,
    allowed_mints: &[String],
) -> Result<ValidatedNutzapTarget, String> {
    let value: Value = serde_json::from_str(raw_json)
        .map_err(|err| format!("kind:10019 lookup returned invalid JSON: {err}"))?;
    let root = value.get("content").unwrap_or(&value);
    let events = root
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "Recipient has no NIP-61 kind:10019 nutzap info on configured relays.".to_string()
        })?;

    let mut newest: Option<(u64, &Value)> = None;
    for row in events {
        let ev = row.get("event").unwrap_or(row);
        if ev.get("kind").and_then(Value::as_u64) != Some(10_019) {
            continue;
        }
        let created_at = ev.get("created_at").and_then(Value::as_u64).unwrap_or(0);
        if newest.as_ref().is_none_or(|(at, _)| created_at >= *at) {
            newest = Some((created_at, ev));
        }
    }
    let Some((_, ev)) = newest else {
        return Err(
            "Recipient has not published NIP-61 kind:10019 nutzap info; refusing to send unredeemable Cashu proofs."
                .to_string(),
        );
    };
    let raw_tags = ev
        .get("tags")
        .and_then(Value::as_array)
        .ok_or_else(|| "Recipient kind:10019 event has no tags.".to_string())?;
    let tags: Vec<Vec<String>> = raw_tags
        .iter()
        .filter_map(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .collect();
    let author = ev
        .get("author")
        .or_else(|| ev.get("pubkey"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let parsed =
        hkask_nostr_core::parse_nutzap_info_tags(author, hkask_nostr_core::NUTZAP_INFO_KIND, &tags)
            .map_err(|err| format!("Recipient kind:10019 nutzap info is invalid: {err}"))?;

    let matched_mint = parsed
        .mints
        .iter()
        .filter(|rm| rm.starts_with("https://"))
        .find_map(|rm| {
            let norm_r = normalize_mint_url(rm);
            allowed_mints
                .iter()
                .find(|am| normalize_mint_url(am) == norm_r)
                .cloned()
        });
    let Some(mint_url) = matched_mint else {
        return Err(
            "No overlapping Cashu mint between your allowed mints and recipient's kind:10019 nutzap info."
                .to_string(),
        );
    };

    Ok(ValidatedNutzapTarget {
        mint_url,
        p2pk_pubkey: parsed.p2pk_pubkey,
    })
}

/// Event emitted when a NIP-57 Lightning zap or NIP-61 Cashu nutzap settles.
pub(crate) struct ZapSettled {
    pub(crate) target_event_id: String,
    pub(crate) amount_sats: u64,
    pub(crate) optimistic_bump: bool,
}

/// Already-minted P2PK-locked Cashu proofs awaiting relay publication so a
/// retry after `nostr_publish_nutzap` failure re-publishes the existing proofs
/// instead of double-spending via `wallet_send_ecash`.
#[derive(Debug, Clone)]
struct PendingNutzapPublish {
    target_event_id: String,
    target_pubkey: String,
    p2pk_pubkey: String,
    mint_url: String,
    allowed_mints: Vec<String>,
    proofs_json: Vec<String>,
    amount_sats: u64,
    comment: String,
}

fn nutzap_relays_accepted(raw_json: &str) -> Result<usize, String> {
    let value: Value = serde_json::from_str(raw_json)
        .map_err(|e| format!("nostr_publish_nutzap returned invalid JSON: {e}"))?;
    let root = value.get("content").unwrap_or(&value);
    let accepted = root
        .get("relays")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|r| {
                    matches!(
                        r.get("state").and_then(Value::as_str),
                        Some("accepted" | "ok" | "sent")
                    ) || r.get("accepted").and_then(Value::as_bool) == Some(true)
                })
                .count()
        })
        .unwrap_or(0);
    Ok(accepted)
}

/// Self-contained GPUI view for the Amethyst-style quick-zap strip.
pub(crate) struct NostrZapView {
    zap: Option<ZapDraftState>,
    pending_nutzap: Option<PendingNutzapPublish>,
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
            pending_nutzap: None,
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

    /// Open the zap strip pre-filled with a bounded auto-zap amount (`amount_sats`)
    /// and fetch wallet status so the operator can confirm or dispatch under wallet caps.
    pub(crate) fn open_with_amount(
        &mut self,
        target_event_id: String,
        recipient_pubkey: String,
        recipient_label: String,
        amount_sats: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_zap_editors(window, cx);
        self.ensure_zap_editors(window, cx);
        let mut draft = ZapDraftState::new(target_event_id, recipient_pubkey, recipient_label);
        draft.amount_sats = amount_sats;
        self.zap = Some(draft);
        if let Some(ed) = &self.zap_amount_editor {
            ed.update(cx, |e, cx| e.set_text(amount_sats.to_string(), window, cx));
        }
        self.fetch_zap_wallet_status(cx);
        cx.notify();
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
            // If proofs were already debited for this target and relay publish
            // failed earlier, re-publish those exact proofs instead of
            // calling `wallet_send_ecash` again (prevents double-spend).
            if let Some(pending) = self
                .pending_nutzap
                .clone()
                .filter(|p| p.target_event_id == target_event_id)
            {
                zap.status = ZapStatus::PayingInvoice;
                cx.notify();
                let mut nutzap_args = json!({
                    "recipient": pending.target_pubkey,
                    "recipient_p2pk_pubkey": pending.p2pk_pubkey,
                    "mint_url": pending.mint_url,
                    "allowed_mints": pending.allowed_mints,
                    "proofs_json": pending.proofs_json,
                    "event_id": pending.target_event_id,
                });
                if !pending.comment.is_empty() {
                    nutzap_args["comment"] = json!(pending.comment);
                }
                let pub_task = match invoke(NOSTR_CLIENT_SERVER, PUBLISH_NUTZAP_TOOL, nutzap_args) {
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
                    let pub_res = pub_task.await;
                    this.update(cx, |this, cx| {
                        let settled = match &pub_res {
                            Ok(raw) => nutzap_relays_accepted(raw).unwrap_or(0) > 0,
                            Err(_) => false,
                        };
                        if let Some(z) = this.zap.as_mut() {
                            z.status = match pub_res {
                                Ok(ref raw) => match nutzap_relays_accepted(raw) {
                                    Ok(n) if n > 0 => ZapStatus::Settled(format!(
                                        "Nutzapped {} sats via Cashu (NIP-61 · {n} relay(s))",
                                        pending.amount_sats
                                    )),
                                    Ok(_) => ZapStatus::Failed(
                                        "Proofs already created, but 0 relays accepted kind:9321; click Nutzap to retry publishing without re-spending."
                                            .to_string(),
                                    ),
                                    Err(err) => ZapStatus::Failed(err),
                                },
                                Err(err) => {
                                    ZapStatus::Failed(sanitize_untrusted_text(&err.to_string()))
                                }
                            };
                        }
                        if settled {
                            this.pending_nutzap = None;
                            cx.emit(ZapSettled {
                                target_event_id: pending.target_event_id.clone(),
                                amount_sats: pending.amount_sats,
                                optimistic_bump: false,
                            });
                        }
                        cx.notify();
                    })
                    .log_err();
                }));
                return;
            }
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
            if summary.allowed_mints.is_empty() {
                zap.status = ZapStatus::Failed(
                    "No allowlisted Cashu mint configured for NIP-61 nutzaps.".to_string(),
                );
                cx.notify();
                return;
            }
            let allowed_mints = summary.allowed_mints;
            zap.status = ZapStatus::ResolvingProfile;
            cx.notify();

            let info_task = match invoke(
                NOSTR_CLIENT_SERVER,
                FETCH_TOOL,
                json!({ "authors": [target_pubkey], "kinds": [10_019], "limit": 5 }),
            ) {
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
                let info_raw = match info_task.await {
                    Ok(raw) => raw,
                    Err(err) => {
                        this.update(cx, |this, cx| {
                            if let Some(z) = this.zap.as_mut() {
                                z.status = ZapStatus::Failed(format!(
                                    "Could not verify recipient NIP-61 kind:10019 nutzap info: {}",
                                    sanitize_untrusted_text(&err.to_string())
                                ));
                            }
                            cx.notify();
                        })
                        .log_err();
                        return;
                    }
                };
                let validated = match parse_nutzap_info_from_events(&info_raw, &allowed_mints) {
                    Ok(v) => v,
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
                let mint_url = validated.mint_url;
                let p2pk_pubkey = validated.p2pk_pubkey;

                this.update(cx, |this, cx| {
                    if let Some(z) = this.zap.as_mut() {
                        z.status = ZapStatus::PayingInvoice;
                    }
                    cx.notify();
                })
                .log_err();

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
                let pending_record = PendingNutzapPublish {
                    target_event_id: target_event_id.clone(),
                    target_pubkey: target_pubkey.clone(),
                    p2pk_pubkey: p2pk_pubkey.clone(),
                    mint_url: mint_url.clone(),
                    allowed_mints: allowed_mints.clone(),
                    proofs_json: proofs_json.clone(),
                    amount_sats,
                    comment: comment_text.clone(),
                };
                this.update(cx, |this, _| {
                    this.pending_nutzap = Some(pending_record.clone());
                })
                .log_err();
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
                    let settled = match &pub_res {
                        Ok(raw) => nutzap_relays_accepted(raw).unwrap_or(0) > 0,
                        Err(_) => false,
                    };
                    if let Some(z) = this.zap.as_mut() {
                        z.status = match pub_res {
                            Ok(ref raw) => match nutzap_relays_accepted(raw) {
                                Ok(n) if n > 0 => ZapStatus::Settled(format!(
                                    "Nutzapped {amount_sats} sats via Cashu (NIP-61 · {n} relay(s))"
                                )),
                                Ok(_) => ZapStatus::Failed(
                                    "Proofs created, but 0 relays accepted kind:9321; click Nutzap to retry publishing without re-spending."
                                        .to_string(),
                                ),
                                Err(err) => ZapStatus::Failed(err),
                            },
                            Err(err) => {
                                ZapStatus::Failed(sanitize_untrusted_text(&err.to_string()))
                            }
                        };
                    }
                    if settled {
                        this.pending_nutzap = None;
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
                        div().min_w(px(32.)).child(
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
                                })),
                        )
                    })),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .min_w_0()
                    .items_center()
                    .when_some(self.zap_amount_editor.clone(), |row, ed| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(cx.theme().colors().border)
                                .bg(cx.theme().colors().editor_background)
                                .child(ed),
                        )
                    })
                    .child(
                        div().flex_shrink_0().child(
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

    #[test]
    fn parse_nutzap_info_fails_closed_without_valid_10019_and_matching_mint() {
        let allowed = vec!["https://mint.example.com".to_string()];
        let pk64 = "b".repeat(64);

        // Missing kind:10019 -> refused
        assert!(parse_nutzap_info_from_events(r#"{"events":[]}"#, &allowed).is_err());

        // Missing pubkey tag -> refused
        let no_pk = r#"{"events":[{"kind":10019,"created_at":10,"tags":[["mint","https://mint.example.com/"]]}]}"#;
        assert!(parse_nutzap_info_from_events(no_pk, &allowed).is_err());

        // Non-overlapping mint -> refused
        let wrong_mint = format!(
            r#"{{"events":[{{"kind":10019,"created_at":10,"tags":[["pubkey","{pk64}"],["mint","https://other.mint"]]}}]}}"#
        );
        assert!(parse_nutzap_info_from_events(&wrong_mint, &allowed).is_err());

        // Valid kind:10019 with trailing-slash mint normalization -> accepted
        let valid = format!(
            r#"{{"events":[{{"kind":10019,"created_at":20,"tags":[["pubkey","{pk64}"],["mint","https://mint.example.com/"]]}}]}}"#
        );
        let parsed = parse_nutzap_info_from_events(&valid, &allowed).expect("valid 10019");
        assert_eq!(parsed.mint_url, "https://mint.example.com");
        assert_eq!(parsed.p2pk_pubkey, format!("02{pk64}"));
    }
}
