//! The Nostr panel's upper region: what the Nostr tools returned, as events.
//!
//! The concept model, pinned. Every view shows one of these and nothing
//! else:
//!
//! - **Event** — a verified NIP-01 object: id, author, claimed time, kind,
//!   tags, content, and where it came from.
//! - **Timeline** — a named query and the events it returned, newest
//!   first. It is never a ranking: the For reading/Everything switch
//!   changes which events are shown and how records collapse, never the
//!   order. The word is the operator's ruling of 2026-11-14 ("please
//!   proceed", keeping "Timeline"; core rung), recorded in
//!   `hkask-bridge-ontology`'s derived registry. The first timeline is the
//!   local relay's archive.
//! - **Conversation** — a root note and its replies in reading order, with
//!   the notes it refers to that were not found. A reply names the note it
//!   answers; the name opens that note's thread, and Back returns to the
//!   thread left behind.
//! - **Relay** — the local relay's state and its admission decisions.
//!
//! Two derived views over Event, decided by NIP-01 kind range:
//!
//! - **Note** — a regular text-bearing event, and an article (kind 30023):
//!   rendered as its text with its reference. A follows update (kind 3)
//!   renders as its own row.
//! - **Record** — a client's own state: a profile (kind 0), a replaceable
//!   (10000–19999) or an addressable (30000–39999) event. Rendered as a
//!   one-line summary from its tags, never its payload; in For reading,
//!   one author's records of one minute collapse under one exact-count
//!   row.
//!
//! The viewer renders what the Nostr tools return and hosts the panel's
//! interactive surfaces: the collapsible `kind: 1` / NIP-10 / NIP-22 note and
//! reply composer (with optional Blossom `media_paths`), inline NIP-25
//! reactions & NIP-09 deletion/unreact, NIP-18 reposts, NIP-02 follow/unfollow,
//! NIP-57 Lightning zaps & NIP-61 Cashu nutzaps, a Notifications tab, and the
//! editor-only NIP-17 DM tab. All signing stays in the editor process behind
//! operator approval.
//!
//! Three rules:
//!
//! - **Everything shown traces to a tool result.** The viewer holds no
//!   Nostr state of its own. It reads the archive through the governed
//!   tool invoker and it ingests the conversation's tool results by their
//!   shape, so it shows what the tools produced, not what the model said.
//! - **Third-party text is shown as text.** Content, tags and relay
//!   messages are drawn as plain characters, with control characters made
//!   visible. Links (`http(s)://`, Markdown `[label](url)`, and `<iframe>`
//!   embeds) are extracted into clickable link rows that name their host
//!   and open on click.
//! - **Followed authors auto-fetch; strangers stay click-to-load.** The
//!   Following feed fetches notes and articles (`kind: 1`, `kind: 30023`)
//!   from the seed relays for your kind-3 contact list, and automatically
//!   fetches their images and loads their videos into `MediaWidget`. Media
//!   from non-followed authors stays at its placeholder until clicked.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use crate::blurhash;
use crate::nostr_composer::{
    CHAR_WARNING_THRESHOLD, ComposerState, PREPARE_ZAP_TOOL, PUBLISH_DELETION_TOOL,
    PUBLISH_FOLLOWS_TOOL, PUBLISH_NUTZAP_TOOL, PUBLISH_REACTION_TOOL, PUBLISH_REPLY_TOOL,
    PUBLISH_REPOST_TOOL, PUBLISH_TOOL, PublishOutcome, QUICK_REACTIONS, ReplyContext,
    WALLET_PAY_INVOICE_TOOL, WALLET_SEND_ECASH_TOOL, WALLET_STATUS_TOOL, WalletBadge,
    ZAP_PRESETS_SATS, ZapDraftState, ZapRail, ZapStatus, parse_custom_sats, parse_wallet_status,
    sanitize_untrusted_text,
};
use acp_thread::AgentThreadEntry;
use agent_client_protocol::v2::schema::ToolCallId;
use gpui::{
    Context, DefiniteLength, Entity, Focusable as _, Render, ScrollHandle, SharedString, Task,
    WeakEntity, Window, px,
};
use gpui::{ImageSource, ObjectFit, RenderImage, img};
use hkask_media_widget::{MediaKind, MediaRef, MediaWidget, approve_gallery_media_path};
use hkask_tool_invoker::InvokeError;
use hkask_types::media_wire::{MEDIA_FETCH_IMAGE_TOOL, MEDIA_SERVER, MEDIA_VIDEO_FETCH_TOOL};
use hkask_types::nostr_wire::{
    MAX_CONTENT_CHARS, MAX_MEDIA_ATTACHMENTS, NOSTR_CLIENT_SERVER, NOSTR_RELAY_SERVER, WireEvent,
};
use hkask_types::wallet_wire::WALLET_SERVER_ID;
use image::Frame;
use kask_bridge::KaskSettings;
use kask_bridge::nostr_dm::{self, DmMessage, DmThreadSummary};
use markdown::{Markdown, MarkdownElement, MarkdownFont, MarkdownStyle};
use serde::Deserialize;
use serde_json::{Value, json};
use settings::Settings as _;
use time::{OffsetDateTime, UtcOffset};
use time_format::TimestampFormat;
use ui::{Label, LabelSize, WithScrollbar as _, prelude::*};
use util::ResultExt as _;

/// The tools the viewer dispatches itself. Pinned against the servers'
/// lists by `dispatched_tools_are_the_servers_own`.
const ARCHIVE_TOOL: &str = "nostr_relay_query";
const STATUS_TOOL: &str = "nostr_relay_status";
const LOG_TOOL: &str = "nostr_relay_log";
const SYNC_TOOL: &str = "nostr_relay_sync";
const THREAD_TOOL: &str = "nostr_thread";
const FETCH_TOOL: &str = "nostr_fetch_events";
const NOTIFICATIONS_TOOL: &str = "nostr_notifications";
/// A labelled click in the detail pane: the network is contacted only
/// there.
const PROFILE_TOOL: &str = "nostr_profile";
// The one fetch a labelled click offers: MEDIA_FETCH_IMAGE_TOOL on
// MEDIA_SERVER, both imported above. The tool name is pinned against that
// server's own generated list by `wire_tool_name_is_the_servers_own` in
// hkask-mcp-media — the same pin chain the relay and client lists carry,
// one link shorter.

/// Events asked for when the archive is loaded: the tool's own maximum.
const ARCHIVE_PAGE: usize = 100;
/// The height a note's media preview decodes and shows at.
const MEDIA_PREVIEW_HEIGHT: u32 = 48;
/// The height a fetched image decodes and shows at, bounded so one image
/// cannot take the pane over (the narrow-pane ruling's spirit).
const FETCHED_IMAGE_HEIGHT: u32 = 240;
/// Admission decisions asked for.
const LOG_PAGE: usize = 50;
/// The selected event's share of the view's height.
const DETAIL_FRACTION: f32 = 0.45;
/// Tags listed for the selected event before the rest are counted.
const DETAIL_TAGS: usize = 16;
/// The deepest reply indent drawn; deeper replies sit at this indent.
const MAX_INDENT_LEVELS: usize = 6;
/// How many threads Back remembers behind the one on screen.
const CONVERSATION_BACK_LIMIT: usize = 8;

/// Which events the Timeline shows. Every named feed reads the same page;
/// For reading is the default (operator ruling 2026-11-14). The switch
/// changes what is shown and how records collapse, never the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TimelineFilter {
    /// Reading kinds as rows, and one author's records of one minute
    /// collapsed under one exact-count row.
    #[default]
    ForReading,
    /// The notes of the authors your own follows list names: your kind 3
    /// read from the local archive, among the allowlisted authors it
    /// holds.
    Following,
    /// The kind 1 notes that start a thread: no note in their tags is
    /// their parent.
    Threads,
    /// The notes that answer a note: kind 1 replies and kind 1111
    /// comments.
    Replies,
    /// Every event as a row.
    Everything,
}

impl TimelineFilter {
    /// Whether this feed shows the row. Threads and Replies read the
    /// same answer-a-note resolver as the reply chip, so a note's chip
    /// and its feed agree; Following reads the reader's own follows.
    /// The switch changes what is shown, never the order (operator
    /// ruling 2026-11-14).
    fn shows(self, row: &Row, follows: Option<&Follows>) -> bool {
        match self {
            TimelineFilter::ForReading | TimelineFilter::Everything => true,
            TimelineFilter::Following => {
                let Some(Follows::Authors(authors)) = follows else {
                    return false;
                };
                authors.contains(row.event.author.as_str())
                    && kind_class(row.event.kind) == KindClass::Note
            }
            TimelineFilter::Threads => row.event.kind == 1 && answered_note(&row.event).0.is_none(),
            TimelineFilter::Replies => answered_note(&row.event).0.is_some(),
        }
    }
}

/// What the reader's own follows are, once the local reads answer: the
/// authors the configured identity's newest archived kind 3 names, or
/// the named reason they are not known. Public data only.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Follows {
    /// The identity's newest archived kind 3 names these authors.
    Authors(BTreeSet<String>),
    /// No identity is set in the Nostr settings.
    NoIdentity,
    /// The identity in the Nostr settings is not a shape the archive
    /// read accepts; the value is never repeated.
    InvalidIdentity,
    /// The identity is set, but no kind 3 of theirs is in the archive.
    NoList,
}

/// An event's derived view over Event, decided by NIP-01 kind range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KindClass {
    /// A regular text-bearing event, rendered as its text with its
    /// reference; an article (kind 30023) is one too.
    Note,
    /// Kind 3: a follows update, rendered as its own row.
    Follows,
    /// A client's own state: a profile (kind 0), a replaceable
    /// (10000–19999) or an addressable (30000–39999) event, rendered as a
    /// one-line summary from its tags, never its payload.
    Record,
    /// 20000–29999: not stored by a relay; named, never un-renderable.
    Ephemeral,
}

/// An event's derived view by NIP-01 kind range.
fn kind_class(kind: u16) -> KindClass {
    match kind {
        0 => KindClass::Record,
        3 => KindClass::Follows,
        30023 => KindClass::Note,
        1..=9999 => KindClass::Note,
        10000..=19999 => KindClass::Record,
        20000..=29999 => KindClass::Ephemeral,
        _ => KindClass::Record,
    }
}

/// A kind's NIP-01 range name.
fn kind_range_name(kind: u16) -> &'static str {
    match kind {
        0..=9999 => "regular",
        10000..=19999 => "replaceable",
        20000..=29999 => "ephemeral",
        _ => "addressable",
    }
}

/// The kinds the Timeline names by sentence, each with the spec that
/// defines it. A kind not in the table is shown by its NIP-01 range with a
/// count from its tags — never an invented label, because an app's
/// private kinds have no spec to cite.
const KIND_LABELS: &[(u16, &str, &str)] = &[
    (0, "set profile", "NIP-01"),
    (1, "posted a note", "NIP-01"),
    (3, "updated follows", "NIP-02"),
    (5, "requested deletions", "NIP-09"),
    (6, "reposted", "NIP-18"),
    (7, "reacted", "NIP-25"),
    (16, "reposted in full", "NIP-18"),
    (1111, "commented", "NIP-22"),
    (9735, "zapped", "NIP-57"),
    (30023, "published an article", "NIP-23"),
    (10002, "set relay list", "NIP-65"),
];

/// A kind's sentence label and the spec that defines it.
fn kind_label(kind: u16) -> Option<(&'static str, &'static str)> {
    KIND_LABELS
        .iter()
        .find(|(known, _, _)| *known == kind)
        .map(|(_, label, spec)| (*label, *spec))
}

/// The tag names that carry relay addresses.
const RELAY_TAG_NAMES: [&str; 2] = ["r", "relay"];
/// The most relay hosts a supporting line lists.
const SUPPORTING_HOSTS: usize = 4;

/// A relay or image address's host, as one line can show it. Credentials
/// never label a host: the last `@` ends them, so a URL that carries a user
/// name still shows the host it contacts.
fn host_of(address: &str) -> &str {
    let no_scheme = address
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(address);
    let authority = no_scheme.split(['/', '?', '#']).next().unwrap_or(no_scheme);
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    host.split(':').next().unwrap_or(host)
}

/// How many relay addresses an event's tags carry.
fn relay_count(event: &WireEvent) -> usize {
    event
        .tags
        .iter()
        .filter(|tag| {
            tag.first()
                .map(|name| RELAY_TAG_NAMES.contains(&name.as_str()))
                .unwrap_or(false)
        })
        .count()
}

/// How many people an event's `p` tags name.
fn people_count(event: &WireEvent) -> usize {
    event
        .tags
        .iter()
        .filter(|tag| tag.first().map(String::as_str) == Some("p"))
        .count()
}

/// The first non-empty value of a tag named by any of `names`, in that
/// order — the event's own tags, never a lookup.
fn tag_value<'a>(event: &'a WireEvent, names: [&str; 2]) -> Option<&'a str> {
    for name in names {
        for tag in event.tags.iter() {
            if tag.first().map(String::as_str) == Some(name)
                && let Some(value) = tag.get(1).filter(|value| !value.is_empty())
            {
                return Some(value.as_str());
            }
        }
    }
    None
}

/// The value of the `e` tag carrying `marker` (NIP-10's marked scheme
/// puts the marker fourth: `["e", id, relay, marker]`).
fn marked_e_tag<'a>(event: &'a WireEvent, marker: &str) -> Option<&'a str> {
    event.tags.iter().find_map(|tag| {
        if tag.first().map(String::as_str) == Some("e")
            && tag.get(3).map(String::as_str) == Some(marker)
            && let Some(value) = tag.get(1).filter(|value| !value.is_empty())
        {
            Some(value.as_str())
        } else {
            None
        }
    })
}

/// The note a kind 1 reply answers, from its NIP-10 tags alone. The
/// `reply`-marked `e` names the parent; a note with only a `root` marker
/// is a direct reply to the root; a note whose `e` tags carry any other
/// marker (a mention) declares no parent; and in the deprecated
/// positional scheme the parent is the last `e`.
fn parent_of_text_reply(event: &WireEvent) -> Option<&str> {
    if let Some(parent) = marked_e_tag(event, "reply") {
        return Some(parent);
    }
    if let Some(root) = marked_e_tag(event, "root") {
        return Some(root);
    }
    let marked_scheme = event.tags.iter().any(|tag| {
        tag.first().map(String::as_str) == Some("e") && tag.get(3).is_some_and(|m| !m.is_empty())
    });
    if marked_scheme {
        return None;
    }
    event.tags.iter().rev().find_map(|tag| {
        if tag.first().map(String::as_str) == Some("e")
            && let Some(value) = tag.get(1).filter(|value| !value.is_empty())
        {
            Some(value.as_str())
        } else {
            None
        }
    })
}

/// The note a reply answers and that note's author, from its tags alone:
/// a kind 1 note follows NIP-10's markers, a kind 1111 comment answers
/// its parent comment (`e`) or the event it comments on (`E`), and the
/// author is the first `p`, else `P`. Nothing is fetched.
fn answered_note(event: &WireEvent) -> (Option<&str>, Option<&str>) {
    let note = match event.kind {
        1 => parent_of_text_reply(event),
        1111 => tag_value(event, ["e", "E"]),
        _ => None,
    };
    (note, tag_value(event, ["p", "P"]))
}

/// The authors a follows list names: every `p` tag's value.
fn follows_of(event: &WireEvent) -> BTreeSet<String> {
    let mut authors = BTreeSet::new();
    for tag in &event.tags {
        if tag.first().map(String::as_str) == Some("p")
            && let Some(value) = tag.get(1).filter(|value| !value.is_empty())
        {
            authors.insert(value.clone());
        }
    }
    authors
}

/// Whether the identity setting is a shape the archive read accepts
/// without echoing: 64-character hex, or `npub1` bech32. Anything else
/// (a secret key pasted into the wrong field, for one) is refused here
/// and never sent, so no error can repeat it.
fn identity_is_sendable(identity: &str) -> bool {
    let trimmed = identity.trim();
    (trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()))
        || trimmed.starts_with("npub1")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaAttachmentKind {
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaAttachment {
    url: String,
    kind: MediaAttachmentKind,
    blurhash: Option<String>,
    /// The media's own claimed dimensions, width by height.
    dimensions: Option<(u32, u32)>,
}

fn classify_media_url(url: &str, mime: Option<&str>) -> Option<MediaAttachmentKind> {
    if let Some(mime) = mime {
        if mime.starts_with("video/") {
            return Some(MediaAttachmentKind::Video);
        }
        if mime.starts_with("image/") {
            return Some(MediaAttachmentKind::Image);
        }
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mp4")
        || lower.ends_with(".webm")
        || lower.ends_with(".mov")
        || lower.ends_with(".m4v")
        || lower.ends_with(".mkv")
    {
        Some(MediaAttachmentKind::Video)
    } else if lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".svg")
    {
        Some(MediaAttachmentKind::Image)
    } else {
        None
    }
}

/// Resolve the first media attachment from an event's `imeta` tags (NIP-92,
/// with or without `blurhash`) or from a direct `https://` media URL in its
/// text.
fn media_of(event: &WireEvent) -> Option<MediaAttachment> {
    for tag in &event.tags {
        if tag.first().map(String::as_str) != Some("imeta") {
            continue;
        }
        let mut url = None;
        let mut mime = None;
        let mut blurhash = None;
        let mut dimensions = None;
        for entry in tag.iter().skip(1) {
            let mut pair = entry.splitn(2, ' ');
            let key = pair.next().unwrap_or_default();
            let value = pair.next().unwrap_or_default().trim();
            if value.is_empty() {
                continue;
            }
            match key {
                "url" => url = Some(value.to_string()),
                "m" => mime = Some(value.to_string()),
                "blurhash" => blurhash = Some(value.to_string()),
                "dim" => {
                    let mut parts = value.splitn(2, 'x');
                    dimensions = match (parts.next(), parts.next()) {
                        (Some(width), Some(height)) => {
                            match (width.parse::<u32>(), height.parse::<u32>()) {
                                (Ok(width), Ok(height)) if width > 0 && height > 0 => {
                                    Some((width, height))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                }
                _ => {}
            }
        }
        if let Some(url) = url
            && url.starts_with("https://")
        {
            let kind =
                classify_media_url(&url, mime.as_deref()).unwrap_or(MediaAttachmentKind::Image);
            return Some(MediaAttachment {
                url,
                kind,
                blurhash,
                dimensions,
            });
        }
    }

    for link in extract_links(&event.content) {
        if let Some(kind) = classify_media_url(&link.url, None) {
            return Some(MediaAttachment {
                url: link.url,
                kind,
                blurhash: None,
                dimensions: None,
            });
        }
    }
    None
}

/// Resolve each note's media attachment and decode any `blurhash` preview in
/// one pass over `rows`, keyed by note id.
fn media_index_of(
    rows: &[Row],
) -> (
    BTreeMap<String, MediaAttachment>,
    BTreeMap<String, Arc<RenderImage>>,
) {
    let mut attachments = BTreeMap::new();
    let mut previews = BTreeMap::new();
    for row in rows {
        let Some(meta) = media_of(&row.event) else {
            continue;
        };
        if let Some(hash) = meta.blurhash.as_deref() {
            let width = match meta.dimensions {
                Some((width, height)) => {
                    (MEDIA_PREVIEW_HEIGHT as f32 * width as f32 / height as f32).round() as u32
                }
                None => MEDIA_PREVIEW_HEIGHT,
            };
            if let Some(pixels) = blurhash::decode(
                hash,
                width.clamp(1, 4 * MEDIA_PREVIEW_HEIGHT),
                MEDIA_PREVIEW_HEIGHT,
            ) {
                previews.insert(
                    row.event.id.clone(),
                    Arc::new(RenderImage::new(vec![Frame::new(pixels)])),
                );
            }
        }
        attachments.insert(row.event.id.clone(), meta);
    }
    (attachments, previews)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtractedLink {
    label: String,
    url: String,
    host: String,
}

fn normalize_http_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches(|c: char| {
        matches!(c, '.' | ',' | ';' | ':' | ')' | ']' | '>' | '"' | '\'')
    });
    let normalized = if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        trimmed.to_string()
    };
    if normalized.starts_with("https://") || normalized.starts_with("http://") {
        Some(normalized)
    } else {
        None
    }
}

/// Extract clickable `http(s)://` links from Markdown `[label](url)`, HTML
/// `<iframe src="...">` embeds, and bare URLs in event content.
fn extract_links(content: &str) -> Vec<ExtractedLink> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    let mut push_link = |label: String, raw_url: &str| {
        let Some(url) = normalize_http_url(raw_url) else {
            return;
        };
        if !seen.insert(url.clone()) {
            return;
        }
        let host = host_of(&url).to_string();
        let label = if label.trim().is_empty() {
            url.clone()
        } else {
            label.trim().to_string()
        };
        links.push(ExtractedLink { label, url, host });
    };

    // 1. Markdown links: [label](url)
    let mut rest = content;
    while let Some(open_bracket) = rest.find('[') {
        let after_bracket = &rest[open_bracket + 1..];
        let Some(close_bracket) = after_bracket.find("](") else {
            rest = after_bracket;
            continue;
        };
        let label = &after_bracket[..close_bracket];
        let after_paren = &after_bracket[close_bracket + 2..];
        let Some(close_paren) = after_paren.find(')') else {
            rest = after_paren;
            continue;
        };
        let url_part = after_paren[..close_paren]
            .split_whitespace()
            .next()
            .unwrap_or_default();
        if !label.contains('\n') && !url_part.is_empty() {
            push_link(label.to_string(), url_part);
        }
        rest = &after_paren[close_paren + 1..];
    }

    // 2. HTML iframe src="..." (e.g. YouTube / SlideShare embeds in NIP-23 articles)
    let mut iframe_rest = content;
    while let Some(idx) = iframe_rest.find("<iframe") {
        let after = &iframe_rest[idx..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        if let Some(src_idx) = tag.find("src=") {
            let after_src = tag[src_idx + 4..].trim_start();
            let quote = after_src.chars().next();
            if matches!(quote, Some('"') | Some('\'')) {
                let q = quote.unwrap();
                let inner = &after_src[1..];
                if let Some(end_q) = inner.find(q) {
                    let url_str = &inner[..end_q];
                    let label = if url_str.contains("youtube.com") || url_str.contains("youtu.be") {
                        "YouTube video".to_string()
                    } else if url_str.contains("slideshare.net") {
                        "SlideShare presentation".to_string()
                    } else {
                        "Embedded link".to_string()
                    };
                    push_link(label, url_str);
                }
            }
        }
        iframe_rest = &after[end..];
    }

    // 3. Bare http(s):// URLs
    for token in content.split_whitespace() {
        let cleaned = token.trim_start_matches(|c: char| matches!(c, '(' | '[' | '<' | '"' | '\''));
        if let Some(url) = normalize_http_url(cleaned) {
            push_link(url.clone(), &url);
        }
    }

    links
}

fn is_platform_video_url(url: &str) -> bool {
    let host = host_of(url).to_ascii_lowercase();
    host == "youtu.be"
        || host.ends_with(".youtube.com")
        || host == "youtube.com"
        || host.ends_with(".vimeo.com")
        || host == "vimeo.com"
}

/// Decode fetched image bytes to a bounded image: scaled to fit
/// [`FETCHED_IMAGE_HEIGHT`], aspect kept, never upscaled, so one image
/// cannot take the pane over.
fn decode_fetched_image(bytes: &[u8]) -> Result<Arc<RenderImage>, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("the fetched image did not decode: {error}"))?;
    // Bounded and never upscaled: a taller image scales to fit
    // FETCHED_IMAGE_HEIGHT with its aspect kept, a shorter one stays its
    // own size — so one image cannot take the pane over.
    let scaled = if image.height() > FETCHED_IMAGE_HEIGHT {
        let scale = FETCHED_IMAGE_HEIGHT as f32 / image.height() as f32;
        let width = ((image.width() as f32 * scale).round() as u32).max(1);
        image.thumbnail(width, FETCHED_IMAGE_HEIGHT)
    } else {
        image
    };
    Ok(Arc::new(RenderImage::new(vec![Frame::new(
        scaled.into_rgba8(),
    )])))
}

/// What a row says an event is, with a count from its tags.
fn what_it_is(event: &WireEvent) -> String {
    match event.kind {
        1111 => match answered_note(event).1 {
            Some(author) => format!("commented on a note by {}", short_hex(author)),
            None => "commented on a note".to_string(),
        },
        3 => {
            let people = people_count(event);
            if people == 1 {
                "updated follows (1 person)".to_string()
            } else {
                format!("updated follows ({people} people)")
            }
        }
        kind => {
            let base = kind_label(kind)
                .map(|(label, _)| label.to_string())
                .unwrap_or_else(|| format!("kind {kind} ({})", kind_range_name(kind)));
            if kind_class(kind) == KindClass::Record {
                let relays = relay_count(event);
                if relays > 0 {
                    return format!(
                        "{base}: {relays} {}",
                        if relays == 1 { "relay" } else { "relays" }
                    );
                }
            }
            base
        }
    }
}

/// A row's supporting line: a note's text, an article's title, a profile's
/// about, or a record's one-line summary from its tags — never a record's
/// payload.
fn supporting_line(event: &WireEvent) -> String {
    match kind_class(event.kind) {
        KindClass::Note => {
            if event.kind == 30023
                && let Some(title) = event
                    .tags
                    .iter()
                    .find(|tag| tag.first().map(String::as_str) == Some("title"))
                    .and_then(|tag| tag.get(1))
                && !title.is_empty()
            {
                return title.clone();
            }
            event.content.lines().next().unwrap_or_default().to_string()
        }
        KindClass::Follows => String::new(),
        KindClass::Record => {
            if event.kind == 0 {
                // The profile's own `about`; the name is already the row's.
                return serde_json::from_str::<Value>(&event.content)
                    .ok()
                    .and_then(|metadata| {
                        metadata
                            .get("about")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
            }
            let mut hosts: Vec<&str> = Vec::new();
            for tag in event.tags.iter() {
                if tag
                    .first()
                    .map(|name| RELAY_TAG_NAMES.contains(&name.as_str()))
                    .unwrap_or(false)
                    && let Some(address) = tag.get(1)
                {
                    hosts.push(host_of(address));
                    if hosts.len() > SUPPORTING_HOSTS {
                        break;
                    }
                }
            }
            if !hosts.is_empty() {
                let mut shown: Vec<String> = hosts
                    .iter()
                    .take(SUPPORTING_HOSTS)
                    .map(|host| host.to_string())
                    .collect();
                if hosts.len() > SUPPORTING_HOSTS {
                    shown.push("…".to_string());
                }
                return shown.join(", ");
            }
            let people = people_count(event);
            if people > 0 {
                return format!("{people} {}", if people == 1 { "person" } else { "people" });
            }
            String::new()
        }
        KindClass::Ephemeral => String::new(),
    }
}

/// A reply's context, from its tags alone: the note it answers and that
/// note's author. Nothing is fetched — the chip that carries it opens the
/// note's thread through the labelled `nostr_thread` click.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplyReference {
    /// The event id the reply's tags name.
    note: String,
    /// The chip's label: "@name" when the page knows the author, else the
    /// shortened key.
    label: String,
}

/// A note's or comment's reply context, from its tags alone. A note with
/// no `e` tag starts a thread and has none.
fn reply_reference(
    event: &WireEvent,
    names: &BTreeMap<String, AuthorProfile>,
) -> Option<ReplyReference> {
    if event.kind != 1 && event.kind != 1111 {
        return None;
    }
    let (note, author) = answered_note(event);
    let note = note?.to_string();
    let label = match author {
        Some(author) => {
            let shown = names
                .get(author)
                .and_then(|profile| profile.name.as_deref())
                .map(str::to_string)
                .unwrap_or_else(|| short_hex(author));
            format!("↳ in reply to @{shown}")
        }
        None => "↳ in reply to a note".to_string(),
    };
    Some(ReplyReference { note, label })
}

/// What the page knows about one author: the `name` and `nip05` of the
/// newest kind 0 in the rows. Both are the author's own claims, shown as
/// text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuthorProfile {
    name: Option<String>,
    nip05: Option<String>,
}

/// Profiles from the page's kind 0 events: each author's newest
/// profile's `name` and `nip05`. No call: the page is already on screen.
fn names_from_rows(rows: &[Row]) -> BTreeMap<String, AuthorProfile> {
    let mut newest: BTreeMap<String, (u64, AuthorProfile)> = BTreeMap::new();
    for row in rows {
        if row.event.kind != 0 {
            continue;
        }
        let Ok(metadata) = serde_json::from_str::<Value>(&row.event.content) else {
            continue;
        };
        let field = |key: &str| {
            metadata
                .get(key)
                .and_then(Value::as_str)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let profile = AuthorProfile {
            name: field("name"),
            nip05: field("nip05"),
        };
        if profile.name.is_none() && profile.nip05.is_none() {
            continue;
        }
        match newest.get(&row.event.author) {
            Some((at, _)) if *at >= row.event.created_at => {}
            _ => {
                newest.insert(row.event.author.clone(), (row.event.created_at, profile));
            }
        }
    }
    newest
        .into_iter()
        .map(|(author, (_, profile))| (author, profile))
        .collect()
}

/// This machine's local offset, or UTC when the system cannot say.
fn local_offset() -> UtcOffset {
    UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
}

/// A claimed Unix time in a given offset.
fn to_local(seconds: u64, offset: UtcOffset) -> Option<OffsetDateTime> {
    i64::try_from(seconds)
        .ok()
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
        .map(|timestamp| timestamp.to_offset(offset))
}

/// The local day of a claimed time, as a sortable bucket.
fn day_bucket(seconds: u64, offset: UtcOffset) -> i64 {
    (seconds as i64 + offset.whole_seconds() as i64) / 86_400
}

/// The local minute of a claimed time: the collapse key.
fn minute_bucket(seconds: u64, offset: UtcOffset) -> i64 {
    (seconds as i64 + offset.whole_seconds() as i64) / 60
}

/// A row's time of day, local.
fn local_time_text(seconds: u64, offset: UtcOffset) -> String {
    to_local(seconds, offset)
        .map(time_format::format_time)
        .unwrap_or_else(|| format!("{seconds} (not a date)"))
}

/// A row's claimed time, relative: now, then minutes, hours and days,
/// then the local date. A claimed future time keeps the local clock's
/// text — the claim is the author's, not the panel's.
fn relative_time_text(seconds: u64, now: OffsetDateTime, offset: UtcOffset) -> String {
    let Some(at) = to_local(seconds, offset) else {
        return format!("{seconds} (not a date)");
    };
    let elapsed = (now - at).whole_seconds();
    if elapsed < 0 {
        return local_time_text(seconds, offset);
    }
    match elapsed {
        0..=59 => "now".to_string(),
        60..=3_599 => format!("{}m", elapsed / 60),
        3_600..=86_399 => format!("{}h", elapsed / 3_600),
        86_400..=604_799 => format!("{}d", elapsed / 86_400),
        _ => day_text(seconds, now, offset),
    }
}

/// A day divider's date, local: Today, Yesterday or the date itself.
fn day_text(seconds: u64, now: OffsetDateTime, offset: UtcOffset) -> String {
    to_local(seconds, offset)
        .map(|timestamp| time_format::format_date_medium(timestamp, now, true))
        .unwrap_or_else(|| format!("{seconds} (not a date)"))
}

/// A `nostr_profile` result's asked author's name, if the result carries
/// one. `Some(None)`: the profile was asked and no readable name came
/// back. `None`: the result is not a profile result, so nothing was
/// asked.
fn profile_name_of(content: &Value) -> Option<Option<String>> {
    let first = content.get("profiles")?.as_array()?.first()?;
    Some(
        first
            .pointer("/fields/name")
            .and_then(Value::as_str)
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty()),
    )
}

/// One line of the assembled Timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TimelineLine<'rows> {
    /// A divider at a local-day boundary; `at` is the day's newest event,
    /// whose local date the divider shows.
    Divider { at: u64 },
    /// One event's row.
    Entry { row: &'rows Row },
    /// One author's records of one local minute, collapsed under one
    /// exact-count row. When the group is expanded, its rows follow.
    Collapsed {
        author: String,
        name: Option<String>,
        minute: i64,
        count: usize,
        newest: u64,
    },
}

/// Emit one collected run of records: a lone record stays a row; two or
/// more of one author and minute, in For reading, collapse under one
/// exact-count row, which their rows follow when the group is expanded.
fn flush_records<'rows>(
    lines: &mut Vec<TimelineLine<'rows>>,
    records: &mut Vec<(&'rows Row, (String, i64))>,
    names: &BTreeMap<String, AuthorProfile>,
    filter: TimelineFilter,
    expanded: &HashSet<(String, i64)>,
) {
    if records.is_empty() {
        return;
    }
    let (first, (author, minute)) = &records[0];
    let count = records.len();
    let newest = first.event.created_at;
    let group = count > 1 && filter == TimelineFilter::ForReading;
    if group {
        let name = names.get(author).and_then(|profile| profile.name.clone());
        lines.push(TimelineLine::Collapsed {
            author: author.clone(),
            name,
            minute: *minute,
            count,
            newest,
        });
    }
    if !group || expanded.contains(&(author.clone(), *minute)) {
        for (row, _) in records.drain(..) {
            lines.push(TimelineLine::Entry { row });
        }
    } else {
        records.clear();
    }
}

/// Assemble the Timeline's lines over the page, newest first as the tools
/// returned it: a divider at each local day, and — For reading — one
/// author's records of one local minute collapsed under one exact-count
/// row. Threads, Replies and Following keep only their own notes, so a
/// day whose every row is filtered out draws no divider. The page's order
/// is never changed: the Timeline is not a ranking.
fn timeline_lines<'rows>(
    rows: &'rows [Row],
    names: &BTreeMap<String, AuthorProfile>,
    filter: TimelineFilter,
    follows: Option<&Follows>,
    expanded: &HashSet<(String, i64)>,
    offset: UtcOffset,
) -> Vec<TimelineLine<'rows>> {
    let mut lines: Vec<TimelineLine> = Vec::new();
    let mut day: Option<i64> = None;
    let mut records: Vec<(&Row, (String, i64))> = Vec::new();
    for row in rows {
        if !filter.shows(row, follows) {
            continue;
        }
        let row_day = day_bucket(row.event.created_at, offset);
        if day != Some(row_day) {
            flush_records(&mut lines, &mut records, names, filter, expanded);
            day = Some(row_day);
            lines.push(TimelineLine::Divider {
                at: row.event.created_at,
            });
        }
        if kind_class(row.event.kind) == KindClass::Record && filter == TimelineFilter::ForReading {
            let key = (
                row.event.author.clone(),
                minute_bucket(row.event.created_at, offset),
            );
            let breaks_run = records
                .last()
                .map(|(_, (author, minute))| author != &key.0 || *minute != key.1)
                .unwrap_or(false);
            if breaks_run {
                flush_records(&mut lines, &mut records, names, filter, expanded);
            }
            records.push((row, key));
        } else {
            flush_records(&mut lines, &mut records, names, filter, expanded);
            lines.push(TimelineLine::Entry { row });
        }
    }
    flush_records(&mut lines, &mut records, names, filter, expanded);
    lines
}

/// What the Timeline says when nothing is on screen: an empty page is
/// the query's truth, a page the filter empties is the filter's — and
/// the Following feed names which of its own truths it is.
fn empty_timeline(
    filter: TimelineFilter,
    follows: Option<&Follows>,
    page_is_empty: bool,
) -> (&'static str, &'static str) {
    if page_is_empty {
        return (
            "No events",
            "The query returned nothing. An empty archive means the relay has stored no event by an allowlisted author.",
        );
    }
    match filter {
        TimelineFilter::Following => match follows {
            Some(Follows::Authors(_)) => (
                "No notes from the authors you follow",
                "This feed shows the notes of the authors your own follows list names, among the allowlisted authors the archive holds.",
            ),
            Some(Follows::NoIdentity) => (
                "No identity set",
                "Following reads your own follows list. Set your npub under Settings > Kask > Nostr.",
            ),
            Some(Follows::InvalidIdentity) => (
                "The npub is not usable",
                "What is written under Settings > Kask > Nostr is not an npub or a public key; check the setting. The value is not repeated here.",
            ),
            Some(Follows::NoList) => (
                "No follows list of yours in the archive",
                "Following reads your own kind 3 from the local archive. Sync the archive from the Relay tab, or publish your follows list, so it is stored.",
            ),
            None => ("Nothing shown", "This feed shows nothing from this page."),
        },
        TimelineFilter::Threads => (
            "No threads",
            "This page holds no note that starts a thread.",
        ),
        TimelineFilter::Replies => (
            "No replies",
            "This page holds no note that answers another note.",
        ),
        _ => ("Nothing shown", "This feed shows nothing from this page."),
    }
}

/// Count reactions by the note each answers: a reaction is a kind 7
/// event whose `e` tag names the note it reacts to. The second value is
/// whether the read hit its limit, which makes every count a floor.
fn reaction_counts(rows: &[Row]) -> (BTreeMap<String, BTreeMap<String, u64>>, bool) {
    let mut counts: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for row in rows {
        if row.event.kind == 7
            && let Some(target) = tag_value(&row.event, ["e", "E"])
        {
            *counts
                .entry(target.to_string())
                .or_default()
                .entry(reaction_emoji(&row.event.content))
                .or_insert(0) += 1;
        }
    }
    (counts, rows.len() >= ARCHIVE_PAGE)
}

/// Track the operator's own `kind: 7` reactions keyed by
/// `(target_event_id, normalized_emoji) -> own_reaction_event_id` so the UI can
/// highlight active reactions, guard duplicates, and unreact via NIP-09.
fn own_reactions_from(
    rows: &[Row],
    my_pubkey_hex: Option<&str>,
) -> BTreeMap<(String, String), String> {
    let Some(mine) = my_pubkey_hex else {
        return BTreeMap::new();
    };
    let mut own = BTreeMap::new();
    for row in rows {
        if row.event.kind == 7
            && row.event.author.eq_ignore_ascii_case(mine)
            && let Some(target) = tag_value(&row.event, ["e", "E"])
        {
            own.insert(
                (target.to_string(), reaction_emoji(&row.event.content)),
                row.event.id.clone(),
            );
        }
    }
    own
}

/// How a reaction's content reads: NIP-25's like is `+` (and empty content is
/// read the same way), `-` is dislike (`👎`), and anything else is the
/// reaction's own text.
fn reaction_emoji(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed == "+" {
        "👍".to_string()
    } else if trimmed == "-" {
        "👎".to_string()
    } else {
        trimmed.to_string()
    }
}

/// How a row reads its reactions: each emoji with its count, the
/// most-reacted first — Amethyst's reactions row — and, when the read
/// was capped, the floor named.
fn reactions_text(counts: &BTreeMap<String, u64>, clipped: bool) -> String {
    let mut chips: Vec<(&String, &u64)> = counts.iter().collect();
    chips.sort_by(|(a_emoji, a_count), (b_emoji, b_count)| {
        b_count.cmp(a_count).then_with(|| a_emoji.cmp(b_emoji))
    });
    let mut parts: Vec<String> = chips
        .into_iter()
        .map(|(emoji, count)| format!("{emoji} {count}"))
        .collect();
    if clipped {
        parts.push("more may exist".to_string());
    }
    parts.join(" · ")
}

/// Decode a BOLT11 invoice's HRP amount in whole satoshis (local, zero network).
fn bolt11_amount_sats(invoice: &str) -> Option<u64> {
    let trimmed = invoice.trim().to_ascii_lowercase();
    let trimmed = trimmed.strip_prefix("lightning:").unwrap_or(&trimmed);
    let sep = trimmed.rfind('1')?;
    let hrp = &trimmed[..sep];
    let rest = hrp
        .strip_prefix("lnbcrt")
        .or_else(|| hrp.strip_prefix("lntbs"))
        .or_else(|| hrp.strip_prefix("lntb"))
        .or_else(|| hrp.strip_prefix("lnbc"))?;
    if rest.is_empty() {
        return None;
    }
    let (digits, mult_msat, div) = match rest.as_bytes().last().copied()? {
        b'm' => (&rest[..rest.len() - 1], 100_000_000u64, 1u64),
        b'u' => (&rest[..rest.len() - 1], 100_000u64, 1u64),
        b'n' => (&rest[..rest.len() - 1], 100u64, 1u64),
        b'p' => (&rest[..rest.len() - 1], 1u64, 10u64),
        b if b.is_ascii_digit() => (rest, 100_000_000_000u64, 1u64),
        _ => return None,
    };
    let n: u64 = digits.parse().ok()?;
    let msat = if div == 10 {
        if !n.is_multiple_of(10) {
            return None;
        }
        n / 10
    } else {
        n.checked_mul(mult_msat)?
    };
    let sats = msat / 1_000;
    (sats > 0).then_some(sats)
}

/// Aggregate verified NIP-57 `kind: 9735` zap receipts per referenced note id
/// as `(zap_count, total_sats)`. Verifies the embedded `kind: 9734` zap
/// request's id and BIP-340 signature through `hkask_nostr_core::Event::from_json`.
fn zap_receipt_totals(rows: &[Row]) -> BTreeMap<String, (u64, u64)> {
    let mut totals: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for row in rows {
        if row.event.kind != 9735 {
            continue;
        }
        let Some(target) = tag_value(&row.event, ["e", "E"]) else {
            continue;
        };
        let Some(bolt11) = tag_value(&row.event, ["bolt11", "bolt11"]) else {
            continue;
        };
        let Some(description) = tag_value(&row.event, ["description", "description"]) else {
            continue;
        };
        let Ok(zap_req) = hkask_nostr_core::Event::from_json(description) else {
            continue;
        };
        if zap_req.kind() != 9734 {
            continue;
        }
        if let Some(sats) = bolt11_amount_sats(bolt11) {
            let entry = totals.entry(target.to_string()).or_insert((0, 0));
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(sats);
        }
    }
    totals
}

fn zap_totals_text(count: u64, total_sats: u64) -> String {
    if count <= 1 {
        format!("⚡ {total_sats} sats")
    } else {
        format!("⚡ {total_sats} sats ({count})")
    }
}

/// One event as a view lists it: the event, and what a tool said about its
/// place.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Row {
    #[serde(flatten)]
    event: WireEvent,
    /// A notification's type (`reply`, `mention`, …).
    #[serde(default, rename = "type")]
    label: Option<String>,
    #[serde(default)]
    depth: usize,
    #[serde(default)]
    parent_missing: bool,
    #[serde(default)]
    is_focus: bool,
}

/// Where a feed's events came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeedOrigin {
    /// The local relay's archive, loaded by the viewer.
    Archive,
    /// A tool result in the conversation below.
    Conversation,
}

/// A line of fact about a result, and whether it is a caveat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Remark {
    text: SharedString,
    caveat: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Feed {
    name: SharedString,
    origin: FeedOrigin,
    rows: Vec<Row>,
    remarks: Vec<Remark>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Conversation {
    root: String,
    rows: Vec<Row>,
    missing: Vec<String>,
    remarks: Vec<Remark>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Surfaced {
    Feed(Feed),
    Conversation(Conversation),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct ListenerReading {
    #[serde(default)]
    state: String,
    address: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct ArchiveReading {
    #[serde(default)]
    stored_events: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct PolicyReading {
    #[serde(default)]
    allowlisted_authors: Vec<String>,
    #[serde(default)]
    refused_kinds: Vec<u16>,
    #[serde(default)]
    sync_relays: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct AdmissionTotals {
    #[serde(default)]
    admitted: u64,
    #[serde(default)]
    refused: u64,
}

/// `nostr_relay_status`, as much of it as the Relay view shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct RelayStatus {
    #[serde(default)]
    listener: ListenerReading,
    #[serde(default)]
    archive: ArchiveReading,
    #[serde(default)]
    policy: PolicyReading,
    #[serde(default)]
    admission_since_start: AdmissionTotals,
    #[serde(default)]
    configuration_warnings: Vec<String>,
    #[serde(default)]
    access_limits: Vec<String>,
}

/// One entry of `nostr_relay_log`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Decision {
    /// Null when the relay's clock could not say.
    #[serde(default)]
    at: Option<u64>,
    path: String,
    author: String,
    kind: u16,
    outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelayReading {
    status: RelayStatus,
    decisions: Vec<Decision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Load {
    Idle,
    Loading,
    Failed(SharedString),
}

/// One resource's request state. A result is applied only if it answers the
/// latest request, so a slow answer cannot overwrite a newer one.
#[derive(Debug)]
struct Request {
    epoch: u64,
    state: Load,
}

impl Request {
    fn new() -> Self {
        Self {
            epoch: 0,
            state: Load::Idle,
        }
    }

    fn begin(&mut self) -> u64 {
        self.epoch = self.epoch.wrapping_add(1);
        self.state = Load::Loading;
        self.epoch
    }

    /// Record the outcome of request `epoch`; false if a newer request has
    /// been made since.
    fn finish(&mut self, epoch: u64, error: Option<String>) -> bool {
        if self.epoch != epoch {
            return false;
        }
        self.state = match error {
            None => Load::Idle,
            Some(error) => Load::Failed(error.into()),
        };
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerTab {
    Timeline,
    Notifications,
    Conversation,
    Relay,
    Messages,
}

pub struct NostrViewer {
    tab: ViewerTab,
    feed: Option<Feed>,
    following_feed: Option<Feed>,
    notifications_feed: Option<Feed>,
    conversation: Option<Conversation>,
    relay: Option<RelayReading>,
    /// The selected event's id, in the view it was selected in.
    selected: Option<String>,
    archive_request: Request,
    following_request: Request,
    notifications_request: Request,
    thread_request: Request,
    relay_request: Request,
    started: bool,
    /// The conversation whose tool results are ingested.
    thread: Option<WeakEntity<acp_thread::AcpThread>>,
    processed_tool_results: HashSet<ToolCallId>,
    /// The Timeline's named feed (operator ruling 2026-11-14: For reading
    /// is the default).
    timeline_filter: TimelineFilter,
    /// The record groups whose collapsed row is expanded, by author and
    /// local minute.
    expanded_records: HashSet<(String, i64)>,
    /// Reaction counts by the note they answer and the emoji each
    /// carries.
    reactions: BTreeMap<String, BTreeMap<String, u64>>,
    /// Operator's own `kind: 7` reactions keyed by `(target_event_id, emoji) -> reaction_event_id`.
    own_reactions: BTreeMap<(String, String), String>,
    /// NIP-57 `kind: 9735` zap totals `(count, total_sats)` keyed by note id.
    zap_totals: BTreeMap<String, (u64, u64)>,
    /// The reactions read hit its limit, so every count is a floor.
    reactions_clipped: bool,
    /// The reactions read's own epoch and failure, so a stale answer is
    /// not applied and a failed read is visible.
    reactions_request: Request,
    /// What the reader's own follows are, once the local reads answer.
    follows: Option<Follows>,
    /// The follows read's own epoch and failure, so a stale answer is
    /// not applied and a failed read is visible.
    follows_request: Request,
    /// Each note's resolved media attachment, keyed by note id.
    media_attachments: BTreeMap<String, MediaAttachment>,
    /// Each note's own blurhash decoded to a small preview, keyed by
    /// note id. Local computation on the note's own tags; nothing is
    /// fetched.
    media_previews: BTreeMap<String, Arc<RenderImage>>,
    /// Images a labelled fetch click asked for, decoded, keyed by note id.
    /// Each is a tool result: the media server's fetch returned the local
    /// path this decoded from.
    fetched_images: BTreeMap<String, Arc<RenderImage>>,
    /// Each fetched image's own request, so a stale answer is not applied
    /// and a failed fetch is visible with its retry offered.
    image_requests: BTreeMap<String, Request>,
    /// Embedded MediaWidget views for video attachments, keyed by note id.
    video_widgets: BTreeMap<String, Entity<MediaWidget>>,
    /// Cached Markdown entities for selected `kind: 30023` articles.
    article_markdowns: BTreeMap<String, Entity<Markdown>>,
    /// Embedded MediaWidget views for platform video links fetched via `video_fetch`.
    link_video_widgets: BTreeMap<String, Entity<MediaWidget>>,
    /// In-flight / failed state for `video_fetch` requests, keyed by link URL.
    link_video_requests: BTreeMap<String, Request>,
    /// Names asked for through the detail pane's labelled Look up
    /// profile: an author's name, or `None` when the profile carried no
    /// readable name. The network was asked only on the labelled click.
    profile_lookups: BTreeMap<String, Option<String>>,
    /// The threads a reply chip's jump left behind, oldest first, so
    /// Back can return to them.
    conversation_back: Vec<String>,
    timeline_scroll_handle: ScrollHandle,
    notifications_scroll_handle: ScrollHandle,
    conversation_scroll_handle: ScrollHandle,
    relay_scroll_handle: ScrollHandle,
    detail_scroll_handle: ScrollHandle,
    composer: ComposerState,
    composer_editor: Option<Entity<editor::Editor>>,
    composer_media_editor: Option<Entity<editor::Editor>>,
    _composer_subscription: Option<gpui::Subscription>,
    _publish_task: Option<Task<()>>,
    /// Whether raw protocol metadata (hex id/author/kind/source) is expanded in `detail()`.
    detail_meta_expanded: bool,
    /// Compact Amethyst-style quick-zap strip state for the selected note (NIP-57 / NIP-61).
    zap: Option<ZapDraftState>,
    zap_amount_editor: Option<Entity<editor::Editor>>,
    zap_comment_editor: Option<Entity<editor::Editor>>,
    _zap_wallet_task: Option<Task<()>>,
    _zap_task: Option<Task<()>>,
    /// Event id currently sending a NIP-25 `kind: 7` reaction, plus last inline feedback.
    reaction_in_flight: Option<String>,
    reaction_feedback: Option<(String, bool, String)>,
    _reaction_task: Option<Task<()>>,
    /// Editor-only NIP-17 encrypted DMs (SQLCipher-backed, never in Steer/MCP).
    dm_threads: Vec<DmThreadSummary>,
    dm_selected_peer: Option<String>,
    dm_messages: Vec<DmMessage>,
    dm_status: Option<(bool, String)>,
    dm_busy: bool,
    dm_loaded_local: bool,
    dm_recipient_editor: Option<Entity<editor::Editor>>,
    dm_input_editor: Option<Entity<editor::Editor>>,
    dm_scroll_handle: ScrollHandle,
    _dm_task: Option<Task<()>>,
}

// ── Reading tool results ────────────────────────────────────────────

/// Text a third party wrote, made safe to draw: control characters other
/// than line breaks and tabs are shown as U+FFFD, so a note cannot hide or
/// reorder what is on screen.
fn display_text(text: &str) -> SharedString {
    let shown: String = text
        .chars()
        .map(|character| match character {
            '\n' | '\t' => character,
            // Controls, and the bidirectional overrides and isolates.
            c if c.is_control() => '\u{FFFD}',
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' => {
                '\u{FFFD}'
            }
            c => c,
        })
        .collect();
    shown.into()
}

/// The first and last characters of a hex id or key.
fn short_hex(hex: &str) -> String {
    if hex.len() <= 16 || !hex.is_ascii() {
        return hex.to_string();
    }
    format!("{}…{}", &hex[..8], &hex[hex.len() - 4..])
}

/// An author's face when no picture is fetched: a color drawn from the
/// pubkey itself, so one author is always the same color. Nothing is
/// fetched — the viewer's third rule.
fn identicon_color(author: &str) -> gpui::Hsla {
    // One face per author needs two things the obvious code gets wrong:
    // a hash that separates keys differing only in repeated bytes — the
    // FNV-1a fold alone is linear in the bytes, so it gets a
    // splitmix-style finalizer — and a hue written as a fraction of a
    // turn, as `Hsla::h` defines: a degree value clamps to 1.0 and every
    // author gets the same face.
    let mut hash = author.as_bytes().iter().fold(0x811c_9dc5_u32, |acc, byte| {
        (acc ^ *byte as u32).wrapping_mul(0x0100_0193)
    });
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7feb_352d);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x846c_a68b);
    hash ^= hash >> 16;
    gpui::hsla((hash % 360) as f32 / 360.0, 0.55, 0.5, 1.0)
}

/// The avatar a row's header carries.
fn avatar(author: &str) -> impl IntoElement + use<> {
    div()
        .flex_shrink_0()
        .size(px(14.))
        .rounded_full()
        .bg(identicon_color(author))
}

/// A Unix time as UTC text. An event's time is what its author claimed;
/// the exact UTC reading and the raw seconds stay in the detail pane.
fn utc_text(seconds: u64) -> String {
    let Some(timestamp) = i64::try_from(seconds)
        .ok()
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
    else {
        return format!("{seconds} (not a date)");
    };
    format!(
        "{} UTC",
        time_format::format_localized_timestamp(
            timestamp,
            OffsetDateTime::now_utc(),
            UtcOffset::UTC,
            TimestampFormat::Absolute,
        )
    )
}

fn rows_at(content: &Value, key: &str) -> Option<Result<Vec<Row>, String>> {
    let list = content.get(key)?.as_array()?;
    Some(
        list.iter()
            .map(|item| serde_json::from_value::<Row>(item.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("a tool returned {key} that are not events: {error}")),
    )
}

/// What a read said about itself: how each relay answered, and whether more
/// may exist.
fn remarks_of(content: &Value) -> Vec<Remark> {
    let mut remarks = Vec::new();
    if let Some(relays) = content.get("relays").and_then(Value::as_array) {
        let mut states: Vec<(String, usize)> = Vec::new();
        for relay in relays {
            let state = relay
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            match states.iter_mut().find(|(seen, _)| *seen == state) {
                Some((_, count)) => *count += 1,
                None => states.push((state, 1)),
            }
        }
        let summary = states
            .iter()
            .map(|(state, count)| format!("{count} {state}"))
            .collect::<Vec<_>>()
            .join(", ");
        remarks.push(Remark {
            text: format!("{} relays asked: {summary}", relays.len()).into(),
            caveat: content.get("every_relay_finished") == Some(&Value::Bool(false)),
        });
    }
    if content.get("more_may_exist") == Some(&Value::Bool(true)) {
        remarks.push(Remark {
            text: "More may exist than is shown.".into(),
            caveat: true,
        });
    }
    if let Some(scope) = content.get("relay_scope").and_then(Value::as_str) {
        remarks.push(Remark {
            text: display_text(scope),
            caveat: false,
        });
    }
    remarks
}

/// Read a tool's result as events, by its shape. `None` when the result is
/// not a list of events: most tool results are not.
pub(crate) fn parse_surfaced(
    content: &Value,
    origin: FeedOrigin,
) -> Option<Result<Surfaced, String>> {
    if let Some(rows) = rows_at(content, "notes") {
        return Some(rows.map(|rows| {
            Surfaced::Conversation(Conversation {
                root: content
                    .get("root")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                rows,
                missing: content
                    .get("missing")
                    .and_then(Value::as_array)
                    .map(|missing| {
                        missing
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                remarks: remarks_of(content),
            })
        }));
    }
    let (rows, name) = if let Some(rows) = rows_at(content, "notifications") {
        (rows, "Notifications")
    } else if let Some(rows) = rows_at(content, "events") {
        // Only a read of relays reports how each relay answered.
        let name = if content.get("relays").is_some() {
            "Read from relays"
        } else {
            "Local archive"
        };
        (rows, name)
    } else {
        return None;
    };
    Some(rows.map(|rows| {
        Surfaced::Feed(Feed {
            name: name.into(),
            origin,
            rows,
            remarks: remarks_of(content),
        })
    }))
}

/// A sync result that stored something, or a published note — the archive
/// on screen is stale either way, and a publish asks the local relay to
/// hold a new event.
fn archive_changed(content: &Value) -> bool {
    (content.get("fetched").is_some()
        && content.get("stored").and_then(Value::as_u64).unwrap_or(0) > 0)
        || content.get("published").is_some()
}

/// The payload of a tool call made through the invoker, or why there is
/// none.
fn content_of(tool: &str, outcome: Result<String, InvokeError>) -> Result<Value, String> {
    let text = outcome.map_err(|error| format!("{tool} failed: {}", error.message()))?;
    let content = hkask_types::tool_response::parse_tool_response(&text)
        .ok_or_else(|| format!("{tool} returned text that is not JSON"))?;
    let error = hkask_types::tool_response::parse_tool_error(&text)
        .or_else(|| hkask_types::tool_response::parse_tool_error_value(&content));
    match error {
        Some(error) => Err(format!("{tool} failed: {}", error.message)),
        None => Ok(content),
    }
}

fn invoke(
    server: &'static str,
    tool: &'static str,
    arguments: Value,
) -> Result<Task<Result<String, InvokeError>>, String> {
    let invoker = hkask_tool_invoker::shared_tool_invoker()
        .ok_or_else(|| hkask_tool_invoker::NOT_WIRED_MESSAGE.to_string())?;
    Ok(invoker.invoke_tool(server, tool, arguments))
}

impl Default for NostrViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl NostrViewer {
    pub fn new() -> Self {
        Self {
            tab: ViewerTab::Timeline,
            feed: None,
            following_feed: None,
            notifications_feed: None,
            conversation: None,
            relay: None,
            selected: None,
            archive_request: Request::new(),
            following_request: Request::new(),
            notifications_request: Request::new(),
            thread_request: Request::new(),
            relay_request: Request::new(),
            started: false,
            thread: None,
            processed_tool_results: HashSet::new(),
            timeline_filter: TimelineFilter::default(),
            expanded_records: HashSet::new(),
            reactions: BTreeMap::new(),
            own_reactions: BTreeMap::new(),
            zap_totals: BTreeMap::new(),
            reactions_clipped: false,
            reactions_request: Request::new(),
            follows: None,
            follows_request: Request::new(),
            media_attachments: BTreeMap::new(),
            media_previews: BTreeMap::new(),
            fetched_images: BTreeMap::new(),
            image_requests: BTreeMap::new(),
            video_widgets: BTreeMap::new(),
            article_markdowns: BTreeMap::new(),
            link_video_widgets: BTreeMap::new(),
            link_video_requests: BTreeMap::new(),
            profile_lookups: BTreeMap::new(),
            conversation_back: Vec::new(),
            timeline_scroll_handle: ScrollHandle::new(),
            notifications_scroll_handle: ScrollHandle::new(),
            conversation_scroll_handle: ScrollHandle::new(),
            relay_scroll_handle: ScrollHandle::new(),
            detail_scroll_handle: ScrollHandle::new(),
            composer: ComposerState::new(),
            composer_editor: None,
            composer_media_editor: None,
            _composer_subscription: None,
            _publish_task: None,
            detail_meta_expanded: false,
            zap: None,
            zap_amount_editor: None,
            zap_comment_editor: None,
            _zap_wallet_task: None,
            _zap_task: None,
            reaction_in_flight: None,
            reaction_feedback: None,
            _reaction_task: None,
            dm_threads: Vec::new(),
            dm_selected_peer: None,
            dm_messages: Vec::new(),
            dm_status: None,
            dm_busy: false,
            dm_loaded_local: false,
            dm_recipient_editor: None,
            dm_input_editor: None,
            dm_scroll_handle: ScrollHandle::new(),
            _dm_task: None,
        }
    }

    /// Load the archive the first time the panel draws.
    pub fn ensure_started(&mut self, cx: &mut Context<Self>) {
        if !self.started {
            self.started = true;
            self.load_archive(cx);
        }
    }

    fn activate_tab(&mut self, tab: ViewerTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.selected = None;
        }
        if tab == ViewerTab::Relay && self.relay.is_none() {
            self.load_relay(cx);
        }
        if tab == ViewerTab::Notifications && self.notifications_feed.is_none() {
            self.load_notifications(cx);
        }
        cx.notify();
    }

    /// Reload what the current view shows.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        match self.tab {
            ViewerTab::Relay => self.load_relay(cx),
            ViewerTab::Notifications => self.load_notifications(cx),
            ViewerTab::Messages => self.sync_dms(cx),
            ViewerTab::Timeline if self.timeline_filter == TimelineFilter::Following => {
                if !matches!(self.follows, Some(Follows::Authors(_))) {
                    self.load_follows(cx);
                } else {
                    self.load_following_feed(cx);
                }
            }
            ViewerTab::Timeline | ViewerTab::Conversation => self.load_archive(cx),
        }
    }

    // ── The archive feed ────────────────────────────────────────────

    /// Load notifications tagging the configured identity (`nostr_notifications`).
    fn load_notifications(&mut self, cx: &mut Context<Self>) {
        let epoch = self.notifications_request.begin();
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            NOTIFICATIONS_TOOL,
            json!({ "limit": 50 }),
        ) {
            Ok(task) => task,
            Err(error) => return self.apply_notifications(epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
            let result =
                content_of(NOTIFICATIONS_TOOL, task.await).and_then(
                    |content| match parse_surfaced(&content, FeedOrigin::Conversation) {
                        Some(Ok(Surfaced::Feed(feed))) => Ok(feed),
                        Some(Err(error)) => Err(error),
                        _ => Err(format!("{NOTIFICATIONS_TOOL} returned no notifications")),
                    },
                );
            this.update(cx, |this, cx| this.apply_notifications(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_notifications(
        &mut self,
        epoch: u64,
        result: Result<Feed, String>,
        cx: &mut Context<Self>,
    ) {
        let (feed, error) = match result {
            Ok(feed) => (Some(feed), None),
            Err(error) => (None, Some(error)),
        };
        if !self.notifications_request.finish(epoch, error) {
            return;
        }
        if let Some(feed) = feed {
            self.load_reactions(&feed, cx);
            self.notifications_feed = Some(feed);
        }
        cx.notify();
    }

    /// Read the local relay's archive. Local: the tool contacts no relay.
    pub fn load_archive(&mut self, cx: &mut Context<Self>) {
        let epoch = self.archive_request.begin();
        cx.notify();
        let task = match invoke(
            NOSTR_RELAY_SERVER,
            ARCHIVE_TOOL,
            json!({ "limit": ARCHIVE_PAGE }),
        ) {
            Ok(task) => task,
            Err(error) => return self.apply_archive(epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
            let result = content_of(ARCHIVE_TOOL, task.await).and_then(|content| {
                match parse_surfaced(&content, FeedOrigin::Archive) {
                    Some(Ok(Surfaced::Feed(feed))) => Ok(feed),
                    Some(Err(error)) => Err(error),
                    _ => Err(format!("{ARCHIVE_TOOL} returned no list of events")),
                }
            });
            this.update(cx, |this, cx| this.apply_archive(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_archive(&mut self, epoch: u64, result: Result<Feed, String>, cx: &mut Context<Self>) {
        let (feed, error) = match result {
            Ok(feed) => (Some(feed), None),
            Err(error) => (None, Some(error)),
        };
        if !self.archive_request.finish(epoch, error) {
            return;
        }
        // A failed load keeps the last feed on screen, under the error.
        if let Some(feed) = feed {
            self.load_reactions(&feed, cx);
            self.load_follows(cx);
            self.show_feed(feed, cx);
        }
        cx.notify();
    }

    fn show_feed(&mut self, feed: Feed, cx: &mut Context<Self>) {
        let (attachments, previews) = media_index_of(&feed.rows);
        self.media_attachments = attachments;
        self.media_previews = previews;
        // A fetched image belongs to the page that offered its click; the
        // next page re-offers its own.
        self.fetched_images.clear();
        self.image_requests.clear();
        self.video_widgets.clear();
        self.article_markdowns.clear();
        if self.tab == ViewerTab::Timeline
            && !feed
                .rows
                .iter()
                .any(|row| Some(&row.event.id) == self.selected.as_ref())
        {
            self.selected = None;
        }
        self.feed = Some(feed);
        self.auto_fetch_followed_media(cx);
    }

    /// Refresh whichever view is currently active (Conversation, Following, or
    /// Archive) after a publish, reaction, or zap.
    fn refresh_active_view(&mut self, cx: &mut Context<Self>) {
        self.load_archive(cx);
        match self.tab {
            ViewerTab::Conversation => {
                if let Some(root) = self.conversation.as_ref().map(|c| c.root.clone()) {
                    if !root.is_empty() {
                        self.read_conversation(root, cx);
                    }
                }
            }
            ViewerTab::Timeline if self.timeline_filter == TimelineFilter::Following => {
                if matches!(self.follows, Some(Follows::Authors(_))) {
                    self.load_following_feed(cx);
                }
            }
            _ => {}
        }
    }

    /// Re-load reactions and zap totals for whichever feed/conversation is visible.
    fn refresh_reactions_for_active_view(&mut self, cx: &mut Context<Self>) {
        match self.tab {
            ViewerTab::Conversation => {
                if let Some(rows) = self.conversation.as_ref().map(|c| c.rows.clone()) {
                    self.load_reactions_for_rows(&rows, true, cx);
                }
            }
            ViewerTab::Timeline if self.timeline_filter == TimelineFilter::Following => {
                if let Some(rows) = self.following_feed.as_ref().map(|f| f.rows.clone()) {
                    self.load_reactions_for_rows(&rows, true, cx);
                }
            }
            _ => {
                if let Some(rows) = self.feed.as_ref().map(|f| f.rows.clone()) {
                    self.load_reactions_for_rows(&rows, false, cx);
                }
            }
        }
    }

    fn my_pubkey_hex(&self, cx: &Context<Self>) -> Option<String> {
        nostr_dm::parse_recipient_pubkey(&KaskSettings::get_global(cx).nostr.npub)
            .ok()
            .map(|pk| pk.to_hex())
    }

    /// Read the reactions (`kind: 7`) and zap receipts (`kind: 9735`) for a
    /// slice of displayed rows. Always queries the local archive; when
    /// `include_seed_relays` is true (Following / Conversation), also queries
    /// seed relays for the operator and followed authors (up to 50) and merges
    /// the deduplicated results.
    fn load_reactions(&mut self, feed: &Feed, cx: &mut Context<Self>) {
        let include_seed = feed.origin != FeedOrigin::Archive;
        self.load_reactions_for_rows(&feed.rows, include_seed, cx);
    }

    fn load_reactions_for_rows(
        &mut self,
        rows: &[Row],
        include_seed_relays: bool,
        cx: &mut Context<Self>,
    ) {
        self.reactions.clear();
        self.own_reactions.clear();
        self.zap_totals.clear();
        self.reactions_clipped = false;
        let notes: Vec<String> = rows
            .iter()
            .filter(|row| kind_class(row.event.kind) == KindClass::Note)
            .map(|row| row.event.id.clone())
            .collect();
        if notes.is_empty() {
            cx.notify();
            return;
        }
        let epoch = self.reactions_request.begin();
        cx.notify();
        let local_task = match invoke(
            NOSTR_RELAY_SERVER,
            ARCHIVE_TOOL,
            json!({ "kinds": [7, 9735], "references": notes, "limit": ARCHIVE_PAGE }),
        ) {
            Ok(task) => task,
            Err(error) => return self.apply_reactions(epoch, Err(error), cx),
        };

        let seed_task = if include_seed_relays {
            let mut authors: Vec<String> = Vec::new();
            if let Some(mine) = self.my_pubkey_hex(cx) {
                authors.push(mine);
            }
            if let Some(Follows::Authors(followed)) = &self.follows {
                for author in followed {
                    if authors.len() >= 50 {
                        break;
                    }
                    if !authors.contains(author) {
                        authors.push(author.clone());
                    }
                }
            }
            if authors.is_empty() {
                None
            } else {
                invoke(
                    NOSTR_CLIENT_SERVER,
                    FETCH_TOOL,
                    json!({ "authors": authors, "kinds": [7, 9735], "limit": ARCHIVE_PAGE }),
                )
                .ok()
            }
        } else {
            None
        };

        cx.spawn(async move |this, cx| {
            let local_result =
                content_of(ARCHIVE_TOOL, local_task.await).and_then(
                    |content| match parse_surfaced(&content, FeedOrigin::Archive) {
                        Some(Ok(Surfaced::Feed(feed))) => Ok(feed),
                        Some(Err(error)) => Err(error),
                        _ => Err(format!("{ARCHIVE_TOOL} returned no list of events")),
                    },
                );
            let merged = match local_result {
                Ok(mut feed) => {
                    if let Some(seed_task) = seed_task {
                        if let Ok(content) = content_of(FETCH_TOOL, seed_task.await) {
                            if let Some(Ok(Surfaced::Feed(seed_feed))) =
                                parse_surfaced(&content, FeedOrigin::Conversation)
                            {
                                let mut seen: HashSet<String> =
                                    feed.rows.iter().map(|r| r.event.id.clone()).collect();
                                for row in seed_feed.rows {
                                    if seen.insert(row.event.id.clone()) {
                                        feed.rows.push(row);
                                    }
                                }
                            }
                        }
                    }
                    Ok(feed)
                }
                Err(err) => Err(err),
            };
            this.update(cx, |this, cx| this.apply_reactions(epoch, merged, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_reactions(
        &mut self,
        epoch: u64,
        result: Result<Feed, String>,
        cx: &mut Context<Self>,
    ) {
        let (feed, error) = match result {
            Ok(feed) => (Some(feed), None),
            Err(error) => (None, Some(error)),
        };
        if !self.reactions_request.finish(epoch, error) {
            return;
        }
        if let Some(feed) = feed {
            let (counts, clipped) = reaction_counts(&feed.rows);
            let mine = self.my_pubkey_hex(cx);
            self.reactions = counts;
            self.own_reactions = own_reactions_from(&feed.rows, mine.as_deref());
            self.zap_totals = zap_receipt_totals(&feed.rows);
            self.reactions_clipped = clipped;
        }
        cx.notify();
    }

    /// Read the authors your own newest kind 3 names. The identity is the
    /// `kask.nostr.npub` setting — public data, read from the live
    /// settings, no dispatch; the follows list is one local archive read.
    /// No relay is contacted. A setting that is not a shape the archive
    /// accepts is refused here and never sent, so no error can repeat it.
    fn load_follows(&mut self, cx: &mut Context<Self>) {
        let identity = KaskSettings::get_global(cx).nostr.npub.trim().to_string();
        if identity.is_empty() {
            self.follows = Some(Follows::NoIdentity);
            cx.notify();
            return;
        }
        if !identity_is_sendable(&identity) {
            self.follows = Some(Follows::InvalidIdentity);
            cx.notify();
            return;
        }
        let epoch = self.follows_request.begin();
        cx.notify();
        let task = match invoke(
            NOSTR_RELAY_SERVER,
            ARCHIVE_TOOL,
            json!({ "kinds": [3], "authors": [identity], "limit": 1 }),
        ) {
            Ok(task) => task,
            Err(error) => return self.apply_follows(epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
            let result = content_of(ARCHIVE_TOOL, task.await).and_then(|content| {
                match parse_surfaced(&content, FeedOrigin::Archive) {
                    Some(Ok(Surfaced::Feed(feed))) => Ok(match feed.rows.first() {
                        Some(row) => Follows::Authors(follows_of(&row.event)),
                        None => Follows::NoList,
                    }),
                    Some(Err(error)) => Err(error),
                    _ => Err(format!("{ARCHIVE_TOOL} returned no list of events")),
                }
            });
            this.update(cx, |this, cx| this.apply_follows(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_follows(
        &mut self,
        epoch: u64,
        result: Result<Follows, String>,
        cx: &mut Context<Self>,
    ) {
        let (follows, error) = match result {
            Ok(follows) => (Some(follows), None),
            Err(error) => (None, Some(error)),
        };
        if !self.follows_request.finish(epoch, error) {
            return;
        }
        if let Some(follows) = follows {
            if matches!(follows, Follows::NoList) {
                self.load_follows_from_relays(cx);
            }
            self.follows = Some(follows);
            if self.timeline_filter == TimelineFilter::Following
                && matches!(self.follows, Some(Follows::Authors(_)))
            {
                self.load_following_feed(cx);
            }
            self.auto_fetch_followed_media(cx);
        }
        cx.notify();
    }

    /// Fallback when the local archive has no kind 3 for the configured identity:
    /// fetch their kind 3 from the seed relays so Following works without a
    /// prior manual sync.
    fn load_follows_from_relays(&mut self, cx: &mut Context<Self>) {
        let identity = KaskSettings::get_global(cx).nostr.npub.trim().to_string();
        if identity.is_empty() || !identity_is_sendable(&identity) {
            return;
        }
        let epoch = self.follows_request.begin();
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            FETCH_TOOL,
            json!({ "kinds": [3], "authors": [identity], "limit": 1 }),
        ) {
            Ok(task) => task,
            Err(error) => {
                self.follows_request.finish(epoch, Some(error));
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let result =
                content_of(FETCH_TOOL, task.await).and_then(|content| {
                    match parse_surfaced(&content, FeedOrigin::Conversation) {
                        Some(Ok(Surfaced::Feed(feed))) => Ok(match feed.rows.first() {
                            Some(row) => Follows::Authors(follows_of(&row.event)),
                            None => Follows::NoList,
                        }),
                        Some(Err(error)) => Err(error),
                        _ => Err(format!("{FETCH_TOOL} returned no list of events")),
                    }
                });
            this.update(cx, |this, cx| {
                let (follows, error) = match result {
                    Ok(follows) => (Some(follows), None),
                    Err(error) => (None, Some(error)),
                };
                if !this.follows_request.finish(epoch, error) {
                    return;
                }
                if let Some(follows) = follows {
                    this.follows = Some(follows);
                    if this.timeline_filter == TimelineFilter::Following
                        && matches!(this.follows, Some(Follows::Authors(_)))
                    {
                        this.load_following_feed(cx);
                    }
                    this.auto_fetch_followed_media(cx);
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Fetch recent notes, articles (kind 30023), and profiles (kind 0) for the
    /// authors in `Follows::Authors` from the local relay and seed relays.
    pub fn load_following_feed(&mut self, cx: &mut Context<Self>) {
        let Some(Follows::Authors(authors)) = &self.follows else {
            return;
        };
        if authors.is_empty() {
            return;
        }
        let authors_vec: Vec<String> = authors.iter().take(50).cloned().collect();
        let epoch = self.following_request.begin();
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            FETCH_TOOL,
            json!({
                "authors": authors_vec,
                "kinds": [0, 1, 30023],
                "limit": 50
            }),
        ) {
            Ok(task) => task,
            Err(error) => return self.apply_following_feed(epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
            let result =
                content_of(FETCH_TOOL, task.await).and_then(|content| {
                    match parse_surfaced(&content, FeedOrigin::Conversation) {
                        Some(Ok(Surfaced::Feed(feed))) => Ok(feed),
                        Some(Err(error)) => Err(error),
                        _ => Err(format!("{FETCH_TOOL} returned no list of events")),
                    }
                });
            this.update(cx, |this, cx| this.apply_following_feed(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_following_feed(
        &mut self,
        epoch: u64,
        result: Result<Feed, String>,
        cx: &mut Context<Self>,
    ) {
        let (feed, error) = match result {
            Ok(feed) => (Some(feed), None),
            Err(error) => (None, Some(error)),
        };
        if !self.following_request.finish(epoch, error) {
            return;
        }
        if let Some(feed) = feed {
            let (mut attachments, mut previews) = media_index_of(&feed.rows);
            self.media_attachments.append(&mut attachments);
            self.media_previews.append(&mut previews);
            self.load_reactions(&feed, cx);
            self.following_feed = Some(feed);
            self.auto_fetch_followed_media(cx);
        }
        cx.notify();
    }

    fn is_followed_author(&self, author: &str) -> bool {
        matches!(&self.follows, Some(Follows::Authors(authors)) if authors.contains(author))
    }

    fn load_video(&mut self, id: String, url: String, cx: &mut Context<Self>) {
        if self.video_widgets.contains_key(&id) {
            return;
        }
        let widget = cx.new(|cx| {
            let mut widget =
                MediaWidget::new(MediaRef::new(SharedString::from(url), MediaKind::Video), cx);
            widget.load(cx);
            widget
        });
        self.video_widgets.insert(id, widget);
        cx.notify();
    }

    fn ensure_article_markdown(&mut self, id: &str, content: &str, cx: &mut Context<Self>) {
        if self.article_markdowns.contains_key(id) {
            return;
        }
        let text = content.to_string();
        let md = cx.new(|cx| Markdown::new(text.into(), None, None, cx));
        self.article_markdowns.insert(id.to_string(), md);
    }

    fn fetch_platform_video(&mut self, url: String, cx: &mut Context<Self>) {
        let epoch = self
            .link_video_requests
            .entry(url.clone())
            .or_insert_with(Request::new)
            .begin();
        cx.notify();
        let task = match invoke(MEDIA_SERVER, MEDIA_VIDEO_FETCH_TOOL, json!({ "url": url })) {
            Ok(task) => task,
            Err(error) => return self.apply_fetched_platform_video(url, epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
            let result = async {
                let content = content_of(MEDIA_VIDEO_FETCH_TOOL, task.await)?;
                let path = content
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{MEDIA_VIDEO_FETCH_TOOL} returned no path"))?
                    .to_string();
                approve_gallery_media_path(std::path::Path::new(&path))
                    .map_err(|error| format!("approve fetched video {path}: {error}"))?;
                Ok(path)
            }
            .await;
            this.update(cx, |this, cx| {
                this.apply_fetched_platform_video(url, epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    fn apply_fetched_platform_video(
        &mut self,
        url: String,
        epoch: u64,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        let (path, error) = match result {
            Ok(path) => (Some(path), None),
            Err(error) => (None, Some(error)),
        };
        let Some(request) = self.link_video_requests.get_mut(&url) else {
            return;
        };
        if !request.finish(epoch, error) {
            return;
        }
        if let Some(path) = path {
            let widget = cx.new(|cx| {
                let mut widget = MediaWidget::new(
                    MediaRef::new(SharedString::from(path), MediaKind::Video),
                    cx,
                );
                widget.load(cx);
                widget
            });
            self.link_video_widgets.insert(url, widget);
        }
        cx.notify();
    }

    fn auto_fetch_followed_media(&mut self, cx: &mut Context<Self>) {
        let mut to_fetch_images = Vec::new();
        let mut to_load_videos = Vec::new();
        for feed in [self.feed.as_ref(), self.following_feed.as_ref()]
            .into_iter()
            .flatten()
        {
            for row in &feed.rows {
                if !self.is_followed_author(&row.event.author) {
                    continue;
                }
                let Some(meta) = self.media_attachments.get(&row.event.id).cloned() else {
                    continue;
                };
                match meta.kind {
                    MediaAttachmentKind::Image => {
                        if !self.fetched_images.contains_key(&row.event.id)
                            && !self.image_requests.contains_key(&row.event.id)
                        {
                            to_fetch_images.push((row.event.id.clone(), meta.url));
                        }
                    }
                    MediaAttachmentKind::Video => {
                        if !self.video_widgets.contains_key(&row.event.id) {
                            to_load_videos.push((row.event.id.clone(), meta.url));
                        }
                    }
                }
            }
        }
        for (id, url) in to_fetch_images {
            self.fetch_image(id, url, cx);
        }
        for (id, url) in to_load_videos {
            self.load_video(id, url, cx);
        }
    }

    // ── A conversation ──────────────────────────────────────────────

    /// Ask the client server for the conversation `event_id` belongs to.
    /// This contacts the local relay and the seed relays. A click made
    /// while a thread is on screen remembers that thread behind the new
    /// one, so the jump a reply chip makes can be walked back.
    fn read_conversation(&mut self, event_id: String, cx: &mut Context<Self>) {
        if let Some(left) = self.thread_left_behind(&event_id) {
            self.conversation_back.push(left);
            let excess = self
                .conversation_back
                .len()
                .saturating_sub(CONVERSATION_BACK_LIMIT);
            self.conversation_back.drain(..excess);
        }
        let epoch = self.thread_request.begin();
        self.tab = ViewerTab::Conversation;
        cx.notify();
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            THREAD_TOOL,
            json!({ "event": event_id }),
        ) {
            Ok(task) => task,
            Err(error) => return self.apply_conversation(epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
            let result =
                content_of(THREAD_TOOL, task.await).and_then(|content| {
                    match parse_surfaced(&content, FeedOrigin::Conversation) {
                        Some(Ok(Surfaced::Conversation(conversation))) => Ok(conversation),
                        Some(Err(error)) => Err(error),
                        _ => Err(format!("{THREAD_TOOL} returned no conversation")),
                    }
                });
            this.update(cx, |this, cx| this.apply_conversation(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_conversation(
        &mut self,
        epoch: u64,
        result: Result<Conversation, String>,
        cx: &mut Context<Self>,
    ) {
        let (conversation, error) = match result {
            Ok(conversation) => (Some(conversation), None),
            Err(error) => (None, Some(error)),
        };
        if !self.thread_request.finish(epoch, error) {
            return;
        }
        if let Some(conversation) = conversation {
            self.load_reactions_for_rows(&conversation.rows, true, cx);
            self.show_conversation(conversation);
        }
        cx.notify();
    }

    fn show_conversation(&mut self, conversation: Conversation) {
        self.selected = conversation
            .rows
            .iter()
            .find(|row| row.is_focus)
            .map(|row| row.event.id.clone());
        self.conversation = Some(conversation);
        self.tab = ViewerTab::Conversation;
    }

    /// The thread a refocus of the conversation leaves behind: only a
    /// click made while a conversation is on screen remembers anything,
    /// and never the thread asked for itself.
    fn thread_left_behind(&self, event_id: &str) -> Option<String> {
        if self.tab != ViewerTab::Conversation {
            return None;
        }
        let conversation = self.conversation.as_ref()?;
        (conversation.root != event_id).then(|| conversation.root.clone())
    }

    /// The names a view's rows know: each author's newest kind 0 in the
    /// rows, then any name a labelled profile lookup asked for. A lookup
    /// never overwrites the archive's own record.
    fn names_of(&self, rows: &[Row]) -> BTreeMap<String, AuthorProfile> {
        let mut names = names_from_rows(rows);
        for (author, name) in &self.profile_lookups {
            if let Some(name) = name {
                names
                    .entry(author.clone())
                    .or_default()
                    .name
                    .get_or_insert_with(|| name.clone());
            }
        }
        names
    }

    // ── A labelled profile lookup ──────────────────────────────────

    /// Ask the client server for this author's profile, through the
    /// governed invoker: the name that comes back is tool-traced, not the
    /// viewer's own. Nothing is written; the click is not offered again
    /// once asked.
    fn look_up_profile(&mut self, author: String, cx: &mut Context<Self>) {
        let task = match invoke(
            NOSTR_CLIENT_SERVER,
            PROFILE_TOOL,
            json!({ "authors": [&author] }),
        ) {
            Ok(task) => task,
            Err(error) => {
                log::warn!("nostr panel: {error}");
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let asked = match content_of(PROFILE_TOOL, task.await) {
                Ok(content) => profile_name_of(&content),
                Err(error) => {
                    log::warn!("nostr panel: {error}");
                    None
                }
            };
            // A tool error or a non-profile result is not a lookup: the
            // labelled click stays available for a retry.
            if let Some(name) = asked {
                this.update(cx, |this, cx| {
                    this.profile_lookups.insert(author, name);
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    // ── A labelled image fetch ──────────────────────────────────

    /// Ask the media server to fetch this note's image, through the
    /// governed invoker: the decoded image is tool-traced, not the
    /// viewer's own. The click's label names the host; the epoch keeps a
    /// slow answer from overwriting a newer click, and a failure is shown
    /// with the click still offered.
    fn fetch_image(&mut self, id: String, url: String, cx: &mut Context<Self>) {
        let epoch = self
            .image_requests
            .entry(id.clone())
            .or_insert_with(Request::new)
            .begin();
        cx.notify();
        let task = match invoke(MEDIA_SERVER, MEDIA_FETCH_IMAGE_TOOL, json!({ "url": url })) {
            Ok(task) => task,
            Err(error) => return self.apply_fetched_image(id, epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
            let result = async {
                let content = content_of(MEDIA_FETCH_IMAGE_TOOL, task.await)?;
                let path = content
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{MEDIA_FETCH_IMAGE_TOOL} returned no path"))?
                    .to_string();
                let bytes = std::fs::read(&path)
                    .map_err(|error| format!("read the fetched image {path}: {error}"))?;
                decode_fetched_image(&bytes)
            }
            .await;
            this.update(cx, |this, cx| {
                this.apply_fetched_image(id, epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// Record one fetch's outcome: the image is shown only when it answers
    /// the latest click, and a refusal is shown with the click still
    /// offered.
    fn apply_fetched_image(
        &mut self,
        id: String,
        epoch: u64,
        result: Result<Arc<RenderImage>, String>,
        cx: &mut Context<Self>,
    ) {
        let (image, error) = match result {
            Ok(image) => (Some(image), None),
            Err(error) => (None, Some(error)),
        };
        let Some(request) = self.image_requests.get_mut(&id) else {
            return;
        };
        if !request.finish(epoch, error) {
            return;
        }
        if let Some(image) = image {
            self.fetched_images.insert(id, image);
        }
        cx.notify();
    }

    // ── The local relay ─────────────────────────────────────────────

    /// Read the local relay's state and its admission decisions. Local.
    fn load_relay(&mut self, cx: &mut Context<Self>) {
        let epoch = self.relay_request.begin();
        cx.notify();
        let tasks = invoke(NOSTR_RELAY_SERVER, STATUS_TOOL, json!({})).and_then(|status| {
            invoke(NOSTR_RELAY_SERVER, LOG_TOOL, json!({ "limit": LOG_PAGE }))
                .map(|log| (status, log))
        });
        let (status, log) = match tasks {
            Ok(tasks) => tasks,
            Err(error) => return self.apply_relay(epoch, Err(error), cx),
        };
        cx.spawn(async move |this, cx| {
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
            this.update(cx, |this, cx| this.apply_relay(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    fn apply_relay(
        &mut self,
        epoch: u64,
        result: Result<RelayReading, String>,
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
        cx.notify();
    }

    // ── Tool results from the conversation below ────────────────────

    /// Show the events in the conversation's tool results. Each result is
    /// read once, in order, so the newest result is what is on screen and a
    /// redraw does not undo the operator's selection.
    pub fn ingest_thread(&mut self, thread: Entity<acp_thread::AcpThread>, cx: &mut Context<Self>) {
        if self.thread.as_ref().and_then(|weak| weak.upgrade()) != Some(thread.clone()) {
            self.processed_tool_results.clear();
        }
        self.thread = Some(thread.downgrade());
        let mut outputs = Vec::new();
        for entry in thread.read(cx).entries() {
            let AgentThreadEntry::ToolCall(call) = entry else {
                continue;
            };
            let Some(output) = call.raw_output.as_ref() else {
                continue; // still running
            };
            if self.processed_tool_results.insert(call.id.clone()) {
                outputs.push(output.clone());
            }
        }
        for output in outputs {
            self.ingest_tool_result(&output, cx);
        }
    }

    /// Show one tool result if it is a list of events or a conversation.
    pub fn ingest_tool_result(&mut self, output: &Value, cx: &mut Context<Self>) {
        let content = match output {
            Value::String(text) => match hkask_types::tool_response::parse_tool_response(text) {
                Some(content) => content,
                None => return,
            },
            value => hkask_types::tool_response::unwrap_tool_envelope(value.clone()),
        };
        if archive_changed(&content)
            && self
                .feed
                .as_ref()
                .is_none_or(|feed| feed.origin == FeedOrigin::Archive)
        {
            self.load_archive(cx);
        }
        match parse_surfaced(&content, FeedOrigin::Conversation) {
            Some(Ok(Surfaced::Feed(feed))) => {
                self.show_feed(feed, cx);
                self.tab = ViewerTab::Timeline;
                // The result on screen is the conversation's, whatever an
                // archive load still in flight returns.
                self.archive_request.begin();
                self.archive_request.state = Load::Idle;
            }
            Some(Ok(Surfaced::Conversation(conversation))) => {
                self.show_conversation(conversation);
                self.thread_request.begin();
                self.thread_request.state = Load::Idle;
            }
            Some(Err(error)) => log::warn!("nostr panel: {error}"),
            None => return,
        }
        cx.notify();
    }

    // ── Drawing ─────────────────────────────────────────────────────

    fn active_timeline_feed(&self) -> Option<&Feed> {
        if self.timeline_filter == TimelineFilter::Following
            && let Some(feed) = &self.following_feed
        {
            return Some(feed);
        }
        self.feed.as_ref()
    }

    fn selected_row(&self) -> Option<&Row> {
        let id = self.selected.as_ref()?;
        let rows = match self.tab {
            ViewerTab::Timeline => &self.active_timeline_feed()?.rows,
            ViewerTab::Notifications => &self.notifications_feed.as_ref()?.rows,
            ViewerTab::Conversation => &self.conversation.as_ref()?.rows,
            ViewerTab::Relay | ViewerTab::Messages => return None,
        };
        rows.iter().find(|row| &row.event.id == id)
    }

    fn tab_button(
        &self,
        tab: ViewerTab,
        label: SharedString,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.tab == tab;
        div()
            .id(SharedString::from(format!("nostr-viewer-tab-{tab:?}")))
            .px_2()
            .h_6()
            .min_w_0()
            .overflow_hidden()
            .rounded_sm()
            .map(|el| {
                if active {
                    el.bg(cx.theme().colors().element_active)
                } else {
                    el.hover(|el| el.bg(cx.theme().colors().element_hover))
                }
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .child(
                Label::new(label)
                    .size(LabelSize::Small)
                    .truncate()
                    .color(if active { Color::Default } else { Color::Muted }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.activate_tab(tab, cx)))
    }

    /// One named-feed button in the Timeline header. Every named feed
    /// reads the same page; the switch changes what is shown, never the
    /// order.
    fn filter_button(
        &self,
        id: &'static str,
        label: &'static str,
        filter: TimelineFilter,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.timeline_filter == filter;
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .px_2()
            .h_6()
            .min_w_0()
            .overflow_hidden()
            .rounded_sm()
            .map(|el| {
                if active {
                    el.bg(cx.theme().colors().element_active)
                } else {
                    el.hover(|el| el.bg(cx.theme().colors().element_hover))
                }
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .child(
                Label::new(label)
                    .size(LabelSize::Small)
                    .truncate()
                    .color(if active { Color::Default } else { Color::Muted }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.timeline_filter != filter {
                    this.timeline_filter = filter;
                    this.selected = None;
                }
                if filter == TimelineFilter::Following
                    && this.following_feed.is_none()
                    && !matches!(this.following_request.state, Load::Loading)
                {
                    this.load_following_feed(cx);
                }
                cx.notify();
            }))
    }

    fn line(text: impl Into<SharedString>, color: Color) -> impl IntoElement {
        // A plain text block that wraps at the pane's width.
        div()
            .w_full()
            .min_w_0()
            .text_xs()
            .child(Label::new(text.into()).size(LabelSize::XSmall).color(color))
    }

    fn request_line(request: &Request, loading: &'static str) -> Option<AnyElement> {
        match &request.state {
            Load::Idle => None,
            Load::Loading => Some(Self::line(loading, Color::Muted).into_any_element()),
            Load::Failed(error) => {
                Some(Self::line(display_text(error), Color::Error).into_any_element())
            }
        }
    }

    /// A request's failure line alone: the reactions read is part of the
    /// page load, so its loading state is the archive's, and only its
    /// failure is new information.
    fn failure_line(request: &Request) -> Option<AnyElement> {
        match &request.state {
            Load::Failed(error) => {
                Some(Self::line(display_text(error), Color::Error).into_any_element())
            }
            _ => None,
        }
    }

    fn remarks(remarks: &[Remark]) -> impl IntoElement + use<> {
        v_flex()
            .w_full()
            .min_w_0()
            .children(remarks.iter().map(|remark| {
                Self::line(
                    remark.text.clone(),
                    if remark.caveat {
                        Color::Warning
                    } else {
                        Color::Hint
                    },
                )
            }))
    }

    fn empty(title: &'static str, hint: &'static str) -> impl IntoElement {
        v_flex()
            .size_full()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_1()
            .px_3()
            .child(Label::new(title).size(LabelSize::Small).color(Color::Muted))
            .child(Self::line(hint, Color::Hint))
    }

    /// A reply's context chip: opens the thread of the note this event
    /// answers, through the same labelled click as the detail pane's
    /// Read conversation button.
    fn reply_chip(
        &self,
        ix: usize,
        reply: &ReplyReference,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let note = reply.note.clone();
        div()
            .id(SharedString::from(format!("nostr-row-{ix}-reply")))
            .min_w_0()
            .flex_shrink(1.)
            .rounded_sm()
            .px_1()
            .cursor_pointer()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(display_text(&reply.label))
                    .size(LabelSize::XSmall)
                    .color(Color::Hint)
                    .truncate(),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.read_conversation(note.clone(), cx);
            }))
    }

    /// One event as a card: the author's face and name, what the event
    /// is, the claimed time, the text it carries, and — for a reply — the
    /// note it answers, clickable to read that thread. Every line
    /// truncates: a long note must not push other notes off screen (the
    /// narrow-pane ruling of 2026-11-13).
    fn row(
        &self,
        ix: usize,
        row: &Row,
        indent: bool,
        names: &BTreeMap<String, AuthorProfile>,
        offset: UtcOffset,
        now: OffsetDateTime,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = row.event.id.clone();
        let selected = self.selected.as_ref() == Some(&id);
        let profile = names.get(row.event.author.as_str());
        let author_text = profile
            .and_then(|profile| profile.name.clone())
            .unwrap_or_else(|| short_hex(&row.event.author));
        let nip05 = profile.and_then(|profile| profile.nip05.clone());
        let kind_text = what_it_is(&row.event);
        let supporting = supporting_line(&row.event);
        let reply = reply_reference(&row.event, names);
        let notification = row.label.clone();
        let has_context =
            reply.is_some() || row.parent_missing || row.is_focus || notification.is_some();
        let trailer = if row.event.served_by.is_empty() {
            format!("{} · the local archive", short_hex(&row.event.id))
        } else {
            format!(
                "{} · {}",
                short_hex(&row.event.id),
                row.event.served_by.join(", ")
            )
        };
        let show_inline_actions =
            row.event.kind == 1 || row.event.kind == 1111 || row.event.kind == 30023;
        let inline_event_id = row.event.id.clone();
        let inline_author = row.event.author.clone();
        let inline_reply_target = if show_inline_actions {
            Some(ReplyContext::from_event(
                &row.event,
                profile
                    .and_then(|p| p.name.as_deref())
                    .map(|n| format!("@{}", display_text(n)))
                    .unwrap_or_else(|| short_hex(&row.event.author)),
            ))
        } else {
            None
        };
        let inline_liked = self
            .own_reactions
            .contains_key(&(row.event.id.clone(), "👍".to_string()));
        let inline_disliked = self
            .own_reactions
            .contains_key(&(row.event.id.clone(), "👎".to_string()));
        let inline_zapping = self
            .zap
            .as_ref()
            .is_some_and(|z| z.target_event_id == row.event.id);
        let inline_busy = self.reaction_in_flight.as_deref() == Some(row.event.id.as_str());
        let inline_like_count = self
            .reactions
            .get(&row.event.id)
            .and_then(|m| m.get("👍"))
            .copied()
            .unwrap_or(0);
        let inline_zap_sats = self
            .zap_totals
            .get(&row.event.id)
            .map(|(_, sats)| *sats)
            .unwrap_or(0);
        let depth = if indent {
            row.depth.min(MAX_INDENT_LEVELS)
        } else {
            0
        };
        div()
            .id(SharedString::from(format!("nostr-row-{ix}")))
            .debug_selector(move || format!("nostr-row-{ix}"))
            .w_full()
            .cursor_pointer()
            .min_w_0()
            .pl(px(8. + 12. * depth as f32))
            .pr_2()
            .py_1()
            .rounded_sm()
            .map(|el| {
                if selected {
                    el.bg(cx.theme().colors().element_selected)
                } else {
                    el.hover(|el| el.bg(cx.theme().colors().element_hover))
                }
            })
            .cursor_pointer()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .child(avatar(&row.event.author))
                    .child(
                        div().min_w_0().flex_shrink(1.).child(
                            Label::new(display_text(&author_text))
                                .size(LabelSize::XSmall)
                                .color(Color::Default)
                                .truncate(),
                        ),
                    )
                    .when_some(nip05, |el, nip05| {
                        el.child(
                            div()
                                .min_w_0()
                                .flex_shrink(1.)
                                .rounded_full()
                                .px_1()
                                .border_1()
                                .border_color(cx.theme().colors().border_variant)
                                .child(
                                    Label::new(display_text(&nip05))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Hint)
                                        .truncate(),
                                ),
                        )
                    })
                    .child(
                        div().min_w_0().flex_shrink(1.).child(
                            Label::new(display_text(&kind_text))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                    )
                    .child(
                        div().ml_auto().flex_shrink_0().child(
                            Label::new(relative_time_text(row.event.created_at, now, offset))
                                .size(LabelSize::XSmall)
                                .color(Color::Hint),
                        ),
                    ),
            )
            .when(!supporting.is_empty(), |el| {
                el.child(
                    Label::new(display_text(&supporting))
                        .size(LabelSize::Small)
                        .truncate(),
                )
            })
            .when_some(self.media_attachments.get(&id).cloned(), |el, meta| {
                let preview = self.media_previews.get(&id).cloned();
                el.child(self.media_block(ix, &id, preview, meta, cx))
            })
            .when(has_context, |el| {
                el.child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .when_some(reply, |el, reply| el.child(self.reply_chip(ix, &reply, cx)))
                        .when(row.parent_missing, |el| {
                            el.child(
                                Label::new("parent not found")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Warning)
                                    .truncate(),
                            )
                        })
                        .when(row.is_focus, |el| {
                            el.child(
                                Label::new("asked for")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Hint)
                                    .truncate(),
                            )
                        })
                        .when_some(notification, |el, notification| {
                            el.child(
                                Label::new(display_text(&notification))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Default)
                                    .truncate(),
                            )
                        }),
                )
            })
            .when_some(
                self.reactions.get(&id).filter(|counts| !counts.is_empty()),
                |el, counts| {
                    el.child(
                        Label::new(display_text(&reactions_text(
                            counts,
                            self.reactions_clipped,
                        )))
                        .size(LabelSize::XSmall)
                        .color(Color::Hint)
                        .truncate(),
                    )
                },
            )
            .when_some(self.zap_totals.get(&id).copied(), |el, (count, sats)| {
                el.child(
                    Label::new(display_text(&zap_totals_text(count, sats)))
                        .size(LabelSize::XSmall)
                        .color(Color::Accent)
                        .truncate(),
                )
            })
            .when(show_inline_actions, |el| {
                let like_id = inline_event_id.clone();
                let like_author = inline_author.clone();
                let dislike_id = inline_event_id.clone();
                let dislike_author = inline_author.clone();
                let repost_id = inline_event_id.clone();
                let repost_author = inline_author.clone();
                let repost_kind = row.event.kind;
                let zap_id = inline_event_id.clone();
                let zap_author = inline_author.clone();
                let zap_label = inline_reply_target
                    .as_ref()
                    .map(|t| t.target_label.clone())
                    .unwrap_or_else(|| short_hex(&inline_author));
                let reply_target = inline_reply_target.clone();
                let like_text = if inline_like_count > 0 {
                    format!("👍 {inline_like_count}")
                } else {
                    "👍".to_string()
                };
                let zap_text = if inline_zap_sats > 0 {
                    format!("⚡ {inline_zap_sats}")
                } else {
                    "⚡ Zap".to_string()
                };
                el.child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .pt_0p5()
                        .flex_wrap()
                        .child(
                            ui::Button::new(("nostr-row-like", ix), like_text)
                                .label_size(LabelSize::XSmall)
                                .toggle_state(inline_liked)
                                .disabled(inline_busy)
                                .tooltip(ui::Tooltip::text(if inline_liked {
                                    "Remove your NIP-25 like (kind:5 deletion)"
                                } else {
                                    "Like (NIP-25 kind:7 '+')"
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.send_reaction(
                                        like_id.clone(),
                                        like_author.clone(),
                                        "+".to_string(),
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            ui::Button::new(("nostr-row-dislike", ix), "👎")
                                .label_size(LabelSize::XSmall)
                                .toggle_state(inline_disliked)
                                .disabled(inline_busy)
                                .tooltip(ui::Tooltip::text(if inline_disliked {
                                    "Remove your NIP-25 dislike (kind:5 deletion)"
                                } else {
                                    "Dislike (NIP-25 kind:7 '-')"
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.send_reaction(
                                        dislike_id.clone(),
                                        dislike_author.clone(),
                                        "-".to_string(),
                                        cx,
                                    );
                                })),
                        )
                        .when_some(reply_target, |row_bar, target| {
                            row_bar.child(
                                ui::Button::new(("nostr-row-reply", ix), "Reply")
                                    .label_size(LabelSize::XSmall)
                                    .tooltip(ui::Tooltip::text(format!(
                                        "Reply via {} ({})",
                                        target.nip_label(),
                                        if target.root_kind == 1 {
                                            "kind:1"
                                        } else {
                                            "kind:1111"
                                        }
                                    )))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_reply_compose(target.clone(), window, cx);
                                    })),
                            )
                        })
                        .child(
                            ui::Button::new(("nostr-row-repost", ix), "🔁")
                                .label_size(LabelSize::XSmall)
                                .disabled(inline_busy)
                                .tooltip(ui::Tooltip::text("Repost (NIP-18 kind:6 / kind:16)"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.send_repost(
                                        repost_id.clone(),
                                        repost_author.clone(),
                                        repost_kind,
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            ui::Button::new(("nostr-row-zap", ix), zap_text)
                                .label_size(LabelSize::XSmall)
                                .toggle_state(inline_zapping)
                                .tooltip(ui::Tooltip::text(
                                    "Zap via NIP-57 Lightning or NIP-61 Cashu Nutzap",
                                ))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.selected = Some(zap_id.clone());
                                    this.detail_meta_expanded = false;
                                    this.toggle_zap_strip(
                                        zap_id.clone(),
                                        zap_author.clone(),
                                        zap_label.clone(),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                )
            })
            .when(selected, |el| {
                el.child(
                    Label::new(display_text(&trailer))
                        .size(LabelSize::XSmall)
                        .color(Color::Hint)
                        .truncate(),
                )
            })
            .on_click(cx.listener({
                let is_article = row.event.kind == 30023;
                let article_content = if is_article {
                    Some(row.event.content.clone())
                } else {
                    None
                };
                move |this, _, _, cx| {
                    if let Some(content) = &article_content {
                        this.ensure_article_markdown(&id, content, cx);
                    }
                    this.selected = Some(id.clone());
                    this.detail_meta_expanded = false;
                    this.detail_scroll_handle
                        .set_offset(gpui::point(px(0.), px(0.)));
                    cx.notify();
                }
            }))
    }

    /// A note's media: an embedded `MediaWidget` for video, a fetched image,
    /// or a blurhash/host placeholder with its labelled fetch click. Every
    /// line truncates (the narrow-pane ruling).
    fn media_block(
        &self,
        ix: usize,
        id: &str,
        preview: Option<Arc<RenderImage>>,
        meta: MediaAttachment,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let url = meta.url;
        let host = host_of(&url).to_owned();
        let id = id.to_string();
        if meta.kind == MediaAttachmentKind::Video {
            if let Some(widget) = self.video_widgets.get(&id) {
                return div()
                    .id(SharedString::from(format!("nostr-video-{ix}")))
                    .debug_selector(move || format!("nostr-video-{ix}"))
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .debug_selector(move || format!("nostr-video-box-{ix}"))
                            .w_full()
                            .h(px(FETCHED_IMAGE_HEIGHT as f32))
                            .flex()
                            .min_w_0()
                            .overflow_hidden()
                            .rounded_sm()
                            .child(widget.clone()),
                    )
                    .child(
                        Label::new(format!("video from {host}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Hint)
                            .truncate(),
                    )
                    .into_any_element();
            }
            return div()
                .id(SharedString::from(format!("nostr-media-{ix}")))
                .debug_selector(move || format!("nostr-media-{ix}"))
                .child(
                    div()
                        .id(SharedString::from(format!("nostr-load-video-{ix}")))
                        .debug_selector(move || format!("nostr-load-video-{ix}"))
                        .min_w_0()
                        .overflow_hidden()
                        .rounded_sm()
                        .px_1()
                        .h_6()
                        .flex()
                        .items_center()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .cursor_pointer()
                        .child(
                            Label::new(format!("Load video from {host}"))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.load_video(id.clone(), url.clone(), cx);
                        })),
                )
                .into_any_element();
        }
        let request = self.image_requests.get(&id);
        let loading = request.is_some_and(|request| matches!(request.state, Load::Loading));
        if let Some(fetched) = self.fetched_images.get(&id) {
            return div()
                .id(SharedString::from(format!("nostr-fetched-image-{ix}")))
                .debug_selector(move || format!("nostr-fetched-image-{ix}"))
                .child(
                    img(ImageSource::Render(fetched.clone()))
                        .max_h(px(FETCHED_IMAGE_HEIGHT as f32))
                        .w_full()
                        .object_fit(ObjectFit::ScaleDown)
                        .rounded_sm(),
                )
                .child(
                    Label::new(format!("image fetched from {host}"))
                        .size(LabelSize::XSmall)
                        .color(Color::Hint)
                        .truncate(),
                )
                .into_any_element();
        }
        let has_preview = preview.is_some();
        div()
            .id(SharedString::from(format!("nostr-media-{ix}")))
            .debug_selector(move || format!("nostr-media-{ix}"))
            .when_some(preview, |el, preview| {
                el.child(
                    img(ImageSource::Render(preview))
                        .w_full()
                        .h(px(MEDIA_PREVIEW_HEIGHT as f32))
                        .object_fit(ObjectFit::Cover)
                        .rounded_sm(),
                )
            })
            .when(has_preview, |el| {
                el.child(
                    Label::new("blurhash placeholder — the image is not fetched")
                        .size(LabelSize::XSmall)
                        .color(Color::Hint)
                        .truncate(),
                )
            })
            .when(!loading, |el| {
                el.child(
                    div()
                        .id(SharedString::from(format!("nostr-load-image-{ix}")))
                        .debug_selector(move || format!("nostr-load-image-{ix}"))
                        .min_w_0()
                        .overflow_hidden()
                        .rounded_sm()
                        .px_1()
                        .h_6()
                        .flex()
                        .items_center()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .cursor_pointer()
                        .child(
                            Label::new(format!("Load the image from {host}"))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.fetch_image(id.clone(), url.clone(), cx);
                        })),
                )
            })
            .when_some(
                request.and_then(|request| Self::request_line(request, "fetching the image…")),
                |el, line| el.child(line),
            )
            .into_any_element()
    }

    fn rows(
        &self,
        rows: &[Row],
        indent: bool,
        scroll_handle: &ScrollHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let names = self.names_of(rows);
        let offset = local_offset();
        let now = OffsetDateTime::now_utc();
        v_flex()
            .id("nostr-viewer-rows")
            .debug_selector(|| "nostr-viewer-rows".into())
            .w_full()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(scroll_handle)
            .custom_scrollbars(
                ui::Scrollbars::always_visible(ui::ScrollAxes::Vertical)
                    .tracked_scroll_handle(scroll_handle),
                window,
                cx,
            )
            .gap_0p5()
            .p_1()
            .pr_3()
            .children(
                rows.iter()
                    .enumerate()
                    .map(|(ix, row)| self.row(ix, row, indent, &names, offset, now, cx)),
            )
    }

    /// One collapsed record group: the name, the exact count, the group's
    /// time and the expand toggle. The count is exact and nothing is
    /// dropped either way; a re-collapse clears a selection that is no
    /// longer on screen.
    #[allow(clippy::too_many_arguments)]
    fn collapsed_row(
        &self,
        ix: usize,
        author: &str,
        name: Option<&str>,
        minute: i64,
        count: usize,
        newest: u64,
        expanded: bool,
        offset: UtcOffset,
        now: OffsetDateTime,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let key = (author.to_string(), minute);
        let author_text = name
            .map(str::to_string)
            .unwrap_or_else(|| short_hex(author));
        let label = format!(
            "{} · published {count} client records {}",
            author_text,
            if expanded { "▾" } else { "▸" },
        );
        div()
            .id(SharedString::from(format!("nostr-collapsed-{ix}")))
            .debug_selector(move || format!("nostr-collapsed-{ix}"))
            .w_full()
            .min_w_0()
            .pl(px(8.))
            .pr_2()
            .py_1()
            .rounded_sm()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .cursor_pointer()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .child(avatar(author))
                    .child(
                        div().min_w_0().flex_shrink(1.).child(
                            Label::new(label)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                    )
                    .child(
                        div().ml_auto().flex_shrink_0().child(
                            Label::new(relative_time_text(newest, now, offset))
                                .size(LabelSize::XSmall)
                                .color(Color::Hint),
                        ),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let was_expanded = this.expanded_records.remove(&key);
                if !was_expanded {
                    this.expanded_records.insert(key.clone());
                } else if let Some(row) = this.selected_row()
                    && kind_class(row.event.kind) == KindClass::Record
                {
                    // A record that is no longer shown is no longer
                    // selected; the detail pane follows the screen.
                    this.selected = None;
                }
                cx.notify();
            }))
    }

    /// A day divider: the local date of the day's newest event.
    fn divider(ix: usize, at: u64, now: OffsetDateTime, offset: UtcOffset) -> impl IntoElement {
        let text = day_text(at, now, offset);
        div()
            .id(SharedString::from(format!("nostr-divider-{ix}")))
            .debug_selector(move || format!("nostr-divider-{ix}"))
            .w_full()
            .min_w_0()
            .px_2()
            .py_1()
            .child(
                Label::new(format!("── {text} ──"))
                    .size(LabelSize::XSmall)
                    .color(Color::Hint)
                    .truncate(),
            )
    }

    /// The selected event, whole: the fields a relay sent, and where the
    /// event came from.
    fn detail(
        &self,
        row: &Row,
        names: &BTreeMap<String, AuthorProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let event = &row.event;
        let source = if event.served_by.is_empty() {
            "Source: the local relay's archive, which verified the id and signature when it stored the event.".to_string()
        } else {
            format!("Returned and verified from: {}", event.served_by.join(", "))
        };
        let mut tag_lines: Vec<String> = event
            .tags
            .iter()
            .take(DETAIL_TAGS)
            .map(|tag| tag.join("  "))
            .collect();
        let unlisted = event.tags.len().saturating_sub(DETAIL_TAGS);
        if unlisted > 0 || event.tags_truncated {
            tag_lines.push(if event.tags_truncated {
                format!("… {unlisted} more shown by the tool, and more it left out")
            } else {
                format!("… {unlisted} more")
            });
        }
        let event_id = event.id.clone();
        let links = extract_links(&event.content);
        let detail_media = self
            .media_attachments
            .get(&event.id)
            .cloned()
            .or_else(|| media_of(event));
        let name = names
            .get(event.author.as_str())
            .and_then(|profile| profile.name.as_deref());
        v_flex()
            .id("nostr-viewer-detail")
            .debug_selector(|| "nostr-viewer-detail".into())
            .w_full()
            .min_w_0()
            .min_h_0()
            .h(DefiniteLength::Fraction(DETAIL_FRACTION))
            .flex_shrink_0()
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll_handle)
            .custom_scrollbars(
                ui::Scrollbars::always_visible(ui::ScrollAxes::Vertical)
                    .tracked_scroll_handle(&self.detail_scroll_handle),
                window,
                cx,
            )
            .border_t_1()
            .border_color(cx.theme().colors().border_variant)
            .p_2()
            .pr_3()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .items_center()
                    .child(
                        div().min_w_0().flex_1().child(
                            Label::new(match name {
                                Some(name) => format!(
                                    "{} · {}",
                                    display_text(name),
                                    utc_text(event.created_at)
                                ),
                                None => format!(
                                    "{} · {}",
                                    short_hex(&event.author),
                                    utc_text(event.created_at)
                                ),
                            })
                            .size(LabelSize::Small)
                            .color(Color::Default)
                            .truncate(),
                        ),
                    )
                    .child(
                        ui::Button::new("nostr-detail-copy-text", "Copy text")
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text("Copy event content to clipboard"))
                            .on_click(cx.listener({
                                let content = event.content.clone();
                                let id = event.id.clone();
                                move |this, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        content.clone(),
                                    ));
                                    this.reaction_feedback =
                                        Some((id.clone(), true, "Copied text to clipboard".into()));
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        ui::Button::new("nostr-detail-copy-id", "Copy ID")
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text("Copy event hex ID to clipboard"))
                            .on_click(cx.listener({
                                let id = event.id.clone();
                                move |this, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        id.clone(),
                                    ));
                                    this.reaction_feedback = Some((
                                        id.clone(),
                                        true,
                                        "Copied event ID to clipboard".into(),
                                    ));
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        ui::Button::new(
                            "nostr-detail-meta-toggle",
                            if self.detail_meta_expanded {
                                "Hide details"
                            } else {
                                "Details"
                            },
                        )
                        .label_size(LabelSize::XSmall)
                        .toggle_state(self.detail_meta_expanded)
                        .tooltip(ui::Tooltip::text(
                            "Show or hide raw event id, author hex, kind, and relay provenance",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.detail_meta_expanded = !this.detail_meta_expanded;
                            cx.notify();
                        })),
                    )
                    .child(
                        ui::Button::new("nostr-detail-close", "Close")
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected = None;
                                this.detail_meta_expanded = false;
                                cx.notify();
                            })),
                    ),
            )
            // Selected-note feedback & support strip: Quick Reactions (NIP-25),
            // Comment (NIP-10 / NIP-22), Zap (NIP-57 / NIP-61), and Delete (NIP-09).
            .when(
                event.kind == 1 || event.kind == 1111 || event.kind == 30023,
                |el| {
                    let reacting = self.reaction_in_flight.as_deref() == Some(event.id.as_str());
                    let event_counts = self.reactions.get(event.id.as_str());
                    let reply_target = ReplyContext::from_event(
                        event,
                        name.map(|n| format!("@{}", display_text(n)))
                            .unwrap_or_else(|| short_hex(&event.author)),
                    );

                    let repost_event_id = event.id.clone();
                    let repost_author = event.author.clone();
                    let repost_kind = event.kind;
                    let follow_author = event.author.clone();
                    let is_followed = self.is_followed_author(&event.author);
                    let zap_event_id = event.id.clone();
                    let zap_author = event.author.clone();
                    let zap_label = reply_target.target_label.clone();
                    let zap_open = self
                        .zap
                        .as_ref()
                        .is_some_and(|z| z.target_event_id == event.id);
                    let my_pubkey_hex = self.my_pubkey_hex(cx);
                    let is_own_event = my_pubkey_hex
                        .as_deref()
                        .is_some_and(|mine| mine.eq_ignore_ascii_case(&event.author));
                    let can_follow = !is_own_event
                        && matches!(
                            self.follows,
                            Some(Follows::Authors(_) | Follows::NoList)
                        );
                    let delete_event_id = event.id.clone();
                    let delete_kind = event.kind;
                    let reaction_note = self
                        .reaction_feedback
                        .as_ref()
                        .filter(|(id, _, _)| id == &event.id)
                        .map(|(_, ok, msg)| (*ok, msg.clone()));

                    el.child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .py_1()
                            .min_w_0()
                            .flex_wrap()
                            .children(QUICK_REACTIONS.iter().enumerate().map(
                                |(r_ix, &(wire, emoji))| {
                                    let count = event_counts
                                        .and_then(|m| m.get(emoji))
                                        .copied()
                                        .unwrap_or(0);
                                    let own_active = self
                                        .own_reactions
                                        .contains_key(&(event.id.clone(), emoji.to_string()));
                                    let label = if count > 0 {
                                        format!("{emoji} {count}")
                                    } else {
                                        emoji.to_string()
                                    };
                                    let r_event_id = event.id.clone();
                                    let r_author = event.author.clone();
                                    let r_wire = wire.to_string();
                                    ui::Button::new(("nostr-detail-react", r_ix), label)
                                        .label_size(LabelSize::XSmall)
                                        .toggle_state(own_active)
                                        .disabled(reacting)
                                        .tooltip(ui::Tooltip::text(if own_active {
                                            format!(
                                                "Remove your {emoji} reaction (NIP-09 kind:5 deletion)"
                                            )
                                        } else {
                                            format!(
                                                "React {emoji} (NIP-25 kind:7 '{wire}')"
                                            )
                                        }))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.send_reaction(
                                                r_event_id.clone(),
                                                r_author.clone(),
                                                r_wire.clone(),
                                                cx,
                                            );
                                        }))
                                },
                            ))
                            .child(
                                ui::Button::new("nostr-detail-comment", "Comment")
                                    .label_size(LabelSize::XSmall)
                                    .tooltip(ui::Tooltip::text(
                                        "Reply via NIP-10 (kind:1) or NIP-22 (kind:1111) (https://nostr-nips.com/nip-10)",
                                    ))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_reply_compose(reply_target.clone(), window, cx);
                                    })),
                            )
                            .child(
                                ui::Button::new("nostr-detail-repost", "🔁 Repost")
                                    .label_size(LabelSize::XSmall)
                                    .disabled(reacting)
                                    .tooltip(ui::Tooltip::text(
                                        "Repost via NIP-18 (kind:6 for notes, kind:16 otherwise; https://nostr-nips.com/nip-18)",
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send_repost(
                                            repost_event_id.clone(),
                                            repost_author.clone(),
                                            repost_kind,
                                            cx,
                                        );
                                    })),
                            )
                            .when(can_follow, |row| {
                                row.child(
                                    ui::Button::new(
                                        "nostr-detail-follow",
                                        if is_followed { "Following ✓" } else { "Follow" },
                                    )
                                    .label_size(LabelSize::XSmall)
                                    .toggle_state(is_followed)
                                    .disabled(reacting)
                                    .tooltip(ui::Tooltip::text(if is_followed {
                                        "Unfollow this author (NIP-02 kind:3; always asks for confirmation)"
                                    } else {
                                        "Follow this author (NIP-02 kind:3; always asks for confirmation)"
                                    }))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_follow(follow_author.clone(), cx);
                                    })),
                                )
                            })
                            .child(
                                ui::Button::new("nostr-detail-zap", "⚡ Zap")
                                    .label_size(LabelSize::XSmall)
                                    .toggle_state(zap_open)
                                    .tooltip(ui::Tooltip::text(
                                        "Zap this post via NIP-57 Lightning or NIP-61 Cashu Nutzap",
                                    ))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.toggle_zap_strip(
                                            zap_event_id.clone(),
                                            zap_author.clone(),
                                            zap_label.clone(),
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .when(is_own_event, |row| {
                                row.child(
                                    ui::Button::new("nostr-detail-delete", "Delete")
                                        .label_size(LabelSize::XSmall)
                                        .disabled(reacting)
                                        .tooltip(ui::Tooltip::text(
                                            "Publish a NIP-09 kind:5 deletion request (always asks for confirmation; irreversible)",
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.request_delete(
                                                delete_event_id.clone(),
                                                delete_kind,
                                                cx,
                                            );
                                        })),
                                )
                            }),
                    )
                    .when_some(reaction_note, |el, (ok, msg)| {
                        el.child(Self::line(
                            msg,
                            if ok { Color::Success } else { Color::Error },
                        ))
                    })
                    .when(zap_open, |el| {
                        if let Some(zap) = self.zap.clone() {
                            el.child(self.render_zap_strip(&zap, cx))
                        } else {
                            el
                        }
                    })
                },
            )
            // Collapsible raw protocol metadata (id, author hex, kind, source).
            .when(self.detail_meta_expanded, |el| {
                el.child(Self::line(format!("id  {}", event.id), Color::Muted))
                    .child(Self::line(
                        match name {
                            Some(name) => format!("author {} · {}", event.author, display_text(name)),
                            None => format!("author {}", event.author),
                        },
                        Color::Muted,
                    ))
                    .child(Self::line(
                        match kind_label(event.kind) {
                            Some((label, spec)) => format!(
                                "kind {} · {} ({}) · {} as the author claims ({})",
                                event.kind,
                                label,
                                spec,
                                utc_text(event.created_at),
                                event.created_at
                            ),
                            None => format!(
                                "kind {} ({}) · {} as the author claims ({})",
                                event.kind,
                                kind_range_name(event.kind),
                                utc_text(event.created_at),
                                event.created_at
                            ),
                        },
                        Color::Muted,
                    ))
                    .child(Self::line(display_text(&source), Color::Hint))
            })
            // Names come from the page's kind 0; an author with none there
            // can be asked for, on a labelled click that says what it
            // contacts. Once asked, the click is not offered again.
            .when(
                name.is_none() && !self.profile_lookups.contains_key(&event.author),
                |el| {
                    let author = event.author.clone();
                    el.child(div().child(
                        ui::Button::new("nostr-look-up-profile", "Look up profile")
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text(
                                "Asks the local relay and the seed relays for this author's profile",
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.look_up_profile(author.clone(), cx);
                            })),
                    ))
                },
            )
            .when(
                matches!(self.profile_lookups.get(&event.author), Some(None)),
                |el| {
                    el.child(Self::line(
                        "Profile asked: this author published no readable name",
                        Color::Hint,
                    ))
                },
            )
            // Only a note or a comment has a conversation, and only the
            // client server can read one.
            .when(event.kind == 1 || event.kind == 1111, |el| {
                el.child(
                    div().child(
                        ui::Button::new("nostr-read-conversation", "Read conversation")
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text(
                                "Asks the local relay and the seed relays for this note's conversation",
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.read_conversation(event_id.clone(), cx);
                            })),
                    ),
                )
            })
            .when(!tag_lines.is_empty(), |el| {
                el.child(Self::line("Tags (text the author wrote)", Color::Hint))
                    .children(
                        tag_lines
                            .into_iter()
                            .map(|tag| Self::line(display_text(&tag), Color::Default)),
                    )
            })
            .child(Self::line(
                if event.content_truncated {
                    "Content (text the author wrote; cut by the tool)"
                } else {
                    "Content (text the author wrote)"
                },
                Color::Hint,
            ))
            .child(
                div()
                    .debug_selector(|| "nostr-viewer-detail-content".into())
                    .w_full()
                    .min_w_0()
                    .text_sm()
                    .child(
                        if event.kind == 30023
                            && let Some(md) = self.article_markdowns.get(&event.id)
                        {
                            MarkdownElement::new(
                                md.clone(),
                                MarkdownStyle::themed(MarkdownFont::Agent, window, cx),
                            )
                            .on_url_click(|url, _, cx| {
                                if let Some(safe) = normalize_http_url(&url) {
                                    cx.open_url(&safe);
                                }
                            })
                            .into_any_element()
                        } else {
                            display_text(&event.content).into_any_element()
                        },
                    ),
            )
            .when_some(detail_media, |el, meta| {
                let preview = self.media_previews.get(&event.id).cloned();
                el.child(self.media_block(usize::MAX, &event.id, preview, meta, cx))
            })
            .when(!links.is_empty(), |el| {
                let total_links = links.len();
                let shown_links = links.into_iter().take(12).enumerate();
                el.child(Self::line("Links (open on click)", Color::Hint))
                    .children(shown_links.map(|(link_ix, link)| {
                        let url = link.url.clone();
                        let open_url = link.url.clone();
                        let fetch_url = link.url.clone();
                        let is_video = is_platform_video_url(&url);
                        let video_widget = self.link_video_widgets.get(&url).cloned();
                        let video_req = self.link_video_requests.get(&url);
                        let video_loading =
                            video_req.is_some_and(|req| matches!(req.state, Load::Loading));
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "nostr-detail-link-{link_ix}"
                                            )))
                                            .min_w_0()
                                            .flex_1()
                                            .overflow_hidden()
                                            .rounded_sm()
                                            .px_1()
                                            .h_6()
                                            .flex()
                                            .items_center()
                                            .hover(|el| el.bg(cx.theme().colors().element_hover))
                                            .cursor_pointer()
                                            .child(
                                                Label::new(format!(
                                                    "{} ({})",
                                                    display_text(&link.label),
                                                    link.host
                                                ))
                                                .size(LabelSize::XSmall)
                                                .color(Color::Accent)
                                                .truncate(),
                                            )
                                            .on_click(move |_, _, cx| {
                                                cx.open_url(&open_url);
                                            }),
                                    )
                                    .when(is_video && video_widget.is_none() && !video_loading, |el| {
                                        el.child(
                                            ui::Button::new(
                                                SharedString::from(format!(
                                                    "nostr-fetch-video-link-{link_ix}"
                                                )),
                                                "Fetch & play",
                                            )
                                            .label_size(LabelSize::XSmall)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.fetch_platform_video(fetch_url.clone(), cx);
                                            })),
                                        )
                                    }),
                            )
                            .when_some(
                                video_req.and_then(|req| {
                                    Self::request_line(req, "Fetching video via yt-dlp…")
                                }),
                                |el, line| el.child(line),
                            )
                            .when_some(video_widget, |el, widget| {
                                el.child(
                                    div()
                                        .w_full()
                                        .h(px(FETCHED_IMAGE_HEIGHT as f32))
                                        .flex()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .rounded_sm()
                                        .child(widget),
                                )
                            })
                    }))
                    .when(total_links > 12, |el| {
                        el.child(Self::line(
                            format!("… {} more links", total_links - 12),
                            Color::Hint,
                        ))
                    })
            })
    }

    fn render_timeline(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(feed) = self.active_timeline_feed() else {
            return v_flex()
                .size_full()
                .min_h_0()
                .overflow_hidden()
                .children(Self::request_line(
                    &self.archive_request,
                    "Reading the local archive…",
                ))
                .when(self.timeline_filter == TimelineFilter::Following, |el| {
                    el.children(Self::request_line(
                        &self.following_request,
                        "Fetching posts from followed authors on seed relays…",
                    ))
                })
                .child(Self::empty(
                    "No events loaded",
                    "The local relay's archive has not been read yet.",
                ))
                .into_any_element();
        };
        let names = self.names_of(&feed.rows);
        let offset = local_offset();
        let lines = timeline_lines(
            &feed.rows,
            &names,
            self.timeline_filter,
            self.follows.as_ref(),
            &self.expanded_records,
            offset,
        );
        let heading = format!("{} · {} events, newest first", feed.name, feed.rows.len());
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_1()
                            .flex_wrap()
                            .child(
                                div().min_w_0().flex_shrink(1.).child(
                                    Label::new(heading)
                                        .size(LabelSize::Small)
                                        .color(Color::Default)
                                        .truncate(),
                                ),
                            )
                            .child(
                                h_flex()
                                    .ml_auto()
                                    .min_w_0()
                                    .flex_wrap()
                                    .justify_end()
                                    .gap_1()
                                    .child(
                                        ui::Button::new(
                                            "nostr-timeline-new-note",
                                            if self.composer.has_draft() && !self.composer.is_open()
                                            {
                                                "+ Resume Draft"
                                            } else {
                                                "+ New Note"
                                            },
                                        )
                                        .style(ui::ButtonStyle::Filled)
                                        .label_size(LabelSize::XSmall)
                                        .tooltip(ui::Tooltip::text(
                                            "Compose a public Nostr kind:1 note (Ctrl/Cmd+Shift+N)",
                                        ))
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.toggle_compose(window, cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        ui::Button::new("nostr-timeline-top", "Top")
                                            .label_size(LabelSize::XSmall)
                                            .tooltip(ui::Tooltip::text(
                                                "Scroll timeline back to top",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.timeline_scroll_handle
                                                    .set_offset(gpui::point(px(0.), px(0.)));
                                                cx.notify();
                                            })),
                                    )
                                    .child(self.filter_button(
                                        "nostr-filter-reading",
                                        "For reading",
                                        TimelineFilter::ForReading,
                                        cx,
                                    ))
                                    .child(self.filter_button(
                                        "nostr-filter-following",
                                        "Following",
                                        TimelineFilter::Following,
                                        cx,
                                    ))
                                    .child(self.filter_button(
                                        "nostr-filter-threads",
                                        "Threads",
                                        TimelineFilter::Threads,
                                        cx,
                                    ))
                                    .child(self.filter_button(
                                        "nostr-filter-replies",
                                        "Replies",
                                        TimelineFilter::Replies,
                                        cx,
                                    ))
                                    .child(self.filter_button(
                                        "nostr-filter-everything",
                                        "Everything",
                                        TimelineFilter::Everything,
                                        cx,
                                    ))
                                    .child(
                                        ui::Button::new("nostr-timeline-refresh", "Refresh")
                                            .label_size(LabelSize::XSmall)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if this.timeline_filter == TimelineFilter::Following
                                                {
                                                    if !matches!(
                                                        this.follows,
                                                        Some(Follows::Authors(_))
                                                    ) {
                                                        this.load_follows(cx);
                                                    } else {
                                                        this.load_following_feed(cx);
                                                    }
                                                } else {
                                                    this.load_archive(cx);
                                                }
                                            })),
                                    ),
                            ),
                    )
                    .child(Self::remarks(&feed.remarks))
                    .children(Self::request_line(
                        &self.archive_request,
                        "Reading the local archive…",
                    ))
                    .when(self.timeline_filter == TimelineFilter::Following, |el| {
                        el.children(Self::request_line(
                            &self.following_request,
                            "Fetching posts from followed authors on seed relays…",
                        ))
                    })
                    .children(Self::failure_line(&self.reactions_request))
                    .children(Self::failure_line(&self.follows_request)),
            )
            .child(if lines.is_empty() {
                let (title, hint) = empty_timeline(
                    self.timeline_filter,
                    self.follows.as_ref(),
                    feed.rows.is_empty(),
                );
                Self::empty(title, hint).into_any_element()
            } else {
                let now = OffsetDateTime::now_utc();
                let line_elements: Vec<AnyElement> = lines
                    .iter()
                    .enumerate()
                    .map(|(ix, line)| match line {
                        TimelineLine::Divider { at } => {
                            Self::divider(ix, *at, now, offset).into_any_element()
                        }
                        TimelineLine::Entry { row } => self
                            .row(ix, row, false, &names, offset, now, cx)
                            .into_any_element(),
                        TimelineLine::Collapsed {
                            author,
                            name,
                            minute,
                            count,
                            newest,
                        } => {
                            let expanded =
                                self.expanded_records.contains(&(author.clone(), *minute));
                            self.collapsed_row(
                                ix,
                                author,
                                name.as_deref(),
                                *minute,
                                *count,
                                *newest,
                                expanded,
                                offset,
                                now,
                                cx,
                            )
                            .into_any_element()
                        }
                    })
                    .collect();
                v_flex()
                    .id("nostr-viewer-rows")
                    .debug_selector(|| "nostr-viewer-rows".into())
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.timeline_scroll_handle)
                    .custom_scrollbars(
                        ui::Scrollbars::always_visible(ui::ScrollAxes::Vertical)
                            .tracked_scroll_handle(&self.timeline_scroll_handle),
                        window,
                        cx,
                    )
                    .gap_0p5()
                    .p_1()
                    .pr_3()
                    .children(line_elements)
                    .into_any_element()
            })
            .children(
                self.selected_row()
                    .map(|row| self.detail(row, &names, window, cx)),
            )
            .into_any_element()
    }

    fn render_notifications(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let request = Self::request_line(
            &self.notifications_request,
            "Loading notifications from relays…",
        );
        let Some(feed) = &self.notifications_feed else {
            return v_flex()
                .size_full()
                .min_h_0()
                .overflow_hidden()
                .children(request)
                .child(Self::empty(
                    "No notifications loaded",
                    "Replies, mentions, reactions, and comments tagging your identity appear here.",
                ))
                .into_any_element();
        };
        let names = self.names_of(&feed.rows);
        let selected_row = self
            .selected
            .as_ref()
            .and_then(|id| feed.rows.iter().find(|row| &row.event.id == id));
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .children(request)
            .child(
                v_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(if feed.rows.is_empty() {
                        Self::empty(
                            "No notifications",
                            "No replies, mentions, reactions, or comments were returned.",
                        )
                        .into_any_element()
                    } else {
                        self.rows(
                            &feed.rows,
                            false,
                            &self.notifications_scroll_handle,
                            window,
                            cx,
                        )
                        .into_any_element()
                    }),
            )
            .when_some(selected_row, |el, row| {
                el.child(self.detail(row, &names, window, cx))
            })
            .into_any_element()
    }

    fn render_conversation(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let request = Self::request_line(
            &self.thread_request,
            "Asking the local relay and the seed relays…",
        );
        let Some(conversation) = &self.conversation else {
            return v_flex()
                .size_full()
                .children(request)
                .child(Self::empty(
                    "No conversation",
                    "Select a note in the timeline and read its conversation, or ask for one in the chat below.",
                ))
                .into_any_element();
        };
        let missing = conversation.missing.len();
        let names = self.names_of(&conversation.rows);
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .when(!self.conversation_back.is_empty(), |el| {
                        el.child(div().child(
                            ui::Button::new("nostr-conversation-back", "Back")
                                .label_size(LabelSize::XSmall)
                                .tooltip(ui::Tooltip::text(
                                    "Returns to the thread you were reading. Asks the local relay and the seed relays.",
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(root) = this.conversation_back.pop() {
                                        this.read_conversation(root, cx);
                                    }
                                })),
                        ))
                    })
                    .child(Self::line(
                        format!(
                            "Conversation {} · {} notes",
                            short_hex(&conversation.root),
                            conversation.rows.len()
                        ),
                        Color::Default,
                    ))
                    .when(missing > 0, |el| {
                        el.child(Self::line(
                            format!(
                                "{missing} referenced notes were not returned by any relay asked: {}",
                                conversation
                                    .missing
                                    .iter()
                                    .map(|id| short_hex(id))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            Color::Warning,
                        ))
                    })
                    .child(Self::remarks(&conversation.remarks))
                    .children(request),
            )
            .child(self.rows(
                &conversation.rows,
                true,
                &self.conversation_scroll_handle,
                window,
                cx,
            ))
            .children(
                self.selected_row()
                    .map(|row| self.detail(row, &names, window, cx)),
            )
            .into_any_element()
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
                    this.load_archive(cx);
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

    fn render_relay(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let request = Self::request_line(&self.relay_request, "Reading the local relay…");
        let Some(reading) = &self.relay else {
            return v_flex()
                .size_full()
                .children(request)
                .child(Self::empty(
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
        v_flex()
            .id("nostr-viewer-relay")
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.relay_scroll_handle)
            .custom_scrollbars(
                ui::Scrollbars::always_visible(ui::ScrollAxes::Vertical)
                    .tracked_scroll_handle(&self.relay_scroll_handle),
                window,
                cx,
            )
            .p_2()
            .pr_3()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        ui::Button::new("nostr-relay-sync", "Sync local relay")
                            .style(ui::ButtonStyle::Filled)
                            .label_size(LabelSize::XSmall)
                            .tooltip(ui::Tooltip::text(
                                "Contacts configured sync relays to pull allowlisted authors' public events into the local archive",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.sync_local_relay(cx))),
                    ),
            )
            .children(request)
            .child(Self::line(
                display_text(&listener),
                if listening {
                    Color::Default
                } else {
                    Color::Warning
                },
            ))
            .child(Self::line(
                format!(
                    "{} events stored · {} allowlisted authors · kinds refused: {:?}",
                    status.archive.stored_events,
                    status.policy.allowlisted_authors.len(),
                    status.policy.refused_kinds
                ),
                Color::Default,
            ))
            .child(Self::line(
                format!(
                    "Since start: {} admitted, {} refused",
                    status.admission_since_start.admitted, status.admission_since_start.refused
                ),
                Color::Default,
            ))
            .child(Self::line(
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
                    .map(|warning| Self::line(display_text(warning), Color::Warning)),
            )
            .child(Self::line("What the relay does not protect", Color::Hint))
            .children(
                status
                    .access_limits
                    .iter()
                    .map(|limit| Self::line(display_text(limit), Color::Muted)),
            )
            .child(Self::line(
                format!(
                    "Admission decisions, newest first ({})",
                    reading.decisions.len()
                ),
                Color::Hint,
            ))
            .when(reading.decisions.is_empty(), |el| {
                el.child(Self::line(
                    "None since the relay started. The log is kept in memory.",
                    Color::Muted,
                ))
            })
            .children(reading.decisions.iter().map(|decision| {
                Self::line(
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
            }))
            .into_any_element()
    }
    /// Ensure the multi-line composer `Editor` exists and syncs edits into `ComposerState`.
    fn ensure_composer_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<editor::Editor> {
        if let Some(existing) = &self.composer_editor {
            return existing.clone();
        }
        let initial_draft = self.composer.draft.clone();
        let editor = cx.new(|cx| {
            let mut ed = editor::Editor::multi_line(window, cx);
            ed.disable_scrollbars_and_minimap(window, cx);
            ed.set_show_gutter(false, cx);
            ed.set_show_line_numbers(false, cx);
            ed.set_show_git_diff_gutter(false, cx);
            ed.set_show_code_actions(false, cx);
            ed.set_show_runnables(false, cx);
            ed.set_show_bookmarks(false, cx);
            ed.set_show_breakpoints(false, cx);
            ed.set_show_wrap_guides(false, cx);
            ed.set_placeholder_text("Write a Nostr note…", window, cx);
            if !initial_draft.is_empty() {
                ed.set_text(initial_draft, window, cx);
            }
            ed
        });
        let sub = cx.subscribe(&editor, |this, ed, event: &editor::EditorEvent, cx| {
            if matches!(event, editor::EditorEvent::BufferEdited) {
                let text = ed.read(cx).text(cx);
                this.composer.set_draft(text);
                cx.notify();
            }
        });
        self.composer_editor = Some(editor.clone());
        self._composer_subscription = Some(sub);
        editor
    }

    /// Open and focus the collapsible compose drawer (`nostr_panel::Compose`).
    pub(crate) fn open_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.open_compose();
        let editor = self.ensure_composer_editor(window, cx);
        editor.update(cx, |ed, cx| {
            if ed.text(cx) != self.composer.draft {
                ed.set_text(self.composer.draft.clone(), window, cx);
            }
            window.focus(&ed.focus_handle(cx), cx);
        });
        cx.notify();
    }

    fn ensure_composer_media_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<editor::Editor> {
        if let Some(existing) = &self.composer_media_editor {
            return existing.clone();
        }
        let editor = cx.new(|cx| {
            let mut ed = editor::Editor::single_line(window, cx);
            ed.set_placeholder_text("Local media file path to attach (Blossom)…", window, cx);
            ed
        });
        self.composer_media_editor = Some(editor.clone());
        editor
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

    /// Toggle the collapsible compose drawer; collapsing always preserves the draft.
    fn toggle_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.toggle();
        if self.composer.is_open() {
            let editor = self.ensure_composer_editor(window, cx);
            editor.update(cx, |ed, cx| {
                if ed.text(cx) != self.composer.draft {
                    ed.set_text(self.composer.draft.clone(), window, cx);
                }
                window.focus(&ed.focus_handle(cx), cx);
            });
        }
        cx.notify();
    }

    fn collapse_compose(&mut self, cx: &mut Context<Self>) {
        if self.composer.is_open() && !self.composer.is_publishing() {
            self.composer.toggle();
            cx.notify();
        }
    }

    fn discard_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.is_publishing() {
            return;
        }
        self.composer.discard();
        if let Some(editor) = &self.composer_editor {
            editor.update(cx, |ed, cx| ed.set_text("", window, cx));
        }
        cx.notify();
    }

    /// Open the compose drawer pre-bound to a NIP-10 / NIP-22 reply target.
    fn open_reply_compose(
        &mut self,
        target: ReplyContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.open_reply(target);
        let editor = self.ensure_composer_editor(window, cx);
        editor.update(cx, |ed, cx| {
            if ed.text(cx) != self.composer.draft {
                ed.set_text(self.composer.draft.clone(), window, cx);
            }
            window.focus(&ed.focus_handle(cx), cx);
        });
        cx.notify();
    }

    /// Dispatch a NIP-25 `kind: 7` reaction (`nostr_publish_reaction`), or
    /// unreact via NIP-09 `kind: 5` deletion if the operator already published
    /// this reaction on `event_id`.
    fn send_reaction(
        &mut self,
        event_id: String,
        author: String,
        content: String,
        cx: &mut Context<Self>,
    ) {
        if self.reaction_in_flight.is_some() {
            return;
        }
        let emoji = reaction_emoji(&content);
        if let Some(own_id) = self
            .own_reactions
            .get(&(event_id.clone(), emoji.clone()))
            .cloned()
        {
            self.request_delete(own_id, 7, cx);
            return;
        }

        self.reaction_in_flight = Some(event_id.clone());
        self.reaction_feedback = None;
        cx.notify();

        let args = json!({
            "target_event_id": event_id,
            "target_pubkey": author,
            "content": content,
        });
        let task = match invoke(NOSTR_CLIENT_SERVER, PUBLISH_REACTION_TOOL, args) {
            Ok(task) => task,
            Err(err) => {
                self.reaction_in_flight = None;
                self.reaction_feedback = Some((event_id, false, err));
                cx.notify();
                return;
            }
        };

        self._reaction_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.reaction_in_flight = None;
                match result {
                    Ok(raw) => match crate::nostr_composer::parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => {
                            let accepted = outcome.receipt().accepted.len();
                            *this
                                .reactions
                                .entry(event_id.clone())
                                .or_default()
                                .entry(emoji.clone())
                                .or_insert(0) += 1;
                            if let Some(published_id) = outcome.receipt().event_id.clone() {
                                this.own_reactions
                                    .insert((event_id.clone(), emoji.clone()), published_id);
                            }
                            this.reaction_feedback = Some((
                                event_id.clone(),
                                true,
                                format!("Reacted {emoji} (NIP-25 · {accepted} relay(s))"),
                            ));
                            this.refresh_active_view(cx);
                        }
                        Ok(_) => {
                            this.reaction_feedback = Some((
                                event_id,
                                false,
                                format!("Reaction {emoji} failed: no relay accepted the event."),
                            ));
                        }
                        Err(err) => {
                            this.reaction_feedback = Some((event_id, false, err));
                        }
                    },
                    Err(err) => {
                        this.reaction_feedback =
                            Some((event_id, false, sanitize_untrusted_text(&err.to_string())));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Dispatch a NIP-18 repost (`nostr_publish_repost`, `kind: 6` or `kind: 16`).
    fn send_repost(
        &mut self,
        event_id: String,
        author: String,
        target_kind: u16,
        cx: &mut Context<Self>,
    ) {
        if self.reaction_in_flight.is_some() {
            return;
        }
        self.reaction_in_flight = Some(event_id.clone());
        self.reaction_feedback = None;
        cx.notify();

        let args = json!({
            "target_event_id": event_id,
            "target_pubkey": author,
            "target_kind": target_kind,
        });
        let task = match invoke(NOSTR_CLIENT_SERVER, PUBLISH_REPOST_TOOL, args) {
            Ok(task) => task,
            Err(err) => {
                self.reaction_in_flight = None;
                self.reaction_feedback = Some((event_id, false, err));
                cx.notify();
                return;
            }
        };

        self._reaction_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.reaction_in_flight = None;
                match result {
                    Ok(raw) => match crate::nostr_composer::parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => {
                            let accepted = outcome.receipt().accepted.len();
                            this.reaction_feedback = Some((
                                event_id.clone(),
                                true,
                                format!("Reposted (NIP-18 · {accepted} relay(s))"),
                            ));
                            this.refresh_active_view(cx);
                        }
                        Ok(_) => {
                            this.reaction_feedback = Some((
                                event_id,
                                false,
                                "Repost failed: no relay accepted the event.".to_string(),
                            ));
                        }
                        Err(err) => {
                            this.reaction_feedback = Some((event_id, false, err));
                        }
                    },
                    Err(err) => {
                        this.reaction_feedback =
                            Some((event_id, false, sanitize_untrusted_text(&err.to_string())));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Follow or unfollow an author by publishing an updated NIP-02 `kind: 3`
    /// contact list (`nostr_publish_follows`). Always prompts in the editor.
    fn toggle_follow(&mut self, author_hex: String, cx: &mut Context<Self>) {
        if self.reaction_in_flight.is_some() {
            return;
        }
        let mut updated: Vec<String> = match &self.follows {
            Some(Follows::Authors(set)) => set.iter().cloned().collect(),
            Some(Follows::NoList) => Vec::new(),
            _ => return,
        };
        let was_followed = updated
            .iter()
            .any(|pk| pk.eq_ignore_ascii_case(&author_hex));
        if was_followed {
            updated.retain(|pk| !pk.eq_ignore_ascii_case(&author_hex));
        } else {
            updated.push(author_hex.clone());
        }
        let feedback_target = self.selected.clone().unwrap_or_else(|| author_hex.clone());
        self.reaction_in_flight = Some(feedback_target.clone());
        self.reaction_feedback = None;
        cx.notify();

        let args = json!({ "followed_pubkeys": updated });
        let task = match invoke(NOSTR_CLIENT_SERVER, PUBLISH_FOLLOWS_TOOL, args) {
            Ok(task) => task,
            Err(err) => {
                self.reaction_in_flight = None;
                self.reaction_feedback = Some((feedback_target, false, err));
                cx.notify();
                return;
            }
        };

        self._reaction_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.reaction_in_flight = None;
                match result {
                    Ok(raw) => match crate::nostr_composer::parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => {
                            let accepted = outcome.receipt().accepted.len();
                            let verb = if was_followed {
                                "Unfollowed"
                            } else {
                                "Followed"
                            };
                            this.reaction_feedback = Some((
                                feedback_target,
                                true,
                                format!("{verb} (NIP-02 kind:3 · {accepted} relay(s))"),
                            ));
                            this.load_follows(cx);
                            this.refresh_active_view(cx);
                        }
                        Ok(_) => {
                            this.reaction_feedback = Some((
                                feedback_target,
                                false,
                                "Follow list update failed: no relay accepted the kind:3 event."
                                    .to_string(),
                            ));
                        }
                        Err(err) => {
                            this.reaction_feedback = Some((feedback_target, false, err));
                        }
                    },
                    Err(err) => {
                        this.reaction_feedback = Some((
                            feedback_target,
                            false,
                            sanitize_untrusted_text(&err.to_string()),
                        ));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Dispatch a NIP-09 `kind: 5` deletion request (`nostr_publish_deletion`)
    /// for one of the operator's own events. Always prompts in the editor.
    fn request_delete(&mut self, event_id: String, target_kind: u16, cx: &mut Context<Self>) {
        if self.reaction_in_flight.is_some() {
            return;
        }
        self.reaction_in_flight = Some(event_id.clone());
        self.reaction_feedback = None;
        cx.notify();

        let args = json!({
            "event_ids": [event_id],
            "target_kind": target_kind,
        });
        let task = match invoke(NOSTR_CLIENT_SERVER, PUBLISH_DELETION_TOOL, args) {
            Ok(task) => task,
            Err(err) => {
                self.reaction_in_flight = None;
                self.reaction_feedback = Some((event_id, false, err));
                cx.notify();
                return;
            }
        };

        self._reaction_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.reaction_in_flight = None;
                match result {
                    Ok(raw) => match crate::nostr_composer::parse_publish_receipt(&raw) {
                        Ok(outcome) if outcome.any_accepted() => {
                            let accepted = outcome.receipt().accepted.len();
                            this.reaction_feedback = Some((
                                event_id.clone(),
                                true,
                                format!("Deletion requested (NIP-09 · {accepted} relay(s))"),
                            ));
                            if target_kind == 7 {
                                this.own_reactions.retain(|_, own_id| own_id != &event_id);
                            }
                            this.refresh_active_view(cx);
                        }
                        Ok(_) => {
                            this.reaction_feedback = Some((
                                event_id,
                                false,
                                "Deletion request failed: no relay accepted the kind:5 event."
                                    .to_string(),
                            ));
                        }
                        Err(err) => {
                            this.reaction_feedback = Some((event_id, false, err));
                        }
                    },
                    Err(err) => {
                        this.reaction_feedback =
                            Some((event_id, false, sanitize_untrusted_text(&err.to_string())));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn toggle_zap_strip(
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
                    this.refresh_reactions_for_active_view(cx);
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
                    let entry = this
                        .zap_totals
                        .entry(target_event_id.clone())
                        .or_insert((0, 0));
                    entry.0 = entry.0.saturating_add(1);
                    entry.1 = entry.1.saturating_add(amount_sats);
                    this.refresh_reactions_for_active_view(cx);
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

    fn submit_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.is_publishing() {
            return;
        }
        if let Some(editor) = &self.composer_editor {
            let current_text = editor.read(cx).text(cx);
            self.composer.set_draft(current_text);
        }
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let payload = match self.composer.begin_publish(now_secs) {
            Ok(payload) => payload,
            Err(_) => {
                cx.notify();
                return;
            }
        };
        let reply_target = self.composer.reply_target.clone();
        cx.notify();

        let (tool, args) = match reply_target {
            Some(target) => (
                PUBLISH_REPLY_TOOL,
                json!({
                    "content": payload.content,
                    "root_event_id": target.root_event_id,
                    "reply_to_event_id": target.reply_to_event_id,
                    "target_pubkey": target.target_pubkey,
                    "root_kind": target.root_kind,
                    "created_at": payload.created_at,
                }),
            ),
            None => {
                let mut note_args = json!({
                    "content": payload.content,
                    "created_at": payload.created_at,
                });
                if !payload.media_paths.is_empty() {
                    note_args["media_paths"] = json!(payload.media_paths);
                }
                (PUBLISH_TOOL, note_args)
            }
        };
        let task = match invoke(NOSTR_CLIENT_SERVER, tool, args) {
            Ok(task) => task,
            Err(err) => {
                self.composer.finish_publish_err(err);
                cx.notify();
                return;
            }
        };

        let editor_handle = self.composer_editor.clone();
        let window_handle = window.window_handle();
        self._publish_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(raw_json) => {
                        let any_accepted = this.composer.finish_publish_ok(&raw_json);
                        if any_accepted {
                            if let Some(ed) = &editor_handle {
                                window_handle
                                    .update(cx, |_, window, cx| {
                                        ed.update(cx, |ed, cx| ed.set_text("", window, cx));
                                    })
                                    .log_err();
                            }
                            this.refresh_active_view(cx);
                        }
                    }
                    Err(err) => {
                        this.composer.finish_publish_err(err.to_string());
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn retry_last_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.composer.restore_last_for_retry() {
            return;
        }
        let editor = self.ensure_composer_editor(window, cx);
        editor.update(cx, |ed, cx| {
            ed.set_text(self.composer.draft.clone(), window, cx);
        });
        self.submit_compose(window, cx);
    }

    fn render_publish_receipt(&self, outcome: &PublishOutcome, cx: &Context<Self>) -> AnyElement {
        let receipt = outcome.receipt();
        let (summary, tone) = match outcome {
            PublishOutcome::Published(_) => (
                format!(
                    "Published to {}/{} relays{}",
                    receipt.accepted.len(),
                    receipt.accepted.len(),
                    if receipt.read_back_found {
                        " · verified"
                    } else {
                        ""
                    }
                ),
                Color::Success,
            ),
            PublishOutcome::PartialFailure(_) => (
                format!(
                    "Published to {}/{} relays ({} failed)",
                    receipt.accepted.len(),
                    receipt.accepted.len() + receipt.failed.len(),
                    receipt.failed.len()
                ),
                Color::Warning,
            ),
            PublishOutcome::AllFailed(_) => (
                format!("Failed on all {} relays", receipt.failed.len()),
                Color::Error,
            ),
        };

        v_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_1()
            .min_w_0()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .bg(cx.theme().colors().surface_background)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .min_w_0()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(summary)
                                .size(LabelSize::Small)
                                .color(tone)
                                .truncate(),
                        ),
                    )
                    .when(self.composer.can_retry_last(), |row| {
                        row.child(
                            ui::Button::new("nostr-receipt-retry", "Retry")
                                .label_size(LabelSize::XSmall)
                                .tooltip(ui::Tooltip::text(
                                    "Re-submit the preserved note payload to relays",
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.retry_last_compose(window, cx);
                                })),
                        )
                    })
                    .child(
                        ui::IconButton::new("nostr-receipt-dismiss", ui::IconName::Close)
                            .icon_size(ui::IconSize::XSmall)
                            .tooltip(ui::Tooltip::text("Dismiss publish receipt"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composer.dismiss_receipt();
                                cx.notify();
                            })),
                    ),
            )
            .children(receipt.failed.iter().take(3).map(|relay| {
                let detail = match &relay.reason {
                    Some(reason) => {
                        format!("{} ({}): {}", host_of(&relay.address), relay.state, reason)
                    }
                    None => format!("{} ({})", host_of(&relay.address), relay.state),
                };
                Label::new(detail)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .truncate()
            }))
            .into_any_element()
    }

    fn render_composer_drawer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.composer.is_open() {
            return None;
        }
        let editor = self.ensure_composer_editor(window, cx);
        let media_editor = if self.composer.reply_target.is_none() {
            Some(self.ensure_composer_media_editor(window, cx))
        } else {
            None
        };
        let chars = self.composer.char_count();
        let publishing = self.composer.is_publishing();
        let can_publish = !publishing && self.composer.has_draft() && chars <= MAX_CONTENT_CHARS;
        let counter_color = if chars > MAX_CONTENT_CHARS {
            Color::Error
        } else if chars >= CHAR_WARNING_THRESHOLD {
            Color::Warning
        } else {
            Color::Muted
        };
        let (npub_short, relay_count) = {
            let nostr_cfg = &KaskSettings::get_global(cx).nostr;
            let trimmed = nostr_cfg.npub.trim();
            let npub = if trimmed.is_empty() {
                "no identity".to_string()
            } else {
                short_hex(trimmed)
            };
            let relays = 1 + nostr_cfg.seed_relays.len();
            (npub, relays)
        };
        let status_text = if publishing {
            "Awaiting approval / publishing…".to_string()
        } else {
            format!("{chars}/{MAX_CONTENT_CHARS} · {npub_short} · {relay_count} relays")
        };
        let media_paths = self.composer.media_paths.clone();
        let error = self.composer.error.clone();
        let reply_banner = self.composer.reply_target.as_ref().map(|target| {
            format!(
                "Replying to {} ({})",
                target.target_label,
                target.nip_label()
            )
        });

        Some(
            v_flex()
                .id("nostr-compose-drawer")
                .w_full()
                .px_2()
                .py_2()
                .gap_2()
                .min_w_0()
                .flex_shrink_0()
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .bg(cx.theme().colors().elevated_surface_background)
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.collapse_compose(cx);
                        cx.stop_propagation();
                    } else if event.keystroke.key == "enter"
                        && (event.keystroke.modifiers.secondary(cx)
                            || event.keystroke.modifiers.control)
                    {
                        this.submit_compose(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .when_some(reply_banner, |this, banner| {
                    this.child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .min_w_0()
                            .items_center()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Label::new(banner)
                                        .size(LabelSize::XSmall)
                                        .color(Color::Accent)
                                        .truncate(),
                                ),
                            )
                            .child(
                                ui::IconButton::new("nostr-reply-clear", ui::IconName::Close)
                                    .icon_size(ui::IconSize::XSmall)
                                    .disabled(publishing)
                                    .tooltip(ui::Tooltip::text("Switch back to a top-level note"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.composer.clear_reply_target();
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .min_h(px(64.))
                        .max_h(px(120.))
                        .overflow_hidden()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(if error.is_some() {
                            cx.theme().status().error_border
                        } else {
                            cx.theme().colors().border
                        })
                        .bg(cx.theme().colors().editor_background)
                        .child(editor),
                )
                .when_some(media_editor, |drawer, med_ed| {
                    let at_limit = media_paths.len() >= MAX_MEDIA_ATTACHMENTS;
                    drawer
                        .child(
                            h_flex()
                                .w_full()
                                .gap_1()
                                .min_w_0()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .px_1()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(cx.theme().colors().border)
                                        .bg(cx.theme().colors().editor_background)
                                        .child(med_ed.clone()),
                                )
                                .child(
                                    ui::Button::new("nostr-compose-attach", "Attach")
                                        .label_size(LabelSize::XSmall)
                                        .disabled(publishing || at_limit)
                                        .tooltip(ui::Tooltip::text(format!(
                                            "Attach local media file for Blossom upload (max {MAX_MEDIA_ATTACHMENTS})"
                                        )))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            let path = med_ed.read(cx).text(cx);
                                            match this.composer.add_media_path(path) {
                                                Ok(()) => {
                                                    med_ed.update(cx, |ed, cx| {
                                                        ed.set_text("", window, cx);
                                                    });
                                                }
                                                Err(err) => {
                                                    this.composer.error = Some(err);
                                                }
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when(!media_paths.is_empty(), |d| {
                            d.child(h_flex().w_full().gap_1().flex_wrap().children(
                                media_paths.iter().enumerate().map(|(m_ix, path)| {
                                    let file_label = std::path::Path::new(path)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(path.as_str());
                                    ui::Button::new(
                                        ("nostr-compose-media-rm", m_ix),
                                        format!("📎 {file_label} ×"),
                                    )
                                    .label_size(LabelSize::XSmall)
                                    .disabled(publishing)
                                    .tooltip(ui::Tooltip::text(format!(
                                        "Remove attachment: {path}"
                                    )))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.composer.remove_media_path(m_ix);
                                        cx.notify();
                                    }))
                                }),
                            ))
                        })
                })
                .when_some(error, |this, err| {
                    this.child(
                        Label::new(err)
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    )
                })
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .min_w_0()
                        .items_center()
                        .child(
                            div().flex_1().min_w_0().child(
                                Label::new(status_text)
                                    .size(LabelSize::Small)
                                    .color(counter_color)
                                    .truncate(),
                            ),
                        )
                        .child(
                            div().flex_shrink_0().child(
                                ui::IconButton::new("nostr-compose-discard", ui::IconName::Trash)
                                    .icon_size(ui::IconSize::Small)
                                    .disabled(publishing)
                                    .tooltip(ui::Tooltip::text("Discard draft"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.discard_compose(window, cx);
                                    })),
                            ),
                        )
                        .child(
                            div().flex_shrink_0().child(
                                ui::Button::new(
                                    "nostr-compose-publish",
                                    if publishing { "Publishing…" } else { "Publish" },
                                )
                                .style(ui::ButtonStyle::Filled)
                                .disabled(!can_publish)
                                .tooltip(ui::Tooltip::text(
                                    "Publish kind:1 note (Ctrl/Cmd+Enter). Editor asks approval before signing.",
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_compose(window, cx);
                                })),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }
    fn ensure_dm_editors(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<editor::Editor>, Entity<editor::Editor>) {
        let recipient = if let Some(existing) = &self.dm_recipient_editor {
            existing.clone()
        } else {
            let ed = cx.new(|cx| {
                let mut e = editor::Editor::single_line(window, cx);
                e.set_placeholder_text("Recipient npub1… or 64-char hex", window, cx);
                e
            });
            self.dm_recipient_editor = Some(ed.clone());
            ed
        };
        let input = if let Some(existing) = &self.dm_input_editor {
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
            self.dm_input_editor = Some(ed.clone());
            ed
        };
        (recipient, input)
    }

    fn load_local_dm_threads(&mut self, cx: &mut Context<Self>) {
        if self.dm_loaded_local || self.dm_busy {
            return;
        }
        self.dm_loaded_local = true;
        self._dm_task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::load_local_threads().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(threads) => {
                        this.dm_threads = threads;
                        if this.dm_selected_peer.is_none() {
                            if let Some(first) = this.dm_threads.first() {
                                let peer = first.peer_pubkey_hex.clone();
                                this.select_dm_peer(peer, cx);
                            }
                        }
                    }
                    Err(err) => {
                        this.dm_status = Some((false, err));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn select_dm_peer(&mut self, peer_hex: String, cx: &mut Context<Self>) {
        self.dm_selected_peer = Some(peer_hex.clone());
        self._dm_task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::load_local_messages(peer_hex).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(messages) => this.dm_messages = messages,
                    Err(err) => this.dm_status = Some((false, err)),
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn sync_dms(&mut self, cx: &mut Context<Self>) {
        if self.dm_busy {
            return;
        }
        self.dm_busy = true;
        self.dm_status = Some((true, "Syncing NIP-17 gift wraps from seed relays…".into()));
        cx.notify();
        self._dm_task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::sync_dms(cx).await;
            this.update(cx, |this, cx| {
                this.dm_busy = false;
                match result {
                    Ok(report) => {
                        let summary = format!(
                            "Synced {} wrap(s), {} new DM(s) (NIP-17)",
                            report.fetched_wraps, report.newly_stored
                        );
                        this.dm_threads = report.threads;
                        this.dm_status = Some((true, summary));
                        if let Some(peer) = this.dm_selected_peer.clone() {
                            this.select_dm_peer(peer, cx);
                        } else if let Some(first) = this.dm_threads.first() {
                            this.select_dm_peer(first.peer_pubkey_hex.clone(), cx);
                        }
                    }
                    Err(err) => {
                        this.dm_status = Some((false, err));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn send_dm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dm_busy {
            return;
        }
        let (recipient_ed, input_ed) = self.ensure_dm_editors(window, cx);
        let recipient_raw = self
            .dm_selected_peer
            .clone()
            .unwrap_or_else(|| recipient_ed.read(cx).text(cx));
        let content = input_ed.read(cx).text(cx);
        self.dm_busy = true;
        self.dm_status = Some((true, "Awaiting DM approval & NIP-59 gift wrap…".into()));
        cx.notify();

        let window_handle = window.window_handle();
        let input_handle = input_ed;
        self._dm_task = Some(cx.spawn(async move |this, cx| {
            let result = nostr_dm::send_dm(recipient_raw, content, cx).await;
            this.update(cx, |this, cx| {
                this.dm_busy = false;
                match result {
                    Ok(report) => {
                        let peer = report.message.peer_pubkey_hex.clone();
                        this.dm_selected_peer = Some(peer);
                        this.dm_messages.push(report.message);
                        this.dm_status = Some((
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
                        this.dm_loaded_local = false;
                        this.load_local_dm_threads(cx);
                    }
                    Err(err) => {
                        this.dm_status = Some((false, err));
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn render_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.load_local_dm_threads(cx);
        let (recipient_ed, input_ed) = self.ensure_dm_editors(window, cx);
        let busy = self.dm_busy;
        let selected_peer = self.dm_selected_peer.clone();
        let status = self.dm_status.clone();

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
                        div().flex_shrink_0().child(
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
                                this.dm_selected_peer = None;
                                this.dm_messages.clear();
                                cx.notify();
                            })),
                    )
                    .children(self.dm_threads.iter().enumerate().map(|(ix, thread)| {
                        let peer_hex = thread.peer_pubkey_hex.clone();
                        let active = selected_peer.as_deref() == Some(peer_hex.as_str());
                        let label = format!(
                            "{} ({})",
                            short_hex(&thread.peer_pubkey_hex),
                            thread.message_count
                        );
                        ui::Button::new(("nostr-dm-peer", ix), label)
                            .label_size(LabelSize::XSmall)
                            .toggle_state(active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_dm_peer(peer_hex.clone(), cx);
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
                    .track_scroll(&self.dm_scroll_handle)
                    .custom_scrollbars(
                        ui::Scrollbars::always_visible(ui::ScrollAxes::Vertical)
                            .tracked_scroll_handle(&self.dm_scroll_handle),
                        window,
                        cx,
                    )
                    .pr_3()
                    .gap_1()
                    .when(self.dm_messages.is_empty(), |el| {
                        el.child(Self::line(
                            "No decrypted NIP-17 messages in this thread yet. Click Sync DMs or send a message below.",
                            Color::Muted,
                        ))
                    })
                    .children(self.dm_messages.iter().map(|msg| {
                        let who = if msg.outgoing { "You" } else { "Peer" };
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
            .into_any_element()
    }
    fn step_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows: Vec<String> = match self.tab {
            ViewerTab::Timeline => self
                .active_timeline_feed()
                .map(|f| f.rows.iter().map(|r| r.event.id.clone()).collect())
                .unwrap_or_default(),
            ViewerTab::Notifications => self
                .notifications_feed
                .as_ref()
                .map(|f| f.rows.iter().map(|r| r.event.id.clone()).collect())
                .unwrap_or_default(),
            ViewerTab::Conversation => self
                .conversation
                .as_ref()
                .map(|c| c.rows.iter().map(|r| r.event.id.clone()).collect())
                .unwrap_or_default(),
            ViewerTab::Relay | ViewerTab::Messages => return,
        };
        if rows.is_empty() {
            return;
        }
        let next_ix = match self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|r| r == id))
        {
            Some(cur) => (cur as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.selected = Some(rows[next_ix].clone());
        self.detail_meta_expanded = false;
        self.detail_scroll_handle
            .set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }
}

impl Render for NostrViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.zap.is_some() {
            self.ensure_zap_editors(window, cx);
        }
        let compose_drawer = self.render_composer_drawer(window, cx);
        let publish_receipt = self
            .composer
            .last_outcome
            .as_ref()
            .map(|outcome| self.render_publish_receipt(outcome, cx));
        let compose_open = self.composer.is_open();
        let has_draft = self.composer.has_draft();
        let compose_tooltip = if has_draft && !compose_open {
            "Resume unsent Nostr note draft (Ctrl/Cmd+Shift+N)"
        } else if compose_open {
            "Collapse composer (preserves draft · Esc)"
        } else {
            "New Note (Ctrl/Cmd+Shift+N)"
        };

        let body = match self.tab {
            ViewerTab::Timeline => self.render_timeline(window, cx),
            ViewerTab::Notifications => self.render_notifications(window, cx),
            ViewerTab::Conversation => self.render_conversation(window, cx),
            ViewerTab::Relay => self.render_relay(window, cx),
            ViewerTab::Messages => self.render_messages(window, cx),
        };
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if this.composer.is_open() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "escape" => {
                        if this.zap.is_some()
                            && !this.zap.as_ref().is_some_and(ZapDraftState::is_busy)
                        {
                            this.zap = None;
                            this.clear_zap_editors(window, cx);
                            cx.notify();
                            cx.stop_propagation();
                        } else if this.selected.is_some() {
                            this.selected = None;
                            this.detail_meta_expanded = false;
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }
                    "down" => {
                        this.step_selection(1, cx);
                        cx.stop_propagation();
                    }
                    "up" => {
                        this.step_selection(-1, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .child(
                h_flex()
                    .h_8()
                    .gap_1()
                    .px_2()
                    .min_w_0()
                    .overflow_hidden()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(self.tab_button(ViewerTab::Timeline, "Timeline".into(), cx))
                    .child(self.tab_button(ViewerTab::Notifications, "Alerts".into(), cx))
                    .child(self.tab_button(ViewerTab::Conversation, "Conversation".into(), cx))
                    .child(self.tab_button(ViewerTab::Relay, "Relay".into(), cx))
                    .child(self.tab_button(ViewerTab::Messages, "Messages".into(), cx))
                    .child(
                        h_flex()
                            .ml_auto()
                            .gap_1()
                            .flex_shrink_0()
                            .child(
                                ui::IconButton::new("nostr-viewer-compose", ui::IconName::Pencil)
                                    .icon_size(ui::IconSize::Small)
                                    .toggle_state(compose_open || has_draft)
                                    .icon_color(if has_draft && !compose_open {
                                        Color::Accent
                                    } else {
                                        Color::Default
                                    })
                                    .tooltip(ui::Tooltip::text(compose_tooltip))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_compose(window, cx);
                                    })),
                            )
                            .child(
                                ui::IconButton::new("nostr-viewer-refresh", ui::IconName::RotateCw)
                                    .icon_size(ui::IconSize::Small)
                                    .tooltip(ui::Tooltip::text(
                                        "Reload from the local relay. Contacts no other relay.",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                            ),
                    ),
            )
            .when_some(compose_drawer, |this, drawer| this.child(drawer))
            .when_some(publish_receipt, |this, receipt| this.child(receipt))
            .child(
                v_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(body),
            )
    }
}

/// Extract a recipient's Lightning Address (`lud16`) from a `nostr_profile`
/// JSON response (`profiles[0].fields.lud16`), with fallbacks for direct or
/// raw `kind: 0` content shapes.
fn extract_lud16_from_profile_response(profile_raw: &str) -> Option<String> {
    serde_json::from_str::<Value>(profile_raw)
        .ok()
        .and_then(|v| {
            let first = v.get("profiles")?.as_array()?.first()?;
            first
                .get("fields")
                .and_then(|fields| fields.get("lud16"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    first
                        .get("lud16")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| {
                    first
                        .get("metadata")?
                        .get("lud16")?
                        .as_str()
                        .map(str::to_string)
                })
                .or_else(|| {
                    first
                        .get("content")?
                        .as_str()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .and_then(|m| m.get("lud16")?.as_str().map(str::to_string))
                })
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, size};
    use hkask_types::nostr_wire::{CLIENT_TOOL_NAMES, RELAY_TOOL_NAMES};

    fn event(id: &str, at: u64, content: &str) -> Value {
        let root: String = "b".repeat(64);
        kind_event(1, id, at, content, &[&["e", root.as_str(), "", "root"]])
    }

    /// An event of any kind with its tags, as the tools return them.
    fn kind_event(kind: u16, id: &str, at: u64, content: &str, tags: &[&[&str]]) -> Value {
        authored_event(&"a".repeat(64), kind, id, at, content, tags)
    }

    /// An event by a named author, as the tools return them.
    fn authored_event(
        author: &str,
        kind: u16,
        id: &str,
        at: u64,
        content: &str,
        tags: &[&[&str]],
    ) -> Value {
        json!({
            "id": id,
            "author": author,
            "created_at": at,
            "kind": kind,
            "content": content,
            "content_truncated": false,
            "tags": tags,
            "tags_truncated": false,
        })
    }

    /// An event as the viewer's own `Row`, built the way `parse_surfaced`
    /// builds it.
    fn wire_row(value: &Value) -> Row {
        serde_json::from_value(value.clone()).expect("an event as the tools return it")
    }

    /// A tool the viewer names that a server does not have fails at
    /// dispatch as "tool not found". The one media tool it names
    /// (MEDIA_FETCH_IMAGE_TOOL) is the media server's own, pinned against
    /// that server's generated list by `wire_tool_name_is_the_servers_own`
    /// in hkask-mcp-media — the same chain, one link shorter.
    #[test]
    fn dispatched_tools_are_the_servers_own() {
        for tool in [ARCHIVE_TOOL, STATUS_TOOL, LOG_TOOL, SYNC_TOOL] {
            assert!(RELAY_TOOL_NAMES.contains(&tool), "{tool}");
        }
        assert!(CLIENT_TOOL_NAMES.contains(&THREAD_TOOL));
        assert!(CLIENT_TOOL_NAMES.contains(&PROFILE_TOOL));
        assert!(CLIENT_TOOL_NAMES.contains(&FETCH_TOOL));
        assert!(CLIENT_TOOL_NAMES.contains(&NOTIFICATIONS_TOOL));
        assert!(CLIENT_TOOL_NAMES.contains(&PUBLISH_TOOL));
        assert!(
            !CLIENT_TOOL_NAMES.contains(&MEDIA_FETCH_IMAGE_TOOL)
                && !RELAY_TOOL_NAMES.contains(&MEDIA_FETCH_IMAGE_TOOL),
            "the fetch tool belongs to the media server, not a nostr server"
        );
    }

    /// expect: the archive query's result reads as the local archive feed,
    /// and a read of relays as a different feed that says how the relays
    /// answered.
    #[test]
    fn a_list_of_events_is_a_feed_named_for_where_it_came_from() {
        let archive = json!({ "returned": 1, "limit": 20, "events": [event("e1", 100, "hello")] });
        let Some(Ok(Surfaced::Feed(feed))) = parse_surfaced(&archive, FeedOrigin::Archive) else {
            panic!("the archive result is a feed");
        };
        assert_eq!(feed.name.as_ref(), "Local archive");
        assert_eq!(feed.rows.len(), 1);
        assert!(feed.rows[0].event.served_by.is_empty());
        assert!(feed.remarks.is_empty());

        let mut served = event("e2", 200, "from relays");
        served["served_by"] = json!(["wss://relay.example.com/"]);
        let read = json!({
            "events": [served],
            "relays": [
                { "address": "wss://relay.example.com/", "state": "complete" },
                { "address": "wss://down.example.com/", "state": "unreachable" },
            ],
            "every_relay_finished": false,
            "more_may_exist": true,
        });
        let Some(Ok(Surfaced::Feed(feed))) = parse_surfaced(&read, FeedOrigin::Conversation) else {
            panic!("a read of relays is a feed");
        };
        assert_eq!(feed.name.as_ref(), "Read from relays");
        assert_eq!(feed.rows[0].event.served_by, ["wss://relay.example.com/"]);
        assert_eq!(
            feed.remarks[0].text.as_ref(),
            "2 relays asked: 1 complete, 1 unreachable"
        );
        assert!(feed.remarks[0].caveat, "not every relay finished");
        assert!(feed.remarks[1].caveat, "more may exist");
    }

    /// expect: a conversation keeps each note's depth, its missing-parent
    /// mark and the list of notes not found; notifications keep their type.
    #[test]
    fn a_conversation_and_notifications_keep_what_the_tool_said_about_each_note() {
        let mut reply = event("e2", 200, "a reply");
        reply["depth"] = json!(1);
        reply["parent_missing"] = json!(true);
        reply["is_focus"] = json!(true);
        let thread = json!({
            "root": "e1",
            "notes": [event("e1", 100, "root"), reply],
            "missing": ["e9"],
            "missing_count": 1,
        });
        let Some(Ok(Surfaced::Conversation(conversation))) =
            parse_surfaced(&thread, FeedOrigin::Conversation)
        else {
            panic!("a thread result is a conversation");
        };
        assert_eq!(conversation.root, "e1");
        assert_eq!(conversation.missing, ["e9"]);
        assert_eq!(conversation.rows[1].depth, 1);
        assert!(conversation.rows[1].parent_missing);
        assert!(conversation.rows[1].is_focus);
        assert!(!conversation.rows[0].is_focus);

        let mut reaction = event("e3", 300, "+");
        reaction["type"] = json!("reaction");
        let notifications = json!({ "notifications": [reaction] });
        let Some(Ok(Surfaced::Feed(feed))) =
            parse_surfaced(&notifications, FeedOrigin::Conversation)
        else {
            panic!("notifications are a feed");
        };
        assert_eq!(feed.name.as_ref(), "Notifications");
        assert_eq!(feed.rows[0].label.as_deref(), Some("reaction"));
    }

    /// expect: a result that is not events is left alone, and one whose
    /// events are malformed is an error, not an empty feed.
    #[test]
    fn other_results_are_not_feeds_and_malformed_events_are_an_error() {
        assert!(
            parse_surfaced(&json!({ "state": "configured" }), FeedOrigin::Conversation).is_none()
        );
        assert!(parse_surfaced(&json!({ "events": "no" }), FeedOrigin::Conversation).is_none());
        let malformed = json!({ "events": [{ "id": 7 }] });
        assert!(matches!(
            parse_surfaced(&malformed, FeedOrigin::Conversation),
            Some(Err(_))
        ));
    }

    /// expect: a control or direction-override character in third-party
    /// text is shown, not obeyed; line breaks and tabs are kept.
    #[test]
    fn control_and_direction_override_characters_are_made_visible() {
        let shown = display_text("a\u{202E}b\u{0007}c\nd\te\u{2066}f");
        assert_eq!(shown.as_ref(), "a\u{FFFD}b\u{FFFD}c\nd\te\u{FFFD}f");
        assert_eq!(short_hex(&"ab".repeat(32)), "abababab…abab");
        assert_eq!(short_hex("short"), "short");
    }

    /// expect: a tool result from the conversation is shown whether it
    /// arrives as text or as a value, and the newest result is what is on
    /// screen; a thread result switches to the conversation and selects the
    /// note that was asked for.
    #[gpui::test]
    fn tool_results_from_the_conversation_are_shown_in_either_transport(cx: &mut TestAppContext) {
        let viewer = cx.new(|_| NostrViewer::new());
        let as_text = json!({ "content": { "events": [event("e1", 100, "first")] } }).to_string();
        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(&Value::String(as_text), cx);
            let feed = viewer.feed.as_ref().expect("a feed is shown");
            assert_eq!(feed.rows[0].event.id, "e1");
            assert_eq!(feed.origin, FeedOrigin::Conversation);
            assert_eq!(viewer.tab, ViewerTab::Timeline);

            let as_value = json!({ "content": { "events": [event("e2", 200, "second")] } });
            viewer.ingest_tool_result(&as_value, cx);
            assert_eq!(viewer.feed.as_ref().expect("a feed").rows[0].event.id, "e2");

            viewer.ingest_tool_result(&json!({ "content": { "state": "configured" } }), cx);
            assert_eq!(
                viewer.feed.as_ref().expect("a feed").rows[0].event.id,
                "e2",
                "a result that is not events changes nothing"
            );

            let mut asked = event("e4", 400, "asked for");
            asked["is_focus"] = json!(true);
            let thread = json!({ "root": "e3", "notes": [event("e3", 300, "root"), asked] });
            viewer.ingest_tool_result(&thread, cx);
            assert_eq!(viewer.tab, ViewerTab::Conversation);
            assert_eq!(viewer.selected.as_deref(), Some("e4"));
            assert_eq!(viewer.selected_row().expect("selected").event.id, "e4");
        });
    }

    /// expect: a result that answers an older request does not replace a
    /// newer one.
    #[test]
    fn a_stale_answer_is_not_applied() {
        let mut request = Request::new();
        let first = request.begin();
        let second = request.begin();
        assert!(!request.finish(first, None), "the older answer is ignored");
        assert_eq!(request.state, Load::Loading);
        assert!(request.finish(second, Some("down".into())));
        assert_eq!(request.state, Load::Failed("down".into()));
    }

    /// expect: every kind the Timeline names by sentence names the spec
    /// that defines it; a kind with no spec is shown by its NIP-01 range,
    /// never an invented label.
    #[test]
    fn kind_labels_name_their_spec_and_unknown_kinds_fall_to_the_range() {
        for (kind, label, spec) in KIND_LABELS {
            assert!(!label.is_empty(), "kind {kind} has a label");
            assert!(
                spec.starts_with("NIP-"),
                "kind {kind} names its spec: {spec}"
            );
        }
        assert_eq!(kind_label(1111), Some(("commented", "NIP-22")));
        assert_eq!(kind_label(10002), Some(("set relay list", "NIP-65")));
        assert_eq!(
            kind_label(10050),
            None,
            "an app's private kind gets no invented label"
        );
        assert_eq!(kind_range_name(1), "regular");
        assert_eq!(kind_range_name(10050), "replaceable");
        assert_eq!(kind_range_name(25000), "ephemeral");
        assert_eq!(kind_range_name(30443), "addressable");
    }

    /// expect: an event's derived view — Note or Record — is decided by
    /// its NIP-01 kind range, with kind 0 a Record (a client's own
    /// state), kind 3 Follows and an article (30023) a Note.
    #[test]
    fn an_events_derived_view_is_decided_by_the_nip_01_range() {
        assert_eq!(kind_class(0), KindClass::Record);
        assert_eq!(kind_class(1), KindClass::Note);
        assert_eq!(kind_class(3), KindClass::Follows);
        assert_eq!(kind_class(7), KindClass::Note);
        assert_eq!(kind_class(1111), KindClass::Note);
        assert_eq!(kind_class(9999), KindClass::Note);
        assert_eq!(kind_class(10002), KindClass::Record);
        assert_eq!(kind_class(20000), KindClass::Ephemeral);
        assert_eq!(kind_class(30023), KindClass::Note);
        assert_eq!(kind_class(30443), KindClass::Record);
    }

    /// expect: a row says what it is, with a count from its tags; a kind
    /// outside the table is named by its range with the count, never a
    /// made-up app label.
    #[test]
    fn a_row_says_what_it_is_with_a_count_from_its_tags() {
        let author: String = "a".repeat(64);
        let one = wire_row(&authored_event(
            &author,
            3,
            "e1",
            100,
            "[]",
            &[&["p", &"b".repeat(64)]],
        ));
        assert_eq!(what_it_is(&one.event), "updated follows (1 person)");
        let four = wire_row(&authored_event(
            &author,
            3,
            "e2",
            100,
            "[]",
            &[
                &["p", &"b".repeat(64)],
                &["p", &"c".repeat(64)],
                &["p", &"d".repeat(64)],
                &["p", &"e".repeat(64)],
            ],
        ));
        assert_eq!(what_it_is(&four.event), "updated follows (4 people)");

        let relay_list = wire_row(&kind_event(
            10002,
            "e3",
            100,
            "",
            &[
                &["r", "wss://nostr.mom/"],
                &["r", "wss://nos.lol/"],
                &["r", "wss://nostr.bitcoiner.social/"],
            ],
        ));
        assert_eq!(what_it_is(&relay_list.event), "set relay list: 3 relays");

        let private = wire_row(&kind_event(
            10050,
            "e4",
            100,
            "AAEABQ",
            &[
                &["relay", "wss://a.example/"],
                &["relay", "wss://b.example/"],
            ],
        ));
        assert_eq!(
            what_it_is(&private.event),
            "kind 10050 (replaceable): 2 relays"
        );

        let profile = wire_row(&kind_event(0, "e5", 100, "{\"name\":\"mdz-axo\"}", &[]));
        assert_eq!(what_it_is(&profile.event), "set profile");
        let note = wire_row(&kind_event(1, "e6", 100, "hello", &[]));
        assert_eq!(what_it_is(&note.event), "posted a note");

        let referenced: String = "c".repeat(64);
        let comment = wire_row(&authored_event(
            &author,
            1111,
            "e7",
            100,
            "you - so are these the dm's?",
            &[
                &["E", referenced.as_str()],
                &["e", referenced.as_str()],
                &["p", referenced.as_str()],
                &["K", "1"],
                &["k", "1"],
            ],
        ));
        assert_eq!(
            what_it_is(&comment.event),
            "commented on a note by cccccccc…cccc"
        );
    }

    /// expect: a note shows its text; an article shows its title; a
    /// profile shows its about; a record never shows its payload — the
    /// one-line summary comes from its tags.
    #[test]
    fn a_note_shows_its_text_and_a_record_never_its_payload() {
        let note = wire_row(&kind_event(1, "e1", 100, "first line\nsecond", &[]));
        assert_eq!(supporting_line(&note.event), "first line");
        let titled = wire_row(&kind_event(
            30023,
            "e2",
            100,
            "# Heading\n\nBody",
            &[&["title", "An article"]],
        ));
        assert_eq!(supporting_line(&titled.event), "An article");
        let untitled = wire_row(&kind_event(30023, "e3", 100, "# Heading\n\nBody", &[]));
        assert_eq!(supporting_line(&untitled.event), "# Heading");
        let profile = wire_row(&kind_event(
            0,
            "e4",
            100,
            "{\"name\":\"mdz-axo\",\"about\":\"hi there\"}",
            &[],
        ));
        assert_eq!(supporting_line(&profile.event), "hi there");
        let follows = wire_row(&kind_event(3, "e5", 100, "[]", &[&["p", &"b".repeat(64)]]));
        assert_eq!(supporting_line(&follows.event), "");
        let encrypted = wire_row(&kind_event(
            10012,
            "e6",
            100,
            "ApyszrE+Dk4zm7chSUgGBfOQ",
            &[
                &["relay", "wss://a.example/"],
                &["relay", "wss://b.example/"],
            ],
        ));
        assert_eq!(supporting_line(&encrypted.event), "a.example, b.example");
    }

    /// expect: a reply's chip names the author of the note it answers,
    /// from its tags and the page's names alone; an author the page does
    /// not name keeps the shortened key; a note with no `e` tag starts a
    /// thread and has no chip.
    #[test]
    fn a_reply_names_the_note_it_answers_from_its_tags_alone() {
        let referenced: String = "b".repeat(64);
        let answered_by: String = "c".repeat(64);
        let comment = wire_row(&kind_event(
            1111,
            "e1",
            100,
            "you - so are these the dm's?",
            &[
                &["E", referenced.as_str()],
                &["e", referenced.as_str()],
                &["p", answered_by.as_str()],
                &["K", "1"],
                &["k", "1"],
            ],
        ));
        let mut names: BTreeMap<String, AuthorProfile> = BTreeMap::new();
        names.insert(
            answered_by.clone(),
            AuthorProfile {
                name: Some("mdz".to_string()),
                nip05: None,
            },
        );
        let reply = reply_reference(&comment.event, &names).expect("a comment answers a note");
        assert_eq!(reply.note, referenced);
        assert_eq!(reply.label, "↳ in reply to @mdz");
        names.remove(&answered_by);
        let reply = reply_reference(&comment.event, &names).expect("a comment answers a note");
        assert_eq!(
            reply.label,
            format!("↳ in reply to @{}", short_hex(&answered_by))
        );
        // A kind-1 note with an `e` tag is a reply too; without a `p`
        // tag it names no author.
        let reply_note = wire_row(&event("e2", 100, "a reply"));
        let reply = reply_reference(&reply_note.event, &names)
            .expect("a note with an e tag answers a note");
        assert_eq!(reply.note, referenced);
        assert_eq!(reply.label, "↳ in reply to a note");
        // A note with no `e` tag starts a thread; a profile is not a
        // reply.
        let root_note = wire_row(&kind_event(1, "e3", 100, "hello", &[]));
        assert_eq!(reply_reference(&root_note.event, &names), None);
        let profile = wire_row(&kind_event(0, "e4", 100, "{}", &[]));
        assert_eq!(reply_reference(&profile.event, &names), None);
    }

    /// expect: a kind 1 reply's chip answers its direct parent, not the
    /// thread root — NIP-10 puts the root first among the `e` tags, so the
    /// chip must read the markers, not the tag order; an unmarked `e`
    /// beside a marker is a mention, never the parent; without markers the
    /// positional scheme puts the parent last.
    #[test]
    fn a_text_notes_chip_answers_its_direct_parent_not_the_root() {
        let root: String = "b".repeat(64);
        let parent: String = "c".repeat(64);
        let parent_author: String = "d".repeat(64);
        let names: BTreeMap<String, AuthorProfile> = BTreeMap::new();
        let reply = wire_row(&kind_event(
            1,
            "e1",
            100,
            "a reply to a reply",
            &[
                &["e", root.as_str(), "", "root"],
                &["e", parent.as_str(), "", "reply"],
                &["p", parent_author.as_str()],
            ],
        ));
        let chip = reply_reference(&reply.event, &names).expect("a marked reply answers a note");
        assert_eq!(chip.note, parent);
        assert_eq!(
            chip.label,
            format!("↳ in reply to @{}", short_hex(&parent_author))
        );
        let mention = wire_row(&kind_event(
            1,
            "e2",
            100,
            "a reply with a mention",
            &[
                &["e", root.as_str(), "", "root"],
                &["e", parent.as_str()],
                &["p", parent_author.as_str()],
            ],
        ));
        let chip = reply_reference(&mention.event, &names).expect("a direct reply answers a note");
        assert_eq!(chip.note, root);
        let positional = wire_row(&kind_event(
            1,
            "e3",
            100,
            "an old-style reply",
            &[
                &["e", root.as_str()],
                &["e", parent.as_str()],
                &["p", parent_author.as_str()],
            ],
        ));
        let chip =
            reply_reference(&positional.event, &names).expect("a positional reply answers a note");
        assert_eq!(chip.note, parent);
        // A note whose only marked `e` tags are mentions declares no
        // parent, so it carries no reply chip.
        let mention_only = wire_row(&kind_event(
            1,
            "e4",
            100,
            "a note that mentions",
            &[&["e", root.as_str(), "", "mention"]],
        ));
        assert_eq!(reply_reference(&mention_only.event, &names), None);
    }

    /// expect: names and NIP-05 addresses come from the newest kind 0 in
    /// the page; an author with none keeps the shortened key.
    #[test]
    fn names_come_from_the_newest_kind_0_in_the_page() {
        let author: String = "a".repeat(64);
        let older = wire_row(&authored_event(
            &author,
            0,
            "e1",
            100,
            "{\"name\":\"old\"}",
            &[],
        ));
        let newer = wire_row(&authored_event(
            &author,
            0,
            "e2",
            200,
            "{\"name\":\"mdz-axo\",\"nip05\":\"mdz-axo@example.com\"}",
            &[],
        ));
        let stranger: String = "b".repeat(64);
        let other = wire_row(&authored_event(&stranger, 1, "e3", 150, "hello", &[]));
        let names = names_from_rows(&[older, newer, other]);
        let profile = names
            .get(&author)
            .expect("the newest kind 0 names the author");
        assert_eq!(profile.name.as_deref(), Some("mdz-axo"));
        assert_eq!(profile.nip05.as_deref(), Some("mdz-axo@example.com"));
        assert!(!names.contains_key(&stranger));
        let broken: String = "c".repeat(64);
        let malformed = wire_row(&authored_event(&broken, 0, "e4", 100, "not json", &[]));
        assert!(names_from_rows(&[malformed]).is_empty());
    }

    /// expect: a row's time reads relative — now, then minutes, hours and
    /// days — and falls back to the local clock for a claimed future
    /// time, the local date beyond a week, and the unreadable claim.
    #[test]
    fn relative_time_reads_now_then_minutes_hours_and_days() {
        let offset = UtcOffset::UTC;
        let now = OffsetDateTime::from_unix_timestamp(1_000_000).expect("a fixed now");
        assert_eq!(relative_time_text(1_000_000, now, offset), "now");
        assert_eq!(relative_time_text(1_000_000 - 30, now, offset), "now");
        assert_eq!(relative_time_text(1_000_000 - 90, now, offset), "1m");
        assert_eq!(relative_time_text(1_000_000 - 3_700, now, offset), "1h");
        assert_eq!(relative_time_text(1_000_000 - 90_000, now, offset), "1d");
        let ahead = 1_000_000 + 60;
        assert_eq!(
            relative_time_text(ahead, now, offset),
            local_time_text(ahead, offset)
        );
        let old = 1_000_000 - 8 * 86_400;
        assert_eq!(
            relative_time_text(old, now, offset),
            day_text(old, now, offset)
        );
        assert_eq!(
            relative_time_text(u64::MAX, now, offset),
            format!("{} (not a date)", u64::MAX)
        );
    }

    /// expect: an author's face is drawn from the pubkey alone — one
    /// author is always the same color, and two authors differ.
    #[test]
    fn an_authors_face_comes_from_the_pubkey_alone() {
        let author: String = "a".repeat(64);
        assert_eq!(identicon_color(&author), identicon_color(&author));
        let other: String = "b".repeat(64);
        assert_ne!(identicon_color(&author), identicon_color(&other));
    }

    /// expect: only a click made while a conversation is on screen
    /// remembers the thread behind it, and never the thread asked for
    /// itself.
    #[test]
    fn only_a_conversation_refocus_remembers_the_thread_behind() {
        let root: String = "b".repeat(64);
        let mut viewer = NostrViewer::new();
        viewer.tab = ViewerTab::Conversation;
        assert_eq!(viewer.thread_left_behind(&root), None);
        viewer.conversation = Some(Conversation {
            root: root.clone(),
            rows: Vec::new(),
            missing: Vec::new(),
            remarks: Vec::new(),
        });
        assert_eq!(viewer.thread_left_behind(&root), None);
        let other: String = "c".repeat(64);
        assert_eq!(viewer.thread_left_behind(&other), Some(root));
        viewer.tab = ViewerTab::Timeline;
        assert_eq!(viewer.thread_left_behind(&other), None);
    }

    /// expect: For reading collapses one author's records of one local
    /// minute under one exact-count row, and nothing is dropped when the
    /// group is expanded; Everything shows every row; a lone record
    /// stays a row.
    #[test]
    fn for_reading_collapses_one_authors_records_of_one_minute_exactly() {
        let author: String = "a".repeat(64);
        let mut values = Vec::new();
        values.push(authored_event(
            &author,
            1111,
            "e0",
            600,
            "you - so are these the dm's?",
            &[&["e", &"f".repeat(64)]],
        ));
        values.push(authored_event(
            &author,
            3,
            "e1",
            500,
            "[]",
            &[&["p", &"b".repeat(64)]],
        ));
        for (id, kind) in [
            ("e2", 10002),
            ("e3", 10051),
            ("e4", 30443),
            ("e5", 10005),
            ("e6", 10007),
            ("e7", 10050),
            ("e8", 10086),
            ("e9", 10012),
            ("e10", 0),
        ] {
            values.push(authored_event(&author, kind, id, 100, "{}", &[]));
        }
        let rows: Vec<Row> = values.iter().map(wire_row).collect();
        let names = names_from_rows(&rows);
        let empty: HashSet<(String, i64)> = HashSet::new();

        let reading = timeline_lines(
            &rows,
            &names,
            TimelineFilter::ForReading,
            None,
            &empty,
            UtcOffset::UTC,
        );
        assert!(matches!(reading[0], TimelineLine::Divider { .. }));
        assert!(matches!(&reading[1], TimelineLine::Entry { row } if row.event.kind == 1111));
        assert!(matches!(&reading[2], TimelineLine::Entry { row } if row.event.kind == 3));
        match &reading[3] {
            TimelineLine::Collapsed { count, .. } => assert_eq!(*count, 9),
            other => panic!("the minute's nine records are collapsed, not {other:?}"),
        }
        assert_eq!(reading.len(), 4, "nothing else is on screen");

        let mut expanded: HashSet<(String, i64)> = HashSet::new();
        expanded.insert((author.clone(), minute_bucket(100, UtcOffset::UTC)));
        let expanded_view = timeline_lines(
            &rows,
            &names,
            TimelineFilter::ForReading,
            None,
            &expanded,
            UtcOffset::UTC,
        );
        assert_eq!(
            expanded_view.len(),
            4 + 9,
            "the collapsed row stays and every record follows"
        );
        assert!(
            expanded_view[4..]
                .iter()
                .all(|line| matches!(line, TimelineLine::Entry { .. }))
        );

        let everything = timeline_lines(
            &rows,
            &names,
            TimelineFilter::Everything,
            None,
            &empty,
            UtcOffset::UTC,
        );
        assert_eq!(everything.len(), 1 + 11, "one divider and every event");

        // A lone record stays a row: one event is not a group.
        let lone = vec![wire_row(&authored_event(
            &author,
            0,
            "z1",
            100,
            "{\"name\":\"solo\"}",
            &[],
        ))];
        let lone_names = names_from_rows(&lone);
        let lone_view = timeline_lines(
            &lone,
            &lone_names,
            TimelineFilter::ForReading,
            None,
            &empty,
            UtcOffset::UTC,
        );
        assert!(matches!(&lone_view[1], TimelineLine::Entry { row } if row.event.kind == 0));
    }

    /// expect: day dividers are drawn at each local day, and only there.
    #[test]
    fn day_dividers_are_drawn_at_each_local_day() {
        let author: String = "a".repeat(64);
        let rows = vec![
            wire_row(&authored_event(&author, 1, "e1", 86_401, "day 2", &[])),
            wire_row(&authored_event(&author, 1, "e2", 1, "day 1", &[])),
        ];
        let names = names_from_rows(&rows);
        let lines = timeline_lines(
            &rows,
            &names,
            TimelineFilter::ForReading,
            None,
            &HashSet::new(),
            UtcOffset::UTC,
        );
        assert!(matches!(lines[0], TimelineLine::Divider { at: 86_401 }));
        assert!(matches!(&lines[1], TimelineLine::Entry { row } if row.event.id == "e1"));
        assert!(matches!(lines[2], TimelineLine::Divider { at: 1 }));
        assert!(matches!(&lines[3], TimelineLine::Entry { row } if row.event.id == "e2"));
    }

    /// expect: the Threads feed shows only the notes that start a thread,
    /// the Replies feed only the notes that answer one — the same
    /// answer-a-note resolver the reply chip reads, so a note's chip and
    /// its feed agree; a day whose every row is filtered out draws no
    /// divider; For reading keeps the whole page.
    #[test]
    fn threads_and_replies_feeds_show_their_own_notes_only() {
        let root: String = "b".repeat(64);
        let parent: String = "c".repeat(64);
        let parent_author: String = "d".repeat(64);
        let commented: String = "e".repeat(64);
        let commenter: String = "f".repeat(64);
        let rows = vec![
            wire_row(&kind_event(
                3,
                "r3",
                3 * 86_400 + 200,
                "[]",
                &[&["p", root.as_str()]],
            )),
            wire_row(&kind_event(1, "n1", 2 * 86_400 + 190, "a post", &[])),
            wire_row(&kind_event(
                1,
                "n2",
                2 * 86_400 + 180,
                "a reply",
                &[
                    &["e", root.as_str(), "", "root"],
                    &["e", parent.as_str(), "", "reply"],
                    &["p", parent_author.as_str()],
                ],
            )),
            wire_row(&kind_event(
                1111,
                "c1",
                2 * 86_400 + 170,
                "a comment",
                &[
                    &["E", commented.as_str()],
                    &["e", commented.as_str()],
                    &["p", commenter.as_str()],
                ],
            )),
            wire_row(&kind_event(
                1,
                "n3",
                86_400 + 100,
                "a mention",
                &[&["e", root.as_str(), "", "mention"]],
            )),
            wire_row(&kind_event(0, "r1", 86_400 + 90, "{\"name\":\"mdz\"}", &[])),
        ];
        let names = names_from_rows(&rows);
        let empty: HashSet<(String, i64)> = HashSet::new();

        let threads = timeline_lines(
            &rows,
            &names,
            TimelineFilter::Threads,
            None,
            &empty,
            UtcOffset::UTC,
        );
        assert_eq!(threads.len(), 4, "two dividers and the two thread roots");
        assert!(matches!(threads[0], TimelineLine::Divider { at } if at == 2 * 86_400 + 190));
        assert!(matches!(&threads[1], TimelineLine::Entry { row } if row.event.id == "n1"));
        assert!(matches!(threads[2], TimelineLine::Divider { at } if at == 86_400 + 100));
        assert!(matches!(&threads[3], TimelineLine::Entry { row } if row.event.id == "n3"));

        let replies = timeline_lines(
            &rows,
            &names,
            TimelineFilter::Replies,
            None,
            &empty,
            UtcOffset::UTC,
        );
        assert_eq!(replies.len(), 3, "one divider and the two answers");
        assert!(matches!(replies[0], TimelineLine::Divider { at } if at == 2 * 86_400 + 180));
        assert!(matches!(&replies[1], TimelineLine::Entry { row } if row.event.id == "n2"));
        assert!(matches!(&replies[2], TimelineLine::Entry { row } if row.event.id == "c1"));

        let reading = timeline_lines(
            &rows,
            &names,
            TimelineFilter::ForReading,
            None,
            &empty,
            UtcOffset::UTC,
        );
        assert_eq!(reading.len(), 3 + 6, "three dividers and every event");
    }

    /// expect: the Timeline's empty message says whose truth it is — an
    /// empty page is the query's truth, a page the filter empties is the
    /// filter's, and the Following feed names which of its own truths
    /// it is.
    #[test]
    fn the_empty_timeline_names_whose_truth_it_is() {
        let (title, _) = empty_timeline(TimelineFilter::ForReading, None, true);
        assert_eq!(title, "No events");
        let (title, _) = empty_timeline(TimelineFilter::Everything, None, true);
        assert_eq!(title, "No events");
        let (title, hint) = empty_timeline(TimelineFilter::Threads, None, false);
        assert_eq!(title, "No threads");
        assert!(hint.contains("starts a thread"));
        let (title, hint) = empty_timeline(TimelineFilter::Replies, None, false);
        assert_eq!(title, "No replies");
        assert!(hint.contains("answers another note"));
        let (title, hint) = empty_timeline(
            TimelineFilter::Following,
            Some(&Follows::Authors(BTreeSet::new())),
            false,
        );
        assert_eq!(title, "No notes from the authors you follow");
        assert!(hint.contains("allowlisted authors"));
        let (title, _) =
            empty_timeline(TimelineFilter::Following, Some(&Follows::NoIdentity), false);
        assert_eq!(title, "No identity set");
        let (title, _) = empty_timeline(
            TimelineFilter::Following,
            Some(&Follows::InvalidIdentity),
            false,
        );
        assert_eq!(title, "The npub is not usable");
        let (title, _) = empty_timeline(TimelineFilter::Following, Some(&Follows::NoList), false);
        assert_eq!(title, "No follows list of yours in the archive");
        let (title, _) = empty_timeline(TimelineFilter::Following, None, false);
        assert_eq!(title, "Nothing shown");
    }

    /// expect: a follows list is its `p` tags — every author it names —
    /// and the identity setting reaches the archive read only in a shape
    /// no error can echo as a secret.
    #[test]
    fn a_follows_list_is_its_p_tags_and_the_identity_shape_is_checked() {
        let friend: String = "b".repeat(64);
        let other: String = "c".repeat(64);
        let list = wire_row(&kind_event(
            3,
            "f1",
            100,
            "[]",
            &[
                &["p", friend.as_str()],
                &["p", other.as_str()],
                &["p", ""],
                &["e", friend.as_str()],
            ],
        ));
        assert_eq!(
            follows_of(&list.event),
            BTreeSet::from([friend, other]),
            "an empty p tag names no one"
        );

        assert!(identity_is_sendable("npub1something"));
        assert!(identity_is_sendable(&"b".repeat(64)));
        assert!(!identity_is_sendable("nsec1secret"));
        assert!(!identity_is_sendable("hello"));
        assert!(!identity_is_sendable(""));
    }

    /// expect: the Following feed shows only the notes of the authors
    /// your own follows list names — records and strangers are filtered
    /// out, a day whose every row is filtered out draws no divider, and
    /// a follows answer that is not authors shows nothing.
    #[test]
    fn the_following_feed_shows_your_follows_notes_only() {
        let friend: String = "b".repeat(64);
        let stranger: String = "c".repeat(64);
        let rows = vec![
            wire_row(&authored_event(
                &friend,
                1,
                "n1",
                2 * 86_400 + 190,
                "a post",
                &[],
            )),
            wire_row(&authored_event(
                &friend,
                1111,
                "c1",
                2 * 86_400 + 180,
                "a comment",
                &[&["e", stranger.as_str()], &["p", stranger.as_str()]],
            )),
            wire_row(&authored_event(
                &friend,
                3,
                "r1",
                2 * 86_400 + 170,
                "[]",
                &[&["p", stranger.as_str()]],
            )),
            wire_row(&authored_event(
                &stranger,
                1,
                "n2",
                2 * 86_400 + 160,
                "a stranger",
                &[],
            )),
            wire_row(&authored_event(
                &friend,
                10002,
                "r2",
                86_400 + 100,
                "",
                &[&["r", "wss://nostr.mom/"]],
            )),
        ];
        let names = names_from_rows(&rows);
        let follows = Follows::Authors(BTreeSet::from([friend]));
        let lines = timeline_lines(
            &rows,
            &names,
            TimelineFilter::Following,
            Some(&follows),
            &HashSet::new(),
            UtcOffset::UTC,
        );
        assert_eq!(
            lines.len(),
            3,
            "one divider and the friend's note and comment"
        );
        assert!(matches!(lines[0], TimelineLine::Divider { at } if at == 2 * 86_400 + 190));
        assert!(matches!(&lines[1], TimelineLine::Entry { row } if row.event.id == "n1"));
        assert!(matches!(&lines[2], TimelineLine::Entry { row } if row.event.id == "c1"));

        for not_authors in [
            None,
            Some(Follows::NoIdentity),
            Some(Follows::InvalidIdentity),
            Some(Follows::NoList),
        ] {
            let lines = timeline_lines(
                &rows,
                &names,
                TimelineFilter::Following,
                not_authors.as_ref(),
                &HashSet::new(),
                UtcOffset::UTC,
            );
            assert!(
                lines.is_empty(),
                "a follows answer that is not authors shows nothing"
            );
        }
    }

    /// expect: with no identity set in the Nostr settings — the default
    /// — the Following feed says so, without dispatching anything.
    #[gpui::test]
    fn with_no_identity_set_the_following_feed_says_so(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
        });
        let viewer = cx.new(|_| NostrViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.load_follows(cx);
            assert_eq!(viewer.follows, Some(Follows::NoIdentity));
        });
    }

    /// expect: reactions are counted by the note each answers and the
    /// emoji each carries — NIP-25's `+` and an empty content both read
    /// as the like — a reaction naming no note counts for nothing, a
    /// read that hits its limit marks every count a floor, and the row's
    /// text says which.
    #[test]
    fn reactions_are_counted_by_the_note_they_answer() {
        let target: String = "b".repeat(64);
        let other: String = "c".repeat(64);
        let rows: Vec<Row> = [
            kind_event(
                7,
                "r1",
                100,
                "+",
                &[&["e", target.as_str()], &["p", &"a".repeat(64)]],
            ),
            kind_event(7, "r2", 99, "❤", &[&["e", target.as_str()]]),
            kind_event(7, "r3", 98, "+", &[&["e", other.as_str()]]),
            kind_event(7, "r4", 97, "+", &[]),
            kind_event(7, "r5", 96, "", &[&["e", target.as_str()]]),
            kind_event(1, "n1", 95, "a note", &[]),
        ]
        .iter()
        .map(wire_row)
        .collect();
        let (counts, clipped) = reaction_counts(&rows);
        let per_target = counts.get(&target).expect("the target's reactions");
        assert_eq!(
            per_target.get("👍"),
            Some(&2),
            "+ and the empty content read as the like"
        );
        assert_eq!(per_target.get("❤"), Some(&1));
        assert_eq!(per_target.len(), 2);
        assert_eq!(counts.get(&other).and_then(|per| per.get("👍")), Some(&1));
        assert_eq!(
            counts.len(),
            2,
            "a reaction naming no note counts for nothing"
        );
        assert!(!clipped);

        let full: Vec<Row> = (0..ARCHIVE_PAGE)
            .map(|ix| {
                wire_row(&kind_event(
                    7,
                    &format!("f{ix}"),
                    100,
                    "+",
                    &[&["e", target.as_str()]],
                ))
            })
            .collect();
        let (counts, clipped) = reaction_counts(&full);
        assert_eq!(
            counts.get(&target).and_then(|per| per.get("👍")),
            Some(&(ARCHIVE_PAGE as u64))
        );
        assert!(
            clipped,
            "a read that hits its limit marks every count a floor"
        );

        let likes = BTreeMap::from([("👍".to_string(), 2), ("❤".to_string(), 1)]);
        assert_eq!(reactions_text(&likes, false), "👍 2 · ❤ 1");
        assert_eq!(reactions_text(&likes, true), "👍 2 · ❤ 1 · more may exist");
        assert_eq!(
            reactions_text(&BTreeMap::from([("👍".to_string(), 1)]), false),
            "👍 1"
        );
    }

    /// expect: a note's `imeta` tag (NIP-92, variadic `key value` pairs)
    /// names the media it carries — url, blurhash, dimensions — and a
    /// dimension that does not parse names none.
    #[test]
    fn an_imeta_tag_names_the_media_a_note_carries() {
        let note = wire_row(&kind_event(
            1,
            "n1",
            100,
            "a photo https://nostr.build/i/x.jpg",
            &[
                &[
                    "imeta",
                    "url https://nostr.build/i/x.jpg",
                    "m image/jpeg",
                    "blurhash 00TSUA",
                    "dim 3024x4032",
                ],
                &["e", &"b".repeat(64)],
            ],
        ));
        let meta = media_of(&note.event).expect("the imeta tag names the media");
        assert_eq!(meta.url, "https://nostr.build/i/x.jpg");
        assert_eq!(meta.blurhash.as_deref(), Some("00TSUA"));
        assert_eq!(meta.dimensions, Some((3024, 4032)));

        let undated = wire_row(&kind_event(
            1,
            "n2",
            100,
            "a photo",
            &[&[
                "imeta",
                "url https://nostr.build/i/y.jpg",
                "blurhash 00TSUA",
                "dim not-a-size",
            ]],
        ));
        let meta = media_of(&undated.event).expect("the imeta tag still names the media");
        assert_eq!(
            meta.dimensions, None,
            "a dimension that does not parse names none"
        );

        let bare = wire_row(&kind_event(
            1,
            "n3",
            100,
            "a photo",
            &[&["imeta", "url https://nostr.build/i/z.jpg"]],
        ));
        let bare_meta = media_of(&bare.event).expect("imeta without blurhash still resolves");
        assert_eq!(bare_meta.blurhash, None);

        let plain = wire_row(&kind_event(1, "n4", 100, "a note", &[]));
        assert!(media_of(&plain.event).is_none());
    }

    #[test]
    fn media_of_resolves_imeta_without_blurhash_video_and_content_urls() {
        let no_blurhash = wire_row(&kind_event(
            1,
            "m1",
            100,
            "photo",
            &[&["imeta", "url https://nostr.build/i/z.jpg", "m image/jpeg"]],
        ));
        assert_eq!(
            media_of(&no_blurhash.event),
            Some(MediaAttachment {
                url: "https://nostr.build/i/z.jpg".into(),
                kind: MediaAttachmentKind::Image,
                blurhash: None,
                dimensions: None,
            })
        );

        let video_imeta = wire_row(&kind_event(
            1,
            "m2",
            100,
            "walk",
            &[&[
                "imeta",
                "url https://blossom.primal.net/clip.mp4",
                "m video/mp4",
            ]],
        ));
        assert_eq!(
            media_of(&video_imeta.event),
            Some(MediaAttachment {
                url: "https://blossom.primal.net/clip.mp4".into(),
                kind: MediaAttachmentKind::Video,
                blurhash: None,
                dimensions: None,
            })
        );

        let bare_video = wire_row(&kind_event(
            1,
            "m3",
            100,
            "Starting to walk... https://blossom.primal.net/walk.mp4",
            &[],
        ));
        assert_eq!(
            media_of(&bare_video.event),
            Some(MediaAttachment {
                url: "https://blossom.primal.net/walk.mp4".into(),
                kind: MediaAttachmentKind::Video,
                blurhash: None,
                dimensions: None,
            })
        );
    }

    #[test]
    fn extract_links_parses_markdown_iframes_and_bare_urls() {
        let content = "See [Effective CTest](https://youtu.be/whaPQ5BU2y8), \
            <iframe src=\"https://www.youtube.com/embed/bsXLMQ6WgIk\"></iframe>, \
            <iframe src=\"//www.slideshare.net/slideshow/embed_code/key/8ArXI0cb8eaBHu\"></iframe>, \
            [bad](javascript:alert(1)), and https://purplekarrot.net/blog.";
        let links = extract_links(content);
        let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://youtu.be/whaPQ5BU2y8",
                "https://www.youtube.com/embed/bsXLMQ6WgIk",
                "https://www.slideshare.net/slideshow/embed_code/key/8ArXI0cb8eaBHu",
                "https://purplekarrot.net/blog",
            ]
        );
        assert_eq!(links[0].label, "Effective CTest");
        assert_eq!(links[1].label, "YouTube video");
        assert_eq!(links[2].label, "SlideShare presentation");
        assert!(is_platform_video_url("https://youtu.be/whaPQ5BU2y8"));
        assert!(is_platform_video_url(
            "https://www.youtube.com/embed/bsXLMQ6WgIk"
        ));
        assert!(is_platform_video_url("https://vimeo.com/12345"));
        assert!(!is_platform_video_url("https://purplekarrot.net/blog"));
    }

    /// expect: each note's own blurhash decodes to a small preview keyed
    /// by note id — local computation, nothing fetched — and a hash that
    /// is not a blurhash is skipped, not guessed at.
    #[test]
    fn a_notes_own_blurhash_decodes_to_its_preview() {
        let rows: Vec<Row> = [
            kind_event(
                1,
                "n1",
                100,
                "a photo",
                &[&[
                    "imeta",
                    "url https://nostr.build/i/x.jpg",
                    "blurhash 00TSUA",
                    "dim 2x1",
                ]],
            ),
            kind_event(
                1,
                "n2",
                99,
                "a broken hash",
                &[&[
                    "imeta",
                    "url https://nostr.build/i/y.jpg",
                    "blurhash not-a-hash",
                ]],
            ),
            kind_event(1, "n3", 98, "a note", &[]),
        ]
        .iter()
        .map(wire_row)
        .collect();
        let (_, previews) = media_index_of(&rows);
        assert_eq!(previews.len(), 1, "only the decodable hash previews");
        assert!(previews.contains_key("n1"));
    }

    /// expect: the fetch click's label names the host, so the reader knows
    /// which host the click contacts. A URL whose host cannot be read is
    /// labelled by its whole URL.
    #[test]
    fn the_fetch_label_names_the_host() {
        assert_eq!(host_of("https://nostr.build/i/x.jpg"), "nostr.build");
        assert_eq!(
            host_of("https://user:pass@example.com:8080/x.png"),
            "example.com"
        );
        assert_eq!(
            host_of("https://cdn.example.co.uk/a?b#c"),
            "cdn.example.co.uk"
        );
        assert_eq!(host_of("not a url"), "not a url");
    }

    /// expect: fetched bytes decode to a bounded image — scaled to fit
    /// FETCHED_IMAGE_HEIGHT, aspect kept, never upscaled — so one image
    /// cannot take the pane over.
    #[test]
    fn a_fetched_image_decodes_bounded_and_never_upscaled() {
        let mut png = Vec::new();
        image::RgbImage::new(800, 400)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode an 800x400 PNG");
        let decoded = decode_fetched_image(&png).expect("a fetched PNG decodes");
        let size = decoded.size(0);
        assert_eq!(size.height, FETCHED_IMAGE_HEIGHT.into());
        assert_eq!(size.width, 480.into(), "the aspect is kept");

        let mut small = Vec::new();
        image::RgbImage::new(20, 10)
            .write_to(
                &mut std::io::Cursor::new(&mut small),
                image::ImageFormat::Png,
            )
            .expect("encode a 20x10 PNG");
        let kept = decode_fetched_image(&small).expect("a small fetched PNG decodes");
        assert_eq!(kept.size(0).width, 20.into(), "never upscaled");

        let refused = decode_fetched_image(b"not an image").expect_err("non-image bytes");
        assert!(refused.contains("did not decode"));
    }

    /// expect: a note's media offers a labelled fetch click — the label
    /// names the host — and once a fetch answered, the fetched image
    /// replaces the placeholder and the click is gone.
    #[gpui::test]
    fn a_media_row_offers_a_labelled_fetch_and_shows_the_fetched_image(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
            if !cx.has_global::<theme::GlobalTheme>() {
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            }
        });
        let viewer = cx.new(|_| NostrViewer::new());
        viewer.update(cx, |viewer, cx| {
            let media = kind_event(
                1,
                "e7",
                30,
                "a photo",
                &[&[
                    "imeta",
                    "url https://nostr.build/i/x.jpg",
                    "blurhash 00TSUA",
                    "dim 2x1",
                ]],
            );
            viewer.ingest_tool_result(&json!({ "events": [media] }), cx);
        });

        struct Host {
            viewer: Entity<NostrViewer>,
        }
        impl Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().size_full().child(self.viewer.clone())
            }
        }
        let (_, cx) = cx.add_window_view(|_window, _cx| Host {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(280.), px(600.)));
        cx.run_until_parked();

        // The Timeline enumerates lines: divider 0, then the media row 1.
        let chip = cx
            .debug_bounds("nostr-load-image-1")
            .expect("the labelled fetch click is laid out");
        assert!(
            chip.right() <= px(280.) + px(1.),
            "the fetch click fits a 280px pane: right edge {:?}",
            chip.right()
        );

        // A fetched image replaces the placeholder: the image is shown and
        // the click is gone.
        viewer.update(cx, |viewer, cx| {
            viewer.fetched_images.insert(
                "e7".to_string(),
                Arc::new(RenderImage::new(vec![Frame::new(image::RgbaImage::new(
                    2, 1,
                ))])),
            );
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("nostr-fetched-image-1").is_some(),
            "the fetched image is shown"
        );
        assert!(
            cx.debug_bounds("nostr-load-image-1").is_none(),
            "the fetch click is gone once the image is fetched"
        );
    }

    /// expect: a refused fetch is visible as a failure and the click stays
    /// offered for a retry — an unwired invoker's refusal included.
    #[gpui::test]
    fn a_fetch_refusal_is_visible_and_the_click_is_offered_again(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
            if !cx.has_global::<theme::GlobalTheme>() {
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            }
        });
        let viewer = cx.new(|_| NostrViewer::new());
        viewer.update(cx, |viewer, cx| {
            let media = kind_event(
                1,
                "e7",
                30,
                "a photo",
                &[&[
                    "imeta",
                    "url https://nostr.build/i/x.jpg",
                    "blurhash 00TSUA",
                    "dim 2x1",
                ]],
            );
            viewer.ingest_tool_result(&json!({ "events": [media] }), cx);
        });

        viewer.update(cx, |viewer, cx| {
            viewer.fetch_image(
                "e7".to_string(),
                "https://nostr.build/i/x.jpg".to_string(),
                cx,
            );
            let request = viewer
                .image_requests
                .get("e7")
                .expect("the request is kept");
            assert!(
                matches!(request.state, Load::Failed(_)),
                "the unwired invoker's refusal is visible, not swallowed"
            );
        });

        struct Host {
            viewer: Entity<NostrViewer>,
        }
        impl Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().size_full().child(self.viewer.clone())
            }
        }
        let (_, cx) = cx.add_window_view(|_window, _cx| Host {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(280.), px(600.)));
        cx.run_until_parked();
        // The Timeline enumerates lines: divider 0, then the media row 1.
        assert!(
            cx.debug_bounds("nostr-load-image-1").is_some(),
            "a refused fetch leaves the click offered for a retry"
        );
    }

    /// expect: the detail keeps the exact UTC reading; the local forms
    /// come from `time_format`, and the machine's locale decides them, so
    /// they are not asserted byte for byte.
    #[test]
    fn utc_text_keeps_the_utc_reading() {
        let shown = utc_text(0);
        assert!(
            shown.starts_with("01/01/1970"),
            "the UTC date is shown: {shown}"
        );
        assert!(
            shown.ends_with(" UTC"),
            "the UTC reading is labelled: {shown}"
        );
    }

    /// expect: a profile lookup reads the name out of the tool's result —
    /// found with a name, found without one, or not a profile result at
    /// all, which is not a lookup.
    #[test]
    fn a_profile_lookup_names_the_author_from_the_tool_result() {
        let author: String = "a".repeat(64);
        let named = json!({
            "profiles": [{ "public_key_hex": author, "found": true,
                          "fields": { "name": "mdz-axo" } }],
        });
        assert_eq!(profile_name_of(&named), Some(Some("mdz-axo".to_string())));
        let unnamed = json!({
            "profiles": [{ "public_key_hex": author, "found": true,
                          "fields": { "about": "hi" } }],
        });
        assert_eq!(profile_name_of(&unnamed), Some(None));
        let absent = json!({
            "profiles": [{ "public_key_hex": author, "found": false }],
        });
        assert_eq!(profile_name_of(&absent), Some(None));
        assert_eq!(profile_name_of(&json!({ "events": [] })), None);
    }

    /// expect: the viewer fits a narrow pane. A long unbroken line of
    /// third-party text must not push a row or the selected event's
    /// content past the pane's edge, and must not make a row taller than
    /// another row of the same shape: one long note would otherwise push
    /// every other note off screen. The Timeline row's header carries the
    /// right-aligned relative time; its supporting, reply-chip and
    /// trailer lines truncate like the header. A selected row gains the
    /// trailer line, a comment gains its reply chip line, and both short
    /// notes carry their reaction count line, so equal heights are
    /// asserted between same-shape rows.
    ///
    /// What this was shown to catch (2026-11-13): with the row's `min_w_0`
    /// and both labels' `truncate` removed, the long row grows from 59.5px
    /// to 391px and the equal-height assertion fails. The right-edge
    /// assertions did not fail under that change (the text wraps instead of
    /// overflowing), so they are a floor, not the discriminating check.
    #[gpui::test]
    fn the_viewer_fits_a_narrow_pane(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
            if !cx.has_global::<theme::GlobalTheme>() {
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            }
        });
        let viewer = cx.new(|_| NostrViewer::new());
        let long = "x".repeat(600);
        let referenced: String = "f".repeat(64);
        viewer.update(cx, |viewer, cx| {
            let mut wide = event("e1", 100, &long);
            wide["tags"] = json!([["t", long]]);
            let records = kind_event(
                10002,
                "e5",
                100,
                "",
                &[&["r", "wss://nostr.mom/"], &["r", "wss://nos.lol/"]],
            );
            let more = kind_event(10051, "e6", 100, "", &[&["relay", "wss://a.example/"]]);
            let media = kind_event(
                1,
                "e7",
                30,
                "a photo",
                &[&[
                    "imeta",
                    "url https://nostr.build/i/x.jpg",
                    "blurhash 00TSUA",
                    "dim 2x1",
                ]],
            );
            let comment = kind_event(
                1111,
                "e3",
                90,
                "a comment",
                &[&["e", referenced.as_str()], &["p", referenced.as_str()]],
            );
            viewer.ingest_tool_result(
                &json!({ "events": [
                    wide, records, more, comment,
                    event("e2", 50, "short"), event("e4", 40, "short too"),
                    media,
                ] }),
                cx,
            );
            viewer.selected = Some("e1".to_string());
            // Both short notes carry a reaction count, so the
            // equal-height assertion still compares same-shape rows.
            viewer
                .reactions
                .insert("e2".to_string(), BTreeMap::from([("👍".to_string(), 12)]));
            viewer
                .reactions
                .insert("e4".to_string(), BTreeMap::from([("❤".to_string(), 1)]));
        });

        struct Host {
            viewer: Entity<NostrViewer>,
        }
        impl Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().size_full().child(self.viewer.clone())
            }
        }
        let (_, cx) = cx.add_window_view(|_window, _cx| Host {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(280.), px(600.)));
        cx.run_until_parked();

        for selector in [
            "nostr-filter-reading",
            "nostr-filter-following",
            "nostr-filter-threads",
            "nostr-filter-replies",
            "nostr-filter-everything",
            "nostr-viewer-rows",
            "nostr-viewer-detail",
            "nostr-divider-0",
            "nostr-row-1",
            "nostr-collapsed-2",
            "nostr-row-3",
            "nostr-row-4",
            "nostr-row-5",
            "nostr-row-6",
            "nostr-viewer-detail-content",
        ] {
            let bounds = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is laid out"));
            assert!(
                bounds.right() <= px(280.) + px(1.),
                "{selector} must fit a 280px pane: right edge {:?}",
                bounds.right()
            );
        }
        let short_a = cx.debug_bounds("nostr-row-4").expect("laid out");
        let short_b = cx.debug_bounds("nostr-row-5").expect("laid out");
        assert_eq!(
            short_a.size.height, short_b.size.height,
            "same-shape rows are the same height however long their notes"
        );
        let detail = cx.debug_bounds("nostr-viewer-detail").expect("laid out");
        assert!(
            detail.bottom() <= px(600.) + px(1.),
            "the selected event's detail must fit a 600px pane: bottom edge {:?}",
            detail.bottom()
        );
    }

    /// Regression pin: a loaded video inside `NostrViewer`'s 240px media box
    /// must scale its frame inside the box so the `TransportBar` (play/pause
    /// and seek slider) stays visible inside the box instead of being pushed
    /// past `overflow_hidden` and clipped off.
    #[gpui::test]
    async fn video_widget_fits_frame_and_transport_inside_box(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
            if !cx.has_global::<theme::GlobalTheme>() {
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            }
        });
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hkask-media-widget/test_data/playback-opus.mp4"
        );
        approve_gallery_media_path(std::path::Path::new(fixture))
            .expect("approve test video fixture");

        let viewer = cx.new(|_| NostrViewer::new());
        viewer.update(cx, |viewer, cx| {
            let clip = kind_event(
                1,
                "clip1",
                40,
                "Knife Fight clip",
                &[&[
                    "imeta",
                    "url https://blossom.primal.net/clip.mp4",
                    "m video/mp4",
                    "dim 1920x1080",
                ]],
            );
            viewer.ingest_tool_result(&json!({ "events": [clip] }), cx);
            viewer.load_video("clip1".to_string(), fixture.to_string(), cx);
        });

        struct Host {
            viewer: Entity<NostrViewer>,
        }
        impl Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().size_full().child(self.viewer.clone())
            }
        }
        let (_, cx) = cx.add_window_view(|_window, _cx| Host {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(520.), px(720.)));
        cx.run_until_parked();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while cx.debug_bounds("media-video-frame").is_none() && std::time::Instant::now() < deadline
        {
            let timer = cx.update(|_window, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(20))
            });
            timer.await;
            cx.run_until_parked();
        }

        let box_bounds = cx
            .debug_bounds("nostr-video-box-1")
            .expect("video wrapper box laid out");
        let frame_bounds = cx
            .debug_bounds("media-video-frame")
            .expect("video frame laid out");
        let transport_bounds = cx
            .debug_bounds("media-transport")
            .expect("transport bar laid out");

        assert!(
            frame_bounds.size.height > px(0.),
            "video frame must be visible, got height {:?}",
            frame_bounds.size.height
        );
        assert!(
            transport_bounds.size.height > px(0.),
            "transport bar must be visible, got height {:?}",
            transport_bounds.size.height
        );
        assert!(
            frame_bounds.bottom() <= box_bounds.bottom() + px(1.),
            "video frame must fit inside the 240px box (box bottom {:?}, frame bottom {:?})",
            box_bounds.bottom(),
            frame_bounds.bottom()
        );
        assert!(
            transport_bounds.bottom() <= box_bounds.bottom() + px(1.),
            "transport bar must stay inside the 240px box (box bottom {:?}, transport bottom {:?})",
            box_bounds.bottom(),
            transport_bounds.bottom()
        );
    }

    #[test]
    fn extract_lud16_reads_nostr_profile_fields_shape() {
        let canonical = r#"{
            "found": 1,
            "asked": 1,
            "profiles": [{
                "public_key_hex": "aa",
                "found": true,
                "fields": {
                    "name": "alice",
                    "lud16": " alice@getalby.com "
                }
            }]
        }"#;
        assert_eq!(
            extract_lud16_from_profile_response(canonical),
            Some("alice@getalby.com".to_string())
        );

        let missing = r#"{"profiles": [{"found": true, "fields": {"name": "bob"}}]}"#;
        assert_eq!(extract_lud16_from_profile_response(missing), None);
    }

    #[test]
    fn reaction_emoji_maps_like_and_dislike_and_tracks_own_reactions() {
        assert_eq!(reaction_emoji("+"), "👍");
        assert_eq!(reaction_emoji(""), "👍");
        assert_eq!(reaction_emoji("-"), "👎");
        assert_eq!(reaction_emoji("🔥"), "🔥");

        let mine = "a".repeat(64);
        let other = "b".repeat(64);
        let mut r1 = kind_event(7, "rx1", 100, "+", &[&["e", "note-1"]]);
        r1["author"] = json!(mine);
        let mut r2 = kind_event(7, "rx2", 101, "-", &[&["e", "note-1"]]);
        r2["author"] = json!(other);
        let rows = vec![wire_row(&r1), wire_row(&r2)];
        let own = own_reactions_from(&rows, Some(&mine));
        assert_eq!(
            own.get(&("note-1".to_string(), "👍".to_string())),
            Some(&"rx1".to_string())
        );
        assert!(!own.contains_key(&("note-1".to_string(), "👎".to_string())));
    }

    #[test]
    fn zap_receipt_totals_sums_valid_nip57_receipts_and_skips_malformed() {
        assert_eq!(bolt11_amount_sats("lnbc210n1dummy"), Some(21));
        assert_eq!(bolt11_amount_sats("lnbc10u1dummy"), Some(1_000));
        assert_eq!(bolt11_amount_sats("lnbc1dummy"), None);

        // Standard NIP-19 published test vector (never a real key).
        let key = hkask_nostr_signer::SecretKey::from_nsec(
            "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5",
        )
        .expect("test key");
        let recipient = "b".repeat(64);
        let signed_zap_req = hkask_nostr_signer::sign_zap_request(
            &key,
            1_700_000_000,
            &recipient,
            21_000,
            &["wss://nos.lol".to_string()],
            None,
            None,
            "gm",
        )
        .expect("signed zap request");
        let valid_desc = signed_zap_req.to_json();
        let forged_desc = r#"{"id":"aa","kind":9734,"pubkey":"bb","created_at":1,"tags":[],"content":"forged","sig":"cc"}"#;
        let signed_note =
            hkask_nostr_signer::sign_event(&key, 1, 1_700_000_000, "not a zap").expect("note");
        let wrong_kind_desc = signed_note.to_json();

        let make_receipt = |id: &str, target: &str, bolt11: &str, desc: &str| {
            let ev = kind_event(
                9735,
                id,
                1_700_000_000,
                "",
                &[&["e", target], &["bolt11", bolt11], &["description", desc]],
            );
            wire_row(&ev)
        };
        let rows = vec![
            make_receipt("z1", "note-a", "lnbc210n1qq", &valid_desc),
            make_receipt("z2", "note-a", "lnbc1u1qq", &valid_desc),
            make_receipt("z3", "note-a", "lnbc5u1qq", &wrong_kind_desc),
            make_receipt("z4", "note-a", "lnbc5u1qq", forged_desc),
            make_receipt("z5", "note-b", "lnbc10u1qq", &valid_desc),
        ];
        let totals = zap_receipt_totals(&rows);
        assert_eq!(totals.get("note-a"), Some(&(2, 121)));
        assert_eq!(totals.get("note-b"), Some(&(1, 1_000)));
        assert_eq!(zap_totals_text(2, 121), "⚡ 121 sats (2)");
        assert_eq!(zap_totals_text(1, 1_000), "⚡ 1000 sats");
    }
}
