//! Note composition state machine, validation, and publish-receipt parsing for
//! the Nostr panel's collapsible compose drawer.
//!
//! Signing never happens here: valid drafts are dispatched to
//! `nostr_publish_note` on `hkask-mcp-nostr-client`, which asks the editor over
//! the inference IPC wire (`kask_bridge/src/nostr_signing.rs`) for operator
//! approval before touching the keychain.

use hkask_types::nostr_wire::{MAX_CONTENT_CHARS, MAX_MEDIA_ATTACHMENTS, WireEvent};
use serde_json::Value;

/// The MCP tool on `hkask-mcp-nostr-client` that publishes a `kind: 1` note
/// (NIP-01: <https://nostr-nips.com/nip-01>).
pub(crate) const PUBLISH_TOOL: &str = "nostr_publish_note";

/// The MCP tool that publishes a NIP-25 `kind: 7` reaction
/// (<https://nostr-nips.com/nip-25>).
pub(crate) const PUBLISH_REACTION_TOOL: &str = "nostr_publish_reaction";

/// The MCP tool that publishes a NIP-10 (`kind: 1`, <https://nostr-nips.com/nip-10>)
/// or NIP-22 (`kind: 1111`, <https://nostr-nips.com/nip-22>) reply/comment.
pub(crate) const PUBLISH_REPLY_TOOL: &str = "nostr_publish_reply";

/// The MCP tool that publishes a NIP-09 `kind: 5` event deletion request
/// (<https://nostr-nips.com/nip-09>). Always requires native editor approval.
pub(crate) const PUBLISH_DELETION_TOOL: &str = "nostr_publish_deletion";

/// The MCP tool that publishes a NIP-18 repost (`kind: 6` or `kind: 16`,
/// <https://nostr-nips.com/nip-18>).
pub(crate) const PUBLISH_REPOST_TOOL: &str = "nostr_publish_repost";

/// The MCP tool that publishes a NIP-02 `kind: 3` follow list
/// (<https://nostr-nips.com/nip-02>). Always requires native editor approval.
pub(crate) const PUBLISH_FOLLOWS_TOOL: &str = "nostr_publish_follows";

/// The MCP tool that publishes a NIP-23 `kind: 30023` long-form Markdown article
/// (<https://nostr-nips.com/nip-23>). Always requires native editor approval.
pub(crate) const PUBLISH_ARTICLE_TOOL: &str = "nostr_publish_article";

/// The MCP tool that publishes a NIP-51 `kind: 10000` (mute) or `kind: 10003`
/// (bookmark) list (<https://nostr-nips.com/nip-51>). Always requires native editor approval.
pub(crate) const PUBLISH_LIST_TOOL: &str = "nostr_publish_list";

/// Convert an article title into a clean ASCII NIP-23 `d`-tag slug.
pub(crate) fn slugify_article_title(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !slug.is_empty() && !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
        if slug.len() >= 64 {
            break;
        }
    }
    slug.trim_matches('-').to_string()
}

/// The MCP tool that prepares a verified NIP-57 Lightning zap invoice
/// (<https://nostr-nips.com/nip-57>).
pub(crate) const PREPARE_ZAP_TOOL: &str = "nostr_prepare_zap";

/// The MCP tool that publishes a NIP-61 Cashu nutzap (`kind: 9321`,
/// <https://nostr-nips.com/nip-61>).
pub(crate) const PUBLISH_NUTZAP_TOOL: &str = "nostr_publish_nutzap";

/// The MCP tools on `hkask-mcp-wallet` used to inspect rails and settle or redeem a zap/nutzap.
pub(crate) const WALLET_STATUS_TOOL: &str = "wallet_status";
pub(crate) const WALLET_PAY_INVOICE_TOOL: &str = "wallet_pay_invoice";
pub(crate) const WALLET_SEND_ECASH_TOOL: &str = "wallet_send_ecash";
pub(crate) const WALLET_RECEIVE_ECASH_TOOL: &str = "wallet_receive_ecash";

/// Amethyst-style quick-zap preset amounts in satoshis.
pub(crate) const ZAP_PRESETS_SATS: &[u64] = &[21, 100, 500, 1_000];

/// Quick reaction choices `(wire_content, display_label)` for NIP-25 `kind: 7`.
pub(crate) const QUICK_REACTIONS: &[(&str, &str)] = &[
    ("+", "👍"),
    ("-", "👎"),
    ("🔥", "🔥"),
    ("🤙", "🤙"),
    ("❤", "❤"),
];

/// Maximum Markdown characters in one NIP-23 `kind: 30023` long-form article.
pub(crate) const MAX_ARTICLE_CHARS: usize = 64_000;

/// Character count at which the live counter switches to warning emphasis.
pub(crate) const CHAR_WARNING_THRESHOLD: usize = 3_800;

/// Why a draft cannot be submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DraftValidationError {
    Empty,
    TooLong { chars: usize, max: usize },
}

impl DraftValidationError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Empty => "Write a note before publishing.".to_string(),
            Self::TooLong { chars, max } => {
                format!("Note is {chars} characters; maximum is {max}.")
            }
        }
    }
}

/// One relay's reported outcome from `nostr_publish_note`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelayOutcome {
    pub(crate) address: String,
    pub(crate) state: String,
    pub(crate) reason: Option<String>,
}

/// Aggregated publish receipt parsed from `nostr_publish_note`'s JSON response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishReceipt {
    pub(crate) event_id: Option<String>,
    pub(crate) accepted: Vec<RelayOutcome>,
    pub(crate) failed: Vec<RelayOutcome>,
    pub(crate) read_back_found: bool,
}

/// Classification of a completed `nostr_publish_note` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishOutcome {
    Published(PublishReceipt),
    PartialFailure(PublishReceipt),
    AllFailed(PublishReceipt),
}

impl PublishOutcome {
    pub(crate) fn receipt(&self) -> &PublishReceipt {
        match self {
            Self::Published(r) | Self::PartialFailure(r) | Self::AllFailed(r) => r,
        }
    }

    pub(crate) fn any_accepted(&self) -> bool {
        !matches!(self, Self::AllFailed(_))
    }
}

/// Validated payload ready to send to `nostr_publish_note` or `nostr_publish_reply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishPayload {
    pub(crate) content: String,
    pub(crate) created_at: u64,
    pub(crate) media_paths: Vec<String>,
}

/// Snapshot of the most recently submitted draft so `Retry` works after partial
/// or total relay failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LastSubmittedPayload {
    pub(crate) content: String,
    pub(crate) reply_target: Option<ReplyContext>,
    pub(crate) quote_target: Option<QuoteContext>,
    pub(crate) media_paths: Vec<String>,
}

/// Lifecycle phase of the collapsible compose drawer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum ComposerPhase {
    #[default]
    Idle,
    Drafting,
    Publishing,
}

/// Target context when the compose drawer is replying/commenting on an event
/// (NIP-10 for `kind: 1` roots; NIP-22 `kind: 1111` for non-`kind: 1` roots).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplyContext {
    pub(crate) root_event_id: String,
    pub(crate) reply_to_event_id: Option<String>,
    pub(crate) target_pubkey: String,
    pub(crate) target_label: String,
    pub(crate) root_kind: u16,
}

impl ReplyContext {
    pub(crate) fn nip_label(&self) -> &'static str {
        if self.root_kind == 1 {
            "NIP-10"
        } else {
            "NIP-22"
        }
    }

    /// Derive a NIP-10 or NIP-22 reply target from an existing `WireEvent`:
    /// - `kind: 1111` (NIP-22): preserves uppercase `E` root event id and uppercase
    ///   `K` root kind (e.g. `30023` article) while setting `reply_to_event_id`
    ///   to this comment.
    /// - `kind: 1` (NIP-10): checks marked `["e", id, ..., "root"]` first, then
    ///   legacy unmarked positional `e` tags (skipping `"mention"`), else treats
    ///   this note as the thread root.
    /// - Other kinds (e.g. `kind: 30023` article): treats this event as the root
    ///   for a NIP-22 `kind: 1111` comment.
    pub(crate) fn from_event(event: &WireEvent, target_label: String) -> Self {
        if event.kind == 1111 {
            let root_e = event
                .tags
                .iter()
                .find_map(|t| {
                    (t.first().map(String::as_str) == Some("E"))
                        .then(|| t.get(1).filter(|v| !v.is_empty()))
                        .flatten()
                        .map(String::as_str)
                })
                .or_else(|| {
                    event.tags.iter().find_map(|t| {
                        (t.first().map(String::as_str) == Some("e")
                            && t.get(3).map(String::as_str) == Some("root"))
                        .then(|| t.get(1).filter(|v| !v.is_empty()))
                        .flatten()
                        .map(String::as_str)
                    })
                })
                .or_else(|| {
                    event.tags.iter().find_map(|t| {
                        (t.first().map(String::as_str) == Some("e")
                            && t.get(3).map(String::as_str) != Some("mention"))
                        .then(|| t.get(1).filter(|v| !v.is_empty()))
                        .flatten()
                        .map(String::as_str)
                    })
                });
            let root_k = event
                .tags
                .iter()
                .find_map(|t| {
                    (t.first().map(String::as_str) == Some("K"))
                        .then(|| t.get(1).and_then(|v| v.parse::<u16>().ok()))
                        .flatten()
                })
                .unwrap_or(event.kind);
            let root_event_id = root_e.unwrap_or(event.id.as_str()).to_string();
            let reply_to_event_id = if root_event_id != event.id {
                Some(event.id.clone())
            } else {
                None
            };
            return Self {
                root_event_id,
                reply_to_event_id,
                target_pubkey: event.author.clone(),
                target_label,
                root_kind: root_k,
            };
        }

        if event.kind == 1 {
            let marked_root = event.tags.iter().find_map(|t| {
                (t.first().map(String::as_str) == Some("e")
                    && t.get(3).map(String::as_str) == Some("root"))
                .then(|| t.get(1).filter(|v| !v.is_empty()))
                .flatten()
                .map(String::as_str)
            });
            let positional_root = if marked_root.is_none() {
                event.tags.iter().find_map(|t| {
                    let marker = t.get(3).map(String::as_str);
                    (t.first().map(String::as_str) == Some("e") && marker != Some("mention"))
                        .then(|| t.get(1).filter(|v| !v.is_empty()))
                        .flatten()
                        .map(String::as_str)
                })
            } else {
                None
            };
            if let Some(root) = marked_root.or(positional_root) {
                return Self {
                    root_event_id: root.to_string(),
                    reply_to_event_id: Some(event.id.clone()),
                    target_pubkey: event.author.clone(),
                    target_label,
                    root_kind: 1,
                };
            }
        }

        Self {
            root_event_id: event.id.clone(),
            reply_to_event_id: None,
            target_pubkey: event.author.clone(),
            target_label,
            root_kind: event.kind,
        }
    }
}

/// Payment rail selected in the quick-zap strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ZapRail {
    /// NIP-57 Lightning zap (`lud16` → `nostr_prepare_zap` → `wallet_pay_invoice`).
    #[default]
    Lightning,
    /// NIP-61 Cashu nutzap (`wallet_send_ecash` → `nostr_publish_nutzap`).
    Nutzap,
}

/// Parsed subset of `wallet_status` surfaced as a badge in the zap strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalletStatusSummary {
    pub(crate) network: String,
    pub(crate) active_rail: String,
    pub(crate) per_tx_cap_sats: Option<u64>,
    pub(crate) default_mint: Option<String>,
    pub(crate) allowed_mints: Vec<String>,
    pub(crate) p2pk_xonly_pubkey: Option<String>,
}

impl WalletStatusSummary {
    pub(crate) fn badge_text(&self) -> String {
        let mut parts = vec![self.network.clone(), self.active_rail.clone()];
        if let Some(cap) = self.per_tx_cap_sats {
            parts.push(format!("cap {cap} sats/tx"));
        }
        parts.join(" · ")
    }
}

/// Pre-fetched wallet state for the zap strip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum WalletBadge {
    #[default]
    Loading,
    Ready(WalletStatusSummary),
    Unavailable(String),
}

/// Parse `wallet_status` JSON into a `WalletStatusSummary`. Returns `Err` if
/// the response is malformed or reports an error rather than silently
/// defaulting to `"cashu"`.
pub(crate) fn parse_wallet_status(raw_json: &str) -> Result<WalletStatusSummary, String> {
    let value: Value = serde_json::from_str(raw_json)
        .map_err(|err| format!("wallet_status was not valid JSON: {err}"))?;
    if let Some(err) = value.get("error").and_then(|e| {
        e.get("message")
            .and_then(Value::as_str)
            .or_else(|| e.as_str())
    }) {
        return Err(sanitize_untrusted_text(err));
    }
    let root = value.get("content").unwrap_or(&value);
    let network = root
        .get("network")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let nwc_configured = root
        .get("nwc_label")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    let allowed_mints: Vec<String> = root
        .get("allowed_mints")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let default_mint = root
        .get("default_mint")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| allowed_mints.first().cloned());
    let active_rail = if nwc_configured {
        "nwc".to_string()
    } else if !allowed_mints.is_empty() {
        "cashu".to_string()
    } else {
        return Err("No NWC or Cashu mint rail is configured in wallet settings.".to_string());
    };
    let per_tx_cap_sats = root
        .get("per_tx_cap_sats")
        .and_then(Value::as_u64)
        .or_else(|| {
            root.pointer("/spending_caps/per_tx_sats")
                .and_then(Value::as_u64)
        });
    let p2pk_xonly_pubkey = root
        .get("p2pk_xonly_pubkey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|s| s.to_ascii_lowercase());
    Ok(WalletStatusSummary {
        network: sanitize_untrusted_text(&network),
        active_rail,
        per_tx_cap_sats,
        default_mint,
        allowed_mints,
        p2pk_xonly_pubkey,
    })
}

/// Parse a user-typed custom satoshi amount (`> 0`).
pub(crate) fn parse_custom_sats(input: &str) -> Result<u64, String> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',' && *c != '_')
        .collect();
    if cleaned.is_empty() {
        return Err("Enter a satoshi amount.".to_string());
    }
    let sats: u64 = cleaned
        .parse()
        .map_err(|_| "Custom amount must be a whole number of sats.".to_string())?;
    if sats == 0 {
        return Err("Zap amount must be at least 1 sat.".to_string());
    }
    Ok(sats)
}

/// Lifecycle of an in-panel NIP-57 / NIP-61 zap flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ZapStatus {
    Ready,
    ResolvingProfile,
    PreparingInvoice,
    PayingInvoice,
    Settled(String),
    Failed(String),
}

/// State for the compact Amethyst-style quick-zap strip on a selected note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZapDraftState {
    pub(crate) target_event_id: String,
    pub(crate) target_pubkey: String,
    pub(crate) target_label: String,
    pub(crate) amount_sats: u64,
    pub(crate) rail: ZapRail,
    pub(crate) wallet: WalletBadge,
    pub(crate) status: ZapStatus,
}

impl ZapDraftState {
    pub(crate) fn new(
        target_event_id: String,
        target_pubkey: String,
        target_label: String,
    ) -> Self {
        Self {
            target_event_id,
            target_pubkey,
            target_label,
            amount_sats: ZAP_PRESETS_SATS[0],
            rail: ZapRail::Lightning,
            wallet: WalletBadge::Loading,
            status: ZapStatus::Ready,
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        matches!(
            self.status,
            ZapStatus::ResolvingProfile | ZapStatus::PreparingInvoice | ZapStatus::PayingInvoice
        )
    }
}

/// Target context when the compose drawer is quoting an existing event
/// (`NIP-10` `q` + `p` tags on a `kind: 1` note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuoteContext {
    pub(crate) event_id: String,
    pub(crate) target_pubkey: String,
    pub(crate) target_label: String,
}

/// Pure state machine for the collapsible Nostr compose drawer.
#[derive(Debug, Clone, Default)]
pub(crate) struct ComposerState {
    pub(crate) phase: ComposerPhase,
    pub(crate) draft: String,
    pub(crate) reply_target: Option<ReplyContext>,
    pub(crate) quote_target: Option<QuoteContext>,
    pub(crate) media_paths: Vec<String>,
    pub(crate) error: Option<String>,
    pub(crate) last_outcome: Option<PublishOutcome>,
    pub(crate) last_submitted: Option<LastSubmittedPayload>,
}

/// Sanitize untrusted third-party text (relay addresses/reasons) by replacing
/// control characters other than newlines/tabs with U+FFFD.
pub(crate) fn sanitize_untrusted_text(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\n' | '\t' => c,
            c if c.is_control() => '\u{FFFD}',
            c => c,
        })
        .collect()
}

/// Validate a note or article draft against `max_chars`, allowing empty text
/// when at least one Blossom media attachment is attached.
pub(crate) fn validate_draft_with_limit(
    text: &str,
    has_media: bool,
    max_chars: usize,
) -> Result<&str, DraftValidationError> {
    if text.trim().is_empty() && !has_media {
        return Err(DraftValidationError::Empty);
    }
    let chars = text.chars().count();
    if chars > max_chars {
        return Err(DraftValidationError::TooLong {
            chars,
            max: max_chars,
        });
    }
    Ok(text)
}

/// Validate a note draft, allowing empty text when at least one Blossom media
/// attachment is attached (top-level `kind: 1` notes only).
pub(crate) fn validate_draft_with_media(
    text: &str,
    has_media: bool,
) -> Result<&str, DraftValidationError> {
    validate_draft_with_limit(text, has_media, MAX_CONTENT_CHARS)
}

/// Parse `nostr_publish_note`'s JSON response into a typed `PublishOutcome`.
pub(crate) fn parse_publish_receipt(raw_json: &str) -> Result<PublishOutcome, String> {
    let value: Value = serde_json::from_str(raw_json)
        .map_err(|err| format!("publish response was not valid JSON: {err}"))?;
    let event_id = value
        .pointer("/published/id")
        .and_then(Value::as_str)
        .map(sanitize_untrusted_text);
    let read_back_found = value
        .pointer("/read_back/found")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let relays = value
        .get("relays")
        .and_then(Value::as_array)
        .ok_or_else(|| "publish response did not include per-relay outcomes".to_string())?;

    let mut accepted = Vec::new();
    let mut failed = Vec::new();
    for entry in relays {
        let address = entry
            .get("address")
            .and_then(Value::as_str)
            .map(sanitize_untrusted_text)
            .unwrap_or_else(|| "<unknown relay>".to_string());
        let state = entry
            .get("state")
            .and_then(Value::as_str)
            .map(sanitize_untrusted_text)
            .unwrap_or_else(|| "unknown".to_string());
        let reason = entry
            .get("reason")
            .and_then(Value::as_str)
            .or_else(|| entry.get("ok_message").and_then(Value::as_str))
            .filter(|s| !s.trim().is_empty())
            .map(sanitize_untrusted_text);
        let outcome = RelayOutcome {
            address,
            state: state.clone(),
            reason,
        };
        if state == "accepted" {
            accepted.push(outcome);
        } else {
            failed.push(outcome);
        }
    }

    let receipt = PublishReceipt {
        event_id,
        accepted,
        failed,
        read_back_found,
    };

    if receipt.accepted.is_empty() {
        Ok(PublishOutcome::AllFailed(receipt))
    } else if receipt.failed.is_empty() {
        Ok(PublishOutcome::Published(receipt))
    } else {
        Ok(PublishOutcome::PartialFailure(receipt))
    }
}

impl ComposerState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn is_open(&self) -> bool {
        !matches!(self.phase, ComposerPhase::Idle)
    }

    pub(crate) fn is_publishing(&self) -> bool {
        matches!(self.phase, ComposerPhase::Publishing)
    }

    pub(crate) fn has_draft(&self) -> bool {
        !self.draft.trim().is_empty()
            || (!self.media_paths.is_empty() && self.reply_target.is_none())
    }

    pub(crate) fn char_count(&self) -> usize {
        self.draft.chars().count()
    }

    /// Attach a local media file path for Blossom upload (`NIP-92 imeta`,
    /// top-level `kind: 1` notes only, max `MAX_MEDIA_ATTACHMENTS`).
    pub(crate) fn add_media_path(&mut self, path: String) -> Result<(), String> {
        if self.is_publishing() {
            return Err("Cannot change attachments while publishing.".to_string());
        }
        let trimmed = path.trim().to_string();
        if trimmed.is_empty() {
            return Err("Enter a local media file path to attach.".to_string());
        }
        if self.media_paths.len() >= MAX_MEDIA_ATTACHMENTS {
            return Err(format!(
                "At most {MAX_MEDIA_ATTACHMENTS} media files may be attached per note."
            ));
        }
        if !self.media_paths.contains(&trimmed) {
            self.media_paths.push(trimmed);
        }
        self.error = None;
        Ok(())
    }

    pub(crate) fn remove_media_path(&mut self, index: usize) {
        if !self.is_publishing() && index < self.media_paths.len() {
            self.media_paths.remove(index);
        }
    }

    /// Open the drawer if idle, or collapse to idle while preserving the draft.
    pub(crate) fn toggle(&mut self) {
        match self.phase {
            ComposerPhase::Idle => {
                self.phase = ComposerPhase::Drafting;
                self.error = None;
            }
            ComposerPhase::Drafting => {
                self.phase = ComposerPhase::Idle;
            }
            ComposerPhase::Publishing => {}
        }
    }

    /// Open the drawer (no-op if already open or publishing).
    pub(crate) fn open_compose(&mut self) {
        if matches!(self.phase, ComposerPhase::Idle) {
            self.phase = ComposerPhase::Drafting;
            self.error = None;
        }
    }

    /// Open the drawer targeting a specific note/article for a NIP-10 or NIP-22 reply.
    pub(crate) fn open_reply(&mut self, target: ReplyContext) {
        if self.is_publishing() {
            return;
        }
        self.reply_target = Some(target);
        self.quote_target = None;
        self.media_paths.clear();
        self.phase = ComposerPhase::Drafting;
        self.error = None;
    }

    /// Open the drawer targeting a specific event for a NIP-10 quote note (`q` tag).
    pub(crate) fn open_quote(&mut self, target: QuoteContext) {
        if self.is_publishing() {
            return;
        }
        self.quote_target = Some(target);
        self.reply_target = None;
        self.phase = ComposerPhase::Drafting;
        self.error = None;
    }

    /// Clear the active reply or quote target (reverts to a top-level `kind: 1` note).
    pub(crate) fn clear_reply_target(&mut self) {
        if !self.is_publishing() {
            self.reply_target = None;
            self.quote_target = None;
        }
    }

    /// Explicitly discard the draft, clear errors/receipts, and collapse.
    pub(crate) fn discard(&mut self) {
        if self.is_publishing() {
            return;
        }
        self.draft.clear();
        self.reply_target = None;
        self.quote_target = None;
        self.media_paths.clear();
        self.error = None;
        self.last_outcome = None;
        self.last_submitted = None;
        self.phase = ComposerPhase::Idle;
    }

    /// Update the draft text from the editor.
    pub(crate) fn set_draft(&mut self, text: String) {
        self.draft = text;
        let has_media = self.reply_target.is_none() && !self.media_paths.is_empty();
        if validate_draft_with_media(&self.draft, has_media).is_ok() {
            self.error = None;
        }
    }

    /// Validate the current draft against `MAX_CONTENT_CHARS` and enter `Publishing`.
    #[cfg(test)]
    pub(crate) fn begin_publish(
        &mut self,
        now_unix_secs: u64,
    ) -> Result<PublishPayload, DraftValidationError> {
        self.begin_publish_with_limit(now_unix_secs, MAX_CONTENT_CHARS)
    }

    /// Validate the current draft against `max_chars` (4,000 for notes, 64,000
    /// for NIP-23 articles) and enter `Publishing`.
    pub(crate) fn begin_publish_with_limit(
        &mut self,
        now_unix_secs: u64,
        max_chars: usize,
    ) -> Result<PublishPayload, DraftValidationError> {
        let has_media = self.reply_target.is_none() && !self.media_paths.is_empty();
        let content = match validate_draft_with_limit(&self.draft, has_media, max_chars) {
            Ok(valid) => valid.to_string(),
            Err(err) => {
                self.error = Some(err.message());
                return Err(err);
            }
        };
        let media_paths = if self.reply_target.is_none() {
            self.media_paths.clone()
        } else {
            Vec::new()
        };
        self.last_submitted = Some(LastSubmittedPayload {
            content: content.clone(),
            reply_target: self.reply_target.clone(),
            quote_target: self.quote_target.clone(),
            media_paths: media_paths.clone(),
        });
        self.error = None;
        self.last_outcome = None;
        self.phase = ComposerPhase::Publishing;
        Ok(PublishPayload {
            content,
            created_at: now_unix_secs,
            media_paths,
        })
    }

    /// Whether a failed/partial publish receipt can be retried in one click.
    pub(crate) fn can_retry_last(&self) -> bool {
        !self.is_publishing()
            && self.last_submitted.is_some()
            && matches!(
                self.last_outcome,
                Some(PublishOutcome::PartialFailure(_) | PublishOutcome::AllFailed(_))
            )
    }

    /// Restore the last submitted payload into the composer so it can be retried.
    pub(crate) fn restore_last_for_retry(&mut self) -> bool {
        if self.is_publishing() {
            return false;
        }
        let Some(last) = self.last_submitted.clone() else {
            return false;
        };
        self.draft = last.content;
        self.reply_target = last.reply_target;
        self.quote_target = last.quote_target;
        self.media_paths = last.media_paths;
        self.error = None;
        self.phase = ComposerPhase::Drafting;
        true
    }

    /// Record a completed `nostr_publish_note` JSON response.
    /// Returns `true` when at least one relay accepted the event.
    pub(crate) fn finish_publish_ok(&mut self, raw_json: &str) -> bool {
        match parse_publish_receipt(raw_json) {
            Ok(outcome) => {
                let any_accepted = outcome.any_accepted();
                if any_accepted {
                    self.draft.clear();
                    self.reply_target = None;
                    self.quote_target = None;
                    self.media_paths.clear();
                    self.error = None;
                    self.phase = ComposerPhase::Idle;
                    if matches!(outcome, PublishOutcome::Published(_)) {
                        self.last_submitted = None;
                    }
                } else {
                    self.error = Some(
                        "No relay accepted the note. Your draft was preserved so you can retry."
                            .to_string(),
                    );
                    self.phase = ComposerPhase::Drafting;
                }
                self.last_outcome = Some(outcome);
                any_accepted
            }
            Err(parse_err) => {
                self.error = Some(parse_err);
                self.phase = ComposerPhase::Drafting;
                false
            }
        }
    }

    /// Record a tool/approval/transport error, preserving the draft in `Drafting`.
    pub(crate) fn finish_publish_err(&mut self, error: String) {
        self.error = Some(sanitize_untrusted_text(&error));
        self.phase = ComposerPhase::Drafting;
    }

    /// Dismiss the last publish receipt banner.
    pub(crate) fn dismiss_receipt(&mut self) {
        self.last_outcome = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::nostr_wire::CLIENT_TOOL_NAMES;
    use hkask_types::wallet_wire::WALLET_TOOL_NAMES;

    #[test]
    fn publish_tool_is_pinned_to_client_server() {
        for tool in [
            PUBLISH_TOOL,
            PUBLISH_REACTION_TOOL,
            PUBLISH_REPLY_TOOL,
            PUBLISH_DELETION_TOOL,
            PUBLISH_FOLLOWS_TOOL,
            PUBLISH_REPOST_TOOL,
            PREPARE_ZAP_TOOL,
            PUBLISH_NUTZAP_TOOL,
        ] {
            assert!(
                CLIENT_TOOL_NAMES.contains(&tool),
                "{tool} must belong to hkask-mcp-nostr-client"
            );
        }
        for tool in [
            WALLET_STATUS_TOOL,
            WALLET_PAY_INVOICE_TOOL,
            WALLET_SEND_ECASH_TOOL,
            WALLET_RECEIVE_ECASH_TOOL,
        ] {
            assert!(
                WALLET_TOOL_NAMES.contains(&tool),
                "{tool} must belong to hkask-mcp-wallet"
            );
        }
    }

    #[test]
    fn reply_context_from_event_handles_nip10_marked_positional_and_nip22() {
        let mk = |id: &str, kind: u16, tags: Vec<Vec<&str>>| WireEvent {
            id: id.to_string(),
            kind,
            author: "b".repeat(64),
            created_at: 100,
            content: "hi".to_string(),
            content_truncated: false,
            tags: tags
                .into_iter()
                .map(|t| t.into_iter().map(str::to_string).collect())
                .collect(),
            tags_truncated: false,
            served_by: Vec::new(),
        };

        // Top-level kind:1 note
        let top = mk(&"1".repeat(64), 1, vec![]);
        let ctx = ReplyContext::from_event(&top, "@alice".into());
        assert_eq!(ctx.root_event_id, "1".repeat(64));
        assert_eq!(ctx.reply_to_event_id, None);
        assert_eq!(ctx.root_kind, 1);

        // NIP-10 marked root reply
        let marked = mk(
            &"2".repeat(64),
            1,
            vec![vec!["e", &"1".repeat(64), "", "root"]],
        );
        let ctx = ReplyContext::from_event(&marked, "@alice".into());
        assert_eq!(ctx.root_event_id, "1".repeat(64));
        assert_eq!(ctx.reply_to_event_id, Some("2".repeat(64)));
        assert_eq!(ctx.root_kind, 1);

        // NIP-10 legacy positional e tag (unmarked)
        let legacy = mk(&"3".repeat(64), 1, vec![vec!["e", &"1".repeat(64)]]);
        let ctx = ReplyContext::from_event(&legacy, "@alice".into());
        assert_eq!(ctx.root_event_id, "1".repeat(64));
        assert_eq!(ctx.reply_to_event_id, Some("3".repeat(64)));
        assert_eq!(ctx.root_kind, 1);

        // NIP-22 kind:1111 comment on a kind:30023 article
        let comment = mk(
            &"4".repeat(64),
            1111,
            vec![
                vec!["E", &"9".repeat(64)],
                vec!["K", "30023"],
                vec!["e", &"9".repeat(64)],
            ],
        );
        let ctx = ReplyContext::from_event(&comment, "@bob".into());
        assert_eq!(ctx.root_event_id, "9".repeat(64));
        assert_eq!(ctx.reply_to_event_id, Some("4".repeat(64)));
        assert_eq!(ctx.root_kind, 30023);
        assert_eq!(ctx.nip_label(), "NIP-22");
    }

    #[test]
    fn parse_custom_sats_and_wallet_status_validate_inputs() {
        assert_eq!(parse_custom_sats("21"), Ok(21));
        assert_eq!(parse_custom_sats("1,000"), Ok(1_000));
        assert!(parse_custom_sats("0").is_err());
        assert!(parse_custom_sats("abc").is_err());

        let ok = r#"{"network":"signet","allowed_mints":["https://testnut.cashu.space"],"per_tx_cap_sats":10000}"#;
        let summary = parse_wallet_status(ok).expect("parses cashu wallet status");
        assert_eq!(summary.network, "signet");
        assert_eq!(summary.active_rail, "cashu");
        assert_eq!(summary.per_tx_cap_sats, Some(10_000));

        let no_rails = r#"{"network":"signet","allowed_mints":[]}"#;
        assert!(parse_wallet_status(no_rails).is_err());
    }

    #[test]
    fn media_attachments_allow_media_only_notes_and_cap_at_four() {
        let mut state = ComposerState::new();
        state.toggle();
        for i in 0..MAX_MEDIA_ATTACHMENTS {
            state
                .add_media_path(format!("/tmp/img{i}.png"))
                .expect("within limit");
        }
        assert!(state.add_media_path("/tmp/img4.png".into()).is_err());
        assert!(state.has_draft());
        let payload = state
            .begin_publish(1_700_000_000)
            .expect("media-only allowed");
        assert!(payload.content.is_empty());
        assert_eq!(payload.media_paths.len(), MAX_MEDIA_ATTACHMENTS);
    }

    #[test]
    fn partial_failure_supports_one_click_retry() {
        let mut state = ComposerState::new();
        state.toggle();
        state.set_draft("retry me".into());
        state.begin_publish(1_700_000_000).expect("valid");
        let partial = r#"{"published":{"id":"e1"},"relays":[{"address":"ws://127.0.0.1:43301","role":"local","state":"accepted"},{"address":"wss://nos.lol","role":"seed","state":"refused","reason":"timeout"}],"read_back":{"found":true}}"#;
        assert!(state.finish_publish_ok(partial));
        assert!(state.can_retry_last());
        assert!(state.restore_last_for_retry());
        assert_eq!(state.draft, "retry me");
        assert!(state.is_open());
    }

    #[test]
    fn open_reply_sets_nip10_or_nip22_context_and_discard_clears_it() {
        let mut state = ComposerState::new();
        let target = ReplyContext {
            root_event_id: "a".repeat(64),
            reply_to_event_id: None,
            target_pubkey: "b".repeat(64),
            target_label: "@alice".to_string(),
            root_kind: 30023,
        };
        assert_eq!(target.nip_label(), "NIP-22");
        state.open_reply(target);
        assert!(state.is_open());
        assert!(state.reply_target.is_some());
        state.clear_reply_target();
        assert!(state.reply_target.is_none());
    }

    #[test]
    fn validate_draft_rejects_empty_and_whitespace() {
        assert_eq!(
            validate_draft_with_media("", false),
            Err(DraftValidationError::Empty)
        );
        assert_eq!(
            validate_draft_with_media("   \n\t  ", false),
            Err(DraftValidationError::Empty)
        );
    }

    #[test]
    fn validate_draft_rejects_over_max_chars_and_accepts_boundary() {
        let exact = "a".repeat(MAX_CONTENT_CHARS);
        assert_eq!(validate_draft_with_media(&exact, false), Ok(exact.as_str()));

        let too_long = "a".repeat(MAX_CONTENT_CHARS + 1);
        assert_eq!(
            validate_draft_with_media(&too_long, false),
            Err(DraftValidationError::TooLong {
                chars: MAX_CONTENT_CHARS + 1,
                max: MAX_CONTENT_CHARS,
            })
        );
    }

    #[test]
    fn parse_publish_receipt_classifies_all_partial_and_zero_accepted() {
        let all_ok = r#"{
            "published": { "id": "abc123" },
            "relays": [
                { "address": "ws://127.0.0.1:43301", "role": "local", "state": "accepted" },
                { "address": "wss://nos.lol", "role": "seed", "state": "accepted" }
            ],
            "read_back": { "found": true, "served_by": ["ws://127.0.0.1:43301"] }
        }"#;
        let outcome = parse_publish_receipt(all_ok).expect("parses all accepted");
        assert!(matches!(outcome, PublishOutcome::Published(_)));
        assert_eq!(outcome.receipt().accepted.len(), 2);
        assert!(outcome.receipt().failed.is_empty());
        assert!(outcome.receipt().read_back_found);

        let partial = r#"{
            "published": { "id": "def456" },
            "relays": [
                { "address": "ws://127.0.0.1:43301", "role": "local", "state": "accepted" },
                { "address": "wss://nos.lol", "role": "seed", "state": "refused", "reason": "rate\u0007limited" }
            ],
            "read_back": { "found": true }
        }"#;
        let outcome = parse_publish_receipt(partial).expect("parses partial failure");
        assert!(matches!(outcome, PublishOutcome::PartialFailure(_)));
        assert_eq!(outcome.receipt().accepted.len(), 1);
        assert_eq!(outcome.receipt().failed.len(), 1);
        assert_eq!(
            outcome.receipt().failed[0].reason.as_deref(),
            Some("rate\u{FFFD}limited")
        );

        let all_failed = r#"{
            "published": { "id": "ghi789" },
            "relays": [
                { "address": "wss://nos.lol", "role": "seed", "state": "refused", "reason": "blocked" }
            ],
            "read_back": { "found": false }
        }"#;
        let outcome = parse_publish_receipt(all_failed).expect("parses all failed");
        assert!(matches!(outcome, PublishOutcome::AllFailed(_)));
        assert!(!outcome.any_accepted());
    }

    #[test]
    fn composer_state_preserves_draft_on_collapse_decline_and_total_failure() {
        let mut state = ComposerState::new();
        assert!(!state.is_open());

        state.toggle();
        assert!(state.is_open());
        state.set_draft("Hello Nostr".to_string());
        assert!(state.has_draft());

        // Collapsing preserves the draft.
        state.toggle();
        assert!(!state.is_open());
        assert_eq!(state.draft, "Hello Nostr");

        // Re-opening restores drafting with the same text.
        state.toggle();
        assert!(state.is_open());
        assert_eq!(state.draft, "Hello Nostr");

        // Submit -> operator declined -> draft preserved, inline error set.
        let payload = state.begin_publish(1_700_000_000).expect("valid draft");
        assert_eq!(payload.content, "Hello Nostr");
        assert!(state.is_publishing());
        state.finish_publish_err("the operator did not approve the note".to_string());
        assert_eq!(state.phase, ComposerPhase::Drafting);
        assert_eq!(state.draft, "Hello Nostr");
        assert!(state.error.is_some());

        // Submit -> all relays failed -> draft preserved so user can retry.
        state.begin_publish(1_700_000_001).expect("valid retry");
        let any_accepted = state.finish_publish_ok(
            r#"{"published":{"id":"e1"},"relays":[{"address":"wss://nos.lol","role":"seed","state":"refused","reason":"down"}],"read_back":{"found":false}}"#,
        );
        assert!(!any_accepted);
        assert_eq!(state.phase, ComposerPhase::Drafting);
        assert_eq!(state.draft, "Hello Nostr");

        // Submit -> accepted -> draft cleared and drawer closes with receipt.
        state.begin_publish(1_700_000_002).expect("valid publish");
        let any_accepted = state.finish_publish_ok(
            r#"{"published":{"id":"e2"},"relays":[{"address":"ws://127.0.0.1:43301","role":"local","state":"accepted"}],"read_back":{"found":true}}"#,
        );
        assert!(any_accepted);
        assert_eq!(state.phase, ComposerPhase::Idle);
        assert!(state.draft.is_empty());
        assert!(matches!(
            state.last_outcome,
            Some(PublishOutcome::Published(_))
        ));
    }

    #[test]
    fn discard_clears_draft_error_and_closes_drawer() {
        let mut state = ComposerState::new();
        state.toggle();
        state.set_draft("unsent".to_string());
        state.error = Some("err".to_string());
        state.discard();
        assert!(!state.is_open());
        assert!(state.draft.is_empty());
        assert!(state.error.is_none());
    }
}
