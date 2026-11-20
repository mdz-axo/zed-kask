//! Note composition state machine, validation, and publish-receipt parsing for
//! the Nostr panel's collapsible compose drawer.
//!
//! Signing never happens here: valid drafts are dispatched to
//! `nostr_publish_note` on `hkask-mcp-nostr-client`, which asks the editor over
//! the inference IPC wire (`kask_bridge/src/nostr_signing.rs`) for operator
//! approval before touching the keychain.

use hkask_types::nostr_wire::MAX_CONTENT_CHARS;
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

/// The MCP tool that prepares a verified NIP-57 Lightning zap invoice
/// (<https://nostr-nips.com/nip-57>).
pub(crate) const PREPARE_ZAP_TOOL: &str = "nostr_prepare_zap";

/// The MCP tools on `hkask-mcp-wallet` used to inspect rails and settle a zap invoice.
pub(crate) const WALLET_STATUS_TOOL: &str = "wallet_status";
pub(crate) const WALLET_PAY_INVOICE_TOOL: &str = "wallet_pay_invoice";

/// Amethyst-style quick-zap preset amounts in satoshis.
pub(crate) const ZAP_PRESETS_SATS: &[u64] = &[21, 100, 500, 1_000];

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
    pub(crate) role: String,
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

/// Validated payload ready to send to `nostr_publish_note`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishPayload {
    pub(crate) content: String,
    pub(crate) created_at: u64,
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
}

/// Lifecycle of an in-panel NIP-57 zap flow.
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

/// Pure state machine for the collapsible Nostr compose drawer.
#[derive(Debug, Clone, Default)]
pub(crate) struct ComposerState {
    pub(crate) phase: ComposerPhase,
    pub(crate) draft: String,
    pub(crate) reply_target: Option<ReplyContext>,
    pub(crate) error: Option<String>,
    pub(crate) last_outcome: Option<PublishOutcome>,
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

/// Validate a plain `kind: 1` note draft against `MAX_CONTENT_CHARS`.
pub(crate) fn validate_draft(text: &str) -> Result<&str, DraftValidationError> {
    if text.trim().is_empty() {
        return Err(DraftValidationError::Empty);
    }
    let chars = text.chars().count();
    if chars > MAX_CONTENT_CHARS {
        return Err(DraftValidationError::TooLong {
            chars,
            max: MAX_CONTENT_CHARS,
        });
    }
    Ok(text)
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
        let role = entry
            .get("role")
            .and_then(Value::as_str)
            .map(sanitize_untrusted_text)
            .unwrap_or_else(|| "relay".to_string());
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
            role,
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
    }

    pub(crate) fn char_count(&self) -> usize {
        self.draft.chars().count()
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

    /// Open the drawer targeting a specific note/article for a NIP-10 or NIP-22 reply.
    pub(crate) fn open_reply(&mut self, target: ReplyContext) {
        if self.is_publishing() {
            return;
        }
        self.reply_target = Some(target);
        self.phase = ComposerPhase::Drafting;
        self.error = None;
    }

    /// Clear the active reply target (reverts to a top-level `kind: 1` note).
    pub(crate) fn clear_reply_target(&mut self) {
        if !self.is_publishing() {
            self.reply_target = None;
        }
    }

    /// Explicitly discard the draft, clear errors/receipts, and collapse.
    pub(crate) fn discard(&mut self) {
        if self.is_publishing() {
            return;
        }
        self.draft.clear();
        self.reply_target = None;
        self.error = None;
        self.last_outcome = None;
        self.phase = ComposerPhase::Idle;
    }

    /// Update the draft text from the editor.
    pub(crate) fn set_draft(&mut self, text: String) {
        self.draft = text;
        if validate_draft(&self.draft).is_ok() {
            self.error = None;
        }
    }

    /// Validate the current draft and enter `Publishing`.
    pub(crate) fn begin_publish(
        &mut self,
        now_unix_secs: u64,
    ) -> Result<PublishPayload, DraftValidationError> {
        let content = match validate_draft(&self.draft) {
            Ok(valid) => valid.to_string(),
            Err(err) => {
                self.error = Some(err.message());
                return Err(err);
            }
        };
        self.error = None;
        self.last_outcome = None;
        self.phase = ComposerPhase::Publishing;
        Ok(PublishPayload {
            content,
            created_at: now_unix_secs,
        })
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
                    self.error = None;
                    self.phase = ComposerPhase::Idle;
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
            PREPARE_ZAP_TOOL,
        ] {
            assert!(
                CLIENT_TOOL_NAMES.contains(&tool),
                "{tool} must belong to hkask-mcp-nostr-client"
            );
        }
        for tool in [WALLET_STATUS_TOOL, WALLET_PAY_INVOICE_TOOL] {
            assert!(
                WALLET_TOOL_NAMES.contains(&tool),
                "{tool} must belong to hkask-mcp-wallet"
            );
        }
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
        assert_eq!(validate_draft(""), Err(DraftValidationError::Empty));
        assert_eq!(
            validate_draft("   \n\t  "),
            Err(DraftValidationError::Empty)
        );
    }

    #[test]
    fn validate_draft_rejects_over_max_chars_and_accepts_boundary() {
        let exact = "a".repeat(MAX_CONTENT_CHARS);
        assert_eq!(validate_draft(&exact), Ok(exact.as_str()));

        let too_long = "a".repeat(MAX_CONTENT_CHARS + 1);
        assert_eq!(
            validate_draft(&too_long),
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
