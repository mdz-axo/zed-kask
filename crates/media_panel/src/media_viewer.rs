//! The media panel's viewing pane — five tabs over real state.
//!
//! - **Media**: the selected asset rendered large via the D18 viz block
//!   renderer (the same widget the conversation renders inline).
//! - **Library**: the gallery's actual assets (via `gallery_list_assets`)
//!   merged with assets surfaced by the conversation's tool results, with
//!   delete (two-step confirm) affordances. Each fetched page reconciles
//!   the list — assets deleted from the gallery drop out of it.
//! - **Queue**: research jobs (via `job_list`) with cancel affordances.
//! - **Detail**: the selected asset's inspector data (via
//!   `gallery_asset_detail`) — record, tags, lineage.
//! - **Transcript**: the recordings browser, and the transcript workspace —
//!   synced reading, word-click seek, two-click select → label editor →
//!   researcher highlight create/rename/delete, and reel compose+render.
//!
//! All tool calls go through the process-global `ToolInvoker` (the governed
//! McpRuntime dispatch) against the `media` server. Failures surface in a
//! status line — visible, never silent.

use std::{collections::HashSet, ops::Range, sync::Arc, time::Duration};

use acp_thread::AgentThreadEntry;
use agent_client_protocol::schema::v2::ToolCallId;
use gpui::{
    Context, Entity, HighlightStyle, InteractiveText, KeyDownEvent, Keystroke, ListAlignment,
    ListOffset, ListState, Render, SharedString, StrikethroughStyle, StyledText, Task, Window,
};
use hkask_mcp_media::{
    tools::jobs::parse_job_list_response,
    types::{JobListPayload, JobRecord},
};
use serde_json::Value;
use ui::{Icon, IconName, Label, LabelSize, prelude::*};
use util::ResultExt as _;

/// Server that hosts the media tools — the `BUILT_IN_MCP_SERVERS` id, the
/// same id used for direct viewer-initiated media tool dispatch.
const MEDIA_SERVER: &str = "media";
const GALLERY_PAGE_LIMIT: usize = 100;
const QUEUE_POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Follow-along polling cadence (S3c) — 4 Hz marks the containing word
/// responsively without saturating the render loop.
const FOLLOW_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The workspace list's fixed header items before the transcript blocks:
/// back row, player, mode chips, alignment note, and the Transcript
/// section label — the whole workspace scrolls as one piece, exactly as it
/// did before virtualization (a pinned header taller than a tight pane
/// would starve the reading area).
const WORKSPACE_HEADER_ITEMS: usize = 5;

/// The workspace list's fixed tail items after the transcript blocks:
/// highlights, cuts, and reel actions — always present as items, so the
/// item count (and with it the reading position) stays stable while the
/// operator curates annotations.
const WORKSPACE_LIST_TAIL_ITEMS: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
enum RequestOwner {
    GalleryPage(usize),
    Jobs,
    Detail(String),
    Edit(&'static str),
    DetailTranscripts(String),
    DetailAnnotations(String),
    Transcript(String),
    TranscriptListing,
}

/// A strict-parsed `educt_list_transcripts` row — the fields the Detail
/// and Recordings views render; a missing or malformed field rejects the
/// whole listing.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TranscriptSummaryRow {
    id: String,
    media_path: String,
    words_count: u64,
    duration_secs: u64,
    has_word_timings: bool,
    /// The five-state alignment (original, aligned, re_anchored, unaligned,
    /// untimed) — what the row's capability note keys on.
    alignment: String,
}

/// A working-view word as the pane renders it (S3a renders paragraph text
/// blocks; S3b's hit-testing element consumes the same words).
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneWord {
    word: String,
    start_ms: u64,
    end_ms: u64,
}

/// A speaker span over working word indices (the latest speaker layer).
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneSpeakerSpan {
    start_word: usize,
    end_word: usize,
    speaker: String,
}

/// One row of the Layers strip — kind + creation time only (provenance and
/// payloads stay in the raw layer records the tools return).
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneLayerRow {
    id: String,
    kind: String,
    created_at: String,
}

/// A highlight entry over source word indices — overlay and list data,
/// extracted from the payload's highlight layers (researcher annotations
/// and model passes both store as highlight layers). `annotation_id` is the
/// storing layer's id (the rename and delete affordances' address);
/// `researcher_supplied` marks entries whose layer was authored by the
/// user — only those rows offer rename and delete. `note` is the entry's
/// stored note (a rename passes it through unchanged).
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneHighlight {
    start_word: usize,
    end_word: usize,
    label: String,
    note: String,
    annotation_id: String,
    researcher_supplied: bool,
}

/// A researcher strike (R2): an unannotated cut over words
/// `start_word..=end_word`, parsed from single-Cut researcher EDL layers
/// under the server's strike identity — multi-op compose EDLs and model
/// EDLs never render as strikes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneStrike {
    start_word: usize,
    end_word: usize,
    annotation_id: String,
}

/// A researcher tag/note row for the Detail tab's annotation surface
/// (strict-parsed from `gallery_annotation_list` — the direct-manipulation
/// CRUD section, 2026-10-27).
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneAnnotation {
    annotation_id: String,
    kind: String,
    value: String,
    created_at: String,
}

/// The Detail tab's open annotation editor: a new tag/note or an existing
/// row's value being replaced. Mirrors the transcript label editor's
/// modal pattern — Enter confirms, Escape cancels.
#[derive(Clone, Debug)]
struct AnnotationEdit {
    kind: &'static str,
    /// `None` creates a new row; `Some` replaces that annotation's value.
    annotation_id: Option<String>,
    buffer: LabelBuffer,
}

/// The strict-parsed `educt_get_transcript` payload the Transcript tab
/// renders (S3a): working words + alignment, the latest paragraph layer's
/// breaks, the latest speaker layer's spans, and the layer summary strip —
/// all from the single dispatch (D2). `words` is `None` on text-only views
/// (Unaligned/Untimed) — those render `working_text` with a visible status
/// note instead.
#[derive(Clone, Debug, PartialEq)]
struct TranscriptPaneState {
    transcript_id: String,
    media_path: String,
    alignment: String,
    alignment_note: Option<String>,
    words: Option<Vec<PaneWord>>,
    working_text: String,
    paragraph_breaks: Option<Vec<usize>>,
    speaker_spans: Option<Vec<PaneSpeakerSpan>>,
    review: Option<PaneReview>,
    layers: Vec<PaneLayerRow>,
    highlights: Vec<PaneHighlight>,
    strikes: Vec<PaneStrike>,
}

/// A rendered paragraph block of the workspace (S3b-2): its text, the char
/// range of each word within that text, and each word's global working-view
/// index and start time — the click-to-seek and follow-along mapping
/// tables, aligned by position.
#[derive(Clone, Debug, Default, PartialEq)]
struct PaneBlock {
    speaker: Option<String>,
    text: String,
    word_char_ranges: Vec<Range<usize>>,
    word_starts: Vec<(usize, u64)>,
}

/// The derived review view's pane projection (task 1a): per-block flags
/// from the server's deterministic derivation — the pane renders them, it
/// never re-derives (D5). `None` means the payload carried no review view
/// (a server predating the field).
#[derive(Clone, Debug, PartialEq)]
struct PaneReview {
    threshold: f64,
    blocks: Vec<PaneReviewFlag>,
    note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct PaneReviewFlag {
    start_word: usize,
    end_word: usize,
    min_confidence: Option<f64>,
    status: PaneReviewStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaneReviewStatus {
    Flagged,
    Ok,
    NoConfidenceRecorded,
}

impl PaneReviewStatus {
    /// Strict-parse the wire string; an unknown status rejects the payload
    /// rather than rendering a guess.
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "flagged" => Ok(Self::Flagged),
            "ok" => Ok(Self::Ok),
            "no-confidence-recorded" => Ok(Self::NoConfidenceRecorded),
            other => Err(format!("review block has unknown status \"{other}\".")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ResourceState {
    Idle,
    Loading,
    Ready,
    Failed(String),
}

#[derive(Clone, Debug)]
struct RequestLifecycle {
    epoch: u64,
    owner: Option<RequestOwner>,
    state: ResourceState,
}

impl RequestLifecycle {
    fn new() -> Self {
        Self {
            epoch: 0,
            owner: None,
            state: ResourceState::Idle,
        }
    }

    /// expect: Repeated requests for the resource currently loading do not duplicate work.
    /// [P1] Motivating: each resource has one current request owner.
    /// pre: owner identifies the requested resource value.
    /// post: a new epoch is returned exactly when the owner supersedes the current request.
    fn begin(&mut self, owner: RequestOwner) -> Option<u64> {
        if self.state == ResourceState::Loading && self.owner.as_ref() == Some(&owner) {
            return None;
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.owner = Some(owner);
        self.state = ResourceState::Loading;
        Some(self.epoch)
    }

    fn owns(&self, epoch: u64) -> bool {
        self.epoch == epoch && self.state == ResourceState::Loading
    }

    fn complete(&mut self, epoch: u64, result: Result<(), String>) -> bool {
        if !self.owns(epoch) {
            return false;
        }
        self.owner = None;
        self.state = match result {
            Ok(()) => ResourceState::Ready,
            Err(error) => ResourceState::Failed(error),
        };
        true
    }

    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.owner = None;
        self.state = ResourceState::Idle;
    }
}

#[derive(Clone, Debug)]
struct GalleryListing {
    gallery_id: String,
    total: u64,
    offset: usize,
    limit: usize,
    assets: Vec<Value>,
}

type JobListing = JobListPayload;

/// One media asset — from the gallery listing or surfaced by a tool result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    /// The media-block body (JSON) — handed to the viz block renderer.
    pub body: String,
    /// Asset src (path or URL) for the library list label.
    pub src: String,
    /// Media kind ("image" | "video" | "audio") for the icon.
    pub kind: String,
    /// The tool that surfaced the asset (provenance label).
    pub tool: SharedString,
    /// The gallery index other gallery tools accept (`image_index`).
    /// `None` for conversation-surfaced assets not yet in the gallery index.
    pub gallery_index: Option<usize>,
    /// The stable asset id from the listing record — the identity that
    /// survives reordering, rescan, and gallery switches. Detail and delete
    /// actions address the asset by this id; the positional index is display
    /// data only. `None` for conversation-surfaced assets.
    pub gallery_asset_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerTab {
    Media,
    Library,
    Queue,
    Detail,
    Transcript,
}

/// The shared two-click delete confirmation (media-refactor plan R4): the
/// first click arms a row's stable identity, the second click on the SAME
/// row confirms (consumes the arm and reports the confirming click), and a
/// click on a different row re-aims at that row — a confirm never acts on
/// the row beside the armed one. One pattern serves every row family
/// (Library asset indexes, transcript recording ids); disarm on any path
/// that can invalidate the aimed row (listing reloads, tab switches,
/// dispatches) so a confirm is never aimed at a row that no longer exists.
#[derive(Clone, Debug, Default)]
struct ConfirmArm<K> {
    /// The armed row key. The listing reconcile re-aims this directly when
    /// it follows an armed row across a merge (the key shifts position).
    armed: Option<K>,
}

impl<K: Clone + Eq> ConfirmArm<K> {
    /// A row click: consume the arm and report the confirming click when
    /// `key` is the armed row; any other row (including the first click
    /// on a disarmed list) arms that row instead.
    fn confirm(&mut self, key: &K) -> bool {
        if self.armed.as_ref() == Some(key) {
            self.armed = None;
            true
        } else {
            self.armed = Some(key.clone());
            false
        }
    }

    /// Disarm without dispatching.
    fn disarm(&mut self) {
        self.armed = None;
    }

    /// Whether `key` is the armed row (renders the Confirm delete state).
    fn is_armed(&self, key: &K) -> bool {
        self.armed.as_ref() == Some(key)
    }
}

pub struct MediaViewer {
    assets: Vec<MediaAsset>,
    selected: Option<usize>,
    active_tab: ViewerTab,
    /// The observed conversation thread (weak — the panel owns it), kept so
    /// `refresh` can re-ingest tool-result display hints without the panel
    /// forwarding the entity again.
    thread: Option<gpui::WeakEntity<acp_thread::AcpThread>>,
    /// The media widget for the currently-selected asset on the Media tab.
    /// Owned directly (not via the strong viz cache) so the edit toolbar can
    /// reach its playback clock and trim marks. A changed selection resolves
    /// through the stable-Asset weak registry before replacing the entity.
    media_widget: Option<Entity<hkask_media_widget::MediaWidget>>,
    media_widget_body: Option<String>,
    /// Asset srcs queued for concatenation (`video_concat`). Two or more
    /// queue entries enable the Concat button.
    concat_queue: Vec<String>,
    /// Total asset count in the gallery (from `gallery_list_assets`).
    gallery_total: Option<u64>,
    /// Gallery listing pagination cursor and server-confirmed page size.
    gallery_offset: usize,
    gallery_limit: usize,
    /// The gallery the current listing belongs to (from `gallery_list_assets`
    /// responses). A response carrying a different id is a root change: the
    /// previous gallery's indexed rows and pending actions are invalid.
    gallery_id: Option<String>,
    gallery_request: RequestLifecycle,
    jobs: Vec<JobRecord>,
    jobs_total: Option<usize>,
    jobs_limit: usize,
    jobs_has_more: bool,
    jobs_request: RequestLifecycle,
    processed_job_results: HashSet<String>,
    queue_poll_task: Option<Task<()>>,
    /// Inspector data for the selected asset (from `gallery_asset_detail`).
    detail: Option<Value>,
    detail_request: RequestLifecycle,
    /// Transcript summaries for the selected asset's Detail view (from
    /// `educt_list_transcripts`). `None` until the request completes;
    /// `Some(vec![])` means loaded-and-none — the section renders only for
    /// non-empty lists.
    detail_transcripts: Option<Vec<TranscriptSummaryRow>>,
    detail_transcripts_request: RequestLifecycle,
    /// Researcher tags/notes for the selected asset (from
    /// `gallery_annotation_list`) — the Detail tab's direct annotation
    /// CRUD surface (2026-10-27).
    detail_annotations: Option<Vec<PaneAnnotation>>,
    detail_annotations_request: RequestLifecycle,
    /// The Detail tab's open annotation editor and its input's focus
    /// handle (lazily created on open, like the transcript label editor's).
    annotation_edit: Option<AnnotationEdit>,
    annotation_input_focus: Option<gpui::FocusHandle>,
    /// The open transcript workspace (from `educt_get_transcript`) — `None`
    /// until the request completes; `Some` renders the Transcript tab. The
    /// state persists across tab switches so returning to an open
    /// transcript does not refetch.
    transcript: Option<TranscriptPaneState>,
    transcript_request: RequestLifecycle,
    /// The follow-along mark (S3c): the global working-word index containing
    /// the playback position, cleared when playback stops or the tab leaves.
    current_word: Option<usize>,
    /// The follow-along polling task — one per open workspace; cancelled by
    /// taking the handle (tab leave, transcript change, viewer drop).
    transcript_follow_task: Option<Task<()>>,
    /// The workspace's virtualized content list (S3d): transcript blocks
    /// plus the highlight, cut, and reel sections as tail items. Owned
    /// here so the follow task can scroll it statefully — no window
    /// needed for a deferred reveal.
    transcript_list: ListState,
    /// The item count last pushed to `transcript_list` — the reset guard,
    /// so a re-render never drops the reading position unless the content
    /// actually changed.
    transcript_list_count: usize,
    /// Whether the follow-along keeps the marked block visible (S3d). A
    /// manual scroll suspends it; the Follow toggle re-enables and
    /// reveals the current mark.
    follow_viewport: bool,
    /// The list offset the follow tick last observed. Any change not
    /// accounted for by a pending follow reveal is a manual scroll and
    /// suspends tracking.
    follow_scroll_baseline: ListOffset,
    /// Follow reveals not yet absorbed by a tick — programmatic scrolls
    /// the tick must not misread as manual scrolling.
    follow_reveals_in_flight: u32,
    /// The recordings listing for the Transcript tab's idle state (from
    /// `educt_list_transcripts` unfiltered) — the browse surface the tab
    /// opens with when no workspace transcript is loaded.
    transcript_listing: Option<Vec<TranscriptSummaryRow>>,
    transcript_listing_request: RequestLifecycle,
    /// S4 part-2: when on, word clicks build highlight ranges instead of
    /// seeking (a mode toggle — every-click-seek plus accidental creation
    /// is worse than an explicit select mode).
    selection_mode: bool,
    /// The first click of a pending two-click selection (a global working
    /// word index).
    selection_anchor: Option<usize>,
    /// S4-3: the open label editor — a completed range awaiting its label
    /// (create) or a researcher highlight being renamed. Word clicks are
    /// inert while it is open; Enter confirms, Escape cancels.
    highlight_edit: Option<HighlightEdit>,
    /// The label editor input's focus handle, created lazily on first open
    /// (pure tests construct the viewer without a GPUI context).
    label_input_focus: Option<gpui::FocusHandle>,
    /// S5 reel editing: which researcher-authored clips (start, end) the
    /// next compose gathers — rebuilt to ALL researcher-authored pane
    /// selections whenever the workspace loads, toggled per clip by the
    /// operator (membership keys on provenance, never the label text).
    reel_selection: HashSet<(usize, usize)>,
    edit_request: RequestLifecycle,
    edit_progress: Option<String>,
    /// Two-step delete confirmation for Library rows: the armed asset row
    /// index (the one shared confirm-arm pattern — see `ConfirmArm`).
    confirm_delete: ConfirmArm<usize>,
    /// Two-step delete confirmation for the Transcript tab's recordings:
    /// the armed transcript id (the same confirm-arm pattern).
    confirm_delete_transcript: ConfirmArm<String>,
    /// Visible status line (errors from tool dispatch, notices).
    status: Option<String>,
    /// Tool-call ids whose results `ingest_thread` has already examined. A
    /// call's result lands once (ACP `update_fields` sets `raw_output` when
    /// the result arrives), so a first-seen id marks a newly-completed call
    /// — the signal that a gallery-mutating tool finished and the Library
    /// listing is stale. Cleared when the observed thread changes.
    processed_tool_results: HashSet<ToolCallId>,
}

impl MediaViewer {
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            selected: None,
            active_tab: ViewerTab::Media,
            thread: None,
            media_widget: None,
            media_widget_body: None,
            concat_queue: Vec::new(),
            gallery_total: None,
            gallery_offset: 0,
            gallery_limit: GALLERY_PAGE_LIMIT,
            gallery_id: None,
            gallery_request: RequestLifecycle::new(),
            jobs: Vec::new(),
            jobs_total: None,
            jobs_limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
            jobs_has_more: false,
            jobs_request: RequestLifecycle::new(),
            processed_job_results: HashSet::new(),
            queue_poll_task: None,
            detail: None,
            detail_request: RequestLifecycle::new(),
            detail_transcripts: None,
            detail_transcripts_request: RequestLifecycle::new(),
            detail_annotations: None,
            detail_annotations_request: RequestLifecycle::new(),
            annotation_edit: None,
            annotation_input_focus: None,
            transcript: None,
            transcript_request: RequestLifecycle::new(),
            current_word: None,
            transcript_follow_task: None,
            transcript_list: ListState::new(0, ListAlignment::Top, px(1000.)),
            transcript_list_count: 0,
            follow_viewport: true,
            follow_scroll_baseline: ListOffset::default(),
            follow_reveals_in_flight: 0,
            transcript_listing: None,
            transcript_listing_request: RequestLifecycle::new(),
            selection_mode: false,
            selection_anchor: None,
            highlight_edit: None,
            label_input_focus: None,
            reel_selection: HashSet::new(),
            edit_request: RequestLifecycle::new(),
            edit_progress: None,
            confirm_delete: ConfirmArm::default(),
            confirm_delete_transcript: ConfirmArm::default(),
            status: None,
            processed_tool_results: HashSet::new(),
        }
    }

    fn suspend_media_widget(&mut self, cx: &mut Context<Self>) {
        if let Some(widget) = &self.media_widget {
            widget.update(cx, |widget, cx| widget.suspend(cx));
        }
    }

    fn clear_media_widget(&mut self, cx: &mut Context<Self>) {
        self.suspend_media_widget(cx);
        self.media_widget = None;
        self.media_widget_body = None;
    }

    fn set_selection(&mut self, selected: Option<usize>, cx: &mut Context<Self>) {
        let next_body = selected
            .and_then(|index| self.assets.get(index))
            .map(|asset| asset.body.as_str());
        if self.media_widget_body.as_deref() != next_body {
            self.clear_media_widget(cx);
        }
        self.selected = selected;
        self.detail = None;
        self.detail_request.invalidate();
        self.detail_transcripts = None;
        self.detail_transcripts_request.invalidate();
        self.detail_annotations = None;
        self.detail_annotations_request.invalidate();
        self.annotation_edit = None;
    }

    fn stop_queue_polling(&mut self) {
        self.queue_poll_task.take();
    }

    fn activate_tab(&mut self, tab: ViewerTab, cx: &mut Context<Self>) {
        if self.active_tab == ViewerTab::Media && tab != ViewerTab::Media {
            self.suspend_media_widget(cx);
        }
        if self.active_tab == ViewerTab::Transcript && tab != ViewerTab::Transcript {
            self.suspend_media_widget(cx);
            self.transcript_follow_task.take();
            self.current_word = None;
            self.selection_mode = false;
            self.selection_anchor = None;
        }
        if self.active_tab == ViewerTab::Queue && tab != ViewerTab::Queue {
            self.stop_queue_polling();
            self.jobs_request.invalidate();
        }
        if self.active_tab == ViewerTab::Detail && tab != ViewerTab::Detail {
            self.detail_request.invalidate();
            self.detail_transcripts = None;
            self.detail_transcripts_request.invalidate();
            self.detail_annotations = None;
            self.detail_annotations_request.invalidate();
            self.annotation_edit = None;
        }
        self.active_tab = tab;
        self.confirm_delete.disarm();
        self.confirm_delete_transcript.disarm();
        match tab {
            ViewerTab::Library => self.load_gallery(cx),
            ViewerTab::Queue => self.load_jobs(cx),
            ViewerTab::Detail => {
                self.load_detail(cx);
                self.load_detail_transcripts(cx);
                self.load_detail_annotations(cx);
            }
            ViewerTab::Media => {}
            ViewerTab::Transcript => {
                if self.transcript.is_some() {
                    self.start_transcript_follow(cx);
                } else {
                    self.load_transcript_listing(cx);
                }
            }
        }
        cx.notify();
    }

    /// Scan an `AcpThread`'s tool results for display hints and merge the
    /// extracted assets. Deduplicates by body. A newly-seen asset
    /// auto-selects: the latest result is what the operator wants to see.
    /// A newly-completed gallery-mutating tool call reloads the Library
    /// listing so chat-driven mutations (delete, organize, imports) are
    /// reflected without a manual refresh.
    pub fn ingest_thread(&mut self, thread: Entity<acp_thread::AcpThread>, cx: &mut Context<Self>) {
        // A different thread (resume/open) resets completion tracking —
        // the new thread's historical calls are all first-seen again.
        if self.thread.as_ref().and_then(|weak| weak.upgrade()) != Some(thread.clone()) {
            self.processed_tool_results.clear();
        }
        self.thread = Some(thread.downgrade());
        let mut gallery_mutated = false;
        let mut tool_results = Vec::new();
        {
            let thread = thread.read(cx);
            for entry in thread.entries() {
                let AgentThreadEntry::ToolCall(call) = entry else {
                    continue;
                };
                let Some(output) = call.raw_output.as_ref() else {
                    continue; // still running — no result to ingest yet
                };
                let tool: SharedString = call.tool_name.clone().unwrap_or_else(|| "tool".into());
                // First sight of a completed result carrying the server's
                // gallery-change contract makes the cached Library stale.
                if self.processed_tool_results.insert(call.id.clone())
                    && tool_result_changed_gallery(output)
                {
                    gallery_mutated = true;
                }
                tool_results.push((output.clone(), tool));
            }
        }
        // Release the thread read before borrowing the viewer context to notify.
        for (output, tool) in tool_results {
            self.ingest_tool_result(&output, &tool, cx);
        }

        // Only while the Library is on screen: switching to the tab already
        // reloads it (`tab_button`), so reloading from other tabs would be
        // wasted dispatches.
        if gallery_mutated && self.active_tab == ViewerTab::Library {
            self.reload_gallery(cx);
        }
    }

    // ── Tool invocations (governed McpRuntime dispatch) ────────────────────

    /// Merge a tool result's display hints into the asset list and select
    /// the newest — the same ingestion `ingest_thread` applies to thread
    /// tool results, reused for viewer-initiated edits so a trimmed clip or
    /// concatenation surfaces immediately.
    /// expect: Show each completed artifact regardless of the tool transport.
    /// [P7] Motivating: structured and text transports share the viewer ingress.
    /// pre: output is an ACP raw output or an invoker response.
    /// post: new valid assets are selected and notify GPUI observers;
    /// repeated bodies neither duplicate nor request a redraw.
    pub fn ingest_tool_result(&mut self, output: &Value, tool: &str, cx: &mut Context<Self>) {
        let gallery_changed = tool_result_changed_gallery(output);
        let hints = match output {
            Value::String(text) => hkask_types::tool_response::display_hints_from_output_text(text),
            value => hkask_types::tool_response::display_hints_from_output_value(value),
        };
        let tool: SharedString = tool.into();
        let mut new_selection = None;
        for hint in hints {
            let Some(asset) = asset_from_hint(&hint, &tool) else {
                continue;
            };
            if gallery_changed
                && asset.gallery_asset_id.is_some()
                && let Err(error) =
                    hkask_media_widget::approve_gallery_media_path(std::path::Path::new(&asset.src))
            {
                log::warn!("failed to approve published gallery media path: {error}");
            }
            if self
                .assets
                .iter()
                .any(|existing| existing.body == asset.body)
            {
                continue;
            }
            self.assets.push(asset);
            new_selection = Some(self.assets.len() - 1);
        }
        if let Some(ix) = new_selection {
            self.set_selection(Some(ix), cx);
            cx.notify();
        }
    }

    /// Dispatch a media-server tool and merge its display-hint assets on
    /// success; surface failures in the status line. The governed invoker
    /// path — identical to `load_gallery`/`load_jobs`.
    fn dispatch_edit(
        &mut self,
        tool: &'static str,
        params: serde_json::Value,
        describe: String,
        cx: &mut Context<Self>,
    ) {
        let epoch = match self.begin_edit(tool) {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.status = None;
        self.edit_progress = Some(format!("{tool}: {describe}…"));
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_edit_result(
                epoch,
                tool,
                Err("Tool invoker not wired — edit actions unavailable.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(MEDIA_SERVER, tool, params);
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|error| error.message());
            this.update(cx, |this, cx| {
                this.apply_edit_result(epoch, tool, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// Admit exactly one direct side-effecting edit at a time.
    fn begin_edit(&mut self, tool: &'static str) -> Result<u64, String> {
        if self.edit_request.state == ResourceState::Loading {
            return Err("Another media edit is already in progress; wait for it to finish.".into());
        }
        self.edit_request
            .begin(RequestOwner::Edit(tool))
            .ok_or_else(|| "This media edit is already in progress.".to_string())
    }

    /// expect: The admitted edit remains visible until its result or failure arrives.
    /// [P1] Motivating: superseded edits cannot overwrite the current operation state.
    /// pre: epoch was returned by edit_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    fn apply_edit_result(
        &mut self,
        epoch: u64,
        tool: &'static str,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        match result {
            Ok(text) => {
                self.ingest_tool_result(&Value::String(text), tool, cx);
                self.edit_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.edit_request
                    .complete(epoch, Err(format!("{tool} failed: {error}")));
            }
        }
        self.edit_progress = None;
        cx.notify();
    }

    /// Trim the selected asset to the widget's transport marks via
    /// `video_clip`. The result surfaces as a new playable asset.
    fn dispatch_trim(&mut self, cx: &mut Context<Self>) {
        let (in_secs, out_secs) = self
            .media_widget
            .as_ref()
            .and_then(|widget| widget.read(cx).trim_range())
            .unwrap_or_else(|| {
                self.status = Some("Set both in and out marks before trimming.".into());
                (f64::NAN, f64::NAN)
            });
        if in_secs.is_nan() {
            cx.notify();
            return;
        }
        let src = self
            .media_widget
            .as_ref()
            .map(|widget| widget.read(cx).src().to_string())
            .unwrap_or_default();
        self.dispatch_edit(
            "video_clip",
            serde_json::json!({
                "video_url": src,
                "start_sec": in_secs,
                "end_sec": out_secs,
            }),
            format!("trimming {in_secs:.1}s–{out_secs:.1}s"),
            cx,
        );
    }

    /// expect: Concat admission accepts only videos and never truncates an over-cap queue.
    /// [P1] Motivating: rejected media work is visible before dispatch.
    /// pre: selected may identify any viewer asset.
    /// post: valid unique videos append up to MAX_CONCAT_ITEMS; every rejection preserves the queue.
    fn queue_selected_for_concat(&mut self) -> Result<(), String> {
        let asset = self
            .selected
            .and_then(|position| self.assets.get(position))
            .ok_or_else(|| "Select a video before queueing it for concat.".to_string())?;
        if asset.kind != "video" {
            return Err("Only video assets can be queued for concat.".into());
        }
        if self.concat_queue.contains(&asset.src) {
            return Ok(());
        }
        if self.concat_queue.len() >= hkask_types::media_limits::MAX_CONCAT_ITEMS {
            return Err(format!(
                "Concat queue limit is {} videos; remove one before adding another.",
                hkask_types::media_limits::MAX_CONCAT_ITEMS
            ));
        }
        self.concat_queue.push(asset.src.clone());
        Ok(())
    }

    /// Concatenate the queued assets via `video_concat`. The result surfaces
    /// as a new playable asset.
    fn dispatch_concat(&mut self, cx: &mut Context<Self>) {
        if self.concat_queue.len() < 2 {
            self.status = Some("Queue at least two clips to concatenate.".into());
            cx.notify();
            return;
        }
        if self.concat_queue.len() > hkask_types::media_limits::MAX_CONCAT_ITEMS {
            self.status = Some(format!(
                "Concat queue exceeds the {}-video limit.",
                hkask_types::media_limits::MAX_CONCAT_ITEMS
            ));
            cx.notify();
            return;
        }
        let urls = self.concat_queue.clone();
        self.dispatch_edit(
            "video_concat",
            serde_json::json!({ "video_urls": urls }),
            format!("concatenating {} clips", urls.len()),
            cx,
        );
    }

    /// Force-refresh the view pane: drop strongly cached non-media viz widgets,
    /// suspend/release the selected media widget when refreshing Media, and
    /// reload the active tab's data from its source.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        hkask_viz_core::clear_widget_cache();
        match self.active_tab {
            ViewerTab::Media => {
                // Re-ingest the thread's tool-result display hints. The
                // assets list is rebuilt from scratch so entries removed
                // from the thread disappear too.
                let thread = self.thread.clone();
                if let Some(thread) = thread
                    && let Some(thread) = thread.upgrade()
                {
                    self.assets.clear();
                    self.set_selection(None, cx);
                    self.ingest_thread(thread, cx);
                } else {
                    self.status =
                        Some("No conversation thread to re-ingest — open a thread first.".into());
                }
            }
            ViewerTab::Library => self.reload_gallery(cx),
            ViewerTab::Queue => self.load_jobs(cx),
            ViewerTab::Detail => {
                self.load_detail(cx);
                self.load_detail_transcripts(cx);
                self.load_detail_annotations(cx);
            }
            ViewerTab::Transcript => {
                if let Some(transcript) = self.transcript.as_ref() {
                    let transcript_id = transcript.transcript_id.clone();
                    self.load_transcript_with(transcript_id, cx);
                } else {
                    self.load_transcript_listing(cx);
                }
            }
        }
        cx.notify();
    }

    /// Load the gallery's current page. The committed offset changes only
    /// after a complete response parses, so a failed page request preserves
    /// the entire last-good snapshot.
    fn load_gallery(&mut self, cx: &mut Context<Self>) {
        self.request_gallery_page(self.gallery_offset, cx);
    }

    fn reload_gallery(&mut self, cx: &mut Context<Self>) {
        self.gallery_request.invalidate();
        self.request_gallery_page(self.gallery_offset, cx);
    }

    fn request_gallery_page(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .gallery_request
            .begin(RequestOwner::GalleryPage(offset))
        else {
            return;
        };
        self.status = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_gallery_listing(
                epoch,
                Err("Tool invoker not wired — panel cannot query the gallery.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_list_assets",
            serde_json::json!({ "offset": offset, "limit": GALLERY_PAGE_LIMIT }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => parse_gallery_listing(
                    hkask_types::tool_response::parse_tool_response(&text),
                    offset,
                ),
                Err(error) => Err(format!("gallery_list_assets failed: {}", error.message())),
            };
            this.update(cx, |this, cx| this.apply_gallery_listing(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    /// expect: Gallery rows and pagination metadata change only for the latest complete response.
    /// [P1] Motivating: malformed or superseded pages cannot corrupt the last-good Library snapshot.
    /// pre: epoch was returned by gallery_request.begin.
    /// post: stale outcomes are ignored; valid owned snapshots commit atomically; owned failures preserve data.
    fn apply_gallery_listing(
        &mut self,
        epoch: u64,
        result: Result<GalleryListing, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.gallery_request.owns(epoch) {
            return;
        }
        match result {
            Ok(listing) => {
                self.commit_gallery_listing(listing, cx);
                self.gallery_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.gallery_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    fn commit_gallery_listing(&mut self, listing: GalleryListing, cx: &mut Context<Self>) {
        let gallery_changed = self
            .gallery_id
            .as_ref()
            .is_some_and(|gallery_id| gallery_id != &listing.gallery_id);
        let selected_src = self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .map(|asset| asset.src.clone());
        let confirmed_src = self
            .confirm_delete
            .armed
            .and_then(|ix| self.assets.get(ix))
            .map(|asset| asset.src.clone());
        for record in &listing.assets {
            if let Some(path) = record.get("path").and_then(Value::as_str)
                && let Err(error) =
                    hkask_media_widget::approve_gallery_media_path(std::path::Path::new(path))
            {
                log::warn!("failed to approve listed gallery media path: {error}");
            }
        }
        let mut next_assets = self.assets.clone();
        if gallery_changed {
            next_assets.retain(|asset| asset.gallery_index.is_none());
        }
        merge_gallery_records(
            &mut next_assets,
            &listing.assets,
            listing.offset,
            Some(listing.total),
        );

        let next_selected = if gallery_changed {
            None
        } else {
            selected_src
                .as_ref()
                .and_then(|src| next_assets.iter().position(|asset| &asset.src == src))
                .or_else(|| (!next_assets.is_empty()).then_some(0))
        };
        let next_confirm_delete = if gallery_changed {
            None
        } else {
            confirmed_src
                .as_ref()
                .and_then(|src| next_assets.iter().position(|asset| &asset.src == src))
        };
        let selected_changed = selected_src
            != next_selected.and_then(|ix| next_assets.get(ix).map(|asset| asset.src.clone()));

        self.assets = next_assets;
        self.gallery_total = Some(listing.total);
        self.gallery_offset = listing.offset;
        self.gallery_limit = listing.limit;
        self.gallery_id = Some(listing.gallery_id);
        self.selected = next_selected;
        self.confirm_delete.armed = next_confirm_delete;
        if gallery_changed || selected_changed {
            self.clear_media_widget(cx);
            self.detail = None;
            self.detail_request.invalidate();
            self.detail_transcripts = None;
            self.detail_transcripts_request.invalidate();
            self.detail_annotations = None;
            self.detail_annotations_request.invalidate();
        }
    }

    #[cfg(test)]
    fn merge_gallery_listing(&mut self, payload: Option<Value>, cx: &mut Context<Self>) {
        let requested_offset = payload
            .as_ref()
            .and_then(|payload| payload.get("offset"))
            .and_then(Value::as_u64)
            .and_then(|offset| usize::try_from(offset).ok())
            .unwrap_or(self.gallery_offset);
        match parse_gallery_listing(payload, requested_offset) {
            Ok(listing) => self.commit_gallery_listing(listing, cx),
            Err(error) => self.gallery_request.state = ResourceState::Failed(error),
        }
    }

    fn previous_gallery_page(&mut self, cx: &mut Context<Self>) {
        let offset = self.gallery_offset.saturating_sub(self.gallery_limit);
        self.request_gallery_page(offset, cx);
    }

    fn next_gallery_page(&mut self, cx: &mut Context<Self>) {
        let Some(total) = self
            .gallery_total
            .and_then(|total| usize::try_from(total).ok())
        else {
            return;
        };
        let offset = self.gallery_offset.saturating_add(self.gallery_limit);
        if offset < total {
            self.request_gallery_page(offset, cx);
        }
    }

    /// Load the research-operation queue (spec: see and manage running jobs).
    fn load_jobs(&mut self, cx: &mut Context<Self>) {
        let Some(epoch) = self.jobs_request.begin(RequestOwner::Jobs) else {
            return;
        };
        self.status = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_jobs_listing(
                epoch,
                Err("Tool invoker not wired — panel cannot query jobs.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let limit = hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT;
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "job_list",
            serde_json::json!({ "limit": limit }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => parse_job_listing(&text),
                Err(error) => Err(format!("job_list failed: {}", error.message())),
            };
            this.update(cx, |this, cx| this.apply_jobs_listing(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    /// expect: Queue rows and failures belong only to the latest jobs request.
    /// [P1] Motivating: slow queue callbacks cannot replace fresher queue state.
    /// pre: epoch was returned by jobs_request.begin.
    /// post: stale outcomes are ignored; failures preserve rows and stop polling.
    fn apply_jobs_listing(
        &mut self,
        epoch: u64,
        result: Result<JobListing, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.jobs_request.owns(epoch) {
            return;
        }
        match result {
            Ok(listing) => {
                let visible_job_ids = listing
                    .jobs
                    .iter()
                    .map(|job| job.id.clone())
                    .collect::<HashSet<_>>();
                self.processed_job_results
                    .retain(|job_id| visible_job_ids.contains(job_id));
                let completed_results = listing
                    .jobs
                    .iter()
                    .filter(|job| job.status == "completed")
                    .filter_map(|job| {
                        job.result
                            .clone()
                            .map(|result| (job.id.clone(), job.op.clone(), result))
                    })
                    .collect::<Vec<_>>();
                self.jobs = listing.jobs;
                self.jobs_total = Some(listing.total);
                self.jobs_limit = listing.limit;
                self.jobs_has_more = listing.has_more;
                self.jobs_request.complete(epoch, Ok(()));
                for (job_id, op, result) in completed_results {
                    if self.processed_job_results.insert(job_id) {
                        self.ingest_tool_result(&result, &op, cx);
                    }
                }
                if self.should_poll_jobs() {
                    self.schedule_queue_poll(cx);
                } else {
                    self.stop_queue_polling();
                }
            }
            Err(error) => {
                self.jobs_request.complete(epoch, Err(error));
                self.stop_queue_polling();
            }
        }
        cx.notify();
    }

    fn should_poll_jobs(&self) -> bool {
        self.active_tab == ViewerTab::Queue && self.jobs.iter().any(job_is_nonterminal)
    }

    fn schedule_queue_poll(&mut self, cx: &mut Context<Self>) {
        if self.queue_poll_task.is_some() || !self.should_poll_jobs() {
            return;
        }
        self.queue_poll_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(QUEUE_POLL_INTERVAL).await;
            this.update(cx, |this, cx| {
                this.queue_poll_task = None;
                if this.should_poll_jobs() {
                    this.load_jobs(cx);
                }
            })
            .log_err();
        }));
    }

    /// Load the inspector data for the selected asset (spec: metadata, tags,
    /// lineage, versions). Addresses the asset by its stable gallery id —
    /// the positional index is display data and cannot survive a root
    /// switch or reordering.
    fn load_detail(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            self.detail = None;
            self.detail_request.invalidate();
            cx.notify();
            return;
        };
        let Some(asset_id) = self.assets[ix].gallery_asset_id.clone() else {
            self.detail = None;
            self.detail_request.invalidate();
            self.status = Some(
                "Asset is not in the gallery index yet — run gallery_refresh \
                 from the curator to index it."
                    .into(),
            );
            cx.notify();
            return;
        };
        let Some(epoch) = self
            .detail_request
            .begin(RequestOwner::Detail(asset_id.clone()))
        else {
            return;
        };
        self.status = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_detail_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load detail.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_asset_detail",
            serde_json::json!({ "image_id": asset_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => hkask_types::tool_response::parse_tool_response(&text)
                    .filter(Value::is_object)
                    .ok_or_else(|| {
                        "gallery_asset_detail returned malformed object data.".to_string()
                    }),
                Err(error) => Err(format!("gallery_asset_detail failed: {}", error.message())),
            };
            this.update(cx, |this, cx| this.apply_detail_result(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    /// expect: Detail preserves its last-good object unless the latest selected-asset request succeeds.
    /// [P1] Motivating: superseded detail errors cannot blank or degrade newer detail.
    /// pre: epoch was returned by detail_request.begin.
    /// post: stale outcomes are ignored and owned failures retain detail.
    fn apply_detail_result(
        &mut self,
        epoch: u64,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.detail_request.owns(epoch) {
            return;
        }
        match result {
            Ok(detail) => {
                self.detail = Some(detail);
                self.detail_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.detail_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Load the selected asset's transcript summaries for the Detail view via
    /// `educt_list_transcripts`. A read beside `load_detail`, not chained to
    /// it: both are keyed by the same stable asset id and protected by their
    /// own request lifecycles, so a slow transcripts response cannot blank a
    /// loaded detail and vice versa. Guard failures stay silent here —
    /// `load_detail` already surfaces the no-selection and unindexed-asset
    /// statuses for the same selection.
    fn load_detail_transcripts(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            self.detail_transcripts = None;
            self.detail_transcripts_request.invalidate();
            return;
        };
        let Some(asset_id) = self.assets[ix].gallery_asset_id.clone() else {
            self.detail_transcripts = None;
            self.detail_transcripts_request.invalidate();
            return;
        };
        let Some(epoch) = self
            .detail_transcripts_request
            .begin(RequestOwner::DetailTranscripts(asset_id.clone()))
        else {
            return;
        };
        self.detail_transcripts = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_detail_transcripts_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load transcripts.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_list_transcripts",
            serde_json::json!({ "gallery_asset_id": asset_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_transcript_listing(&value),
                    None => Err("educt_list_transcripts returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_list_transcripts failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_detail_transcripts_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer transcripts request owns the rows even if an older outcome arrives later.
    /// [P1] Motivating: overlapping transcript loads cannot regress visible detail state.
    /// pre: epoch was returned by detail_transcripts_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    fn apply_detail_transcripts_result(
        &mut self,
        epoch: u64,
        result: Result<Vec<TranscriptSummaryRow>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.detail_transcripts_request.owns(epoch) {
            return;
        }
        match result {
            Ok(rows) => {
                self.detail_transcripts = Some(rows);
                self.detail_transcripts_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.detail_transcripts_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Load the selected asset's researcher tags/notes via
    /// `gallery_annotation_list` — the Detail tab's annotation surface
    /// (2026-10-27). A read beside `load_detail`, keyed by the same stable
    /// asset id and protected by its own request lifecycle; guard failures
    /// stay silent (load_detail surfaces the no-selection and
    /// unindexed-asset statuses for the same selection).
    fn load_detail_annotations(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            self.detail_annotations = None;
            self.detail_annotations_request.invalidate();
            return;
        };
        let Some(asset_id) = self.assets[ix].gallery_asset_id.clone() else {
            self.detail_annotations = None;
            self.detail_annotations_request.invalidate();
            return;
        };
        let Some(epoch) = self
            .detail_annotations_request
            .begin(RequestOwner::DetailAnnotations(asset_id.clone()))
        else {
            return;
        };
        self.detail_annotations = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_detail_annotations_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load annotations.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_annotation_list",
            serde_json::json!({ "gallery_asset_id": asset_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_annotation_listing(&value),
                    None => Err("gallery_annotation_list returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "gallery_annotation_list failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_detail_annotations_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer annotations request owns the rows even if an older outcome arrives later.
    /// [P1] Motivating: overlapping annotation loads cannot regress visible detail state.
    /// pre: epoch was returned by detail_annotations_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    fn apply_detail_annotations_result(
        &mut self,
        epoch: u64,
        result: Result<Vec<PaneAnnotation>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.detail_annotations_request.owns(epoch) {
            return;
        }
        match result {
            Ok(rows) => {
                self.detail_annotations = Some(rows);
                self.detail_annotations_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.detail_annotations_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Dispatch a researcher-annotation edit (create/update/delete) under
    /// the single edit admission, then reload the annotation list so the
    /// section reflects it.
    fn dispatch_annotation_edit(
        &mut self,
        tool: &'static str,
        params: serde_json::Value,
        done: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .and_then(|asset| asset.gallery_asset_id.as_deref())
            .is_none()
        {
            self.status = Some(
                "Asset is not in the gallery index yet — annotations need an indexed asset.".into(),
            );
            cx.notify();
            return;
        }
        let epoch = match self.begin_edit(tool) {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some(format!("{tool}…"));
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_annotation_edit_result(
                epoch,
                done,
                Err("Tool invoker not wired — panel cannot edit annotations.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(MEDIA_SERVER, tool, params);
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|error| error.message());
            this.update(cx, |this, cx| {
                this.apply_annotation_edit_result(epoch, done, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: The owned annotation edit commits once and the list reloads
    /// with it; failures surface visibly and clear progress.
    fn apply_annotation_edit_result(
        &mut self,
        epoch: u64,
        done: &'static str,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(_acknowledgement) => {
                self.status = Some(format!("Annotation {done} — the list reloads."));
                self.edit_request.complete(epoch, Ok(()));
                self.load_detail_annotations(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err(format!("annotation {done} failed")));
            }
        }
        cx.notify();
    }

    /// Open the annotation editor for a new tag/note or an existing row's
    /// value, focus its input, and hint the keys (the transcript label
    /// editor's modal pattern).
    fn open_annotation_editor(
        &mut self,
        edit: AnnotationEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .annotation_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let replacing = edit.annotation_id.is_some();
        self.status = Some(if replacing {
            "Replace the value — Enter saves; Escape cancels.".into()
        } else {
            "Write the tag or note — Enter saves; Escape cancels.".into()
        });
        self.annotation_edit = Some(edit);
        focus.focus(window, cx);
        cx.notify();
    }

    /// Confirm the open annotation editor: create dispatches with the typed
    /// value, replace dispatches over the row's identity. Blank values
    /// refuse visibly and keep the buffer; an in-flight edit refuses and
    /// keeps the buffer for retry.
    fn confirm_annotation_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.annotation_edit.take() else {
            return;
        };
        if self.edit_request.state == ResourceState::Loading {
            self.annotation_edit = Some(edit);
            self.status =
                Some("Another media edit is already in progress; wait for it to finish.".into());
            cx.notify();
            return;
        }
        let value = edit.buffer.label();
        if value.is_empty() {
            self.annotation_edit = Some(edit);
            self.status = Some("A tag or note needs text — Escape cancels.".into());
            cx.notify();
            return;
        }
        let Some(asset_id) = self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .and_then(|asset| asset.gallery_asset_id.clone())
        else {
            self.annotation_edit = Some(edit);
            self.status = Some(
                "Asset is not in the gallery index yet — annotations need an indexed asset.".into(),
            );
            cx.notify();
            return;
        };
        match edit.annotation_id.clone() {
            None => self.dispatch_annotation_edit(
                "gallery_annotation_create",
                annotation_create_params(&asset_id, edit.kind, &value),
                "created",
                cx,
            ),
            Some(annotation_id) => self.dispatch_annotation_edit(
                "gallery_annotation_update",
                annotation_update_params(&asset_id, &annotation_id, &value),
                "updated",
                cx,
            ),
        }
    }

    /// Cancel the open annotation editor without dispatching.
    fn cancel_annotation_edit(&mut self, cx: &mut Context<Self>) {
        if self.annotation_edit.take().is_some() {
            self.status = Some("Annotation edit cancelled.".into());
            cx.notify();
        }
    }

    /// Route the annotation editor's keystrokes — the label editor's key
    /// handling over the annotation confirm/cancel pair.
    fn handle_annotation_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match keystroke.key.as_str() {
            "escape" => self.cancel_annotation_edit(cx),
            "enter" => self.confirm_annotation_edit(cx),
            _ => {
                if let Some(edit) = self.annotation_edit.as_mut()
                    && edit.buffer.apply_keystroke(keystroke)
                {
                    cx.notify();
                }
            }
        }
    }

    /// Paste clipboard text into the open annotation editor (control
    /// characters dropped — single-line values).
    fn paste_into_annotation_edit(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(edit) = self.annotation_edit.as_mut() {
            edit.buffer.insert_str(&text);
            cx.notify();
        }
    }

    /// The Detail tab's annotation editor row — the label editor's input,
    /// parameterized for a tag/note value. An associated fn so the caller
    /// can hold the detail state's immutable borrow.
    fn render_annotation_editor(
        edit: &AnnotationEdit,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let context = if edit.annotation_id.is_some() {
            format!("Replace {}", edit.kind)
        } else {
            format!("New {}", edit.kind)
        };
        let typed = edit.buffer.text();
        let cursor = edit.buffer.cursor();
        let before: String = typed.chars().take(cursor).collect();
        let after: String = typed.chars().skip(cursor).collect();
        let mut input = div()
            .id("media-detail-annotation-input")
            .track_focus(focus)
            .flex()
            .items_center()
            .min_w_0()
            .w(px(160.))
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border)
            .px_1()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let keystroke = &event.keystroke;
                if (keystroke.modifiers.control || keystroke.modifiers.platform)
                    && keystroke.key == "v"
                {
                    this.paste_into_annotation_edit(cx);
                } else {
                    this.handle_annotation_key(&event.keystroke, cx);
                }
                cx.stop_propagation();
            }))
            .on_click({
                let focus = focus.clone();
                cx.listener(move |_, _, window, cx| {
                    window.focus(&focus, cx);
                })
            });
        if typed.is_empty() {
            input = input.child(
                Label::new(SharedString::from("(value)"))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            );
        } else {
            input = input
                .child(
                    Label::new(SharedString::from(before))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                )
                .child(
                    div()
                        .id("media-detail-annotation-caret")
                        .w(px(1.))
                        .h(px(12.))
                        .bg(cx.theme().colors().text),
                )
                .child(
                    Label::new(SharedString::from(after))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                );
        }
        div()
            .id("media-detail-annotation-editor")
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                Label::new(SharedString::from(context))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(input)
            .child(
                div()
                    .id("media-detail-annotation-confirm")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("save"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_annotation_edit(cx);
                    })),
            )
            .child(
                div()
                    .id("media-detail-annotation-cancel")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("cancel"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_annotation_edit(cx);
                    })),
            )
            .into_any_element()
    }

    /// Open a transcript in the workspace tab: clear prior state, switch to
    /// the Transcript tab, and dispatch `educt_get_transcript` for the id —
    /// the Detail tab's entry point (S3a).
    fn open_transcript(&mut self, transcript_id: String, cx: &mut Context<Self>) {
        self.transcript = None;
        self.transcript_request.invalidate();
        self.current_word = None;
        self.selection_mode = false;
        self.selection_anchor = None;
        self.highlight_edit = None;
        self.transcript_follow_task.take();
        // A fresh open starts the virtualized list from the top — a
        // previous workspace's reading position must not survive a
        // reopen (S3d).
        self.transcript_list.reset(0);
        self.transcript_list_count = 0;
        self.follow_reveals_in_flight = 0;
        self.follow_viewport = true;
        self.activate_tab(ViewerTab::Transcript, cx);
        self.load_transcript_with(transcript_id, cx);
    }

    /// Close the open workspace back to the recordings browser (S6): the
    /// transcript state clears, the follow task cancels, and the listing
    /// reloads — the browser stays reachable for the viewer's lifetime
    /// (2026-10-26 review: it previously disappeared after the first open).
    /// The shared player stays bound in the registry; reopening any
    /// workspace rebinds through the same path.
    fn close_transcript_workspace(&mut self, cx: &mut Context<Self>) {
        self.transcript = None;
        self.transcript_request.invalidate();
        self.current_word = None;
        self.selection_anchor = None;
        self.highlight_edit = None;
        self.transcript_follow_task.take();
        self.status = Some("Back to the recordings browser.".into());
        self.load_transcript_listing(cx);
        cx.notify();
    }

    /// Load the workspace transcript via `educt_get_transcript` — the
    /// single-dispatch data contract (D2): summary + working view + layers
    /// arrive together; the strict parse rejects a malformed payload rather
    /// than rendering a half view.
    fn load_transcript_with(&mut self, transcript_id: String, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .transcript_request
            .begin(RequestOwner::Transcript(transcript_id.clone()))
        else {
            return;
        };
        // The last-good workspace stays visible while a reload is in
        // flight (2026-10-26 review: eagerly nulling it flashed the idle
        // browser after every highlight edit; open_transcript nulls
        // explicitly when the operator leaves one workspace for another).
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_transcript_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load the transcript.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_get_transcript",
            serde_json::json!({ "transcript_id": transcript_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_transcript_payload(&value),
                    None => Err("educt_get_transcript returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!("educt_get_transcript failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                this.apply_transcript_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer transcript request owns the workspace even if an older outcome arrives later.
    /// [P1] Motivating: overlapping transcript loads cannot regress the visible workspace.
    /// pre: epoch was returned by transcript_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    fn apply_transcript_result(
        &mut self,
        epoch: u64,
        result: Result<TranscriptPaneState, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.transcript_request.owns(epoch) {
            return;
        }
        match result {
            Ok(state) => {
                // S5 reel editing: a fresh workspace loads with ALL
                // researcher-authored pane selections included in the next
                // compose (membership keys on provenance, never the label
                // text — free-text labels must not affect the reel flow).
                self.reel_selection = state
                    .highlights
                    .iter()
                    .filter(|highlight| highlight.researcher_supplied)
                    .map(|highlight| (highlight.start_word, highlight.end_word))
                    .collect();
                // A fresh workspace invalidates any open label editor —
                // its annotation ids belong to the previous view.
                self.highlight_edit = None;
                self.transcript = Some(state);
                self.current_word = None;
                self.transcript_request.complete(epoch, Ok(()));
                self.start_transcript_follow(cx);
                // A fresh workspace follows the mark: tracking on, and the
                // scroll baseline re-anchored so the first tick cannot
                // misread the load-time layout as a manual scroll (S3d).
                self.follow_viewport = true;
                self.follow_reveals_in_flight = 0;
                self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
            }
            Err(error) => {
                self.transcript_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Load the recordings listing for the Transcript tab's idle state via
    /// `educt_list_transcripts` — the browse surface the tab opens with
    /// (operator report 2026-10-26: the tab must list what's available,
    /// not dead-end).
    /// Delete one stored recording through `educt_delete_transcript` — the
    /// CRUD delete the recordings listing owed the reference model
    /// (recordings are project-scoped assets the operator manages, not
    /// read-only fixtures). Durable exports and rendered reels are preserved
    /// by the server, detached from the removed record.
    fn delete_transcript_recording(&mut self, transcript_id: String, cx: &mut Context<Self>) {
        let epoch = match self.begin_edit("educt_delete_transcript") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Deleting recording…".into());
        self.confirm_delete_transcript.disarm();
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_transcript_deletion_result(
                epoch,
                Err("Tool invoker not wired — panel cannot delete recordings.".to_string()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_delete_transcript",
            serde_json::json!({
                "transcript_id": transcript_id,
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("transcripts_removed")
                        .and_then(serde_json::Value::as_u64)
                        .filter(|removed| *removed > 0)
                        .map(|removed| removed as usize)
                        .ok_or_else(|| {
                            "educt_delete_transcript returned no removal acknowledgement."
                                .to_string()
                        }),
                    None => Err("educt_delete_transcript returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_delete_transcript failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_transcript_deletion_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// The deletion's result gate: owned epochs only, the edit lifecycle
    /// closes either way, and a successful deletion reloads the recordings
    /// listing so the removed row disappears from the browser.
    fn apply_transcript_deletion_result(
        &mut self,
        epoch: u64,
        result: Result<usize, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(removed) => {
                self.status = Some(format!(
                    "Deleted {removed} recording{} — exports and rendered reels are preserved.",
                    if removed == 1 { "" } else { "s" }
                ));
                self.edit_request.complete(epoch, Ok(()));
                self.load_transcript_listing(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("recording deletion failed".to_string()));
            }
        }
        cx.notify();
    }

    fn load_transcript_listing(&mut self, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .transcript_listing_request
            .begin(RequestOwner::TranscriptListing)
        else {
            return;
        };
        self.transcript_listing = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_transcript_listing_result(
                epoch,
                Err("Tool invoker not wired — panel cannot list transcripts.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_list_transcripts",
            serde_json::json!({ "limit": 500 }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_transcript_listing(&value),
                    None => Err("educt_list_transcripts returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_list_transcripts failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_transcript_listing_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer listing request owns the rows even if an older outcome arrives later.
    /// [P1] Motivating: overlapping listing loads cannot regress the visible browse surface.
    /// pre: epoch was returned by transcript_listing_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    fn apply_transcript_listing_result(
        &mut self,
        epoch: u64,
        result: Result<Vec<TranscriptSummaryRow>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.transcript_listing_request.owns(epoch) {
            return;
        }
        match result {
            Ok(rows) => {
                self.transcript_listing = Some(rows);
                // A deletion or concurrent listing change may have removed the
                // recording whose Confirm delete is armed — never leave a confirm
                // aimed at a row that no longer exists.
                if let Some(id) = self.confirm_delete_transcript.armed.as_ref()
                    && !self
                        .transcript_listing
                        .as_ref()
                        .is_some_and(|listing| listing.iter().any(|row| row.id == *id))
                {
                    self.confirm_delete_transcript.disarm();
                }
                self.transcript_listing_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.transcript_listing_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// The word-click outcome under the two-mode interaction model (S4
    /// part-2): seek-only when selection mode is off; anchor → range when
    /// on; a repeated click on the anchor clears the pending selection;
    /// while the label editor is open, clicks are inert.
    /// Pure so the state machine is pin-testable.
    fn handle_word_selection(&self, clicked: usize) -> WordClickOutcome {
        if self.highlight_edit.is_some() {
            // The label editor is modal to its range: stray clicks neither
            // seek the player nor start a new selection (Enter/Escape only).
            return WordClickOutcome::EditorOpen;
        }
        if !self.selection_mode {
            return WordClickOutcome::SeekOnly;
        }
        match self.selection_anchor {
            None => WordClickOutcome::AnchorSet(clicked),
            Some(anchor) if anchor == clicked => WordClickOutcome::AnchorCleared,
            Some(anchor) => WordClickOutcome::OpenLabelEditor {
                start_word: anchor.min(clicked),
                end_word: anchor.max(clicked),
            },
        }
    }

    /// A word was clicked in the workspace: dispatch by the selection
    /// state machine — seek outside select mode, build ranges inside it,
    /// ignore clicks while the label editor is open. Range completion
    /// opens the label editor, gated on index-identical views (D5: the
    /// panel never re-implements the server's projection; re-anchored
    /// views cannot source-map a hand highlight in v1).
    fn word_clicked(
        &mut self,
        global_word: usize,
        start_ms: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.handle_word_selection(global_word) {
            WordClickOutcome::SeekOnly => {
                self.seek_workspace_transcript(start_ms, cx);
            }
            WordClickOutcome::AnchorSet(word) => {
                self.selection_anchor = Some(word);
                self.status = Some(
                    "Click the last word to finish selecting, then create a highlight or cut."
                        .into(),
                );
                cx.notify();
            }
            WordClickOutcome::AnchorCleared => {
                self.selection_anchor = None;
                self.status = Some("Selection cleared.".into());
                cx.notify();
            }
            WordClickOutcome::EditorOpen => {
                self.status = Some("Label editor open — Enter confirms, Escape cancels.".into());
                cx.notify();
            }
            WordClickOutcome::OpenLabelEditor {
                start_word,
                end_word,
            } => {
                self.selection_anchor = None;
                let alignment = self
                    .transcript
                    .as_ref()
                    .map(|state| state.alignment.as_str())
                    .unwrap_or_default();
                if let Err(reason) = highlight_range_gate(alignment) {
                    self.status = Some(reason.into());
                    cx.notify();
                    return;
                }
                self.open_label_editor(
                    HighlightEdit {
                        kind: HighlightEditKind::Create {
                            start_word,
                            end_word,
                        },
                        buffer: LabelBuffer::default(),
                    },
                    window,
                    cx,
                );
            }
        }
    }

    /// Create a researcher highlight over the selected source range via
    /// `educt_annotation_create` — the label editor's confirm arm — then
    /// reload the workspace so the new highlight lands in the overlay and
    /// list (single-flight via the edit admission).
    fn create_workspace_highlight(
        &mut self,
        start_word: usize,
        end_word: usize,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_create") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Creating highlight…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                "Highlight",
                "created",
                "creation",
                Err("Tool invoker not wired — panel cannot create highlights.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_create",
            highlight_create_params(&transcript_id, start_word, end_word, &label),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("annotation_id")
                        .and_then(serde_json::Value::as_str)
                        .map(|id| id.to_string())
                        .ok_or_else(|| {
                            "educt_annotation_create returned no annotation_id.".to_string()
                        }),
                    None => Err("educt_annotation_create returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_create failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    "Highlight",
                    "created",
                    "creation",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// Strike the pending range instead of labeling it (R2): close the
    /// editor and dispatch the cut — a strike carries no label, so the
    /// label flow ends here. The range already passed the selection's
    /// alignment gate when the editor opened; an in-flight edit keeps the
    /// editor open for retry (the confirm path's admission contract).
    fn strike_selected_range(
        &mut self,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        if self.edit_request.state == ResourceState::Loading {
            self.status =
                Some("Another media edit is already in progress; wait for it to finish.".into());
            cx.notify();
            return;
        }
        self.highlight_edit = None;
        self.create_workspace_strike(start_word, end_word, cx);
    }

    /// Create a researcher strike over the word range via
    /// `educt_annotation_create` (kind strike — a single-Cut EDL with blank
    /// label/note), then reload the workspace so the line-through overlay
    /// and the Strikes list reflect it (single-flight via the edit
    /// admission).
    fn create_workspace_strike(
        &mut self,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_create") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Cutting selection…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                "Strike",
                "struck",
                "creation",
                Err("Tool invoker not wired — panel cannot strike words.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_create",
            strike_create_params(&transcript_id, start_word, end_word),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("annotation_id")
                        .and_then(serde_json::Value::as_str)
                        .map(|id| id.to_string())
                        .ok_or_else(|| {
                            "educt_annotation_create returned no annotation_id.".to_string()
                        }),
                    None => Err("educt_annotation_create returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_create failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    "Strike",
                    "struck",
                    "creation",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// expect: The owned annotation edit (create, rename, delete, or
    /// strike) commits once and the workspace reloads with it; failures
    /// surface visibly and clear progress.
    fn apply_workspace_annotation_result(
        &mut self,
        epoch: u64,
        transcript_id: String,
        subject: &'static str,
        done: &'static str,
        failed: &'static str,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(_acknowledgement) => {
                self.status = Some(format!(
                    "{subject} {done} — the transcript reloads with it."
                ));
                self.edit_request.complete(epoch, Ok(()));
                self.load_transcript_with(transcript_id, cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request.complete(
                    epoch,
                    Err(format!("{} {failed} failed", subject.to_lowercase())),
                );
            }
        }
        cx.notify();
    }

    /// The pane's compose flow (R3): build the reel EDL directly from the
    /// pane's annotations — Keep ops from the toggled-in researcher
    /// highlights and Cut ops from the strikes, per the mode — store it via
    /// `educt_store_layer`, and render via `educt_render_edl` with the
    /// stored layer id: a two-invoke chain under the single edit admission,
    /// ending in the render's display hint (which lands the reel as a
    /// playable asset through the existing ingestion).
    fn compose_workspace_reel(&mut self, mode: ReelComposeMode, cx: &mut Context<Self>) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let selection = self.reel_selection.clone();
        let Some(compose_params) = compose_reel_params(state, &selection, mode) else {
            self.status = Some(mode.empty_status().into());
            cx.notify();
            return;
        };
        let transcript_id = state.transcript_id.clone();
        let epoch = match self.begin_edit("educt_store_layer") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some(mode.progress().into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_edit_result(
                epoch,
                "educt_store_layer",
                Err("Tool invoker not wired — panel cannot compose reels.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let store_task = invoker.invoke_tool(MEDIA_SERVER, "educt_store_layer", compose_params);
        cx.spawn(async move |this, cx| {
            fn finish(
                this: &gpui::WeakEntity<MediaViewer>,
                epoch: u64,
                tool: &'static str,
                result: Result<String, String>,
                cx: &mut gpui::AsyncApp,
            ) {
                this.update(cx, |viewer, cx| {
                    viewer.apply_edit_result(epoch, tool, result, cx)
                })
                .log_err();
            }
            let store_text = match store_task.await {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        "educt_store_layer",
                        Err(format!("educt_store_layer failed: {}", error.message())),
                        cx,
                    );
                    return;
                }
            };
            let Some(edl_id) = hkask_types::tool_response::parse_tool_response(&store_text)
                .and_then(|value| {
                    value
                        .pointer("/stored/id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
            else {
                finish(
                    &this,
                    epoch,
                    "educt_store_layer",
                    Err("educt_store_layer returned no EDL layer id.".into()),
                    cx,
                );
                return;
            };
            let render_task = invoker.invoke_tool(
                MEDIA_SERVER,
                "educt_render_edl",
                serde_json::json!({
                    "transcript_id": transcript_id,
                    "layer_id": edl_id,
                }),
            );
            match render_task.await {
                Ok(text) => finish(&this, epoch, "educt_render_edl", Ok(text), cx),
                Err(error) => finish(
                    &this,
                    epoch,
                    "educt_render_edl",
                    Err(format!("educt_render_edl failed: {}", error.message())),
                    cx,
                ),
            }
        })
        .detach();
    }

    /// Delete a researcher-authored annotation (highlight or strike) via
    /// `educt_annotation_delete`, then reload the workspace so the row,
    /// overlay, and any reel selection reflect the removal (single-flight
    /// via the edit admission; the completion reuses the create path's
    /// reload semantics). The subject names the kind for the progress and
    /// failure messages; the server dispatches on the stored layer kind.
    fn delete_workspace_annotation(
        &mut self,
        annotation_id: String,
        subject: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_delete") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some(format!("Deleting {}…", subject.to_lowercase()));
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                subject,
                "deleted",
                "deletion",
                Err(format!(
                    "Tool invoker not wired — panel cannot delete {}s.",
                    subject.to_lowercase()
                )),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_delete",
            serde_json::json!({
                "transcript_id": transcript_id,
                "annotation_id": annotation_id,
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("deleted")
                        .and_then(serde_json::Value::as_bool)
                        .filter(|deleted| *deleted)
                        .map(|_| String::new())
                        .ok_or_else(|| {
                            "educt_annotation_delete returned no deletion acknowledgement."
                                .to_string()
                        }),
                    None => Err("educt_annotation_delete returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_delete failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    subject,
                    "deleted",
                    "deletion",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// Rename a researcher highlight via `educt_annotation_update` — the
    /// label editor's confirm arm for an existing row — preserving its
    /// range and note (only the label changes), then reload the workspace so
    /// the row, overlay, and label color reflect it (single-flight via the
    /// edit admission).
    fn rename_workspace_highlight(
        &mut self,
        annotation_id: String,
        start_word: usize,
        end_word: usize,
        label: String,
        note: String,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_update") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Renaming highlight…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                "Highlight",
                "renamed",
                "rename",
                Err("Tool invoker not wired — panel cannot rename highlights.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_update",
            highlight_update_params(
                &transcript_id,
                &annotation_id,
                start_word,
                end_word,
                &label,
                &note,
            ),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("annotation_id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| *id == annotation_id)
                        .map(|_| String::new())
                        .ok_or_else(|| {
                            "educt_annotation_update returned no matching annotation_id."
                                .to_string()
                        }),
                    None => Err("educt_annotation_update returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_update failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    "Highlight",
                    "renamed",
                    "rename",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// Open the label editor (S4-3) for a completed range (create) or an
    /// existing researcher row (rename), focus its input, and hint the
    /// keys. The editor is modal to the pane's word clicks.
    fn open_label_editor(
        &mut self,
        edit: HighlightEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .label_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let creating = matches!(edit.kind, HighlightEditKind::Create { .. });
        self.status = Some(if creating {
            "Name the highlight, or choose Cut selection. Enter confirms; Escape cancels.".into()
        } else {
            "Rename the highlight — Enter saves; Escape cancels.".into()
        });
        self.highlight_edit = Some(edit);
        focus.focus(window, cx);
        cx.notify();
    }

    /// Confirm the open label editor: create dispatches with the typed
    /// label (empty falls back to the default), rename dispatches with the
    /// typed label over the unchanged range and note. Admission refusals
    /// (edit in flight, blank rename label) surface in the status line and
    /// keep the typed buffer for retry.
    fn confirm_label_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.highlight_edit.take() else {
            return;
        };
        if self.edit_request.state == ResourceState::Loading {
            self.highlight_edit = Some(edit);
            self.status =
                Some("Another media edit is already in progress; wait for it to finish.".into());
            cx.notify();
            return;
        }
        match &edit.kind {
            HighlightEditKind::Create {
                start_word,
                end_word,
            } => {
                let label = resolve_create_label(edit.buffer.text().as_str());
                self.create_workspace_highlight(*start_word, *end_word, label, cx);
            }
            HighlightEditKind::Rename {
                annotation_id,
                start_word,
                end_word,
                note,
            } => {
                let label = edit.buffer.label();
                if label.is_empty() {
                    self.highlight_edit = Some(edit);
                    self.status =
                        Some("A renamed highlight needs a nonblank label — Escape cancels.".into());
                    cx.notify();
                    return;
                }
                self.rename_workspace_highlight(
                    annotation_id.clone(),
                    *start_word,
                    *end_word,
                    label,
                    note.clone(),
                    cx,
                );
            }
        }
    }

    /// Cancel the open label editor without dispatching anything.
    fn cancel_label_edit(&mut self, cx: &mut Context<Self>) {
        if self.highlight_edit.take().is_some() {
            self.status = Some("Label edit cancelled.".into());
            cx.notify();
        }
    }

    /// Paste clipboard text into the open label editor (2026-10-26
    /// review: modifier chords were rejected outright, so a label could
    /// not be pasted). Control characters are dropped — the input is
    /// single-line. Inert with no editor open.
    fn paste_into_label_edit(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(edit) = self.highlight_edit.as_mut() {
            edit.buffer.insert_str(&text);
            cx.notify();
        }
    }

    /// Route the label input's keystrokes: Enter confirms, Escape cancels,
    /// editing keys go to the buffer. Bound on the input element itself, so
    /// it only fires while the editor is open.
    fn handle_label_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match keystroke.key.as_str() {
            "escape" => self.cancel_label_edit(cx),
            "enter" => self.confirm_label_edit(cx),
            _ => {
                if let Some(edit) = self.highlight_edit.as_mut()
                    && edit.buffer.apply_keystroke(keystroke)
                {
                    cx.notify();
                }
            }
        }
    }

    /// The label editor row (S4-3): the focused single-line input plus
    /// confirm/cancel affordances, rendered above the transcript text so it
    /// stays in view. An associated fn so the caller can hold the workspace
    /// state's immutable borrow.
    fn render_label_editor(
        edit: &HighlightEdit,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (context, confirm_text, placeholder) = match &edit.kind {
            HighlightEditKind::Create { .. } => (
                "Name this highlight".to_string(),
                "Create",
                "Highlight name",
            ),
            HighlightEditKind::Rename { .. } => {
                ("Rename highlight".to_string(), "Save", "Highlight name")
            }
        };
        let typed = edit.buffer.text();
        let cursor = edit.buffer.cursor();
        let before: String = typed.chars().take(cursor).collect();
        let after: String = typed.chars().skip(cursor).collect();
        let mut input = div()
            .id("media-transcript-label-input")
            .track_focus(focus)
            .flex()
            .items_center()
            .gap_0()
            .min_w_0()
            .w(px(160.))
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border)
            .px_1()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let keystroke = &event.keystroke;
                if (keystroke.modifiers.control || keystroke.modifiers.platform)
                    && keystroke.key == "v"
                {
                    this.paste_into_label_edit(cx);
                } else {
                    this.handle_label_key(&event.keystroke, cx);
                }
                cx.stop_propagation();
            }))
            .on_click({
                let focus = focus.clone();
                cx.listener(move |_, _, window, cx| {
                    window.focus(&focus, cx);
                })
            });
        if typed.is_empty() {
            input = input.child(
                Label::new(SharedString::from(placeholder))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            );
        } else {
            input = input
                .child(
                    Label::new(SharedString::from(before))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                )
                .child(
                    div()
                        .id("media-transcript-label-caret")
                        .w(px(1.))
                        .h(px(12.))
                        .bg(cx.theme().colors().text),
                )
                .child(
                    Label::new(SharedString::from(after))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                );
        }
        let editor = div()
            .id("media-transcript-label-editor")
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .debug_selector(|| "media-transcript-label-editor".into())
            .child(
                Label::new(SharedString::from(context))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(input)
            .child(
                div()
                    .id("media-transcript-label-confirm")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(confirm_text))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_label_edit(cx);
                    })),
            );
        // R2: the create arm also offers Strike — cut the range without a
        // label (a rename has no strike affordance).
        let editor = match &edit.kind {
            HighlightEditKind::Create {
                start_word,
                end_word,
            } => {
                let (start_word, end_word) = (*start_word, *end_word);
                editor.child(
                    div()
                        .id("media-transcript-label-strike")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("Cut selection"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Error),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.strike_selected_range(start_word, end_word, cx);
                        })),
                )
            }
            HighlightEditKind::Rename { .. } => editor,
        };
        editor
            .child(
                div()
                    .id("media-transcript-label-cancel")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Cancel"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_label_edit(cx);
                    })),
            )
            .into_any_element()
    }

    /// Seek the workspace's bound player to a working word's start — the
    /// click-to-seek entry (S3b-2); the interactive text's click listener
    /// resolves the word's start_ms through the pane's word map.
    fn seek_workspace_transcript(&mut self, start_ms: u64, cx: &mut Context<Self>) {
        if let Some(widget) = &self.media_widget {
            widget.update(cx, |widget, cx| {
                widget.seek_to_secs(start_ms as f64 / 1000.0, cx)
            });
        }
    }

    /// Follow-along (S3c): poll the bound player's transport position on a
    /// GPUI-native timer and mark the word containing it, while the
    /// workspace is visible and a transcript is open. Self-cancels when
    /// the tab leaves or the workspace closes; the handle drop cancels the
    /// spawned future (the same lifecycle as the queue poll).
    fn start_transcript_follow(&mut self, cx: &mut Context<Self>) {
        if self.transcript_follow_task.is_some() {
            return;
        }
        let entity = cx.entity().downgrade();
        self.transcript_follow_task = Some(cx.spawn(async move |_this, cx| {
            loop {
                cx.background_executor().timer(FOLLOW_POLL_INTERVAL).await;
                let keep_going = match entity.update(cx, |viewer, cx| {
                    if viewer.active_tab != ViewerTab::Transcript || viewer.transcript.is_none() {
                        viewer.transcript_follow_task = None;
                        viewer.current_word = None;
                        cx.notify();
                        return false;
                    }
                    // D3: poll only while playing — a paused or stopped
                    // player keeps its frozen mark without the widget
                    // reads (2026-10-26 review: the loop previously polled
                    // regardless of the playing state). The scroll baseline
                    // still updates while paused, so a manual scroll during
                    // a pause suspends follow too (S3d).
                    let playing = viewer
                        .media_widget
                        .as_ref()
                        .is_some_and(|widget| widget.read(cx).is_transport_playing());
                    if !playing {
                        viewer.note_follow_scroll_tick(cx);
                        return true;
                    }
                    let position = viewer
                        .media_widget
                        .as_ref()
                        .and_then(|widget| widget.read(cx).transport_position_secs());
                    let words = viewer
                        .transcript
                        .as_ref()
                        .and_then(|state| state.words.as_deref());
                    let next = position
                        .and_then(|secs| {
                            words.map(|words| current_word_index(words, (secs * 1000.0) as u64))
                        })
                        .flatten();
                    if next != viewer.current_word {
                        viewer.current_word = next;
                        cx.notify();
                    }
                    // S3d: keep the marked block visible — a manual scroll
                    // (any offset change not from a follow reveal)
                    // suspends tracking; the Follow toggle re-enables it.
                    viewer.note_follow_scroll_tick(cx);
                    viewer.follow_marked_block_into_view();
                    true
                }) {
                    Ok(keep_going) => keep_going,
                    Err(_) => break,
                };
                if !keep_going {
                    break;
                }
            }
        }));
    }

    /// Observe the workspace list's scroll position once per follow tick.
    /// Any movement not accounted for by a pending follow reveal is a
    /// manual scroll: tracking suspends until the operator re-enables it
    /// through the Follow toggle (S3d).
    fn note_follow_scroll_tick(&mut self, cx: &mut Context<Self>) {
        let top = self.transcript_list.logical_scroll_top();
        if top != self.follow_scroll_baseline {
            if self.follow_reveals_in_flight > 0 {
                self.follow_reveals_in_flight -= 1;
            } else if self.follow_viewport {
                self.follow_viewport = false;
                cx.notify();
            }
            self.follow_scroll_baseline = top;
        }
    }

    /// Keep the follow-along mark visible (S3d): scroll the marked block
    /// into view when tracking is enabled and the block is not already
    /// fully within the list's viewport. Pure list-state work — callable
    /// from the follow task without a window.
    fn follow_marked_block_into_view(&mut self) {
        if !self.follow_viewport {
            return;
        }
        let Some(word) = self.current_word else {
            return;
        };
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let Some(block_ix) = block_index_for_word(state, word) else {
            return;
        };
        let item_ix = WORKSPACE_HEADER_ITEMS + block_ix;
        let visible = self
            .transcript_list
            .bounds_for_item(item_ix)
            .is_some_and(|item| {
                let viewport = self.transcript_list.viewport_bounds();
                item.top() >= viewport.top() && item.bottom() <= viewport.bottom()
            });
        if !visible {
            self.follow_reveals_in_flight = self.follow_reveals_in_flight.saturating_add(1);
            // Anchor by item index, not by measured pixels: blocks deep in
            // a long recording are unmeasured until rendered, so a
            // pixel-based reveal would compute against zero heights and
            // scroll nowhere. The item-anchored scroll lands the marked
            // block at the viewport top (the chat list's scroll-to-item
            // precedent).
            self.transcript_list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
        }
    }

    /// The Follow toggle (S3d): re-enable viewport tracking (revealing the
    /// current mark) or suspend it. A manual scroll suspends tracking; this
    /// is the operator's way back.
    fn toggle_follow_viewport(&mut self, cx: &mut Context<Self>) {
        self.follow_viewport = !self.follow_viewport;
        if self.follow_viewport
            && let Some(word) = self.current_word
            && let Some(state) = self.transcript.as_ref()
            && let Some(block_ix) = block_index_for_word(state, word)
        {
            self.follow_reveals_in_flight = self.follow_reveals_in_flight.saturating_add(1);
            self.transcript_list.scroll_to(ListOffset {
                item_ix: WORKSPACE_HEADER_ITEMS + block_ix,
                offset_in_item: px(0.),
            });
        }
        self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
        cx.notify();
    }

    /// Delete the gallery-index entry for the selected asset (two-step confirm; the
    /// file on disk is left untouched — `delete_file: false`). Addresses the
    /// asset by its stable gallery id — a positional index captured before a
    /// root switch could delete a DIFFERENT asset in the new gallery.
    fn delete_asset(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(asset) = self.assets.get(ix) else {
            return;
        };
        let Some(asset_id) = asset.gallery_asset_id.clone() else {
            self.status = Some("Asset is not in the gallery index — nothing to delete.".into());
            cx.notify();
            return;
        };
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot delete.".into());
            cx.notify();
            return;
        };
        self.status = None;
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_delete_image",
            serde_json::json!({ "image_id": asset_id, "delete_file": false }),
        );
        cx.spawn(async move |this, cx| {
            match task.await.and_then(|response| {
                parse_delete_acknowledgement(&response, &asset_id)
                    .map_err(|message| hkask_tool_invoker::InvokeError::Failed { message, kind: None })
            }) {
                Ok(transcripts_detached) => {
                    this.update(cx, |this, cx| {
                        this.confirm_delete.disarm();
                        this.status = Some(format!(
                            "Asset removed from the gallery index; {transcripts_detached} transcript link(s) detached."
                        ));
                        // Supersede any listing admitted before deletion.
                        this.reload_gallery(cx);
                    })
                    .log_err();
                }
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.status =
                            Some(format!("gallery_delete_image failed: {}", error.message()));
                        cx.notify();
                    })
                    .log_err();
                }
            }
        })
        .detach();
    }

    /// Cancel a queued/running job.
    fn cancel_job(&mut self, job_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot cancel jobs.".into());
            cx.notify();
            return;
        };
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "job_cancel",
            serde_json::json!({ "job_id": job_id }),
        );
        cx.spawn(async move |this, cx| match task.await {
            Ok(_) => {
                this.update(cx, |this, cx| this.load_jobs(cx)).log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.status = Some(format!("job_cancel failed: {}", error.message()));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    fn presented_status(&self) -> Option<(SharedString, ui::Color)> {
        let resource = match self.active_tab {
            ViewerTab::Media => resource_status(
                "Edit",
                &self.edit_request.state,
                !self.assets.is_empty(),
                self.edit_progress.as_deref(),
            ),
            ViewerTab::Library => resource_status(
                "Library",
                &self.gallery_request.state,
                self.gallery_total.is_some(),
                None,
            ),
            ViewerTab::Queue => resource_status(
                "Queue",
                &self.jobs_request.state,
                self.jobs_total.is_some(),
                None,
            ),
            ViewerTab::Detail => resource_status(
                "Detail",
                &self.detail_request.state,
                self.detail.is_some(),
                None,
            ),
            ViewerTab::Transcript => resource_status(
                "Transcript",
                &self.transcript_request.state,
                self.transcript.is_some(),
                None,
            )
            .or_else(|| {
                // S5's progress-visible contract: while an edit is in flight
                // on this tab (compose/render, highlight create), the edit
                // progress surfaces — previously it rendered nowhere
                // (2026-10-26 review). Failures clear edit_progress and
                // fall through to the specific status line below.
                self.edit_progress
                    .clone()
                    .map(|progress| (progress.into(), ui::Color::Muted))
            }),
        };
        resource.or_else(|| {
            self.status
                .clone()
                .map(|status| (status.into(), ui::Color::Error))
        })
    }

    // ── Rendering ──────────────────────────────────────────────────────────

    fn tab_button(
        &self,
        tab: ViewerTab,
        label: SharedString,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.active_tab == tab;
        div()
            .id(SharedString::from(format!("media-viewer-tab-{:?}", tab)))
            .px_2()
            .h_6()
            // min_w_0 + a truncating label: without them the tab's
            // min-content (its full label, e.g. "Library (12)") fed the
            // pane's min-content and the tab bar overflowed narrow docks.
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
                    .color(if active {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.activate_tab(tab, cx);
            }))
    }

    fn render_media(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            return self
                .render_empty(
                    "No media yet",
                    "Ask the curator to ingest, search, or excerpt evidence — or open the Library.",
                )
                .into_any_element();
        };
        let asset = self.assets[ix].clone();

        // The media widget for the selected asset — SHARED with the
        // conversation-inline render via the viz-core registry (one player
        // per stable Asset, exact body when unindexed). The viewer keeps the
        // entity so the edit toolbar can reach its playback clock and trim
        // marks; selection changes suspend before resolving the next identity.
        if self.media_widget_body.as_deref() != Some(asset.body.as_str()) {
            match hkask_viz_core::shared_media_widget(&asset.body, window, cx) {
                Some(widget) => {
                    self.media_widget = Some(widget);
                    self.media_widget_body = Some(asset.body.clone());
                }
                None => {
                    self.media_widget = None;
                    self.media_widget_body = None;
                    return self
                        .render_empty(
                            "Unrenderable media block",
                            "The tool produced a display hint the viewer cannot render.",
                        )
                        .into_any_element();
                }
            }
        }
        let Some(widget) = self.media_widget.clone() else {
            return self
                .render_empty(
                    "Unrenderable media block",
                    "The tool produced a display hint the viewer cannot render.",
                )
                .into_any_element();
        };

        // The header row is width-contained: the pane is a flex item whose
        // min-content is the widest row's content width, so an untruncated
        // src label here used to inflate the whole pane past the dock
        // (min_w_0 + truncate breaks that propagation; overflow_hidden is
        // the clip of last resort at extreme narrowness).
        let header = h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .min_w_0()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Label::new(asset.src.clone())
                    .size(LabelSize::Small)
                    .color(ui::Color::Muted)
                    .flex_1()
                    .truncate(),
            )
            .child(
                Label::new(asset.tool.clone())
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint)
                    .truncate(),
            );

        // ── Edit toolbar: trim marks + concatenation queue ───────────────
        // Width discipline: the pane is a flex item whose min-content is
        // this row's content width, and seven buttons + two readout labels
        // ≈ 800px — wider than any dock pane. The row wraps (the same
        // overflow policy as the Detail tab's kv rows) so every action
        // stays reachable at ~320px panes; min_w_0 keeps the row's
        // min-content from inflating the pane; the readout labels truncate.
        let (position_label, marks_label, trim_ready) = widget.read(cx).edit_state_labels();
        let edit_active = self.edit_request.state == ResourceState::Loading;
        let concat_count = self.concat_queue.len();
        let queued_current = self.concat_queue.contains(&asset.src);
        let toolbar = h_flex()
            .gap_2()
            .px_3()
            .py_1()
            .flex_wrap()
            .min_w_0()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Label::new(position_label)
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .truncate(),
            )
            .child(
                Label::new(marks_label)
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .truncate(),
            )
            .child(
                ui::Button::new("mark-in", "Mark In")
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener({
                        let widget = widget.clone();
                        move |_this, _event, _window, cx| {
                            widget.update(cx, |widget, cx| widget.mark_in(cx));
                        }
                    })),
            )
            .child(
                ui::Button::new("mark-out", "Mark Out")
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener({
                        let widget = widget.clone();
                        move |_this, _event, _window, cx| {
                            widget.update(cx, |widget, cx| widget.mark_out(cx));
                        }
                    })),
            )
            .child(
                ui::Button::new("clear-marks", "Clear Marks")
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener({
                        let widget = widget.clone();
                        move |_this, _event, _window, cx| {
                            widget.update(cx, |widget, cx| widget.clear_marks(cx));
                        }
                    })),
            )
            .child(
                ui::Button::new("trim", "Trim to Marks")
                    .label_size(LabelSize::XSmall)
                    .disabled(!trim_ready || edit_active)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.dispatch_trim(cx);
                    })),
            )
            .child(
                ui::Button::new(
                    "queue-concat",
                    if queued_current {
                        "Queued ✓"
                    } else {
                        "Queue for Concat"
                    },
                )
                .label_size(LabelSize::XSmall)
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    match this.queue_selected_for_concat() {
                        Ok(()) => this.status = None,
                        Err(error) => this.status = Some(error),
                    }
                    cx.notify();
                })),
            )
            .child(
                ui::Button::new("concat", format!("Concat ({concat_count})"))
                    .label_size(LabelSize::XSmall)
                    .disabled(concat_count < 2 || edit_active)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.dispatch_concat(cx);
                    })),
            )
            .when(concat_count > 0, |el| {
                el.child(
                    ui::Button::new("clear-queue", "Clear Queue")
                        .label_size(LabelSize::XSmall)
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.concat_queue.clear();
                            cx.notify();
                        })),
                )
            });

        // No scroll wrapper on the Media tab: a scroll container gives
        // its children indefinite height, and the video widget's
        // aspect-fit sizing needs a definite height derived from the
        // pane (min_h_0 lets flex_1 shrink below content size).
        // Library/Queue/Detail scroll; the player fills.
        let content = div()
            .id("media-viewer-content")
            .flex_1()
            .min_h_0()
            .p_3()
            .child(widget);

        // flex_1 + min_h_0, NOT size_full: this root is a flex-column child
        // of the viewer (main axis = vertical), where h_full means 100% of
        // the PARENT height — tab bar + 100% overflows the pane by the tab
        // bar's height (the "player larger than the window" bug). flex_1
        // takes the REMAINING height; min_h_0 lets it shrink below content.
        v_flex()
            .flex_1()
            .min_h_0()
            .child(Self::tab_purpose(
                "Playback and derivative edits — marks and trims serve audio and video; results land as new assets.",
            ))
            .child(header)
            .child(toolbar)
            .child(content)
            .into_any_element()
    }

    fn render_library(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        if self.gallery_total.is_none() {
            match &self.gallery_request.state {
                ResourceState::Loading => {
                    return self
                        .render_empty("Loading Library", "Fetching the first gallery page…")
                        .into_any_element();
                }
                ResourceState::Failed(error) => {
                    return self
                        .render_empty("Library unavailable", error)
                        .into_any_element();
                }
                ResourceState::Idle => {
                    return self
                        .render_empty("Library not loaded", "Open or refresh Library to load it.")
                        .into_any_element();
                }
                ResourceState::Ready => {}
            }
        }
        let total = self.gallery_total.unwrap_or(0);
        let total_as_usize = usize::try_from(total).unwrap_or(usize::MAX);
        let page_end = self
            .gallery_offset
            .saturating_add(self.gallery_limit)
            .min(total_as_usize);
        let rows = self
            .assets
            .iter()
            .enumerate()
            .filter(|(_, asset)| {
                asset
                    .gallery_index
                    .is_none_or(|index| (self.gallery_offset..page_end).contains(&index))
            })
            .map(|(ix, asset)| {
                let selected = self.selected == Some(ix);
                let confirming = self.confirm_delete.is_armed(&ix);
                let icon = if asset.kind == "image" {
                    IconName::Image
                } else {
                    IconName::File
                };
                div()
                    .id(SharedString::from(format!("media-library-row-{ix}")))
                    .h_8()
                    .px_2()
                    .gap_2()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .map(|el| {
                        if selected {
                            el.bg(cx.theme().colors().element_active)
                        } else {
                            el.hover(|el| el.bg(cx.theme().colors().element_hover))
                        }
                    })
                    .child(Icon::new(icon).color(ui::Color::Muted))
                    // The clickable select area — the Remove button is a
                    // sibling, not nested, so both clicks never fire together.
                    .child(
                        div()
                            .id(SharedString::from(format!("media-library-select-{ix}")))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .child(
                                Label::new(asset.src.clone())
                                    .size(LabelSize::Small)
                                    .color(ui::Color::Default)
                                    .truncate(),
                            )
                            .child(
                                Label::new(asset.tool.clone())
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Hint),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_selection(Some(ix), cx);
                                this.activate_tab(ViewerTab::Media, cx);
                            })),
                    )
                    .child(
                        // Curate affordance: two-step delete (index entry only).
                        div()
                            .id(SharedString::from(format!("media-library-delete-{ix}")))
                            .px_2()
                            .h_5()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .hover(|el| el.bg(cx.theme().colors().element_hover))
                            .cursor_pointer()
                            .child(
                                Label::new(if confirming {
                                    "Confirm remove?"
                                } else {
                                    "Remove"
                                })
                                .size(LabelSize::XSmall)
                                .color(if confirming {
                                    ui::Color::Error
                                } else {
                                    ui::Color::Muted
                                }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.confirm_delete.confirm(&ix) {
                                    this.delete_asset(ix, cx);
                                } else {
                                    cx.notify();
                                }
                            })),
                    )
            })
            .collect::<Vec<_>>();
        let first = if total == 0 {
            0
        } else {
            self.gallery_offset.saturating_add(1)
        };
        let pagination = h_flex()
            .h_8()
            .gap_2()
            .px_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                ui::Button::new("media-library-previous", "Previous")
                    .label_size(LabelSize::XSmall)
                    .disabled(self.gallery_offset == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.previous_gallery_page(cx))),
            )
            .child(
                ui::Button::new("media-library-next", "Next")
                    .label_size(LabelSize::XSmall)
                    .disabled(page_end >= total_as_usize)
                    .on_click(cx.listener(|this, _, _, cx| this.next_gallery_page(cx))),
            )
            .child(
                Label::new(format!("Showing {first}–{page_end} of {total}"))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            );
        let content = if rows.is_empty() {
            self.render_empty(
                "Library empty",
                "This page has no indexed assets and the conversation has surfaced none.",
            )
            .into_any_element()
        } else {
            v_flex()
                .id("media-viewer-library-rows")
                .flex_1()
                .overflow_y_scroll()
                .gap_0p5()
                .p_2()
                .children(rows)
                .into_any_element()
        };
        v_flex()
            .id("media-viewer-library")
            .flex_1()
            .min_h_0()
            .child(Self::tab_purpose(
                "Every indexed asset — browse and select; delete removes the listing, never the file on disk.",
            ))
            .child(pagination)
            .child(content)
            .into_any_element()
    }

    fn render_queue(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        if self.jobs_total.is_none() {
            match &self.jobs_request.state {
                ResourceState::Loading => {
                    return self
                        .render_empty("Loading Queue", "Fetching research jobs…")
                        .into_any_element();
                }
                ResourceState::Failed(error) => {
                    return self
                        .render_empty("Queue unavailable", error)
                        .into_any_element();
                }
                ResourceState::Idle => {
                    return self
                        .render_empty("Queue not loaded", "Open or refresh Queue to load it.")
                        .into_any_element();
                }
                ResourceState::Ready => {}
            }
        }
        if self.jobs.is_empty() {
            return self
                .render_empty(
                    "Queue ready — no jobs",
                    "No research jobs have been submitted (or process-local history restarted).",
                )
                .into_any_element();
        }
        let rows = self
            .jobs
            .iter()
            .enumerate()
            .map(|(ix, job)| {
                let cancellable = job.status == "queued" || job.status == "running";
                let short_id: SharedString = if job.id.len() > 8 {
                    job.id[..8].to_string().into()
                } else {
                    job.id.clone().into()
                };
                div()
                    .id(SharedString::from(format!("media-queue-row-{ix}")))
                    .h_8()
                    .px_2()
                    .gap_2()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(short_id)
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Hint),
                    )
                    .child(
                        Label::new(job.op.clone())
                            .size(LabelSize::Small)
                            .color(ui::Color::Default),
                    )
                    .child(
                        Label::new(job.status.clone())
                            .size(LabelSize::XSmall)
                            .color(if job.status == "failed" {
                                ui::Color::Error
                            } else if job.status == "completed" {
                                ui::Color::Success
                            } else {
                                ui::Color::Muted
                            }),
                    )
                    .child(
                        Label::new(job.created_at.clone())
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Hint),
                    )
                    .when(cancellable, |el| {
                        let job_id = job.id.clone();
                        el.child(
                            div()
                                .id(SharedString::from(format!("media-queue-cancel-{ix}")))
                                .ml_auto()
                                .px_2()
                                .h_5()
                                .flex()
                                .items_center()
                                .rounded_sm()
                                .hover(|el| el.bg(cx.theme().colors().element_hover))
                                .cursor_pointer()
                                .child(
                                    Label::new("Cancel")
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Muted),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.cancel_job(job_id.clone(), cx);
                                })),
                        )
                    })
            })
            .collect::<Vec<_>>();
        let total = self.jobs_total.unwrap_or(self.jobs.len());
        let count_label = if self.jobs_has_more {
            format!(
                "Showing {} of {total} jobs (more available; limit {})",
                self.jobs.len(),
                self.jobs_limit
            )
        } else {
            format!("Showing {} of {total} jobs", self.jobs.len())
        };
        v_flex()
            .id("media-viewer-queue")
            .flex_1()
            .min_h_0()
            .child(Self::tab_purpose(
                "Async research jobs — the long-operation path (transcribe, batch edits); history is process-local.",
            ))
            .child(
                h_flex().h_8().px_2().items_center().child(
                    Label::new(count_label)
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                ),
            )
            .child(
                v_flex()
                    .id("media-viewer-queue-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .p_2()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_detail(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(detail) = self.detail.as_ref() else {
            let (title, hint) = match &self.detail_request.state {
                ResourceState::Loading => ("Loading Detail", "Fetching asset metadata…"),
                ResourceState::Failed(error) => ("Detail unavailable", error.as_str()),
                ResourceState::Ready => ("Detail ready — empty", "The asset returned no detail."),
                ResourceState::Idle => (
                    "No detail",
                    "Select an indexed gallery asset, then open Detail.",
                ),
            };
            return self.render_empty(title, hint).into_any_element();
        };
        let mut sections = v_flex().gap_2().p_3();
        sections = sections.child(Self::tab_purpose(
            "The selected asset's inspector — record, lineage, tags, linked transcripts, reel provenance.",
        ));
        if let Some(image) = detail.get("image").and_then(|i| i.as_object()) {
            sections = sections.child(Self::section_label("Record"));
            for (key, value) in image {
                if let Some(text) = scalar_text(value) {
                    sections = sections.child(Self::kv_row(key, &text, cx));
                }
            }
        }
        if let Some(tags) = detail.get("tags").and_then(|t| t.as_array()) {
            sections = sections.child(Self::section_label("Tags"));
            for tag in tags {
                let tag_type = tag.get("tag_type").and_then(|t| t.as_str()).unwrap_or("?");
                let value = tag.get("value").and_then(|v| v.as_str()).unwrap_or("?");
                sections = sections.child(Self::kv_row(tag_type, value, cx));
            }
        }
        if let Some(lineage) = detail.get("lineage") {
            if !lineage.is_null() {
                sections = sections.child(Self::section_label("Lineage"));
                if let Some(map) = lineage.as_object() {
                    for (key, value) in map {
                        if let Some(text) = scalar_text(value) {
                            sections = sections.child(Self::kv_row(key, &text, cx));
                        }
                    }
                }
            }
        }
        if let Some(faces) = detail.get("faces").and_then(|f| f.as_array()) {
            if !faces.is_empty() {
                sections = sections.child(Self::section_label("Faces"));
                sections = sections.child(Self::kv_row("count", &faces.len().to_string(), cx));
            }
        }
        if let Some(transcripts) = &self.detail_transcripts
            && !transcripts.is_empty()
        {
            sections = sections.child(Self::section_label("Transcripts"));
            for transcript in transcripts {
                let timings = if transcript.has_word_timings {
                    "word timings"
                } else {
                    "no word timings"
                };
                let transcript_id = transcript.id.clone();
                let mut row = div()
                    .id(SharedString::from(format!(
                        "media-detail-open-{}",
                        transcript.id
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(transcript.id.clone()))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .child(
                        Label::new(SharedString::from(format!(
                            "{} words, {timings}",
                            transcript.words_count
                        )))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                    );
                if let Some(note) = transcript_alignment_note(&transcript.alignment) {
                    row = row.child(
                        Label::new(SharedString::from(note))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Error),
                    );
                }
                sections = sections.child(row.on_click(cx.listener(move |this, _, _, cx| {
                    this.open_transcript(transcript_id.clone(), cx);
                })));
            }
        }
        if let ResourceState::Failed(error) = &self.detail_transcripts_request.state {
            sections = sections.child(Self::section_label("Transcripts"));
            sections = sections.child(Self::kv_row("unavailable", error, cx));
        }
        // The annotation surface (2026-10-27): researcher tags/notes with
        // direct create/replace/delete — the same direct-manipulation
        // rationale as the transcript highlights, on the inspector's home
        // surface. The whole section needs an indexed asset.
        if let Some(asset_id) = self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .and_then(|asset| asset.gallery_asset_id.clone())
        {
            sections = sections.child(Self::section_label("Researcher tags & notes"));
            if let Some(edit) = self.annotation_edit.clone()
                && let Some(focus) = self.annotation_input_focus.clone()
            {
                sections = sections.child(Self::render_annotation_editor(&edit, &focus, cx));
            } else {
                for kind in ["tag", "note"] {
                    sections = sections.child(
                        div()
                            .id(SharedString::from(format!(
                                "media-detail-annotation-add-{kind}"
                            )))
                            .cursor_pointer()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .hover(|el| el.bg(cx.theme().colors().element_hover))
                            .child(
                                Label::new(SharedString::from(format!("add {kind}")))
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Muted),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_annotation_editor(
                                    AnnotationEdit {
                                        kind,
                                        annotation_id: None,
                                        buffer: LabelBuffer::default(),
                                    },
                                    window,
                                    cx,
                                );
                            })),
                    );
                }
            }
            if let Some(annotations) = &self.detail_annotations {
                for annotation in annotations {
                    let annotation_id = annotation.annotation_id.clone();
                    let row_label = format!("{}: {}", annotation.kind, annotation.value);
                    let edit_annotation_id = annotation.annotation_id.clone();
                    let edit_value = annotation.value.clone();
                    let kind_static: &'static str = if annotation.kind == "note" {
                        "note"
                    } else {
                        "tag"
                    };
                    let delete_asset_id = asset_id.clone();
                    sections = sections.child(
                        div()
                            .id(SharedString::from(format!(
                                "media-detail-annotation-{annotation_id}"
                            )))
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                Label::new(SharedString::from(row_label))
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Default),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "media-detail-annotation-edit-{annotation_id}"
                                    )))
                                    .cursor_pointer()
                                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                                    .child(
                                        Label::new(SharedString::from("edit"))
                                            .size(LabelSize::XSmall)
                                            .color(ui::Color::Default),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_annotation_editor(
                                            AnnotationEdit {
                                                kind: kind_static,
                                                annotation_id: Some(edit_annotation_id.clone()),
                                                buffer: LabelBuffer::from_str(&edit_value),
                                            },
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "media-detail-annotation-delete-{annotation_id}"
                                    )))
                                    .cursor_pointer()
                                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                                    .child(
                                        Label::new(SharedString::from("delete"))
                                            .size(LabelSize::XSmall)
                                            .color(ui::Color::Error),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.dispatch_annotation_edit(
                                            "gallery_annotation_delete",
                                            annotation_delete_params(
                                                &delete_asset_id,
                                                &annotation_id,
                                            ),
                                            "deleted",
                                            cx,
                                        );
                                    })),
                            ),
                    );
                }
            }
        }
        if let ResourceState::Failed(error) = &self.detail_annotations_request.state {
            sections = sections.child(Self::section_label("Tags & notes"));
            sections = sections.child(Self::kv_row("unavailable", error, cx));
        }
        if let Some(provenance) = parse_reel_provenance(detail) {
            sections = sections.child(Self::section_label("Reel provenance"));
            sections = sections.child(Self::kv_row("rendered from transcript", &provenance, cx));
        }
        v_flex()
            .id("media-viewer-detail")
            .flex_1()
            .overflow_y_scroll()
            .child(sections)
            .into_any_element()
    }

    /// The Transcript workspace (S3a): the open transcript's working view
    /// rendered as paragraph blocks with speaker labels and a layers
    /// strip, beside the recording's shared player (S3b: the same
    /// viz-core-registry widget the Media tab and inline renders use — one
    /// player per media identity, rebound on tab activation, suspended on
    /// leave; D1). Alignment degradations surface as visible status notes,
    /// never silence (D5); click-to-seek and follow-along land with the
    /// S3b hit-testing element.
    fn render_transcript(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let Some(state) = self.transcript.as_ref() else {
            return self.render_transcript_browser(cx);
        };
        let media_path = state.media_path.clone();
        let body = transcript_media_body(&media_path);
        if self.media_widget_body.as_deref() != Some(body.as_str()) {
            match hkask_viz_core::shared_media_widget(&body, window, cx) {
                Some(widget) => {
                    self.media_widget = Some(widget);
                    self.media_widget_body = Some(body);
                }
                None => {
                    // Degradation surfaced, never silent: the transcript text
                    // still renders without the player.
                    self.media_widget = None;
                    self.media_widget_body = None;
                }
            }
        }
        // The label input's focus handle, created on first need (pure tests
        // construct the viewer without a GPUI context).
        let label_input_focus = self
            .label_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let state = self
            .transcript
            .as_ref()
            .expect("the empty-state guard returned above");
        // The open label editor (S4-3) stays pinned above the list —
        // always in view while a create or rename is pending.
        let pending_editor = self.highlight_edit.clone();
        // The whole workspace content (header rows, transcript blocks,
        // annotation sections) renders as ONE virtualized list (S3d):
        // everything scrolls together exactly as the workspace always
        // did. Timed blocks are interactive text — every word is a click
        // range that seeks the bound player to its start (S3b-2), the
        // word containing the playback position carries the follow-along
        // mark (S3c), and the follow task keeps the marked block in view
        // through the list state.
        let blocks = Arc::new(build_pane_blocks(state));
        let item_count = WORKSPACE_HEADER_ITEMS + blocks.len() + WORKSPACE_LIST_TAIL_ITEMS;
        if self.transcript_list_count != item_count {
            self.transcript_list.reset(item_count);
            self.transcript_list_count = item_count;
            self.follow_reveals_in_flight = 0;
            self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
        }
        let list_state = self.transcript_list.clone();
        let workspace_items = cx.processor(move |this, ix, _window, cx| {
            this.render_workspace_list_item(ix, blocks.as_slice(), cx)
        });
        let workspace_list = gpui::list(list_state, workspace_items)
            .flex_1()
            .min_h_0()
            .p_3();
        let mut workspace = v_flex().id("media-viewer-transcript").flex_1().min_w_0();
        if let Some(edit) = pending_editor {
            workspace = workspace.child(v_flex().px_3().pt_3().child(Self::render_label_editor(
                &edit,
                &label_input_focus,
                cx,
            )));
        }
        workspace.child(workspace_list).into_any_element()
    }

    /// One workspace list item (S3d): a fixed header row, a transcript
    /// block, or one of the fixed tail sections — highlights, cuts, reel
    /// actions (the operator-ruled presentation order: transcript text
    /// before annotation inventories).
    fn render_workspace_list_item(
        &self,
        ix: usize,
        blocks: &[PaneBlock],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if ix < WORKSPACE_HEADER_ITEMS {
            return self.render_workspace_header_item(ix, cx);
        }
        let block_ix = ix - WORKSPACE_HEADER_ITEMS;
        if let Some(block) = blocks.get(block_ix) {
            return div()
                .pb_2()
                .child(self.render_workspace_block(block, block_ix, cx))
                .into_any_element();
        }
        match ix - WORKSPACE_HEADER_ITEMS - blocks.len() {
            0 => self.render_workspace_highlights_section(cx),
            1 => self.render_workspace_cuts_section(cx),
            _ => self.render_workspace_reel_section(cx),
        }
    }

    /// The fixed header rows at the top of the workspace list (S3d): the
    /// back row, the player, the mode chips, the alignment note, and the
    /// Transcript label with the review note — the same rows the
    /// workspace always showed, scrolling with the content.
    fn render_workspace_header_item(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let row = match ix {
            0 => {
                let Some(state) = self.transcript.as_ref() else {
                    return div().into_any_element();
                };
                let recording_name = recording_display_name(&state.media_path);
                let back = div()
                    .id("media-transcript-back-to-recordings")
                    .debug_selector(|| "media-transcript-back-to-recordings".into())
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("← Recordings"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_transcript_workspace(cx);
                    }));
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(back)
                    .child(
                        Label::new(SharedString::from(recording_name))
                            .size(LabelSize::Small)
                            .color(ui::Color::Default),
                    )
                    .into_any_element()
            }
            1 => match self.media_widget.clone() {
                Some(widget) => div()
                    .h(px(140.))
                    .flex()
                    .min_w_0()
                    .overflow_hidden()
                    .child(widget)
                    .into_any_element(),
                // Degradation surfaced, never silent: the transcript text
                // still renders without the player.
                None => Label::new(SharedString::from(
                    "Playback unavailable — the transcript and editing controls remain available.",
                ))
                .size(LabelSize::XSmall)
                .color(ui::Color::Error)
                .into_any_element(),
            },
            2 => {
                let selection_mode = self.selection_mode;
                let following = self.follow_viewport;
                // S4 part-2: the select-range mode toggle — an explicit mode
                // beats every-click-seek plus accidental creation.
                let select_chip = div()
                    .id("media-transcript-select-mode")
                    .debug_selector(|| "media-transcript-select-mode".into())
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if selection_mode {
                            "Finish selection"
                        } else {
                            "Select text"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if selection_mode {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selection_mode = !this.selection_mode;
                        this.selection_anchor = None;
                        // Toggling the mode abandons any open label editor —
                        // it is the tail of a selection this toggle ends.
                        this.highlight_edit = None;
                        this.status = Some(if this.selection_mode {
                            "Select mode on — click the first word of the range.".into()
                        } else {
                            "Select mode off — clicks seek playback.".into()
                        });
                        cx.notify();
                    }));
                // The Follow toggle (S3d): viewport tracking is on by
                // default; a manual scroll suspends it and this affordance
                // re-enables it.
                let follow_chip = div()
                    .id("media-transcript-follow-toggle")
                    .debug_selector(|| "media-transcript-follow-toggle".into())
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if following {
                            "Following"
                        } else {
                            "Follow"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if following {
                            ui::Color::Muted
                        } else {
                            ui::Color::Default
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_follow_viewport(cx);
                    }));
                h_flex()
                    .items_center()
                    .flex_wrap()
                    .gap_2()
                    .child(select_chip)
                    .child(follow_chip)
                    .into_any_element()
            }
            3 => {
                let Some(state) = self.transcript.as_ref() else {
                    return div().into_any_element();
                };
                match state.alignment.as_str() {
                    "re_anchored" => Label::new(SharedString::from(
                        "Estimated timings — review existing ranges before creating a reel.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .into_any_element(),
                    "unaligned" => Label::new(SharedString::from(
                        "Corrected text only — playback and timed selection are unavailable.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error)
                    .into_any_element(),
                    "untimed" => Label::new(SharedString::from(
                        "Text-only transcript — playback and timed selection are unavailable.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error)
                    .into_any_element(),
                    _ => div().into_any_element(),
                }
            }
            _ => {
                let Some(state) = self.transcript.as_ref() else {
                    return div().into_any_element();
                };
                let mut row = v_flex();
                row = row.child(Self::section_label("Transcript"));
                // Review flags (task 1a): the derived marker rides the
                // payload; the pane renders it, never re-derives (D5).
                let review = state
                    .review
                    .as_ref()
                    .filter(|_| matches!(state.alignment.as_str(), "original" | "aligned"));
                if let Some(note) = review.and_then(|review| review.note.as_deref()) {
                    row = row.child(
                        Label::new(SharedString::from(format!("Review: {note}")))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .mt_1(),
                    );
                }
                row.into_any_element()
            }
        };
        div().pb_2().child(row).into_any_element()
    }

    /// One transcript block as a workspace list item (S3a/S3b/S3c):
    /// speaker label, per-block review marker, interactive text with word
    /// click-to-seek ranges, highlight and strike overlays, and the
    /// follow-along mark. The marked block carries the layout-test
    /// selector (S3d).
    fn render_workspace_block(
        &self,
        block: &PaneBlock,
        block_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        let current_word = self.current_word;
        let selection_anchor = self.selection_anchor;
        let highlight_style = HighlightStyle {
            background_color: Some(cx.theme().colors().element_active),
            ..Default::default()
        };
        // R2: struck words render line-through.
        let strike_style = HighlightStyle {
            strikethrough: Some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(cx.theme().colors().text),
            }),
            ..Default::default()
        };
        let review = state
            .review
            .as_ref()
            .filter(|_| matches!(state.alignment.as_str(), "original" | "aligned"));
        let per_block_no_confidence = review.is_some_and(|review| review.note.is_none());
        let mut section = v_flex();
        if let Some(speaker) = &block.speaker {
            section = section.child(
                Label::new(SharedString::from(speaker.clone()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .mt_1(),
            );
        }
        if let Some(flag) = review.and_then(|review| review.blocks.get(block_index)) {
            let marker = match flag.status {
                PaneReviewStatus::Flagged => Some("Review suggested".to_string()),
                PaneReviewStatus::NoConfidenceRecorded if per_block_no_confidence => {
                    Some("Review unavailable".to_string())
                }
                _ => None,
            };
            if let Some(marker) = marker {
                section = section.child(
                    Label::new(SharedString::from(marker))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted)
                        .mt_1(),
                );
            }
        }
        if block.word_char_ranges.is_empty() {
            // Text-only view: no word map, no interaction — a wrapping
            // StyledText (a single-line Label would overflow the same
            // way the interactive blocks did).
            return section
                .child(div().min_w_0().child(StyledText::new(block.text.clone())))
                .into_any_element();
        }
        let mut highlights = Vec::new();
        if let Some(current) = current_word
            && let Some(local) = block
                .word_starts
                .iter()
                .position(|(index, _)| *index == current)
        {
            highlights.push((block.word_char_ranges[local].clone(), highlight_style));
        }
        // The pending selection's anchor carries the same mark.
        if let Some(anchor) = selection_anchor
            && let Some(local) = block
                .word_starts
                .iter()
                .position(|(index, _)| *index == anchor)
        {
            highlights.push((block.word_char_ranges[local].clone(), highlight_style));
        }
        // Highlight overlays (S4): researcher/model ranges painted in
        // deterministic label colors — only on index-identical views
        // (D5: the panel never re-implements the server's projection).
        // Strike marks (R2): struck words render line-through under the
        // same gate.
        if matches!(state.alignment.as_str(), "original" | "aligned") {
            for highlight in &state.highlights {
                if let Some(range) =
                    block_word_char_range(block, highlight.start_word, highlight.end_word)
                {
                    highlights.push((
                        range,
                        HighlightStyle {
                            background_color: Some(highlight_label_color(&highlight.label)),
                            ..Default::default()
                        },
                    ));
                }
            }
            for strike in &state.strikes {
                if let Some(range) =
                    block_word_char_range(block, strike.start_word, strike.end_word)
                {
                    highlights.push((range, strike_style));
                }
            }
        }
        let word_starts = block.word_starts.clone();
        let entity = cx.entity().downgrade();
        let interactive = InteractiveText::new(
            SharedString::from(format!("media-transcript-paragraph-{block_index}")),
            StyledText::new(block.text.clone())
                .with_highlights(resolved_highlight_runs(highlights)),
        )
        .on_click(
            block.word_char_ranges.clone(),
            move |range_index, window, cx| {
                if let Some((global_word, start_ms)) = word_starts.get(range_index) {
                    entity
                        .update(cx, |viewer, cx| {
                            viewer.word_clicked(*global_word, *start_ms, window, cx)
                        })
                        .ok();
                }
            },
        );
        // Block layout (NOT flex): the text element wraps only when it
        // receives a definite width — a flex row lets the paragraph be
        // probed at max-content and laid out as one overflowing line
        // (operator report 2026-10-26). min_w_0 + block gives the pane
        // width down the chain.
        let container = div()
            .id(SharedString::from(format!(
                "media-transcript-block-{block_index}"
            )))
            .min_w_0();
        let marked = current_word
            .is_some_and(|word| block.word_starts.iter().any(|(index, _)| *index == word));
        let container = if marked {
            container
                // Test-support hook (S3d): the follow-along visibility
                // test locates the marked block through this selector
                // (noop in release).
                .debug_selector(|| "media-transcript-marked-block".into())
        } else if block_index == 0 {
            container
                // Test-support hook: exposes this element's laid-out
                // bounds to layout ground-truth tests (noop in release).
                .debug_selector(|| "media-transcript-paragraph".into())
        } else {
            container
        };
        section
            .child(container.child(interactive))
            .into_any_element()
    }

    /// The highlights tail section (S4): labeled selection rows — seek,
    /// reel-inclusion toggle, rename, delete — on index-identical views.
    fn render_workspace_highlights_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        if state.highlights.is_empty() {
            return div().into_any_element();
        }
        let index_identical = matches!(state.alignment.as_str(), "original" | "aligned");
        let mut section = v_flex().gap_2();
        section = section.child(Self::section_label("Highlights"));
        if !index_identical {
            section = section.child(
                Label::new(SharedString::from(
                    "Timed navigation and text overlays are unavailable for this view.",
                ))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            );
        }
        for (row_index, highlight) in state.highlights.iter().enumerate() {
            let row = if index_identical {
                format!(
                    "{} — {}",
                    highlight.label,
                    transcript_range_excerpt(state, highlight.start_word, highlight.end_word)
                )
            } else {
                highlight.label.clone()
            };
            let start_ms = if index_identical {
                state
                    .words
                    .as_deref()
                    .and_then(|words| words.get(highlight.start_word))
                    .map(|word| word.start_ms)
            } else {
                None
            };
            let row_element = div()
                .id(SharedString::from(format!(
                    "media-transcript-highlight-row-{row_index}"
                )))
                .cursor_pointer()
                .flex()
                .flex_wrap()
                .gap_2()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .child(
                    Label::new(SharedString::from(row))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                );
            if let Some(start_ms) = start_ms {
                section =
                    section.child(row_element.on_click(cx.listener(move |this, _, _, cx| {
                        this.seek_workspace_transcript(start_ms, cx);
                    })));
            } else {
                section = section.child(row_element);
            }
            if highlight.researcher_supplied {
                let annotation_id = highlight.annotation_id.clone();
                let (start_word, end_word) = (highlight.start_word, highlight.end_word);
                let included = self.reel_selection.contains(&(start_word, end_word));
                let include_toggle = div()
                    .id(SharedString::from(format!(
                        "media-transcript-reel-toggle-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if included {
                            "Included in reel"
                        } else {
                            "Add to reel"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if included {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.reel_selection.remove(&(start_word, end_word)) {
                            this.reel_selection.insert((start_word, end_word));
                        }
                        cx.notify();
                    }));
                let edit = HighlightEdit {
                    kind: HighlightEditKind::Rename {
                        annotation_id: highlight.annotation_id.clone(),
                        start_word: highlight.start_word,
                        end_word: highlight.end_word,
                        note: highlight.note.clone(),
                    },
                    buffer: LabelBuffer::from_str(highlight.label.as_str()),
                };
                let rename = div()
                    .id(SharedString::from(format!(
                        "media-transcript-highlight-rename-{}",
                        highlight.annotation_id
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Rename"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_label_editor(edit.clone(), window, cx);
                    }));
                let delete = div()
                    .id(SharedString::from(format!(
                        "media-transcript-highlight-delete-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Delete"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Error),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.delete_workspace_annotation(annotation_id.clone(), "Highlight", cx);
                    }));
                section = section.child(
                    h_flex()
                        .flex_wrap()
                        .gap_3()
                        .child(include_toggle)
                        .child(rename)
                        .child(delete),
                );
            }
        }
        section.into_any_element()
    }

    /// The cuts tail section (R2): struck ranges with seek and Restore.
    fn render_workspace_cuts_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        if state.strikes.is_empty() {
            return div().into_any_element();
        }
        let index_identical = matches!(state.alignment.as_str(), "original" | "aligned");
        let mut section = v_flex().gap_2();
        section = section.child(Self::section_label("Cuts"));
        if !index_identical {
            section = section.child(
                Label::new(SharedString::from(
                    "Timed navigation and line-through overlays are unavailable for this view.",
                ))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            );
        }
        for strike in &state.strikes {
            let row = if index_identical {
                transcript_range_excerpt(state, strike.start_word, strike.end_word)
            } else {
                "Cut selection".to_string()
            };
            let start_ms = if index_identical {
                state
                    .words
                    .as_deref()
                    .and_then(|words| words.get(strike.start_word))
                    .map(|word| word.start_ms)
            } else {
                None
            };
            let row_element = div()
                .id(SharedString::from(format!(
                    "media-transcript-strike-row-{}",
                    strike.annotation_id
                )))
                .cursor_pointer()
                .flex()
                .flex_wrap()
                .gap_2()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .child(
                    Label::new(SharedString::from(row))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                );
            if let Some(start_ms) = start_ms {
                section =
                    section.child(row_element.on_click(cx.listener(move |this, _, _, cx| {
                        this.seek_workspace_transcript(start_ms, cx);
                    })));
            } else {
                section = section.child(row_element);
            }
            let annotation_id = strike.annotation_id.clone();
            section = section.child(
                div()
                    .id(SharedString::from(format!(
                        "media-transcript-strike-delete-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Restore"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.delete_workspace_annotation(annotation_id.clone(), "Strike", cx);
                    })),
            );
        }
        section.into_any_element()
    }

    /// The reel tail section (S5): plain-language compose actions, visible
    /// only when their inputs exist.
    fn render_workspace_reel_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        let any_researcher_highlight = state
            .highlights
            .iter()
            .any(|highlight| highlight.researcher_supplied);
        let affordances = [
            (ReelComposeMode::Assemble, any_researcher_highlight),
            (ReelComposeMode::Reduce, !state.strikes.is_empty()),
            (
                ReelComposeMode::Mixed,
                any_researcher_highlight && !state.strikes.is_empty(),
            ),
        ];
        let mut section = v_flex().gap_2();
        if affordances.iter().any(|(_, visible)| *visible) {
            section = section.child(Self::section_label("Create a reel"));
        }
        for (mode, visible) in affordances {
            if !visible {
                continue;
            }
            section = section.child(
                div()
                    .id(SharedString::from(format!(
                        "media-transcript-compose-reel-{}",
                        mode.key()
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(mode.label()))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.compose_workspace_reel(mode, cx);
                    })),
            );
        }
        section.into_any_element()
    }

    /// The Transcript tab's idle state: the recordings browser — every
    /// stored transcript, click-to-open (operator report 2026-10-26: the
    /// tab's empty state must list what's available, not dead-end).
    fn render_transcript_browser(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if let ResourceState::Failed(error) = &self.transcript_request.state {
            return self
                .render_empty("Transcript unavailable", error.as_str())
                .into_any_element();
        }
        let Some(listing) = self.transcript_listing.as_ref() else {
            let (title, hint) = match &self.transcript_listing_request.state {
                ResourceState::Loading => ("Loading recordings", "Fetching stored transcripts…"),
                ResourceState::Failed(error) => ("Transcripts unavailable", error.as_str()),
                _ => (
                    "No transcripts",
                    "Transcribe a recording to begin — it will appear here.",
                ),
            };
            return self.render_empty(title, hint).into_any_element();
        };
        if listing.is_empty() {
            return self
                .render_empty(
                    "No transcripts",
                    "Transcribe a recording to begin — it will appear here.",
                )
                .into_any_element();
        }
        let mut sections = v_flex().gap_2().p_3();
        sections = sections.child(Self::tab_purpose(
            "Choose a recording to read, select text, and create a reel.",
        ));
        sections = sections.child(Self::section_label("Recordings"));
        for transcript in listing {
            let timings = if transcript.has_word_timings {
                ""
            } else {
                " · text only"
            };
            let transcript_id = transcript.id.clone();
            let title = recording_display_name(&transcript.media_path);
            let metadata = format!(
                "{} · {} words{timings}",
                format_recording_duration(transcript.duration_secs),
                transcript.words_count
            );
            let mut row = div()
                .id(SharedString::from(format!(
                    "media-transcript-open-{}",
                    transcript.id
                )))
                .cursor_pointer()
                .flex()
                .flex_col()
                .gap_1()
                .py_1()
                .flex_1()
                .min_w_0()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .child(
                    Label::new(SharedString::from(title))
                        .size(LabelSize::Small)
                        .color(ui::Color::Default),
                )
                .child(
                    Label::new(SharedString::from(metadata))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                );
            // The capability note says what a click opens before the click:
            // gated recordings flag their read-only state on the row rather
            // than surfacing the gate inside the workspace.
            if let Some(note) = transcript_alignment_note(&transcript.alignment) {
                row = row.child(
                    Label::new(SharedString::from(note))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Error),
                );
            }
            // The Delete affordance is a SIBLING of the clickable open area,
            // never nested in it (the Library row pattern), so opening and
            // deleting can never fire together. Two clicks: Delete arms the
            // confirmation, Confirm delete dispatches the server's
            // `educt_delete_transcript` (durable exports and rendered reels
            // are preserved by the server, detached from the removed record).
            let confirming = self.confirm_delete_transcript.is_armed(&transcript.id);
            let delete_row_id = transcript.id.clone();
            let delete_button = div()
                .id(SharedString::from(format!(
                    "media-transcript-delete-{}",
                    transcript.id
                )))
                .debug_selector(move || {
                    if confirming {
                        "media-transcript-delete-confirm".into()
                    } else {
                        "media-transcript-delete".into()
                    }
                })
                .cursor_pointer()
                .px_2()
                .py_1()
                .flex_none()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .child(
                    Label::new(if confirming {
                        SharedString::from("Confirm delete")
                    } else {
                        SharedString::from("Delete")
                    })
                    .size(LabelSize::XSmall)
                    .color(if confirming {
                        ui::Color::Error
                    } else {
                        ui::Color::Muted
                    }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.confirm_delete_transcript.confirm(&delete_row_id) {
                        this.delete_transcript_recording(delete_row_id.clone(), cx);
                    } else {
                        cx.notify();
                    }
                }));
            sections = sections.child(
                div()
                    .id(SharedString::from(format!(
                        "media-transcript-row-{}",
                        transcript.id
                    )))
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(row.on_click(cx.listener(move |this, _, _, cx| {
                        this.open_transcript(transcript_id.clone(), cx);
                    })))
                    .child(delete_button),
            );
        }
        v_flex()
            .id("media-viewer-transcript-browser")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .child(sections)
            .into_any_element()
    }

    fn section_label(title: &str) -> impl IntoElement + use<> {
        Label::new(SharedString::from(title.to_string()))
            .size(LabelSize::XSmall)
            .color(ui::Color::Hint)
            .mt_1()
    }

    /// One line of tab purpose copy — the orientation contract (2026-10-27):
    /// every tab states what it is for before it shows content.
    fn tab_purpose(text: &str) -> Div {
        div().px_3().pt_2().child(
            Label::new(SharedString::from(text.to_string()))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
        )
    }

    fn kv_row(key: &str, value: &str, _cx: &Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(
                Label::new(SharedString::from(key.to_string()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(value.to_string()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Default),
            )
    }

    fn render_empty(&self, title: &str, hint: &str) -> impl IntoElement + use<> {
        v_flex()
            .size_full()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_1()
            .child(
                Label::new(SharedString::from(title.to_string()))
                    .size(LabelSize::Small)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(hint.to_string()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint),
            )
    }
}

fn resource_status(
    label: &str,
    state: &ResourceState,
    has_last_good: bool,
    loading_detail: Option<&str>,
) -> Option<(SharedString, ui::Color)> {
    match state {
        ResourceState::Loading => Some((
            loading_detail
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if has_last_good {
                        format!("Refreshing {label} — showing last-good data…")
                    } else {
                        format!("Loading {label}…")
                    }
                })
                .into(),
            ui::Color::Muted,
        )),
        ResourceState::Failed(error) => Some((
            if has_last_good {
                format!("{label} degraded — showing last-good data: {error}")
            } else {
                format!("{label} failed: {error}")
            }
            .into(),
            ui::Color::Error,
        )),
        ResourceState::Idle | ResourceState::Ready => None,
    }
}

fn parse_gallery_listing(
    payload: Option<Value>,
    requested_offset: usize,
) -> Result<GalleryListing, String> {
    let payload = payload
        .ok_or_else(|| "gallery_list_assets returned unparsable or failed output.".to_string())?;
    let object = payload
        .as_object()
        .ok_or_else(|| "gallery_list_assets returned a non-object payload.".to_string())?;
    let gallery_id = object
        .get("gallery_id")
        .and_then(Value::as_str)
        .filter(|gallery_id| !gallery_id.is_empty())
        .ok_or_else(|| "gallery_list_assets returned no gallery_id.".to_string())?
        .to_string();
    let total = object
        .get("total")
        .and_then(Value::as_u64)
        .ok_or_else(|| "gallery_list_assets returned no valid total.".to_string())?;
    let offset = object
        .get("offset")
        .and_then(Value::as_u64)
        .and_then(|offset| usize::try_from(offset).ok())
        .ok_or_else(|| "gallery_list_assets returned no valid offset.".to_string())?;
    if offset != requested_offset {
        return Err(format!(
            "gallery_list_assets returned offset {offset} for requested offset {requested_offset}."
        ));
    }
    let limit = object
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|limit| usize::try_from(limit).ok())
        .filter(|limit| *limit > 0)
        .ok_or_else(|| "gallery_list_assets returned no valid limit.".to_string())?;
    let assets = object
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| "gallery_list_assets returned no assets array.".to_string())?;
    if assets.len() > limit {
        return Err("gallery_list_assets returned more rows than its limit.".into());
    }
    for (position, asset) in assets.iter().enumerate() {
        let record = asset
            .as_object()
            .ok_or_else(|| format!("gallery_list_assets row {position} is not an object."))?;
        let expected_index = offset
            .checked_add(position)
            .ok_or_else(|| "gallery_list_assets row index overflowed.".to_string())?;
        let index = record
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| format!("gallery_list_assets row {position} has no valid index."))?;
        if index != expected_index {
            return Err(format!(
                "gallery_list_assets row {position} has index {index}, expected {expected_index}."
            ));
        }
        for field in ["id", "path", "media_type"] {
            if record
                .get(field)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(format!(
                    "gallery_list_assets row {position} has no valid {field}."
                ));
            }
        }
    }
    Ok(GalleryListing {
        gallery_id,
        total,
        offset,
        limit,
        assets: assets.clone(),
    })
}

fn parse_delete_acknowledgement(output: &str, expected_id: &str) -> Result<usize, String> {
    let payload = hkask_types::tool_response::parse_tool_response(output)
        .ok_or_else(|| "gallery_delete_image returned malformed data".to_string())?;
    if payload.get("deleted").and_then(Value::as_bool) != Some(true)
        || payload.get("image_id").and_then(Value::as_str) != Some(expected_id)
        || payload.get("file_deleted").and_then(Value::as_bool) != Some(false)
        || payload.get("gallery_changed").and_then(Value::as_bool) != Some(true)
    {
        return Err("gallery_delete_image returned an invalid acknowledgement".into());
    }
    payload
        .get("transcripts_detached")
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| "gallery_delete_image returned no valid transcript count".to_string())
}

/// Strictly parse an `educt_list_transcripts` response into Detail rows.
/// Every summary must carry a non-empty `id`, a `words_count`, and a
/// `has_word_timings` flag, and the declared `count` must match the rows.
/// A malformed listing is an error, never an empty success — degradation
/// The exact `gallery_annotation_create` request.
fn annotation_create_params(gallery_asset_id: &str, kind: &str, value: &str) -> serde_json::Value {
    serde_json::json!({
        "gallery_asset_id": gallery_asset_id,
        "kind": kind,
        "value": value,
    })
}

/// The exact `gallery_annotation_update` request — stable identity plus
/// the replacement value.
fn annotation_update_params(
    gallery_asset_id: &str,
    annotation_id: &str,
    value: &str,
) -> serde_json::Value {
    serde_json::json!({
        "gallery_asset_id": gallery_asset_id,
        "annotation_id": annotation_id,
        "value": value,
    })
}

/// The exact `gallery_annotation_delete` request.
fn annotation_delete_params(gallery_asset_id: &str, annotation_id: &str) -> serde_json::Value {
    serde_json::json!({
        "gallery_asset_id": gallery_asset_id,
        "annotation_id": annotation_id,
    })
}

/// Strict-parse `gallery_annotation_list` rows — a malformed row rejects
/// the whole listing, never degrades to an empty success (the house
/// pattern).
fn parse_annotation_listing(payload: &Value) -> Result<Vec<PaneAnnotation>, String> {
    let annotations = payload
        .get("annotations")
        .and_then(Value::as_array)
        .ok_or_else(|| "gallery_annotation_list returned no annotations array.".to_string())?;
    let mut rows = Vec::with_capacity(annotations.len());
    for annotation in annotations {
        let object = annotation
            .as_object()
            .ok_or_else(|| "annotation row is not an object.".to_string())?;
        let annotation_id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "annotation row has no id.".to_string())?
            .to_string();
        let kind = object
            .get("tag_type")
            .and_then(Value::as_str)
            .filter(|kind| matches!(*kind, "tag" | "note"))
            .ok_or_else(|| "annotation row has no tag/note kind.".to_string())?
            .to_string();
        let value = object
            .get("value")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "annotation row has no value.".to_string())?
            .to_string();
        let created_at = object
            .get("created_at")
            .and_then(Value::as_str)
            .filter(|created| !created.is_empty())
            .ok_or_else(|| "annotation row has no created_at.".to_string())?
            .to_string();
        rows.push(PaneAnnotation {
            annotation_id,
            kind,
            value,
            created_at,
        });
    }
    Ok(rows)
}

/// surfaces, it does not masquerade as "no transcripts".
fn parse_transcript_listing(payload: &Value) -> Result<Vec<TranscriptSummaryRow>, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "educt_list_transcripts returned no object data.".to_string())?;
    let transcripts = object
        .get("transcripts")
        .and_then(Value::as_array)
        .ok_or_else(|| "educt_list_transcripts returned no transcripts array.".to_string())?;
    let count = object
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| "educt_list_transcripts returned no count.".to_string())?;
    if count != transcripts.len() as u64 {
        return Err(format!(
            "educt_list_transcripts count {count} does not match {} rows.",
            transcripts.len()
        ));
    }
    transcripts
        .iter()
        .map(|row| {
            let object = row
                .as_object()
                .ok_or_else(|| "transcript summary is not an object.".to_string())?;
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "transcript summary has no id.".to_string())?;
            let media_path = object
                .get("media_path")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .ok_or_else(|| "transcript summary has no media_path.".to_string())?;
            let words_count = object
                .get("words_count")
                .and_then(Value::as_u64)
                .ok_or_else(|| "transcript summary has no words_count.".to_string())?;
            let has_word_timings = object
                .get("has_word_timings")
                .and_then(Value::as_bool)
                .ok_or_else(|| "transcript summary has no has_word_timings.".to_string())?;
            let duration_secs = object
                .get("audio_duration_secs")
                .and_then(Value::as_f64)
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .ok_or_else(|| "transcript summary has no valid audio_duration_secs.".to_string())?
                .round() as u64;
            let alignment = object
                .get("alignment")
                .and_then(Value::as_str)
                .filter(|alignment| {
                    matches!(
                        *alignment,
                        "original" | "aligned" | "re_anchored" | "unaligned" | "untimed"
                    )
                })
                .ok_or_else(|| {
                    "transcript summary has no valid alignment (original, aligned, re_anchored, unaligned, or untimed)."
                        .to_string()
                })?
                .to_string();
            Ok(TranscriptSummaryRow {
                id: id.to_string(),
                media_path: media_path.to_string(),
                words_count,
                duration_secs,
                has_word_timings,
                alignment,
            })
        })
        .collect()
}

/// The recordings-browser / Detail row's capability note for a transcript's
/// alignment — None for the hand-annotatable states (the Reduct-normal
/// default: every row is an editable bundle unless flagged), a named gate
/// note for the read-only ones, so the entry surfaces say what a click
/// opens (the F10 alignment flags, 2026-10-27).
fn recording_display_name(media_path: &str) -> String {
    let stem = std::path::Path::new(media_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let compact: String = stem.chars().filter(|character| *character != '-').collect();
    if matches!(compact.len(), 32 | 64)
        && compact
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return "Untitled recording".to_string();
    }
    let display = stem.replace(['-', '_'], " ");
    let display = display.trim();
    if display.is_empty() {
        "Untitled recording".to_string()
    } else {
        display.to_string()
    }
}

fn format_recording_duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn transcript_range_excerpt(
    state: &TranscriptPaneState,
    start_word: usize,
    end_word: usize,
) -> String {
    let Some(words) = state.words.as_deref() else {
        return "Selected text".to_string();
    };
    if start_word > end_word || end_word >= words.len() {
        return "Selected text".to_string();
    }
    let text = words[start_word..=end_word]
        .iter()
        .map(|word| word.word.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = text.chars();
    let excerpt: String = chars.by_ref().take(72).collect();
    if chars.next().is_some() {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}

fn transcript_alignment_note(alignment: &str) -> Option<&'static str> {
    match alignment {
        "re_anchored" => Some("Estimated timings · review only"),
        "unaligned" => Some("Timed editing unavailable"),
        _ => None,
    }
}

/// The reel-provenance line for the Detail view, from the detail payload's
/// `transcript_render` record: the source transcript this asset was rendered
/// from, with its creation time. Absent or null (most assets are not reels)
/// renders nothing; a malformed record renders nothing rather than breaking
/// the whole detail view.
fn parse_reel_provenance(detail: &Value) -> Option<String> {
    let render = detail.get("transcript_render")?.as_object()?;
    let source = render.get("source_transcript_id")?.as_str()?;
    if source.is_empty() {
        return None;
    }
    let created = render
        .get("created_at")
        .and_then(Value::as_str)
        .unwrap_or("unknown time");
    Some(format!("{source} (created {created})"))
}

/// Deterministic label color (D8): the label's FNV-1a hash spreads hues —
/// no palette editor, no settings.
fn highlight_label_color(label: &str) -> gpui::Hsla {
    let hash = label.bytes().fold(0x811c_9dc5_u32, |acc, byte| {
        (acc ^ byte as u32).wrapping_mul(0x0100_0193)
    });
    let hue = (hash % 360) as f32 / 360.0;
    gpui::hsla(hue, 0.45, 0.5, 0.35)
}

/// The word-click state machine's outcome (S4 part-2, S4-3).
#[derive(Clone, Debug, PartialEq, Eq)]
enum WordClickOutcome {
    /// Selection mode is off — the click seeks.
    SeekOnly,
    /// The click set the pending selection's anchor.
    AnchorSet(usize),
    /// The click repeated the anchor — the pending selection cleared.
    AnchorCleared,
    /// The label editor is open — the click is inert (the editor is modal
    /// to its range).
    EditorOpen,
    /// The click completed a range — open the label editor over
    /// [start, end] (the alignment gate runs before the editor opens).
    OpenLabelEditor { start_word: usize, end_word: usize },
}

/// The label editor's open target (S4-3): a completed two-click range
/// awaiting its label, or a researcher highlight being renamed (the range
/// and note pass through `educt_annotation_update` unchanged — only the
/// label is replaced).
#[derive(Clone, Debug)]
enum HighlightEditKind {
    Create {
        start_word: usize,
        end_word: usize,
    },
    Rename {
        annotation_id: String,
        start_word: usize,
        end_word: usize,
        note: String,
    },
}

/// The open label editor (S4-3): its target and the typed text.
#[derive(Clone, Debug)]
struct HighlightEdit {
    kind: HighlightEditKind,
    buffer: LabelBuffer,
}

/// A single-line text buffer with a cursor — the label input's model.
/// Char-indexed so multi-byte labels edit as characters, not bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LabelBuffer {
    chars: Vec<char>,
    cursor: usize,
}

impl LabelBuffer {
    fn from_str(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
        }
    }

    /// The typed text, untrimmed — the render reads it verbatim.
    fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// The trimmed label to dispatch.
    fn label(&self) -> String {
        self.text().trim().to_string()
    }

    /// The cursor's char index (the render splits the text there).
    fn cursor(&self) -> usize {
        self.cursor.min(self.chars.len())
    }

    fn insert(&mut self, ch: char) {
        self.cursor = self.cursor.min(self.chars.len());
        self.chars.insert(self.cursor, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        self.cursor = self.cursor.min(self.chars.len());
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    fn delete_forward(&mut self) {
        self.cursor = self.cursor.min(self.chars.len());
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.chars.len();
    }

    /// Insert pasted text, dropping control characters — the input is
    /// single-line (2026-10-26 review: labels could not be pasted at all).
    fn insert_str(&mut self, text: &str) {
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            self.insert(ch);
        }
    }

    /// Apply an editing keystroke (backspace, delete, cursor moves, typed
    /// characters). Enter/Escape are the viewer's concern, not the
    /// buffer's. Returns whether the buffer consumed the key.
    fn apply_keystroke(&mut self, keystroke: &Keystroke) -> bool {
        match keystroke.key.as_str() {
            "backspace" => self.backspace(),
            "delete" => self.delete_forward(),
            "left" => self.move_left(),
            "right" => self.move_right(),
            "home" => self.move_home(),
            "end" => self.move_end(),
            _ => match typed_char(keystroke) {
                Some(ch) => self.insert(ch),
                None => return false,
            },
        }
        true
    }
}

/// The character a keystroke types, if any — `key_char` when the platform
/// supplies it (shifted symbols, non-ASCII layouts), the plain key as the
/// fallback (shift uppercases ASCII). Modifier chords (ctrl/alt/cmd/fn)
/// are shortcuts, not typing.
fn typed_char(keystroke: &Keystroke) -> Option<char> {
    let modifiers = &keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
        return None;
    }
    if let Some(key_char) = &keystroke.key_char {
        let mut chars = key_char.chars();
        let first = chars.next()?;
        return if chars.next().is_none() {
            Some(first)
        } else {
            None
        };
    }
    if keystroke.key == "space" {
        return Some(' ');
    }
    let mut chars = keystroke.key.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(if modifiers.shift {
        first.to_ascii_uppercase()
    } else {
        first
    })
}

/// The completion gate for hand highlights (D5): only index-identical
/// views can source-map a hand-created range — the panel never
/// re-implements the server's projection. Returns the visible reason
/// when the view blocks creation (S4 AC3's five states).
fn highlight_range_gate(alignment: &str) -> Result<(), &'static str> {
    match alignment {
        "original" | "aligned" => Ok(()),
        "re_anchored" => Err(
            "Direct selection is unavailable for this corrected transcript. Review existing ranges or re-transcribe the section for precise timing.",
        ),
        "unaligned" => Err(
            "Direct selection needs a timing-aligned transcript. Re-transcribe or realign this correction first.",
        ),
        "untimed" => {
            Err("Direct selection needs word-level timings; this transcript is text only.")
        }
        _ => Err("Direct selection is unavailable for this transcript."),
    }
}

/// The create-confirm label (S4-3): the typed text, or the default label
/// when the editor is confirmed empty (the two-click + Enter fast path).
fn resolve_create_label(typed: &str) -> String {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        "Highlight".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The exact `educt_annotation_create` request for a pane-created
/// highlight: the label editor's text and a provenance-honest note.
fn highlight_create_params(
    transcript_id: &str,
    start_word: usize,
    end_word: usize,
    label: &str,
) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "start_word": start_word,
        "end_word": end_word,
        "label": label,
        "note": "two-click selection from the transcript pane",
    })
}

/// The exact `educt_annotation_create` request for a pane strike (R2):
/// kind strike with blank label and note — the server enforces the
/// unannotated-cut invariant; the pane asks for it explicitly.
fn strike_create_params(
    transcript_id: &str,
    start_word: usize,
    end_word: usize,
) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "start_word": start_word,
        "end_word": end_word,
        "label": "",
        "note": "",
        "kind": "strike",
    })
}

/// Compose the pane's text marks into the sorted, non-overlapping run list
/// gpui's `with_highlights` consumes. Its cursor runs forward only, so an
/// out-of-order or overlapping mark would style text past its range — a
/// struck word inside a highlight could lose its line-through, and the
/// follow-along mark inside any overlay could shift styling past its
/// word. Overlapping marks merge field-wise (the later mark's set fields
/// win), so a struck word inside a highlight keeps BOTH the background
/// and the line-through. The cost is bounded by the marked envelope, not
/// the block length.
fn resolved_highlight_runs(
    marks: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if marks.is_empty() {
        return Vec::new();
    }
    let envelope_start = marks
        .iter()
        .map(|(range, _)| range.start)
        .min()
        .unwrap_or(0);
    let envelope_end = marks.iter().map(|(range, _)| range.end).max().unwrap_or(0);
    let mut cells: Vec<Option<HighlightStyle>> =
        vec![None; envelope_end.saturating_sub(envelope_start)];
    for (range, style) in marks {
        let start = range.start.max(envelope_start);
        let end = range.end.min(envelope_end);
        for cell in &mut cells[start - envelope_start..end - envelope_start] {
            *cell = Some(match cell.take() {
                None => style,
                Some(previous) => previous.highlight(style),
            });
        }
    }
    let mut runs = Vec::new();
    let mut open: Option<(usize, HighlightStyle)> = None;
    for (offset, cell) in cells.into_iter().enumerate() {
        let index = offset + envelope_start;
        match (open, cell) {
            (None, Some(style)) => open = Some((index, style)),
            (Some((start, style)), Some(next)) if style != next => {
                runs.push((start..index, style));
                open = Some((index, next));
            }
            (Some((start, style)), None) => {
                runs.push((start..index, style));
                open = None;
            }
            _ => {}
        }
    }
    if let Some((start, style)) = open {
        runs.push((start..envelope_end, style));
    }
    runs
}

/// The char range within `block` covering the word range
/// `start_word..=end_word` — None when the range misses the block or its
/// clamped local indices fall outside the word map. The block owns the
/// global→local mapping (D5); callers never re-implement it.
fn block_word_char_range(
    block: &PaneBlock,
    start_word: usize,
    end_word: usize,
) -> Option<Range<usize>> {
    let block_first = block
        .word_starts
        .first()
        .map(|(index, _)| *index)
        .unwrap_or(0);
    let block_last = block
        .word_starts
        .last()
        .map(|(index, _)| *index)
        .unwrap_or(0);
    if start_word > block_last || end_word < block_first {
        return None;
    }
    let first = block
        .word_char_ranges
        .get(start_word.max(block_first) - block_first)?;
    let last = block
        .word_char_ranges
        .get(end_word.min(block_last) - block_first)?;
    Some(first.start..last.end)
}

/// The exact `educt_annotation_update` request for a pane rename (S4-3):
/// stable identity plus the replacement label over the unchanged range
/// and note.
fn highlight_update_params(
    transcript_id: &str,
    annotation_id: &str,
    start_word: usize,
    end_word: usize,
    label: &str,
    note: &str,
) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "annotation_id": annotation_id,
        "start_word": start_word,
        "end_word": end_word,
        "label": label,
        "note": note,
    })
}

/// The pane's three reel compose modes (R3): assemble keeps the toggled-in
/// researcher highlights, reduce cuts the strikes out of the whole
/// transcript, and mixed keeps the toggled-in highlights minus the strikes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReelComposeMode {
    Assemble,
    Reduce,
    Mixed,
}

impl ReelComposeMode {
    /// The compose affordance's element-id slug.
    fn key(self) -> &'static str {
        match self {
            Self::Assemble => "assemble",
            Self::Reduce => "reduce",
            Self::Mixed => "mixed",
        }
    }

    /// The compose affordance's label.
    fn label(self) -> &'static str {
        match self {
            Self::Assemble => "From included highlights",
            Self::Reduce => "From the recording with cuts applied",
            Self::Mixed => "From included highlights with cuts applied",
        }
    }

    /// The nothing-to-compose status for the mode.
    fn empty_status(self) -> &'static str {
        match self {
            Self::Assemble => "No included highlights — include at least one highlight first.",
            Self::Reduce => "No cuts to apply — cut a selection first.",
            Self::Mixed => "Include highlights and cut selections before creating this reel.",
        }
    }

    /// The in-flight edit progress for the mode.
    fn progress(self) -> &'static str {
        match self {
            Self::Assemble => "Creating reel from included highlights…",
            Self::Reduce => "Creating reel with cuts applied…",
            Self::Mixed => "Creating reel from included highlights with cuts applied…",
        }
    }
}

/// Union inclusive word ranges: sort, then merge overlapping AND adjacent
/// ranges — mirroring the server's union_ranges (adjacent ranges share no
/// words between them, so merging adds none). Keeps stay pairwise disjoint
/// through this, satisfying the selection algebra's disjoint-Keep check.
fn union_word_ranges(ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
    sorted.sort();
    let mut union: Vec<(usize, usize)> = Vec::new();
    for (start, end) in sorted {
        match union.last_mut() {
            Some((_, last_end)) if start <= last_end.saturating_add(1) => {
                if end > *last_end {
                    *last_end = end;
                }
            }
            _ => union.push((start, end)),
        }
    }
    union
}

/// The exact `educt_store_layer` request for a pane-composed reel (R3): the
/// EDL built directly from the pane's annotations — Keep ops from the
/// toggled-in researcher highlights (union-merged so overlapping
/// selections never trip the algebra's disjoint-Keep check) and Cut ops
/// from every strike. Assemble keeps only, reduce cuts only, mixed carries
/// both; `None` when the mode has nothing to compose. Provenance carries
/// the pane_reel_compose template — never researcher_annotation — so a
/// single-cut reduce EDL can never masquerade as a researcher strike when
/// the pane re-parses its own transcript. Membership keys on provenance
/// and the toggle, never the label text: free-text labels name clips for
/// the human without affecting reel composition.
fn compose_reel_params(
    state: &TranscriptPaneState,
    selection: &HashSet<(usize, usize)>,
    mode: ReelComposeMode,
) -> Option<serde_json::Value> {
    let keeps: Vec<(usize, usize)> = match mode {
        ReelComposeMode::Reduce => Vec::new(),
        ReelComposeMode::Assemble | ReelComposeMode::Mixed => state
            .highlights
            .iter()
            .filter(|highlight| {
                highlight.researcher_supplied
                    && selection.contains(&(highlight.start_word, highlight.end_word))
            })
            .map(|highlight| (highlight.start_word, highlight.end_word))
            .collect(),
    };
    let cuts: Vec<(usize, usize)> = match mode {
        ReelComposeMode::Assemble => Vec::new(),
        ReelComposeMode::Reduce | ReelComposeMode::Mixed => state
            .strikes
            .iter()
            .map(|strike| (strike.start_word, strike.end_word))
            .collect(),
    };
    // Mixed is "keeps minus strikes": with nothing toggled in there is
    // nothing to keep, so it refuses with its empty status rather than
    // silently composing the reduce reel (cuts-only ops read as the whole
    // transcript under the algebra's subtractive base).
    if mode == ReelComposeMode::Mixed && keeps.is_empty() {
        return None;
    }
    let mut ops = Vec::new();
    for (start_word, end_word) in union_word_ranges(&keeps) {
        ops.push(serde_json::json!({
            "op": "keep",
            "range": {"start_word": start_word, "end_word": end_word},
        }));
    }
    for (start_word, end_word) in cuts {
        ops.push(serde_json::json!({
            "op": "cut",
            "range": {"start_word": start_word, "end_word": end_word},
        }));
    }
    if ops.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "transcript_id": state.transcript_id,
        "layer": {
            "kind": "edl",
            "provenance": {
                "model": "user",
                "prompt_template": "pane_reel_compose",
                "created_at": hkask_types::time::now_rfc3339(),
            },
            "ops": ops,
        }
    }))
}

/// The working word containing the playback position — the last word whose
/// start_ms is at or before it (S3c's follow-along index math). `None`
/// before the first word or for an empty view.
fn current_word_index(words: &[PaneWord], position_ms: u64) -> Option<usize> {
    words.iter().rposition(|word| word.start_ms <= position_ms)
}

/// The list item index of the block containing `word` — the same boundary
/// walk `build_pane_blocks` performs over the paragraph breaks, so the
/// follow-along can map its mark to a list item without rebuilding blocks
/// (S3d). Pinned against the builder by
/// `block_index_for_word_matches_block_boundaries`.
fn block_index_for_word(state: &TranscriptPaneState, word: usize) -> Option<usize> {
    let words_len = state.words.as_ref()?.len();
    if word >= words_len {
        return None;
    }
    let breaks = state.paragraph_breaks.as_deref().unwrap_or(&[]);
    let mut start = 0usize;
    let mut block = 0usize;
    for &break_after in breaks {
        if break_after >= words_len || break_after < start {
            continue;
        }
        if word <= break_after {
            return Some(block);
        }
        start = break_after + 1;
        block += 1;
    }
    Some(block)
}

/// Build the pane's paragraph blocks: paragraphs over the working words
/// (breaks honored; out-of-range breaks skipped), each labeled with the
/// speaker covering its first word. Text-only views (no words) yield a
/// single unlabeled block of the working text. The char ranges and word
/// starts are the click-to-seek and follow-along mapping tables.
fn build_pane_blocks(state: &TranscriptPaneState) -> Vec<PaneBlock> {
    let Some(words) = state.words.as_deref() else {
        return vec![PaneBlock {
            speaker: None,
            text: state.working_text.clone(),
            word_char_ranges: Vec::new(),
            word_starts: Vec::new(),
        }];
    };
    let breaks = state.paragraph_breaks.as_deref().unwrap_or(&[]);
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut start = 0usize;
    for &break_after in breaks {
        if break_after >= words.len() || break_after < start {
            continue;
        }
        ranges.push(start..break_after + 1);
        start = break_after + 1;
    }
    if start < words.len() {
        ranges.push(start..words.len());
    }
    if ranges.is_empty() {
        ranges.push(0..words.len());
    }
    ranges
        .into_iter()
        .map(|range| {
            let block_start = range.start;
            let mut text = String::new();
            let mut cursor = 0usize;
            let mut word_char_ranges = Vec::with_capacity(range.len());
            let mut word_starts = Vec::with_capacity(range.len());
            for global_index in range {
                let word = &words[global_index];
                if !text.is_empty() {
                    text.push(' ');
                    cursor += 1;
                }
                let char_count = word.word.chars().count();
                word_char_ranges.push(cursor..cursor + char_count);
                word_starts.push((global_index, word.start_ms));
                text.push_str(&word.word);
                cursor += char_count;
            }
            PaneBlock {
                speaker: speaker_at_word(state.speaker_spans.as_deref(), block_start),
                text,
                word_char_ranges,
                word_starts,
            }
        })
        .collect()
}

/// The media block body for a transcript's media path — the same
/// display-hint shape the viz-core registry keys shared widgets by, so
/// the workspace player is the SAME instance the Media tab and inline
/// renders use (D1: one player per media identity). Audio containers take
/// the audio player; everything else is a video source.
fn transcript_media_body(media_path: &str) -> String {
    let extension = media_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let kind = if matches!(
        extension.as_str(),
        "mp3" | "wav" | "m4a" | "aac" | "ogg" | "opus" | "flac" | "wma"
    ) {
        "audio"
    } else {
        "video"
    };
    serde_json::json!({ "kind": kind, "src": media_path }).to_string()
}

/// The speaker covering a working word index, if a speaker layer exists.
fn speaker_at_word(spans: Option<&[PaneSpeakerSpan]>, word_index: usize) -> Option<String> {
    spans?
        .iter()
        .find(|span| span.start_word <= word_index && word_index <= span.end_word)
        .map(|span| span.speaker.clone())
}

/// The newest layer of `kind` from a get_transcript layer list, as its raw
/// layer object — deterministic (created_at, then id), mirroring the
/// server's own select_layer_of_kind tie-break.
fn latest_layer_payload<'a>(layers: &'a [Value], kind: &str) -> Option<&'a Value> {
    layers
        .iter()
        .filter_map(|record| {
            let layer = record.get("layer")?;
            if layer.get("kind").and_then(Value::as_str) == Some(kind) {
                let created_at = record.get("created_at").and_then(Value::as_str)?;
                let record_id = record.get("id").and_then(Value::as_str)?;
                Some((created_at, record_id, layer))
            } else {
                None
            }
        })
        .max_by(|left, right| left.0.cmp(right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, _, layer)| layer)
}

/// Strictly parse an `educt_get_transcript` payload into the workspace
/// state. Consumes only what the pane renders — summary id/media path, the
/// working view (text + words + alignment), the latest paragraph layer's
/// breaks, the latest speaker layer's spans, and the layer rows for the
/// strip — and rejects a malformed payload outright: a half view is worse
/// than a visible error.
fn parse_transcript_payload(payload: &Value) -> Result<TranscriptPaneState, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "educt_get_transcript returned no object data.".to_string())?;
    let summary = object
        .get("summary")
        .and_then(Value::as_object)
        .ok_or_else(|| "educt_get_transcript returned no summary object.".to_string())?;
    let transcript_id = summary
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "transcript summary has no id.".to_string())?
        .to_string();
    let media_path = summary
        .get("media_path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "transcript summary has no media_path.".to_string())?
        .to_string();
    let working = object
        .get("working_transcript")
        .and_then(Value::as_object)
        .ok_or_else(|| "educt_get_transcript returned no working_transcript.".to_string())?;
    let working_text = working
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "working transcript has no text.".to_string())?
        .to_string();
    let alignment = working
        .get("alignment")
        .and_then(Value::as_str)
        .filter(|alignment| {
            matches!(
                *alignment,
                "original" | "aligned" | "re_anchored" | "unaligned" | "untimed"
            )
        })
        .ok_or_else(|| "working transcript has no valid alignment.".to_string())?
        .to_string();
    let alignment_note = working
        .get("alignment_error")
        .and_then(Value::as_str)
        .map(str::to_string);
    let words = if matches!(alignment.as_str(), "original" | "aligned" | "re_anchored") {
        let words = working
            .get("words")
            .and_then(Value::as_array)
            .filter(|words| !words.is_empty())
            .ok_or_else(|| {
                format!("working transcript alignment {alignment} requires timed words.")
            })?;
        Some(
            words
                .iter()
                .map(|word| {
                    let object = word
                        .as_object()
                        .ok_or_else(|| "working word is not an object.".to_string())?;
                    Ok(PaneWord {
                        word: object
                            .get("word")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "working word has no text.".to_string())?
                            .to_string(),
                        start_ms: object
                            .get("start_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "working word has no start_ms.".to_string())?,
                        end_ms: object
                            .get("end_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "working word has no end_ms.".to_string())?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        )
    } else {
        None
    };
    let layers_array = object
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(|| "educt_get_transcript returned no layers array.".to_string())?;
    let mut layers = Vec::with_capacity(layers_array.len());
    let mut highlights = Vec::new();
    let mut strikes = Vec::new();
    for layer in layers_array {
        let object = layer
            .as_object()
            .ok_or_else(|| "layer record is not an object.".to_string())?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "layer record has no id.".to_string())?
            .to_string();
        let layer_object = object
            .get("layer")
            .and_then(Value::as_object)
            .ok_or_else(|| "layer record has no layer object.".to_string())?;
        let kind = layer_object
            .get("kind")
            .and_then(Value::as_str)
            .filter(|kind| !kind.is_empty())
            .ok_or_else(|| "layer record has no kind.".to_string())?
            .to_string();
        if kind == "highlight" {
            // The pane's researcher affordances mirror the server's
            // mutate_user_highlight guard exactly: model "user" AND the
            // researcher_annotation template. Other user-authored highlight
            // layers — the legacy workspace_reel_gather compose copies — are pane
            // bookkeeping, never annotations: their entries are excluded
            // (2026-10-26 review: they re-entered the list as researcher
            // rows whose rename/delete the server always refuses, and every
            // compose added another copy).
            let provenance = layer_object.get("provenance");
            let researcher_supplied = provenance
                .and_then(|provenance| provenance.get("model"))
                .and_then(Value::as_str)
                == Some("user")
                && provenance
                    .and_then(|provenance| provenance.get("prompt_template"))
                    .and_then(Value::as_str)
                    == Some("researcher_annotation");
            let is_gather_layer = provenance
                .and_then(|provenance| provenance.get("prompt_template"))
                .and_then(Value::as_str)
                == Some("workspace_reel_gather");
            if is_gather_layer {
                // Gathered copies never render — the originals are already
                // in the list.
                let created_at = object
                    .get("created_at")
                    .and_then(Value::as_str)
                    .filter(|created| !created.is_empty())
                    .ok_or_else(|| "layer record has no created_at.".to_string())?
                    .to_string();
                layers.push(PaneLayerRow {
                    id,
                    kind,
                    created_at,
                });
                continue;
            }
            let entries = layer_object
                .get("highlights")
                .and_then(Value::as_array)
                .ok_or_else(|| "highlight layer has no highlights array.".to_string())?;
            for entry in entries {
                let entry = entry
                    .as_object()
                    .ok_or_else(|| "highlight entry is not an object.".to_string())?;
                let start_word = entry
                    .get("start_word")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| "highlight entry has no start_word.".to_string())?;
                let end_word = entry
                    .get("end_word")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| "highlight entry has no end_word.".to_string())?;
                let label = entry
                    .get("label")
                    .and_then(Value::as_str)
                    .filter(|label| !label.is_empty())
                    .ok_or_else(|| "highlight entry has no label.".to_string())?
                    .to_string();
                let note = entry
                    .get("note")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "highlight entry has no note.".to_string())?
                    .to_string();
                highlights.push(PaneHighlight {
                    start_word,
                    end_word,
                    label,
                    note,
                    annotation_id: id.clone(),
                    researcher_supplied,
                });
            }
        } else if kind == "edl" {
            // Strikes (R2): researcher-authored single-Cut EDLs. The pane
            // mirrors the server's strike identity exactly (model "user"
            // AND the researcher_annotation template AND exactly one Cut
            // op), so multi-op compose EDLs and model EDLs stay in the
            // Layers strip but never render as strikes.
            let provenance = layer_object.get("provenance");
            let researcher_supplied = provenance
                .and_then(|provenance| provenance.get("model"))
                .and_then(Value::as_str)
                == Some("user")
                && provenance
                    .and_then(|provenance| provenance.get("prompt_template"))
                    .and_then(Value::as_str)
                    == Some("researcher_annotation");
            if researcher_supplied {
                let ops = layer_object
                    .get("ops")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "edl layer has no ops array.".to_string())?;
                if ops.len() == 1 {
                    let entry = ops
                        .first()
                        .and_then(Value::as_object)
                        .ok_or_else(|| "edl op is not an object.".to_string())?;
                    let op = entry
                        .get("op")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "edl op has no op.".to_string())?;
                    if op == "cut" {
                        let range = entry
                            .get("range")
                            .and_then(Value::as_object)
                            .ok_or_else(|| "edl cut op has no range.".to_string())?;
                        let start_word = range
                            .get("start_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "edl cut range has no start_word.".to_string())?;
                        let end_word = range
                            .get("end_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "edl cut range has no end_word.".to_string())?;
                        strikes.push(PaneStrike {
                            start_word,
                            end_word,
                            annotation_id: id.clone(),
                        });
                    }
                }
            }
        }
        let created_at = object
            .get("created_at")
            .and_then(Value::as_str)
            .filter(|created| !created.is_empty())
            .ok_or_else(|| "layer record has no created_at.".to_string())?
            .to_string();
        layers.push(PaneLayerRow {
            id,
            kind,
            created_at,
        });
    }
    let paragraph_breaks = latest_layer_payload(layers_array, "paragraph")
        .map(|layer| {
            layer
                .get("breaks_after")
                .and_then(Value::as_array)
                .ok_or_else(|| "paragraph layer has no breaks_after.".to_string())?
                .iter()
                .map(|break_after| {
                    break_after
                        .as_u64()
                        .and_then(|index| usize::try_from(index).ok())
                        .ok_or_else(|| "paragraph break is not a word index.".to_string())
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    let speaker_spans = latest_layer_payload(layers_array, "speaker")
        .map(|layer| {
            let spans = layer
                .get("spans")
                .and_then(Value::as_array)
                .ok_or_else(|| "speaker layer has no spans.".to_string())?;
            spans
                .iter()
                .map(|span| {
                    let object = span
                        .as_object()
                        .ok_or_else(|| "speaker span is not an object.".to_string())?;
                    Ok(PaneSpeakerSpan {
                        start_word: object
                            .get("start_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "speaker span has no start_word.".to_string())?,
                        end_word: object
                            .get("end_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "speaker span has no end_word.".to_string())?,
                        speaker: object
                            .get("speaker")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "speaker span has no speaker.".to_string())?
                            .to_string(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    // Review flags (task 1a): optional and tolerant — the running editor
    // may predate the server field, so its absence parses as None; when
    // present, the view parses strictly.
    let review = object.get("review").map(parse_pane_review).transpose()?;
    Ok(TranscriptPaneState {
        transcript_id,
        media_path,
        alignment,
        alignment_note,
        words,
        working_text,
        paragraph_breaks,
        speaker_spans,
        review,
        layers,
        highlights,
        strikes,
    })
}

/// Strict-parse the derived review view (task 1a). Only invoked when the
/// payload carries a "review" object; a malformed one rejects the whole
/// payload — never a half view.
fn parse_pane_review(payload: &Value) -> Result<PaneReview, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "review is not an object.".to_string())?;
    let threshold = object
        .get("threshold")
        .and_then(Value::as_f64)
        .ok_or_else(|| "review has no threshold.".to_string())?;
    let blocks_array = object
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or_else(|| "review has no blocks array.".to_string())?;
    let mut blocks = Vec::with_capacity(blocks_array.len());
    for block in blocks_array {
        let object = block
            .as_object()
            .ok_or_else(|| "review block is not an object.".to_string())?;
        let start_word = object
            .get("start_word")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| "review block has no start_word.".to_string())?;
        let end_word = object
            .get("end_word")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| "review block has no end_word.".to_string())?;
        let min_confidence = match object.get("min_confidence") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_f64()
                    .ok_or_else(|| "review block min_confidence is not a number.".to_string())?,
            ),
        };
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| "review block has no status.".to_string())?;
        let status = PaneReviewStatus::parse(status)?;
        blocks.push(PaneReviewFlag {
            start_word,
            end_word,
            min_confidence,
            status,
        });
    }
    let note = object
        .get("note")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(PaneReview {
        threshold,
        blocks,
        note,
    })
}

fn parse_job_listing(output: &str) -> Result<JobListing, String> {
    parse_job_list_response(output).map_err(|error| format!("job_list failed: {error}"))
}

fn job_is_nonterminal(job: &JobRecord) -> bool {
    matches!(job.status.as_str(), "queued" | "running" | "cancelling")
}

/// Render a scalar JSON value as display text (None for objects/arrays).
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

impl Render for MediaViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let library_label: SharedString = format!(
            "Library ({})",
            self.gallery_total
                .map(|total| total.to_string())
                .unwrap_or_else(|| self.assets.len().to_string())
        )
        .into();
        let queue_label: SharedString = format!("Queue ({})", self.jobs.len()).into();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h_8()
                    .gap_1()
                    .px_2()
                    // Width-contained: tab labels truncate and the row
                    // clips rather than inflating the pane's min-content
                    // past the dock (the tab bar is the viewer's widest
                    // fixed chrome at narrow pane widths).
                    .min_w_0()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(self.tab_button(ViewerTab::Media, "Media".into(), cx))
                    .child(self.tab_button(ViewerTab::Library, library_label, cx))
                    .child(self.tab_button(ViewerTab::Queue, queue_label, cx))
                    .child(self.tab_button(ViewerTab::Detail, "Detail".into(), cx))
                    .child(self.tab_button(ViewerTab::Transcript, "Transcript".into(), cx))
                    // Force-refresh: rebuild cached widgets + reload the
                    // active tab. Right-aligned so it stays put as tab labels
                    // change width. flex_shrink_0 keeps the icon whole when
                    // the row is tight.
                    .child(
                        div().ml_auto().flex_shrink_0().child(
                            ui::IconButton::new("media-viewer-refresh", ui::IconName::RotateCw)
                                .icon_size(ui::IconSize::Small)
                                .tooltip(ui::Tooltip::text(
                                    "Refresh — rebuild widgets and reload this tab",
                                ))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.refresh(cx);
                                })),
                        ),
                    )
                    .when_some(self.presented_status(), |el, (status, color)| {
                        el.child(
                            Label::new(status)
                                .size(LabelSize::XSmall)
                                .color(color)
                                .truncate(),
                        )
                    }),
            )
            .child(match self.active_tab {
                ViewerTab::Media => self.render_media(_window, cx).into_any_element(),
                ViewerTab::Library => self.render_library(cx).into_any_element(),
                ViewerTab::Queue => self.render_queue(cx).into_any_element(),
                ViewerTab::Detail => self.render_detail(cx).into_any_element(),
                ViewerTab::Transcript => self.render_transcript(_window, cx).into_any_element(),
            })
    }
}

/// Parse a fenced ```media display hint into a `MediaAsset` — the block body
/// (JSON between the fences) plus the kind/src fields for labels.
fn asset_from_hint(hint: &str, tool: &SharedString) -> Option<MediaAsset> {
    let trimmed = hint.trim();
    let after_fence = trimmed.strip_prefix("```media")?;
    let body = after_fence.trim().strip_suffix("```")?.trim().to_string();
    let payload = hkask_media_widget::MediaBlockBody::parse(&body).ok()?;
    payload.to_media_ref().ok()?;
    let kind = payload.kind;
    let src = payload.src;
    let gallery_asset_id = payload.gallery_asset_id;
    Some(MediaAsset {
        body,
        src,
        kind,
        tool: tool.clone(),
        gallery_index: None,
        gallery_asset_id,
    })
}

/// Reconcile the asset list against one `gallery_list_assets` page.
/// Records merge by src: a conversation-surfaced asset for the same file
/// gains its gallery index and stable id; new gallery assets append. Then
/// removals: the page vouches for global indexes `[offset, offset + len)`
/// (the server's `index` is `offset + i` — the `image_index` other gallery
/// tools accept), so an indexed asset inside that window whose src is
/// absent from the page was deleted from the gallery — drop it. When the
/// payload's `total` is known, an indexed asset at or beyond `total` is
/// past the end of the active listing — the terminal page shrank and the
/// tail is exhausted — drop it too (an empty last page removes nothing on
/// its own, but `total` bounds it). Unindexed (conversation-surfaced)
/// assets and assets whose index falls outside the page are untouched:
/// the page says nothing about them.
fn merge_gallery_records(
    assets: &mut Vec<MediaAsset>,
    records: &[Value],
    offset: usize,
    total: Option<u64>,
) {
    let listed_srcs: HashSet<&str> = records
        .iter()
        .filter_map(|record| record.get("path").and_then(|path| path.as_str()))
        .collect();
    for record in records {
        let Some(src) = record.get("path").and_then(|p| p.as_str()) else {
            continue;
        };
        let kind = record
            .get("media_type")
            .and_then(|k| k.as_str())
            .unwrap_or("image")
            .to_string();
        let gallery_index = record
            .get("index")
            .and_then(|i| i.as_u64())
            .map(|i| i as usize);
        let gallery_asset_id = record
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_string);
        if let Some(existing) = assets.iter_mut().find(|existing| existing.src == src) {
            existing.gallery_index = gallery_index;
            if let Some(asset_id) = gallery_asset_id.as_deref()
                && let Ok(mut body) = serde_json::from_str::<Value>(&existing.body)
                && let Some(object) = body.as_object_mut()
            {
                object.insert(
                    "gallery_asset_id".to_string(),
                    Value::String(asset_id.to_string()),
                );
                existing.body = body.to_string();
            }
            existing.gallery_asset_id = gallery_asset_id;
        } else {
            let mut body = serde_json::json!({"kind": kind, "src": src});
            if let Some(asset_id) = gallery_asset_id.as_deref() {
                body["gallery_asset_id"] = Value::String(asset_id.to_string());
            }
            assets.push(MediaAsset {
                body: body.to_string(),
                src: src.to_string(),
                kind,
                tool: "gallery".into(),
                gallery_index,
                gallery_asset_id,
            });
        }
    }
    let window = offset..offset + records.len();
    let active_bound = total.map(|total| total as usize);
    assets.retain(|asset| match asset.gallery_index {
        Some(index) => {
            let in_window = window.contains(&index);
            let beyond_total = active_bound.is_some_and(|bound| index >= bound);
            !(in_window || beyond_total) || listed_srcs.contains(asset.src.as_str())
        }
        None => true,
    });
}

/// Read the producer-authored gallery mutation fact from a completed tool
/// result. Tool names are routing labels, not evidence that state changed.
fn tool_result_changed_gallery(output: &Value) -> bool {
    let payload = match output {
        Value::String(text) => hkask_types::tool_response::parse_tool_response(text),
        value => {
            let payload = hkask_types::tool_response::unwrap_tool_envelope(value.clone());
            if hkask_types::tool_response::parse_tool_error_value(&payload).is_some() {
                None
            } else {
                Some(payload)
            }
        }
    };
    payload
        .as_ref()
        .and_then(|payload| payload.get("gallery_changed"))
        .and_then(Value::as_bool)
        == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invoker-global tests serialize: mock invokers install into a
    /// process-global, and two installers racing under parallel test
    /// execution cross-answer each other's calls (observed live
    /// 2026-10-25: the pagination test recorded a job_list call). One
    /// installer at a time — the same pattern as the widget crate's
    /// audio-device GLOBAL_TEST_LOCK.
    static INVOKER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn hint(body: &str) -> String {
        format!("```media\n{body}\n```")
    }

    /// dcterms:identifier: `MediaViewer::activate_tab`
    /// expect: Leaving the Media tab suspends the shared player before the hidden tab renders.
    /// [P1] Motivating: Panel navigation cannot leave ghost audio or playback polling behind.
    #[gpui::test]
    fn leaving_media_tab_suspends_retained_player(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| {
            hkask_media_widget::MediaWidget::new(
                hkask_media_widget::MediaRef::new(
                    "/definitely/missing.png".into(),
                    hkask_media_widget::MediaKind::Image,
                ),
                cx,
            )
        });
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, _cx| {
            viewer.media_widget = Some(widget.clone());
            viewer.media_widget_body = Some("asset-body".to_string());
            viewer.active_tab = ViewerTab::Media;
        });

        viewer.update(cx, |viewer, cx| viewer.activate_tab(ViewerTab::Detail, cx));

        assert!(widget.read_with(cx, |widget, _cx| widget.is_suspended()));
        assert_eq!(
            viewer.read_with(cx, |viewer, _cx| viewer.active_tab),
            ViewerTab::Detail
        );
    }

    /// expect: [P7] MediaViewer::ingest_tool_result makes new artifacts visible
    /// without an unrelated render; replaying an existing artifact stays quiet.
    #[gpui::test]
    fn ingest_tool_result_notifies_observers_only_for_new_assets(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let _subscription = cx.update(|cx| {
            let notifications = notifications.clone();
            cx.observe(&viewer, move |_, _| {
                notifications.set(notifications.get() + 1);
            })
        });
        cx.run_until_parked();
        assert_eq!(notifications.get(), 0);

        for (index, source) in ["/tmp/structured.png", "/tmp/text.png"]
            .into_iter()
            .enumerate()
        {
            let output = hkask_mcp_media::media_block::enrich_with_omc_and_provenance(
                serde_json::json!({"output": source}),
                "image_create_collage",
                "image",
                serde_json::json!({}),
                None,
            );
            let output = if index == 0 {
                output
            } else {
                Value::String(output.to_string())
            };
            viewer.update(cx, |viewer, cx| {
                viewer.ingest_tool_result(&output, "image_create_collage", cx)
            });
            cx.run_until_parked();
            assert_eq!(
                notifications.get(),
                index + 1,
                "new artifact must notify GPUI observers"
            );

            viewer.update(cx, |viewer, cx| {
                viewer.ingest_tool_result(&output, "image_create_collage", cx)
            });
            cx.run_until_parked();
            assert_eq!(
                notifications.get(),
                index + 1,
                "duplicate must not request another redraw"
            );
        }
        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(&serde_json::json!({"status": "ok"}), "gallery_status", cx);
        });
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            2,
            "hint-free output does not change the view"
        );
    }

    /// expect: [P7] The public viewer ingress accepts both live transports,
    /// retains metadata, selects new assets, and does not duplicate replays.
    #[gpui::test]
    fn ingest_tool_result_accepts_structured_and_text_transports(cx: &mut gpui::TestAppContext) {
        let output = hkask_mcp_media::media_block::enrich_with_omc_and_provenance(
            serde_json::json!({"output": "/tmp/雪/quote\"-back\\slash\nimage.png"}),
            "image_create_collage",
            "image",
            serde_json::json!({"prompt": "snow"}),
            None,
        );
        let wrapped = serde_json::json!({"content": output});
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.detail = Some(serde_json::json!({"stale": true}));
            viewer.ingest_tool_result(&wrapped, "image_create_collage", cx);
            assert_eq!(viewer.assets.len(), 1);
            assert_eq!(viewer.selected, Some(0));
            assert!(viewer.detail.is_none());
            viewer.ingest_tool_result(
                &Value::String(wrapped.to_string()),
                "image_create_collage",
                cx,
            );
            viewer.ingest_tool_result(&output, "image_create_collage", cx);
            assert_eq!(viewer.assets.len(), 1);
            let second = hkask_mcp_media::media_block::media_block("video", "/tmp/second.mp4");
            viewer.ingest_tool_result(
                &serde_json::json!({"display_hints": [second]}),
                "video_clip",
                cx,
            );
            assert_eq!(viewer.selected, Some(1));
            assert_eq!(viewer.assets[1].src, "/tmp/second.mp4");
        });
    }

    struct JobListInvoker(String);

    impl hkask_tool_invoker::ToolInvoker for JobListInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: Value,
        ) -> gpui::Task<Result<String, hkask_tool_invoker::InvokeError>> {
            assert_eq!(server, MEDIA_SERVER);
            assert_eq!(tool, "job_list");
            assert_eq!(
                args,
                serde_json::json!({
                    "limit": hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT
                })
            );
            gpui::Task::ready(Ok(self.0.clone()))
        }
    }

    /// expect: [P7] Queue refresh distinguishes an empty queue from a broken
    /// response, and preserves the last good rows when refresh fails.
    #[gpui::test]
    fn load_jobs_surfaces_strict_rows_and_response_failures(cx: &mut gpui::TestAppContext) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        struct RestoreInvoker(Option<std::sync::Arc<dyn hkask_tool_invoker::ToolInvoker>>);
        impl Drop for RestoreInvoker {
            fn drop(&mut self) {
                hkask_tool_invoker::set_tool_invoker(self.0.take());
            }
        }
        let _restore = RestoreInvoker(hkask_tool_invoker::shared_tool_invoker());
        let record = hkask_mcp_media::types::JobRecord {
            id: "job-1".into(),
            op: "transcribe".into(),
            status: "running".into(),
            created_at: "2026-09-05T00:00:00Z".into(),
            completed_at: None,
            result: None,
            error: None,
        };
        let viewer = cx.new(|_| MediaViewer::new());
        let response = serde_json::json!({"content": {
            "jobs": [record],
            "total": 1,
            "limit": hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
            "has_more": false,
            "history_scope": hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE,
            "restart_behavior": hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR
        }})
        .to_string();
        hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(JobListInvoker(response))));
        viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(viewer.jobs.len(), 1);
            assert_eq!(viewer.jobs[0].id, "job-1");
            assert_eq!(viewer.jobs_request.state, ResourceState::Ready);
        });
        for response in [
            "not json",
            "{}",
            r#"{"content":[{"id":"broken"}]}"#,
            r#"{"content":{"jobs":[]}}"#,
            r#"{"content":[]}"#,
            r#"{"error":"job store unavailable","kind":"unavailable"}"#,
        ] {
            hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(JobListInvoker(
                response.into(),
            ))));
            viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
            cx.run_until_parked();
            viewer.update(cx, |viewer, _| {
                assert_eq!(viewer.jobs.len(), 1, "failed refresh preserves known jobs");
                let ResourceState::Failed(status) = &viewer.jobs_request.state else {
                    panic!("queue failure must be visible");
                };
                assert!(status.contains("job_list"));
                if response.contains("job store unavailable") {
                    assert!(status.contains("job store unavailable"));
                }
            });
        }
    }

    #[test]
    fn asset_from_hint_parses_kind_and_src() {
        let asset = asset_from_hint(
            &hint(r#"{"kind":"video","src":"/tmp/out.mp4"}"#),
            &"video_fetch".into(),
        )
        .expect("parses");
        assert_eq!(asset.kind, "video");
        assert_eq!(asset.src, "/tmp/out.mp4");
        assert_eq!(asset.tool, "video_fetch");
        assert!(asset.body.starts_with('{'));
        assert!(asset.gallery_index.is_none());
    }

    /// expect: [P7] Retiring a producer must not erase historical media provenance.
    #[test]
    fn historical_creative_hint_remains_readable() {
        let body = r#"{"kind":"image","src":"/tmp/historical.png","gallery_asset_id":"old-asset","ontology":"omc:CreativeWork","provenance":{"tool":"generate_image","args":{"prompt":"historical"}}}"#;
        let asset = asset_from_hint(&hint(body), &"generate_image".into())
            .expect("historical producer is not an execution request");
        assert_eq!(asset.gallery_asset_id.as_deref(), Some("old-asset"));
        assert_eq!(asset.body, body);
        assert_eq!(asset.tool, "generate_image");
    }

    /// expect: [P7] Invalid media never becomes a selectable phantom asset.
    #[test]
    fn asset_from_hint_rejects_missing_src_and_unsupported_kind() {
        for body in [
            r#"{"kind":"image"}"#,
            r#"{"kind":"hologram","src":"/tmp/x"}"#,
        ] {
            assert!(asset_from_hint(&hint(body), &"image_create_collage".into()).is_none());
        }
    }

    /// expect: [P7] Server-authored hints preserve paths and provenance in both consumers.
    #[test]
    fn server_hint_round_trips_through_viewer_and_widget() -> anyhow::Result<()> {
        let source = "/tmp/雪/quote\"-back\\slash\nimage.png";
        for kind in ["image", "svg", "video", "audio"] {
            let output = hkask_mcp_media::media_block::enrich_with_omc_and_provenance(
                serde_json::json!({"output": source, "gallery_asset_id": "asset-1"}),
                "image_create_collage",
                kind,
                serde_json::json!({"prompt": "雪\n\""}),
                Some("span-1".into()),
            );
            let hint = output["display_hint"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing hint"))?;
            let asset = asset_from_hint(hint, &"image_create_collage".into())
                .ok_or_else(|| anyhow::anyhow!("missing asset"))?;
            let body = hkask_media_widget::MediaBlockBody::parse(&asset.body)?;
            assert_eq!(asset.src, source);
            assert_eq!(body.to_media_ref()?.src(), source);
            assert_eq!(body.kind, kind);
            assert_eq!(body.ontology.as_deref(), Some("omc:CreativeWork"));
            assert_eq!(body.gallery_asset_id.as_deref(), Some("asset-1"));
            assert_eq!(asset.gallery_asset_id.as_deref(), Some("asset-1"));
            assert_eq!(
                body.provenance.tool.as_deref(),
                Some("image_create_collage")
            );
            assert_eq!(body.provenance.span_id.as_deref(), Some("span-1"));
            assert_eq!(
                body.provenance.args,
                serde_json::json!({"prompt": "雪\n\""})
            );
            // Additional metadata stays in the original body handed to the renderer.
            let mut extended: Value = serde_json::from_str(&asset.body)?;
            extended["width"] = serde_json::json!(640);
            extended["height"] = serde_json::json!(480);
            extended["caption"] = serde_json::json!("雪\n\"");
            let extended_hint = self::hint(&extended.to_string());
            let asset = asset_from_hint(&extended_hint, &"image_create_collage".into())
                .ok_or_else(|| anyhow::anyhow!("extended body rejected"))?;
            assert_eq!(serde_json::from_str::<Value>(&asset.body)?, extended);
        }
        Ok(())
    }

    #[test]
    fn asset_from_hint_rejects_non_media_fences_and_bad_json() {
        assert!(asset_from_hint("```rust\nfn main() {}\n```", &"t".into()).is_none());
        assert!(asset_from_hint("```media\nnot json\n```", &"t".into()).is_none());
        assert!(asset_from_hint("plain text", &"t".into()).is_none());
    }

    fn indexed_asset(src: &str, index: usize) -> MediaAsset {
        MediaAsset {
            body: format!(r#"{{"kind":"image","src":"{src}"}}"#),
            src: src.into(),
            kind: "image".into(),
            tool: "gallery".into(),
            gallery_index: Some(index),
            gallery_asset_id: Some(format!("asset-{index}")),
        }
    }

    fn surfaced_asset(src: &str) -> MediaAsset {
        surfaced_asset_of_kind(src, "image")
    }

    fn surfaced_asset_of_kind(src: &str, kind: &str) -> MediaAsset {
        MediaAsset {
            body: serde_json::json!({"kind": kind, "src": src}).to_string(),
            src: src.into(),
            kind: kind.into(),
            tool: "image_create_collage".into(),
            gallery_index: None,
            gallery_asset_id: None,
        }
    }

    fn job(status: &str) -> JobRecord {
        JobRecord {
            id: format!("job-{status}"),
            op: "video_clip".into(),
            status: status.into(),
            created_at: "2026-09-14T00:00:00Z".into(),
            completed_at: None,
            result: None,
            error: None,
        }
    }

    /// The confirmed-delete bug this pins: deleting in the Library reloads
    /// the listing, but the merge used to only add and update — the deleted
    /// asset's row never left the list, so a confirmed removal appeared to
    /// do nothing. The merge must reconcile: in-page absences are
    /// deletions; unindexed (conversation-surfaced) and out-of-page assets
    /// stay; survivors' indexes follow the positional shift; a
    /// conversation-surfaced asset for an indexed file gains its index
    /// (listing indices are the gallery image_index contract — server:
    /// `index = offset + i`).
    #[test]
    fn merge_gallery_records_merges_by_src_and_reconciles_deletions() {
        let mut assets = vec![
            surfaced_asset("/gallery/keep-a.png"),
            indexed_asset("/gallery/deleted.png", 1),
            indexed_asset("/gallery/keep-b.png", 2),
            surfaced_asset("/tmp/chat-only.png"),
            indexed_asset("/gallery/out-of-page.png", 150),
        ];
        // The fresh page (offset 0): deleted.png is gone; keep-b shifted
        // from index 2 to 1.
        let records = serde_json::json!([
            {"id": "asset-a", "index": 0, "path": "/gallery/keep-a.png", "media_type": "image"},
            {"id": "asset-b", "index": 1, "path": "/gallery/keep-b.png", "media_type": "image"},
        ]);
        merge_gallery_records(&mut assets, records.as_array().unwrap(), 0, None);

        let srcs: Vec<&str> = assets.iter().map(|a| a.src.as_str()).collect();
        assert_eq!(
            srcs,
            vec![
                "/gallery/keep-a.png",
                "/gallery/keep-b.png",
                "/tmp/chat-only.png",
                "/gallery/out-of-page.png",
            ],
            "the deleted in-page asset must drop; unindexed and out-of-page assets stay"
        );
        let keep_a = assets.first().expect("keep-a survives");
        assert_eq!(
            keep_a.gallery_index,
            Some(0),
            "a conversation-surfaced asset for an indexed file gains its index"
        );
        assert_eq!(keep_a.gallery_asset_id.as_deref(), Some("asset-a"));
        assert_eq!(
            hkask_media_widget::MediaBlockBody::parse(&keep_a.body)
                .expect("reconciled body stays renderable")
                .gallery_asset_id
                .as_deref(),
            Some("asset-a"),
            "the renderer body must carry the same stable Asset identity as the panel row"
        );
        assert_eq!(
            assets
                .iter()
                .find(|a| a.src == "/gallery/keep-b.png")
                .expect("keep-b survives")
                .gallery_index,
            Some(1),
            "a survivor's index follows the positional shift"
        );
    }

    /// A page fetched at a non-zero offset vouches only for its own window:
    /// an asset on an earlier page absent from THIS page must survive —
    /// only an in-window absence is a deletion.
    #[test]
    fn merge_gallery_records_reconciles_only_the_fetched_window() {
        let mut assets = vec![
            indexed_asset("/gallery/early.png", 5),
            indexed_asset("/gallery/in-window-deleted.png", 100),
            indexed_asset("/gallery/in-window-kept.png", 101),
        ];
        // Page at offset 100: one record — in-window-kept shifted down to
        // 100 because in-window-deleted was removed.
        let records = serde_json::json!([
            {"index": 100, "path": "/gallery/in-window-kept.png", "media_type": "image"},
        ]);
        merge_gallery_records(&mut assets, records.as_array().unwrap(), 100, None);

        let srcs: Vec<&str> = assets.iter().map(|a| a.src.as_str()).collect();
        assert_eq!(
            srcs,
            vec!["/gallery/early.png", "/gallery/in-window-kept.png"],
            "only in-window absences are deletions; earlier-page assets survive"
        );
    }

    /// expect: [P1] Empty and shortened terminal pages remove obsolete gallery
    /// rows without erasing conversation artifacts.
    #[gpui::test]
    fn gallery_terminal_pages_remove_exhausted_tail(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.assets = vec![
                indexed_asset("/gallery/keep.png", 0),
                indexed_asset("/gallery/tail.png", 1),
                surfaced_asset("/tmp/history.png"),
            ];
            viewer.merge_gallery_listing(Some(serde_json::json!({
                "gallery_id": "gallery-a", "total": 1, "offset": 0, "limit": 100,
                "assets": [{"id": "/gallery/keep.png", "index": 0, "path": "/gallery/keep.png", "media_type": "image", "missing": false, "metadata_stale": false}]
            })), cx);
            assert_eq!(viewer.assets.len(), 2, "exhausted tail must disappear");
            viewer.merge_gallery_listing(Some(serde_json::json!({
                "gallery_id": "gallery-a", "total": 0, "offset": 0, "limit": 100, "assets": []
            })), cx);
            assert_eq!(viewer.assets.len(), 1);
            assert_eq!(viewer.assets[0].src, "/tmp/history.png");
        });
    }

    /// expect: [P1] A different gallery invalidates old selection and pending
    /// deletion, even when the next root uses the same positional index.
    #[gpui::test]
    fn gallery_root_change_clears_selection_and_confirmation(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            for gallery in ["a", "b"] {
                viewer.merge_gallery_listing(Some(serde_json::json!({
                    "gallery_id": gallery, "total": 1, "offset": 0, "limit": 100,
                    "assets": [{"id": gallery, "index": 0, "path": format!("/{gallery}.png"), "media_type": "image", "missing": false, "metadata_stale": false}]
                })), cx);
                if gallery == "a" {
                    viewer.selected = Some(0);
                    viewer.confirm_delete.confirm(&0);
                    viewer.detail = Some(serde_json::json!({"old": true}));
                }
            }
            assert_eq!(viewer.assets.len(), 1, "old gallery rows are invalid");
            assert!(viewer.selected.is_none());
            assert!(viewer.confirm_delete.armed.is_none());
            assert!(viewer.detail.is_none());
        });
    }

    /// expect: [P1] A delayed response for a superseded listing request —
    /// e.g. gallery A's slow response arriving after the panel already
    /// holds gallery B — is dropped, never merged over the current view.
    #[gpui::test]
    fn delayed_listing_response_is_dropped(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            // Two listing requests were issued; the second owns the resource.
            let stale_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(0))
                .expect("first request starts");
            let current_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
                .expect("different page supersedes it");
            viewer.apply_gallery_listing(
                current_epoch,
                parse_gallery_listing(
                    Some(serde_json::json!({
                        "gallery_id": "gallery-b", "total": 101, "offset": 100, "limit": 100,
                        "assets": [{"id": "b100", "index": 100, "path": "/b.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                    })),
                    GALLERY_PAGE_LIMIT,
                ),
                cx,
            );
            assert_eq!(viewer.assets.len(), 1);
            assert_eq!(viewer.assets[0].src, "/b.png");
            assert_eq!(viewer.gallery_id.as_deref(), Some("gallery-b"));
            // The superseded request's delayed response (gallery A, epoch 1)
            // must not overwrite the view or retarget the gallery.
            viewer.apply_gallery_listing(
                stale_epoch,
                parse_gallery_listing(
                    Some(serde_json::json!({
                        "gallery_id": "gallery-a", "total": 1, "offset": 0, "limit": 100,
                        "assets": [{"id": "a0", "index": 0, "path": "/a.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                    })),
                    0,
                ),
                cx,
            );
            assert_eq!(viewer.assets.len(), 1, "delayed response must be dropped");
            assert_eq!(viewer.assets[0].src, "/b.png");
            assert_eq!(viewer.gallery_id.as_deref(), Some("gallery-b"));
        });
    }

    /// expect: [P1] After a root switch, no stale A-gallery row survives to
    /// act on B: the confirmed deletion is invalidated and no row carries
    /// A's stable asset id — a delete addressed by that id cannot hit B.
    #[gpui::test]
    fn stale_gallery_rows_cannot_act_after_root_change(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.merge_gallery_listing(
                Some(serde_json::json!({
                    "gallery_id": "gallery-a", "total": 1, "offset": 0, "limit": 100,
                    "assets": [{"id": "a0", "index": 0, "path": "/a.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                })),
                cx,
            );
            viewer.confirm_delete.confirm(&0); // pending deletion of A's row
            viewer.merge_gallery_listing(
                Some(serde_json::json!({
                    "gallery_id": "gallery-b", "total": 1, "offset": 0, "limit": 100,
                    "assets": [{"id": "b0", "index": 0, "path": "/b.png", "media_type": "image", "missing": false, "metadata_stale": false}]
                })),
                cx,
            );
            // The switch invalidated the pending action and every A row.
            assert!(viewer.confirm_delete.armed.is_none(),
                "a confirmation captured in A must not act on B");
            assert!(viewer.assets.iter().all(|asset| asset.src != "/a.png"),
                "A's rows are gone");
            // B's row carries B's stable id — a delete addressed at A's id
            // finds no target in the panel, and the server resolves ids
            // against the active gallery, so it cannot hit B either.
            assert!(viewer
                .assets
                .iter()
                .all(|asset| asset.gallery_asset_id.as_deref() != Some("a0")));
            assert_eq!(
                viewer.assets[0].gallery_asset_id.as_deref(),
                Some("b0"),
                "actions address B's rows by B's stable id"
            );
        });
    }

    /// dcterms:identifier: `tool_result_changed_gallery`
    /// expect: Every publisher family reports mutation from its completed result, while reads do not.
    /// [P7] Motivating: Library freshness follows observed state changes rather than tool-name guesses.
    #[test]
    fn gallery_freshness_is_result_derived_across_publisher_families() {
        for tool in [
            "image_create_collage",
            "video_clip",
            "audio_capture",
            "educt_render_edl",
            "video_fetch",
            "gallery_add_media",
            "gallery_delete_image",
        ] {
            let output = serde_json::json!({
                "content": {
                    "gallery_changed": true,
                    "gallery_asset_id": format!("asset-{tool}")
                }
            });
            assert!(
                tool_result_changed_gallery(&output),
                "publisher missed: {tool}"
            );
        }

        for output in [
            serde_json::json!({"content":{"gallery_changed":false}}),
            serde_json::json!({"content":{"jobs":[],"total":0}}),
            serde_json::json!({"content":{"display_hints":["```media\n{\"kind\":\"image\",\"src\":\"/existing.png\",\"gallery_asset_id\":\"existing\"}\n```"]}}),
            serde_json::json!({"error":{"code":"unavailable","message":"read failed"}}),
        ] {
            assert!(!tool_result_changed_gallery(&output));
        }
    }

    /// Selection and delete-confirmation repair across a reconciling
    /// merge: gallery indexes are positional, so after a deletion the old
    /// indices point at different assets. The selection must follow its
    /// asset (or clear with its detail when the asset left), and a pending
    /// confirmation must follow its asset — a stale confirm index would
    /// delete the WRONG asset on confirm.
    #[gpui::test]
    fn merge_gallery_listing_repairs_selection_and_confirm(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, _| {
            viewer.assets = vec![
                indexed_asset("/gallery/doomed.png", 0),
                indexed_asset("/gallery/survivor.png", 1),
            ];
            viewer.selected = Some(0); // the asset about to be deleted
            viewer.confirm_delete.confirm(&1); // the survivor, pre-shift
        });
        let payload = serde_json::json!({
            "gallery_id": "gallery-a",
            "total": 1,
            "offset": 0,
            "limit": GALLERY_PAGE_LIMIT,
            "assets": [
                {"id": "survivor", "index": 0, "path": "/gallery/survivor.png", "media_type": "image"},
            ]
        });
        viewer.update(cx, |viewer, cx| {
            viewer.merge_gallery_listing(Some(payload), cx)
        });
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.selected,
                Some(0),
                "the deleted selection falls back to the first asset"
            );
            assert!(
                viewer.detail.is_none(),
                "the deleted asset's detail is cleared"
            );
            assert_eq!(
                viewer.confirm_delete.armed,
                Some(0),
                "the pending confirmation follows its asset to the shifted index"
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::apply_gallery_listing`
    /// expect: A malformed latest page and a stale earlier failure cannot alter a last-good Library snapshot.
    /// [P1] Motivating: Library pagination is an atomic latest-owner transition.
    #[gpui::test]
    fn gallery_failure_is_atomic_and_stale_failure_is_ignored(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.assets = vec![indexed_asset("/good.png", 0)];
            viewer.gallery_total = Some(1);
            viewer.gallery_offset = 0;
            viewer.gallery_limit = GALLERY_PAGE_LIMIT;
            viewer.gallery_id = Some("gallery-good".into());
            viewer.selected = Some(0);
            let before = (
                viewer.assets.clone(),
                viewer.gallery_total,
                viewer.gallery_offset,
                viewer.gallery_limit,
                viewer.gallery_id.clone(),
                viewer.selected,
            );
            let malformed_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
                .expect("page request starts");
            let malformed = parse_gallery_listing(
                Some(serde_json::json!({
                    "gallery_id": "gallery-good", "total": 2, "offset": 100, "limit": 100,
                    "assets": [{"id": "broken", "index": 100, "media_type": "image"}]
                })),
                GALLERY_PAGE_LIMIT,
            );
            viewer.apply_gallery_listing(malformed_epoch, malformed, cx);
            assert_eq!(
                (
                    viewer.assets.clone(),
                    viewer.gallery_total,
                    viewer.gallery_offset,
                    viewer.gallery_limit,
                    viewer.gallery_id.clone(),
                    viewer.selected,
                ),
                before,
                "every last-good field survives malformed refresh"
            );
            assert!(matches!(
                viewer.gallery_request.state,
                ResourceState::Failed(_)
            ));

            let stale_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(0))
                .expect("refresh starts");
            let current_epoch = viewer
                .gallery_request
                .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
                .expect("next page supersedes refresh");
            viewer.apply_gallery_listing(
                current_epoch,
                parse_gallery_listing(
                    Some(serde_json::json!({
                        "gallery_id": "gallery-good", "total": 101, "offset": 100, "limit": 100,
                        "assets": [{"id": "latest", "index": 100, "path": "/latest.png", "media_type": "image"}]
                    })),
                    GALLERY_PAGE_LIMIT,
                ),
                cx,
            );
            viewer.apply_gallery_listing(stale_epoch, Err("stale failure".into()), cx);
            assert_eq!(viewer.gallery_request.state, ResourceState::Ready);
            assert!(viewer.assets.iter().any(|asset| asset.src == "/latest.png"));
        });
    }

    /// dcterms:identifier: `RequestLifecycle::begin`
    /// expect: Repeated clicks for the same loading resource remain single-flight.
    /// [P1] Motivating: refresh controls cannot fan out duplicate work.
    #[test]
    fn repeated_loading_owner_is_single_flight() {
        let mut request = RequestLifecycle::new();
        assert!(request.begin(RequestOwner::Jobs).is_some());
        assert!(request.begin(RequestOwner::Jobs).is_none());
        assert!(
            request
                .begin(RequestOwner::GalleryPage(GALLERY_PAGE_LIMIT))
                .is_some()
        );
    }

    /// dcterms:identifier: `MediaViewer::apply_jobs_listing`
    /// expect: Queue polling exists only for an active nonterminal queue and stops on leave, terminal data, or error.
    /// [P1] Motivating: background queue work is bounded by visible demand.
    #[gpui::test]
    fn queue_polling_stops_at_every_stop_condition(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.active_tab = ViewerTab::Queue;
            let epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            viewer.apply_jobs_listing(
                epoch,
                Ok(JobListing {
                    jobs: vec![job("running")],
                    total: 1,
                    limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
                    has_more: false,
                    history_scope: hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE.into(),
                    restart_behavior: hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR.into(),
                }),
                cx,
            );
            assert!(viewer.queue_poll_task.is_some());
            let stale_epoch = viewer.jobs_request.epoch;
            viewer.activate_tab(ViewerTab::Media, cx);
            assert!(
                viewer.queue_poll_task.is_none(),
                "leaving Queue cancels timer"
            );
            viewer.apply_jobs_listing(stale_epoch, Err("stale queue error".into()), cx);
            assert_eq!(viewer.jobs.len(), 1);
            assert_eq!(viewer.jobs_request.state, ResourceState::Idle);

            viewer.active_tab = ViewerTab::Queue;
            let terminal_epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            viewer.apply_jobs_listing(
                terminal_epoch,
                Ok(JobListing {
                    jobs: vec![job("completed")],
                    total: 1,
                    limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
                    has_more: false,
                    history_scope: hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE.into(),
                    restart_behavior: hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR.into(),
                }),
                cx,
            );
            assert!(
                viewer.queue_poll_task.is_none(),
                "terminal queue does not poll"
            );

            let running_epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            viewer.apply_jobs_listing(
                running_epoch,
                Ok(JobListing {
                    jobs: vec![job("running")],
                    total: 1,
                    limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
                    has_more: false,
                    history_scope: hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE.into(),
                    restart_behavior: hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR.into(),
                }),
                cx,
            );
            assert!(viewer.queue_poll_task.is_some());
            let error_epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            viewer.apply_jobs_listing(error_epoch, Err("queue unavailable".into()), cx);
            assert!(viewer.queue_poll_task.is_none(), "error cancels timer");
            assert_eq!(viewer.jobs.len(), 1, "error preserves last-good jobs");
        });
    }

    /// dcterms:identifier: `MediaViewer::apply_jobs_listing`
    /// expect: A completed background publisher becomes one visible asset exactly once.
    /// [P7] Motivating: job completion and foreground publication share one result ingress.
    #[gpui::test]
    fn completed_job_result_is_ingested_once(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.active_tab = ViewerTab::Queue;
            let result = serde_json::json!({
                "gallery_changed": true,
                "gallery_asset_id": "job-asset",
                "display_hint": "```media\n{\"kind\":\"image\",\"src\":\"/missing-job.png\",\"gallery_asset_id\":\"job-asset\"}\n```"
            });
            let mut completed = job("completed");
            completed.id = "job-1".into();
            completed.result = Some(result);
            for _ in 0..2 {
                let epoch = viewer
                    .jobs_request
                    .begin(RequestOwner::Jobs)
                    .expect("listing starts");
                viewer.apply_jobs_listing(
                    epoch,
                    Ok(JobListing {
                        jobs: vec![completed.clone()],
                        total: 1,
                        limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
                        has_more: false,
                        history_scope: hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE.into(),
                        restart_behavior: hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR.into(),
                    }),
                    cx,
                );
            }
            assert_eq!(
                viewer
                    .assets
                    .iter()
                    .filter(|asset| asset.gallery_asset_id.as_deref() == Some("job-asset"))
                    .count(),
                1
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::apply_detail_result`
    /// expect: A newer asset detail owns the view even if an older failure arrives later.
    /// [P1] Motivating: overlapping inspector work cannot regress visible state.
    #[gpui::test]
    fn overlapping_detail_requests_drop_stale_errors(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let old_detail = viewer
                .detail_request
                .begin(RequestOwner::Detail("asset-old".into()))
                .expect("old detail starts");
            let new_detail = viewer
                .detail_request
                .begin(RequestOwner::Detail("asset-new".into()))
                .expect("new detail supersedes");
            viewer.apply_detail_result(
                new_detail,
                Ok(serde_json::json!({"image": {"id": "asset-new"}})),
                cx,
            );
            viewer.apply_detail_result(old_detail, Err("stale detail error".into()), cx);
            assert_eq!(viewer.detail_request.state, ResourceState::Ready);
            assert_eq!(
                viewer
                    .detail
                    .as_ref()
                    .and_then(|d| d.pointer("/image/id"))
                    .and_then(Value::as_str),
                Some("asset-new")
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::begin_edit`
    /// expect: Trim and concat cannot overlap and hide an admitted side effect.
    /// [P1] Motivating: every direct edit remains visible until completion or failure.
    #[test]
    fn direct_edits_are_serialized_until_terminal() {
        let mut viewer = MediaViewer::new();
        let trim_epoch = viewer.begin_edit("video_clip").expect("trim admitted");
        let error = viewer
            .begin_edit("video_concat")
            .expect_err("concat must wait for trim");
        assert!(error.contains("already in progress"));
        assert!(viewer.edit_request.owns(trim_epoch));
        assert!(viewer.edit_request.complete(trim_epoch, Ok(())));
        assert!(viewer.begin_edit("video_concat").is_ok());
    }

    /// dcterms:identifier: `MediaViewer::confirm_label_edit`
    /// expect: Confirming a create closes the editor and surfaces the
    /// dispatch outcome visibly (the unwired test invoker names itself);
    /// an in-flight edit refuses a second confirm and keeps the typed
    /// buffer; a blank rename refuses without dispatching; Escape cancels
    /// without dispatching.
    /// [P1] Motivating: the label editor never loses typed text to a silent
    /// refusal, and every refusal surfaces, never swallows.
    #[gpui::test]
    fn label_editor_confirm_and_cancel_lifecycle(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        let blank_modifiers = || gpui::Modifiers {
            control: false,
            alt: false,
            shift: false,
            platform: false,
            function: false,
        };
        let key = |key: &str| Keystroke {
            modifiers: blank_modifiers(),
            key: key.into(),
            key_char: None,
        };
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-1".into(),
                media_path: "/m".into(),
                alignment: "original".into(),
                alignment_note: None,
                words: None,
                working_text: String::new(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
            });

            // Create confirm: the editor closes and the unwired invoker
            // refusal surfaces (the edit request completes into Failed).
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 3,
                    end_word: 9,
                },
                buffer: LabelBuffer::from_str("climax"),
            });
            viewer.confirm_label_edit(cx);
            assert!(
                viewer.highlight_edit.is_none(),
                "confirm consumed the editor"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("Tool invoker not wired"),
                "status: {:?}",
                viewer.status
            );
            assert!(matches!(
                viewer.edit_request.state,
                ResourceState::Failed(_)
            ));

            // An in-flight edit refuses a second confirm and keeps the
            // typed buffer for retry.
            viewer.edit_request.state = ResourceState::Loading;
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 3,
                    end_word: 9,
                },
                buffer: LabelBuffer::from_str("thesis"),
            });
            viewer.confirm_label_edit(cx);
            assert!(
                viewer.highlight_edit.is_some(),
                "a refused confirm keeps the typed buffer"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("already in progress")
            );
            assert_eq!(viewer.edit_request.state, ResourceState::Loading);
            viewer.edit_request.state = ResourceState::Idle;

            // A blank rename refuses without dispatching; the editor stays.
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Rename {
                    annotation_id: "layer-1".into(),
                    start_word: 3,
                    end_word: 9,
                    note: "kept note".into(),
                },
                buffer: LabelBuffer::from_str("   "),
            });
            viewer.confirm_label_edit(cx);
            assert!(
                viewer.highlight_edit.is_some(),
                "the blank rename stays open"
            );
            assert!(
                viewer.status.as_deref().unwrap().contains("nonblank label"),
                "status: {:?}",
                viewer.status
            );
            assert_eq!(viewer.edit_request.state, ResourceState::Idle);

            // A valid rename dispatches with the note preserved (the
            // unwired invoker names the rename path).
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Rename {
                    annotation_id: "layer-1".into(),
                    start_word: 3,
                    end_word: 9,
                    note: "kept note".into(),
                },
                buffer: LabelBuffer::from_str("climax"),
            });
            viewer.confirm_label_edit(cx);
            assert!(viewer.highlight_edit.is_none());
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("cannot rename highlights"),
                "status: {:?}",
                viewer.status
            );

            // Escape cancels without dispatching anything.
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 1,
                    end_word: 2,
                },
                buffer: LabelBuffer::default(),
            });
            viewer.handle_label_key(&key("escape"), cx);
            assert!(viewer.highlight_edit.is_none());
            assert!(viewer.status.as_deref().unwrap().contains("cancelled"));
            // Escape cancels without dispatching: the rename's completed
            // edit state is untouched (no new admission, no Loading).
            assert!(
                matches!(viewer.edit_request.state, ResourceState::Failed(_)),
                "cancel must not admit or complete an edit: {:?}",
                viewer.edit_request.state
            );
        });
    }

    /// A minimal workspace state for the interaction tests below.
    fn pane_state(transcript_id: &str) -> TranscriptPaneState {
        TranscriptPaneState {
            transcript_id: transcript_id.into(),
            media_path: "/m".into(),
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
        }
    }

    /// dcterms:identifier: `parse_transcript_payload`
    /// expect: The compose flow's gather layer (model "user", template
    /// workspace_reel_gather) never re-enters the highlight list — its
    /// entries are bookkeeping copies of researcher annotations, and the
    /// rename/delete affordances keyed to them would always be refused by
    /// the server's researcher-annotation guard (2026-10-26 review).
    #[test]
    fn gather_layers_do_not_reenter_the_highlight_list() {
        let payload = serde_json::json!({
            "summary": {"id": "t-1", "media_path": "/m", "words_count": 2, "has_word_timings": true},
            "working_transcript": {"text": "a b", "alignment": "original", "words": [
                {"word": "a", "start_ms": 0, "end_ms": 500},
                {"word": "b", "start_ms": 500, "end_ms": 900}
            ]},
            "layers": [
                {"id": "layer-researcher", "created_at": "c1", "layer": {
                    "kind": "highlight",
                    "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c1"},
                    "highlights": [{"start_word": 0, "end_word": 0, "label": "thesis", "note": "n"}]
                }},
                {"id": "layer-gather", "created_at": "c2", "layer": {
                    "kind": "highlight",
                    "provenance": {"model": "user", "prompt_template": "workspace_reel_gather", "created_at": "c2"},
                    "highlights": [{"start_word": 0, "end_word": 0, "label": "thesis", "note": "gathered"}]
                }},
                {"id": "layer-model", "created_at": "c3", "layer": {
                    "kind": "highlight",
                    "provenance": {"model": "pass-model", "prompt_template": "semantic", "created_at": "c3"},
                    "highlights": [{"start_word": 1, "end_word": 1, "label": "model-theme", "note": "n"}]
                }}
            ],
            "exports": [], "renders": []
        });
        let state = parse_transcript_payload(&payload).expect("parses");
        assert_eq!(state.highlights.len(), 2, "researcher + model rows only");
        assert!(state.highlights[0].researcher_supplied);
        assert_eq!(state.highlights[0].annotation_id, "layer-researcher");
        assert!(!state.highlights[1].researcher_supplied);
        assert_eq!(state.highlights[1].annotation_id, "layer-model");
        assert!(
            !state
                .highlights
                .iter()
                .any(|highlight| highlight.annotation_id == "layer-gather"),
            "the gather layer never re-enters the list"
        );
        // The gather layer still shows in the Layers strip — it is real
        // stored state, just not an annotation.
        assert_eq!(state.layers.len(), 3);
    }

    /// dcterms:identifier: `parse_transcript_payload`
    /// expect: Strike layers parse under the server's strike identity only
    /// (model "user", template researcher_annotation, exactly one Cut op) —
    /// model EDLs, multi-op researcher EDLs, and single-Keep researcher EDLs
    /// stay in the Layers strip but never render as strikes. [P1]
    #[test]
    fn strike_layers_parse_with_the_server_identity() {
        let payload = serde_json::json!({
            "summary": {"id": "t-1", "media_path": "/m", "words_count": 3, "has_word_timings": true},
            "working_transcript": {"text": "a b c", "alignment": "original", "words": [
                {"word": "a", "start_ms": 0, "end_ms": 300},
                {"word": "b", "start_ms": 400, "end_ms": 700},
                {"word": "c", "start_ms": 800, "end_ms": 900}
            ]},
            "layers": [
                {"id": "layer-strike", "created_at": "c1", "layer": {
                    "kind": "edl",
                    "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c1"},
                    "ops": [{"op": "cut", "range": {"start_word": 1, "end_word": 1}}]
                }},
                {"id": "layer-model-edl", "created_at": "c2", "layer": {
                    "kind": "edl",
                    "provenance": {"model": "pass-model", "prompt_template": "semantic", "created_at": "c2"},
                    "ops": [{"op": "cut", "range": {"start_word": 0, "end_word": 0}}]
                }},
                {"id": "layer-multi-op", "created_at": "c3", "layer": {
                    "kind": "edl",
                    "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c3"},
                    "ops": [
                        {"op": "cut", "range": {"start_word": 0, "end_word": 0}},
                        {"op": "cut", "range": {"start_word": 2, "end_word": 2}}
                    ]
                }},
                {"id": "layer-keep", "created_at": "c4", "layer": {
                    "kind": "edl",
                    "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "c4"},
                    "ops": [{"op": "keep", "range": {"start_word": 0, "end_word": 2}}]
                }}
            ],
            "exports": [], "renders": []
        });
        let state = parse_transcript_payload(&payload).expect("parses");
        assert_eq!(state.strikes.len(), 1, "only the strike identity renders");
        assert_eq!(state.strikes[0].start_word, 1);
        assert_eq!(state.strikes[0].end_word, 1);
        assert_eq!(state.strikes[0].annotation_id, "layer-strike");
        assert!(
            !state
                .strikes
                .iter()
                .any(|strike| strike.annotation_id == "layer-model-edl"
                    || strike.annotation_id == "layer-multi-op"
                    || strike.annotation_id == "layer-keep"),
            "non-strike EDLs never render as strikes"
        );
        assert_eq!(state.layers.len(), 4, "every EDL stays in the Layers strip");
        assert!(state.highlights.is_empty());
    }

    /// dcterms:identifier: `MediaViewer::strike_selected_range`
    /// expect: Striking from the open label editor closes the editor and
    /// dispatches the cut (kind strike) — the unwired invoker refusal
    /// surfaces visibly, progress clears, and the edit request completes
    /// into Failed, the same admission contract the highlight create path
    /// carries. [P1]
    #[gpui::test]
    fn strike_from_the_label_editor_closes_and_dispatches(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(pane_state("t-1"));
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 3,
                    end_word: 9,
                },
                buffer: LabelBuffer::from_str("typed but struck"),
            });
            viewer.strike_selected_range(3, 9, cx);
            assert!(
                viewer.highlight_edit.is_none(),
                "the strike consumed the editor"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("Tool invoker not wired"),
                "status: {:?}",
                viewer.status
            );
            assert!(
                viewer.edit_progress.is_none(),
                "the refusal cleared the edit progress: {:?}",
                viewer.edit_progress
            );
            assert!(matches!(
                viewer.edit_request.state,
                ResourceState::Failed(_)
            ));
        });
    }

    /// dcterms:identifier: `strike_create_params`
    /// expect: The strike create request carries the exact server contract
    /// — kind strike with blank label and note over the selected range.
    #[test]
    fn strike_create_params_carry_the_exact_contract() {
        let params = strike_create_params("t-1", 34, 51);
        let object = params.as_object().expect("object");
        assert_eq!(object.len(), 6);
        assert_eq!(params["transcript_id"], serde_json::json!("t-1"));
        assert_eq!(params["start_word"], serde_json::json!(34));
        assert_eq!(params["end_word"], serde_json::json!(51));
        assert_eq!(params["label"], serde_json::json!(""));
        assert_eq!(params["note"], serde_json::json!(""));
        assert_eq!(params["kind"], serde_json::json!("strike"));
    }

    /// dcterms:identifier: `MediaViewer::presented_status`
    /// expect: While an edit is in flight on the Transcript tab, the edit
    /// progress surfaces (S5's progress-visible contract); outside an
    /// edit, the specific status message falls through as before
    /// (2026-10-26 review: compose progress previously rendered nowhere).
    #[gpui::test]
    fn transcript_tab_surfaces_edit_progress(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.active_tab = ViewerTab::Transcript;
            viewer.edit_request.state = ResourceState::Loading;
            viewer.edit_progress = Some("composing reel from pane highlights…".into());
            let status = viewer.presented_status().map(|(text, _)| text.to_string());
            assert_eq!(
                status.as_deref(),
                Some("composing reel from pane highlights…"),
                "the compose progress is visible on the Transcript tab"
            );
            viewer.edit_progress = None;
            viewer.status = Some("Highlight created — the transcript reloads with it.".into());
            let status = viewer.presented_status().map(|(text, _)| text.to_string());
            assert_eq!(
                status.as_deref(),
                Some("Highlight created — the transcript reloads with it."),
                "outside an edit, the specific status message shows"
            );
            let _ = cx;
        });
    }

    /// dcterms:identifier: `MediaViewer::paste_into_label_edit`
    /// expect: Clipboard text lands in the open editor's buffer with
    /// control characters dropped; with no editor open the paste is inert
    /// (2026-10-26 review: labels could not be pasted at all).
    #[gpui::test]
    fn label_editor_pastes_clipboard_text(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 0,
                    end_word: 1,
                },
                buffer: LabelBuffer::from_str("climax"),
            });
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(" pasted\u{7}label ".into()));
            viewer.paste_into_label_edit(cx);
            assert_eq!(
                viewer
                    .highlight_edit
                    .as_ref()
                    .expect("editor open")
                    .buffer
                    .text(),
                "climax pastedlabel ",
                "control characters drop on paste"
            );
            viewer.highlight_edit = None;
            cx.write_to_clipboard(gpui::ClipboardItem::new_string("x".into()));
            viewer.paste_into_label_edit(cx);
            assert!(
                viewer.highlight_edit.is_none(),
                "paste is inert with no editor"
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::close_transcript_workspace`
    /// expect: Closing clears the workspace and pending interaction state
    /// and reloads the recordings listing — the browser stays reachable
    /// (2026-10-26 review: it was unreachable after the first open). The
    /// listing load surfaces its own unavailability when no invoker is
    /// wired — the degradation is visible, never silent. The invoker lock
    /// is required: this test asserts the unwired-invoker refusal, and a
    /// parallel installer test's mock in the process global would answer
    /// the listing dispatch instead (observed flaky 2026-10-26).
    #[gpui::test]
    fn close_workspace_returns_to_the_recordings_browser(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(pane_state("t-1"));
            viewer.selection_mode = true;
            viewer.selection_anchor = Some(2);
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 0,
                    end_word: 1,
                },
                buffer: LabelBuffer::default(),
            });
            viewer.active_tab = ViewerTab::Transcript;
            viewer.close_transcript_workspace(cx);
            assert!(viewer.transcript.is_none(), "the workspace closed");
            assert!(viewer.selection_anchor.is_none());
            assert!(viewer.highlight_edit.is_none());
            assert!(viewer.transcript_follow_task.is_none());
            assert!(
                matches!(
                    viewer.transcript_listing_request.state,
                    ResourceState::Failed(_)
                ),
                "the listing reload surfaced its unavailability: {:?}",
                viewer.transcript_listing_request.state
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::load_transcript_with`
    /// expect: A reload keeps the last-good workspace visible while the
    /// request is in flight (no idle-browser flash after every highlight
    /// edit — 2026-10-26 review); an explicit open still clears
    /// immediately (2026-10-26 review). The invoker lock is required: this
    /// test asserts the unwired-invoker refusal, and a parallel installer
    /// test's mock in the process global would answer the dispatch
    /// instead.
    #[gpui::test]
    fn reload_keeps_the_last_good_workspace(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(pane_state("t-old"));
            viewer.load_transcript_with("t-old".into(), cx);
            assert!(
                viewer.transcript.is_some(),
                "the last-good workspace stays visible during the reload"
            );
            assert!(
                matches!(viewer.transcript_request.state, ResourceState::Failed(_)),
                "the unwired invoker's refusal surfaced: {:?}",
                viewer.transcript_request.state
            );
            viewer.open_transcript("t-new".into(), cx);
            assert!(
                viewer.transcript.is_none(),
                "an explicit open clears the old workspace immediately"
            );
        });
    }

    /// dcterms:identifier: `parse_annotation_listing`
    /// expect: Annotation listings parse strictly or reject — never
    /// degrade to an empty success; the kind is the closed tag/note set.
    #[test]
    fn annotation_listing_is_strict() {
        let valid = serde_json::json!({
            "annotations": [
                {"id": "ann-1", "tag_type": "tag", "value": "thesis", "created_at": "c1", "confidence": 1.0, "model_used": "user"},
                {"id": "ann-2", "tag_type": "note", "value": "check audio at 0:30", "created_at": "c2", "confidence": 1.0, "model_used": "user"}
            ]
        });
        let rows = parse_annotation_listing(&valid).expect("valid listing parses");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "tag");
        assert_eq!(rows[0].value, "thesis");
        assert_eq!(rows[1].kind, "note");
        for invalid in [
            r#"{"annotations": [{}]}"#,
            r#"{"annotations": [{"id": ""}]}"#,
            r#"{"annotations": [{"id": "a", "tag_type": "vibe"}]}"#,
            r#"{"annotations": [{"id": "a", "tag_type": "tag"}]}"#,
            r#"{"annotations": [{"id": "a", "tag_type": "tag", "value": ""}]}"#,
            r#"{"annotations": [{"id": "a", "tag_type": "tag", "value": "v"}]}"#,
            r#"{"annotations": ["a"]}"#,
            r#"{}"#,
        ] {
            let payload: Value = serde_json::from_str(invalid).expect("fixture json");
            assert!(
                parse_annotation_listing(&payload).is_err(),
                "malformed listing must reject: {invalid}"
            );
        }
    }

    /// dcterms:identifier: `annotation_create_params` / `annotation_update_params` / `annotation_delete_params`
    /// expect: The three annotation dispatches carry their exact request
    /// contracts.
    #[test]
    fn annotation_params_carry_the_exact_contract() {
        let create = annotation_create_params("asset-1", "tag", "thesis");
        let create = create.as_object().expect("object");
        assert_eq!(create.len(), 3);
        assert_eq!(create["gallery_asset_id"], serde_json::json!("asset-1"));
        assert_eq!(create["kind"], serde_json::json!("tag"));
        assert_eq!(create["value"], serde_json::json!("thesis"));

        let update = annotation_update_params("asset-1", "ann-9", "revised");
        let update = update.as_object().expect("object");
        assert_eq!(update.len(), 3);
        assert_eq!(update["gallery_asset_id"], serde_json::json!("asset-1"));
        assert_eq!(update["annotation_id"], serde_json::json!("ann-9"));
        assert_eq!(update["value"], serde_json::json!("revised"));

        let delete = annotation_delete_params("asset-1", "ann-9");
        let delete = delete.as_object().expect("object");
        assert_eq!(delete.len(), 2);
        assert_eq!(delete["annotation_id"], serde_json::json!("ann-9"));
    }

    /// dcterms:identifier: `MediaViewer::confirm_annotation_edit`
    /// expect: Confirming a new tag closes the editor and surfaces the
    /// dispatch outcome visibly (the unwired test invoker names itself);
    /// a blank value refuses without dispatching; an unindexed asset
    /// refuses and keeps the buffer; the replace arm dispatches over the
    /// row's identity; Escape cancels without dispatching.
    /// [P1] Motivating: the annotation editor never loses typed text to a
    /// silent refusal, and every refusal surfaces, never swallows.
    #[gpui::test]
    fn annotation_editor_confirm_and_cancel_lifecycle(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.selected = Some(0);
            viewer.assets = vec![indexed_asset("/annotation-a.png", 1)];

            // Create confirm: the editor closes and the unwired invoker
            // refusal surfaces.
            viewer.annotation_edit = Some(AnnotationEdit {
                kind: "tag",
                annotation_id: None,
                buffer: LabelBuffer::from_str("thesis"),
            });
            viewer.confirm_annotation_edit(cx);
            assert!(
                viewer.annotation_edit.is_none(),
                "confirm consumed the editor"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("cannot edit annotations"),
                "status: {:?}",
                viewer.status
            );
            assert!(matches!(
                viewer.edit_request.state,
                ResourceState::Failed(_)
            ));

            // A blank value refuses without dispatching; the editor stays.
            viewer.edit_request.state = ResourceState::Idle;
            viewer.annotation_edit = Some(AnnotationEdit {
                kind: "note",
                annotation_id: None,
                buffer: LabelBuffer::from_str("   "),
            });
            viewer.confirm_annotation_edit(cx);
            assert!(
                viewer.annotation_edit.is_some(),
                "the blank value stays open"
            );
            assert!(
                viewer.status.as_deref().unwrap().contains("needs text"),
                "status: {:?}",
                viewer.status
            );
            assert_eq!(viewer.edit_request.state, ResourceState::Idle);

            // The replace arm dispatches over the row's identity (the
            // unwired invoker names the annotation path again).
            viewer.annotation_edit = Some(AnnotationEdit {
                kind: "note",
                annotation_id: Some("ann-9".into()),
                buffer: LabelBuffer::from_str("revised"),
            });
            viewer.confirm_annotation_edit(cx);
            assert!(viewer.annotation_edit.is_none());
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("cannot edit annotations"),
                "status: {:?}",
                viewer.status
            );

            // An unindexed asset refuses and keeps the buffer.
            viewer.edit_request.state = ResourceState::Idle;
            viewer.assets = vec![surfaced_asset("/annotation-b.png")];
            viewer.annotation_edit = Some(AnnotationEdit {
                kind: "tag",
                annotation_id: None,
                buffer: LabelBuffer::from_str("orphan"),
            });
            viewer.confirm_annotation_edit(cx);
            assert!(
                viewer.annotation_edit.is_some(),
                "the unindexed refusal keeps the buffer"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("not in the gallery index"),
                "status: {:?}",
                viewer.status
            );

            // Escape cancels without dispatching anything.
            viewer.handle_annotation_key(
                &Keystroke {
                    modifiers: gpui::Modifiers {
                        control: false,
                        alt: false,
                        shift: false,
                        platform: false,
                        function: false,
                    },
                    key: "escape".into(),
                    key_char: None,
                },
                cx,
            );
            assert!(viewer.annotation_edit.is_none());
            assert!(viewer.status.as_deref().unwrap().contains("cancelled"));
            assert_eq!(viewer.edit_request.state, ResourceState::Idle);
        });
    }

    /// dcterms:identifier: `MediaViewer::queue_selected_for_concat`
    /// expect: Non-video and over-cap concat additions fail visibly without changing the queue.
    /// [P1] Motivating: concat admission never silently drops or coerces selected work.
    #[test]
    fn concat_queue_rejects_type_and_cap_without_truncation() {
        let mut viewer = MediaViewer::new();
        viewer.assets.push(surfaced_asset("/image.png"));
        viewer.selected = Some(0);
        let error = viewer
            .queue_selected_for_concat()
            .expect_err("image must be rejected");
        assert!(error.contains("Only video"));
        assert!(viewer.concat_queue.is_empty());

        viewer
            .assets
            .push(surfaced_asset_of_kind("/overflow.mp4", "video"));
        viewer.selected = Some(1);
        viewer.concat_queue = (0..hkask_types::media_limits::MAX_CONCAT_ITEMS)
            .map(|index| format!("/{index}.mp4"))
            .collect();
        let before = viewer.concat_queue.clone();
        let error = viewer
            .queue_selected_for_concat()
            .expect_err("over-cap addition must be rejected");
        assert!(error.contains(&hkask_types::media_limits::MAX_CONCAT_ITEMS.to_string()));
        assert_eq!(viewer.concat_queue, before);
    }

    /// dcterms:identifier: `parse_delete_acknowledgement`
    /// expect: Deletion refreshes the Library only after an exact successful acknowledgement.
    /// [P1] Motivating: malformed deletion responses cannot falsely remove visible work.
    #[test]
    fn delete_acknowledgement_is_strict() {
        let valid = serde_json::json!({"content": {
            "deleted": true,
            "image_id": "asset-1",
            "file_deleted": false,
            "transcripts_detached": 2,
            "gallery_changed": true
        }})
        .to_string();
        assert_eq!(parse_delete_acknowledgement(&valid, "asset-1"), Ok(2));
        for invalid in [
            r#"{"content":{"deleted":false,"image_id":"asset-1","file_deleted":false,"transcripts_detached":0,"gallery_changed":true}}"#,
            r#"{"content":{"deleted":true,"image_id":"other","file_deleted":false,"transcripts_detached":0,"gallery_changed":true}}"#,
            r#"{"content":{"deleted":true,"image_id":"asset-1","file_deleted":false,"transcripts_detached":0}}"#,
        ] {
            assert!(parse_delete_acknowledgement(invalid, "asset-1").is_err());
        }
    }

    /// dcterms:identifier: `parse_job_listing`
    /// expect: The panel preserves every current queue field and rejects compatibility shapes.
    /// [P7] Motivating: the producer and consumer share one strict contract.
    #[test]
    fn job_listing_preserves_strict_contract_and_rejects_legacy_shapes() {
        let record = job("running");
        let listing = parse_job_listing(
            &serde_json::json!({
                "content": {
                    "jobs": [record],
                    "total": 3,
                    "limit": 1,
                    "has_more": true,
                    "history_scope": hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE,
                    "restart_behavior": hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR
                }
            })
            .to_string(),
        )
        .expect("current response parses");
        assert_eq!(listing.jobs.len(), 1);
        assert_eq!(listing.total, 3);
        assert_eq!(listing.limit, 1);
        assert!(listing.has_more);
        assert_eq!(
            listing.history_scope,
            hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE
        );
        for response in [
            r#"{"content":[]}"#,
            r#"{"content":{"jobs":[]}}"#,
            r#"{"content":{"jobs":[],"total":0,"limit":20,"has_more":false,"history_scope":"ephemeral_process_local"}}"#,
        ] {
            assert!(
                parse_job_listing(response).is_err(),
                "legacy/incomplete shape decoded"
            );
        }
    }

    struct GalleryPageInvoker {
        calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    }

    impl hkask_tool_invoker::ToolInvoker for GalleryPageInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: Value,
        ) -> gpui::Task<Result<String, hkask_tool_invoker::InvokeError>> {
            assert_eq!(server, MEDIA_SERVER);
            assert_eq!(tool, "gallery_list_assets");
            self.calls.lock().expect("calls lock").push(args.clone());
            let offset = args.get("offset").and_then(Value::as_u64).expect("offset");
            let limit = args.get("limit").and_then(Value::as_u64).expect("limit");
            gpui::Task::ready(Ok(serde_json::json!({
                "content": {
                    "gallery_id": "gallery-pages",
                    "total": 250,
                    "offset": offset,
                    "limit": limit,
                    "assets": [{
                        "id": format!("asset-{offset}"),
                        "index": offset,
                        "path": format!("/{offset}.mp4"),
                        "media_type": "video"
                    }]
                }
            })
            .to_string()))
        }
    }

    /// dcterms:identifier: `MediaViewer::next_gallery_page`
    /// expect: Previous and next actions dispatch reachable adjacent pages and commit their offsets.
    /// [P1] Motivating: every gallery page is reachable from Library controls.
    #[gpui::test]
    fn gallery_previous_and_next_reach_adjacent_pages(cx: &mut gpui::TestAppContext) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        struct RestoreInvoker(Option<std::sync::Arc<dyn hkask_tool_invoker::ToolInvoker>>);
        impl Drop for RestoreInvoker {
            fn drop(&mut self) {
                hkask_tool_invoker::set_tool_invoker(self.0.take());
            }
        }
        let _restore = RestoreInvoker(hkask_tool_invoker::shared_tool_invoker());
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(GalleryPageInvoker {
            calls: calls.clone(),
        })));
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, _| {
            viewer.gallery_total = Some(250);
            viewer.gallery_id = Some("gallery-pages".into());
            viewer.gallery_limit = GALLERY_PAGE_LIMIT;
        });
        viewer.update(cx, |viewer, cx| viewer.next_gallery_page(cx));
        cx.run_until_parked();
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.gallery_offset), 100);
        viewer.update(cx, |viewer, cx| viewer.previous_gallery_page(cx));
        cx.run_until_parked();
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.gallery_offset), 0);
        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["offset"], 100);
        assert_eq!(calls[1]["offset"], 0);
    }

    /// dcterms:identifier: `resource_status`
    /// expect: In-progress work uses neutral presentation, never error presentation.
    /// [P1] Motivating: users can distinguish progress from failure.
    #[test]
    fn loading_status_is_not_error_colored() {
        let (_, color) = resource_status("Queue", &ResourceState::Loading, false, None)
            .expect("loading is presented");
        assert_eq!(color, ui::Color::Muted);
    }

    /// dcterms:identifier: `parse_transcript_listing`
    /// expect: Transcript listings parse strictly or reject, never degrade to an empty success.
    /// [P1] Motivating: a malformed listing must not masquerade as "no transcripts".
    #[test]
    fn transcript_listing_is_strict() {
        let valid = serde_json::json!({
            "transcripts": [
                {"id": "caa6566e", "media_path": "/generated/vonnegut.mp4", "words_count": 558, "audio_duration_secs": 276.392, "has_word_timings": true, "alignment": "re_anchored"},
                {"id": "t-untimed", "media_path": "/generated/plain.mp3", "words_count": 12, "audio_duration_secs": 11, "has_word_timings": false, "alignment": "untimed"}
            ],
            "count": 2
        });
        assert_eq!(
            parse_transcript_listing(&valid),
            Ok(vec![
                TranscriptSummaryRow {
                    id: "caa6566e".into(),
                    media_path: "/generated/vonnegut.mp4".into(),
                    words_count: 558,
                    duration_secs: 276,
                    has_word_timings: true,
                    alignment: "re_anchored".into(),
                },
                TranscriptSummaryRow {
                    id: "t-untimed".into(),
                    media_path: "/generated/plain.mp3".into(),
                    words_count: 12,
                    duration_secs: 11,
                    has_word_timings: false,
                    alignment: "untimed".into(),
                }
            ])
        );
        for invalid in [
            r#"{"transcripts": [], "count": 1}"#,
            r#"{"count": 0}"#,
            r#"{"transcripts": []}"#,
            r#"{"transcripts": [{}], "count": 1}"#,
            r#"{"transcripts": [{"id": ""}], "count": 1}"#,
            r#"{"transcripts": [{"id": "t", "words_count": 3}], "count": 1}"#,
            r#"{"transcripts": [{"id": "t", "media_path": "", "words_count": 3}], "count": 1}"#,
            r#"{"transcripts": [{"id": "t", "has_word_timings": true}], "count": 1}"#,
            r#"{"transcripts": [{"id": "t", "media_path": "/m", "words_count": 3, "has_word_timings": true}], "count": 1}"#,
            r#"{"transcripts": [{"id": "t", "media_path": "/m", "words_count": 3, "has_word_timings": true, "alignment": "sideways"}], "count": 1}"#,
            r#"{"transcripts": ["t"], "count": 1}"#,
            r#"{}"#,
        ] {
            let payload: Value = serde_json::from_str(invalid).expect("fixture parses");
            assert!(
                parse_transcript_listing(&payload).is_err(),
                "expected rejection for {invalid}"
            );
        }
    }

    /// dcterms:identifier: `transcript_alignment_note`
    /// expect: The hand-annotatable states carry no note (the default row IS
    /// the editable bundle); the gated states name their gate, and untimed
    /// rows rely on the row's existing no-timings text. [P1]
    #[test]
    fn transcript_alignment_note_marks_gated_recordings_only() {
        assert_eq!(transcript_alignment_note("original"), None);
        assert_eq!(transcript_alignment_note("aligned"), None);
        assert_eq!(transcript_alignment_note("untimed"), None);
        assert_eq!(
            transcript_alignment_note("re_anchored"),
            Some("Estimated timings · review only")
        );
        assert_eq!(
            transcript_alignment_note("unaligned"),
            Some("Timed editing unavailable")
        );
    }

    #[test]
    fn human_recording_labels_avoid_internal_identifiers() {
        assert_eq!(
            recording_display_name("/generated/dunning-2017-audio.mp3"),
            "dunning 2017 audio"
        );
        assert_eq!(
            recording_display_name("/generated/2d014afb-07ca-418b-9d53-8cb1f7329fec.mp4"),
            "Untitled recording"
        );
        assert_eq!(format_recording_duration(11), "11s");
        assert_eq!(format_recording_duration(276), "4m 36s");
        assert_eq!(format_recording_duration(4963), "1h 22m");

        let mut state = pane_state("t");
        state.words = Some(vec![
            PaneWord {
                word: "first".into(),
                start_ms: 0,
                end_ms: 100,
            },
            PaneWord {
                word: "second".into(),
                start_ms: 100,
                end_ms: 200,
            },
            PaneWord {
                word: "third".into(),
                start_ms: 200,
                end_ms: 300,
            },
        ]);
        assert_eq!(transcript_range_excerpt(&state, 1, 2), "second third");
    }

    /// dcterms:identifier: `parse_reel_provenance`
    /// expect: A render record names its source transcript; absence renders nothing.
    /// [P1] Motivating: reel provenance shows when an asset was cut from a transcript.
    #[test]
    fn reel_provenance_names_source_and_absence_is_none() {
        let rendered = serde_json::json!({
            "transcript_render": {
                "gallery_asset_id": "reel-1",
                "source_transcript_id": "caa6566e",
                "created_at": "2026-10-25T18:45:57Z"
            }
        });
        assert_eq!(
            parse_reel_provenance(&rendered).as_deref(),
            Some("caa6566e (created 2026-10-25T18:45:57Z)")
        );
        assert_eq!(parse_reel_provenance(&serde_json::json!({})), None);
        assert_eq!(
            parse_reel_provenance(&serde_json::json!({"transcript_render": null})),
            None
        );
        assert_eq!(
            parse_reel_provenance(&serde_json::json!({
                "transcript_render": {"gallery_asset_id": "reel-1"}
            })),
            None
        );
    }

    /// dcterms:identifier: `MediaViewer::apply_detail_transcripts_result`
    /// expect: A newer transcript request owns the rows even if an older outcome arrives later.
    /// [P1] Motivating: overlapping transcript loads cannot regress visible detail state.
    #[gpui::test]
    fn overlapping_detail_transcripts_requests_drop_stale_errors(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let old_request = viewer
                .detail_transcripts_request
                .begin(RequestOwner::DetailTranscripts("asset-old".into()))
                .expect("old transcript request starts");
            let new_request = viewer
                .detail_transcripts_request
                .begin(RequestOwner::DetailTranscripts("asset-new".into()))
                .expect("new transcript request supersedes");
            viewer.apply_detail_transcripts_result(
                new_request,
                Ok(vec![TranscriptSummaryRow {
                    id: "t-new".into(),
                    media_path: "/generated/new.mp4".into(),
                    words_count: 4,
                    duration_secs: 2,
                    has_word_timings: true,
                    alignment: "original".into(),
                }]),
                cx,
            );
            viewer.apply_detail_transcripts_result(
                old_request,
                Err("stale listing error".into()),
                cx,
            );
            assert_eq!(
                viewer.detail_transcripts_request.state,
                ResourceState::Ready
            );
            assert_eq!(
                viewer.detail_transcripts.as_deref(),
                Some(
                    &[TranscriptSummaryRow {
                        id: "t-new".into(),
                        media_path: "/generated/new.mp4".into(),
                        words_count: 4,
                        duration_secs: 2,
                        has_word_timings: true,
                        alignment: "original".into(),
                    }][..]
                )
            );

            let failure = viewer
                .detail_transcripts_request
                .begin(RequestOwner::DetailTranscripts("asset-new".into()))
                .expect("failure request starts");
            viewer.apply_detail_transcripts_result(
                failure,
                Err("educt_list_transcripts failed: unreachable".into()),
                cx,
            );
            assert_eq!(
                viewer.detail_transcripts_request.state,
                ResourceState::Failed("educt_list_transcripts failed: unreachable".into())
            );
        });
    }

    /// dcterms:identifier: `parse_transcript_payload`
    /// expect: A get_transcript payload parses strictly into the pane state
    /// or rejects — never a half view.
    /// [P1] Motivating: the workspace renders the real working view, and a
    /// malformed payload must surface as a visible error.
    #[test]
    fn transcript_payload_parses_strictly() {
        let valid = serde_json::json!({
            "summary": {
                "id": "caa6566e",
                "media_path": "/generated/vonnegut.mp4",
                "words_count": 558,
                "has_word_timings": true
            },
            "working_transcript": {
                "text": "There's no reason why the simple shapes",
                "alignment": "original",
                "correction_layer_id": null,
                "realignment_layer_id": null,
                "alignment_error": null,
                "words": [
                    {"word": "There's", "start_ms": 21700, "end_ms": 21999, "confidence": null},
                    {"word": "no", "start_ms": 21999, "end_ms": 22119, "confidence": null},
                    {"word": "reason", "start_ms": 22119, "end_ms": 22420, "confidence": null}
                ]
            },
            "layers": [
                {
                    "id": "layer-paragraph",
                    "transcript_id": "caa6566e",
                    "created_at": "2026-10-25T18:45:03Z",
                    "layer": {
                        "kind": "paragraph",
                        "provenance": {"model": "test", "prompt_template": "t", "created_at": "2026-10-25T18:45:03Z"},
                        "breaks_after": [1]
                    }
                },
                {
                    "id": "layer-speaker",
                    "transcript_id": "caa6566e",
                    "created_at": "2026-10-25T18:45:04Z",
                    "layer": {
                        "kind": "speaker",
                        "provenance": {"model": "test", "prompt_template": "t", "created_at": "2026-10-25T18:45:04Z"},
                        "spans": [
                            {"start_word": 0, "end_word": 2, "speaker": "Vonnegut", "confidence": 0.9}
                        ]
                    }
                },
                {
                    "id": "layer-highlight",
                    "transcript_id": "caa6566e",
                    "created_at": "2026-10-25T18:45:05Z",
                    "layer": {
                        "kind": "highlight",
                        "provenance": {"model": "user", "prompt_template": "researcher_annotation", "created_at": "2026-10-25T18:45:05Z"},
                        "highlights": [
                            {"start_word": 1, "end_word": 2, "label": "thesis", "note": "n"}
                        ]
                    }
                }
            ],
            "exports": [],
            "renders": []
        });
        let state = parse_transcript_payload(&valid).expect("valid payload parses");
        // Tolerant absence (task 1a): a payload without a review view
        // parses — the pane renders no flags, never a fabricated green.
        assert_eq!(state.review, None);
        assert_eq!(state.transcript_id, "caa6566e");
        assert_eq!(state.media_path, "/generated/vonnegut.mp4");
        assert_eq!(state.alignment, "original");
        assert_eq!(
            state.words.as_deref(),
            Some(
                &[
                    PaneWord {
                        word: "There's".into(),
                        start_ms: 21700,
                        end_ms: 21999
                    },
                    PaneWord {
                        word: "no".into(),
                        start_ms: 21999,
                        end_ms: 22119
                    },
                    PaneWord {
                        word: "reason".into(),
                        start_ms: 22119,
                        end_ms: 22420
                    }
                ][..]
            )
        );
        assert_eq!(state.paragraph_breaks.as_deref(), Some(&[1usize][..]));
        assert_eq!(
            state.speaker_spans.as_deref(),
            Some(
                &[PaneSpeakerSpan {
                    start_word: 0,
                    end_word: 2,
                    speaker: "Vonnegut".into()
                }][..]
            )
        );
        assert_eq!(state.layers.len(), 3);
        assert_eq!(state.layers[0].kind, "paragraph");
        assert_eq!(
            state.highlights,
            vec![PaneHighlight {
                start_word: 1,
                end_word: 2,
                label: "thesis".into(),
                note: "n".into(),
                annotation_id: "layer-highlight".into(),
                researcher_supplied: true
            }]
        );

        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"summary": {}}),
            serde_json::json!({"summary": {"id": ""}}),
            serde_json::json!({
                "summary": {"id": "t", "media_path": ""}
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"}
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {}
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {"text": "hi", "alignment": "sideways"}
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {"text": "hi", "alignment": "original"}
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "original",
                    "words": [{"word": "hi"}]
                }
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "original",
                    "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
                }
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "untimed"
                }
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "original",
                    "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
                },
                "layers": [{}]
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "original",
                    "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
                },
                "layers": [{
                    "id": "l", "created_at": "2026-10-25T00:00:00Z",
                    "layer": {"kind": "paragraph", "breaks_after": ["two"]}
                }]
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "original",
                    "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
                },
                "layers": [{
                    "id": "l", "created_at": "2026-10-25T00:00:00Z",
                    "layer": {"kind": "speaker", "spans": [{"start_word": 0, "end_word": 0}]}
                }]
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "original",
                    "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
                },
                "layers": [{
                    "id": "l", "created_at": "2026-10-25T00:00:00Z",
                    "layer": {"kind": "highlight", "highlights": [{"start_word": 0, "end_word": 0}]}
                }]
            }),
            serde_json::json!({
                "summary": {"id": "t", "media_path": "/m"},
                "working_transcript": {
                    "text": "hi", "alignment": "original",
                    "words": [{"word": "hi", "start_ms": 0, "end_ms": 1}]
                },
                "layers": [{
                    "id": "l", "created_at": "2026-10-25T00:00:00Z",
                    "layer": {"kind": "highlight", "highlights": ["no"]}
                }]
            }),
        ] {
            assert!(
                parse_transcript_payload(&invalid).is_err(),
                "expected rejection for {invalid}"
            );
        }

        // Text-only views parse with words None — the pane renders the text
        // with a visible degradation note.
        let untimed = serde_json::json!({
            "summary": {"id": "t2", "media_path": "/m2"},
            "working_transcript": {
                "text": "plain text only",
                "alignment": "untimed",
                "alignment_error": "source transcript has no word-level timings"
            },
            "layers": []
        });
        let state = parse_transcript_payload(&untimed).expect("untimed payload parses");
        assert_eq!(state.review, None);
        assert_eq!(state.words, None);
        assert_eq!(state.working_text, "plain text only");
        assert_eq!(
            state.alignment_note.as_deref(),
            Some("source transcript has no word-level timings")
        );
    }

    /// dcterms:identifier: `parse_transcript_payload`
    /// expect: A derived review view parses strictly when present —
    /// flagged blocks carry the stored minimum confidence and gaps surface
    /// no-confidence-recorded — and a malformed one rejects the payload.
    #[test]
    fn transcript_payload_parses_review_flags() {
        let payload = serde_json::json!({
            "summary": {
                "id": "t-review",
                "media_path": "/generated/review.wav",
                "words_count": 4,
                "has_word_timings": true
            },
            "working_transcript": {
                "text": "a b c d",
                "alignment": "original",
                "correction_layer_id": null,
                "realignment_layer_id": null,
                "alignment_error": null,
                "words": [
                    {"word": "a", "start_ms": 0, "end_ms": 500, "confidence": null},
                    {"word": "b", "start_ms": 500, "end_ms": 1000, "confidence": null},
                    {"word": "c", "start_ms": 1000, "end_ms": 1500, "confidence": null},
                    {"word": "d", "start_ms": 1500, "end_ms": 2000, "confidence": null}
                ]
            },
            "review": {
                "threshold": 0.75,
                "paragraph_layer_id": "p-1",
                "speaker_layer_id": "s-1",
                "blocks": [
                    {"start_word": 0, "end_word": 1, "min_confidence": 0.9, "status": "ok"},
                    {"start_word": 2, "end_word": 3, "min_confidence": 0.5, "status": "flagged"},
                    {"start_word": 4, "end_word": 5, "min_confidence": null, "status": "no-confidence-recorded"}
                ],
                "note": null
            },
            "layers": [],
            "exports": [],
            "renders": []
        });
        let state = parse_transcript_payload(&payload).expect("payload with review parses");
        let review = state.review.expect("review view present");
        assert_eq!(review.threshold, 0.75);
        assert_eq!(review.note, None);
        assert_eq!(
            review.blocks,
            vec![
                PaneReviewFlag {
                    start_word: 0,
                    end_word: 1,
                    min_confidence: Some(0.9),
                    status: PaneReviewStatus::Ok
                },
                PaneReviewFlag {
                    start_word: 2,
                    end_word: 3,
                    min_confidence: Some(0.5),
                    status: PaneReviewStatus::Flagged
                },
                PaneReviewFlag {
                    start_word: 4,
                    end_word: 5,
                    min_confidence: None,
                    status: PaneReviewStatus::NoConfidenceRecorded
                },
            ]
        );

        // Malformed review views reject the whole payload — never a half
        // view: an unknown status, a non-numeric confidence, a missing
        // threshold.
        let mut unknown_status = payload.clone();
        unknown_status["review"]["blocks"][0]["status"] = serde_json::json!("green");
        assert!(parse_transcript_payload(&unknown_status).is_err());

        let mut bad_confidence = payload.clone();
        bad_confidence["review"]["blocks"][0]["min_confidence"] = serde_json::json!("high");
        assert!(parse_transcript_payload(&bad_confidence).is_err());

        let mut no_threshold = payload;
        no_threshold["review"]
            .as_object_mut()
            .expect("review object")
            .remove("threshold");
        assert!(parse_transcript_payload(&no_threshold).is_err());
    }

    /// dcterms:identifier: `latest_layer_payload`
    /// expect: The newest layer of a kind wins, deterministically.
    #[test]
    fn latest_paragraph_layer_wins_deterministically() {
        let older = serde_json::json!({
            "id": "a-paragraph",
            "created_at": "2026-10-25T00:00:00Z",
            "layer": {"kind": "paragraph", "breaks_after": [9]}
        });
        let newer_same_timestamp = serde_json::json!({
            "id": "b-paragraph",
            "created_at": "2026-10-25T00:00:00Z",
            "layer": {"kind": "paragraph", "breaks_after": [1]}
        });
        let layers = [older, newer_same_timestamp];
        let latest = latest_layer_payload(&layers, "paragraph").expect("found");
        assert_eq!(latest.get("breaks_after"), Some(&serde_json::json!([1])));
    }

    /// dcterms:identifier: `MediaViewer::handle_word_selection`
    /// expect: Seek outside select mode; anchor → clear on repeat; anchor →
    /// ordered range on the second click — never an inverted range.
    /// [P1] Motivating: creation must be deliberate and range-ordered.
    #[test]
    fn word_selection_state_machine() {
        let mut viewer = MediaViewer::new();
        viewer.selection_mode = false;
        assert_eq!(viewer.handle_word_selection(5), WordClickOutcome::SeekOnly);

        viewer.selection_mode = true;
        viewer.selection_anchor = None;
        assert_eq!(
            viewer.handle_word_selection(5),
            WordClickOutcome::AnchorSet(5)
        );

        viewer.selection_anchor = Some(5);
        assert_eq!(
            viewer.handle_word_selection(5),
            WordClickOutcome::AnchorCleared
        );

        viewer.selection_anchor = Some(9);
        assert_eq!(
            viewer.handle_word_selection(3),
            WordClickOutcome::OpenLabelEditor {
                start_word: 3,
                end_word: 9
            }
        );

        viewer.selection_anchor = Some(3);
        assert_eq!(
            viewer.handle_word_selection(9),
            WordClickOutcome::OpenLabelEditor {
                start_word: 3,
                end_word: 9
            }
        );

        // The label editor is modal (S4-3): while open, clicks neither seek
        // nor build a new selection.
        viewer.selection_mode = false;
        viewer.selection_anchor = Some(2);
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 3,
                end_word: 9,
            },
            buffer: LabelBuffer::default(),
        });
        assert_eq!(
            viewer.handle_word_selection(5),
            WordClickOutcome::EditorOpen
        );
    }

    /// dcterms:identifier: `highlight_create_params`
    /// expect: The create dispatch carries the exact request contract with
    /// the label editor's typed label.
    #[test]
    fn highlight_create_params_carry_the_exact_contract() {
        let params = highlight_create_params("t-1", 34, 51, "climax");
        let object = params.as_object().expect("object");
        assert_eq!(object.len(), 5);
        assert_eq!(object["transcript_id"], serde_json::json!("t-1"));
        assert_eq!(object["start_word"], serde_json::json!(34));
        assert_eq!(object["end_word"], serde_json::json!(51));
        assert_eq!(object["label"], serde_json::json!("climax"));
        assert!(object["note"].as_str().is_some());
    }

    /// dcterms:identifier: `resolve_create_label`
    /// expect: Confirming an empty create editor falls back to the default
    /// label; typed text trims through verbatim.
    #[test]
    fn create_label_resolves_empty_to_the_default() {
        assert_eq!(resolve_create_label(""), "Highlight");
        assert_eq!(resolve_create_label("   "), "Highlight");
        assert_eq!(resolve_create_label("  thesis  "), "thesis");
    }

    /// dcterms:identifier: `highlight_update_params`
    /// expect: The rename dispatch carries the exact request contract —
    /// identity plus the replacement label over the unchanged range/note.
    #[test]
    fn highlight_update_params_carry_the_exact_contract() {
        let params = highlight_update_params("t-1", "ann-9", 34, 51, "climax", "kept note");
        let object = params.as_object().expect("object");
        assert_eq!(object.len(), 6);
        assert_eq!(object["transcript_id"], serde_json::json!("t-1"));
        assert_eq!(object["annotation_id"], serde_json::json!("ann-9"));
        assert_eq!(object["start_word"], serde_json::json!(34));
        assert_eq!(object["end_word"], serde_json::json!(51));
        assert_eq!(object["label"], serde_json::json!("climax"));
        assert_eq!(object["note"], serde_json::json!("kept note"));
    }

    /// dcterms:identifier: `highlight_range_gate`
    /// expect: Every alignment state answers the gate — the two
    /// index-identical views pass, every other state blocks with a
    /// visible reason (S4 AC3).
    #[test]
    fn highlight_range_gate_covers_every_alignment() {
        assert!(highlight_range_gate("original").is_ok());
        assert!(highlight_range_gate("aligned").is_ok());
        for alignment in ["re_anchored", "unaligned", "untimed", "unexpected"] {
            let reason = highlight_range_gate(alignment).expect_err("blocked");
            assert!(
                reason.starts_with("Direct selection"),
                "{alignment}: {reason}"
            );
            assert!(!reason.contains("index-identical"), "{alignment}: {reason}");
        }
    }

    /// dcterms:identifier: `LabelBuffer`
    /// expect: The label input's model edits multi-byte text as characters,
    /// clamps cursor moves at both edges, and routes keystrokes (typed
    /// characters insert; modifier chords never do).
    #[test]
    fn label_buffer_edits_and_moves_the_cursor() {
        let mut buffer = LabelBuffer::from_str("hi");
        assert_eq!(buffer.cursor(), 2);
        buffer.insert(' ');
        buffer.insert('é');
        assert_eq!(buffer.text(), "hi é");
        assert_eq!(buffer.cursor(), 4);
        buffer.backspace();
        assert_eq!(buffer.text(), "hi ");
        buffer.move_home();
        buffer.insert('X');
        assert_eq!(buffer.text(), "Xhi ");
        assert_eq!(buffer.cursor(), 1);
        buffer.delete_forward();
        assert_eq!(buffer.text(), "Xi ");
        buffer.move_left();
        buffer.backspace();
        buffer.backspace();
        assert_eq!(buffer.text(), "Xi ", "backspace clamps at the start");
        buffer.move_end();
        buffer.move_right();
        assert_eq!(buffer.cursor(), 3, "move_right clamps at the end");
        assert_eq!(buffer.label(), "Xi");

        let mut typed = LabelBuffer::default();
        assert!(typed.apply_keystroke(&Keystroke {
            modifiers: gpui::Modifiers {
                control: false,
                alt: false,
                shift: false,
                platform: false,
                function: false,
            },
            key: "space".into(),
            key_char: Some(" ".into()),
        }));
        assert_eq!(typed.text(), " ");
        assert!(
            !typed.apply_keystroke(&Keystroke {
                modifiers: gpui::Modifiers {
                    control: true,
                    alt: false,
                    shift: false,
                    platform: false,
                    function: false,
                },
                key: "c".into(),
                key_char: None,
            }),
            "modifier chords are shortcuts, not typing"
        );
        assert_eq!(typed.text(), " ");
    }

    /// dcterms:identifier: `compose_reel_params`
    /// expect: The three compose modes build the reel EDL directly —
    /// assemble keeps only the toggled-in researcher highlights
    /// (overlapping and adjacent selections union-merge), reduce cuts every
    /// strike, mixed carries both; empty op sets yield None, model-pass rows
    /// never compose, and the pane_reel_compose provenance never matches
    /// the pane strike identity. [P1]
    #[test]
    fn compose_reel_params_builds_each_mode() {
        let state = TranscriptPaneState {
            transcript_id: "t-1".into(),
            media_path: "/m".into(),
            alignment: "original".into(),
            alignment_note: None,
            words: None,
            working_text: String::new(),
            paragraph_breaks: None,
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: vec![
                PaneHighlight {
                    start_word: 0,
                    end_word: 3,
                    label: "a".into(),
                    note: "n".into(),
                    annotation_id: "layer-0".into(),
                    researcher_supplied: true,
                },
                PaneHighlight {
                    start_word: 2,
                    end_word: 5,
                    label: "b".into(),
                    note: "n".into(),
                    annotation_id: "layer-2".into(),
                    researcher_supplied: true,
                },
                PaneHighlight {
                    start_word: 6,
                    end_word: 7,
                    label: "c".into(),
                    note: "n".into(),
                    annotation_id: "layer-6".into(),
                    researcher_supplied: true,
                },
                PaneHighlight {
                    start_word: 100,
                    end_word: 110,
                    label: "model-theme".into(),
                    note: "n".into(),
                    annotation_id: "layer-100".into(),
                    researcher_supplied: false,
                },
            ],
            strikes: vec![PaneStrike {
                start_word: 4,
                end_word: 4,
                annotation_id: "strike-4".into(),
            }],
        };
        let toggled: HashSet<(usize, usize)> = [(0, 3), (2, 5), (6, 7)].into_iter().collect();

        // Assemble: the three overlapping/adjacent keeps union-merge into
        // one disjoint Keep op [0,7]; the model row never composes.
        let assemble =
            compose_reel_params(&state, &toggled, ReelComposeMode::Assemble).expect("assemble");
        assert_eq!(assemble["transcript_id"], serde_json::json!("t-1"));
        assert_eq!(assemble["layer"]["kind"], serde_json::json!("edl"));
        assert_eq!(
            assemble["layer"]["provenance"]["prompt_template"],
            serde_json::json!("pane_reel_compose")
        );
        let ops = assemble["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 1, "overlapping and adjacent keeps union-merge");
        assert_eq!(ops[0]["op"], serde_json::json!("keep"));
        assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(0));
        assert_eq!(ops[0]["range"]["end_word"], serde_json::json!(7));

        // Reduce: one Cut op per strike, no keeps.
        let reduce =
            compose_reel_params(&state, &toggled, ReelComposeMode::Reduce).expect("reduce");
        let ops = reduce["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["op"], serde_json::json!("cut"));
        assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(4));

        // Mixed: the merged keep plus the cut.
        let mixed = compose_reel_params(&state, &toggled, ReelComposeMode::Mixed).expect("mixed");
        let ops = mixed["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0]["op"], serde_json::json!("keep"));
        assert_eq!(ops[1]["op"], serde_json::json!("cut"));

        // Empty op sets yield None — and Mixed with nothing toggled in
        // refuses rather than silently composing the reduce reel.
        let none_toggled: HashSet<(usize, usize)> = HashSet::new();
        assert!(compose_reel_params(&state, &none_toggled, ReelComposeMode::Assemble).is_none());
        assert!(compose_reel_params(&state, &none_toggled, ReelComposeMode::Mixed).is_none());
        let no_strikes = TranscriptPaneState {
            strikes: Vec::new(),
            ..state
        };
        assert!(compose_reel_params(&no_strikes, &toggled, ReelComposeMode::Reduce).is_none());

        // The pane_reel_compose provenance never matches the pane strike
        // identity: a single-cut reduce EDL re-parsed as a transcript layer
        // renders no strike row — the compose flow's own output cannot
        // masquerade as a researcher strike.
        let payload = serde_json::json!({
            "summary": {"id": "t-1", "media_path": "/m", "words_count": 8, "has_word_timings": true},
            "working_transcript": {"text": "a b c d e f g h", "alignment": "original", "words": [
                {"word": "a", "start_ms": 0, "end_ms": 100}
            ]},
            "layers": [
                {"id": "compose-1", "created_at": "c1", "layer": reduce["layer"].clone()}
            ],
            "exports": [], "renders": []
        });
        let parsed = parse_transcript_payload(&payload).expect("parses");
        assert!(
            parsed.strikes.is_empty(),
            "a composed single-cut EDL is not a researcher strike"
        );
        assert_eq!(parsed.layers.len(), 1, "it stays in the Layers strip");
    }

    /// dcterms:identifier: `resolved_highlight_runs`
    /// expect: Overlapping and out-of-order marks compose into the sorted,
    /// non-overlapping run list gpui consumes — a struck word inside a
    /// highlight keeps BOTH styles (its run carries the background and
    /// the line-through), so the follow-along mark no longer shifts
    /// styling past its range. [P1]
    #[test]
    fn resolved_highlight_runs_sorts_merges_and_keeps_both_styles() {
        let background = HighlightStyle {
            background_color: Some(gpui::black()),
            ..Default::default()
        };
        let strike = HighlightStyle {
            strikethrough: Some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            }),
            ..Default::default()
        };
        // Out of order, with the strike overlapping the highlight.
        let runs =
            resolved_highlight_runs(vec![(10..12, strike), (0..6, background), (4..8, strike)]);
        assert_eq!(
            runs.len(),
            4,
            "0..4 background, 4..6 both, 6..8 strike, 10..12 strike"
        );
        assert_eq!(runs[0].0, 0..4);
        assert!(runs[0].1.background_color.is_some());
        assert!(runs[0].1.strikethrough.is_none());
        assert_eq!(runs[1].0, 4..6);
        assert!(
            runs[1].1.background_color.is_some() && runs[1].1.strikethrough.is_some(),
            "the overlap keeps both styles"
        );
        assert_eq!(runs[2].0, 6..8);
        assert!(runs[2].1.strikethrough.is_some());
        assert!(runs[2].1.background_color.is_none());
        assert_eq!(runs[3].0, 10..12);
        // No marks, no runs — the plain text path.
        assert!(resolved_highlight_runs(Vec::new()).is_empty());
    }

    /// dcterms:identifier: `block_word_char_range`
    /// expect: The block-owned word-to-char mapping gates ranges that
    /// miss the block, clamps partial overlaps to the block bounds, and
    /// returns None when the clamped indices fall outside the word map
    /// (the R2 refactor's behavior-equivalence pin). [P1]
    #[test]
    fn block_word_char_range_gates_clamps_and_bounds() {
        let block = PaneBlock {
            speaker: None,
            text: "a b c".into(),
            word_char_ranges: vec![0..1, 2..3, 4..5],
            word_starts: vec![(0, 0), (1, 100), (2, 200)],
        };
        assert_eq!(block_word_char_range(&block, 0, 2), Some(0..5));
        assert_eq!(block_word_char_range(&block, 1, 1), Some(2..3));
        assert_eq!(
            block_word_char_range(&block, 5, 9),
            None,
            "a range missing the block maps nothing"
        );
        // A text-only block has no word map: nothing maps, even word 0.
        assert_eq!(block_word_char_range(&PaneBlock::default(), 0, 0), None);
    }

    /// dcterms:identifier: `MediaViewer::strike_selected_range`
    /// expect: An in-flight edit refuses the strike and keeps the editor
    /// open for retry — the confirm path's admission contract, pinned on
    /// the strike arm. [P1]
    #[gpui::test]
    fn strike_admission_refusal_keeps_the_editor(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(pane_state("t-1"));
            viewer.edit_request.state = ResourceState::Loading;
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 3,
                    end_word: 9,
                },
                buffer: LabelBuffer::from_str("typed but refused"),
            });
            viewer.strike_selected_range(3, 9, cx);
            assert!(
                viewer.highlight_edit.is_some(),
                "a refused strike keeps the editor for retry"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("already in progress"),
                "status: {:?}",
                viewer.status
            );
            assert_eq!(viewer.edit_request.state, ResourceState::Loading);
        });
    }

    /// dcterms:identifier: `MediaViewer::delete_workspace_annotation`
    /// expect: The kind-dispatched delete refusal names its subject — the
    /// strike arm says strikes, the highlight arm stays byte-identical to
    /// the pre-R2 message — both surfacing the unwired invoker and
    /// completing the edit request into Failed. [P1]
    #[gpui::test]
    fn annotation_delete_refusals_name_the_subject(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(pane_state("t-1"));
            viewer.delete_workspace_annotation("layer-strike".into(), "Strike", cx);
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("cannot delete strikes"),
                "status: {:?}",
                viewer.status
            );
            assert!(matches!(
                viewer.edit_request.state,
                ResourceState::Failed(_)
            ));
            viewer.delete_workspace_annotation("layer-highlight".into(), "Highlight", cx);
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("cannot delete highlights"),
                "the highlight message is unchanged: {:?}",
                viewer.status
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::compose_workspace_reel`
    /// expect: A compose dispatch runs under the edit admission — the
    /// unwired invoker completes the request into Failed with progress
    /// cleared — and a mode with nothing to compose surfaces its per-mode
    /// empty status without admitting an edit. [P1]
    #[gpui::test]
    fn compose_dispatch_refusal_and_empty_status_surface(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let mut state = pane_state("t-1");
            state.highlights = vec![PaneHighlight {
                start_word: 0,
                end_word: 2,
                label: "thesis".into(),
                note: String::new(),
                annotation_id: "layer-0".into(),
                researcher_supplied: true,
            }];
            state.strikes = vec![PaneStrike {
                start_word: 4,
                end_word: 5,
                annotation_id: "strike-4".into(),
            }];
            viewer.transcript = Some(state);
            viewer.reel_selection.insert((0, 2));
            viewer.compose_workspace_reel(ReelComposeMode::Assemble, cx);
            assert!(
                matches!(viewer.edit_request.state, ResourceState::Failed(_)),
                "the unwired compose refusal fails the edit request"
            );
            assert!(
                viewer.edit_progress.is_none(),
                "the refusal cleared the compose progress"
            );
            // Nothing toggled in: the assemble empty status surfaces
            // without admitting an edit.
            viewer.edit_request.state = ResourceState::Idle;
            viewer.reel_selection.clear();
            viewer.compose_workspace_reel(ReelComposeMode::Assemble, cx);
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("No included highlights"),
                "status: {:?}",
                viewer.status
            );
            assert_eq!(viewer.edit_request.state, ResourceState::Idle);
        });
    }

    /// dcterms:identifier: `transcript_media_body`
    /// expect: The workspace media body infers kind by extension, escapes
    /// paths, and parses as the media block the widget registry consumes.
    /// [P1] Motivating: the shared-player binding is only as good as the
    /// body it keys by.
    #[test]
    fn transcript_media_body_infers_kind_and_parses() {
        let video = transcript_media_body("/generated/vonnegut.mp4");
        assert_eq!(video, r#"{"kind":"video","src":"/generated/vonnegut.mp4"}"#);
        let audio = transcript_media_body("/generated/dunning-2017-audio.mp3");
        assert_eq!(
            audio,
            r#"{"kind":"audio","src":"/generated/dunning-2017-audio.mp3"}"#
        );
        // A path with JSON-hostile characters still parses and round-trips
        // the src through the block parser.
        let hostile = transcript_media_body("/generated/quote\"path.wav");
        let parsed =
            hkask_media_widget::MediaBlockBody::parse(&hostile).expect("escaped path still parses");
        assert_eq!(parsed.kind, "audio");
        assert_eq!(parsed.src, "/generated/quote\"path.wav");

        let parsed = hkask_media_widget::MediaBlockBody::parse(&video).expect("parses");
        assert_eq!(parsed.kind, "video");
        assert_eq!(parsed.src, "/generated/vonnegut.mp4");
    }

    /// dcterms:identifier: `current_word_index`
    /// expect: The marked word is the last whose start is at or before the
    /// position — never a word that starts after it.
    /// [P1] Motivating: follow-along must mark the word being heard.
    #[test]
    fn current_word_index_marks_the_containing_word() {
        let words = vec![
            PaneWord {
                word: "a".into(),
                start_ms: 1000,
                end_ms: 1500,
            },
            PaneWord {
                word: "b".into(),
                start_ms: 2000,
                end_ms: 2500,
            },
            PaneWord {
                word: "c".into(),
                start_ms: 3000,
                end_ms: 3500,
            },
        ];
        assert_eq!(current_word_index(&words, 0), None);
        assert_eq!(current_word_index(&words, 1000), Some(0));
        assert_eq!(current_word_index(&words, 2100), Some(1));
        assert_eq!(current_word_index(&words, 3000), Some(2));
        assert_eq!(current_word_index(&words, 9999), Some(2));
        assert_eq!(current_word_index(&[], 100), None);
    }

    /// dcterms:identifier: `block_index_for_word`
    /// expect: The mark-to-item mapping mirrors the block builder's
    /// boundaries exactly — every word maps to the block that contains it,
    /// so the follow-along scrolls the block the operator is hearing.
    /// [P1] Motivating: a mapping drift would scroll the wrong block.
    #[test]
    fn block_index_for_word_matches_block_boundaries() {
        let state = TranscriptPaneState {
            transcript_id: "t-parity".into(),
            media_path: "/generated/parity.wav".into(),
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..50)
                    .map(|index| PaneWord {
                        word: format!("parity-word-{index}"),
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
            working_text: String::new(),
            paragraph_breaks: Some(vec![9, 19, 29, 39]),
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
        };
        let blocks = build_pane_blocks(&state);
        assert_eq!(blocks.len(), 5, "four breaks split 50 words into 5 blocks");
        for (block_index, block) in blocks.iter().enumerate() {
            for (word, _) in &block.word_starts {
                assert_eq!(
                    block_index_for_word(&state, *word),
                    Some(block_index),
                    "word {word} must map to its own block"
                );
            }
        }
        assert_eq!(
            block_index_for_word(&state, 50),
            None,
            "a past-the-end word has no block"
        );
        let no_breaks = TranscriptPaneState {
            paragraph_breaks: None,
            ..state
        };
        assert_eq!(
            block_index_for_word(&no_breaks, 49),
            Some(0),
            "without breaks every word maps to the single block"
        );
    }

    /// dcterms:identifier: `ConfirmArm`
    /// expect: The shared two-click confirm: the first click arms, the same
    /// row's next click confirms and consumes the arm, a different row's
    /// click re-aims at it (a confirm never acts on the row beside the
    /// armed one), and disarm leaves no aim. One pattern serves every row
    /// family — Library indexes and transcript ids alike (R4).
    /// [P1] Motivating: two hand-rolled confirm state machines drifted
    /// apart in the disarm paths; one implementation cannot.
    #[test]
    fn confirm_arm_arms_confirms_and_disarms() {
        let mut arm = ConfirmArm::default();
        assert!(
            !arm.confirm(&"row-a"),
            "the first click arms, never confirms"
        );
        assert!(arm.is_armed(&"row-a"), "the clicked row is armed");
        assert!(
            !arm.confirm(&"row-b") && arm.is_armed(&"row-b"),
            "clicking another row re-aims at it"
        );
        assert!(arm.confirm(&"row-b"), "the armed row's next click confirms");
        assert!(arm.armed.is_none(), "confirm consumes the arm");
        arm.confirm(&"row-c");
        arm.disarm();
        assert!(!arm.is_armed(&"row-c"), "disarm leaves no aim");
    }

    /// dcterms:identifier: `build_pane_blocks`
    /// expect: Blocks honor breaks, align char ranges with the word text,
    /// and carry the global indices and start times the click map consumes.
    /// [P1] Motivating: click-to-seek and follow-along are only as good as
    /// the mapping tables.
    #[test]
    fn pane_blocks_align_char_ranges_and_word_starts() {
        let state = TranscriptPaneState {
            transcript_id: "t".into(),
            media_path: "/m".into(),
            alignment: "original".into(),
            alignment_note: None,
            words: Some(vec![
                PaneWord {
                    word: "hello".into(),
                    start_ms: 100,
                    end_ms: 200,
                },
                PaneWord {
                    word: "world".into(),
                    start_ms: 300,
                    end_ms: 400,
                },
                PaneWord {
                    word: "again".into(),
                    start_ms: 500,
                    end_ms: 600,
                },
            ]),
            working_text: "hello world again".into(),
            paragraph_breaks: Some(vec![1]),
            review: None,
            speaker_spans: Some(vec![PaneSpeakerSpan {
                start_word: 0,
                end_word: 2,
                speaker: "Vonnegut".into(),
            }]),
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
        };
        let blocks = build_pane_blocks(&state);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "hello world");
        assert_eq!(blocks[0].word_char_ranges, vec![0..5, 6..11]);
        assert_eq!(blocks[0].word_starts, vec![(0, 100), (1, 300)]);
        assert_eq!(blocks[1].text, "again");
        assert_eq!(blocks[1].word_char_ranges, vec![0..5]);
        assert_eq!(blocks[1].word_starts, vec![(2, 500)]);
        assert_eq!(blocks[0].speaker.as_deref(), Some("Vonnegut"));

        // Text-only views yield one unlabeled block with no word map.
        let mut text_only = state;
        text_only.words = None;
        let blocks = build_pane_blocks(&text_only);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "hello world again");
        assert!(blocks[0].word_char_ranges.is_empty());
        assert!(blocks[0].word_starts.is_empty());
    }

    /// dcterms:identifier: `MediaViewer::apply_transcript_result`
    /// expect: A newer transcript request owns the workspace even if an older outcome arrives later.
    /// [P1] Motivating: overlapping transcript loads cannot regress the visible workspace.
    #[gpui::test]
    fn overlapping_transcript_requests_drop_stale_errors(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let old_request = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-old".into()))
                .expect("old transcript request starts");
            let new_request = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-new".into()))
                .expect("new transcript request supersedes");
            let state = TranscriptPaneState {
                transcript_id: "t-new".into(),
                media_path: "/m".into(),
                alignment: "original".into(),
                alignment_note: None,
                words: Some(vec![PaneWord {
                    word: "hi".into(),
                    start_ms: 0,
                    end_ms: 1,
                }]),
                working_text: "hi".into(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
            };
            viewer.apply_transcript_result(new_request, Ok(state.clone()), cx);
            viewer.apply_transcript_result(old_request, Err("stale error".into()), cx);
            assert_eq!(viewer.transcript_request.state, ResourceState::Ready);
            assert_eq!(viewer.transcript.as_ref(), Some(&state));

            let failure = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-new".into()))
                .expect("failure request starts");
            viewer.apply_transcript_result(
                failure,
                Err("educt_get_transcript failed: unreachable".into()),
                cx,
            );
            assert_eq!(
                viewer.transcript_request.state,
                ResourceState::Failed("educt_get_transcript failed: unreachable".into())
            );
            // Retention semantics (mirrors apply_detail_result): the apply
            // path never clears on failure — the real load path clears at
            // dispatch start, so a failed reload leaves the pane empty while
            // a lifecycle-level failure retains the last-good view.
            assert_eq!(viewer.transcript.as_ref(), Some(&state));
        });
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    /// The edit-dispatch ingestion path: a `video_clip` result (the exact
    /// envelope shape the server emits — content-wrapped, display_hint
    /// inside) must surface as a new selected asset, the same way thread
    /// tool results do. Ground truth for the shape: the server's
    /// `enrich_with_omc_and_provenance` (hkask-mcp-media/src/media_block.rs).
    #[gpui::test]
    fn merge_tool_result_surfaces_clip_as_new_selected_asset(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let output = serde_json::json!({
                "content": {
                    "status": "clipped",
                    "source": "/tmp/source.mp4",
                    "start_sec": 10.0,
                    "end_sec": 40.0,
                    "duration": 30.0,
                    "output": "/tmp/clip.mp4",
                    "display_hint": "```media\n{\"kind\":\"video\",\"src\":\"/tmp/clip.mp4\"}\n```"
                }
            })
            .to_string();

            viewer.ingest_tool_result(&Value::String(output), "video_clip", cx);

            assert_eq!(viewer.assets.len(), 1, "the clip must surface as an asset");
            let selected = viewer
                .selected
                .expect("the new asset must be auto-selected");
            assert_eq!(viewer.assets[selected].src, "/tmp/clip.mp4");
            assert_eq!(viewer.assets[selected].kind, "video");
            assert_eq!(viewer.assets[selected].tool, "video_clip");
        });
    }

    /// Dedup: re-ingesting the same result must not duplicate the asset
    /// (the same discipline `ingest_thread` applies to thread results).
    #[gpui::test]
    fn merge_tool_result_deduplicates_by_body(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let output = serde_json::json!({
                "content": {
                    "status": "clipped",
                    "output": "/tmp/clip.mp4",
                    "display_hint": "```media\n{\"kind\":\"video\",\"src\":\"/tmp/clip.mp4\"}\n```"
                }
            })
            .to_string();

            let output = Value::String(output);
            viewer.ingest_tool_result(&output, "video_clip", cx);
            viewer.ingest_tool_result(&output, "video_clip", cx);

            assert_eq!(
                viewer.assets.len(),
                1,
                "identical results must not duplicate"
            );
        });
    }

    /// A result without a display_hint (or unparseable output) must not
    /// crash or add phantom assets — it is a no-op the status line already
    /// covers by clearing on success.
    #[gpui::test]
    fn merge_tool_result_ignores_output_without_display_hint(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(
                &serde_json::json!({"content": {"status": "clipped"}}),
                "video_clip",
                cx,
            );
            assert!(viewer.assets.is_empty());
            viewer.ingest_tool_result(&Value::String("not json at all".into()), "video_clip", cx);
            assert!(viewer.assets.is_empty());
        });
    }
}

#[cfg(test)]
mod viewer_layout_tests {
    use super::*;
    use gpui::{DefiniteLength, TestAppContext, px, size};

    const FIXTURE: &str =
        "/home/mdz-axolotl/Documents/zk-data/media-mcp/generated/vonnegut-shape-of-stories.mp4";

    /// Layout ground truth at the VIEWER level — the full production chain
    /// (tab bar → tab content → toolbar → player), not the widget in
    /// isolation. The widget-level test passed while the app was broken,
    /// so the break is in this chain. The property: the video area's
    /// laid-out height tracks window height exactly.
    #[gpui::test]
    fn viewer_video_area_scales_with_window_size(cx: &mut TestAppContext) {
        if !std::path::Path::new(FIXTURE).exists() {
            return;
        }
        init_layout_test_globals(cx);

        let viewer = cx.new(|_| MediaViewer::new());
        let output = serde_json::json!({
            "content": {
                "status": "fetched",
                "display_hint": format!(
                    "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
                )
            }
        })
        .to_string();
        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(&Value::String(output), "video_fetch", cx)
        });
        let asset_count = viewer.update(cx, |viewer, cx| {
            let count = viewer.assets.len();
            let _ = cx;
            count
        });
        assert_eq!(asset_count, 1, "fixture asset must merge");

        struct Host {
            viewer: Entity<MediaViewer>,
        }
        impl gpui::Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                // The production embedding: a pane-sized flex child, as the
                // media panel renders the viewer.
                div().size_full().child(self.viewer.clone())
            }
        }

        let (_, cx) = cx.add_window_view(|_window, _cx| Host {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();
        let short_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out in the viewer chain");

        // THE FIT assertion — the one this bug class evaded: the whole
        // widget (video area + transport) must fit INSIDE the window. The
        // broken layout (h_full on the tab content, a flex-column main-axis
        // child) made the widget extend past the window bottom by the tab
        // bar's height while every scaling assertion passed.
        let widget_bounds = cx
            .debug_bounds("media-widget")
            .expect("widget laid out with debug bounds");
        assert!(
            widget_bounds.bottom() <= px(600.) + px(1.),
            "the player must fit inside a 600px window: widget bottom {:?}",
            widget_bounds.bottom()
        );

        cx.simulate_resize(size(px(800.), px(900.)));
        cx.run_until_parked();
        let tall_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out after resize");

        // 300px window delta minus sub-pixel rounding — the property is
        // "tracks the window", not exact pixel equality.
        let height_delta = tall_bounds.size.height - short_bounds.size.height;
        assert!(
            height_delta > px(295.) && height_delta < px(305.),
            "video area height must track window height through the full viewer chain: \
             {:?} at 600px vs {:?} at 900px (delta {height_delta:?})",
            short_bounds.size.height,
            tall_bounds.size.height
        );

        let tall_widget_bounds = cx
            .debug_bounds("media-widget")
            .expect("widget laid out after resize");
        assert!(
            tall_widget_bounds.bottom() <= px(900.) + px(1.),
            "the player must fit inside a 900px window: widget bottom {:?}",
            tall_widget_bounds.bottom()
        );
    }

    /// The production panel embedding for the narrow-pane probes: the
    /// viewer pane on top (`flex_1`, `min_h_0`, `min_w_0`), the 1px split
    /// divider, and the director below at a fractional height — exactly as
    /// `media_panel.rs` renders them (minus the drag wiring, which needs a
    /// real pointer drag the harness doesn't synthesize).
    struct NarrowPaneHost {
        viewer: Entity<MediaViewer>,
    }

    impl gpui::Render for NarrowPaneHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .child(self.viewer.clone()),
                )
                .child(div().w_full().flex_shrink_0().h(px(1.)))
                .child(
                    div()
                        .w_full()
                        .h(DefiniteLength::Fraction(0.5))
                        .min_h_0()
                        .flex()
                        .flex_col(),
                )
        }
    }

    /// The theme/settings globals the viewer's render reads — shared by
    /// this module's layout tests.
    fn init_layout_test_globals(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
            if !cx.has_global::<theme::GlobalTheme>() {
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            }
        });
    }

    /// THE transcript-pane wrap contract (operator report 2026-10-26:
    /// paragraph text went off the pane edge). A single long paragraph's
    /// max-content width is far wider than the dock; the block must wrap
    /// at the definite pane width — the former flex-row container broke
    /// the definite-width propagation and laid the text as one line.
    #[gpui::test]
    fn transcript_pane_wraps_within_narrow_dock(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-long".into(),
                media_path: "/generated/long.mp4".into(),
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..200)
                        .map(|index| PaneWord {
                            word: format!("unwrappable-word-{index}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: String::new(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Transcript;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        let bounds = cx
            .debug_bounds("media-transcript-paragraph")
            .expect("paragraph block laid out");
        assert!(
            bounds.right() <= px(480.),
            "the paragraph wraps within the dock: {bounds:?}"
        );
        assert!(
            bounds.size.height > px(20.),
            "the wrapped paragraph is taller than one line: {bounds:?}"
        );
    }

    /// The label editor row (S4-3) fits the narrow dock: its fixed-width
    /// input and affordances lay out within the pane, never past its edge
    /// — the same wrap contract the paragraph blocks carry.
    #[gpui::test]
    fn label_editor_fits_narrow_dock(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-edit".into(),
                media_path: "/generated/edit.wav".into(),
                alignment: "original".into(),
                alignment_note: None,
                words: None,
                working_text: "words in the workspace".into(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Transcript;
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word: 0,
                    end_word: 1,
                },
                buffer: LabelBuffer::from_str("climax"),
            });
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        let bounds = cx
            .debug_bounds("media-transcript-label-editor")
            .expect("label editor row laid out");
        assert!(
            bounds.right() <= px(480.),
            "the label editor fits the dock: {bounds:?}"
        );
    }

    /// THE horizontal-fit invariant — the one the scaling test above does
    /// not encode. No part of the viewer — media content, header, toolbar,
    /// or tab bar — may exceed the pane's available width at any pane
    /// width, with the video's natural width (640px) larger than the pane.
    /// Historically the pane div lacked `min_w_0`, so the viewer's
    /// min-content width (untruncated header src + seven inline toolbar
    /// buttons ≈ 685px) inflated the pane and the video rendered clipped
    /// at the dock edge, unviewable. This test pins the fix in the current
    /// production embedding (top/bottom split): the pane is a flex-column
    /// child and still carries `min_w_0`. The final iteration resizes back
    /// — the no-thrash probe.
    #[gpui::test]
    fn viewer_content_fits_narrow_pane(cx: &mut TestAppContext) {
        if !std::path::Path::new(FIXTURE).exists() {
            return;
        }
        init_layout_test_globals(cx);

        let viewer = cx.new(|_| MediaViewer::new());
        let output = serde_json::json!({
            "content": {
                "status": "fetched",
                "display_hint": format!(
                    "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
                )
            }
        })
        .to_string();
        viewer.update(cx, |viewer, cx| {
            viewer.ingest_tool_result(&Value::String(output), "video_fetch", cx)
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });

        // (dock width, pane available) — the split is horizontal
        // (top/bottom panes), so the viewer spans the dock's full width;
        // the director no longer takes a horizontal share. 480px is the
        // adversarial dock: far narrower than the fixture's natural 640px
        // video.
        for (dock_width, pane_available) in [
            (px(700.), px(700.)),
            (px(480.), px(480.)),
            (px(700.), px(700.)),
        ] {
            cx.simulate_resize(size(dock_width, px(600.)));
            cx.run_until_parked();
            assert_viewer_fits_narrow_pane(cx, dock_width, pane_available);
        }
    }

    /// A failed player must not turn the transcript workspace into a dead
    /// end: Back and range selection remain reachable while the error is
    /// visible. The missing-file fixture fails synchronously and needs no
    /// codec, device, network, or operator data.
    #[gpui::test]
    fn transcript_media_failure_keeps_navigation_and_curation_reachable(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        let media_path = "/definitely-missing-transcript-audio.mp3";
        let widget = cx.new(|cx| {
            hkask_media_widget::MediaWidget::new(
                hkask_media_widget::MediaRef::new(
                    media_path.into(),
                    hkask_media_widget::MediaKind::Audio,
                ),
                cx,
            )
        });
        widget.update(cx, |widget, cx| widget.load(cx));
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-media-failure".into(),
                media_path: media_path.into(),
                alignment: "original".into(),
                alignment_note: None,
                words: Some(vec![PaneWord {
                    word: "recover".into(),
                    start_ms: 0,
                    end_ms: 100,
                }]),
                working_text: "recover".into(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Transcript;
            viewer.media_widget = Some(widget);
            viewer.media_widget_body = Some(transcript_media_body(media_path));
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(320.)));
        cx.run_until_parked();

        // Back rides at the top of the workspace list — reachable without
        // scrolling even on the tightest pane (the harness halves the
        // window, so the viewer area here is ~160px).
        let back = cx
            .debug_bounds("media-transcript-back-to-recordings")
            .expect("Back remains laid out during media failure");
        assert!(
            back.bottom() <= px(320.),
            "Back remains reachable during media failure: {back:?}"
        );
        // The failed 140px player leaves the mode chips and the paragraph
        // below the tight fold; the reachability contract under
        // virtualization is that scrolling renders them — no dead end.
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(ListOffset {
                item_ix: 2,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        for selector in ["media-transcript-select-mode", "media-transcript-paragraph"] {
            let bounds = cx.debug_bounds(selector).unwrap_or_else(|| {
                panic!("{selector} renders when scrolled to during media failure")
            });
            assert!(
                bounds.bottom() <= px(320.),
                "{selector} remains reachable during media failure: {bounds:?}"
            );
        }
    }

    /// The follow-along mark must stay VISIBLE during playback (S3d, the
    /// repair plan's R1 acceptance): with the mark on a late block and
    /// viewport tracking on, the marked block is scrolled into the pane.
    /// A virtualized off-screen block renders nothing at all, so its
    /// bounds being present AND inside the pane is visibility proof —
    /// before the fix the marked block sits far below the fold.
    #[gpui::test]
    fn follow_along_scrolls_the_marked_block_into_view(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-follow".into(),
                media_path: "/generated/follow.wav".into(),
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..200)
                        .map(|index| PaneWord {
                            word: format!("follow-word-{index}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: String::new(),
                paragraph_breaks: Some((1..20).map(|block| block * 10 - 1).collect()),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Transcript;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        // The mark lands on a late block — the follow must scroll that
        // block into view (the loop calls the same method each tick;
        // the mark computation is separately pinned by
        // current_word_index_marks_the_containing_word).
        viewer.update(cx, |viewer, cx| {
            viewer.current_word = Some(190);
            viewer.follow_viewport = true;
            viewer.follow_marked_block_into_view();
            cx.notify();
        });
        cx.run_until_parked();

        let bounds = cx
            .debug_bounds("media-transcript-marked-block")
            .expect("the follow-along must scroll the marked block into view");
        assert!(
            bounds.top() >= px(0.),
            "the marked block is on-screen, not above the pane: {bounds:?}"
        );
        assert!(
            bounds.bottom() <= px(600.),
            "the marked block is within the pane: {bounds:?}"
        );
    }

    /// A manual scroll suspends follow-along viewport tracking, and the
    /// Follow toggle re-enables it (S3d, R1 acceptance). The observable a
    /// wheel scroll produces on the follow tick is the list-offset change;
    /// the toggle then reveals the current mark again.
    #[gpui::test]
    fn manual_scroll_suspends_follow_and_the_toggle_re_enables(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-suspend".into(),
                media_path: "/generated/suspend.wav".into(),
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..200)
                        .map(|index| PaneWord {
                            word: format!("suspend-word-{index}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: String::new(),
                paragraph_breaks: Some((1..20).map(|block| block * 10 - 1).collect()),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Transcript;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        // Baseline the tick, scroll by hand, and observe the next tick:
        // the offset changed with no follow reveal pending, so tracking
        // suspends.
        viewer.update(cx, |viewer, cx| {
            viewer.note_follow_scroll_tick(cx);
            viewer.transcript_list.scroll_by(px(200.));
            viewer.note_follow_scroll_tick(cx);
            assert!(
                !viewer.follow_viewport,
                "a manual scroll suspends viewport tracking"
            );
            cx.notify();
        });
        cx.run_until_parked();
        // Scrolling back to the top is the operator's way back to the Follow
        // chip — the whole workspace scrolls together now (S3d).
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_by(px(-200.));
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-transcript-follow-toggle").is_some(),
            "the Follow toggle renders in the workspace header rows"
        );

        // The toggle re-enables tracking and reveals the current mark.
        viewer.update(cx, |viewer, cx| {
            viewer.current_word = Some(190);
            viewer.toggle_follow_viewport(cx);
            assert!(
                viewer.follow_viewport,
                "the toggle re-enables viewport tracking"
            );
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-transcript-marked-block").is_some(),
            "re-enabling follow reveals the marked block"
        );
    }

    /// The recordings browser's two-click delete: the Delete affordance
    /// renders per row, arming swaps it for Confirm delete, and the
    /// dispatch closes the edit lifecycle through the unwired-invoker
    /// degradation path (tests run without a shared tool invoker).
    #[gpui::test]
    fn transcript_browser_delete_confirms_and_closes_the_edit(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_listing = Some(vec![TranscriptSummaryRow {
                id: "t-delete".into(),
                media_path: "/generated/dunning.mp4".into(),
                words_count: 558,
                duration_secs: 276,
                has_word_timings: true,
                alignment: "original".into(),
            }]);
            viewer.active_tab = ViewerTab::Transcript;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(320.)));
        cx.run_until_parked();

        // The Delete affordance is laid out on the row, alongside the open
        // affordance — the operator can always remove a recording.
        assert!(
            cx.debug_bounds("media-transcript-delete").is_some(),
            "the Delete affordance must render on the recordings row"
        );

        // Arming the confirmation swaps the affordance.
        viewer.update(cx, |viewer, cx| {
            viewer
                .confirm_delete_transcript
                .confirm(&"t-delete".to_owned());
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-transcript-delete-confirm").is_some(),
            "the armed row must show Confirm delete"
        );
        assert!(
            cx.debug_bounds("media-transcript-delete").is_none(),
            "the plain Delete affordance must be replaced while confirming"
        );

        // The dispatch closes the edit lifecycle even on the unwired
        // degradation path — a second admission must not report an edit
        // already in progress (the lifecycle would be stuck Loading).
        viewer.update(cx, |viewer, cx| {
            viewer.delete_transcript_recording("t-delete".into(), cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, cx| {
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("cannot delete recordings")),
                "the unwired-invoker degradation must surface a status, got {:?}",
                viewer.status
            );
            assert_eq!(
                viewer.confirm_delete_transcript.armed, None,
                "dispatch disarms the confirmation"
            );
            viewer.delete_transcript_recording("t-delete".into(), cx);
            assert!(
                !viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("already in progress")),
                "a completed edit must admit the next one"
            );
            cx.notify();
        });
    }

    /// The shared fit assertions: the video area and the whole player
    /// widget must stay within the dock's right edge, and the video area's
    /// width must stay within the pane's available width (+1px sub-pixel
    /// tolerance).
    fn assert_viewer_fits_narrow_pane(
        cx: &mut gpui::VisualTestContext,
        dock_width: gpui::Pixels,
        pane_available: gpui::Pixels,
    ) {
        let widget_bounds = cx
            .debug_bounds("media-widget")
            .expect("widget laid out with debug bounds");
        let video_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out with debug bounds");
        assert!(
            widget_bounds.right() <= dock_width + px(1.),
            "the player must fit inside the {dock_width:?} dock: widget right {:?}",
            widget_bounds.right()
        );
        assert!(
            video_bounds.right() <= dock_width + px(1.),
            "the video area must fit inside the {dock_width:?} dock: right {:?}",
            video_bounds.right()
        );
        assert!(
            video_bounds.size.width <= pane_available + px(1.),
            "the video area must stay within the pane's ~{pane_available:?} available \
             width, got {:?} — the pane inflated to its content's min-width",
            video_bounds.size.width
        );
    }
}
