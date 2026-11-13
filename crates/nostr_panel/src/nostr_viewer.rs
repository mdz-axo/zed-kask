//! The Nostr panel's upper region: what the Nostr tools returned, as events.
//!
//! The concept model, pinned. Every view shows one of these and nothing
//! else:
//!
//! - **Event** — a verified NIP-01 object: id, author, claimed time, kind,
//!   tags, content, and where it came from.
//! - **Feed** — a named query and the events it returned, newest first. It
//!   is never a ranking. The first feed is the local relay's archive.
//! - **Conversation** — a root note and its replies in reading order, with
//!   the notes it refers to that were not found.
//! - **Relay** — the local relay's state and its admission decisions.
//!
//! There is no follow, like, repost or compose affordance here: the tools
//! behind this panel only read.
//!
//! Three rules:
//!
//! - **Everything shown traces to a tool result.** The viewer holds no
//!   Nostr state of its own. It reads the archive through the governed
//!   tool invoker and it ingests the conversation's tool results by their
//!   shape, so it shows what the tools produced, not what the model said.
//! - **Third-party text is shown as text.** Content, tags and relay
//!   messages are drawn as plain characters, with control characters made
//!   visible. Nothing is rendered as markup, no link is opened and no image
//!   is fetched.
//! - **The network is contacted only by a labelled click.** Loading the
//!   archive and the relay's state reads this machine. Reading a
//!   conversation asks the seed relays, and its button says so.

use std::collections::HashSet;

use acp_thread::AgentThreadEntry;
use agent_client_protocol::v2::schema::ToolCallId;
use gpui::{Context, DefiniteLength, Entity, Render, SharedString, Task, WeakEntity, Window, px};
use hkask_tool_invoker::InvokeError;
use hkask_types::nostr_wire::{NOSTR_CLIENT_SERVER, NOSTR_RELAY_SERVER, WireEvent};
use serde::Deserialize;
use serde_json::{Value, json};
use ui::{Label, LabelSize, prelude::*};
use util::ResultExt as _;

/// The tools the viewer dispatches itself. Pinned against the servers'
/// lists by `dispatched_tools_are_the_servers_own`.
const ARCHIVE_TOOL: &str = "nostr_relay_query";
const STATUS_TOOL: &str = "nostr_relay_status";
const LOG_TOOL: &str = "nostr_relay_log";
const THREAD_TOOL: &str = "nostr_thread";

/// Events asked for when the archive is loaded: the tool's own maximum.
const ARCHIVE_PAGE: usize = 100;
/// Admission decisions asked for.
const LOG_PAGE: usize = 50;
/// The selected event's share of the view's height.
const DETAIL_FRACTION: f32 = 0.45;
/// Tags listed for the selected event before the rest are counted.
const DETAIL_TAGS: usize = 16;
/// The deepest reply indent drawn; deeper replies sit at this indent.
const MAX_INDENT_LEVELS: usize = 6;

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
    at: u64,
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
    Conversation,
    Relay,
}

pub struct NostrViewer {
    tab: ViewerTab,
    feed: Option<Feed>,
    conversation: Option<Conversation>,
    relay: Option<RelayReading>,
    /// The selected event's id, in the view it was selected in.
    selected: Option<String>,
    archive_request: Request,
    thread_request: Request,
    relay_request: Request,
    started: bool,
    /// The conversation whose tool results are ingested.
    thread: Option<WeakEntity<acp_thread::AcpThread>>,
    processed_tool_results: HashSet<ToolCallId>,
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

/// A Unix time as UTC text. An event's time is what its author claimed.
fn utc_text(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| format!("{seconds} (not a date)"))
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

/// A sync result that stored something: the archive on screen is stale.
fn archive_changed(content: &Value) -> bool {
    content.get("fetched").is_some()
        && content.get("stored").and_then(Value::as_u64).unwrap_or(0) > 0
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
            conversation: None,
            relay: None,
            selected: None,
            archive_request: Request::new(),
            thread_request: Request::new(),
            relay_request: Request::new(),
            started: false,
            thread: None,
            processed_tool_results: HashSet::new(),
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
        cx.notify();
    }

    /// Reload what the current view shows from this machine.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        match self.tab {
            ViewerTab::Relay => self.load_relay(cx),
            ViewerTab::Timeline | ViewerTab::Conversation => self.load_archive(cx),
        }
    }

    // ── The archive feed ────────────────────────────────────────────

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
            self.show_feed(feed);
        }
        cx.notify();
    }

    fn show_feed(&mut self, feed: Feed) {
        if self.tab == ViewerTab::Timeline
            && !feed
                .rows
                .iter()
                .any(|row| Some(&row.event.id) == self.selected.as_ref())
        {
            self.selected = None;
        }
        self.feed = Some(feed);
    }

    // ── A conversation ──────────────────────────────────────────────

    /// Ask the client server for the conversation `event_id` belongs to.
    /// This contacts the local relay and the seed relays.
    fn read_conversation(&mut self, event_id: String, cx: &mut Context<Self>) {
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
                self.show_feed(feed);
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

    fn selected_row(&self) -> Option<&Row> {
        let id = self.selected.as_ref()?;
        let rows = match self.tab {
            ViewerTab::Timeline => &self.feed.as_ref()?.rows,
            ViewerTab::Conversation => &self.conversation.as_ref()?.rows,
            ViewerTab::Relay => return None,
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

    fn row(
        &self,
        ix: usize,
        row: &Row,
        indent: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = row.event.id.clone();
        let selected = self.selected.as_ref() == Some(&id);
        let mut heading = format!(
            "{} · {} · kind {}",
            utc_text(row.event.created_at),
            short_hex(&row.event.author),
            row.event.kind
        );
        if let Some(label) = &row.label {
            heading.push_str(&format!(" · {label}"));
        }
        if row.parent_missing {
            heading.push_str(" · parent not found");
        }
        if row.is_focus {
            heading.push_str(" · asked for");
        }
        let depth = if indent {
            row.depth.min(MAX_INDENT_LEVELS)
        } else {
            0
        };
        let first_line = row.event.content.lines().next().unwrap_or_default();
        div()
            .id(SharedString::from(format!("nostr-row-{ix}")))
            .debug_selector(move || format!("nostr-row-{ix}"))
            .w_full()
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
                Label::new(heading)
                    .size(LabelSize::XSmall)
                    .color(if row.parent_missing {
                        Color::Warning
                    } else {
                        Color::Muted
                    })
                    .truncate(),
            )
            .child(
                Label::new(display_text(first_line))
                    .size(LabelSize::Small)
                    .truncate(),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(id.clone());
                cx.notify();
            }))
    }

    fn rows(&self, rows: &[Row], indent: bool, cx: &Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .id("nostr-viewer-rows")
            .debug_selector(|| "nostr-viewer-rows".into())
            .w_full()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .gap_0p5()
            .p_1()
            .children(
                rows.iter()
                    .enumerate()
                    .map(|(ix, row)| self.row(ix, row, indent, cx)),
            )
    }

    /// The selected event, whole: the fields a relay sent, and where the
    /// event came from.
    fn detail(&self, row: &Row, cx: &Context<Self>) -> impl IntoElement + use<> {
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
        v_flex()
            .id("nostr-viewer-detail")
            .debug_selector(|| "nostr-viewer-detail".into())
            .w_full()
            .min_w_0()
            .min_h_0()
            .h(DefiniteLength::Fraction(DETAIL_FRACTION))
            .flex_shrink_0()
            .overflow_y_scroll()
            .border_t_1()
            .border_color(cx.theme().colors().border_variant)
            .p_2()
            .gap_1()
            .child(Self::line(format!("id  {}", event.id), Color::Muted))
            .child(Self::line(format!("author  {}", event.author), Color::Muted))
            .child(Self::line(
                format!(
                    "kind {} · {} as the author claims ({})",
                    event.kind,
                    utc_text(event.created_at),
                    event.created_at
                ),
                Color::Muted,
            ))
            .child(Self::line(display_text(&source), Color::Hint))
            // Only a note has a conversation, and only the client server
            // can read one.
            .when(event.kind == 1, |el| {
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
                    .child(display_text(&event.content)),
            )
    }

    fn render_timeline(&self, cx: &Context<Self>) -> AnyElement {
        let Some(feed) = &self.feed else {
            return v_flex()
                .size_full()
                .children(Self::request_line(
                    &self.archive_request,
                    "Reading the local archive…",
                ))
                .child(Self::empty(
                    "No events loaded",
                    "The local relay's archive has not been read yet.",
                ))
                .into_any_element();
        };
        let heading = format!("{} · {} events, newest first", feed.name, feed.rows.len());
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .child(Self::line(heading, Color::Default))
                    .child(Self::remarks(&feed.remarks))
                    .children(Self::request_line(
                        &self.archive_request,
                        "Reading the local archive…",
                    )),
            )
            .child(if feed.rows.is_empty() {
                Self::empty(
                    "No events",
                    "The query returned nothing. An empty archive means the relay has stored no event by an allowlisted author.",
                )
                .into_any_element()
            } else {
                self.rows(&feed.rows, false, cx).into_any_element()
            })
            .children(self.selected_row().map(|row| self.detail(row, cx)))
            .into_any_element()
    }

    fn render_conversation(&self, cx: &Context<Self>) -> AnyElement {
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
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
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
            .child(self.rows(&conversation.rows, true, cx))
            .children(self.selected_row().map(|row| self.detail(row, cx)))
            .into_any_element()
    }

    fn render_relay(&self, _cx: &Context<Self>) -> AnyElement {
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
            .p_2()
            .gap_1()
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
                        utc_text(decision.at),
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
}

impl Render for NostrViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.tab {
            ViewerTab::Timeline => self.render_timeline(cx),
            ViewerTab::Conversation => self.render_conversation(cx),
            ViewerTab::Relay => self.render_relay(cx),
        };
        v_flex()
            .size_full()
            .min_w_0()
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
                    .child(self.tab_button(ViewerTab::Conversation, "Conversation".into(), cx))
                    .child(self.tab_button(ViewerTab::Relay, "Relay".into(), cx))
                    .child(
                        div().ml_auto().flex_shrink_0().child(
                            ui::IconButton::new("nostr-viewer-refresh", ui::IconName::RotateCw)
                                .icon_size(ui::IconSize::Small)
                                .tooltip(ui::Tooltip::text(
                                    "Reload from the local relay. Contacts no other relay.",
                                ))
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        ),
                    ),
            )
            .child(div().w_full().flex_1().min_h_0().min_w_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, size};
    use hkask_types::nostr_wire::{CLIENT_TOOL_NAMES, RELAY_TOOL_NAMES};

    fn event(id: &str, at: u64, content: &str) -> Value {
        json!({
            "id": id,
            "author": "a".repeat(64),
            "created_at": at,
            "kind": 1,
            "content": content,
            "content_truncated": false,
            "tags": [["e", "b".repeat(64), "", "root"]],
            "tags_truncated": false,
        })
    }

    /// A tool the viewer names that a server does not have fails at
    /// dispatch as "tool not found".
    #[test]
    fn dispatched_tools_are_the_servers_own() {
        for tool in [ARCHIVE_TOOL, STATUS_TOOL, LOG_TOOL] {
            assert!(RELAY_TOOL_NAMES.contains(&tool), "{tool}");
        }
        assert!(CLIENT_TOOL_NAMES.contains(&THREAD_TOOL));
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
        assert_eq!(utc_text(0), "1970-01-01 00:00 UTC");
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

    /// expect: the viewer fits a narrow pane. A long unbroken line of
    /// third-party text must not push a row or the selected event's content
    /// past the pane's edge, and must not make a row taller than its two
    /// lines: one long note would otherwise push every other note off
    /// screen.
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
        viewer.update(cx, |viewer, cx| {
            let mut wide = event("e1", 100, &long);
            wide["tags"] = json!([["t", long]]);
            viewer.ingest_tool_result(&json!({ "events": [wide, event("e2", 50, "short")] }), cx);
            viewer.selected = Some("e1".to_string());
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
            "nostr-viewer-rows",
            "nostr-viewer-detail",
            "nostr-row-0",
            "nostr-row-1",
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
        let long_row = cx.debug_bounds("nostr-row-0").expect("laid out");
        let short_row = cx.debug_bounds("nostr-row-1").expect("laid out");
        assert_eq!(
            long_row.size.height, short_row.size.height,
            "a row is the same height however long its note"
        );
        let detail = cx.debug_bounds("nostr-viewer-detail").expect("laid out");
        assert!(
            detail.bottom() <= px(600.) + px(1.),
            "the selected event's detail must fit a 600px pane: bottom edge {:?}",
            detail.bottom()
        );
    }
}
