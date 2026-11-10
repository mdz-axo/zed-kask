//! The media panel's viewing pane — task modes over real state (T1,
//! media-panel-redesign plan).
//!
//! - **Workspace**: the selected asset in one screen — the player and
//!   derivative-edit toolbar (marks, trim, concat — serving audio and
//!   video through one position source) as the primary area, with a
//!   toggleable inspector pane whose pane-internal tabs render the
//!   selected asset's record, tags, lineage, faces, transcript links,
//!   researcher annotations, and reel provenance (via
//!   `gallery_asset_detail`, `educt_list_transcripts`,
//!   `gallery_annotation_list`).
//! - **Library**: the gallery's actual assets (via `gallery_list_assets`)
//!   merged with assets surfaced by the conversation's tool results, with
//!   delete (two-step confirm) affordances. Each fetched page reconciles
//!   the list — assets deleted from the gallery drop out of it.
//! - **Activity layer** (T2): research jobs (via `job_list`) as ambient
//!   process state, never a destination — badges on the affected Library
//!   rows, a live-count Activity toggle in the toolbar, and an on-demand
//!   drawer whose rows keep op/status/created_at with Cancel for
//!   queued/running jobs and the process-local history disclosure.
//! - **Transcript**: the synced recordings — the recording-first browser
//!   (per-recording transcription status, the Transcribe affordance), and
//!   the synced-recording workspace (follow-along viewport tracking,
//!   word-click seek, two-click select → label editor → highlight
//!   create/rename/delete, cuts, and reel compose+render), plus the
//!   visual-transcript event surface (S5 — a reading-surface toggle over
//!   the same recording: the time-ordered event log with click-to-seek,
//!   select-span → researcher tag authoring, and armed-target redaction
//!   decision authoring + render dispatch with its review lines).
//!
//! All tool calls go through the process-global `ToolInvoker` (the governed
//! McpRuntime dispatch) against the `media` server. Failures surface in a
//! status line — visible, never silent.

use std::{collections::HashSet, ops::Range, sync::Arc, time::Duration};

use acp_thread::AgentThreadEntry;
use agent_client_protocol::v2::schema::ToolCallId;
use gpui::{
    Context, Entity, HighlightStyle, InteractiveText, KeyDownEvent, Keystroke, ListAlignment,
    ListOffset, ListState, MouseButton, Render, SharedString, StrikethroughStyle, StyledText, Task,
    Window,
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
/// The nonterminal job statuses — the observation probes' filter values.
/// The server's `job_list` takes a SINGULAR status filter applied before
/// the newest-first sort and limit, so one probe per status observes live
/// jobs anywhere in the bounded store: an older running job behind a full
/// page of newer completions included (loop-register L11, the deferred
/// impedance the activity layer settles).
const NONTERMINAL_JOB_STATUSES: [&str; 3] = ["queued", "running", "cancelling"];
/// The activity drawer's share of the viewer's height (T2): the drawer is
/// an on-demand strip below the active mode — bounded like the inspector
/// pane, scrolling internally so it stays usable in every dock shape.
const ACTIVITY_DRAWER_FRACTION: f32 = 0.3;
/// Follow-along polling cadence (S3c) — 4 Hz marks the containing word
/// responsively without saturating the render loop.
const FOLLOW_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The transcript search's rescan debounce (S3): a burst of edits rescans
/// once, this long after the last keystroke — the GPUI-native timer
/// (never smol::Timer — .rules).
const TRANSCRIPT_SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// The workspace list's fixed header items INSIDE the scrolling list (the
/// composed two-pane fix, 2026-11-07): the alignment/status row and the
/// surface's section label — read-once status copy, content-like, so they
/// scroll with the text. The player, the surface/mode chips, and the
/// search input PIN ABOVE the list (the interaction band, rendered by
/// `render_transcript_workspace`) — the persistent master object and its
/// controls never virtualize away with the navigated detail. This
/// reverses the one-list design's explicit judgment ("a pinned header
/// taller than a tight pane would starve the reading area", 2026-11-04):
/// the operator's live walk on the 62-minute recording showed the cost of
/// that choice — every deep follow-along reveal displaced the primary
/// media object out of the pane (the synced-recording contract broken in
/// its common case) — so the bounded band (~200px: 140px player + chips +
/// search) takes precedence over maximal reading height, and the reading
/// area keeps the remainder at every pane height (the operator's
/// bounded-player ruling, 2026-11-02).
const WORKSPACE_HEADER_ITEMS: usize = 2;

/// The workspace list's fixed tail items after the transcript blocks:
/// highlights, cuts, and reel actions — always present as items, so the
/// item count (and with it the reading position) stays stable while the
/// operator curates annotations.
const WORKSPACE_LIST_TAIL_ITEMS: usize = 3;

/// The events surface's fixed tail items after the event rows: the
/// visual-transcript authoring section (span tag + redaction dispatch +
/// last render review) — the events sibling of the words tail.
const WORKSPACE_EVENTS_TAIL_ITEMS: usize = 1;

/// The inspector pane's share of the workspace body's height (T1): the
/// player is the primary area and keeps the larger share; the pane scrolls
/// internally, so its seven tabs stay usable in every dock shape (a fixed
/// height would starve the player in short docks).
const INSPECTOR_PANE_FRACTION: f32 = 0.38;

#[derive(Clone, Debug, PartialEq, Eq)]
enum RequestOwner {
    GalleryPage(usize),
    Jobs,
    JobObservation,
    Detail(String),
    Edit(&'static str),
    DetailTranscripts(String),
    DetailAnnotations(String),
    Transcript(String),
    TranscriptListing,
    VisualEvents(String),
}

/// A strict-parsed `educt_list_transcripts` row — the fields the workspace
/// inspector's Transcripts tab and the Library's recordings section render;
/// a missing or malformed field rejects the whole listing.
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
    /// The recording this transcript is stored against, when it has one:
    /// the stable gallery asset id of the RECORDING (R3,
    /// 2026-10-28). `None` for unlinked transcripts — a recording
    /// that left the gallery index keeps its transcript and its durable
    /// exports, detached (the server's detach contract).
    gallery_asset_id: Option<String>,
}

/// The per-recording transcription status (R2/R5): a running transcribe
/// job whose source is the recording shows Transcribing; a stored
/// transcript linked to the recording shows Transcribed; otherwise Not
/// transcribed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordingTranscriptionStatus {
    Transcribing,
    Transcribed,
    NotTranscribed,
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

/// S6 reel composer: one clip of the composed reel — a researcher highlight
/// carried into the ordered clip list with its own inclusion flag and
/// in-reel strike sub-ranges. Identity is the highlight's (annotation id +
/// source word range): composer state re-matches across every workspace
/// reload on that key, so order, inclusion, and strikes survive reloads
/// (a rename changes neither the id nor the range).
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReelClip {
    annotation_id: String,
    start_word: usize,
    end_word: usize,
    label: String,
    include: bool,
    /// In-reel strike sub-ranges over the clip's own source range — the
    /// compose bakes them into the clip's Keep op through the server's
    /// published difference helper, never as a global Cut op (the algebra
    /// unions ALL cuts and subtracts them from EVERY keep).
    strikes: Vec<(usize, usize)>,
}

/// S6 reel preview: the composed reel's ordered span plan driving the
/// shared player without rendering. `spans` is `edl_to_clip_plan`'s output
/// over the stored SOURCE bundle words — the render path's own mapping;
/// the working view's timings are never read (they are interpolated
/// estimates over different indices on `re_anchored` views).
#[derive(Clone, Debug, PartialEq)]
struct ReelPreview {
    /// (start_ms, end_ms) per span, composed order.
    spans: Vec<(u64, u64)>,
    /// The span currently playing.
    index: usize,
}

impl ReelPreview {
    /// One walk step: if the position has crossed the CURRENT span's end,
    /// move to the next span in composed order — seeking its start — or
    /// finish when the plan is done. Spans play in COMPOSED order, not time
    /// order: a span whose absolute times sit before the current one is
    /// entered by the seek, never skipped by the position (a later clip in
    /// the composed list always plays, however early it sits in the
    /// recording).
    fn advance(&mut self, position_ms: u64) -> ReelPreviewStep {
        let Some(&(_, current_end)) = self.spans.get(self.index) else {
            return ReelPreviewStep::Finished;
        };
        if position_ms < current_end {
            return ReelPreviewStep::Hold;
        }
        self.index += 1;
        match self.spans.get(self.index) {
            Some(&(next_start, _)) => ReelPreviewStep::Seek(next_start),
            None => ReelPreviewStep::Finished,
        }
    }
}

/// One preview walk step's outcome (S6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReelPreviewStep {
    /// The current span is still playing — keep polling.
    Hold,
    /// The walk moved onto a new span — seek the player to this start (ms).
    Seek(u64),
    /// The plan played through — the preview is done.
    Finished,
}

/// The workspace's reading surface (operator refinement 2026-10-30): the
/// word-transcript pane and the visual-transcript event log are parallel
/// annotations over the same recording — the operator switches surfaces
/// while the click-to-seek discipline stays identical on both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TranscriptSurface {
    #[default]
    Words,
    Events,
}

/// One visual-transcript event row (S5), strict-parsed from a
/// `layers_transcript` response: a time-ordered layer item the operator
/// reads, seeks playback from, selects spans over, and arms redaction
/// targets on. `model` is the provenance model — `"user"` marks a
/// researcher-authored event (the human/machine distinction, design §3).
/// `box_norm` carries the object payload's normalized box when the row has
/// one (tolerant: a payload is arbitrary JSON, so a malformed box renders
/// the row without the target affordance rather than rejecting the whole
/// event log).
#[derive(Clone, Debug, PartialEq)]
struct PaneEventRow {
    start_sec: f64,
    end_sec: Option<f64>,
    kind: String,
    layer_id: String,
    item_id: String,
    searchable_text: String,
    model: String,
    box_norm: Option<[f64; 4]>,
    /// The object payload's own label (e.g. `"person"`), preferred over
    /// `searchable_text` as the redaction target's display label.
    object_label: Option<String>,
    /// The S1 refinement's read-time corroboration: the sensory badges of
    /// every overlapping event plus its own ("heard" via audio_event AND
    /// "seen" via object/semantic; "spoken" via the transcript stream).
    evidence: Vec<String>,
}

impl PaneEventRow {
    /// The row's range end, falling back to its start for point-in-time
    /// events (the store's point-item contract).
    fn end_or_start(&self) -> f64 {
        self.end_sec.unwrap_or(self.start_sec)
    }

    /// The stable source ref for redaction lineage (`source_tag_refs`,
    /// design §6): `layer_id/item_id`.
    fn source_ref(&self) -> String {
        format!("{}/{}", self.layer_id, self.item_id)
    }
}

/// The strict-parsed `layers_transcript` payload for the open recording
/// (S1's reading surface). `status` is `"completed"` or the named
/// `"no_layers"` degradation (with its note) — never silent.
#[derive(Clone, Debug, PartialEq)]
struct VisualEventsState {
    status: String,
    events: Vec<PaneEventRow>,
    events_total: usize,
    kinds_present: Vec<String>,
    note: Option<String>,
}

impl VisualEventsState {
    /// Whether the loaded page is a truncated prefix of the recording's
    /// full event log — the server pages `layers_transcript` (the panel
    /// requests the 5000-event maximum), so a longer recording's log
    /// reads as a prefix of its total.
    fn truncated(&self) -> bool {
        self.events.len() < self.events_total
    }

    /// The truncation note's wording — page vs total, so the operator
    /// reading the list never mistakes a truncated page for the whole
    /// log.
    fn truncation_note(&self) -> String {
        format!(
            "showing {} of {} events — refine your search or window",
            self.events.len(),
            self.events_total
        )
    }
}

/// The armed redaction target (S5 criterion 3): one event carrying a
/// normalized box — the tier-1 decision's spatial target, keyed by its
/// stable source ref so the authored decision carries the OMC lineage
/// (`source_tag_refs`, design §6).
#[derive(Clone, Debug, PartialEq)]
struct RedactionTarget {
    label: String,
    box_norm: [f64; 4],
    source_ref: String,
}

/// The tier-1 cover effect for the next authored redaction decision — the
/// card's contract vocabulary (blur|pixelate|black; the server also serves
/// delogo, which the panel leaves to direct tool use).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PaneRedactionEffect {
    #[default]
    Blur,
    Pixelate,
    Black,
}

impl PaneRedactionEffect {
    fn as_str(&self) -> &'static str {
        match self {
            PaneRedactionEffect::Blur => "blur",
            PaneRedactionEffect::Pixelate => "pixelate",
            PaneRedactionEffect::Black => "black",
        }
    }

    /// The effect chip cycles the vocabulary.
    fn next(&self) -> Self {
        match self {
            PaneRedactionEffect::Blur => PaneRedactionEffect::Pixelate,
            PaneRedactionEffect::Pixelate => PaneRedactionEffect::Black,
            PaneRedactionEffect::Black => PaneRedactionEffect::Blur,
        }
    }
}

/// One rendered segment of the redaction plan (S3's `segments[]`): mode,
/// source-PTS range, and how many decision items it applies.
#[derive(Clone, Debug, PartialEq)]
struct PaneRenderSegment {
    mode: String,
    start_sec: f64,
    end_sec: f64,
    items: usize,
}

/// One decision's applied rendering details (S3's `applied_items[]`) —
/// what was drawn where, for review/lineage in the panel.
#[derive(Clone, Debug, PartialEq)]
struct PaneAppliedRedaction {
    item_id: String,
    effect: String,
    pixel_x: u32,
    pixel_y: u32,
    pixel_w: u32,
    pixel_h: u32,
    local_start_sec: f64,
    local_end_sec: f64,
}

/// The last redaction render's review/lineage summary, strict-parsed from
/// `layers_render_redactions` (S5 criterion 3: render the result's
/// `segments[]` and `applied_items[]` for review).
#[derive(Clone, Debug, PartialEq)]
struct RedactionRenderSummary {
    decision_layer_id: String,
    decisions_rendered: usize,
    reencoded_segments: usize,
    stream_copied_segments: usize,
    segments: Vec<PaneRenderSegment>,
    applied: Vec<PaneAppliedRedaction>,
}

/// A researcher tag/note row for the workspace inspector's Annotations tab
/// (strict-parsed from `gallery_annotation_list` — the direct-manipulation
/// CRUD surface, 2026-10-27).
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneAnnotation {
    annotation_id: String,
    kind: String,
    value: String,
    created_at: String,
}

/// The workspace inspector's open annotation editor: a new tag/note or an
/// existing row's value being replaced. Mirrors the transcript label editor's
/// modal pattern — Enter confirms, Escape cancels.
#[derive(Clone, Debug)]
struct AnnotationEdit {
    kind: &'static str,
    /// `None` creates a new row; `Some` replaces that annotation's value.
    annotation_id: Option<String>,
    buffer: LabelBuffer,
}

/// The strict-parsed `educt_get_transcript` payload the workspace's
/// transcript sub-surface renders (S3a): working words + alignment, the
/// latest paragraph layer's
/// breaks, the latest speaker layer's spans, and the layer summary strip —
/// all from the single dispatch (D2). `words` is `None` on text-only views
/// (Unaligned/Untimed) — those render `working_text` with a visible status
/// note instead.
#[derive(Clone, Debug, PartialEq)]
struct TranscriptPaneState {
    transcript_id: String,
    media_path: String,
    /// The gallery recording this transcript is stored against, when it
    /// has one: the stable asset id of the RECORDING (R3, 2026-10-28).
    /// `None` for an unlinked transcript — a recording that left the
    /// gallery index keeps its transcript and its durable exports,
    /// detached (the server's detach contract).
    gallery_asset_id: Option<String>,
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
    /// The live correction layer's edits (S5): the working view names the
    /// layer by `correction_layer_id`, and a new correction merges over
    /// these — disjoint edits carried verbatim, overlaps superseded — so
    /// the newest correction layer never silently reverts an earlier fix
    /// (the silent-revert guard).
    correction_edits: Vec<PaneCorrectionEdit>,
    /// The immutable source bundle's word timings (S6): the render path's
    /// own word→time input — `edl_to_clip_plan` runs over these server-side,
    /// and the reel preview maps its spans through the same words so
    /// preview and rendered reel agree on `re_anchored` views (the working
    /// `words` above carry interpolated estimates over different indices
    /// there). Empty when the stored bundle carries no word timings (a
    /// text-only source).
    source_words: Vec<PaneWord>,
}

/// One stored correction edit of the live correction layer (S5) — the
/// merged prior corrections a new correction must carry. Source word
/// indices, the server's `CorrectionEdit` wire shape.
#[derive(Clone, Debug, PartialEq)]
struct PaneCorrectionEdit {
    start_word: usize,
    end_word: usize,
    replacement: String,
    reason: String,
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

/// The panel's task modes (T1/T2/T3, media-panel-redesign plan): primary
/// navigation mirrors user tasks, never server subsystems. `Workspace` is
/// the one-asset mode — the player, the edit toolbar, and the toggleable
/// inspector pane in one screen; an open transcript renders as its
/// sub-surface (T3), bound to the linked recording's player. `Library` is
/// the browse mode — every indexed asset as rows, with the recordings'
/// transcription state and their stored transcripts nested beneath them
/// (T3). The Queue tab the activity layer replaced (T2) and the Transcript
/// tab these surfaces absorbed (T3) are gone: process state is ambient
/// (badges, the toolbar Activity toggle, an on-demand drawer) and
/// transcripts live on the surfaces that use them — never a navigation
/// destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerTab {
    Workspace,
    Library,
}

/// The workspace inspector's pane-internal tabs (T1): the seven Detail
/// sections as alternate views of the same selected object — the
/// legitimate tab use (HIG "Tab views": closely related areas of
/// content). These are not navigation destinations; the pane toggles as
/// a whole and every tab renders the same selected asset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InspectorTab {
    #[default]
    Record,
    Tags,
    Lineage,
    Faces,
    Transcripts,
    Annotations,
    Reel,
}

impl InspectorTab {
    /// The tab set every pane consumer iterates — one source for the
    /// seven sections so render and tests cannot drift.
    const ALL: [Self; 7] = [
        Self::Record,
        Self::Tags,
        Self::Lineage,
        Self::Faces,
        Self::Transcripts,
        Self::Annotations,
        Self::Reel,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Record => "Record",
            Self::Tags => "Tags",
            Self::Lineage => "Lineage",
            Self::Faces => "Faces",
            Self::Transcripts => "Transcripts",
            Self::Annotations => "Annotations",
            Self::Reel => "Reel",
        }
    }

    /// The full element id the tab chip renders under — a `&'static str`
    /// so layout tests can address it through `debug_bounds` (which keys on
    /// static ids).
    fn element_id(self) -> &'static str {
        match self {
            Self::Record => "media-inspector-tab-record",
            Self::Tags => "media-inspector-tab-tags",
            Self::Lineage => "media-inspector-tab-lineage",
            Self::Faces => "media-inspector-tab-faces",
            Self::Transcripts => "media-inspector-tab-transcripts",
            Self::Annotations => "media-inspector-tab-annotations",
            Self::Reel => "media-inspector-tab-reel",
        }
    }
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
    /// The activity drawer's visibility (T2): the toolbar's Activity
    /// toggle opens it over any mode — ambient process state observed on
    /// demand (FCP Background Tasks), never a navigation destination.
    activity_open: bool,
    /// The observed conversation thread (weak — the panel owns it), kept so
    /// `refresh` can re-ingest tool-result display hints without the panel
    /// forwarding the entity again.
    thread: Option<gpui::WeakEntity<acp_thread::AcpThread>>,
    /// The media widget for the currently-selected asset in the workspace.
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
    /// The nonterminal research jobs observed ANYWHERE in the bounded
    /// store — the newest-first page's live rows plus the status-probe
    /// rows (T2, loop-register L11: a live job deeper than the page
    /// stays observed, not silently dropped). Badges, the toolbar
    /// Activity count, and the poll condition read this set.
    live_jobs: Vec<JobRecord>,
    /// The non-terminal transcribe job ids the last observation saw
    /// (R5) — when one lands while the Library is showing, its recordings
    /// section reloads the listing once so the finished transcript
    /// appears under its recording.
    transcribing_ids: HashSet<String>,
    jobs_total: Option<usize>,
    jobs_limit: usize,
    jobs_has_more: bool,
    jobs_request: RequestLifecycle,
    /// The observation battery's lifecycle (the three status-filtered
    /// probes a tick issues when the page admits deeper jobs) — its own
    /// single-flight guard, so a slow battery cannot overlap the next.
    job_observation_request: RequestLifecycle,
    /// The process-local history disclosure (T2): the
    /// `history_scope`/`restart_behavior` fields the strict listing parse
    /// already validates — the drawer's footer copy renders them verbatim.
    jobs_history_scope: Option<String>,
    jobs_restart_behavior: Option<String>,
    processed_job_results: HashSet<String>,
    queue_poll_task: Option<Task<()>>,
    /// Inspector data for the selected asset (from `gallery_asset_detail`).
    detail: Option<Value>,
    detail_request: RequestLifecycle,
    /// Transcript summaries for the selected asset's inspector pane (from
    /// `educt_list_transcripts`). `None` until the request completes;
    /// `Some(vec![])` means loaded-and-none — the Transcripts tab renders
    /// its empty copy for that.
    detail_transcripts: Option<Vec<TranscriptSummaryRow>>,
    detail_transcripts_request: RequestLifecycle,
    /// Researcher tags/notes for the selected asset (from
    /// `gallery_annotation_list`) — the inspector's Annotations tab
    /// direct-manipulation CRUD surface (2026-10-27).
    detail_annotations: Option<Vec<PaneAnnotation>>,
    detail_annotations_request: RequestLifecycle,
    /// The inspector's open annotation editor and its input's focus
    /// handle (lazily created on open, like the transcript label editor's).
    annotation_edit: Option<AnnotationEdit>,
    annotation_input_focus: Option<gpui::FocusHandle>,
    /// The workspace's inspector pane (T1): whether the supporting pane
    /// renders under the player. Toggling never clears the selection, the
    /// retained player, or loaded inspector data — the pane only changes
    /// what renders.
    inspector_open: bool,
    /// The inspector pane's active pane-internal tab (T1) — one of the
    /// seven Detail sections.
    inspector_tab: InspectorTab,
    /// The open transcript workspace (from `educt_get_transcript`) — `None`
    /// until the request completes; `Some` renders the workspace's
    /// transcript sub-surface (T3). The state persists across mode
    /// switches so returning to an open transcript does not refetch.
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
    /// The stored-transcript listing (from `educt_list_transcripts`
    /// unfiltered) — the Library's recordings section renders the
    /// recordings' transcription state and their transcript rows from
    /// it (T3; operator report 2026-10-26: the section must list what's
    /// available, not dead-end).
    transcript_listing: Option<Vec<TranscriptSummaryRow>>,
    transcript_listing_request: RequestLifecycle,
    /// S4 part-2: when on, word clicks build highlight ranges instead of
    /// seeking (a mode toggle — every-click-seek plus accidental creation
    /// is worse than an explicit select mode).
    selection_mode: bool,
    /// The first click of a pending two-click selection (a global working
    /// word index).
    selection_anchor: Option<usize>,
    /// S2: the press word of an in-progress drag selection (a global
    /// working word index) — armed by a press over a word, cleared by any
    /// in-surface press that is not over one, and consumed by the release.
    /// Press-and-release across words is the one-gesture selection path,
    /// active in both interaction modes; a same-word release stays a
    /// plain click (the click path seeks).
    word_drag_anchor: Option<usize>,
    /// S2: the word under the pointer (a global working word index),
    /// tracked by exact hover: None over word gaps and outside the text,
    /// so a release there cancels the pending drag rather than selecting
    /// from a stale position.
    drag_last_word: Option<usize>,
    /// S2: the selection popover — a completed word range awaiting its
    /// action (Highlight / Cut / Copy time / Include in reel), opened by
    /// drag-select and the two-click range. Not modal: a fresh anchor or a
    /// newer completed range replaces/dismisses it, seek clicks leave it
    /// be, and the label editor (its Highlight arm) supersedes it.
    selection_popover: Option<(usize, usize)>,
    /// The selection popover's focus handle, created lazily on first open
    /// (pure tests construct the viewer without a GPUI context).
    selection_popover_focus: Option<gpui::FocusHandle>,
    /// S8: the right-click context menu — the completed word range the
    /// menu's actions apply to (Highlight / Strike out / Copy time /
    /// Include in reel), opened by a right click on the word surface.
    /// The range is captured when the menu opens, so the actions never
    /// read a working selection that has since moved.
    context_menu: Option<(usize, usize)>,
    /// The context menu's focus handle, created lazily on first open
    /// (pure tests construct the viewer without a GPUI context).
    context_menu_focus: Option<gpui::FocusHandle>,
    /// S8: the word under the pointer when the right button was pressed —
    /// the context menu's fallback target (a pending selection or open
    /// tray owns the range instead).
    context_menu_fallback: Option<usize>,
    /// S3: the in-transcript search — the typed query, its ordered matches
    /// over working word indices, and the current match. Blank query → no
    /// matches → no marks.
    transcript_search: TranscriptSearch,
    /// The search input's focus handle, created lazily on first render
    /// together with its focus/blur listeners (S3: focusing the search
    /// suspends viewport tracking so typing and the auto-landed matches
    /// never fight the follow-along; blur restores what it took).
    transcript_search_focus: Option<gpui::FocusHandle>,
    /// The search input's focus/blur subscriptions — held for the viewer's
    /// lifetime (dropping them would unregister the listeners).
    transcript_search_focus_subscriptions: Vec<gpui::Subscription>,
    /// Whether focusing the search input suspended viewport tracking (S3):
    /// blur restores tracking only when the search took it — a manually
    /// suspended follow stays suspended.
    transcript_search_suspended_follow: bool,
    /// S3: the pending debounced rescan — a keystroke burst rescans once,
    /// 150 ms after the last edit. Dropping the task cancels the pending
    /// rescan (the follow-task lifecycle pattern).
    transcript_search_task: Option<Task<()>>,
    /// S4-3: the open label editor — a completed range awaiting its label
    /// (create) or a researcher highlight being renamed. Word clicks are
    /// inert while it is open; Enter confirms, Escape cancels.
    highlight_edit: Option<HighlightEdit>,
    /// The label editor input's focus handle, created lazily on first open
    /// (pure tests construct the viewer without a GPUI context).
    label_input_focus: Option<gpui::FocusHandle>,
    /// S6 reel composer: the ordered clip list — the ONE source of truth
    /// for reel membership AND order (the unordered `reel_selection` set it
    /// replaces is deleted; no set-and-list pair can drift). Re-matched to
    /// highlight identity on every workspace load: existing clips keep
    /// their composer deltas, new researcher highlights join included at
    /// the tail, deleted highlights drop their clips.
    reel_clips: Vec<ReelClip>,
    /// S6 reel preview: the armed preview's span plan and walk position —
    /// `None` when no preview is running.
    reel_preview: Option<ReelPreview>,
    /// The preview's polling task — the follow-task lifecycle pattern:
    /// single instance, cancelled by taking the handle (the teardown
    /// sites), self-cancels when the substrate leaves.
    reel_preview_task: Option<Task<()>>,
    /// The workspace's reading surface (operator refinement 2026-10-30):
    /// the word-transcript pane or the visual-transcript event log.
    transcript_surface: TranscriptSurface,
    /// The visual-transcript event log for the open recording (from
    /// `layers_transcript`) — `None` until the events surface first loads
    /// it; the last-good log stays visible while a reload is in flight.
    visual_events: Option<VisualEventsState>,
    visual_events_request: RequestLifecycle,
    /// The events surface's pending two-click span selection: the armed
    /// first-click event index (S5, mirroring the word pane's anchor).
    event_selection_anchor: Option<usize>,
    /// The events surface's completed span, in source-PTS seconds — the
    /// pending target of both authoring flows (tag, redaction).
    event_selection_span: Option<(f64, f64)>,
    /// The armed redaction target: one box-carrying event (criterion 3's
    /// "select target"), keyed by its stable source ref.
    redaction_target: Option<RedactionTarget>,
    /// The effect for the next authored redaction decision (cycled chip).
    redaction_effect: PaneRedactionEffect,
    /// The last redaction render's review/lineage summary (S3's segments[]
    /// + applied_items[]), rendered in the events tail.
    redaction_render: Option<RedactionRenderSummary>,
    edit_request: RequestLifecycle,
    edit_progress: Option<String>,
    /// Two-step delete confirmation for Library rows: the armed asset row
    /// index (the one shared confirm-arm pattern — see `ConfirmArm`).
    confirm_delete: ConfirmArm<usize>,
    /// Two-step delete confirmation for the Library's transcript rows:
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
            active_tab: ViewerTab::Workspace,
            activity_open: false,
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
            live_jobs: Vec::new(),
            transcribing_ids: HashSet::new(),
            jobs_total: None,
            jobs_limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
            jobs_has_more: false,
            jobs_request: RequestLifecycle::new(),
            job_observation_request: RequestLifecycle::new(),
            jobs_history_scope: None,
            jobs_restart_behavior: None,
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
            inspector_open: true,
            inspector_tab: InspectorTab::default(),
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
            word_drag_anchor: None,
            drag_last_word: None,
            selection_popover: None,
            selection_popover_focus: None,
            context_menu: None,
            context_menu_focus: None,
            context_menu_fallback: None,
            transcript_search: TranscriptSearch::default(),
            transcript_search_focus: None,
            transcript_search_focus_subscriptions: Vec::new(),
            transcript_search_suspended_follow: false,
            transcript_search_task: None,
            highlight_edit: None,
            label_input_focus: None,
            reel_clips: Vec::new(),
            reel_preview: None,
            reel_preview_task: None,
            transcript_surface: TranscriptSurface::default(),
            visual_events: None,
            visual_events_request: RequestLifecycle::new(),
            event_selection_anchor: None,
            event_selection_span: None,
            redaction_target: None,
            redaction_effect: PaneRedactionEffect::default(),
            redaction_render: None,
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
        if self.inspector_should_load() {
            self.load_inspector(cx);
        }
    }

    /// Toggle the workspace's inspector pane (T1): the supporting pane hides
    /// or returns without touching the selection, the retained player, or
    /// any loaded inspector data. Reopening refreshes the reads for the
    /// same object.
    fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspector_open = !self.inspector_open;
        cx.notify();
        if self.inspector_should_load() {
            self.load_inspector(cx);
        }
    }

    /// Whether the inspector's reads should fire now (T1): the workspace is
    /// the active mode, the pane is visible, and the selection is an
    /// indexed asset. Surfaced (unindexed) assets produce no reads — their
    /// inspector renders the honest not-indexed copy instead, and the
    /// status line stays quiet for the routine case.
    fn inspector_should_load(&self) -> bool {
        self.active_tab == ViewerTab::Workspace
            && self.inspector_open
            && self
                .selected
                .and_then(|ix| self.assets.get(ix))
                .and_then(|asset| asset.gallery_asset_id.as_deref())
                .is_some()
    }

    /// Fire the inspector's reads for the selected asset (T1): the detail
    /// object, its transcript links, and its researcher annotations — the
    /// same trio the Detail tab loaded on activation, now a pane of the
    /// workspace. Each read is keyed to the selected asset's stable id and
    /// guarded by its own request lifecycle, so a slow response in one
    /// cannot blank another.
    fn load_inspector(&mut self, cx: &mut Context<Self>) {
        self.load_detail(cx);
        self.load_detail_transcripts(cx);
        self.load_detail_annotations(cx);
    }

    fn stop_queue_polling(&mut self) {
        self.queue_poll_task.take();
    }

    fn activate_tab(&mut self, tab: ViewerTab, cx: &mut Context<Self>) {
        if self.active_tab == ViewerTab::Workspace && tab != ViewerTab::Workspace {
            // Leaving the workspace tears down the transcript sub-surface's
            // interaction state (T3, the old Transcript-leave rule): the
            // shared player suspends, the follow task cancels, and a pending
            // selection abandons — the transcript state itself persists, so
            // re-entry resumes it without a refetch.
            self.suspend_media_widget(cx);
            self.transcript_follow_task.take();
            self.stop_reel_preview(cx);
            self.current_word = None;
            self.selection_mode = false;
            self.selection_anchor = None;
        }
        // The activity poll (T2) is deliberately NOT touched here: job
        // observation is ambient, not bound to any mode — a tab switch can
        // neither start nor stop it (only the observed live set does).
        // Leaving the workspace keeps the inspector's loaded data (the
        // transcript workspace's persistence rule, T1): it belongs to the
        // selected asset, not to the mode, and a re-entry refreshes it.
        self.active_tab = tab;
        self.confirm_delete.disarm();
        self.confirm_delete_transcript.disarm();
        match tab {
            ViewerTab::Library => {
                self.load_gallery(cx);
                // The recordings section (T3): the Library's entry loads the
                // stored-transcript listing beside the gallery page — the
                // section's transcription states and transcript rows read it.
                self.load_transcript_listing(cx);
            }
            ViewerTab::Workspace => {
                if self.transcript.is_some() {
                    self.start_transcript_follow(cx);
                }
                if self.inspector_should_load() {
                    self.load_inspector(cx);
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
        let mut job_submitted = false;
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
                let first_seen = self.processed_tool_results.insert(call.id.clone());
                // First sight of a completed result carrying the server's
                // gallery-change contract makes the cached Library stale.
                if first_seen && tool_result_changed_gallery(output) {
                    gallery_mutated = true;
                }
                // First sight of a completed job_submit starts job
                // observation (T2): a chat-submitted job is ambient
                // process state — its badges and the poll must not wait
                // for the drawer to open (opening the retired Queue tab
                // was the old trigger).
                if first_seen && &*tool == "job_submit" {
                    job_submitted = true;
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
        // A submitted job begins the activity tick: the poll chain, the
        // badges, and the drawer all read the listing it fetches.
        if job_submitted {
            self.load_jobs(cx);
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
    /// reload the active mode's data from its source.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        hkask_viz_core::clear_widget_cache();
        match self.active_tab {
            ViewerTab::Workspace => {
                if let Some(transcript) = self.transcript.as_ref() {
                    // A refresh over the open transcript sub-surface (T3)
                    // reloads the reading surface — the old Transcript
                    // refresh, same behavior in its new home. The thread
                    // re-ingest below rebuilds the surfaced assets and would
                    // drop the recording linkage mid-read; the inspector
                    // keeps its loaded data (the persistence rule).
                    let transcript_id = transcript.transcript_id.clone();
                    self.load_transcript_with(transcript_id, cx);
                    if self.transcript_surface == TranscriptSurface::Events {
                        self.load_visual_events(cx);
                    }
                } else {
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
                        self.status = Some(
                            "No conversation thread to re-ingest — open a thread first.".into(),
                        );
                    }
                }
            }
            ViewerTab::Library => {
                self.reload_gallery(cx);
                self.load_transcript_listing(cx);
            }
        }
        // The activity drawer refreshes with the visible surface when it
        // is open (T2): force-refresh reloads what is on screen.
        if self.activity_open {
            self.load_jobs(cx);
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
            // The inspector keeps its Idle copy after a listing repair
            // moves the selection (the pane's reads follow the user's
            // actions — entry, selection change, toggle — not the
            // background listing merge; reopening the pane reloads).
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

    /// Load the activity layer's job listing (T2): the newest-first page
    /// that feeds the drawer's history rows, the completed-result ingest,
    /// and the poll's page half. The status probes complete the
    /// observation when the page admits deeper jobs.
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
                self.jobs_history_scope = Some(listing.history_scope);
                self.jobs_restart_behavior = Some(listing.restart_behavior);
                self.jobs_request.complete(epoch, Ok(()));
                for (job_id, op, result) in completed_results {
                    if self.processed_job_results.insert(job_id) {
                        self.ingest_tool_result(&result, &op, cx);
                    }
                }
                if listing.has_more {
                    // Deeper pages exist: the newest-first page cannot see
                    // whether a live job hides behind them — only the status
                    // probes can (T2, loop-register L11). The tick settles
                    // when the battery lands.
                    self.observe_live_jobs(cx);
                } else {
                    // The page IS the whole bounded store: its nonterminal
                    // rows are the complete live set.
                    self.live_jobs = self
                        .jobs
                        .iter()
                        .filter(|job| job_is_nonterminal(job))
                        .cloned()
                        .collect();
                    self.settle_activity_tick(cx);
                }
            }
            Err(error) => {
                self.jobs_request.complete(epoch, Err(error));
                self.stop_queue_polling();
            }
        }
        cx.notify();
    }

    /// The poll condition (T2, loop-register L11): poll while any
    /// nonterminal job is observed ANYWHERE in the bounded store. No
    /// page-visibility limit — a live job deeper than the newest-first
    /// page stays observed through the status probes — and no tab
    /// limit: the activity layer is ambient, not a place you visit. A
    /// failed observation battery over a listing with deeper pages
    /// leaves the deep live set unknown — keep observing rather than
    /// silently stop.
    fn should_poll_jobs(&self) -> bool {
        !self.live_jobs.is_empty()
            || (self.jobs_has_more
                && matches!(self.job_observation_request.state, ResourceState::Failed(_)))
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

    /// One observation battery (T2): the three status-filtered `job_list`
    /// probes. The server filters by status BEFORE the newest-first sort
    /// and limit, so each probe sees every job of that status anywhere in
    /// the bounded store; together with the page's own nonterminal rows
    /// they form the complete live set.
    fn observe_live_jobs(&mut self, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .job_observation_request
            .begin(RequestOwner::JobObservation)
        else {
            return; // a battery is already in flight; its apply settles the tick
        };
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_live_observation(
                epoch,
                Err("Tool invoker not wired — panel cannot observe jobs.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let limit = hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT;
        let tasks = NONTERMINAL_JOB_STATUSES.map(|status| {
            invoker.invoke_tool(
                MEDIA_SERVER,
                "job_list",
                serde_json::json!({ "status": status, "limit": limit }),
            )
        });
        cx.spawn(async move |this, cx| {
            let mut observed = Vec::new();
            let mut failure = None;
            for task in tasks {
                let outcome = match task.await {
                    Ok(text) => parse_job_listing(&text),
                    Err(error) => Err(format!("job_list failed: {}", error.message())),
                };
                match outcome {
                    Ok(listing) => observed.extend(listing.jobs),
                    Err(error) => failure = Some(error),
                }
            }
            let result = match failure {
                Some(error) => Err(error),
                None => Ok(observed),
            };
            this.update(cx, |this, cx| {
                this.apply_live_observation(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: The observed live set belongs only to the latest observation battery.
    /// [P1] Motivating: a slow probe battery cannot graft stale rows onto the current live set.
    /// pre: epoch was returned by job_observation_request.begin.
    /// post: stale outcomes are ignored; the owned battery settles the tick exactly once.
    fn apply_live_observation(
        &mut self,
        epoch: u64,
        result: Result<Vec<JobRecord>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.job_observation_request.owns(epoch) {
            return;
        }
        let page_live = self
            .jobs
            .iter()
            .filter(|job| job_is_nonterminal(job))
            .cloned()
            .collect::<Vec<_>>();
        match result {
            Ok(observed) => {
                // Defensive: the probes filter server-side, but the panel's
                // live set is nonterminal BY CONSTRUCTION — a
                // filter-violating response row never enters it (retention
                // would otherwise keep it forever).
                let mut live = page_live;
                live.extend(observed.into_iter().filter(job_is_nonterminal));
                dedupe_jobs_newest_first(&mut live);
                self.live_jobs = live;
                self.job_observation_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                let status = format!("job_list status observation failed: {error}");
                self.job_observation_request.complete(epoch, Err(error));
                self.status = Some(status);
                // A failed battery proves nothing about deeper jobs: keep
                // every previously observed live entry (badges and the
                // poll condition survive a transient probe failure).
                let mut live = page_live;
                let retained = self
                    .live_jobs
                    .iter()
                    .filter(|job| !live.iter().any(|known| known.id == job.id))
                    .cloned()
                    .collect::<Vec<_>>();
                live.extend(retained);
                dedupe_jobs_newest_first(&mut live);
                self.live_jobs = live;
            }
        }
        self.settle_activity_tick(cx);
        cx.notify();
    }

    /// The activity tick's settle step (T2): the live set is final for
    /// this tick — reconcile the R5 transcribe tracking against it, then
    /// arm or stop the 1 Hz poll by the observed-anywhere condition.
    fn settle_activity_tick(&mut self, cx: &mut Context<Self>) {
        // A transcribe job that landed while the Library is showing reloads
        // the stored-transcript listing once so the finished transcript
        // appears under its recording in the recordings section (R5 — the
        // section is the recordings browser's new home, T3). Deep transcribe
        // jobs count too — the probes observe them.
        let still_transcribing = self
            .live_jobs
            .iter()
            .filter(|job| job.op == "transcribe")
            .map(|job| job.id.clone())
            .collect::<HashSet<_>>();
        let landed = self
            .transcribing_ids
            .iter()
            .any(|id| !still_transcribing.contains(id));
        self.transcribing_ids = still_transcribing;
        if landed && self.active_tab == ViewerTab::Library {
            self.load_transcript_listing(cx);
        }
        if self.should_poll_jobs() {
            self.schedule_queue_poll(cx);
        } else {
            self.stop_queue_polling();
        }
    }

    /// Toggle the activity drawer (T2): opening refreshes the job listing
    /// — the drawer is observed on demand (FCP Background Tasks), so it
    /// never shows stale history as its opening state.
    fn toggle_activity(&mut self, cx: &mut Context<Self>) {
        self.activity_open = !self.activity_open;
        if self.activity_open {
            self.load_jobs(cx);
        }
        cx.notify();
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

    /// Load the selected asset's transcript summaries for the inspector's
    /// Transcripts tab via
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
    /// `gallery_annotation_list` — the inspector's Annotations tab
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

    /// The inspector's annotation editor row — the label editor's input,
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
            .id("media-inspector-annotation-input")
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
                        .id("media-inspector-annotation-caret")
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
            .id("media-inspector-annotation-editor")
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
                    .id("media-inspector-annotation-confirm")
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
                    .id("media-inspector-annotation-cancel")
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

    /// Open a transcript in the workspace's sub-surface (T3, S3a): clear
    /// prior state, select the linked recording so the workspace stays with
    /// the asset (the ruling — header, edit toolbar, and inspector pane all
    /// serve the recording the transcript belongs to), switch to the
    /// Workspace mode, and dispatch `educt_get_transcript` for the id — the
    /// entry point of the Library's transcript rows and the inspector's
    /// Transcripts tab.
    fn open_transcript(&mut self, transcript_id: String, cx: &mut Context<Self>) {
        self.transcript = None;
        self.transcript_request.invalidate();
        self.current_word = None;
        self.selection_mode = false;
        self.selection_anchor = None;
        self.selection_popover = None;
        self.context_menu = None;
        self.reset_transcript_search();
        self.highlight_edit = None;
        self.transcript_follow_task.take();
        self.stop_reel_preview(cx);
        // A fresh workspace starts on the word surface with no visual-event
        // or redaction state carried over (S5).
        self.transcript_surface = TranscriptSurface::Words;
        self.visual_events = None;
        self.visual_events_request.invalidate();
        self.event_selection_anchor = None;
        self.event_selection_span = None;
        self.redaction_target = None;
        self.redaction_render = None;
        // A fresh open starts the virtualized list from the top — a
        // previous workspace's reading position must not survive a
        // reopen (S3d).
        self.transcript_list.reset(0);
        self.transcript_list_count = 0;
        self.follow_reveals_in_flight = 0;
        self.follow_viewport = true;
        // The reading surface is the sub-surface's primary area (T3): the
        // inspector pane yields its screen share on open — the header's
        // toggle restores it in one click, serving the same recording
        // (its reads re-fire on reopen). With the pane open in a narrow
        // dock, purpose + header + pane + the pinned 140px player row
        // starve the reading list below usability (the S3d starvation the
        // header-items design exists to avoid).
        self.inspector_open = false;
        // R3 selection coherence (T3): when the Library's listing links the
        // transcript to a recording on the current page, select that
        // recording so the workspace chrome serves the transcript's own
        // asset. An unlinked transcript (or one whose recording is on
        // another page) keeps the current selection — the sub-surface
        // degrades to its own header copy, never hides.
        let recording_index = self
            .transcript_listing
            .as_ref()
            .and_then(|listing| {
                listing
                    .iter()
                    .find(|row| row.id == transcript_id)
                    .and_then(|row| row.gallery_asset_id.clone())
            })
            .and_then(|asset_id| {
                self.assets
                    .iter()
                    .position(|asset| asset.gallery_asset_id.as_deref() == Some(asset_id.as_str()))
            });
        if let Some(ix) = recording_index
            && self.selected != Some(ix)
        {
            self.set_selection(Some(ix), cx);
        }
        self.activate_tab(ViewerTab::Workspace, cx);
        self.load_transcript_with(transcript_id, cx);
    }

    /// Close the transcript sub-surface back to the plain asset workspace
    /// (S6, T3): the transcript state clears, the follow task cancels, and
    /// the operator returns to the selected recording's plain workspace —
    /// the same mode, the surface they left. The recordings browser is the
    /// Library's recordings section now, reachable by its mode at any time,
    /// so the 2026-10-26 reachability concern (the browser disappearing
    /// after the first open) is structural: a mode can never be unreachable.
    /// The shared player stays bound in the registry; reopening any
    /// workspace rebinds through the same path.
    fn close_transcript_workspace(&mut self, cx: &mut Context<Self>) {
        self.transcript = None;
        self.transcript_request.invalidate();
        self.current_word = None;
        self.selection_mode = false;
        self.selection_anchor = None;
        self.selection_popover = None;
        self.context_menu = None;
        self.reset_transcript_search();
        self.highlight_edit = None;
        self.transcript_follow_task.take();
        self.stop_reel_preview(cx);
        self.transcript_surface = TranscriptSurface::Words;
        self.visual_events = None;
        self.visual_events_request.invalidate();
        self.event_selection_anchor = None;
        self.event_selection_span = None;
        self.redaction_target = None;
        self.redaction_render = None;
        self.status = Some("Back to the asset workspace.".into());
        cx.notify();
    }

    // ── Visual-transcript surface (S5: the events reading surface) ───────

    /// Switch the workspace's reading surface (operator refinement
    /// 2026-10-30): word-transcript pane ⇄ visual-transcript event log —
    /// parallel annotations over the same recording, one seek discipline.
    /// The pending surface-local selections and any open label editor belong
    /// to the surface being left. The first switch to Events loads the
    /// event log for the open recording.
    fn toggle_transcript_surface(&mut self, surface: TranscriptSurface, cx: &mut Context<Self>) {
        if self.transcript_surface == surface {
            return;
        }
        self.transcript_surface = surface;
        self.selection_anchor = None;
        self.selection_popover = None;
        self.context_menu = None;
        self.highlight_edit = None;
        self.event_selection_anchor = None;
        self.event_selection_span = None;
        self.status = Some(match surface {
            TranscriptSurface::Words => {
                "Word transcript — click a word to seek; Select text to annotate.".into()
            }
            TranscriptSurface::Events => {
                "Visual transcript — click an event to seek; Select span to annotate.".into()
            }
        });
        if surface == TranscriptSurface::Events && self.visual_events.is_none() {
            self.load_visual_events(cx);
        }
        cx.notify();
    }

    /// Load the open recording's visual transcript via `layers_transcript`
    /// (S1's reading surface): one time-ordered event list across layer
    /// kinds. The last-good log stays visible while a reload is in flight;
    /// a malformed payload rejects visibly and never degrades to an empty
    /// success.
    fn load_visual_events(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let media_path = state.media_path.clone();
        let Some(epoch) = self
            .visual_events_request
            .begin(RequestOwner::VisualEvents(media_path.clone()))
        else {
            return;
        };
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_visual_events_result(
                epoch,
                Err("Tool invoker not wired — panel cannot read the visual transcript.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        // The dispatch pins the maximum page (limit 5000 — the server's
        // cap, clamped 1..=5000): the event log never reads a silent
        // 500-event default prefix. Longer logs still page — the list's
        // bottom truncation note names what the page omits.
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "layers_transcript",
            serde_json::json!({ "media_path": media_path, "limit": 5000 }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_visual_events(&value),
                    None => Err("layers_transcript returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!("layers_transcript failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                this.apply_visual_events_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// The event-log load's result gate: owned epochs only. A committed
    /// reload clears the index-keyed span selection — the operator
    /// re-clicks rather than selecting against shifted indexes.
    fn apply_visual_events_result(
        &mut self,
        epoch: u64,
        result: Result<VisualEventsState, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.visual_events_request.owns(epoch) {
            return;
        }
        match result {
            Ok(state) => {
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                self.visual_events = Some(state);
                self.visual_events_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.visual_events_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// The event-click state machine under the same two-mode interaction
    /// model as the word pane (S4 part-2): seek-only when selection mode is
    /// off; anchor → span when on; a repeated click on the anchor clears
    /// the pending selection; while the label editor is open, clicks are
    /// inert. Pure so the state machine is pin-testable.
    fn handle_event_click(&self, clicked: usize) -> EventClickOutcome {
        if self.highlight_edit.is_some() {
            return EventClickOutcome::EditorOpen;
        }
        if !self.selection_mode {
            return EventClickOutcome::SeekOnly;
        }
        let Some(events) = self
            .visual_events
            .as_ref()
            .map(|state| state.events.as_slice())
        else {
            return EventClickOutcome::SeekOnly;
        };
        match self.event_selection_anchor {
            None => match events.get(clicked) {
                Some(_) => EventClickOutcome::AnchorSet(clicked),
                // A stale anchor index against a reloaded list re-anchors
                // on the clicked row rather than guessing.
                None => EventClickOutcome::SpanRejected,
            },
            Some(anchor) if anchor == clicked => EventClickOutcome::AnchorCleared,
            Some(anchor) => match (events.get(anchor), events.get(clicked)) {
                (Some(first), Some(last)) => {
                    let start = first.start_sec.min(last.start_sec);
                    let end = first.end_or_start().max(last.end_or_start());
                    if end <= start {
                        EventClickOutcome::SpanRejected
                    } else {
                        EventClickOutcome::SpanSelected {
                            start_sec: start,
                            end_sec: end,
                        }
                    }
                }
                _ => EventClickOutcome::SpanRejected,
            },
        }
    }

    /// An event row was clicked in the visual-transcript surface: dispatch
    /// by the selection state machine — seek outside select mode (the
    /// word pane's click-to-seek discipline over the same bound player),
    /// build the span inside it, ignore clicks while the label editor is
    /// open.
    fn event_clicked(&mut self, index: usize, start_sec: f64, cx: &mut Context<Self>) {
        match self.handle_event_click(index) {
            EventClickOutcome::SeekOnly => {
                self.seek_workspace_transcript((start_sec * 1000.0) as u64, cx);
            }
            EventClickOutcome::AnchorSet(event) => {
                self.event_selection_anchor = Some(event);
                self.status = Some(
                    "Click the last event to finish the span, then tag it or redact it.".into(),
                );
                cx.notify();
            }
            EventClickOutcome::AnchorCleared => {
                self.event_selection_anchor = None;
                self.status = Some("Span selection cleared.".into());
                cx.notify();
            }
            EventClickOutcome::EditorOpen => {
                self.status = Some("Label editor open — Enter confirms, Escape cancels.".into());
                cx.notify();
            }
            EventClickOutcome::SpanSelected { start_sec, end_sec } => {
                self.event_selection_anchor = None;
                self.event_selection_span = Some((start_sec, end_sec));
                self.status = Some(
                    "Span selected — Tag span names it; arm a Target and Redact span to redact."
                        .into(),
                );
                cx.notify();
            }
            EventClickOutcome::SpanRejected => {
                self.event_selection_anchor = None;
                self.status =
                    Some("That span is empty — pick two events whose ranges cover time.".into());
                cx.notify();
            }
        }
    }

    /// Arm (or re-arm) the redaction target from one box-carrying event row
    /// — criterion 3's "select target". Only events carrying a normalized
    /// box can arm; the refusal names the requirement.
    fn arm_redaction_target(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self
            .visual_events
            .as_ref()
            .and_then(|state| state.events.get(index))
        else {
            return;
        };
        let Some(box_norm) = row.box_norm else {
            self.status = Some("Only events carrying a box can arm a redaction target.".into());
            cx.notify();
            return;
        };
        let label = row
            .object_label
            .clone()
            .unwrap_or_else(|| row.searchable_text.clone());
        self.redaction_target = Some(RedactionTarget {
            label: label.clone(),
            box_norm,
            source_ref: row.source_ref(),
        });
        self.status = Some(format!(
            "Target armed: {label} — select a span, then Redact span."
        ));
        cx.notify();
    }

    /// Open the label editor over the pending event span (criterion 2's
    /// authoring arm) — the same modal editor as the word pane's create
    /// flow; Enter confirms, Escape cancels.
    fn open_event_span_tag_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start_sec, end_sec)) = self.event_selection_span else {
            return;
        };
        self.open_label_editor(
            HighlightEdit {
                kind: HighlightEditKind::EventTag { start_sec, end_sec },
                buffer: LabelBuffer::default(),
            },
            window,
            cx,
        );
    }

    /// Author a researcher tag over the selected span through
    /// `layers_store` (criterion 2): kind `researcher`, provenance model
    /// `"user"` — a human act, never machine-authored (design §3) — one
    /// item over the span whose searchable text keys the label. The event
    /// log reloads with the stored tag on success.
    fn create_event_tag(
        &mut self,
        start_sec: f64,
        end_sec: f64,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let (media_path, gallery_asset_id) =
            (state.media_path.clone(), state.gallery_asset_id.clone());
        let epoch = match self.begin_edit("layers_store") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Storing researcher tag…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_event_authoring_result(
                epoch,
                Err("Tool invoker not wired — panel cannot author tags.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "layers_store",
            researcher_tag_params(
                &media_path,
                gallery_asset_id.as_deref(),
                start_sec,
                end_sec,
                &label,
            ),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty())
                        .map(|_| String::new())
                        .ok_or_else(|| {
                            "layers_store returned no layer id for the tag.".to_string()
                        }),
                    None => Err("layers_store returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!("layers_store failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                this.apply_event_authoring_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// The tag authoring's result gate: owned epochs only; success clears
    /// the consumed span and reloads the event log so the stored tag lands
    /// in it (a researcher layer over the same recording).
    fn apply_event_authoring_result(
        &mut self,
        epoch: u64,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(_acknowledgement) => {
                self.status =
                    Some("Researcher tag stored — the visual transcript reloads with it.".into());
                self.edit_request.complete(epoch, Ok(()));
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                self.load_visual_events(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("tag authoring failed".to_string()));
            }
        }
        cx.notify();
    }

    /// Author an approved redaction decision over the selected span and
    /// dispatch the render (criterion 3): a two-invoke chain under the
    /// single edit admission, exactly the reel-compose precedent — store
    /// the decision layer via `layers_store` (kind `researcher`, model
    /// `"user"`, one approved item carrying the S3 vocabulary: op redact,
    /// status approved, effect, box target, source-tag lineage), then
    /// render via `layers_render_redactions` with the STORED LAYER ID
    /// (required — the render consumes an approved decision layer, never
    /// free parameters). The armed target names the box; the pending span
    /// names the range.
    fn dispatch_redaction_render(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let Some(target) = self.redaction_target.clone() else {
            self.status = Some("Arm a target first — click Target on an event with a box.".into());
            cx.notify();
            return;
        };
        let Some(span) = self.event_selection_span else {
            self.status = Some("Select a span first — Select span, then click two events.".into());
            cx.notify();
            return;
        };
        let (media_path, gallery_asset_id) =
            (state.media_path.clone(), state.gallery_asset_id.clone());
        let effect = self.redaction_effect;
        let epoch = match self.begin_edit("layers_store") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Authoring redaction decision & rendering…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_redaction_render_result(
                epoch,
                Err("Tool invoker not wired — panel cannot redact.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let store_task = invoker.invoke_tool(
            MEDIA_SERVER,
            "layers_store",
            redaction_decision_params(
                &media_path,
                gallery_asset_id.as_deref(),
                &target,
                span,
                effect,
            ),
        );
        cx.spawn(async move |this, cx| {
            fn finish(
                this: &gpui::WeakEntity<MediaViewer>,
                epoch: u64,
                result: Result<RedactionRenderOutcome, String>,
                cx: &mut gpui::AsyncApp,
            ) {
                this.update(cx, |viewer, cx| {
                    viewer.apply_redaction_render_result(epoch, result, cx)
                })
                .log_err();
            }
            let store_text = match store_task.await {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        Err(format!("layers_store failed: {}", error.message())),
                        cx,
                    );
                    return;
                }
            };
            let Some(layer_id) = hkask_types::tool_response::parse_tool_response(&store_text)
                .and_then(|value| {
                    value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty())
                        .map(str::to_string)
                })
            else {
                finish(
                    &this,
                    epoch,
                    Err("layers_store returned no layer id for the redaction decision.".into()),
                    cx,
                );
                return;
            };
            let render_task = invoker.invoke_tool(
                MEDIA_SERVER,
                "layers_render_redactions",
                serde_json::json!({
                    "media_path": media_path,
                    "layer_id": layer_id,
                }),
            );
            let render_text = match render_task.await {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        Err(format!(
                            "layers_render_redactions failed: {}",
                            error.message()
                        )),
                        cx,
                    );
                    return;
                }
            };
            let outcome = match hkask_types::tool_response::parse_tool_response(&render_text) {
                Some(value) => {
                    parse_redaction_render(&value).map(|summary| RedactionRenderOutcome {
                        // The raw render text rides along: the applied result
                        // routes it through the display-hint ingestion so the
                        // rendered asset lands as a playable panel asset,
                        // exactly as a composed reel does.
                        render_text,
                        summary,
                    })
                }
                None => Err("layers_render_redactions returned no parsable data.".to_string()),
            };
            finish(&this, epoch, outcome, cx);
        })
        .detach();
    }

    /// The redaction chain's result gate: owned epochs only. Success
    /// commits the review/lineage summary, ingests the render's display
    /// hints (landing the rendered asset), surfaces the render counts,
    /// clears the consumed span, and reloads the event log (the decision
    /// layer is a researcher layer over the same recording — its items
    /// appear in the log).
    fn apply_redaction_render_result(
        &mut self,
        epoch: u64,
        result: Result<RedactionRenderOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(outcome) => {
                let RedactionRenderOutcome {
                    render_text,
                    summary,
                } = outcome;
                self.ingest_tool_result(
                    &Value::String(render_text),
                    "layers_render_redactions",
                    cx,
                );
                self.status = Some(format!(
                    "Redaction rendered — {} decision(s), {} segment(s) re-encoded, \
                     {} stream-copied. The visual transcript reloads.",
                    summary.decisions_rendered,
                    summary.reencoded_segments,
                    summary.stream_copied_segments
                ));
                self.redaction_render = Some(summary);
                self.edit_request.complete(epoch, Ok(()));
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                self.load_visual_events(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("redaction render failed".to_string()));
            }
        }
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
                // S6: every annotation CRUD reloads the workspace. The preview
                // task tears down with the reload (its span plan derives from
                // state that just changed), while the composer state
                // RE-MATCHES to highlight identity and survives — order,
                // inclusion, and in-reel strikes keep their deltas; new
                // researcher highlights join included, deleted ones drop.
                self.stop_reel_preview(cx);
                self.rematch_reel_clips(&state);
                // A fresh workspace invalidates any open label editor and
                // selection popover — their annotation ids and word ranges
                // belong to the previous view — and the transcript search
                // (the pending matches are stale in the reloaded view).
                self.highlight_edit = None;
                self.selection_popover = None;
                self.context_menu = None;
                self.reset_transcript_search();
                self.transcript = Some(state);
                self.current_word = None;
                self.transcript_request.complete(epoch, Ok(()));
                // A fresh workspace state re-reads the recording's layers:
                // the visual-event log and its index-keyed selections do not
                // survive it. While the events surface is live, the reload
                // is dispatched immediately so the log never blanks.
                self.visual_events = None;
                self.visual_events_request.invalidate();
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                if self.transcript_surface == TranscriptSurface::Events {
                    self.load_visual_events(cx);
                }
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

    /// Load the stored-transcript listing via `educt_list_transcripts` — the
    /// Library's recordings section reads it (T3): transcription states and
    /// nested transcript rows (operator report 2026-10-26: the section must
    /// list what's available, not dead-end).
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
            Some(anchor) => WordClickOutcome::CompleteRange {
                start_word: anchor.min(clicked),
                end_word: anchor.max(clicked),
            },
        }
    }

    /// A word was clicked in the workspace: dispatch by the selection
    /// state machine — seek outside select mode, build ranges inside it,
    /// ignore clicks while the label editor is open. Range completion
    /// opens the selection popover (whose Highlight action opens the label
    /// editor), gated on index-identical views (D5: the
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
                // A fresh anchor dismisses the tray — the operator is
                // building a new selection, so the pending range's actions
                // will not run. (Seek clicks keep it: seeking is preview,
                // the pending range still stands.)
                self.selection_popover = None;
                self.context_menu = None;
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
            WordClickOutcome::CompleteRange {
                start_word,
                end_word,
            } => {
                self.selection_anchor = None;
                let alignment = self
                    .transcript
                    .as_ref()
                    .map(|state| state.alignment.as_str())
                    .unwrap_or_default();
                if let Err(reason) = selection_tray_mode(alignment) {
                    self.status = Some(reason.into());
                    cx.notify();
                    return;
                }
                self.open_selection_popover(start_word, end_word, window, cx);
            }
        }
    }

    /// The press-and-release drag outcome (S2) — pure so the state machine
    /// is pin-testable, mirroring [`handle_word_selection`]: the alignment
    /// gate applies at the opening site (`word_drag_released`), not here.
    fn handle_word_drag(&self, anchor: usize, release: usize) -> WordDragOutcome {
        if self.highlight_edit.is_some() {
            return WordDragOutcome::Inert;
        }
        if anchor == release {
            return WordDragOutcome::Click;
        }
        WordDragOutcome::CompleteRange {
            start_word: anchor.min(release),
            end_word: anchor.max(release),
        }
    }

    /// A press-and-release completed over the transcript surface (S2):
    /// consume the pending drag, then dispatch by [`WordDragOutcome`] —
    /// same-word releases and editor-open presses are inert (the click
    /// path seeks; the editor is modal to its range), and a cross-word
    /// release opens the selection popover over the ordered union range
    /// under the same index-identical gate as the two-click flow (D5). A
    /// release with no pending anchor or no hovered word is a no-op: the
    /// press was not over a word, or the release landed on a word gap.
    fn word_drag_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (anchor, release) = (self.word_drag_anchor.take(), self.drag_last_word);
        let (Some(anchor), Some(release)) = (anchor, release) else {
            return;
        };
        match self.handle_word_drag(anchor, release) {
            WordDragOutcome::Click => {}
            WordDragOutcome::Inert => {
                self.status = Some("Label editor open — Enter confirms, Escape cancels.".into());
                cx.notify();
            }
            WordDragOutcome::CompleteRange {
                start_word,
                end_word,
            } => {
                let alignment = self
                    .transcript
                    .as_ref()
                    .map(|state| state.alignment.as_str())
                    .unwrap_or_default();
                if let Err(reason) = selection_tray_mode(alignment) {
                    self.status = Some(reason.into());
                    cx.notify();
                    return;
                }
                self.open_selection_popover(start_word, end_word, window, cx);
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

    /// Apply a researcher correction over the selected working range (the
    /// transcript-layer S5): resolve the selection's SOURCE range (identity
    /// on original/aligned views; `educt_locate` basis "source" on a
    /// re_anchored view — the server-side translation through the
    /// re-anchored index map, the D5 v2 consumer), merge the new edit over
    /// the live correction layer's edits (disjoint priors carried verbatim,
    /// overlaps superseded and surfaced — the silent-revert guard), store
    /// the merged layer, chain `educt_realign_transcript` naming it (an
    /// honest no-op when the layer is one-for-one), and reload the
    /// workspace — one edit admission over the whole chain.
    fn dispatch_workspace_correction(
        &mut self,
        start_word: usize,
        end_word: usize,
        replacement: String,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let transcript_id = state.transcript_id.clone();
        let alignment = state.alignment.clone();
        let prior_edits = state.correction_edits.clone();
        // The selection's rendered form — the locate contract quotes the
        // rendered words exactly, and both sides join word text with single
        // spaces (`rendered_transcript`).
        let quote = state
            .words
            .as_deref()
            .and_then(|words| words.get(start_word..=end_word))
            .map(|slice| {
                slice
                    .iter()
                    .map(|word| word.word.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        let epoch = match self.begin_edit("educt_store_layer") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Applying correction…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_correction_result(
                epoch,
                transcript_id,
                Err("Tool invoker not wired — panel cannot correct transcripts.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let locate_needed = alignment == "re_anchored";
        let locate_params =
            correction_locate_params(&transcript_id, quote.as_deref().unwrap_or_default());
        cx.spawn(async move |this, cx| {
            fn finish(
                this: &gpui::WeakEntity<MediaViewer>,
                epoch: u64,
                transcript_id: &str,
                result: Result<usize, String>,
                cx: &mut gpui::AsyncApp,
            ) {
                this.update(cx, |viewer, cx| {
                    viewer.apply_workspace_correction_result(
                        epoch,
                        transcript_id.to_string(),
                        result,
                        cx,
                    )
                })
                .log_err();
            }
            // 1. The source range: identity on index-identical views; the
            //    server's own translation on re_anchored views.
            let (source_start, source_end) = if locate_needed {
                let locate_text = match invoker
                    .invoke_tool(MEDIA_SERVER, "educt_locate", locate_params)
                    .await
                {
                    Ok(text) => text,
                    Err(error) => {
                        finish(
                            &this,
                            epoch,
                            &transcript_id,
                            Err(format!("educt_locate failed: {}", error.message())),
                            cx,
                        );
                        return;
                    }
                };
                match parse_correction_source_range(&locate_text, start_word, end_word) {
                    Ok(range) => range,
                    Err(error) => {
                        finish(&this, epoch, &transcript_id, Err(error), cx);
                        return;
                    }
                }
            } else {
                (start_word, end_word)
            };
            // 2. Merge over the live layer's edits — the server's own
            //    supersession semantics, never a fork.
            let new_edit = PaneCorrectionEdit {
                start_word: source_start,
                end_word: source_end,
                replacement,
                reason: CORRECTION_REASON.to_string(),
            };
            let prior: Vec<hkask_mcp_media::transcript_layers::CorrectionEdit> =
                prior_edits.iter().map(server_correction_edit).collect();
            let (merged, superseded) = hkask_mcp_media::transcript_retranscribe::merge_over_prior(
                &prior,
                server_correction_edit(&new_edit),
            );
            let merged_pane: Vec<PaneCorrectionEdit> =
                merged.iter().map(pane_correction_edit).collect();
            let superseded = superseded.len();
            // 3. Store the merged correction layer.
            let store_params = match correction_store_params(
                &transcript_id,
                &merged_pane,
                &hkask_types::time::now_rfc3339(),
            ) {
                Ok(params) => params,
                Err(error) => {
                    finish(&this, epoch, &transcript_id, Err(error), cx);
                    return;
                }
            };
            let store_text = match invoker
                .invoke_tool(MEDIA_SERVER, "educt_store_layer", store_params)
                .await
            {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        &transcript_id,
                        Err(format!("educt_store_layer failed: {}", error.message())),
                        cx,
                    );
                    return;
                }
            };
            let stored_id = hkask_types::tool_response::parse_tool_response(&store_text)
                .and_then(|value| {
                    value
                        .pointer("/stored/id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .ok_or_else(|| "educt_store_layer returned no stored layer id.".to_string());
            let stored_id = match stored_id {
                Ok(id) => id,
                Err(error) => {
                    finish(&this, epoch, &transcript_id, Err(error), cx);
                    return;
                }
            };
            // 4. Realign names the stored layer — an honest no-op when the
            //    merged layer is one-for-one; the re-anchoring decision
            //    otherwise (the reload reads the true state).
            let realign_params = correction_realign_params(&transcript_id, &stored_id);
            if let Err(error) = invoker
                .invoke_tool(MEDIA_SERVER, "educt_realign_transcript", realign_params)
                .await
            {
                finish(
                    &this,
                    epoch,
                    &transcript_id,
                    Err(format!(
                        "educt_realign_transcript failed: {}",
                        error.message()
                    )),
                    cx,
                );
                return;
            }
            finish(&this, epoch, &transcript_id, Ok(superseded), cx);
        })
        .detach();
    }

    /// The correction chain's completion (S5): commit once and reload the
    /// workspace so the corrected working view lands; failures surface
    /// visibly and clear progress. The superseded count rides the success
    /// status — overlapping prior corrections are superseded visibly,
    /// never silently (the merge's contract).
    fn apply_workspace_correction_result(
        &mut self,
        epoch: u64,
        transcript_id: String,
        result: Result<usize, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(superseded) => {
                self.status = Some(if superseded == 0 {
                    "Correction applied — the transcript reloads with it.".to_string()
                } else {
                    format!(
                        "Correction applied — superseded {superseded} overlapping prior \
                         correction(s) — the transcript reloads with it."
                    )
                });
                self.edit_request.complete(epoch, Ok(()));
                self.load_transcript_with(transcript_id, cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("transcript correction failed".to_string()));
            }
        }
        cx.notify();
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
        let selection = self.reel_clips.clone();
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
        self.status = Some(match &edit.kind {
            HighlightEditKind::Create { .. } => {
                "Name this highlight, or choose Cut selection. Enter confirms; Escape cancels."
                    .into()
            }
            HighlightEditKind::Correction { .. } => {
                "Type the corrected text for the selection — Enter applies it as a \
                 correction layer; Escape cancels."
                    .into()
            }
            HighlightEditKind::EventTag { .. } => {
                "Name this event tag. Enter confirms; Escape cancels.".into()
            }
            HighlightEditKind::Rename { .. } => {
                "Rename the highlight — Enter saves; Escape cancels.".into()
            }
        });
        self.highlight_edit = Some(edit);
        // The editor supersedes any open selection popover (S2): opening it
        // from the tray's Highlight action or a row's Rename never leaves a
        // stale tray rendered beside the modal editor.
        self.selection_popover = None;
        self.context_menu = None;
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
            HighlightEditKind::Correction {
                start_word,
                end_word,
            } => {
                // A correction replaces text; an empty or blank
                // replacement is a deletion, which belongs to Cut — the
                // panel-side gate the server's correction-pass guard
                // mirrors (store validation checks bounds and
                // disjointness only).
                let replacement = edit.buffer.text();
                if replacement.trim().is_empty() {
                    self.highlight_edit = Some(edit);
                    self.status = Some(
                        "A correction needs replacement text — deletions belong to Cut; \
                         Escape cancels."
                            .into(),
                    );
                    cx.notify();
                    return;
                }
                self.dispatch_workspace_correction(
                    *start_word,
                    *end_word,
                    replacement.trim().to_string(),
                    cx,
                );
            }
            HighlightEditKind::EventTag { start_sec, end_sec } => {
                let label = edit.buffer.label();
                if label.is_empty() {
                    self.highlight_edit = Some(edit);
                    self.status = Some("A tag needs a nonblank label — Escape cancels.".into());
                    cx.notify();
                    return;
                }
                self.create_event_tag(*start_sec, *end_sec, label, cx);
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

    // ── Selection popover (S2: the completed range's action tray) ─────────

    /// Open the selection popover (S2) over the completed range — the
    /// selection's action tray (Highlight / Cut / Copy time / Include in
    /// reel), pinned above the list like the label editor. The tray takes
    /// focus so its keys work without clicking first: Enter highlights,
    /// Delete cuts, Escape dismisses. Not modal — a newer completed range
    /// replaces this one, a fresh anchor dismisses it, and seek clicks
    /// leave it be (seeking is preview, the pending range stands).
    fn open_selection_popover(
        &mut self,
        start_word: usize,
        end_word: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .selection_popover_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The tray owns the completed range; no anchor lingers.
        self.selection_anchor = None;
        self.selection_popover = Some((start_word, end_word));
        // The tray's hint names its mode's actions (S5): the full tray on
        // index-identical views, the correction tray on re_anchored views.
        let mode = self
            .transcript
            .as_ref()
            .and_then(|state| selection_tray_mode(&state.alignment).ok());
        self.status = Some(match mode {
            Some(SelectionTrayMode::CorrectOnly) => {
                "Selection ready — this corrected view supports Suggest correction and \
                 Copy time. Enter corrects; Escape dismisses."
                    .into()
            }
            _ => "Selection ready — Highlight, Cut, Copy time, Suggest correction, or \
                  Include in reel. Delete cuts; Escape dismisses."
                .into(),
        });
        focus.focus(window, cx);
        cx.notify();
    }

    /// Dismiss the selection popover without acting (the tray's Escape).
    fn dismiss_selection_popover(&mut self, cx: &mut Context<Self>) {
        if self.selection_popover.take().is_some() {
            self.status = Some("Selection dismissed.".into());
            cx.notify();
        }
    }

    /// S8: the right-click context menu's target range — the words the
    /// menu's actions apply to. An open selection popover owns its range;
    /// otherwise a pending two-click anchor completes to the right-click
    /// target word, and a bare right click over a word selects that word
    /// alone. A right click outside the words (no target, no anchor) has
    /// no range and opens nothing.
    fn context_menu_range(&self) -> Option<(usize, usize)> {
        if let Some((start_word, end_word)) = self.selection_popover {
            return Some((start_word, end_word));
        }
        match (self.selection_anchor, self.context_menu_fallback) {
            (Some(anchor), Some(word)) => Some((anchor.min(word), anchor.max(word))),
            (Some(anchor), None) => Some((anchor, anchor)),
            (_, Some(word)) => Some((word, word)),
            _ => None,
        }
    }

    /// S8: a right mouse-up over the word surface opens the context menu
    /// for the range under the pointer — the pointer alternative to the
    /// select-then-tray gesture. The menu's actions reuse the tray's
    /// dispatchers (Highlight opens the label editor, Strike out
    /// dispatches the strike, Copy time writes the span, Include in reel
    /// flips or creates membership); the range is captured when the menu
    /// opens, so the actions never read a working selection that has
    /// since moved.
    fn open_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The menu is a word-surface affordance — the events surface has
        // its own span authoring (tag/redact), not word highlights.
        if self.transcript_surface != TranscriptSurface::Words {
            self.status =
                Some("Right-click the word transcript to choose Highlight or Strike out.".into());
            cx.notify();
            return;
        }
        let Some((start_word, end_word)) = self.context_menu_range() else {
            self.status = Some("Right-click a word to choose Highlight or Strike out.".into());
            cx.notify();
            return;
        };
        // A right click completes a pending two-click selection — the
        // menu owns the completed range.
        self.selection_anchor = None;
        self.context_menu = Some((start_word, end_word));
        let focus = self
            .context_menu_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        focus.focus(window, cx);
        self.status = Some(format!(
            "Context menu — words {start_word}–{end_word}: Highlight or Strike out."
        ));
        cx.notify();
    }

    /// Dismiss the right-click context menu without acting (its Escape).
    fn dismiss_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            self.status = Some("Context menu dismissed.".into());
            cx.notify();
        }
    }

    /// S8: consume the pending selection state the context menu acts on —
    /// an open tray or a two-click anchor — so the action's range is not
    /// left dangling after the menu fires.
    fn clear_pending_selection(&mut self) {
        self.selection_popover = None;
        self.selection_anchor = None;
    }

    /// Whether the open view serves the correction tray (S5) — the
    /// index-identical-only actions refuse on such views.
    fn selection_tray_is_correct_only(&self) -> bool {
        self.transcript.as_ref().is_some_and(|state| {
            selection_tray_mode(&state.alignment) == Ok(SelectionTrayMode::CorrectOnly)
        })
    }

    /// Route the selection popover's keys by the view's tray mode: Enter
    /// opens the low-effort editor (the label editor on the full tray,
    /// the correction editor on the correction tray), Delete cuts on the
    /// full tray only, Escape dismisses. Bound on the tray itself, so
    /// the keys only fire while it holds focus — the label input's
    /// Delete stays text editing (forward-delete), which is exactly why
    /// this binding is not on the input.
    fn handle_selection_popover_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = self
            .transcript
            .as_ref()
            .and_then(|state| selection_tray_mode(&state.alignment).ok());
        match keystroke.key.as_str() {
            "escape" => self.dismiss_selection_popover(cx),
            "enter" => match mode {
                Some(SelectionTrayMode::CorrectOnly) => self.popover_correct(window, cx),
                _ => self.popover_highlight(window, cx),
            },
            // Delete cuts on the full tray only — the correction tray has
            // nothing to cut (cut needs an index-identical view).
            "delete" | "backspace" => {
                if mode != Some(SelectionTrayMode::CorrectOnly) {
                    self.popover_cut(cx);
                }
            }
            _ => {}
        }
    }

    /// The tray's Suggest correction action (S5): close the tray and open
    /// the correction editor — the label-editor pattern — over the same
    /// range, the replacement step. Available on every timed view:
    /// original/aligned selections are source-identical, re_anchored
    /// selections translate at dispatch through educt_locate.
    fn popover_correct(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.open_label_editor(
            HighlightEdit {
                kind: HighlightEditKind::Correction {
                    start_word,
                    end_word,
                },
                buffer: LabelBuffer::default(),
            },
            window,
            cx,
        );
    }

    /// The tray's Highlight action: close the tray and open the label
    /// editor over the same range — the naming step. Index-identical
    /// views only: highlight anchors source words through the annotation
    /// tools without a translation, so a re_anchored view refuses (a
    /// stale tray or a mis-routed key must never anchor working
    /// indices — the 2026-10-25 overshoot trap).
    fn popover_highlight(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.open_highlight_editor(start_word, end_word, window, cx);
    }

    /// Open the label editor over a range to create a highlight — the
    /// naming step, shared by the selection popover's Highlight arm and
    /// the right-click context menu's Highlight entry. Index-identical
    /// views only (the popover_highlight guard's reason).
    fn open_highlight_editor(
        &mut self,
        start_word: usize,
        end_word: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection_tray_is_correct_only() {
            self.status = Some(
                "Highlight needs an index-identical view — Suggest correction instead.".into(),
            );
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

    /// The tray's Cut action (the Delete key shares it): strike the range —
    /// a cut carries no label, so the tray ends here. Index-identical
    /// views only (the popover_highlight guard's reason).
    fn popover_cut(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        if self.selection_tray_is_correct_only() {
            self.status =
                Some("Cut needs an index-identical view — Suggest correction instead.".into());
            cx.notify();
            return;
        }
        self.strike_selected_range(start_word, end_word, cx);
    }

    /// The tray's Copy time action: close the tray and copy the range's
    /// time span — the shared range step, also the context menu's Copy
    /// time entry.
    fn popover_copy_time(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.copy_time_range(start_word, end_word, cx);
    }

    /// Copy a word range's time span to the clipboard in the panel's
    /// marks vocabulary ("in 2.0s out 5.9s" — the same readout the trim
    /// marks label uses, so one vocabulary serves the panel). A view
    /// without word timings degrades honestly instead of copying a
    /// made-up span.
    fn copy_time_range(&mut self, start_word: usize, end_word: usize, cx: &mut Context<Self>) {
        let span = self.transcript.as_ref().and_then(|state| {
            let words = state.words.as_deref()?;
            let first = words.get(start_word)?;
            let last = words.get(end_word)?;
            Some((first.start_ms, last.end_ms))
        });
        let Some((start_ms, end_ms)) = span else {
            self.status = Some("No word timings on this view — nothing to copy.".into());
            cx.notify();
            return;
        };
        let copied = format!(
            "in {:.1}s out {:.1}s",
            start_ms as f64 / 1000.0,
            end_ms as f64 / 1000.0
        );
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
        self.status = Some(format!("Copied {copied}."));
        cx.notify();
    }

    /// S6: the composer's re-match on every workspace load — the reload
    /// persistence mechanism. Existing clips keep their composer deltas
    /// (order, inclusion, in-reel strikes) keyed to highlight identity
    /// (annotation id + source range), with labels refreshed from the
    /// reloaded highlights; new researcher highlights join included at the
    /// tail (the fresh-workspace semantics); deleted highlights drop their
    /// clips; model-pass rows never compose.
    fn rematch_reel_clips(&mut self, state: &TranscriptPaneState) {
        let mut clips: Vec<ReelClip> = self
            .reel_clips
            .drain(..)
            .filter(|clip| {
                state.highlights.iter().any(|highlight| {
                    highlight.researcher_supplied
                        && Self::reel_clip_identity_matches(clip, highlight)
                })
            })
            .collect();
        for clip in &mut clips {
            if let Some(highlight) = state.highlights.iter().find(|highlight| {
                highlight.researcher_supplied && Self::reel_clip_identity_matches(clip, highlight)
            }) {
                clip.label = highlight.label.clone();
            }
        }
        for highlight in state
            .highlights
            .iter()
            .filter(|highlight| highlight.researcher_supplied)
        {
            let exists = clips
                .iter()
                .any(|clip| Self::reel_clip_identity_matches(clip, highlight));
            if !exists {
                clips.push(ReelClip {
                    annotation_id: highlight.annotation_id.clone(),
                    start_word: highlight.start_word,
                    end_word: highlight.end_word,
                    label: highlight.label.clone(),
                    include: true,
                    strikes: Vec::new(),
                });
            }
        }
        self.reel_clips = clips;
    }

    /// The composer's identity key: a highlight is the same clip across
    /// reloads when its annotation id AND source range match (a rename
    /// changes neither).
    fn reel_clip_identity_matches(clip: &ReelClip, highlight: &PaneHighlight) -> bool {
        clip.annotation_id == highlight.annotation_id
            && clip.start_word == highlight.start_word
            && clip.end_word == highlight.end_word
    }

    /// S6: move a clip within the composed order — the reel row's ↑/↓.
    /// Bounds-guarded: a no-op at the list's edges, never a panic.
    fn move_reel_clip(&mut self, clip_index: usize, up: bool, cx: &mut Context<Self>) {
        if clip_index >= self.reel_clips.len() {
            return;
        }
        let target = if up {
            clip_index.checked_sub(1)
        } else {
            Some(clip_index + 1)
        };
        if let Some(target) = target
            && target < self.reel_clips.len()
        {
            self.reel_clips.swap(clip_index, target);
            cx.notify();
        }
    }

    /// S6: flip one clip's inclusion — the highlights row's toggle and the
    /// popover's Include arm both write here; the clip's include flag is
    /// the ONE membership source of truth (no set beside the list). Keyed
    /// to highlight identity, so the flip survives reloads and is safe on
    /// re_anchored views too (it flips stored source-anchored membership,
    /// never a working selection).
    fn toggle_reel_clip_include(
        &mut self,
        annotation_id: &str,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(clip) = self.reel_clips.iter_mut().find(|clip| {
            clip.annotation_id == annotation_id
                && clip.start_word == start_word
                && clip.end_word == end_word
        }) {
            clip.include = !clip.include;
            cx.notify();
        }
    }

    /// S6: undo one in-reel strike on a clip (the reel row's per-strike
    /// Restore). Bounds-guarded.
    fn remove_reel_clip_strike(
        &mut self,
        clip_index: usize,
        strike_index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(clip) = self.reel_clips.get_mut(clip_index)
            && strike_index < clip.strikes.len()
        {
            clip.strikes.remove(strike_index);
            cx.notify();
        }
    }

    /// S6: the selection popover's Include-in-reel arm. Over a composed
    /// clip's range it flips that clip's include flag (the row toggle's
    /// path — one source of truth, no dispatch); over a fresh range it
    /// dispatches the existing create path with the default label — the
    /// reload re-matches every researcher highlight into the composer, so
    /// the one click carries the range into the reel (a clip with no
    /// highlight behind it would have no reload identity to survive on).
    fn popover_toggle_reel_include(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.include_reel_range(start_word, end_word, cx);
    }

    /// Flip a range's reel membership — the shared range step, also the
    /// context menu's Include in reel entry. Over a composed clip's range
    /// it flips that clip's include flag (the row toggle's path — one
    /// source of truth, no dispatch); over a fresh range it dispatches
    /// the existing create path with the default label — the reload
    /// re-matches every researcher highlight into the composer, so the
    /// one click carries the range into the reel (a clip with no
    /// highlight behind it would have no reload identity to survive on).
    fn include_reel_range(&mut self, start_word: usize, end_word: usize, cx: &mut Context<Self>) {
        if self.selection_tray_is_correct_only() {
            self.status = Some(
                "Include in reel needs an index-identical view — Suggest correction \n                 instead."
                    .into(),
            );
            cx.notify();
            return;
        }
        match self
            .reel_clips
            .iter()
            .find(|clip| clip.start_word == start_word && clip.end_word == end_word)
        {
            Some(clip) => {
                let now_included = !clip.include;
                let (annotation_id, clip_start, clip_end) =
                    (clip.annotation_id.clone(), clip.start_word, clip.end_word);
                self.toggle_reel_clip_include(&annotation_id, clip_start, clip_end, cx);
                self.status = Some(
                    if now_included {
                        "Added to the reel."
                    } else {
                        "Removed from the reel."
                    }
                    .into(),
                );
                cx.notify();
            }
            None => {
                self.create_workspace_highlight(start_word, end_word, resolve_create_label(""), cx);
            }
        }
    }

    /// S6: the selection popover's in-reel strike arm — a sub-range cut
    /// inside ONE clip, recorded on that clip (the compose bakes it into
    /// the clip's Keep op; never a whole-recording strike). The completed
    /// range must lie within exactly one INCLUDED clip; anything else
    /// refuses honestly. Index-identical views only — the tray gate (the
    /// strike anchors working-view indices, which only match source tools
    /// on index-identical views).
    fn popover_strike_in_reel(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        if self.selection_tray_is_correct_only() {
            self.status = Some(
                "Strike in reel clip needs an index-identical view — Suggest \n                 correction instead."
                    .into(),
            );
            cx.notify();
            return;
        }
        let inside: Vec<usize> = self
            .reel_clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                clip.include && clip.start_word <= start_word && end_word <= clip.end_word
            })
            .map(|(index, _)| index)
            .collect();
        match inside.as_slice() {
            [clip_index] => {
                self.reel_clips[*clip_index]
                    .strikes
                    .push((start_word, end_word));
                self.status = Some("Struck the sub-range inside the reel clip.".into());
                cx.notify();
            }
            _ => {
                self.status = Some(
                    "Strike in reel clip needs a range inside exactly one included clip.".into(),
                );
                cx.notify();
            }
        }
    }

    // ── In-transcript search (S3) ───────────────────────────────────────

    /// S3: schedule the debounced search rescan — one burst of edits
    /// rescans once, 150 ms after the last keystroke. Taking the old task
    /// cancels any pending rescan (the follow-task lifecycle pattern); the
    /// timer is GPUI-native (never smol::Timer — .rules).
    fn schedule_transcript_search_rescan(&mut self, cx: &mut Context<Self>) {
        self.transcript_search_task.take();
        self.transcript_search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(TRANSCRIPT_SEARCH_DEBOUNCE)
                .await;
            this.update(cx, |viewer, cx| {
                viewer.rescan_transcript_search(cx);
            })
            .ok();
        }));
    }

    /// S3: recompute the search matches from the typed query over the
    /// loaded transcript — the debounced rescan's landing. A fresh match
    /// list starts the cycle on its first match and lands it in view (the
    /// find-bar precedent: the stabilized query shows where the results
    /// are); zero matches clear the marks and surface beside the input,
    /// never silently. The query is trimmed and matched case-insensitively
    /// for ASCII, exactly for non-ASCII (the server's lexical rule).
    fn rescan_transcript_search(&mut self, cx: &mut Context<Self>) {
        self.transcript_search_task.take();
        let words = self
            .transcript
            .as_ref()
            .and_then(|state| state.words.as_deref());
        let query = self.transcript_search.query.text();
        self.transcript_search.matches = transcript_search_matches(words.unwrap_or(&[]), &query);
        self.transcript_search.current = 0;
        if !self.transcript_search.matches.is_empty() {
            self.reveal_transcript_search_match();
        }
        cx.notify();
    }

    /// S3: scroll the current match's block into view — Enter/Shift-Enter's
    /// jump and the rescan's first-match landing, using the follow reveal's
    /// item-anchored idiom (blocks deep in a long recording are unmeasured
    /// until rendered, so the reveal anchors by item index, not pixels). The
    /// reveal accounting keeps the follow tick from misreading this scroll
    /// as manual.
    fn reveal_transcript_search_match(&mut self) {
        let Some(&(start_word, _)) = self
            .transcript_search
            .matches
            .get(self.transcript_search.current)
        else {
            return;
        };
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let Some(block_ix) = block_index_for_word(state, start_word) else {
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
            self.transcript_list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
            self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
        }
    }

    /// Route the search input's keystrokes (S3): Enter cycles to the next
    /// match and lands it (Shift-Enter cycles back; both wrap), Escape
    /// clears the query and its marks, editing keys go to the buffer and
    /// schedule the debounced rescan. Bound on the input element itself, so
    /// the keys only fire while it holds focus.
    fn handle_transcript_search_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let count = self.transcript_search.matches.len();
        match keystroke.key.as_str() {
            "enter" if count > 0 => {
                self.transcript_search.current = if keystroke.modifiers.shift {
                    (self.transcript_search.current + count - 1) % count
                } else {
                    (self.transcript_search.current + 1) % count
                };
                self.reveal_transcript_search_match();
                cx.notify();
            }
            "escape" => {
                // Escape clears the query — the search leaves no marks
                // behind, immediately (no debounce on the way out).
                self.transcript_search = TranscriptSearch::default();
                self.transcript_search_task.take();
                cx.notify();
            }
            _ => {
                if self.transcript_search.query.apply_keystroke(keystroke) {
                    self.schedule_transcript_search_rescan(cx);
                    cx.notify();
                }
            }
        }
    }

    /// Paste clipboard text into the search query (S3) — the label
    /// editor's paste fix (2026-10-26): modifier chords never type, so a
    /// phrase could not be pasted otherwise. Control characters are
    /// dropped — the input is single-line. Inert with no clipboard text.
    fn paste_into_transcript_search(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.transcript_search.query.insert_str(&text);
        self.schedule_transcript_search_rescan(cx);
        cx.notify();
    }

    /// S3: clear the transcript search with the surface — the query, the
    /// matches, the pending rescan, and the follow custody all belong to the
    /// workspace they were opened on (a reload's fresh working view makes
    /// the pending matches stale).
    fn reset_transcript_search(&mut self) {
        self.transcript_search = TranscriptSearch::default();
        self.transcript_search_task.take();
        self.transcript_search_suspended_follow = false;
    }

    /// S3: register the search input's focus/blur listeners — focusing the
    /// search suspends viewport tracking (typing and the auto-landed matches
    /// must not fight the follow-along — the manual-scroll precedent), and
    /// blur restores tracking only when the search took it (a manually
    /// suspended follow stays suspended). Idempotent; called from the
    /// workspace render, so the listeners are live before any focus path
    /// can reach the input (click or keyboard). Render-time registration
    /// works (the deferred activation rides the normal effect cycle) —
    /// verified by bisect 2026-11-05 after an earlier false claim that it
    /// never activates; do not re-assert that.
    fn ensure_transcript_search_focus_listeners(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.transcript_search_focus_subscriptions.is_empty() {
            return;
        }
        let Some(focus) = self.transcript_search_focus.clone() else {
            return;
        };
        let on_focus = cx.on_focus(&focus, window, |viewer, _window, cx| {
            if viewer.follow_viewport {
                viewer.follow_viewport = false;
                viewer.transcript_search_suspended_follow = true;
                cx.notify();
            }
        });
        let on_blur = cx.on_blur(&focus, window, |viewer, _window, cx| {
            if viewer.transcript_search_suspended_follow {
                viewer.transcript_search_suspended_follow = false;
                viewer.follow_viewport = true;
                cx.notify();
            }
        });
        self.transcript_search_focus_subscriptions = vec![on_focus, on_blur];
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
            HighlightEditKind::Correction { .. } => {
                ("Correct selection".to_string(), "Apply", "Corrected text")
            }
            HighlightEditKind::EventTag { .. } => {
                ("Name this event tag".to_string(), "Create tag", "Tag name")
            }
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
            HighlightEditKind::Correction { .. }
            | HighlightEditKind::EventTag { .. }
            | HighlightEditKind::Rename { .. } => editor,
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

    /// The selection popover (S2): the completed range's action tray —
    /// Highlight / Cut / Copy time / Suggest correction / Include in reel
    /// on the full tray; Suggest correction + Copy time on the correction
    /// tray (re_anchored views, S5) — pinned above the list (the
    /// label-editor pin pattern) so the actions never virtualize away.
    /// The tray holds focus while open; its keys bind here by the same
    /// mode (Enter opens the mode's editor, Delete cuts on the full tray
    /// only, Escape dismisses), never on the label input (Delete there is
    /// text editing). An associated fn so the caller can hold the
    /// workspace state's immutable borrow.
    fn render_selection_popover(
        excerpt: &str,
        include_label: &str,
        strike_in_clip: bool,
        correct_only: bool,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("media-transcript-selection-popover")
            .track_focus(focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_selection_popover_key(&event.keystroke, window, cx);
                cx.stop_propagation();
            }))
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .debug_selector(|| "media-transcript-selection-popover".into())
            .child(
                Label::new(SharedString::from(format!("Selection — {excerpt}")))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .when(!correct_only, |tray| {
                tray.child(
                    div()
                        .id("media-transcript-popover-highlight")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("Highlight"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.popover_highlight(window, cx);
                        })),
                )
                .child(
                    div()
                        .id("media-transcript-popover-cut")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("Cut"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Error),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.popover_cut(cx);
                        })),
                )
                .when(strike_in_clip, |tray| {
                    tray.child(
                        div()
                            .id("media-transcript-popover-strike-in-reel")
                            .cursor_pointer()
                            .hover(|el| el.bg(cx.theme().colors().element_hover))
                            .child(
                                Label::new(SharedString::from("Strike in reel clip"))
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Error),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.popover_strike_in_reel(cx);
                            })),
                    )
                })
            })
            .child(
                div()
                    .id("media-transcript-popover-copy-time")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Copy time"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.popover_copy_time(cx);
                    })),
            )
            .child(
                div()
                    .id("media-transcript-popover-correct")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Suggest correction"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.popover_correct(window, cx);
                    })),
            )
            .when(!correct_only, |tray| {
                tray.child(
                    div()
                        .id("media-transcript-popover-include")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from(include_label))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.popover_toggle_reel_include(cx);
                        })),
                )
            })
            .into_any_element()
    }

    /// S8: the right-click context menu — the pointer alternative to the
    /// select-then-tray gesture. The entries reuse the tray's dispatchers
    /// over the captured range: Highlight opens the label editor, Strike
    /// out dispatches the strike, Copy time writes the span, Include in
    /// reel flips or creates membership. Escape dismisses.
    fn render_context_menu(
        excerpt: &str,
        include_label: &str,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("media-transcript-context-menu")
            .track_focus(focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    this.dismiss_context_menu(cx);
                }
                cx.stop_propagation();
            }))
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .debug_selector(|| "media-transcript-context-menu".into())
            .child(
                Label::new(SharedString::from(format!("Right-click — {excerpt}")))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-highlight")
                    .debug_selector(|| "media-transcript-context-menu-highlight".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Highlight"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.open_highlight_editor(start_word, end_word, window, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-strike")
                    .debug_selector(|| "media-transcript-context-menu-strike".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Strike out"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Error),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.strike_selected_range(start_word, end_word, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-copy-time")
                    .debug_selector(|| "media-transcript-context-menu-copy-time".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Copy time"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.copy_time_range(start_word, end_word, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-include")
                    .debug_selector(|| "media-transcript-context-menu-include".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(include_label))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.include_reel_range(start_word, end_word, cx);
                        }
                    })),
            )
            .into_any_element()
    }

    // ── S6 reel preview (the auto-seek chain on the shared player) ─────────

    /// S6: toggle the composed reel's preview — plays the composed spans on
    /// the shared player without rendering, seeking span-to-span as playback
    /// crosses each span's end. Player-local (no admission — the seek paths'
    /// class); the position source and the seek target are the S4
    /// rate-contract pair.
    fn toggle_reel_preview(&mut self, cx: &mut Context<Self>) {
        if self.reel_preview.is_some() {
            self.stop_reel_preview(cx);
            self.status = Some("Reel preview stopped.".into());
            cx.notify();
            return;
        }
        self.start_reel_preview(cx);
    }

    /// S6: stop the preview and cancel its task — the one teardown entry
    /// every site shares (surface switch, open, close, reload, the toggle's
    /// off arm) and idempotent.
    fn stop_reel_preview(&mut self, cx: &mut Context<Self>) {
        self.reel_preview_task.take();
        self.reel_preview = None;
        cx.notify();
    }

    /// S6: arm and start the preview — build the span plan (the render
    /// path's own clip plan over the SOURCE bundle words), prime the first
    /// span with an immediate seek, then poll the player on the follow
    /// loop's task pattern: single-instance guard, GPUI-native timer,
    /// self-cancel when the substrate leaves, poll only while playing.
    fn start_reel_preview(&mut self, cx: &mut Context<Self>) {
        if self.reel_preview.is_some() {
            return;
        }
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let spans = match reel_preview_plan(state, &self.reel_clips) {
            Ok(spans) => spans,
            Err(reason) => {
                self.status = Some(reason.into());
                cx.notify();
                return;
            }
        };
        let first_start = spans[0].0;
        self.reel_preview = Some(ReelPreview { spans, index: 0 });
        self.seek_workspace_transcript(first_start, cx);
        self.status = Some("Previewing the composed reel — press play to hear it.".into());
        cx.notify();
        let entity = cx.entity().downgrade();
        self.reel_preview_task = Some(cx.spawn(async move |_this, cx| {
            loop {
                cx.background_executor().timer(FOLLOW_POLL_INTERVAL).await;
                let keep_going = entity
                    .update(cx, |viewer, cx| viewer.reel_preview_tick(cx))
                    .unwrap_or_default();
                if !keep_going {
                    return;
                }
            }
        }));
    }

    /// S6: one preview poll — the task body, directly callable (the
    /// teardown pins drive it). Returns false (state cleared) when the
    /// substrate is gone — the surface left, the transcript closed, the
    /// widget cleared (hidden media suspends); holds while playback is
    /// paused (the D3 poll-only-while-playing discipline); otherwise
    /// advances the walk. Position reads `transport_position_secs` — the
    /// S4 rate-conversion accessor, media seconds at every rate.
    fn reel_preview_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.active_tab != ViewerTab::Workspace
            || self.transcript.is_none()
            || self.media_widget.is_none()
            || self.reel_preview.is_none()
        {
            self.stop_reel_preview(cx);
            return false;
        }
        let playing = self
            .media_widget
            .as_ref()
            .is_some_and(|widget| widget.read(cx).is_transport_playing());
        if !playing {
            return true;
        }
        let Some(position_ms) = self
            .media_widget
            .as_ref()
            .and_then(|widget| widget.read(cx).transport_position_secs())
            .map(|secs| (secs * 1000.0) as u64)
        else {
            return true;
        };
        self.step_reel_preview(position_ms, cx)
    }

    /// S6: one walk step — advance past every span the position has crossed,
    /// seeking the newly-current span's start through the bound widget.
    /// Separated from the tick so the walk is drivable with a scripted
    /// position timeline (the playing gate lives in the tick).
    fn step_reel_preview(&mut self, position_ms: u64, cx: &mut Context<Self>) -> bool {
        let step = self
            .reel_preview
            .as_mut()
            .map(|preview| preview.advance(position_ms));
        match step {
            Some(ReelPreviewStep::Hold) => true,
            Some(ReelPreviewStep::Seek(target_ms)) => {
                self.seek_workspace_transcript(target_ms, cx);
                true
            }
            Some(ReelPreviewStep::Finished) => {
                self.reel_preview = None;
                self.reel_preview_task.take();
                self.status = Some("Reel preview finished.".into());
                cx.notify();
                false
            }
            None => false,
        }
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
    /// transcript sub-surface is visible (Workspace mode with a transcript
    /// open). Self-cancels when the mode leaves or the workspace closes;
    /// the handle drop cancels the spawned future (the same lifecycle as the
    /// queue poll).
    fn start_transcript_follow(&mut self, cx: &mut Context<Self>) {
        if self.transcript_follow_task.is_some() {
            return;
        }
        let entity = cx.entity().downgrade();
        self.transcript_follow_task = Some(cx.spawn(async move |_this, cx| {
            loop {
                cx.background_executor().timer(FOLLOW_POLL_INTERVAL).await;
                let keep_going = match entity.update(cx, |viewer, cx| {
                    if viewer.active_tab != ViewerTab::Workspace || viewer.transcript.is_none() {
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
        // The reveal targets word blocks (S3d); on the events surface the
        // body rows are events, so the word mark must not drive scrolling.
        if !self.follow_viewport || self.transcript_surface != TranscriptSurface::Words {
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
            // The workspace carries both request lifecycles its surfaces own
            // (T3): the edit lifecycle (trim/concat — and the transcript
            // sub-surface's compose/highlight edits share it), then the
            // transcript load behind the open sub-surface. S5's
            // progress-visible contract: while an edit is in flight, the
            // progress surfaces — previously it rendered nowhere
            // (2026-10-26 review). Failures fall through to the specific
            // status line below.
            ViewerTab::Workspace => resource_status(
                "Edit",
                &self.edit_request.state,
                !self.assets.is_empty(),
                self.edit_progress.as_deref(),
            )
            .or_else(|| {
                resource_status(
                    "Transcript",
                    &self.transcript_request.state,
                    self.transcript.is_some(),
                    None,
                )
            }),
            ViewerTab::Library => resource_status(
                "Library",
                &self.gallery_request.state,
                self.gallery_total.is_some(),
                None,
            ),
        };
        resource
            // The activity layer (T2): with the drawer closed, a failed job
            // listing has no surface of its own — the toolbar status line
            // carries it (the open drawer renders the failure itself).
            .or_else(|| {
                (!self.activity_open).then(|| match &self.jobs_request.state {
                    ResourceState::Failed(error) => {
                        Some((format!("Activity: {error}").into(), ui::Color::Error))
                    }
                    _ => None,
                })?
            })
            .or_else(|| {
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

    /// The asset workspace (T1): the player, the edit toolbar, and the
    /// toggleable inspector pane in one screen — the primary area is the
    /// player; the inspector is its supporting pane. Serves audio and
    /// video through one position source (fc4f469dc5 — no per-kind split).
    /// An open transcript renders as the workspace's sub-surface (T3): the
    /// reading list replaces the plain player area while the header, edit
    /// toolbar, and inspector pane stay the recording's — the ruling: you
    /// select an asset and stay with it.
    fn render_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if self.transcript.is_some() {
            return self
                .render_transcript_workspace(window, cx)
                .into_any_element();
        }
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
        //
        // The current selection's highlight (HIG "Split views": each pane
        // that leads to the object persistently highlights it): the same
        // element_active treatment the Library row carries, so the selected
        // object reads identically in both panes.
        let header = h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .min_w_0()
            .overflow_hidden()
            .bg(cx.theme().colors().element_active)
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Label::new(asset.src.clone())
                    .size(LabelSize::Small)
                    .color(ui::Color::Default)
                    .flex_1()
                    .truncate(),
            )
            .child(
                Label::new(asset.tool.clone())
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint)
                    .truncate(),
            )
            .child(self.workspace_inspector_toggle(cx));

        // ── Edit toolbar: trim marks + concatenation queue ───────────────
        let toolbar = self.render_edit_toolbar(&widget, &asset, cx);

        // No scroll wrapper on the workspace's player area: a scroll
        // container gives its children indefinite height, and the video
        // widget's aspect-fit sizing needs a definite height derived from
        // the pane (min_h_0 lets flex_1 shrink below content size).
        // Library/the activity drawer scroll and the inspector scrolls
        // internally; the player fills.
        let content = div()
            .id("media-viewer-content")
            // Test-support hook: exposes the content container's laid-out
            // bounds to layout ground-truth tests (noop in release builds).
            .debug_selector(|| "media-viewer-content".into())
            .flex_1()
            .min_h_0()
            // The no-overlap backstop: the shared player shrinks with the
            // pane (its usable-height floor moved to the content-flow
            // embedder, `block_renderer`), and anything that still wants
            // more height at extreme pane heights clips here instead of
            // painting past the viewer pane into the Steer director — the
            // close-transcript overlap (operator report 2026-11-04).
            .overflow_hidden()
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
                "Playback and derivative edits beside the asset's inspector — marks and trims serve audio and video; results land as new assets.",
            ))
            .child(header)
            .child(toolbar)
            .child(content)
            .when(self.inspector_open, |el| el.child(self.render_inspector(cx)))
            .into_any_element()
    }

    /// The edit toolbar (trim marks + concatenation queue) — shared by the
    /// plain workspace and the transcript sub-surface (T3): both bind the
    /// same shared player, so the toolbar serves the recording either way.
    /// Width discipline: the pane is a flex item whose min-content is this
    /// row's content width, and seven buttons + two readout labels ≈ 800px —
    /// wider than any dock pane. The row wraps (the same overflow policy as
    /// the inspector's kv rows) so every action stays reachable at ~320px
    /// panes; min_w_0 keeps the row's min-content from inflating the pane;
    /// the readout labels truncate.
    fn render_edit_toolbar(
        &self,
        widget: &Entity<hkask_media_widget::MediaWidget>,
        asset: &MediaAsset,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (position_label, marks_label, trim_ready) = widget.read(cx).edit_state_labels();
        let edit_active = self.edit_request.state == ResourceState::Loading;
        let concat_count = self.concat_queue.len();
        let queued_current = self.concat_queue.contains(&asset.src);
        h_flex()
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
            })
    }

    /// The workspace's inspector toggle chip (T1): the supporting pane is
    /// part of the workspace, not a navigation destination — one click
    /// hides or returns it without touching the player or selection.
    /// Shared by the plain workspace header and the transcript
    /// sub-surface's header (T3), so the pane stays reachable in both.
    fn workspace_inspector_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("media-workspace-inspector-toggle")
            .flex_shrink_0()
            .px_2()
            .h_5()
            .flex()
            .items_center()
            .rounded_sm()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .cursor_pointer()
            .child(
                Label::new(if self.inspector_open {
                    "Hide inspector"
                } else {
                    "Inspector"
                })
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_inspector(cx)))
    }

    /// The Library (T3): the browse mode — every indexed asset as rows,
    /// and the recordings browser's new home: a gallery video/audio asset
    /// carries its transcription state with the Transcribe affordance on
    /// its row (R5) and its stored transcripts nested beneath it (open,
    /// delete — durable exports are preserved by the server on deletion).
    /// Unlinked transcripts — their recordings left the gallery index —
    /// trail in their own section with honest degradation copy, reachable,
    /// never silently hidden (operator report 2026-10-26: the section must
    /// list what's available, not dead-end).
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
        // The stored transcripts whose recordings left the gallery index —
        // rendered in their own section below the rows, never hidden.
        let unlinked_rows: Vec<usize> = self
            .transcript_listing
            .as_ref()
            .map(|listing| {
                listing
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| row.gallery_asset_id.is_none())
                    .map(|(row_index, _)| row_index)
                    .collect()
            })
            .unwrap_or_default();
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
                // The recording's transcription state (R2/R5, T3): with
                // the listing loaded, the full status — a live transcribe
                // job (the observed-anywhere live set), a stored linked
                // transcript, or Not transcribed. Without the listing,
                // only a live job is knowable — the recordings status
                // line above the rows explains the rest; a not-yet-loaded
                // listing must not render a dishonest Not-transcribed.
                let recording_id = if asset.kind == "image" {
                    None
                } else {
                    asset.gallery_asset_id.as_deref()
                };
                let transcription = match recording_id {
                    Some(_) => match self.transcript_listing.as_ref() {
                        Some(_) => Some(self.recording_transcription_status(asset)),
                        None => self
                            .live_jobs
                            .iter()
                            .any(|job| {
                                job.op == "transcribe"
                                    && job.source.as_deref() == Some(asset.src.as_str())
                            })
                            .then_some(RecordingTranscriptionStatus::Transcribing),
                    },
                    None => None,
                };
                let transcript_row_indices: Vec<usize> =
                    match (recording_id, self.transcript_listing.as_ref()) {
                        (Some(id), Some(listing)) => listing
                            .iter()
                            .enumerate()
                            .filter(|(_, row)| row.gallery_asset_id.as_deref() == Some(id))
                            .map(|(row_index, _)| row_index)
                            .collect(),
                        _ => Vec::new(),
                    };
                let line = h_flex()
                    .h_8()
                    .gap_2()
                    .flex()
                    .items_center()
                    .min_w_0()
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
                            // The ambient activity badge (T2): a live
                            // research job whose primary source is this
                            // asset — transient progress, never
                            // error-styled (HIG Progress indicators).
                            .when_some(self.asset_activity_verb(asset), |el, verb| {
                                let badge_selector = format!("media-library-activity-{ix}");
                                el.child(
                                    div()
                                        .id(SharedString::from(badge_selector.clone()))
                                        .debug_selector(move || badge_selector)
                                        .flex_shrink_0()
                                        .child(
                                            Label::new(verb)
                                                .size(LabelSize::XSmall)
                                                .color(ui::Color::Hint),
                                        ),
                                )
                            })
                            // The recording's transcription state (T3) —
                            // the status the old recordings browser carried,
                            // now beside the recording's own Library row.
                            .when_some(transcription, |el, status| {
                                el.child(
                                    Label::new(match status {
                                        RecordingTranscriptionStatus::Transcribing => {
                                            "Transcribing…"
                                        }
                                        RecordingTranscriptionStatus::Transcribed => "Transcribed",
                                        RecordingTranscriptionStatus::NotTranscribed => {
                                            "Not transcribed"
                                        }
                                    })
                                    .size(LabelSize::XSmall)
                                    .color(match status {
                                        RecordingTranscriptionStatus::Transcribing => {
                                            ui::Color::Hint
                                        }
                                        RecordingTranscriptionStatus::Transcribed => {
                                            ui::Color::Muted
                                        }
                                        RecordingTranscriptionStatus::NotTranscribed => {
                                            ui::Color::Error
                                        }
                                    })
                                    .flex_shrink_0(),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_selection(Some(ix), cx);
                                this.activate_tab(ViewerTab::Workspace, cx);
                            })),
                    );
                // The Transcribe affordance (R5, operator ruling 2026-10-28):
                // a not-transcribed recording can be taken to its
                // transcript without a chat turn — a sibling of the select
                // area, so opening and transcribing never fire together.
                let line = match transcription {
                    Some(RecordingTranscriptionStatus::NotTranscribed) => {
                        let recording_id = recording_id.unwrap_or_default().to_string();
                        let source = asset.src.clone();
                        line.child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-recording-transcribe-{ix}"
                                )))
                                .debug_selector(|| "media-recording-transcribe".into())
                                .cursor_pointer()
                                .px_2()
                                .h_5()
                                .flex()
                                .items_center()
                                .flex_shrink_0()
                                .rounded_sm()
                                .hover(|el| el.bg(cx.theme().colors().element_hover))
                                .child(
                                    Label::new("Transcribe")
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Default),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.transcribe_recording(
                                        recording_id.clone(),
                                        source.clone(),
                                        cx,
                                    );
                                })),
                        )
                    }
                    _ => line,
                };
                let line = line.child(
                    // Curate affordance: two-step delete (index entry only).
                    div()
                        .id(SharedString::from(format!("media-library-delete-{ix}")))
                        .px_2()
                        .h_5()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
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
                );
                div()
                    .id(SharedString::from(format!("media-library-row-{ix}")))
                    .px_2()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(line)
                    // The recording's stored transcripts, nested beneath
                    // its row (the old recordings browser's arrangement).
                    .children(transcript_row_indices.iter().map(|row_index| {
                        div()
                            .pl_4()
                            .child(self.render_transcript_row(*row_index, cx))
                    }))
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
        // The recordings section's status line (T3): when the listing is
        // not loaded, the rows render without transcription states — this
        // line names why (D5, never silence), always visible above the
        // content, never scrolled out of view.
        let recordings_note = match self.transcript_listing.as_ref() {
            Some(_) => None,
            None => match &self.transcript_listing_request.state {
                ResourceState::Loading => Some((
                    "Loading the recordings' transcripts…".to_string(),
                    ui::Color::Muted,
                )),
                ResourceState::Failed(error) => Some((
                    format!("Transcripts unavailable: {error}"),
                    ui::Color::Error,
                )),
                ResourceState::Idle | ResourceState::Ready => Some((
                    "Recordings' transcripts not loaded — reload the Library to list them."
                        .to_string(),
                    ui::Color::Muted,
                )),
            },
        };
        let content = if rows.is_empty() && unlinked_rows.is_empty() {
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
                .when(!unlinked_rows.is_empty(), |el| {
                    el.child(Self::section_label("Unlinked transcripts"))
                        .child(
                            Label::new(SharedString::from(
                                "Their recordings left the gallery index — the transcripts, \
                                 their durable exports, and the stored media path remain \
                                 openable here.",
                            ))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Hint),
                        )
                        .children(
                            unlinked_rows
                                .iter()
                                .map(|row_index| self.render_transcript_row(*row_index, cx))
                                .collect::<Vec<_>>(),
                        )
                })
                .into_any_element()
        };
        v_flex()
            .id("media-viewer-library")
            .flex_1()
            .min_h_0()
            .child(Self::tab_purpose(
                "Every indexed asset — browse and select; recordings carry their transcription state and their transcripts beneath them. Delete removes the listing, never the file on disk.",
            ))
            .child(pagination)
            .when_some(recordings_note, |el, (note, color)| {
                el.child(
                    div().px_3().py_1().child(
                        Label::new(SharedString::from(note))
                            .size(LabelSize::XSmall)
                            .color(color)
                            .truncate(),
                    ),
                )
            })
            .child(content)
            .into_any_element()
    }

    /// The activity drawer (T2): background research jobs observed on
    /// demand — the ambient process state's monitor (FCP Background
    /// Tasks: automatic work, observed when wanted, cancellable here).
    /// Not a navigation mode: the toolbar's Activity toggle opens it over
    /// any mode. Rows keep op/status/created_at with Cancel for
    /// queued/running jobs — the observed live set first (a live job
    /// deeper than the newest-first page is observed by the status probes
    /// and stays cancellable here, loop-register L11), then the page's
    /// recent history. The footer carries the process-local history
    /// disclosure verbatim from the fields the strict listing parse
    /// already validates.
    fn render_activity_drawer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let body = if self.jobs_total.is_none() {
            let (title, hint) = match &self.jobs_request.state {
                ResourceState::Loading => ("Loading activity", "Fetching research jobs…"),
                ResourceState::Failed(error) => ("Activity unavailable", error.as_str()),
                ResourceState::Idle => (
                    "Activity not loaded",
                    "Open the drawer or refresh to load it.",
                ),
                ResourceState::Ready => (
                    "Activity ready — no jobs",
                    "The job listing returned no rows.",
                ),
            };
            self.render_empty(title, hint).into_any_element()
        } else if self.jobs.is_empty() && self.live_jobs.is_empty() {
            self.render_empty(
                "No research jobs",
                "No research jobs have been submitted (or process-local history restarted).",
            )
            .into_any_element()
        } else {
            let live_ids = self
                .live_jobs
                .iter()
                .map(|job| job.id.as_str())
                .collect::<HashSet<_>>();
            let recent = self
                .jobs
                .iter()
                .filter(|job| !live_ids.contains(job.id.as_str()))
                .collect::<Vec<_>>();
            let mut rows = v_flex()
                .id("media-activity-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap_0p5()
                .p_2();
            if !self.live_jobs.is_empty() {
                rows = rows.child(Self::section_label("Active"));
                for job in &self.live_jobs {
                    rows = rows.child(self.activity_job_row(job, cx));
                }
            }
            if !recent.is_empty() {
                rows = rows.child(Self::section_label("Recent"));
                for job in recent {
                    rows = rows.child(self.activity_job_row(job, cx));
                }
            }
            rows.into_any_element()
        };
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
            .id("media-activity-drawer")
            .debug_selector(|| "media-activity-drawer".into())
            .h(gpui::relative(ACTIVITY_DRAWER_FRACTION))
            .min_h_0()
            .min_w_0()
            .flex_shrink_0()
            .border_t_1()
            .border_color(cx.theme().colors().border_variant)
            .child(Self::tab_purpose(
                "Background research jobs — watch and cancel here; history is process-local and bounded.",
            ))
            .child(
                h_flex()
                    .h_8()
                    .px_2()
                    .items_center()
                    .min_w_0()
                    .child(
                        Label::new(count_label)
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .truncate(),
                    ),
            )
            .child(body)
            .when_some(self.jobs_restart_behavior.as_deref(), |el, disclosure| {
                el.child(
                    div()
                        .id("media-activity-disclosure")
                        .debug_selector(|| "media-activity-disclosure".to_string())
                        .px_2()
                        .py_1()
                        .min_w_0()
                        .border_t_1()
                        .border_color(cx.theme().colors().border_variant)
                        .child(
                            Label::new(SharedString::from(disclosure.to_string()))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Hint),
                        ),
                )
            })
            .into_any_element()
    }

    /// One activity-drawer job row (the retired Queue tab's row shape,
    /// keyed by the stable job id so rows survive listing merges):
    /// op/status/created_at with Cancel for queued/running jobs.
    fn activity_job_row(
        &self,
        job: &JobRecord,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let cancellable = job.status == "queued" || job.status == "running";
        let short_id: SharedString = if job.id.len() > 8 {
            job.id[..8].to_string().into()
        } else {
            job.id.clone().into()
        };
        let row_selector = format!("media-activity-row-{}", job.id);
        let cancel_selector = format!("media-activity-cancel-{}", job.id);
        div()
            .id(SharedString::from(row_selector.clone()))
            .debug_selector(move || row_selector)
            .h_8()
            .px_2()
            .gap_2()
            .flex()
            .items_center()
            .min_w_0()
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
                        .id(SharedString::from(cancel_selector.clone()))
                        .debug_selector(move || cancel_selector)
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
    }

    /// The workspace's inspector pane (T1): the seven Detail sections as
    /// pane-internal tabs — alternate views of the same selected object
    /// (HIG "Tab views": closely related areas of content). A supporting
    /// pane, not a destination: it renders under the player, toggles
    /// without touching the selection or the player, and every tab
    /// renders the same selected asset.
    fn render_inspector(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tabs = h_flex()
            .id("media-inspector-tabs")
            .gap_1()
            .px_2()
            .py_1()
            .flex_wrap()
            .min_w_0()
            .overflow_hidden()
            .border_t_1()
            .border_color(cx.theme().colors().border_variant)
            .children(InspectorTab::ALL.map(|tab| self.inspector_tab_button(tab, cx)));
        let Some(detail) = self.detail.as_ref() else {
            // The pane-level data states (the old Detail tab's empty view):
            // the inspector's reads follow the selection, so every tab
            // shares one loading/failed/idle state. An unindexed selection
            // states its own condition instead of pointing elsewhere.
            let unindexed_selection = self
                .selected
                .and_then(|ix| self.assets.get(ix))
                .is_some_and(|asset| asset.gallery_asset_id.is_none());
            let (title, hint) = match &self.detail_request.state {
                ResourceState::Loading => (
                    "Loading inspector",
                    "Fetching the selected asset's metadata…",
                ),
                ResourceState::Failed(error) => ("Inspector unavailable", error.as_str()),
                ResourceState::Ready => {
                    ("Inspector ready — empty", "The asset returned no detail.")
                }
                ResourceState::Idle if unindexed_selection => (
                    "No inspector data",
                    "The selected asset is not in the gallery index — run gallery_refresh to index it.",
                ),
                ResourceState::Idle => (
                    "No inspector data",
                    "Select an indexed gallery asset in the Library — the inspector follows the selection.",
                ),
            };
            return v_flex()
                .id("media-inspector")
                .h(gpui::relative(INSPECTOR_PANE_FRACTION))
                .min_h_0()
                .flex_shrink_0()
                .debug_selector(|| "media-inspector".into())
                .child(tabs)
                .child(self.render_empty(title, hint).into_any_element())
                .into_any_element();
        };
        v_flex()
            .id("media-inspector")
            .h(gpui::relative(INSPECTOR_PANE_FRACTION))
            .min_h_0()
            .flex_shrink_0()
            .debug_selector(|| "media-inspector".into())
            .child(tabs)
            .child(
                v_flex()
                    .id("media-inspector-section")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.render_inspector_section(detail, cx)),
            )
            .into_any_element()
    }

    /// One pane-internal inspector tab chip (T1) — the tab bar's tab
    /// button at pane scale: the active tab carries the selected
    /// treatment; the row wraps so all seven stay reachable in a narrow
    /// dock.
    fn inspector_tab_button(
        &self,
        tab: InspectorTab,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.inspector_tab == tab;
        div()
            .id(tab.element_id())
            .debug_selector(move || tab.element_id().to_string())
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
                Label::new(tab.label())
                    .size(LabelSize::Small)
                    .truncate()
                    .color(if active {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.inspector_tab = tab;
                cx.notify();
            }))
    }

    /// The active inspector tab's body (T1) — the old Detail sections, one
    /// per pane-internal tab, with honest absence copy where a section's
    /// data is missing (the stacked view could hide an empty section; a
    /// tab must always render something).
    fn render_inspector_section(&self, detail: &Value, cx: &mut Context<Self>) -> gpui::AnyElement {
        match self.inspector_tab {
            InspectorTab::Record => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(image) = detail.get("image").and_then(|i| i.as_object()) {
                    for (key, value) in image {
                        if let Some(text) = scalar_text(value) {
                            body = body.child(Self::kv_row(key, &text, cx));
                        }
                    }
                } else {
                    body = body.child(Self::inspector_absent("No record in the asset detail."));
                }
                body.into_any_element()
            }
            InspectorTab::Tags => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(tags) = detail.get("tags").and_then(|t| t.as_array()) {
                    for tag in tags {
                        let tag_type = tag.get("tag_type").and_then(|t| t.as_str()).unwrap_or("?");
                        let value = tag.get("value").and_then(|v| v.as_str()).unwrap_or("?");
                        body = body.child(Self::kv_row(tag_type, value, cx));
                    }
                } else {
                    body = body.child(Self::inspector_absent("No tags."));
                }
                body.into_any_element()
            }
            InspectorTab::Lineage => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(map) = detail
                    .get("lineage")
                    .filter(|lineage| !lineage.is_null())
                    .and_then(|lineage| lineage.as_object())
                {
                    for (key, value) in map {
                        if let Some(text) = scalar_text(value) {
                            body = body.child(Self::kv_row(key, &text, cx));
                        }
                    }
                } else {
                    body = body.child(Self::inspector_absent("No lineage."));
                }
                body.into_any_element()
            }
            InspectorTab::Faces => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(faces) = detail.get("faces").and_then(|f| f.as_array())
                    && !faces.is_empty()
                {
                    body = body.child(Self::kv_row("count", &faces.len().to_string(), cx));
                } else {
                    body = body.child(Self::inspector_absent("No faces."));
                }
                body.into_any_element()
            }
            InspectorTab::Transcripts => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(transcripts) = &self.detail_transcripts {
                    if transcripts.is_empty() {
                        body = body.child(Self::inspector_absent(
                            "No transcripts yet — transcribe the recording and one appears here.",
                        ));
                    }
                    for transcript in transcripts {
                        let timings = if transcript.has_word_timings {
                            "word timings"
                        } else {
                            "no word timings"
                        };
                        let transcript_id = transcript.id.clone();
                        let mut row = div()
                            .id(SharedString::from(format!(
                                "media-inspector-open-{}",
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
                        body = body.child(row.on_click(cx.listener(move |this, _, _, cx| {
                            this.open_transcript(transcript_id.clone(), cx);
                        })));
                    }
                }
                // A failed reload keeps the last-good rows visible and
                // states the failure beside them (the request-lifecycle
                // contract: failures surface, never silence); an in-flight
                // first load shows its own copy.
                if let ResourceState::Failed(error) = &self.detail_transcripts_request.state {
                    body = body.child(Self::inspector_absent(&format!(
                        "Transcripts unavailable: {error}"
                    )));
                }
                if self.detail_transcripts.is_none()
                    && self.detail_transcripts_request.state == ResourceState::Loading
                {
                    body = body.child(Self::inspector_absent("Loading transcripts…"));
                }
                body.into_any_element()
            }
            InspectorTab::Annotations => {
                let mut body = v_flex().gap_2().p_3();
                // The annotation surface (2026-10-27): researcher tags/notes
                // with direct create/replace/delete — the same
                // direct-manipulation rationale as the transcript
                // highlights, on the inspector's home surface. The whole
                // tab needs an indexed asset.
                let Some(asset_id) = self
                    .selected
                    .and_then(|ix| self.assets.get(ix))
                    .and_then(|asset| asset.gallery_asset_id.clone())
                else {
                    return body
                        .child(Self::inspector_absent(
                            "Annotations need an indexed asset — surface this asset through the gallery first.",
                        ))
                        .into_any_element();
                };
                if let Some(edit) = self.annotation_edit.clone()
                    && let Some(focus) = self.annotation_input_focus.clone()
                {
                    body = body.child(Self::render_annotation_editor(&edit, &focus, cx));
                } else {
                    for kind in ["tag", "note"] {
                        body = body.child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-inspector-annotation-add-{kind}"
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
                        body = body.child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-inspector-annotation-{annotation_id}"
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
                                            "media-inspector-annotation-edit-{annotation_id}"
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
                                            "media-inspector-annotation-delete-{annotation_id}"
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
                if let ResourceState::Failed(error) = &self.detail_annotations_request.state {
                    body = body.child(Self::inspector_absent(&format!(
                        "Annotations unavailable: {error}"
                    )));
                }
                if self.detail_annotations.is_none()
                    && self.detail_annotations_request.state == ResourceState::Loading
                {
                    body = body.child(Self::inspector_absent("Loading annotations…"));
                }
                body.into_any_element()
            }
            InspectorTab::Reel => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(provenance) = parse_reel_provenance(detail) {
                    body = body.child(Self::kv_row("rendered from transcript", &provenance, cx));
                } else {
                    body = body.child(Self::inspector_absent(
                        "No reel provenance — the asset was not rendered from a transcript.",
                    ));
                }
                body.into_any_element()
            }
        }
    }

    /// The gallery recording a workspace transcript belongs to (R3): the
    /// linked asset from the panel's current gallery page. Recordings on
    /// unlisted pages fall to the unlinked path — the honest degradation.
    fn recording_asset<'a>(&'a self, state: &'a TranscriptPaneState) -> Option<&'a MediaAsset> {
        let asset_id = state.gallery_asset_id.as_deref()?;
        self.assets
            .iter()
            .find(|asset| asset.gallery_asset_id.as_deref() == Some(asset_id))
    }

    /// The workspace's media body (R3): the RECORDING's asset body when the
    /// transcript links to a live gallery recording — the same body the
    /// workspace player renders, so a video-keyed transcript plays the video. An
    /// unlinked transcript (its recording left the gallery index) falls
    /// to its stored media_path — the transcript and its durable exports
    /// stay reachable.
    fn workspace_media_body(&self, state: &TranscriptPaneState) -> String {
        match self.recording_asset(state) {
            Some(asset) => asset.body.clone(),
            None => transcript_media_body(&state.media_path),
        }
    }

    /// The transcript sub-surface of the asset workspace (T3, S3a): the
    /// open transcript's working view rendered as paragraph blocks with
    /// speaker labels and a layers strip, INSIDE the workspace — the
    /// reading list replaces the plain player area while the header, the
    /// edit toolbar, and the inspector pane stay the recording's (the
    /// ruling: you select an asset and stay with it). The player inside
    /// the list's header rows is the recording's shared player (S3b: the
    /// same viz-core-registry widget the workspace and inline renders use
    /// — one player per media identity, rebound through the R3 body,
    /// suspended on leave; D1). Alignment degradations surface as visible
    /// status notes, never silence (D5); click-to-seek and follow-along
    /// land with the S3b hit-testing element. An unlinked transcript
    /// renders here too — reachable, with its own degradation copy,
    /// never silently hidden.
    fn render_transcript_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // The search input's focus handle (S3) — created on first need, and
        // its focus/blur listeners registered with it on the same first
        // render (`ensure_transcript_search_focus_listeners` — idempotent).
        // Registering at render (not from the input's click handler) keeps
        // keyboard-driven focus covered: an arrow/tab arrival at the input
        // suspends follow exactly like a click. Placed before the state
        // borrow (the registration takes &mut self). The header's search
        // row reads the stored handle.
        if self.transcript_search_focus.is_none() {
            self.transcript_search_focus = Some(cx.focus_handle());
        }
        self.ensure_transcript_search_focus_listeners(window, cx);
        let Some(state) = self.transcript.as_ref() else {
            // The caller (render_workspace) opens the sub-surface only
            // with a transcript loaded; keep the function total anyway.
            return div().into_any_element();
        };
        // The workspace media body is the RECORDING's (R3) — the header's
        // name and hint derive from the same linkage, all read before the
        // widget binding mutates the viewer.
        let body = self.workspace_media_body(state);
        let recording = self.recording_asset(state).cloned();
        let recording_name = match recording.as_ref() {
            Some(asset) => recording_display_name(&asset.src),
            None => recording_display_name(&state.media_path),
        };
        let tool_label = recording.as_ref().map(|asset| asset.tool.to_string());
        let unlinked = recording.is_none();
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
        let widget = self.media_widget.clone();
        // The sub-surface's header: the close affordance returns to the
        // plain asset workspace; the recording's name flows from the
        // gallery asset (R3) and falls to the stored media_path when
        // unlinked — the hint names what the player follows. The inspector
        // toggle keeps the pane reachable while reading (T3).
        let close = div()
            .id("media-transcript-close")
            .debug_selector(|| "media-transcript-close".into())
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("← Close transcript"))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_transcript_workspace(cx);
            }));
        let header = h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .min_w_0()
            .overflow_hidden()
            .bg(cx.theme().colors().element_active)
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(close)
            .child(
                Label::new(SharedString::from(recording_name))
                    .size(LabelSize::Small)
                    .color(ui::Color::Default)
                    .flex_1()
                    .truncate(),
            )
            .when(unlinked, |el| {
                el.child(
                    Label::new(SharedString::from(
                        "unlinked recording — the player follows the stored media path",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint)
                    .truncate(),
                )
            })
            .when_some(tool_label, |el, tool| {
                el.child(
                    Label::new(SharedString::from(tool))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Hint)
                        .truncate(),
                )
            })
            .child(self.workspace_inspector_toggle(cx));
        // The edit toolbar serves the same shared player while the
        // recording is the workspace's selected asset (open_transcript
        // selects it, T3); an unlinked transcript has no gallery asset for
        // the trim/concat actions — honest absence, not a dead toolbar.
        let recording_selected = recording.as_ref().is_some_and(|asset| {
            self.selected
                .and_then(|ix| self.assets.get(ix))
                .is_some_and(|selected| selected.gallery_asset_id == asset.gallery_asset_id)
        });
        let toolbar = match (recording_selected, recording.as_ref()) {
            (true, Some(asset)) => Some(
                self.render_edit_toolbar(
                    widget
                        .as_ref()
                        .expect("the sub-surface binds the recording's player"),
                    asset,
                    cx,
                ),
            ),
            _ => None,
        };
        // The label input's focus handle, created on first need (pure tests
        // construct the viewer without a GPUI context).
        let label_input_focus = self
            .label_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The selection popover's focus handle — the same lazy idiom.
        let selection_popover_focus = self
            .selection_popover_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The right-click context menu's focus handle — the same lazy idiom.
        let context_menu_focus = self
            .context_menu_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The open label editor (S4-3) stays pinned above the list —
        // always in view while a create or rename is pending.
        let pending_editor = self.highlight_edit.clone();
        // The open selection popover (S2) pins beside it — the completed
        // range's action tray. The render carries the excerpt and the
        // include toggle's CURRENT label ("Include in reel" vs "Remove
        // from reel" over an included highlight's exact range); the
        // actions read the range from the tray state, not from the render.
        let pending_popover = self.selection_popover.map(|(start_word, end_word)| {
            let excerpt = transcript_range_excerpt(state, start_word, end_word);
            let include_label = if self.reel_clips.iter().any(|clip| {
                clip.include && clip.start_word == start_word && clip.end_word == end_word
            }) {
                "Remove from reel"
            } else {
                "Include in reel"
            };
            // The in-reel strike arm (S6) needs the completed range inside
            // exactly one INCLUDED clip.
            let strike_in_clip = self
                .reel_clips
                .iter()
                .filter(|clip| {
                    clip.include && clip.start_word <= start_word && end_word <= clip.end_word
                })
                .count()
                == 1;
            // The tray's action set follows the view's mode (S5): the
            // correction-only set on re_anchored views.
            let correct_only =
                selection_tray_mode(&state.alignment) == Ok(SelectionTrayMode::CorrectOnly);
            (excerpt, include_label, strike_in_clip, correct_only)
        });
        // The whole sub-surface content (header rows, surface body rows,
        // annotation sections) renders as ONE virtualized list (S3d):
        // everything scrolls together exactly as the workspace always
        // did. On the word surface, timed blocks are interactive text —
        // every word is a click range that seeks the bound player to its
        // start (S3b-2), the word containing the playback position carries
        // the follow-along mark (S3c), and the follow task keeps the marked
        // block in view through the list state. On the events surface
        // (S5) the body rows are the visual-transcript events — click an
        // event to seek to its start, the same seek discipline — and the
        // tail is the authoring section.
        let blocks = Arc::new(build_pane_blocks(state));
        let events = Arc::new(
            self.visual_events
                .as_ref()
                .map(|state| state.events.clone())
                .unwrap_or_default(),
        );
        let item_count = match self.transcript_surface {
            TranscriptSurface::Words => {
                WORKSPACE_HEADER_ITEMS + blocks.len() + WORKSPACE_LIST_TAIL_ITEMS
            }
            TranscriptSurface::Events => {
                WORKSPACE_HEADER_ITEMS
                    + events.len()
                    + WORKSPACE_EVENTS_TAIL_ITEMS
                    + usize::from(self.visual_events_truncated())
            }
        };
        if self.transcript_list_count != item_count {
            self.transcript_list.reset(item_count);
            self.transcript_list_count = item_count;
            self.follow_reveals_in_flight = 0;
            self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
        }
        let list_state = self.transcript_list.clone();
        let workspace_items = cx.processor(move |this, ix, _window, cx| {
            this.render_workspace_list_item(ix, blocks.as_slice(), events.as_slice(), cx)
        });
        let workspace_list = gpui::list(list_state, workspace_items)
            .flex_1()
            .min_h_0()
            .p_3();
        let mut workspace = v_flex()
            .id("media-viewer-transcript")
            .flex_1()
            .min_h_0()
            // S2: press-and-release drag selection over the word surface.
            // The surface root spans every block plus the toolbar, so a
            // press anywhere in the surface resets or arms the pending
            // drag from the exact word hover (None outside words cancels a
            // stale drag — including one left mid-drag on the toolbar),
            // and a release anywhere in the surface completes it:
            // cross-word releases open the label editor on the union
            // range, same-word releases stay plain clicks
            // (InteractiveText seeks through its own click).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.word_drag_anchor = if this.highlight_edit.is_some() {
                        None
                    } else {
                        this.drag_last_word
                    };
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    this.word_drag_released(window, cx);
                }),
            )
            // S8: right-click opens the context menu — the pointer
            // alternative to the select-then-tray gesture. The press
            // remembers the hovered word as the menu's fallback target;
            // the release resolves the range (an open tray or pending
            // anchor wins over the hovered word) and opens the menu.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _event, _window, cx| {
                    this.context_menu_fallback = this.drag_last_word;
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, _event, window, cx| {
                    this.open_context_menu(window, cx);
                }),
            );
        // The pinned interaction band (the composed two-pane fix,
        // 2026-11-07): the player, the surface/mode chips, and the search
        // input pin ABOVE the list — master-detail inside the
        // sub-surface: the player is the persistent master object; the
        // transcript is the navigated detail. The follow-along's deep
        // reveals scroll the text BENEATH the band instead of displacing
        // the player out of the pane (the operator's "pushes the video
        // from the view"), and the search input the operator reaches for
        // mid-reading never scrolls away. THE HEIGHT BUDGET (the one-list
        // design's rejected concern, now answered by composition): the
        // band caps at 55% of the workspace, and the player row is the
        // flexible member — the reading area keeps at least 45% at every
        // pane height, the chips/search rows are irreducible, and below
        // them the player row shrinks first (the widget's own pins keep
        // the transport visible at every height) with overflow_hidden as
        // the degenerate-clip backstop (the S1 discipline: visible
        // truncation, never corruption of a neighboring surface).
        // Pinned by follow_scroll_keeps_the_pinned_player_in_view.
        let player_row = match widget.clone() {
            Some(widget) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .child(widget)
                .into_any_element(),
            // Degradation surfaced, never silent: the transcript text
            // still renders without the player.
            None => div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    Label::new(SharedString::from(
                        "Playback unavailable — the transcript and editing controls remain available.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error),
                )
                .into_any_element(),
        };
        let chips_row = self.render_transcript_chips_row(cx);
        let search_row = self.render_transcript_search_row(cx);
        let words_surface = self.transcript_surface == TranscriptSurface::Words;
        workspace = workspace.child(
            v_flex()
                .id("media-transcript-band")
                .debug_selector(|| "media-transcript-band".into())
                .max_h(gpui::relative(0.55))
                .overflow_hidden()
                .px_3()
                .pt_2()
                .gap_1()
                .child(player_row)
                .child(chips_row)
                .when(words_surface, |el| el.child(search_row)),
        );
        if let Some(edit) = pending_editor {
            workspace = workspace.child(v_flex().px_3().pt_3().child(Self::render_label_editor(
                &edit,
                &label_input_focus,
                cx,
            )));
        }
        if let Some((excerpt, include_label, strike_in_clip, correct_only)) = pending_popover {
            workspace =
                workspace.child(v_flex().px_3().pt_3().child(Self::render_selection_popover(
                    &excerpt,
                    include_label,
                    strike_in_clip,
                    correct_only,
                    &selection_popover_focus,
                    cx,
                )));
        }
        // S8: the right-click context menu — pinned above the list like
        // the selection popover, over the range captured when it opened.
        if let Some((start_word, end_word)) = self.context_menu {
            let excerpt = transcript_range_excerpt(state, start_word, end_word);
            let include_label = if self.reel_clips.iter().any(|clip| {
                clip.include && clip.start_word == start_word && clip.end_word == end_word
            }) {
                "Remove from reel"
            } else {
                "Include in reel"
            };
            workspace = workspace.child(v_flex().px_3().pt_3().child(Self::render_context_menu(
                &excerpt,
                include_label,
                &context_menu_focus,
                cx,
            )));
        }
        workspace = workspace.child(workspace_list);
        let inspector = self.inspector_open.then(|| self.render_inspector(cx));
        v_flex()
            .flex_1()
            .min_h_0()
            .child(Self::tab_purpose(
                "The recording's transcript in its asset workspace — select text to highlight and cut, compose the reel; the player stays bound to the recording.",
            ))
            .child(header)
            .when_some(toolbar, |el, toolbar| el.child(toolbar))
            .child(workspace)
            .when_some(inspector, |el, pane| el.child(pane))
            .into_any_element()
    }

    /// One workspace list item (S3d): a fixed header row, one body row of
    /// the active surface (a word block or a visual-transcript event), or
    /// one of the fixed tail sections — highlights, cuts, reel actions on
    /// the word surface; the visual-transcript authoring section on the
    /// events surface.
    fn render_workspace_list_item(
        &self,
        ix: usize,
        blocks: &[PaneBlock],
        events: &[PaneEventRow],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if ix < WORKSPACE_HEADER_ITEMS {
            return self.render_workspace_header_item(ix, cx);
        }
        let body_ix = ix - WORKSPACE_HEADER_ITEMS;
        match self.transcript_surface {
            TranscriptSurface::Words => {
                if let Some(block) = blocks.get(body_ix) {
                    return div()
                        .pb_2()
                        .child(self.render_workspace_block(block, body_ix, cx))
                        .into_any_element();
                }
                match body_ix - blocks.len() {
                    0 => self.render_workspace_highlights_section(cx),
                    1 => self.render_workspace_cuts_section(cx),
                    _ => self.render_workspace_reel_section(cx),
                }
            }
            TranscriptSurface::Events => {
                if let Some(row) = events.get(body_ix) {
                    return div()
                        .pb_1()
                        .child(self.render_workspace_event_row(row, body_ix, cx))
                        .into_any_element();
                }
                // A page shorter than the recording's total events carries
                // the truncation note as the list's bottom row — the count
                // the operator reads never silently claims completeness.
                let tail_ix = body_ix - events.len();
                if tail_ix == 0 && self.visual_events_truncated() {
                    return self.render_workspace_events_truncation_note();
                }
                self.render_workspace_redaction_section(cx)
            }
        }
    }

    /// The workspace list's fixed header items (the composed two-pane fix,
    /// 2026-11-07): the status row (the events surface's load state, or the
    /// words surface's alignment note) and the surface's section label —
    /// read-once status copy, content-like, scrolling with the text. The
    /// player, the chips, and the search input live in the PINNED
    /// interaction band above the list (render_transcript_workspace) —
    /// never virtualized away with the navigated detail.
    fn render_workspace_header_item(&self, ix: usize, _cx: &mut Context<Self>) -> AnyElement {
        let row = match ix {
            0 => {
                let Some(state) = self.transcript.as_ref() else {
                    return div().into_any_element();
                };
                // On the events surface this row carries the event log's
                // own load status — the named no_layers degradation and
                // load failures surface here, never silently (design §8).
                if self.transcript_surface == TranscriptSurface::Events {
                    return match &self.visual_events_request.state {
                        ResourceState::Loading => {
                            Label::new(SharedString::from("Loading visual transcript…"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Muted)
                                .into_any_element()
                        }
                        ResourceState::Failed(error) => Label::new(SharedString::from(format!(
                            "Visual transcript unavailable — {error}"
                        )))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Error)
                        .into_any_element(),
                        _ => match self.visual_events.as_ref() {
                            Some(events) if events.status == "no_layers" => Label::new(
                                SharedString::from(events.note.clone().unwrap_or_else(|| {
                                    "No stored layers for this recording — run a layer pass \
                                     (layers_shot_pass, layers_semantic_pass, layers_audio_pass) \
                                     or author one (layers_store)."
                                        .to_string()
                                })),
                            )
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .into_any_element(),
                            _ => div().into_any_element(),
                        },
                    };
                }
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
                // The section label names the active reading surface: the
                // word pane's review note, or the event log's counts and
                // kinds.
                if self.transcript_surface == TranscriptSurface::Events {
                    let mut row = v_flex();
                    row = row.child(Self::section_label("Visual transcript"));
                    if let Some(events) = self.visual_events.as_ref()
                        && events.status == "completed"
                    {
                        let kinds = events.kinds_present.join(", ");
                        row = row.child(
                            Label::new(SharedString::from(format!(
                                "{} events across: {}",
                                events.events.len(),
                                if kinds.is_empty() { "no kinds" } else { &kinds }
                            )))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .mt_1(),
                        );
                    }
                    return div().pb_2().child(row).into_any_element();
                }
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

    /// The surface and mode chips (the pinned interaction band): the
    /// Words/Events reading-surface toggle, the Select-text mode toggle,
    /// and the Follow toggle (words surface only). Previously list item
    /// 1 of the one-list design; pinned with the player and the search
    /// input so mid-reading affordances never scroll away with the text
    /// (the composed two-pane fix, 2026-11-07).
    fn render_transcript_chips_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let selection_mode = self.selection_mode;
        let following = self.follow_viewport;
        let surface = self.transcript_surface;
        // The reading-surface toggle (operator refinement
        // 2026-10-30): word-transcript pane ⇄ visual-transcript
        // event log — parallel annotations over the same recording;
        // the active surface renders Default, the other Muted.
        let words_chip = div()
            .id("media-transcript-surface-words")
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("Words"))
                    .size(LabelSize::XSmall)
                    .color(if surface == TranscriptSurface::Words {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_transcript_surface(TranscriptSurface::Words, cx);
            }));
        let events_chip = div()
            .id("media-transcript-surface-events")
            .debug_selector(|| "media-transcript-surface-events".into())
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("Events"))
                    .size(LabelSize::XSmall)
                    .color(if surface == TranscriptSurface::Events {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_transcript_surface(TranscriptSurface::Events, cx);
            }));
        // S4 part-2: the select-range mode toggle — an explicit mode
        // beats every-click-seek plus accidental creation. The same
        // mode serves both surfaces: word ranges on the word surface,
        // event spans on the events surface.
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
                } else if surface == TranscriptSurface::Events {
                    "Select span"
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selection_mode = !this.selection_mode;
                this.selection_anchor = None;
                // Toggling the mode abandons any open label editor —
                // it is the tail of a selection this toggle ends —
                // and any pending event span (the same rule on the
                // events surface).
                this.highlight_edit = None;
                this.event_selection_anchor = None;
                this.event_selection_span = None;
                this.status = Some(if this.selection_mode {
                    if surface == TranscriptSurface::Events {
                        "Select mode on — click the first event of the span.".into()
                    } else {
                        "Select mode on — click the first word of the range.".into()
                    }
                } else {
                    "Select mode off — clicks seek playback.".into()
                });
                cx.notify();
            }));
        let mut chips = h_flex().items_center().flex_wrap().gap_2();
        chips = chips
            .child(words_chip)
            .child(events_chip)
            .child(select_chip);
        // The Follow toggle (S3d): viewport tracking is on by
        // default; a manual scroll suspends it and this affordance
        // re-enables it. A word-surface affordance — the mark and
        // reveal target word blocks.
        if surface == TranscriptSurface::Words {
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
            chips = chips.child(follow_chip);
        }
        chips.into_any_element()
    }

    /// The in-transcript search row (S3): the lexical input with its live
    /// count, pinned in the interaction band on the words surface only
    /// (the events log has its own reading model). Previously list item 2
    /// of the one-list design, which kept a placeholder row on the events
    /// surface for reading-position stability — pinning removes that
    /// hack: the row is never a list item, and the operator reaches for
    /// the search mid-reading, where the one-list design had already
    /// scrolled it away.
    fn render_transcript_search_row(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(focus) = self.transcript_search_focus.clone() {
            let search = &self.transcript_search;
            let typed = search.query.text();
            let cursor = search.query.cursor();
            let before: String = typed.chars().take(cursor).collect();
            let after: String = typed.chars().skip(cursor).collect();
            let mut input = div()
                .id("media-transcript-search-input")
                .track_focus(&focus)
                .flex()
                .items_center()
                .min_w_0()
                .w(px(160.))
                .overflow_hidden()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().colors().border)
                .px_1()
                .debug_selector(|| "media-transcript-search-input".into())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    let keystroke = &event.keystroke;
                    if (keystroke.modifiers.control || keystroke.modifiers.platform)
                        && keystroke.key == "v"
                    {
                        this.paste_into_transcript_search(cx);
                    } else {
                        this.handle_transcript_search_key(keystroke, cx);
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
                    Label::new(SharedString::from("Search transcript"))
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
                            .id("media-transcript-search-caret")
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
            let has_words = self
                .transcript
                .as_ref()
                .is_some_and(|state| state.words.is_some());
            h_flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(input)
                .when_some(
                    transcript_search_status(&self.transcript_search, has_words),
                    |row, status| {
                        row.child(
                            div()
                                .id("media-transcript-search-status")
                                .debug_selector(|| "media-transcript-search-status".into())
                                .child(
                                    Label::new(SharedString::from(status))
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Muted),
                                ),
                        )
                    },
                )
                .into_any_element()
        } else {
            // Unreachable in practice — the workspace render creates
            // the handle before the list renders — but a render path
            // never unwraps.
            div().into_any_element()
        }
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
        // The in-progress drag's anchor and release-target words carry
        // the same mark (S2): live feedback for the press-and-release
        // selection — the target mark vanishes over word gaps, showing
        // why a release there selects nothing.
        let drag_anchor = self.word_drag_anchor;
        let drag_target = self.drag_last_word.filter(|_| drag_anchor.is_some());
        for drag_mark in [drag_anchor, drag_target] {
            if let Some(word) = drag_mark
                && let Some(local) = block
                    .word_starts
                    .iter()
                    .position(|(index, _)| *index == word)
            {
                highlights.push((block.word_char_ranges[local].clone(), highlight_style));
            }
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
        // S3: search matches paint after the researcher overlays so a match
        // stays visible inside a highlight, and the field-wise composition
        // keeps a struck match's line-through; the current match paints over
        // the plain match. Marks render on every words view — the search is
        // working-view-local, never source-mapped.
        let search_match_style = HighlightStyle {
            background_color: Some(cx.theme().colors().element_hover),
            ..Default::default()
        };
        let search_current_style = HighlightStyle {
            background_color: Some(cx.theme().colors().element_active),
            ..Default::default()
        };
        for (index, (start_word, end_word)) in self.transcript_search.matches.iter().enumerate() {
            if let Some(range) = block_word_char_range(block, *start_word, *end_word) {
                let style = if index == self.transcript_search.current {
                    search_current_style
                } else {
                    search_match_style
                };
                highlights.push((range, style));
            }
        }
        let word_starts = block.word_starts.clone();
        let entity = cx.entity().downgrade();
        // S2: exact hover tracking for the press-and-release selection —
        // map the hovered character to its word; characters outside the
        // word ranges (gaps) read as None so a release there cancels the
        // pending drag instead of selecting from a stale position. The
        // state tracks always (the press needs the current word); the
        // repaint fires only while a drag is pending.
        let hover_entity = entity.clone();
        let hover_ranges = block.word_char_ranges.clone();
        let interactive = InteractiveText::new(
            SharedString::from(format!("media-transcript-paragraph-{block_index}")),
            StyledText::new(block.text.clone())
                .with_highlights(resolved_highlight_runs(highlights)),
        )
        .on_hover(move |hovered, _event, _window, cx| {
            let hovered_word = hovered.and_then(|char_ix| {
                hover_ranges
                    .iter()
                    .position(|range| range.contains(&char_ix))
            });
            hover_entity
                .update(cx, |viewer, cx| {
                    if viewer.drag_last_word != hovered_word {
                        viewer.drag_last_word = hovered_word;
                        if viewer.word_drag_anchor.is_some() {
                            cx.notify();
                        }
                    }
                })
                .ok();
        })
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
            .debug_selector(|| format!("media-transcript-block-{block_index}"))
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
                // S6: membership reads the clip's include flag — the ONE
                // source of truth. Safe on re_anchored views too: the flag
                // keys the stored highlight's own source anchors, never a
                // working selection.
                let included = self.reel_clips.iter().any(|clip| {
                    clip.annotation_id == annotation_id
                        && clip.start_word == start_word
                        && clip.end_word == end_word
                        && clip.include
                });
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
                    .on_click({
                        let toggle_annotation_id = annotation_id.clone();
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_reel_clip_include(
                                &toggle_annotation_id,
                                start_word,
                                end_word,
                                cx,
                            );
                        })
                    });
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

    /// The reel tail section (S5/S6): the reel composer's surface — the
    /// ordered clip list with per-clip reorder, include toggle, duration
    /// readout, and in-reel strikes with their Restore — plus the preview
    /// toggle (player-local, no admission) and the plain-language compose
    /// actions. Visible only when their inputs exist.
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
        let any_included_clip = self.reel_clips.iter().any(|clip| clip.include);
        let mut section = v_flex().gap_2();
        if affordances.iter().any(|(_, visible)| *visible)
            || !self.reel_clips.is_empty()
            || any_included_clip
        {
            section = section.child(Self::section_label("Compose a reel"));
        }
        // The composed clip list (S6): order, inclusion, and in-reel strikes,
        // editable in place. Clip rows carry the composed position, label,
        // word range, and duration (the baked span total over the source
        // bundle words); the ↑/↓ buttons reorder, the include toggle flips
        // membership, and each in-reel strike restores individually.
        let clip_count = self.reel_clips.len();
        for (clip_index, clip) in self.reel_clips.iter().enumerate() {
            let row = format!(
                "{}. {} — words {}–{}",
                clip_index + 1,
                clip.label,
                clip.start_word,
                clip.end_word,
            );
            let duration = reel_clip_duration_ms(state, clip);
            let mut row_element = div()
                .id(SharedString::from(format!(
                    "media-transcript-reel-clip-{clip_index}"
                )))
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Label::new(SharedString::from(row))
                        .size(LabelSize::XSmall)
                        .color(if clip.include {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                );
            if let Some(duration_ms) = duration {
                row_element = row_element.child(
                    Label::new(SharedString::from(format!(
                        "{:.1}s",
                        duration_ms as f64 / 1000.0
                    )))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
                );
            }
            section = section.child(row_element);

            let mut controls = div()
                .id(SharedString::from(format!(
                    "media-transcript-reel-clip-controls-{clip_index}"
                )))
                .flex()
                .flex_wrap()
                .gap_2();
            if clip_index > 0 {
                controls = controls.child(
                    div()
                        .id(SharedString::from(format!(
                            "media-transcript-reel-clip-up-{clip_index}"
                        )))
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("↑"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.move_reel_clip(clip_index, true, cx);
                        })),
                );
            }
            if clip_index + 1 < clip_count {
                controls = controls.child(
                    div()
                        .id(SharedString::from(format!(
                            "media-transcript-reel-clip-down-{clip_index}"
                        )))
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("↓"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.move_reel_clip(clip_index, false, cx);
                        })),
                );
            }
            let annotation_id = clip.annotation_id.clone();
            let (start_word, end_word) = (clip.start_word, clip.end_word);
            controls = controls.child(
                div()
                    .id(SharedString::from(format!(
                        "media-transcript-reel-toggle-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if clip.include {
                            "Included in reel"
                        } else {
                            "Add to reel"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if clip.include {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_reel_clip_include(&annotation_id, start_word, end_word, cx);
                    })),
            );
            section = section.child(controls);

            for (strike_index, (strike_start, strike_end)) in clip.strikes.iter().enumerate() {
                section = section.child(
                    div()
                        .id(SharedString::from(format!(
                            "media-transcript-reel-clip-strike-{clip_index}-{strike_index}"
                        )))
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Label::new(SharedString::from(format!(
                                "Struck words {strike_start}–{strike_end}"
                            )))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Error),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-transcript-reel-clip-restore-{clip_index}-{strike_index}"
                                )))
                                .cursor_pointer()
                                .hover(|el| el.bg(cx.theme().colors().element_hover))
                                .child(
                                    Label::new(SharedString::from("Restore"))
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Default),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_reel_clip_strike(clip_index, strike_index, cx);
                                })),
                        ),
                );
            }
        }
        // The preview toggle (S6): player-local — the seek paths' class, no
        // admission. The auto-seek chain plays the composed spans on the
        // shared player without rendering.
        if any_included_clip {
            let previewing = self.reel_preview.is_some();
            section = section.child(
                div()
                    .id("media-transcript-reel-preview-toggle")
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if previewing {
                            "Stop preview"
                        } else {
                            "Preview reel"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if previewing {
                            ui::Color::Error
                        } else {
                            ui::Color::Default
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_reel_preview(cx);
                    })),
            );
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

    /// One visual-transcript event row (S5): kind, source-PTS range,
    /// searchable text, and the provenance model — the row seeks playback
    /// to its start on click (the word pane's click-to-seek discipline,
    /// criterion 1). Box-carrying rows carry the Target arm chip (criterion
    /// 3's "select target"); the anchored row and rows inside the pending
    /// span highlight.
    fn render_workspace_event_row(
        &self,
        row: &PaneEventRow,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let time = match row.end_sec {
            Some(end) => format!("{:.1}s–{:.1}s", row.start_sec, end),
            None => format!("{:.1}s", row.start_sec),
        };
        let anchored = self.event_selection_anchor == Some(index);
        let in_span = self
            .event_selection_span
            .is_some_and(|(start, end)| row.start_sec < end && row.end_or_start() > start);
        let row_id = SharedString::from(format!("media-event-row-{index}"));
        let mut element = div()
            .id(row_id)
            .debug_selector(move || format!("media-event-row-{index}"))
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .when(anchored || in_span, |el| {
                el.bg(cx.theme().colors().element_hover)
            })
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from(row.kind.clone()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(time))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(row.searchable_text.clone()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Default),
            )
            .child(
                Label::new(SharedString::from(format!("· {}", row.model)))
                    .size(LabelSize::XSmall)
                    .color(if row.model == "user" {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            );
        // The S1 refinement's read-time corroboration badges: the sensory
        // streams that confirm this event (the door slam "heard" AND
        // "seen" renders both). Derived server-side at read time; a
        // human-confirmed corroboration can be authored as a researcher
        // item.
        for badge in &row.evidence {
            element = element.child(
                Label::new(SharedString::from(format!("· {}", badge)))
                    .size(LabelSize::XSmall)
                    .color(if badge == "authored" {
                        ui::Color::Muted
                    } else {
                        ui::Color::Accent
                    }),
            );
        }
        // The redaction target arm: only rows carrying a normalized box
        // (machine object tags — the tier-1 spatial source). Arming stops
        // propagation so the click does not also seek.
        if let Some(_box_norm) = row.box_norm {
            let armed = self
                .redaction_target
                .as_ref()
                .is_some_and(|target| target.source_ref == row.source_ref());
            let target_chip = div()
                .id(SharedString::from(format!("media-event-target-{index}")))
                .cursor_pointer()
                .flex()
                .flex_wrap()
                .gap_2()
                .hover(|el| el.bg(cx.theme().colors().element_active))
                .child(
                    Label::new(SharedString::from(if armed {
                        "◎ Target"
                    } else {
                        "Target"
                    }))
                    .size(LabelSize::XSmall)
                    .color(if armed {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
                )
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.arm_redaction_target(index, cx);
                }));
            element = element.child(target_chip);
        }
        let start_sec = row.start_sec;
        element
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.event_clicked(index, start_sec, cx);
            }))
            .into_any_element()
    }

    /// Whether the loaded event log is a truncated page: the server pages
    /// `layers_transcript` (the panel requests the 5000-event maximum),
    /// so a longer recording's log reads as a prefix of its total.
    fn visual_events_truncated(&self) -> bool {
        self.visual_events
            .as_ref()
            .is_some_and(VisualEventsState::truncated)
    }

    /// The truncation note at the bottom of the event list: a page
    /// shorter than the recording's total names what it omits — page vs
    /// total, never a bare count that silently claims completeness.
    fn render_workspace_events_truncation_note(&self) -> AnyElement {
        let Some(state) = self.visual_events.as_ref() else {
            return div().into_any_element();
        };
        div()
            .pb_2()
            .debug_selector(|| "media-events-truncation-note".into())
            .child(
                Label::new(SharedString::from(state.truncation_note()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .into_any_element()
    }

    /// The events surface's tail section (S5): the pending span, the
    /// select-to-author tag flow (criterion 2), the redaction dispatch
    /// (criterion 3 — armed target + effect + span → author the approved
    /// decision and render it), and the last render's review/lineage lines
    /// (S3's segments[] + applied_items[]).
    fn render_workspace_redaction_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut section = v_flex()
            .gap_2()
            .debug_selector(|| "media-events-authoring-section".into());
        section = section.child(Self::section_label("Visual-transcript actions"));
        // The pending span: what both authoring flows consume.
        match self.event_selection_span {
            Some((start, end)) => {
                let span_label = Label::new(SharedString::from(format!(
                    "Selected span: {start:.1}s–{end:.1}s"
                )))
                .size(LabelSize::XSmall)
                .color(ui::Color::Default);
                let tag_chip = div()
                    .id("media-event-tag-span")
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Tag span"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.open_event_span_tag_editor(window, cx);
                    }));
                section = section.child(
                    h_flex()
                        .items_center()
                        .flex_wrap()
                        .gap_2()
                        .child(span_label)
                        .child(tag_chip),
                );
            }
            None => {
                let hint = if self.selection_mode {
                    "Click two events to select a span."
                } else {
                    "Turn on Select span, then click two events to select a span."
                };
                section = section.child(
                    Label::new(SharedString::from(hint))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                );
            }
        }
        // The redaction block: armed target, effect cycle, dispatch.
        let mut redaction = v_flex().gap_1();
        redaction = redaction.child(
            Label::new(SharedString::from("Redaction"))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
        );
        let target_line = match self.redaction_target.as_ref() {
            Some(target) => format!(
                "Target: {} (box {:.2},{:.2} {:.0}%×{:.0}%)",
                target.label,
                target.box_norm[0],
                target.box_norm[1],
                target.box_norm[2] * 100.0,
                target.box_norm[3] * 100.0
            ),
            None => "No target armed — click Target on an event with a box.".to_string(),
        };
        redaction = redaction.child(
            Label::new(SharedString::from(target_line))
                .size(LabelSize::XSmall)
                .color(if self.redaction_target.is_some() {
                    ui::Color::Default
                } else {
                    ui::Color::Muted
                }),
        );
        let effect = self.redaction_effect;
        let effect_chip = div()
            .id("media-event-redaction-effect")
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from(format!("Effect: {}", effect.as_str())))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Default),
            )
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.redaction_effect = this.redaction_effect.next();
                cx.notify();
            }));
        let ready = self.redaction_target.is_some() && self.event_selection_span.is_some();
        let redact_chip = div()
            .id("media-event-redact-span")
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("Redact span"))
                    .size(LabelSize::XSmall)
                    .color(if ready {
                        ui::Color::Error
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.dispatch_redaction_render(cx);
            }));
        redaction = redaction.child(
            h_flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(effect_chip)
                .child(redact_chip),
        );
        section = section.child(redaction);
        // The last render's review/lineage lines (S3's result rendered for
        // review — criterion 3).
        if let Some(summary) = self.redaction_render.as_ref() {
            let mut render = v_flex().gap_1();
            render = render.child(
                Label::new(SharedString::from(format!(
                    "Last render — {} decision(s) · {} re-encoded · {} stream-copied · layer {}",
                    summary.decisions_rendered,
                    summary.reencoded_segments,
                    summary.stream_copied_segments,
                    summary.decision_layer_id
                )))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            );
            for segment in &summary.segments {
                render = render.child(
                    Label::new(SharedString::from(format!(
                        "  {} {:.1}s–{:.1}s ({} item(s))",
                        segment.mode, segment.start_sec, segment.end_sec, segment.items
                    )))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
                );
            }
            for applied in &summary.applied {
                render = render.child(
                    Label::new(SharedString::from(format!(
                        "  {} {}×{} at {},{} over {:.1}s–{:.1}s",
                        applied.effect,
                        applied.pixel_w,
                        applied.pixel_h,
                        applied.pixel_x,
                        applied.pixel_y,
                        applied.local_start_sec,
                        applied.local_end_sec
                    )))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
                );
            }
            section = section.child(render);
        }
        section.into_any_element()
    }

    /// One stored-transcript row (S3a, T3): the recording's name and
    /// metadata; a click opens the workspace's transcript sub-surface.
    /// Rendered nested beneath its recording's Library row, and in the
    /// unlinked section below the rows. The Delete affordance is a SIBLING
    /// of the clickable open area (the Library row pattern), so opening and
    /// deleting can never fire together. Two clicks: Delete arms the
    /// confirmation, Confirm delete dispatches the server's
    /// `educt_delete_transcript` (durable exports and rendered reels are
    /// preserved by the server, detached from the removed record).
    fn render_transcript_row(&self, row_index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(listing) = self.transcript_listing.as_ref() else {
            return div().into_any_element();
        };
        let Some(transcript) = listing.get(row_index) else {
            return div().into_any_element();
        };
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
        if let Some(note) = transcript_alignment_note(&transcript.alignment) {
            row = row.child(
                Label::new(SharedString::from(note))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error),
            );
        }
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
            .child(delete_button)
            .into_any_element()
    }

    /// The per-recording transcription status (R2/R5): a live
    /// transcribe job whose source is the recording shows Transcribing
    /// (the observed-anywhere live set — a deep job counts too, T2); a
    /// stored transcript linked to the recording shows Transcribed;
    /// otherwise Not transcribed.
    fn recording_transcription_status(&self, asset: &MediaAsset) -> RecordingTranscriptionStatus {
        let running = self
            .live_jobs
            .iter()
            .any(|job| job.source.as_deref() == Some(asset.src.as_str()));
        if running {
            return RecordingTranscriptionStatus::Transcribing;
        }
        let recording_id = asset.gallery_asset_id.as_deref();
        let stored = self.transcript_listing.as_ref().is_some_and(|listing| {
            listing
                .iter()
                .any(|row| row.gallery_asset_id.as_deref() == recording_id)
        });
        if stored {
            RecordingTranscriptionStatus::Transcribed
        } else {
            RecordingTranscriptionStatus::NotTranscribed
        }
    }

    /// The ambient activity badge for one Library row (T2): the verb of
    /// a live research job whose primary source is this asset —
    /// transient progress (HIG Progress indicators: present only while
    /// an operation is ongoing, never error-styled). `None` when no
    /// live job targets the asset. The job record carries its primary
    /// `source` (the transcribe input, the trim/clip source); for concat
    /// jobs only the first input badges — the server exposes no wider
    /// subject list on the record.
    fn asset_activity_verb(&self, asset: &MediaAsset) -> Option<&'static str> {
        self.live_jobs
            .iter()
            .find(|job| job.source.as_deref() == Some(asset.src.as_str()))
            .map(|job| job_op_verb(&job.op))
    }

    /// The Transcribe affordance (R5, operator ruling 2026-10-28):
    /// submit the async transcribe job against the recording's source,
    /// storing and linking the transcript on completion — the operator
    /// can take a recording from import to transcript to reel entirely
    /// in the panel.
    fn transcribe_recording(
        &mut self,
        gallery_asset_id: String,
        source: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot transcribe.".into());
            cx.notify();
            return;
        };
        self.status = Some("Transcription queued — the Activity drawer tracks it.".into());
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "job_submit",
            serde_json::json!({
                "op": "transcribe",
                "params": serde_json::json!({
                    "audio_url": source,
                    "gallery_asset_id": gallery_asset_id,
                })
                .to_string(),
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(_text) => Ok(()),
                Err(error) => Err(format!("transcription submit failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.status = Some(error);
                }
                this.load_jobs(cx);
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn section_label(title: &str) -> impl IntoElement + use<> {
        Label::new(SharedString::from(title.to_string()))
            .size(LabelSize::XSmall)
            .color(ui::Color::Hint)
            .mt_1()
    }

    /// Honest absence copy for one inspector tab (T1): a pane-internal tab
    /// always renders something — the stacked Detail view could hide an
    /// empty section, a tab cannot.
    fn inspector_absent(text: &str) -> Div {
        div().px_3().child(
            Label::new(SharedString::from(text.to_string()))
                .size(LabelSize::XSmall)
                .color(ui::Color::Hint),
        )
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
            let gallery_asset_id = object
                .get("gallery_asset_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string);
            Ok(TranscriptSummaryRow {
                id: id.to_string(),
                media_path: media_path.to_string(),
                words_count,
                duration_secs,
                has_word_timings,
                alignment,
                gallery_asset_id,
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

/// The reel-provenance line for the inspector's Reel tab, from the detail payload's
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
    /// The click completed a range — open the selection popover over
    /// [start, end] (the alignment gate runs before the popover opens; the
    /// popover's Highlight action opens the label editor).
    CompleteRange { start_word: usize, end_word: usize },
}

/// The press-and-release drag outcome (S2) — the one-gesture selection
/// path, active in both interaction modes. Mirrors
/// [`WordClickOutcome`]'s pure, pin-testable shape; the alignment gate
/// applies at the opening site (`word_drag_released`), not here.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WordDragOutcome {
    /// The label editor is open — the drag is inert (the editor is modal
    /// to its range).
    Inert,
    /// Press and release landed on the same word — the plain click path
    /// seeks (InteractiveText fires its click only for same-range
    /// down/up, so no selection is pending).
    Click,
    /// Press and release spanned words — open the selection popover over
    /// [start, end] (the alignment gate runs before the popover opens; the
    /// popover's Highlight action opens the label editor).
    CompleteRange { start_word: usize, end_word: usize },
}

/// The label editor's open target (S4-3): a completed two-click range
/// awaiting its label, a researcher highlight being renamed (the range and
/// note pass through `educt_annotation_update` unchanged — only the label
/// is replaced), a completed visual-transcript span awaiting its tag name
/// (S5 criterion 2 — dispatched through `layers_store`, kind
/// `researcher`, model `"user"`), or a completed selection awaiting its
/// correction text (the transcript-layer S5 — dispatched through
/// `educt_store_layer`, kind correction, then chained
/// `educt_realign_transcript`).
#[derive(Clone, Debug)]
enum HighlightEditKind {
    Create {
        start_word: usize,
        end_word: usize,
    },
    Correction {
        start_word: usize,
        end_word: usize,
    },
    Rename {
        annotation_id: String,
        start_word: usize,
        end_word: usize,
        note: String,
    },
    EventTag {
        start_sec: f64,
        end_sec: f64,
    },
}

/// The open label editor (S4-3): its target and the typed text.
#[derive(Clone, Debug)]
struct HighlightEdit {
    kind: HighlightEditKind,
    buffer: LabelBuffer,
}

/// S3: in-transcript search state — the typed query, the ordered match list
/// over working word indices (each match the inclusive word range it
/// covers), and the current match the Enter cycle sits on. The rescan is
/// debounced; Enter/Shift-Enter cycle and jump. Blank query → no matches →
/// no marks and no count.
#[derive(Clone, Debug, Default)]
struct TranscriptSearch {
    query: LabelBuffer,
    matches: Vec<(usize, usize)>,
    current: usize,
}

/// A single-line text buffer with a cursor — the model behind the panel's
/// inline inputs (the label editor and the transcript search). Char-indexed
/// so multi-byte text edits as characters, not bytes.
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

/// The completed selection's tray mode (D5 v2, the transcript-layer S5):
/// which action set a completed word range may offer on this view.
/// Original/aligned views are index-identical to the source, so every
/// tray action is available; a re_anchored view opens the CORRECTION
/// tray — the one action set whose dispatch translates the working
/// selection to source indices through the server (`educt_locate` basis
/// "source"), never a panel-side projection. The unselectable views keep
/// the visible refusal (the panel never re-implements the server's
/// projection — D5: index-identical-only actions stay unavailable on
/// corrected views rather than anchoring working indices to source-word
/// tools).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectionTrayMode {
    /// Highlight / Cut / Copy time / Suggest correction / Include in reel.
    Full,
    /// Suggest correction + Copy time — the translation-backed set.
    CorrectOnly,
}

fn selection_tray_mode(alignment: &str) -> Result<SelectionTrayMode, &'static str> {
    match alignment {
        "original" | "aligned" => Ok(SelectionTrayMode::Full),
        "re_anchored" => Ok(SelectionTrayMode::CorrectOnly),
        "unaligned" => Err(
            "Direct selection needs a timing-aligned transcript. Re-transcribe or realign this correction first.",
        ),
        "untimed" => {
            Err("Direct selection needs word-level timings; this transcript is text only.")
        }
        _ => Err("Direct selection is unavailable for this transcript."),
    }
}

/// The authoring-path reason every researcher correction carries (S5) —
/// the correction edit's bookkeeping trail, naming where the layer came
/// from (the layer's own provenance names the authoring path: model
/// "user" + the researcher_correction template).
const CORRECTION_REASON: &str = "researcher correction from the transcript workspace selection";

/// Convert a pane correction edit to the server's wire type — one home
/// for the field mapping, so the merge and the store request cannot
/// disagree.
fn server_correction_edit(
    edit: &PaneCorrectionEdit,
) -> hkask_mcp_media::transcript_layers::CorrectionEdit {
    hkask_mcp_media::transcript_layers::CorrectionEdit {
        start_word: edit.start_word,
        end_word: edit.end_word,
        replacement: edit.replacement.clone(),
        reason: edit.reason.clone(),
    }
}

/// Convert the server's wire edit back to the pane type (the merge
/// output).
fn pane_correction_edit(
    edit: &hkask_mcp_media::transcript_layers::CorrectionEdit,
) -> PaneCorrectionEdit {
    PaneCorrectionEdit {
        start_word: edit.start_word,
        end_word: edit.end_word,
        replacement: edit.replacement.clone(),
        reason: edit.reason.clone(),
    }
}

/// The correction store request (S5): the server's own layer types
/// serialize the layer — the schema cannot drift from the contract —
/// under the researcher authoring path. `created_at` is injected so the
/// fixture pins the wire shape byte-for-byte.
fn correction_store_params(
    transcript_id: &str,
    edits: &[PaneCorrectionEdit],
    created_at: &str,
) -> Result<serde_json::Value, String> {
    let layer = hkask_mcp_media::transcript_layers::TranscriptLayer::Correction(
        hkask_mcp_media::transcript_layers::CorrectionLayer {
            provenance: hkask_mcp_media::transcript_layers::LayerProvenance {
                model: "user".to_string(),
                prompt_template: "researcher_correction".to_string(),
                created_at: created_at.to_string(),
            },
            edits: edits.iter().map(server_correction_edit).collect(),
        },
    );
    let layer_value = serde_json::to_value(&layer)
        .map_err(|error| format!("the correction layer failed to serialize: {error}"))?;
    Ok(serde_json::json!({
        "transcript_id": transcript_id,
        "layer": layer_value,
    }))
}

/// The realign request (S5): names the stored correction layer.
fn correction_realign_params(transcript_id: &str, layer_id: &str) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "layer_id": layer_id,
    })
}

/// The locate request (S5): the selection's rendered quote under the
/// source basis — the server converts through the re-anchored index map.
fn correction_locate_params(transcript_id: &str, quote: &str) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "text": quote,
        "basis": "source",
    })
}

/// Parse the locate response for the selection's SOURCE range: the
/// candidate whose WORKING range is exactly the selection — locate
/// returns every match with its working range alongside the converted
/// source range, so the translation is deterministic, never a guess.
fn parse_correction_source_range(
    response: &str,
    start_word: usize,
    end_word: usize,
) -> Result<(usize, usize), String> {
    let value = hkask_types::tool_response::parse_tool_response(response)
        .ok_or_else(|| "educt_locate returned no parsable data.".to_string())?;
    let ranges = value
        .get("ranges")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "educt_locate returned no ranges array.".to_string())?;
    let candidate = ranges
        .iter()
        .find(|range| {
            range.get("working_start_word").and_then(Value::as_u64) == Some(start_word as u64)
                && range.get("working_end_word").and_then(Value::as_u64) == Some(end_word as u64)
        })
        .ok_or_else(|| {
            "The selection no longer matches the transcript — reload the workspace and reselect."
                .to_string()
        })?;
    let source_start = candidate
        .get("start_word")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| "the located range has no source start_word.".to_string())?;
    let source_end = candidate
        .get("end_word")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| "the located range has no source end_word.".to_string())?;
    Ok((source_start, source_end))
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

/// S6: the composed reel's ordered keep ranges — ONE home for the op walk
/// the compose request and the preview plan share. For each included clip in
/// composer order: the clip's source range minus its own in-reel strikes and
/// minus every word an earlier included clip already played (the selection
/// algebra admits each source span once — overlapping Keep ops are rejected
/// — so a later clip that overlaps an earlier one repeats only its unplayed
/// remainder). Both subtractions run through the server's published helpers
/// (union_ranges + subtract_cuts_from_range — the algebra's own difference
/// math, never a panel fork; the panel's former union_word_ranges mirror is
/// deleted).
fn composed_keep_ranges(clips: &[ReelClip]) -> Vec<hkask_mcp_media::transcript_select::WordRange> {
    use hkask_mcp_media::transcript_select::{WordRange, subtract_cuts_from_range, union_ranges};
    let mut played: Vec<WordRange> = Vec::new();
    let mut keeps: Vec<WordRange> = Vec::new();
    for clip in clips.iter().filter(|clip| clip.include) {
        let base = WordRange::new(clip.start_word, clip.end_word);
        let mut cuts: Vec<WordRange> = clip
            .strikes
            .iter()
            .map(|(start, end)| WordRange::new(*start, *end))
            .collect();
        cuts.extend(played.iter().copied());
        let pieces = subtract_cuts_from_range(base, &union_ranges(&cuts));
        played.extend(pieces.iter().copied());
        keeps.extend(pieces);
    }
    keeps
}

/// The pane's source bundle words as the server's timed-word wire type (S6)
/// — one home for the field mapping, so every word→time path (the preview
/// plan, the duration readout) maps identically to the render path.
fn source_timed_words(state: &TranscriptPaneState) -> Vec<hkask_mcp_media::transcript::TimedWord> {
    state
        .source_words
        .iter()
        .map(|word| hkask_mcp_media::transcript::TimedWord {
            word: word.word.clone(),
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            confidence: None,
        })
        .collect()
}

/// The preview's span plan (S6): the composed clip list mapped to media
/// spans through the render path's own function over the stored SOURCE
/// bundle words (`edl_to_clip_plan` — the word→time mapping the render
/// uses), so preview and rendered reel agree on `re_anchored` views, where
/// the working view's timings are interpolated estimates over different
/// indices. Honest refusals, never silent: no included clips, or a source
/// with no word timings.
fn reel_preview_plan(
    state: &TranscriptPaneState,
    clips: &[ReelClip],
) -> Result<Vec<(u64, u64)>, &'static str> {
    let keep_ranges = composed_keep_ranges(clips);
    if keep_ranges.is_empty() {
        return Err("Nothing to preview — include at least one clip in the reel first.");
    }
    let words = source_timed_words(state);
    if words.is_empty() {
        return Err("Reel preview needs the recording's source word timings.");
    }
    let edl = hkask_mcp_media::transcript_select::Edl {
        ops: keep_ranges
            .into_iter()
            .map(|range| hkask_mcp_media::transcript_select::EdlEntry {
                range,
                op: hkask_mcp_media::transcript_select::EdlOp::Keep,
            })
            .collect(),
    };
    hkask_mcp_media::transcript_select::edl_to_clip_plan(&words, &edl)
        .map_err(|_| "Reel preview needs the recording's source word timings.")
}

/// One clip's composed duration over the source bundle words (S6's duration
/// readout): the time-span total of the clip's strike-subtracted pieces —
/// what the reel will actually play for this clip. `None` when the source
/// carries no word timings (the readout degrades honestly).
fn reel_clip_duration_ms(state: &TranscriptPaneState, clip: &ReelClip) -> Option<u64> {
    use hkask_mcp_media::transcript_select::{
        WordRange, subtract_cuts_from_range, union_ranges, word_range_to_time_range,
    };
    let words = source_timed_words(state);
    if words.is_empty() {
        return None;
    }
    let cuts: Vec<WordRange> = clip
        .strikes
        .iter()
        .map(|(start, end)| WordRange::new(*start, *end))
        .collect();
    let pieces = subtract_cuts_from_range(
        WordRange::new(clip.start_word, clip.end_word),
        &union_ranges(&cuts),
    );
    let mut total = 0;
    for piece in pieces {
        let (start_ms, end_ms) = word_range_to_time_range(&words, piece).ok()?;
        total += end_ms - start_ms;
    }
    Some(total)
}

/// The event-click state machine's outcome (S5, mirroring the word pane's
/// S4 part-2 model).
#[derive(Clone, Copy, Debug, PartialEq)]
enum EventClickOutcome {
    /// Selection mode is off — the click seeks.
    SeekOnly,
    /// The click set the pending span's anchor.
    AnchorSet(usize),
    /// The click repeated the anchor — the pending selection cleared.
    AnchorCleared,
    /// The label editor is open — the click is inert.
    EditorOpen,
    /// The click completed a span — [start_sec, end_sec] in source-PTS
    /// seconds, pending for both authoring flows.
    SpanSelected { start_sec: f64, end_sec: f64 },
    /// The two clicked events cover no time (or a stale anchor no longer
    /// resolves) — the pending selection clears with a named refusal.
    SpanRejected,
}

/// The redaction chain's success payload: the raw render response text
/// (for display-hint ingestion — the rendered asset lands as a playable
/// panel asset) plus its strict-parsed review/lineage summary.
struct RedactionRenderOutcome {
    render_text: String,
    summary: RedactionRenderSummary,
}

/// The exact `layers_store` request for a select-to-authored researcher
/// tag (S5 criterion 2): kind `researcher`, provenance model `"user"` — a
/// human act, never machine-authored (design §3) — one item over the
/// selected span whose searchable text keys the label for `layers_search`.
fn researcher_tag_params(
    media_path: &str,
    gallery_asset_id: Option<&str>,
    start_sec: f64,
    end_sec: f64,
    label: &str,
) -> serde_json::Value {
    let mut params = serde_json::json!({
        "media_path": media_path,
        "kind": "researcher",
        "label": label,
        "provenance": {
            "model": "user",
            "prompt_template": "media-panel:select-to-author",
        },
        "items": [{
            "start_sec": start_sec,
            "end_sec": end_sec,
            "searchable_text": format!("tag: {label}"),
            "payload": { "label": label },
        }],
    });
    if let Some(asset_id) = gallery_asset_id {
        params["gallery_asset_id"] = serde_json::json!(asset_id);
    }
    params
}

/// The exact `layers_store` request for an approved redaction decision
/// (S5 criterion 3, the S3 contract): kind `researcher`, model `"user"`,
/// one APPROVED item carrying the redaction vocabulary — op redact,
/// status approved, effect, the armed target's box, and the source-tag
/// lineage — over the selected span. Any unapproved item fails the whole
/// layer's render, so the panel authors approved decisions only.
fn redaction_decision_params(
    media_path: &str,
    gallery_asset_id: Option<&str>,
    target: &RedactionTarget,
    span: (f64, f64),
    effect: PaneRedactionEffect,
) -> serde_json::Value {
    let [x, y, w, h] = target.box_norm;
    let mut params = serde_json::json!({
        "media_path": media_path,
        "kind": "researcher",
        "label": "redaction decisions",
        "provenance": {
            "model": "user",
            "prompt_template": "media-panel:select-to-author",
        },
        "items": [{
            "start_sec": span.0,
            "end_sec": span.1,
            "searchable_text": format!("redact: {} ({})", target.label, effect.as_str()),
            "payload": {
                "op": "redact",
                "status": "approved",
                "effect": effect.as_str(),
                "target": {
                    "kind": "box",
                    "box": { "x": x, "y": y, "w": w, "h": h },
                },
                "source_tag_refs": [target.source_ref],
            },
        }],
    });
    if let Some(asset_id) = gallery_asset_id {
        params["gallery_asset_id"] = serde_json::json!(asset_id);
    }
    params
}

/// Strict-parse a `layers_transcript` response (S1's reading surface) into
/// the event log the panel renders: status, the time-ordered events, the
/// total, and the kinds present. Contract fields reject the whole payload
/// (a malformed event log never degrades to a partial success); the
/// optional object box is parsed tolerantly because payloads are arbitrary
/// JSON by contract (design §3) — a malformed box renders its row without
/// the target affordance instead of rejecting the log.
fn parse_visual_events(payload: &Value) -> Result<VisualEventsState, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "layers_transcript returned no object data.".to_string())?;
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "layers_transcript returned no status.".to_string())?;
    let note = object
        .get("note")
        .and_then(Value::as_str)
        .map(str::to_string);
    let events = object
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| "layers_transcript returned no events array.".to_string())?;
    match status {
        "no_layers" => {
            if !events.is_empty() {
                return Err(
                    "layers_transcript reported no_layers with nonempty events.".to_string()
                );
            }
            Ok(VisualEventsState {
                status: status.to_string(),
                events: Vec::new(),
                events_total: 0,
                kinds_present: Vec::new(),
                note,
            })
        }
        "completed" => {
            let mut rows = Vec::with_capacity(events.len());
            for event in events {
                let event = event
                    .as_object()
                    .ok_or_else(|| "visual-transcript event is not an object.".to_string())?;
                let start_sec = event
                    .get("start_sec")
                    .and_then(Value::as_f64)
                    .filter(|secs| secs.is_finite() && *secs >= 0.0)
                    .ok_or_else(|| {
                        "visual-transcript event has no nonnegative start_sec.".to_string()
                    })?;
                let end_sec =
                    match event.get("end_sec") {
                        None | Some(&Value::Null) => None,
                        Some(value) => {
                            let end = value.as_f64().filter(|secs| secs.is_finite()).ok_or_else(
                                || "visual-transcript event end_sec is not a number.".to_string(),
                            )?;
                            if end < start_sec {
                                return Err(
                                    "visual-transcript event end_sec precedes its start_sec."
                                        .to_string(),
                                );
                            }
                            Some(end)
                        }
                    };
                let kind = event
                    .get("kind")
                    .and_then(Value::as_str)
                    .filter(|kind| !kind.trim().is_empty())
                    .ok_or_else(|| "visual-transcript event has no kind.".to_string())?
                    .to_string();
                let layer_id = event
                    .get("layer_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| "visual-transcript event has no layer_id.".to_string())?
                    .to_string();
                let item_id = event
                    .get("item_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| "visual-transcript event has no item_id.".to_string())?
                    .to_string();
                let searchable_text = event
                    .get("searchable_text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .ok_or_else(|| {
                        "visual-transcript event has blank searchable_text.".to_string()
                    })?
                    .to_string();
                let provenance = event
                    .get("provenance")
                    .and_then(Value::as_object)
                    .ok_or_else(|| "visual-transcript event has no provenance.".to_string())?;
                let model = provenance
                    .get("model")
                    .and_then(Value::as_str)
                    .filter(|model| !model.is_empty())
                    .ok_or_else(|| "visual-transcript event provenance has no model.".to_string())?
                    .to_string();
                // The S1 refinement's read-time corroboration: the sensory
                // badges of every overlapping event plus its own. The server
                // always emits `evidence`; a missing array is a contract
                // break, not a silent empty.
                let evidence = event
                    .get("evidence")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "visual-transcript event has no evidence array.".to_string())?
                    .iter()
                    .map(|badge| {
                        badge
                            .as_str()
                            .filter(|badge| !badge.is_empty())
                            .map(str::to_string)
                            .ok_or_else(|| {
                                "visual-transcript event evidence entry is not a nonempty string."
                                    .to_string()
                            })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                // The optional object payload: a well-formed normalized box
                // arms the redaction target; anything else renders without
                // the affordance (payloads are arbitrary JSON by contract).
                let payload = event.get("payload");
                let (box_norm, object_label) = match payload {
                    None | Some(Value::Null) => (None, None),
                    Some(value) => {
                        let object_label = value
                            .get("label")
                            .and_then(Value::as_str)
                            .filter(|label| !label.trim().is_empty())
                            .map(str::to_string);
                        let r#box = value.get("box").and_then(Value::as_object);
                        let box_norm = r#box.and_then(|r#box| {
                            let mut values = [0.0; 4];
                            for (slot, key) in values.iter_mut().zip(["x", "y", "w", "h"]) {
                                *slot = r#box
                                    .get(key)
                                    .and_then(Value::as_f64)
                                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))?;
                            }
                            let [x, y, w, h] = values;
                            (w > 0.0 && h > 0.0 && x + w <= 1.0 && y + h <= 1.0).then_some(values)
                        });
                        (box_norm, object_label)
                    }
                };
                rows.push(PaneEventRow {
                    start_sec,
                    end_sec,
                    kind,
                    layer_id,
                    item_id,
                    searchable_text,
                    model,
                    box_norm,
                    object_label,
                    evidence,
                });
            }
            let events_total = object
                .get("events_total")
                .and_then(Value::as_u64)
                .ok_or_else(|| "layers_transcript returned no events_total.".to_string())?
                as usize;
            let kinds_present = object
                .get("kinds_present")
                .and_then(Value::as_array)
                .ok_or_else(|| "layers_transcript returned no kinds_present.".to_string())?
                .iter()
                .map(|kind| {
                    kind.as_str()
                        .filter(|kind| !kind.is_empty())
                        .map(str::to_string)
                        .ok_or_else(|| "kinds_present entry is not a kind string.".to_string())
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(VisualEventsState {
                status: status.to_string(),
                events: rows,
                events_total,
                kinds_present,
                note,
            })
        }
        other => Err(format!(
            "layers_transcript returned unknown status {other:?}."
        )),
    }
}

/// Strict-parse a `layers_render_redactions` response (S3's result JSON)
/// into the review/lineage summary the events tail renders: the render
/// plan's segments[] (mode + range + item count) and applied_items[] (what
/// was drawn where). A malformed result rejects visibly.
fn parse_redaction_render(payload: &Value) -> Result<RedactionRenderSummary, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "layers_render_redactions returned no object data.".to_string())?;
    if object.get("status").and_then(Value::as_str) != Some("rendered") {
        return Err(format!(
            "layers_render_redactions returned status {:?}, expected \"rendered\".",
            object.get("status").and_then(Value::as_str)
        ));
    }
    let decision_layer_id = object
        .get("decision_layer_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "layers_render_redactions returned no decision_layer_id.".to_string())?
        .to_string();
    let decisions_rendered = object
        .get("decisions_rendered")
        .and_then(Value::as_u64)
        .ok_or_else(|| "layers_render_redactions returned no decisions_rendered.".to_string())?
        as usize;
    let reencoded_segments = object
        .get("reencoded_segments")
        .and_then(Value::as_u64)
        .ok_or_else(|| "layers_render_redactions returned no reencoded_segments.".to_string())?
        as usize;
    let stream_copied_segments = object
        .get("stream_copied_segments")
        .and_then(Value::as_u64)
        .ok_or_else(|| "layers_render_redactions returned no stream_copied_segments.".to_string())?
        as usize;
    let segments = object
        .get("segments")
        .and_then(Value::as_array)
        .ok_or_else(|| "layers_render_redactions returned no segments.".to_string())?
        .iter()
        .map(|segment| {
            let mode = segment
                .get("mode")
                .and_then(Value::as_str)
                .filter(|mode| matches!(*mode, "stream_copy" | "reencode"))
                .ok_or_else(|| "render segment has an unknown mode.".to_string())?
                .to_string();
            let start_sec = segment
                .get("start_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite() && *secs >= 0.0)
                .ok_or_else(|| "render segment has no start_sec.".to_string())?;
            let end_sec = segment
                .get("end_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite() && *secs > start_sec)
                .ok_or_else(|| "render segment has no end beyond its start.".to_string())?;
            let items = segment
                .get("items")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .ok_or_else(|| "render segment has no items array.".to_string())?;
            Ok(PaneRenderSegment {
                mode,
                start_sec,
                end_sec,
                items,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let applied = object
        .get("applied_items")
        .and_then(Value::as_array)
        .ok_or_else(|| "layers_render_redactions returned no applied_items.".to_string())?
        .iter()
        .map(|item| {
            let item_id = item
                .get("item_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "applied item has no item_id.".to_string())?
                .to_string();
            let effect = item
                .get("effect")
                .and_then(Value::as_str)
                .filter(|effect| !effect.is_empty())
                .ok_or_else(|| "applied item has no effect.".to_string())?
                .to_string();
            let pixel = |key: &str| {
                item.get(key)
                    .and_then(Value::as_u64)
                    .filter(|v| *v <= u32::MAX as u64)
                    .map(|v| v as u32)
                    .ok_or_else(|| format!("applied item has no {key}."))
            };
            let local_start_sec = item
                .get("local_start_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite())
                .ok_or_else(|| "applied item has no local_start_sec.".to_string())?;
            let local_end_sec = item
                .get("local_end_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite())
                .ok_or_else(|| "applied item has no local_end_sec.".to_string())?;
            Ok(PaneAppliedRedaction {
                item_id,
                effect,
                pixel_x: pixel("pixel_x")?,
                pixel_y: pixel("pixel_y")?,
                pixel_w: pixel("pixel_w")?,
                pixel_h: pixel("pixel_h")?,
                local_start_sec,
                local_end_sec,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RedactionRenderSummary {
        decision_layer_id,
        decisions_rendered,
        reencoded_segments,
        stream_copied_segments,
        segments,
        applied,
    })
}

/// The exact `educt_store_layer` request for a pane-composed reel (R3, S6
/// ordered form): the EDL built directly from the pane's ordered clip list
/// — Keep ops from the included clips in composer order, with in-reel
/// strikes baked into the keeps through the server's own difference math
/// (a composer EDL carries no Cut ops in the assemble form; a per-clip
/// strike as a Cut would strike EVERY clip overlapping the range), and Cut
/// ops from every whole-recording strike per the mode. Assemble keeps
/// only, reduce cuts only, mixed carries both; `None` when the mode has
/// nothing to compose. Provenance carries the pane_reel_compose template —
/// never researcher_annotation — so a composed EDL can never masquerade
/// as a researcher strike when the pane re-parses its own transcript.
fn compose_reel_params(
    state: &TranscriptPaneState,
    clips: &[ReelClip],
    mode: ReelComposeMode,
) -> Option<serde_json::Value> {
    let keeps = match mode {
        ReelComposeMode::Reduce => Vec::new(),
        ReelComposeMode::Assemble | ReelComposeMode::Mixed => composed_keep_ranges(clips),
    };
    let cuts: Vec<(usize, usize)> = match mode {
        ReelComposeMode::Assemble => Vec::new(),
        ReelComposeMode::Reduce | ReelComposeMode::Mixed => state
            .strikes
            .iter()
            .map(|strike| (strike.start_word, strike.end_word))
            .collect(),
    };
    // Mixed is "keeps minus strikes": with nothing included there is
    // nothing to keep, so it refuses with its empty status rather than
    // silently composing the reduce reel (cuts-only ops read as the whole
    // transcript under the algebra's subtractive base).
    if mode == ReelComposeMode::Mixed && keeps.is_empty() {
        return None;
    }
    let mut ops = Vec::new();
    for range in keeps {
        ops.push(serde_json::json!({
            "op": "keep",
            "range": {"start_word": range.start_word, "end_word": range.end_word},
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

/// The ordered match list for the in-transcript search (S3): every
/// occurrence of the trimmed query over the working words joined by single
/// spaces (the same join the pane blocks render), mapped to the inclusive
/// word range each occurrence covers — so a phrase spanning words marks and
/// jumps as one range. The case rule matches the server's
/// `educt_search_transcripts` lexical mode (ASCII case-insensitive,
/// non-ASCII exact — `to_ascii_lowercase` preserves byte offsets, so the
/// lowered text's positions map back to the original) so the panel and the
/// server agree on what a match is. Occurrences never overlap (the scan
/// resumes past each hit); a blank query or wordless view matches nothing.
fn transcript_search_matches(words: &[PaneWord], query: &str) -> Vec<(usize, usize)> {
    let query = query.trim();
    if query.is_empty() || words.is_empty() {
        return Vec::new();
    }
    let mut joined = String::new();
    let mut word_starts: Vec<usize> = Vec::with_capacity(words.len());
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            joined.push(' ');
        }
        word_starts.push(joined.len());
        joined.push_str(&word.word);
    }
    let haystack = joined.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let mut matches = Vec::new();
    let mut from = 0;
    while let Some(found) = haystack[from..].find(&needle) {
        let start = from + found;
        let end = start + needle.len();
        // Byte offsets map back through the per-word starts: the last word
        // starting at or before the hit starts, and the last word starting
        // before the hit ends (the trimmed query never ends on a space).
        let start_word = word_starts.partition_point(|&offset| offset <= start) - 1;
        let end_word = word_starts.partition_point(|&offset| offset < end) - 1;
        matches.push((start_word, end_word));
        from = end;
    }
    matches
}

/// The search input's status label (S3): the current match's position over
/// the total, the honest zero-match state, or the word-timing degradation
/// note for text-only views — `None` while the query is blank (nothing to
/// report, no noise).
fn transcript_search_status(search: &TranscriptSearch, has_words: bool) -> Option<String> {
    if search.query.text().trim().is_empty() {
        return None;
    }
    if !has_words {
        return Some("Search needs a word-timed view".into());
    }
    if search.matches.is_empty() {
        return Some("No matches".into());
    }
    Some(format!("{}/{}", search.current + 1, search.matches.len()))
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
/// the transcript workspace's player is the SAME instance the asset
/// workspace's player and inline renders use (D1: one player per media
/// identity). Audio containers take the audio player; everything else is
/// a video source.
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
    // The recording this transcript is stored against, when it has one
    // (R3): the workspace keys its media to the RECORDING — the same
    // asset the asset workspace's player renders — and falls to the
    // transcript's stored media_path only when the recording left the
    // gallery index.
    let gallery_asset_id = summary
        .get("gallery_asset_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
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
    // The live correction layer's edits (S5): the working view names the
    // layer by correction_layer_id; an absent id parses as no prior
    // edits; a named layer missing from the records rejects the payload
    // rather than silently dropping prior corrections (the silent-revert
    // guard).
    let correction_layer_id = working
        .get("correction_layer_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let correction_edits = match &correction_layer_id {
        None => Vec::new(),
        Some(live_id) => {
            let record = layers_array
                .iter()
                .find(|layer| layer.get("id").and_then(Value::as_str) == Some(live_id.as_str()))
                .ok_or_else(|| {
                    format!(
                        "the working view names correction layer {live_id}, but the \
                         layers list has no such record."
                    )
                })?;
            let layer_object = record
                .get("layer")
                .and_then(Value::as_object)
                .ok_or_else(|| "the correction layer record has no layer object.".to_string())?;
            if layer_object.get("kind").and_then(Value::as_str) != Some("correction") {
                return Err(format!("layer {live_id} is not a correction layer."));
            }
            let edits = layer_object
                .get("edits")
                .and_then(Value::as_array)
                .ok_or_else(|| "the correction layer has no edits array.".to_string())?;
            edits
                .iter()
                .map(|edit| {
                    let object = edit
                        .as_object()
                        .ok_or_else(|| "correction edit is not an object.".to_string())?;
                    Ok(PaneCorrectionEdit {
                        start_word: object
                            .get("start_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "correction edit has no start_word.".to_string())?,
                        end_word: object
                            .get("end_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "correction edit has no end_word.".to_string())?,
                        replacement: object
                            .get("replacement")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "correction edit has no replacement.".to_string())?
                            .to_string(),
                        reason: object
                            .get("reason")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "correction edit has no reason.".to_string())?
                            .to_string(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
        }
    };
    // The immutable source bundle's word timings (S6): the render path's
    // own word→time input (`educt_get_transcript` carries the stored bundle
    // under "transcript"). Absent when the stored bundle has no word
    // timings (a text-only source) — the preview refuses honestly then.
    let source_words = object
        .get("transcript")
        .and_then(Value::as_object)
        .and_then(|bundle| bundle.get("words"))
        .and_then(Value::as_array)
        .map(|words| {
            words
                .iter()
                .map(|word| {
                    let entry = word
                        .as_object()
                        .ok_or_else(|| "source bundle word is not an object.".to_string())?;
                    Ok(PaneWord {
                        word: entry
                            .get("word")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "source bundle word has no text.".to_string())?
                            .to_string(),
                        start_ms: entry
                            .get("start_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "source bundle word has no start_ms.".to_string())?,
                        end_ms: entry
                            .get("end_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "source bundle word has no end_ms.".to_string())?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(TranscriptPaneState {
        transcript_id,
        media_path,
        gallery_asset_id,
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
        correction_edits,
        source_words,
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
    NONTERMINAL_JOB_STATUSES.contains(&job.status.as_str())
}

/// The ambient verb for a live research operation's badge — short enough
/// for a row chip, never error-styled.
fn job_op_verb(op: &str) -> &'static str {
    match op {
        "transcribe" => "Transcribing…",
        "audio_trim" => "Trimming…",
        "audio_concat" | "video_concat" => "Joining…",
        "video_clip" => "Clipping…",
        _ => "Working…",
    }
}

/// Order the live set newest-first and drop duplicate ids — a job can
/// appear in both the page and a probe snapshot within one tick. Stable
/// sort: equal `created_at` keeps insertion order (page before probes).
fn dedupe_jobs_newest_first(jobs: &mut Vec<JobRecord>) {
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut seen = HashSet::new();
    jobs.retain(|job| seen.insert(job.id.clone()));
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
        let live_count = self.live_jobs.len();
        let activity_label: SharedString = if live_count > 0 {
            format!("Activity ({live_count})").into()
        } else {
            "Activity".into()
        };
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
                    .child(self.tab_button(ViewerTab::Workspace, "Workspace".into(), cx))
                    .child(self.tab_button(ViewerTab::Library, library_label, cx))
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
                    // The activity toggle (T2) — the ambient status strip:
                    // the live-job count over any mode, never error-styled
                    // (progress is transient). Opening the drawer observes
                    // on demand and cancels from there.
                    .child(
                        div()
                            .id("media-activity-toggle")
                            .debug_selector(|| "media-activity-toggle".to_string())
                            .flex_shrink_0()
                            .px_2()
                            .h_6()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .map(|el| {
                                if self.activity_open {
                                    el.bg(cx.theme().colors().element_active)
                                } else {
                                    el.hover(|el| el.bg(cx.theme().colors().element_hover))
                                }
                            })
                            .cursor_pointer()
                            .child(
                                Label::new(activity_label)
                                    .size(LabelSize::XSmall)
                                    .color(if live_count > 0 {
                                        ui::Color::Hint
                                    } else {
                                        ui::Color::Muted
                                    })
                                    .truncate(),
                            )
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.toggle_activity(cx);
                            })),
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
                ViewerTab::Workspace => self.render_workspace(_window, cx).into_any_element(),
                ViewerTab::Library => self.render_library(cx).into_any_element(),
            })
            // The on-demand activity drawer (T2): a bounded strip over any
            // mode — job status is ambient, never a navigation destination.
            .when(self.activity_open, |el| {
                el.child(self.render_activity_drawer(cx))
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
    use crate::test_fixtures::scheduled_job;

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
    /// expect: Leaving the workspace suspends the shared player before the hidden mode renders.
    /// [P1] Motivating: Panel navigation cannot leave ghost audio or playback polling behind.
    #[gpui::test]
    fn leaving_workspace_suspends_retained_player(cx: &mut gpui::TestAppContext) {
        // The lock: entering Library dispatches `gallery_list_assets`, and
        // an unlocked dispatch can land in a concurrent lock-holder's
        // scripted invoker (the cross-talk the lock exists to prevent).
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
            viewer.active_tab = ViewerTab::Workspace;
        });

        viewer.update(cx, |viewer, cx| viewer.activate_tab(ViewerTab::Library, cx));

        assert!(widget.read_with(cx, |widget, _cx| widget.is_suspended()));
        assert_eq!(
            viewer.read_with(cx, |viewer, _cx| viewer.active_tab),
            ViewerTab::Library
        );
    }

    /// dcterms:identifier: `MediaViewer::toggle_inspector`
    /// expect: Toggling the inspector pane never clears the selection, the
    /// retained player, or its body, and never suspends the player — the
    /// pane only changes what renders (T1 criterion 2).
    /// [P1] Motivating: the workspace keeps the object being worked on.
    #[gpui::test]
    fn inspector_toggle_preserves_selection_and_player(cx: &mut gpui::TestAppContext) {
        let widget = cx.new(|cx| {
            hkask_media_widget::MediaWidget::new(
                hkask_media_widget::MediaRef::new(
                    "/definitely/missing.png".into(),
                    hkask_media_widget::MediaKind::Image,
                ),
                cx,
            )
        });
        let viewer = cx.new(|_| {
            let mut viewer = MediaViewer::new();
            // A surfaced (unindexed) asset: the toggle pin must not dispatch —
            // this test installs no invoker and holds no lock, so any
            // inspector read would cross-wire into the process-global
            // invoker of whichever lock-holding test is running.
            viewer.assets = vec![surfaced_asset("/a.png")];
            viewer.selected = Some(0);
            viewer.active_tab = ViewerTab::Workspace;
            viewer
        });
        viewer.update(cx, |viewer, _cx| {
            viewer.media_widget = Some(widget.clone());
            viewer.media_widget_body = Some("asset-0-body".to_string());
        });

        viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
        viewer.read_with(cx, |viewer, _cx| {
            assert!(!viewer.inspector_open, "the first toggle closes the pane");
            assert_eq!(viewer.selected, Some(0));
            assert!(viewer.media_widget.is_some(), "the player is retained");
            assert_eq!(viewer.media_widget_body.as_deref(), Some("asset-0-body"));
        });

        viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
        viewer.read_with(cx, |viewer, _cx| {
            assert!(viewer.inspector_open, "the second toggle reopens the pane");
            assert_eq!(viewer.selected, Some(0));
            assert!(viewer.media_widget.is_some());
            assert_eq!(viewer.media_widget_body.as_deref(), Some("asset-0-body"));
        });
        assert!(
            !widget.read_with(cx, |widget, _cx| widget.is_suspended()),
            "the pane toggle never suspends the player"
        );
    }

    /// dcterms:identifier: `MediaViewer::load_inspector`
    /// expect: The inspector's reads (detail, transcript links, annotations)
    /// fire on workspace entry with the pane visible and on a selection change
    /// under the open pane, never with the pane closed or from another mode —
    /// the old Detail-tab activation contract, now the pane's.
    /// [P1] Motivating: the inspector follows the selection without a
    /// separate destination to visit.
    #[gpui::test]
    fn inspector_loads_follow_selection_and_pane_visibility(cx: &mut gpui::TestAppContext) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (calls, _restore) = install_scripted_invoker(vec![
            r#"{"content":{"image":{"src":"/a.png"},"tags":[]}}"#.to_string(),
            r#"{"content":[]}"#.to_string(),
            r#"{"content":[]}"#.to_string(),
        ]);
        let viewer = cx.new(|_| {
            let mut viewer = MediaViewer::new();
            viewer.assets = vec![indexed_asset("/a.png", 0)];
            viewer.active_tab = ViewerTab::Library;
            viewer
        });
        // Selecting from the Library loads nothing while the Library is the
        // active mode; entering the workspace with the pane open fires the
        // inspector's three reads for the selected asset.
        viewer.update(cx, |viewer, cx| {
            viewer.set_selection(Some(0), cx);
        });
        assert_eq!(
            calls.lock().expect("calls lock").len(),
            0,
            "no inspector reads from the Library mode"
        );
        viewer.update(cx, |viewer, cx| {
            viewer.activate_tab(ViewerTab::Workspace, cx);
        });
        let recorded = calls.lock().expect("calls lock").clone();
        assert_eq!(
            recorded
                .iter()
                .map(|(tool, _)| tool.as_str())
                .collect::<Vec<_>>(),
            vec![
                "gallery_asset_detail",
                "educt_list_transcripts",
                "gallery_annotation_list"
            ],
            "the workspace entry fires the inspector's trio"
        );
        assert_eq!(recorded[0].1, serde_json::json!({ "image_id": "asset-0" }));

        // A selection change under the closed pane loads nothing; reopening
        // the pane loads the reads for the NEW selection.
        viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
        viewer.update(cx, |viewer, _cx| {
            viewer.assets.push(indexed_asset("/b.png", 1));
        });
        viewer.update(cx, |viewer, cx| viewer.set_selection(Some(1), cx));
        assert_eq!(
            calls.lock().expect("calls lock").len(),
            3,
            "a closed pane loads nothing on selection change"
        );
        viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));
        let reopened = calls.lock().expect("calls lock").clone();
        assert_eq!(
            reopened[3..]
                .iter()
                .map(|(tool, _)| tool.as_str())
                .collect::<Vec<_>>(),
            vec![
                "gallery_asset_detail",
                "educt_list_transcripts",
                "gallery_annotation_list"
            ],
            "reopening the pane refreshes the reads for the new selection"
        );
        assert_eq!(reopened[3].1, serde_json::json!({ "image_id": "asset-1" }));
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
        let _restore = InvokerRestore(hkask_tool_invoker::shared_tool_invoker());
        let record = hkask_mcp_media::types::JobRecord {
            id: "job-1".into(),
            op: "transcribe".into(),
            status: "running".into(),
            created_at: "2026-09-05T00:00:00Z".into(),
            completed_at: None,
            result: None,
            error: None,
            source: None,
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
            source: None,
        }
    }

    /// A strict `job_list` response envelope for the scripted invoker —
    /// the same contract shape `parse_job_list_response` validates.
    fn jobs_listing_response(jobs: &[JobRecord], total: usize, has_more: bool) -> String {
        serde_json::json!({
            "content": {
                "jobs": jobs,
                "total": total,
                "limit": hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
                "has_more": has_more,
                "history_scope": hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE,
                "restart_behavior": hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR
            }
        })
        .to_string()
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

    /// dcterms:identifier: `MediaViewer::should_poll_jobs`
    /// expect: The activity poll arms while any nonterminal job is observed — on any mode, drawer closed — and stops only at complete quiescence or a listing failure; a superseded epoch never mutates state.
    /// [P1] Motivating: background queue work is bounded by observed demand, never by page visibility (T2).
    #[gpui::test]
    fn activity_polling_arms_on_any_live_job_and_stops_only_at_quiescence(
        cx: &mut gpui::TestAppContext,
    ) {
        // The lock: the tab switches below dispatch the modes' loads
        // (`gallery_list_assets`, `educt_list_transcripts`) through the
        // process-global invoker — unlocked they cross-wire into a
        // concurrent lock-holder's scripted invoker.
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            // Ambient from any mode: Library is active and the drawer is
            // closed — no jobs surface is visible (visiting the retired
            // Queue tab was the old visibility condition).
            viewer.active_tab = ViewerTab::Library;
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
            assert!(
                viewer.queue_poll_task.is_some(),
                "a live job arms the poll with no jobs surface visible"
            );
            // Mode switches never cancel observation — the poll condition
            // is the observed live set, not the visible page.
            viewer.activate_tab(ViewerTab::Workspace, cx);
            assert!(
                viewer.queue_poll_task.is_some(),
                "leaving a mode does not cancel the timer"
            );
            viewer.activate_tab(ViewerTab::Library, cx);
            assert!(
                viewer.queue_poll_task.is_some(),
                "entering a mode does not cancel the timer"
            );
            // Closing the drawer never cancels observation either.
            viewer.activity_open = true;
            viewer.toggle_activity(cx);
            assert!(!viewer.activity_open);
            assert!(
                viewer.queue_poll_task.is_some(),
                "closing the drawer does not cancel the timer"
            );

            // Complete quiescence stops the poll: the page covers the
            // whole bounded store (has_more false) and holds no live job.
            let quiescent_epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            viewer.apply_jobs_listing(
                quiescent_epoch,
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
                "complete quiescence stops the poll"
            );
            assert!(viewer.live_jobs.is_empty());

            // A superseded epoch's outcome is dropped; the owned error
            // stops the poll and preserves the last-good rows.
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
            let superseded_epoch = viewer.jobs_request.epoch;
            let error_epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("supersedes");
            assert_ne!(error_epoch, superseded_epoch);
            viewer.apply_jobs_listing(superseded_epoch, Err("stale queue error".into()), cx);
            assert_eq!(
                viewer.jobs.len(),
                1,
                "a superseded epoch's outcome is dropped"
            );
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
            // Library mode suppresses the inspector trio the ingest's
            // auto-select would otherwise fire (the retired Queue tab's old
            // role in this setup) — this test holds no invoker lock, so any
            // dispatch would cross-wire into a concurrent lock-holder's
            // scripted invoker.
            viewer.active_tab = ViewerTab::Library;
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

    /// dcterms:identifier: `MediaViewer::observe_live_jobs`
    /// expect: A nonterminal job deeper than the newest-first page — behind 20 newer completed jobs — stays observed (the poll stays armed) and its landing updates the live set (loop-register L11, the falsifier T2 settles).
    /// [P1] Motivating: page visibility never bounds job observation.
    #[gpui::test]
    fn deep_running_job_behind_a_full_page_stays_observed_and_updated(
        cx: &mut gpui::TestAppContext,
    ) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let limit = hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT;
        // The deep job: older than everything on the page (the L11 shape).
        let mut deep = scheduled_job("job-deep", "transcribe", "running", "2026-09-01T00:00:00Z");
        deep.source = Some("/talk.wav".into());
        // Twenty newer completed jobs fill the page.
        let newer: Vec<JobRecord> = (0..limit)
            .map(|index| {
                scheduled_job(
                    &format!("job-new-{index}"),
                    "video_clip",
                    "completed",
                    "2026-09-02T00:00:00Z",
                )
            })
            .collect();
        let page = jobs_listing_response(&newer, limit + 1, true);
        let probe_empty = jobs_listing_response(&[], 0, false);
        let probe_deep = jobs_listing_response(&[deep], 1, false);
        // Tick 1: the unfiltered page plus the three status probes
        // (queued, running, cancelling — the dispatch order); the running
        // probe finds the deep job. Tick 2 (the poll's next link): the
        // deep job landed — every probe is empty.
        let (calls, _restore) = install_scripted_invoker(vec![
            page.clone(),
            probe_empty.clone(),
            probe_deep,
            probe_empty.clone(),
            page,
            probe_empty.clone(),
            probe_empty.clone(),
            probe_empty,
        ]);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            // No jobs surface is visible: the Workspace mode with no open
            // transcript, drawer closed — the observation must be ambient,
            // not visited. (The Library is the recordings section's home
            // since T3 — a transcribe landing THERE would also reload the
            // stored-transcript listing, a dispatch this test's scripted
            // queue does not carry; the workspace shows no such surface.)
            viewer.active_tab = ViewerTab::Workspace;
            viewer.load_jobs(cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.jobs.len(),
                limit,
                "the page holds only the newest jobs"
            );
            assert!(
                !viewer.jobs.iter().any(|job| job.id == "job-deep"),
                "the deep job is beyond the page — that is the L11 shape"
            );
            assert!(
                viewer.live_jobs.iter().any(|job| job.id == "job-deep"),
                "the status probes observe the deep running job"
            );
            assert_eq!(viewer.live_jobs.len(), 1);
            assert!(
                viewer.queue_poll_task.is_some(),
                "the poll stays armed while a live job is observed anywhere"
            );
            assert_eq!(
                viewer.transcribing_ids,
                ["job-deep".to_string()].into_iter().collect::<HashSet<_>>(),
                "deep transcribe jobs feed the R5 landing tracking"
            );
            assert_eq!(
                viewer.jobs_restart_behavior.as_deref(),
                Some(hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR),
                "the drawer's disclosure copy is sourced from the validated listing fields"
            );
            assert_eq!(
                viewer.jobs_history_scope.as_deref(),
                Some(hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE)
            );
        });
        // The dispatch shape: the page asks unfiltered, then one
        // status-filtered `job_list` per nonterminal status — the server
        // filters BEFORE the newest-first sort and limit, which is exactly
        // what observes beyond the page.
        {
            let calls = calls.lock().expect("calls lock");
            let status_args: Vec<Value> = calls
                .iter()
                .map(|(_, args)| args.get("status").cloned().unwrap_or(Value::Null))
                .collect();
            assert_eq!(
                status_args,
                vec![
                    Value::Null,
                    Value::String("queued".into()),
                    Value::String("running".into()),
                    Value::String("cancelling".into()),
                ],
                "one tick: the page, then one status probe per nonterminal status"
            );
        }
        // Tick 2 — the poll's next link observes the landing: the deep job
        // is gone from every probe, so the live set empties, the R5
        // tracking updates, and the poll stops.
        viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer.live_jobs.is_empty(),
                "the landing of a job beyond the page is observed and updated"
            );
            assert!(viewer.transcribing_ids.is_empty());
            assert!(
                viewer.queue_poll_task.is_none(),
                "the poll stops when no live job remains anywhere"
            );
        });
        // The second tick repeated the same dispatch shape.
        {
            let calls = calls.lock().expect("calls lock");
            assert_eq!(
                calls.len(),
                8,
                "two ticks of the page plus three probes each"
            );
        }
    }

    /// dcterms:identifier: `MediaViewer::settle_activity_tick`
    /// expect: A transcribe job landing anywhere in the store reloads the
    /// stored-transcript listing exactly once while the Library — the
    /// recordings browser's home since T3 — is showing, so the finished
    /// transcript appears under its recording (R5 continuity through the
    /// T2 poll restructure) — and not while another mode is showing.
    /// [P7] Motivating: the finished transcript appears under its recording.
    #[gpui::test]
    fn transcribe_landing_reloads_the_recordings_listing(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let transcript_listing = r#"{"content": {"transcripts": [], "count": 0}}"#.to_string();
        let (calls, _restore) = install_scripted_invoker(vec![transcript_listing]);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_listing = Some(vec![]);
            let running_listing = JobListing {
                jobs: vec![scheduled_job(
                    "job-r5",
                    "transcribe",
                    "running",
                    "2026-09-03T00:00:00Z",
                )],
                total: 1,
                limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
                has_more: false,
                history_scope: hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE.into(),
                restart_behavior: hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR.into(),
            };
            // Landing away from the Library (the Workspace mode is showing)
            // does not reload the recordings section's listing.
            viewer.active_tab = ViewerTab::Workspace;
            let epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            viewer.apply_jobs_listing(epoch, Ok(running_listing.clone()), cx);
            assert_eq!(
                viewer.transcribing_ids,
                ["job-r5".to_string()].into_iter().collect::<HashSet<_>>()
            );
            let landed_epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            let mut landed_listing = running_listing.clone();
            landed_listing.jobs = vec![scheduled_job(
                "job-r5",
                "transcribe",
                "completed",
                "2026-09-03T00:00:00Z",
            )];
            viewer.apply_jobs_listing(landed_epoch, Ok(landed_listing), cx);
            assert!(
                viewer.transcribing_ids.is_empty(),
                "the landing is observed from the live set"
            );
            assert_eq!(
                viewer.transcript_listing_request.state,
                ResourceState::Idle,
                "no reload while the Workspace mode is showing"
            );

            // Landing while the Library is showing reloads its recordings
            // section once — the finished transcript appears under its
            // recording (R5).
            viewer.active_tab = ViewerTab::Library;
            let epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("restarts");
            viewer.apply_jobs_listing(epoch, Ok(running_listing), cx);
            let landed_epoch = viewer
                .jobs_request
                .begin(RequestOwner::Jobs)
                .expect("starts");
            let landed_listing = JobListing {
                jobs: vec![scheduled_job(
                    "job-r5",
                    "transcribe",
                    "completed",
                    "2026-09-03T00:00:00Z",
                )],
                total: 1,
                limit: hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT,
                has_more: false,
                history_scope: hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE.into(),
                restart_behavior: hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR.into(),
            };
            viewer.apply_jobs_listing(landed_epoch, Ok(landed_listing), cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.transcript_listing_request.state,
                ResourceState::Ready,
                "the landing reloaded the recordings listing (R5)"
            );
            assert_eq!(
                viewer.transcript_listing.as_ref().map(Vec::len),
                Some(0),
                "the reloaded listing committed"
            );
            let calls = calls.lock().expect("calls lock");
            assert_eq!(calls.len(), 1, "the reload dispatched exactly once");
            assert_eq!(calls[0].0, "educt_list_transcripts");
        });
    }

    /// dcterms:identifier: `MediaViewer::toggle_activity`
    /// expect: Opening the activity drawer refreshes the job listing (observed on demand, never stale); closing dispatches nothing.
    /// [P1] Motivating: the drawer's opening state is current, and closing is free.
    #[gpui::test]
    fn opening_the_activity_drawer_refreshes_the_listing(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let page = jobs_listing_response(&[job("completed")], 1, false);
        let (calls, _restore) = install_scripted_invoker(vec![page]);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            assert!(!viewer.activity_open);
            viewer.toggle_activity(cx);
            assert!(viewer.activity_open);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.jobs_request.state,
                ResourceState::Ready,
                "opening the drawer dispatched a listing refresh"
            );
            assert_eq!(viewer.jobs.len(), 1);
        });
        let calls_before_close = calls.lock().expect("calls lock").len();
        viewer.update(cx, |viewer, cx| viewer.toggle_activity(cx));
        cx.run_until_parked();
        assert!(!viewer.read_with(cx, |viewer, _| viewer.activity_open));
        assert_eq!(
            calls.lock().expect("calls lock").len(),
            calls_before_close,
            "closing the drawer dispatches nothing"
        );
    }

    /// dcterms:identifier: `MediaViewer::asset_activity_verb`
    /// expect: The Library badge names the live job's verb from the record's primary source, and only for the asset it targets.
    /// [P1] Motivating: ambient status is accurate to the affected asset.
    #[test]
    fn asset_activity_verb_follows_the_primary_source() {
        let mut viewer = MediaViewer::new();
        viewer.assets = vec![surfaced_asset("/talk.wav")];
        let mut live = scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
        live.source = Some("/talk.wav".into());
        viewer.live_jobs = vec![live];
        assert_eq!(
            viewer.asset_activity_verb(&viewer.assets[0]),
            Some("Transcribing…")
        );
        let mut elsewhere =
            scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
        elsewhere.source = Some("/elsewhere.wav".into());
        viewer.live_jobs = vec![elsewhere];
        assert_eq!(
            viewer.asset_activity_verb(&viewer.assets[0]),
            None,
            "a job against another asset does not badge this one"
        );
        viewer.live_jobs.clear();
        assert_eq!(viewer.asset_activity_verb(&viewer.assets[0]), None);
        // The verb vocabulary covers every research op.
        for (op, verb) in [
            ("transcribe", "Transcribing…"),
            ("audio_trim", "Trimming…"),
            ("audio_concat", "Joining…"),
            ("video_concat", "Joining…"),
            ("video_clip", "Clipping…"),
            ("render_video", "Working…"),
        ] {
            assert_eq!(job_op_verb(op), verb);
        }
    }

    /// dcterms:identifier: `MediaViewer::apply_live_observation`
    /// expect: A filter-violating probe response never grafts a terminal row onto the live set, and a failed battery retains the previously observed live entries.
    /// [P1] Motivating: the live set is nonterminal by construction — retention must not immortalize a bad row.
    #[gpui::test]
    fn live_observation_rejects_terminal_rows_and_retains_through_failure(
        cx: &mut gpui::TestAppContext,
    ) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            // A live entry observed by a previous tick, not on the page.
            let mut deep =
                scheduled_job("job-deep", "transcribe", "running", "2026-09-01T00:00:00Z");
            deep.source = Some("/talk.wav".into());
            viewer.live_jobs = vec![deep.clone()];
            let epoch = viewer
                .job_observation_request
                .begin(RequestOwner::JobObservation)
                .expect("starts");
            // The probe response violates its own filter: a completed row.
            let mut terminal =
                scheduled_job("job-bad", "video_clip", "completed", "2026-09-02T00:00:00Z");
            terminal.source = Some("/clip.mp4".into());
            viewer.apply_live_observation(epoch, Ok(vec![terminal, deep]), cx);
            assert_eq!(viewer.live_jobs.len(), 1);
            assert!(
                viewer.live_jobs.iter().all(|job| job_is_nonterminal(job)),
                "the live set stays nonterminal by construction"
            );
            assert!(viewer.live_jobs.iter().any(|job| job.id == "job-deep"));

            // A failed battery retains previously observed live entries —
            // it proves nothing about deeper jobs.
            let epoch = viewer
                .job_observation_request
                .begin(RequestOwner::JobObservation)
                .expect("restarts");
            viewer.apply_live_observation(epoch, Err("probe failed".into()), cx);
            assert_eq!(
                viewer.live_jobs.len(),
                1,
                "a failed battery retains the observed live set"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("observation failed")),
                "the battery failure is surfaced"
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
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
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

            // S5: the correction editor rides the same admission contract
            // — an in-flight edit refuses a correction confirm and keeps
            // the typed buffer for retry.
            viewer.edit_request.state = ResourceState::Loading;
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Correction {
                    start_word: 3,
                    end_word: 9,
                },
                buffer: LabelBuffer::from_str("fixed words"),
            });
            viewer.confirm_label_edit(cx);
            assert!(
                viewer.highlight_edit.is_some(),
                "a refused correction keeps the typed buffer"
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

            // A blank replacement refuses without dispatching — a
            // correction replaces text; deletions belong to Cut.
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Correction {
                    start_word: 3,
                    end_word: 9,
                },
                buffer: LabelBuffer::from_str("   "),
            });
            viewer.confirm_label_edit(cx);
            assert!(
                viewer.highlight_edit.is_some(),
                "the blank correction stays open"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("needs replacement text"),
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
            gallery_asset_id: None,
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
            correction_edits: Vec::new(),
            source_words: Vec::new(),
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
    /// expect: While an edit is in flight on the workspace (the transcript
    /// sub-surface's compose/highlight edits share the edit lifecycle), the
    /// edit progress surfaces (S5's progress-visible contract); once the
    /// edit completes, the specific status message falls through as
    /// before (2026-10-26 review: compose progress previously rendered
    /// nowhere).
    #[gpui::test]
    fn workspace_surfaces_edit_progress(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.active_tab = ViewerTab::Workspace;
            viewer.edit_request.state = ResourceState::Loading;
            viewer.edit_progress = Some("composing reel from pane highlights…".into());
            let status = viewer.presented_status().map(|(text, _)| text.to_string());
            assert_eq!(
                status.as_deref(),
                Some("composing reel from pane highlights…"),
                "the compose progress is visible on the workspace"
            );
            // The edit completes: the lifecycle returns to Ready and the
            // completion message is the specific status that shows.
            viewer.edit_request.state = ResourceState::Ready;
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
    /// and returns to the plain asset workspace — the same mode (T3: the
    /// recordings browser is the Library's section now, reachable by its
    /// mode at any time, so the 2026-10-26 reachability concern is
    /// structural). Library entry reloads the recordings section's listing
    /// — the browser's data is one mode away, never unreachable; the
    /// unwired-invoker refusal surfaces because the entry dispatch is
    /// visible (D5). The invoker lock is required: a parallel installer
    /// test's mock in the process global would answer the dispatch
    /// instead (observed flaky 2026-10-26).
    #[gpui::test]
    fn closing_the_transcript_returns_to_the_asset_workspace(cx: &mut gpui::TestAppContext) {
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
            viewer.active_tab = ViewerTab::Workspace;
            viewer.close_transcript_workspace(cx);
            assert!(viewer.transcript.is_none(), "the workspace closed");
            assert!(viewer.selection_anchor.is_none());
            assert!(viewer.highlight_edit.is_none());
            assert!(viewer.transcript_follow_task.is_none());
            assert!(
                !viewer.selection_mode,
                "close tears down the Select-text mode with the rest of the sub-surface state \
                 (open_transcript resets it; the close path must match)"
            );
            assert_eq!(
                viewer.active_tab,
                ViewerTab::Workspace,
                "closing stays with the asset's plain workspace"
            );
            // The recordings browser is the Library's section: entering the
            // mode reloads the stored-transcript listing — the entry
            // dispatch surfaces its own unavailability when no invoker is
            // wired, proving the section's data path is live, never
            // dead-ended by the close.
            viewer.activate_tab(ViewerTab::Library, cx);
            assert!(
                matches!(
                    viewer.transcript_listing_request.state,
                    ResourceState::Failed(_)
                ),
                "the Library entry dispatched the recordings listing load: {:?}",
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
        let _restore = InvokerRestore(hkask_tool_invoker::shared_tool_invoker());
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
                    gallery_asset_id: None,
                },
                TranscriptSummaryRow {
                    id: "t-untimed".into(),
                    media_path: "/generated/plain.mp3".into(),
                    words_count: 12,
                    duration_secs: 11,
                    has_word_timings: false,
                    alignment: "untimed".into(),
                    gallery_asset_id: None,
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
    fn recording_keyed_workspace_bodies_prefer_the_recording() {
        let mut viewer = MediaViewer::new();
        // A video-keyed transcript: the gallery recording is the identity,
        // so the workspace renders the RECORDING's asset body — the video
        // plays, the same body the asset workspace's player renders.
        viewer.assets = vec![MediaAsset {
            body: r#"{"kind":"video","src":"/talk.mp4"}"#.into(),
            src: "/talk.mp4".into(),
            kind: "video".into(),
            tool: "gallery".into(),
            gallery_index: Some(0),
            gallery_asset_id: Some("asset-talk".into()),
        }];
        let mut state = pane_state("t-video");
        state.gallery_asset_id = Some("asset-talk".into());
        assert_eq!(
            viewer.workspace_media_body(&state),
            r#"{"kind":"video","src":"/talk.mp4"}"#,
            "a linked transcript renders the recording's asset body"
        );
        // An unlinked transcript falls to its stored media_path — the
        // recording left the gallery index; the transcript and its durable
        // exports stay reachable.
        let unlinked = pane_state("t-unlinked");
        assert_eq!(
            viewer.workspace_media_body(&unlinked),
            transcript_media_body(&unlinked.media_path),
            "an unlinked transcript falls to its stored media body"
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
                    gallery_asset_id: None,
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
                        gallery_asset_id: None,
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
    /// ordered range on the second click — never an inverted range. The
    /// completed range opens the selection popover; Highlight on the
    /// popover opens the label editor.
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
            WordClickOutcome::CompleteRange {
                start_word: 3,
                end_word: 9
            }
        );

        viewer.selection_anchor = Some(3);
        assert_eq!(
            viewer.handle_word_selection(9),
            WordClickOutcome::CompleteRange {
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

    /// dcterms:identifier: `MediaViewer::handle_word_drag`
    /// expect: press-and-release across words completes the ordered union
    /// range in EITHER interaction mode (drag is the one-gesture selection
    /// path — seek mode keeps click-to-seek); same-word release stays a
    /// plain click; the open label editor makes drags inert. The completed
    /// range opens the selection popover; Highlight on the popover opens
    /// the label editor.
    #[test]
    fn word_drag_state_machine() {
        let mut viewer = MediaViewer::new();
        viewer.selection_mode = false;
        assert_eq!(
            viewer.handle_word_drag(2, 5),
            WordDragOutcome::CompleteRange {
                start_word: 2,
                end_word: 5
            },
            "drag-select completes a range outside Select text mode"
        );
        assert_eq!(
            viewer.handle_word_drag(9, 3),
            WordDragOutcome::CompleteRange {
                start_word: 3,
                end_word: 9
            },
            "the range is the ordered union, never inverted"
        );
        assert_eq!(
            viewer.handle_word_drag(4, 4),
            WordDragOutcome::Click,
            "same-word press/release is a plain click (the click path seeks)"
        );
        viewer.highlight_edit = Some(HighlightEdit {
            kind: HighlightEditKind::Create {
                start_word: 0,
                end_word: 1,
            },
            buffer: LabelBuffer::default(),
        });
        assert_eq!(
            viewer.handle_word_drag(2, 5),
            WordDragOutcome::Inert,
            "drags are inert while the label editor is open"
        );
    }

    /// dcterms:identifier: `MediaViewer::popover_cut`
    /// expect: The tray's Cut consumes the popover and dispatches the
    /// existing strike path; an in-flight edit surfaces the admission
    /// refusal instead of dispatching.
    /// [P1] Motivating: Delete/Cut on a pending selection must reach the
    /// strike dispatcher, never a silent no-op.
    #[gpui::test]
    fn popover_cut_dispatches_the_strike_path(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-popover-cut".into(),
                media_path: "/m.wav".into(),
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.selection_popover = Some((3, 9));
            viewer.edit_request.state = ResourceState::Loading;
            viewer.popover_cut(cx);
            assert!(
                viewer.selection_popover.is_none(),
                "the cut consumes the tray"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("Another media edit")),
                "the strike arm surfaced the admission refusal: {:?}",
                viewer.status
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::popover_include_in_reel`
    /// expect: Over a composed clip's range the tray flips that clip's
    /// include flag (the highlights row's toggle path, no dispatch — the
    /// clip's flag is the one membership source of truth); over a fresh
    /// range it dispatches the existing create path — an in-flight edit
    /// surfaces the admission refusal and the range never enters the
    /// composer directly (a clip with no highlight behind it has no reload
    /// identity to survive on, so the fresh arm creates).
    /// [P1] Motivating: selection → reel is one click (the low-effort path),
    /// and membership state stays honest.
    #[gpui::test]
    fn popover_include_toggles_reel_membership(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-popover-include".into(),
                media_path: "/m.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: None,
                working_text: String::new(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: vec![PaneHighlight {
                    start_word: 3,
                    end_word: 9,
                    label: "thesis".into(),
                    note: String::new(),
                    annotation_id: "ann-1".into(),
                    researcher_supplied: true,
                }],
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.reel_clips = vec![reel_clip("ann-1", 3, 9, true, &[])];

            // Included → the tray flips the clip's include flag off.
            viewer.selection_popover = Some((3, 9));
            viewer.popover_toggle_reel_include(cx);
            assert_eq!(
                reel_clip_identities(viewer),
                vec![("ann-1".into(), false, vec![])],
                "the include toggle flips the included clip off"
            );
            assert!(viewer.selection_popover.is_none());

            // Excluded → the tray flips it back on.
            viewer.selection_popover = Some((3, 9));
            viewer.popover_toggle_reel_include(cx);
            assert_eq!(
                reel_clip_identities(viewer),
                vec![("ann-1".into(), true, vec![])],
                "the include toggle re-adds the highlight's clip"
            );
            assert!(viewer.selection_popover.is_none());

            // A fresh range dispatches the create path — an in-flight edit
            // surfaces the refusal, and no clip materializes.
            viewer.edit_request.state = ResourceState::Loading;
            viewer.selection_popover = Some((20, 22));
            viewer.popover_toggle_reel_include(cx);
            assert_eq!(
                reel_clip_identities(viewer),
                vec![("ann-1".into(), true, vec![])],
                "a fresh range rides the create dispatch, never a bare clip insert"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("Another media edit")),
                "the create arm surfaced the admission refusal: {:?}",
                viewer.status
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::popover_copy_time`
    /// expect: Copy time consumes the tray and writes the selection's
    /// time span to the clipboard in the panel's marks vocabulary
    /// ("in 0.3s out 0.5s" — the same readout the trim marks label
    /// uses); a view without word timings degrades honestly instead of
    /// copying a made-up span.
    /// [P1] Motivating: the copied span must be the exact selection, and
    /// degradation must surface, never fabricate.
    #[gpui::test]
    fn popover_copy_time_writes_the_span(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-popover-copy".into(),
                media_path: "/m.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(vec![
                    PaneWord {
                        word: "the".into(),
                        start_ms: 200,
                        end_ms: 290,
                    },
                    PaneWord {
                        word: "span".into(),
                        start_ms: 300,
                        end_ms: 390,
                    },
                    PaneWord {
                        word: "to".into(),
                        start_ms: 400,
                        end_ms: 470,
                    },
                    PaneWord {
                        word: "copy".into(),
                        start_ms: 480,
                        end_ms: 500,
                    },
                ]),
                working_text: String::new(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.selection_popover = Some((1, 3));
            viewer.popover_copy_time(cx);
            let copied = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .expect("the span is on the clipboard");
            assert_eq!(copied, "in 0.3s out 0.5s");
            assert!(
                viewer.selection_popover.is_none(),
                "the copy consumes the tray"
            );
        });

        // No word timings on the view: honest degradation, nothing new
        // copied (the clipboard keeps the earlier span — the degraded arm
        // writes nothing rather than a made-up span).
        viewer.update(cx, |viewer, cx| {
            viewer.transcript.as_mut().unwrap().words = None;
            viewer.selection_popover = Some((1, 3));
            viewer.popover_copy_time(cx);
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("No word timings")),
                "the degraded copy surfaces honestly: {:?}",
                viewer.status
            );
            let copied = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .expect("the clipboard still holds the earlier span");
            assert_eq!(
                copied, "in 0.3s out 0.5s",
                "the degraded arm writes nothing new"
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::close_transcript_workspace`
    /// expect: The selection popover tears down with the surface — the
    /// close path, a fresh open, and a surface switch all reset it
    /// (teardown symmetry: the tray belongs to the word surface it was
    /// opened on).
    /// [P1] Motivating: teardown asymmetries are latent traps for later
    /// surfaces (the S1 hygiene rule).
    #[gpui::test]
    fn selection_popover_resets_with_the_surface(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-popover-teardown".into(),
                media_path: "/m.wav".into(),
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.selection_popover = Some((3, 9));
            viewer.close_transcript_workspace(cx);
            assert!(
                viewer.selection_popover.is_none(),
                "the close path resets the tray"
            );

            viewer.selection_popover = Some((3, 9));
            viewer.open_transcript("t-popover-reopen".into(), cx);
            assert!(
                viewer.selection_popover.is_none(),
                "the open path resets the tray"
            );

            // Re-establish the word surface's state (close/open cleared
            // it) — the surface switch must reset the tray too.
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-popover-teardown".into(),
                media_path: "/m.wav".into(),
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.selection_popover = Some((3, 9));
            viewer.toggle_transcript_surface(TranscriptSurface::Events, cx);
            assert!(
                viewer.selection_popover.is_none(),
                "leaving the word surface resets the tray"
            );
        });
    }

    /// dcterms:identifier: `transcript_search_matches`
    /// expect: The search matches the trimmed query case-insensitively over
    /// the working words joined by single spaces (the same join the blocks
    /// render), maps every occurrence to the inclusive word range it covers
    /// (phrases span words), never overlaps, matches nothing on an empty
    /// query or wordless view, and treats non-ASCII exactly (the server's
    /// `educt_search_transcripts` lexical rule, so the panel and the server
    /// agree on what a match is).
    /// [P1] Motivating: the match list is the search's ground truth — wrong
    /// ranges mark and jump to the wrong words.
    #[test]
    fn transcript_search_matches_orders_maps_and_edges() {
        let words = |texts: &[&str]| {
            texts
                .iter()
                .map(|text| PaneWord {
                    word: (*text).into(),
                    start_ms: 0,
                    end_ms: 0,
                })
                .collect::<Vec<_>>()
        };
        let transcript = words(&["We", "see", "the", "Dunning", "effect", "in", "dunning"]);
        assert_eq!(
            transcript_search_matches(&transcript, "dunning"),
            vec![(3, 3), (6, 6)],
            "case-insensitive, ordered, word-mapped"
        );
        assert_eq!(
            transcript_search_matches(&transcript, "the dunning effect"),
            vec![(2, 4)],
            "a phrase maps to the covering word range"
        );
        assert_eq!(
            transcript_search_matches(&transcript, "we see"),
            vec![(0, 1)],
            "a lowercase query matches capitalized words"
        );
        assert!(
            transcript_search_matches(&transcript, "absent").is_empty(),
            "no occurrence is an empty list, never silence downstream"
        );
        assert!(
            transcript_search_matches(&transcript, "   ").is_empty(),
            "a whitespace-only query matches nothing"
        );
        assert!(
            transcript_search_matches(&[], "dunning").is_empty(),
            "a wordless view matches nothing"
        );
        assert_eq!(
            transcript_search_matches(&words(&["aaaa"]), "aa"),
            vec![(0, 0), (0, 0)],
            "adjacent occurrences count separately and never overlap"
        );
        // Non-ASCII is exact (the server's rule): the accented word does
        // not answer to the plain query and vice versa.
        let accented = words(&["Café", "cafe"]);
        assert_eq!(transcript_search_matches(&accented, "café"), vec![(0, 0)]);
        assert_eq!(transcript_search_matches(&accented, "CAFE"), vec![(1, 1)]);
        assert_eq!(transcript_search_matches(&accented, "cafe"), vec![(1, 1)]);
    }

    /// dcterms:identifier: `transcript_search_status`
    /// expect: The search status is honest at every state — no label for a
    /// blank query, the word-timing degradation note on text-only views, "No
    /// matches" for zero results (never silence), and the current-position
    /// count while matches exist.
    /// [P1] Motivating: zero matches must surface, and a text-only view must
    /// say why it cannot search rather than claim absence.
    #[test]
    fn transcript_search_status_counts_zero_and_degrades() {
        let mut search = TranscriptSearch::default();
        assert_eq!(transcript_search_status(&search, true), None);
        search.query = LabelBuffer::from_str("dunning");
        assert_eq!(
            transcript_search_status(&search, false).as_deref(),
            Some("Search needs a word-timed view"),
            "a text-only view names why it cannot search, not 'no matches'"
        );
        assert_eq!(
            transcript_search_status(&search, true).as_deref(),
            Some("No matches"),
            "zero matches surface, never silence"
        );
        search.matches = vec![(3, 3), (9, 9), (12, 12)];
        search.current = 1;
        assert_eq!(
            transcript_search_status(&search, true).as_deref(),
            Some("2/3"),
            "the count names the current match's position"
        );
    }

    /// dcterms:identifier: `reset_transcript_search`
    /// expect: The search tears down with the surface — the close path, a
    /// fresh open, and a fresh workspace reload all clear the query, the
    /// matches, the pending rescan, and the follow custody (stale matches
    /// would mark words that no longer exist in the reloaded view).
    /// [P1] Motivating: teardown asymmetries are latent traps (the S1
    /// hygiene rule).
    #[gpui::test]
    fn transcript_search_resets_with_the_surface(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(pane_state("t-search-teardown"));
            let populate = |viewer: &mut MediaViewer, cx: &mut Context<MediaViewer>| {
                viewer.transcript_search.query = LabelBuffer::from_str("dunning");
                viewer.transcript_search.matches = vec![(3, 3)];
                viewer.transcript_search.current = 0;
                viewer.schedule_transcript_search_rescan(cx);
            };
            let assert_cleared = |viewer: &MediaViewer| {
                assert_eq!(viewer.transcript_search.query.text(), "");
                assert!(viewer.transcript_search.matches.is_empty());
                assert_eq!(viewer.transcript_search.current, 0);
                assert!(
                    viewer.transcript_search_task.is_none(),
                    "the pending rescan cancels with the surface"
                );
                assert!(!viewer.transcript_search_suspended_follow);
            };
            populate(viewer, cx);
            viewer.close_transcript_workspace(cx);
            assert_cleared(viewer);

            populate(viewer, cx);
            viewer.open_transcript("t-search-open".into(), cx);
            assert_cleared(viewer);

            // The reload path: a fresh workspace state invalidates the
            // pending search the same way.
            populate(viewer, cx);
            let epoch = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-search-open".into()))
                .expect("the reload owns its request");
            viewer.apply_transcript_result(epoch, Ok(pane_state("t-search-open")), cx);
            assert_cleared(viewer);
        });
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

    // ── S5: in-panel layered transcription correction ─────────────────────
    //
    // The correction flow rides S2's selection tray, the label-editor
    // machinery, and the edit admission; the pins below cover the tray's
    // mode contract (the re_anchored correction path, D5 v2), the wire
    // schema (built from the server's own layer types), the strict parse
    // of the live correction layer's edits, and the full scripted dispatch
    // chain on both view kinds.

    /// dcterms:identifier: `selection_tray_mode`
    /// expect: Every alignment answers the tray gate — original/aligned
    /// views open the full action tray, a re_anchored view opens the
    /// correction tray (the locate-translated path, D5 v2), and the
    /// unselectable states keep the visible refusal. [P1] Motivating: the
    /// D5 v1 blanket refusal hid the one action that can translate
    /// selections on corrected transcripts; the unaligned/untimed
    /// refusals stay honest (no timed words to select over).
    #[test]
    fn selection_tray_mode_covers_every_alignment() {
        assert!(matches!(
            selection_tray_mode("original"),
            Ok(SelectionTrayMode::Full)
        ));
        assert!(matches!(
            selection_tray_mode("aligned"),
            Ok(SelectionTrayMode::Full)
        ));
        assert!(matches!(
            selection_tray_mode("re_anchored"),
            Ok(SelectionTrayMode::CorrectOnly)
        ));
        for alignment in ["unaligned", "untimed", "unexpected"] {
            let reason = selection_tray_mode(alignment).expect_err("blocked");
            assert!(
                reason.starts_with("Direct selection"),
                "{alignment}: {reason}"
            );
            assert!(!reason.contains("index-identical"), "{alignment}: {reason}");
        }
    }

    /// dcterms:identifier: `correction_store_params`
    /// expect: The correction store request carries the server's layer
    /// contract verbatim — kind correction, researcher provenance (model
    /// "user", the researcher_correction authoring path), and the merged
    /// edits over SOURCE word ranges; the realign request names the stored
    /// layer; the locate request declares basis "source". [P1]
    /// Motivating: a hand-rolled layer JSON drifts from the server's serde
    /// types silently — building it from the server's own types and
    /// pinning the wire shape keeps the schema pinned byte-for-byte.
    #[test]
    fn correction_params_carry_the_exact_contract() {
        let edits = vec![PaneCorrectionEdit {
            start_word: 3,
            end_word: 5,
            replacement: "fixed words".into(),
            reason: CORRECTION_REASON.to_string(),
        }];
        let store = correction_store_params("t-1", &edits, "2026-11-06T00:00:00Z")
            .expect("the server layer types serialize");
        assert_eq!(store["transcript_id"], "t-1");
        assert_eq!(store["layer"]["kind"], "correction");
        assert_eq!(store["layer"]["provenance"]["model"], "user");
        assert_eq!(
            store["layer"]["provenance"]["prompt_template"],
            "researcher_correction"
        );
        assert_eq!(
            store["layer"]["provenance"]["created_at"],
            "2026-11-06T00:00:00Z"
        );
        assert_eq!(store["layer"]["edits"][0]["start_word"], 3);
        assert_eq!(store["layer"]["edits"][0]["end_word"], 5);
        assert_eq!(store["layer"]["edits"][0]["replacement"], "fixed words");
        assert_eq!(store["layer"]["edits"][0]["reason"], CORRECTION_REASON);
        let realign = correction_realign_params("t-1", "layer-9");
        assert_eq!(realign["transcript_id"], "t-1");
        assert_eq!(realign["layer_id"], "layer-9");
        let locate = correction_locate_params("t-1", "misheard words");
        assert_eq!(locate["transcript_id"], "t-1");
        assert_eq!(locate["text"], "misheard words");
        assert_eq!(locate["basis"], "source");
    }

    /// dcterms:identifier: `parse_transcript_payload`
    /// expect: The live correction layer's edits parse strictly — the
    /// working view names it by correction_layer_id; an absent id parses as
    /// no prior edits; a named layer missing from the records rejects the
    /// payload rather than silently dropping prior corrections. [P1]
    /// Motivating: a dropped prior edit would silently revert an earlier
    /// correction at the next merge (the silent-revert guard).
    #[test]
    fn correction_layer_edits_parse_strictly() {
        let payload = serde_json::json!({
            "summary": {"id": "t-1", "media_path": "/m", "words_count": 3, "has_word_timings": true},
            "working_transcript": {"text": "a b c", "alignment": "aligned", "correction_layer_id": "layer-c1", "words": [
                {"word": "a", "start_ms": 0, "end_ms": 100},
                {"word": "b", "start_ms": 100, "end_ms": 200},
                {"word": "c", "start_ms": 200, "end_ms": 300}
            ]},
            "layers": [
                {"id": "layer-c1", "created_at": "c1", "layer": {
                    "kind": "correction",
                    "provenance": {"model": "user", "prompt_template": "researcher_correction", "created_at": "c1"},
                    "edits": [{"start_word": 1, "end_word": 1, "replacement": "bee", "reason": "fix"}]
                }}
            ],
            "exports": [], "renders": []
        });
        let state = parse_transcript_payload(&payload).expect("parses");
        assert_eq!(
            state.correction_edits,
            vec![PaneCorrectionEdit {
                start_word: 1,
                end_word: 1,
                replacement: "bee".into(),
                reason: "fix".into(),
            }]
        );

        // No live correction → no prior edits.
        let uncorrected = serde_json::json!({
            "summary": {"id": "t-2", "media_path": "/m", "words_count": 1, "has_word_timings": true},
            "working_transcript": {"text": "a", "alignment": "original", "words": [
                {"word": "a", "start_ms": 0, "end_ms": 100}
            ]},
            "layers": [],
            "exports": [], "renders": []
        });
        assert!(
            parse_transcript_payload(&uncorrected)
                .expect("parses")
                .correction_edits
                .is_empty()
        );

        // A named layer missing from the records rejects the payload —
        // never a silent drop of prior corrections.
        let mut orphan = payload;
        orphan["working_transcript"]["correction_layer_id"] = serde_json::json!("layer-gone");
        assert!(parse_transcript_payload(&orphan).is_err());
    }

    /// The reloaded view Pin D's chain answers `educt_get_transcript` with:
    /// the original transcript corrected one-for-one over words [0..=1].
    const CORRECTED_VIEW_PAYLOAD: &str = r#"{"summary": {"id": "t-correct", "media_path": "/m", "words_count": 3, "has_word_timings": true}, "working_transcript": {"text": "measured words here", "alignment": "aligned", "correction_layer_id": "layer-new", "words": [{"word": "measured", "start_ms": 0, "end_ms": 500}, {"word": "words", "start_ms": 500, "end_ms": 900}, {"word": "here", "start_ms": 900, "end_ms": 1300}]}, "layers": [{"id": "layer-new", "created_at": "c1", "layer": {"kind": "correction", "provenance": {"model": "user", "prompt_template": "researcher_correction", "created_at": "c1"}, "edits": [{"start_word": 0, "end_word": 1, "replacement": "measured words", "reason": "researcher correction from the transcript workspace selection"}]}}], "exports": [], "renders": []}"#;

    /// dcterms:identifier: `MediaViewer::dispatch_workspace_correction`
    /// expect: Confirming a correction over working words [a..b] stores a
    /// merged correction layer through educt_store_layer (working indices
    /// equal source indices on an original view), chains
    /// educt_realign_transcript naming the stored layer, and the reloaded
    /// working view shows the replacement at [a..b] — the request touches
    /// only the layer, never the source bundle. [P1] Motivating: the
    /// acceptance is the correction landing in the working view (the
    /// layered discipline: source words are never mutated).
    #[gpui::test]
    fn correction_appears_in_the_working_view(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let (calls, _restore) = install_scripted_invoker(vec![
            r#"{"stored": {"id": "layer-new"}}"#.to_string(),
            r#"{"alignment": "aligned", "note": "correction is already one-for-one aligned; no re-anchoring needed", "applied_layer": {"id": "layer-new"}}"#.to_string(),
            CORRECTED_VIEW_PAYLOAD.to_string(),
        ]);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let mut state = pane_state("t-correct");
            state.words = Some(vec![
                PaneWord {
                    word: "misheard".into(),
                    start_ms: 0,
                    end_ms: 400,
                },
                PaneWord {
                    word: "wrods".into(),
                    start_ms: 400,
                    end_ms: 800,
                },
                PaneWord {
                    word: "here".into(),
                    start_ms: 800,
                    end_ms: 1200,
                },
            ]);
            viewer.transcript = Some(state);
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Correction {
                    start_word: 0,
                    end_word: 1,
                },
                buffer: LabelBuffer::from_str("measured words"),
            });
            viewer.confirm_label_edit(cx);
        });
        cx.run_until_parked();

        let recorded = calls.lock().unwrap().clone();
        assert_eq!(
            recorded
                .iter()
                .map(|(tool, _)| tool.as_str())
                .collect::<Vec<_>>(),
            [
                "educt_store_layer",
                "educt_realign_transcript",
                "educt_get_transcript"
            ],
            "store → realign → reload, in order"
        );
        // The store request: the layer anchors the selection's range (the
        // source-identical indices of an original view) with the typed
        // replacement, under the researcher authoring path.
        assert_eq!(recorded[0].1["transcript_id"], "t-correct");
        assert_eq!(recorded[0].1["layer"]["kind"], "correction");
        assert_eq!(recorded[0].1["layer"]["edits"][0]["start_word"], 0);
        assert_eq!(recorded[0].1["layer"]["edits"][0]["end_word"], 1);
        assert_eq!(
            recorded[0].1["layer"]["edits"][0]["replacement"],
            "measured words"
        );
        // The realign names the stored layer.
        assert_eq!(recorded[1].1["layer_id"], "layer-new");
        // The reload landed: the working view carries the correction.
        viewer.update(cx, |viewer, _| {
            let state = viewer.transcript.as_ref().expect("reloaded");
            let words = state.words.as_deref().expect("timed view");
            assert_eq!(words[0].word, "measured");
            assert_eq!(words[1].word, "words");
            assert_eq!(words[2].word, "here");
            assert_eq!(state.alignment, "aligned");
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("Correction applied")),
                "the completion surfaces: {:?}",
                viewer.status
            );
        });
    }

    /// The reloaded view Pin E's chain answers `educt_get_transcript` with:
    /// the re_anchored working view after the second correction.
    const REANCHORED_VIEW_PAYLOAD: &str = r#"{"summary": {"id": "t-reanchored", "media_path": "/m", "words_count": 9, "has_word_timings": true}, "working_transcript": {"text": "kept words here", "alignment": "re_anchored", "correction_layer_id": "layer-second", "words": [{"word": "kept", "start_ms": 0, "end_ms": 100}, {"word": "words", "start_ms": 100, "end_ms": 200}, {"word": "here", "start_ms": 200, "end_ms": 300}]}, "layers": [{"id": "layer-second", "created_at": "c2", "layer": {"kind": "correction", "provenance": {"model": "user", "prompt_template": "researcher_correction", "created_at": "c2"}, "edits": [{"start_word": 40, "end_word": 41, "replacement": "words here", "reason": "researcher correction from the transcript workspace selection"}]}}], "exports": [], "renders": []}"#;

    /// dcterms:identifier: `MediaViewer::dispatch_workspace_correction`
    /// expect: On a re_anchored view the dispatch translates the working
    /// selection to SOURCE word indices through educt_locate basis
    /// "source" (disambiguated by the candidate's working range — the
    /// `.rules` retranscribe trap), carries disjoint prior corrections
    /// verbatim, supersedes the overlapping one visibly, and chains
    /// educt_realign_transcript; the reloaded view reads re_anchored. [P1]
    /// Motivating: a correction anchored to working indices on a
    /// cardinality-changed view lands shifted by the cumulative delta
    /// (observed 2026-10-25) — the source range must come from the
    /// server's own index map.
    #[gpui::test]
    fn reanchored_correction_translates_and_chains_realign(cx: &mut gpui::TestAppContext) {
        let _invoker_lock = INVOKER_TEST_LOCK.lock().unwrap();
        let (calls, _restore) = install_scripted_invoker(vec![
            r#"{"status": "located", "ranges": [{"start_word": 40, "end_word": 41, "start_ms": 100, "end_ms": 300, "working_start_word": 1, "working_end_word": 2}], "count": 1, "index_basis": "source", "working_view": "realigned_working", "source_word_count": 9, "working_word_count": 9, "cardinality_changed": false}"#.to_string(),
            r#"{"stored": {"id": "layer-second"}}"#.to_string(),
            r#"{"alignment": "re_anchored", "reanchored": {"correction_layer_id": "layer-second"}}"#.to_string(),
            REANCHORED_VIEW_PAYLOAD.to_string(),
        ]);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            let mut state = pane_state("t-reanchored");
            state.alignment = "re_anchored".into();
            state.words = Some(vec![
                PaneWord {
                    word: "kept".into(),
                    start_ms: 0,
                    end_ms: 100,
                },
                PaneWord {
                    word: "wrods".into(),
                    start_ms: 100,
                    end_ms: 200,
                },
                PaneWord {
                    word: "heer".into(),
                    start_ms: 200,
                    end_ms: 300,
                },
            ]);
            state.correction_edits = vec![
                // A disjoint prior correction rides verbatim.
                PaneCorrectionEdit {
                    start_word: 7,
                    end_word: 7,
                    replacement: "prior-kept".into(),
                    reason: "prior".into(),
                },
                // A prior correction overlapping the translated source
                // range is superseded visibly.
                PaneCorrectionEdit {
                    start_word: 41,
                    end_word: 43,
                    replacement: "prior-overlap".into(),
                    reason: "prior".into(),
                },
            ];
            viewer.transcript = Some(state);
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::Correction {
                    start_word: 1,
                    end_word: 2,
                },
                buffer: LabelBuffer::from_str("words here"),
            });
            viewer.confirm_label_edit(cx);
        });
        cx.run_until_parked();

        let recorded = calls.lock().unwrap().clone();
        assert_eq!(
            recorded
                .iter()
                .map(|(tool, _)| tool.as_str())
                .collect::<Vec<_>>(),
            [
                "educt_locate",
                "educt_store_layer",
                "educt_realign_transcript",
                "educt_get_transcript"
            ],
            "locate → store → realign → reload, in order"
        );
        // The locate request quotes the selection's rendered form and
        // declares the source basis.
        assert_eq!(recorded[0].1["text"], "wrods heer");
        assert_eq!(recorded[0].1["basis"], "source");
        // THE TRAP PIN: the new edit anchors the TRANSLATED source range,
        // never the working range (1..=2) — and the disjoint prior edit
        // rides verbatim while the overlapping one is superseded.
        let edits = &recorded[1].1["layer"]["edits"];
        let edits = edits.as_array().expect("edits array");
        assert_eq!(edits.len(), 2, "kept prior + new edit: {edits:?}");
        assert_eq!(
            edits[0]["start_word"], 7,
            "the disjoint prior rides verbatim"
        );
        assert_eq!(edits[0]["replacement"], "prior-kept");
        assert_eq!(
            edits[1]["start_word"], 40,
            "the new edit anchors the SOURCE range"
        );
        assert_eq!(edits[1]["end_word"], 41);
        assert_eq!(edits[1]["replacement"], "words here");
        assert_eq!(recorded[2].1["layer_id"], "layer-second");
        viewer.update(cx, |viewer, _| {
            let state = viewer.transcript.as_ref().expect("reloaded");
            assert_eq!(state.alignment, "re_anchored");
            assert_eq!(
                transcript_alignment_note(&state.alignment),
                Some("Estimated timings · review only")
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("superseded 1")),
                "the overlapping prior's supersession is surfaced: {:?}",
                viewer.status
            );
        });
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
    /// expect: The three compose modes build the reel EDL directly from
    /// the ordered clip list — assemble keeps only the included clips in
    /// composer order (a later clip that overlaps an earlier one keeps only
    /// its unplayed remainder; adjacent clips stay separate ops), reduce
    /// cuts every strike, mixed carries both; empty op sets yield None, and
    /// the pane_reel_compose provenance never matches the pane strike
    /// identity. [P1]
    #[test]
    fn compose_reel_params_builds_each_mode() {
        let state = TranscriptPaneState {
            transcript_id: "t-1".into(),
            media_path: "/m".into(),
            gallery_asset_id: None,
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
            ],
            strikes: vec![PaneStrike {
                start_word: 4,
                end_word: 4,
                annotation_id: "strike-4".into(),
            }],
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        };
        let clips = [
            reel_clip("layer-0", 0, 3, true, &[]),
            reel_clip("layer-2", 2, 5, true, &[]),
            reel_clip("layer-6", 6, 7, true, &[]),
        ];

        // Assemble: the ordered builder — the first clip keeps its range,
        // the overlapping second keeps only its unplayed remainder [4,5],
        // the adjacent third stays its own op.
        let assemble =
            compose_reel_params(&state, &clips, ReelComposeMode::Assemble).expect("assemble");
        assert_eq!(assemble["transcript_id"], serde_json::json!("t-1"));
        assert_eq!(assemble["layer"]["kind"], serde_json::json!("edl"));
        assert_eq!(
            assemble["layer"]["provenance"]["prompt_template"],
            serde_json::json!("pane_reel_compose")
        );
        let ops = assemble["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 3, "ordered keeps, the overlap trimmed");
        assert_eq!(ops[0]["op"], serde_json::json!("keep"));
        assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(0));
        assert_eq!(ops[0]["range"]["end_word"], serde_json::json!(3));
        assert_eq!(ops[1]["range"]["start_word"], serde_json::json!(4));
        assert_eq!(ops[1]["range"]["end_word"], serde_json::json!(5));
        assert_eq!(ops[2]["range"]["start_word"], serde_json::json!(6));
        assert_eq!(ops[2]["range"]["end_word"], serde_json::json!(7));

        // Reduce: one Cut op per strike, no keeps.
        let reduce = compose_reel_params(&state, &clips, ReelComposeMode::Reduce).expect("reduce");
        let ops = reduce["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["op"], serde_json::json!("cut"));
        assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(4));

        // Mixed: the ordered keeps plus the cut.
        let mixed = compose_reel_params(&state, &clips, ReelComposeMode::Mixed).expect("mixed");
        let ops = mixed["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 4);
        assert_eq!(ops[0]["op"], serde_json::json!("keep"));
        assert_eq!(ops[3]["op"], serde_json::json!("cut"));

        // Empty op sets yield None — and Mixed with nothing included
        // refuses rather than silently composing the reduce reel.
        let none_included = [reel_clip("layer-0", 0, 3, false, &[])];
        assert!(compose_reel_params(&state, &none_included, ReelComposeMode::Assemble).is_none());
        assert!(compose_reel_params(&state, &none_included, ReelComposeMode::Mixed).is_none());
        let no_strikes = TranscriptPaneState {
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            ..state
        };
        assert!(compose_reel_params(&no_strikes, &clips, ReelComposeMode::Reduce).is_none());

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

    // ── S6 reel composer pins ─────────────────────────────────────────────

    fn reel_highlight(annotation_id: &str, start_word: usize, end_word: usize) -> PaneHighlight {
        PaneHighlight {
            start_word,
            end_word,
            label: format!("{annotation_id} label"),
            note: "n".into(),
            annotation_id: annotation_id.into(),
            researcher_supplied: true,
        }
    }

    fn reel_clip(
        annotation_id: &str,
        start_word: usize,
        end_word: usize,
        include: bool,
        strikes: &[(usize, usize)],
    ) -> ReelClip {
        ReelClip {
            annotation_id: annotation_id.into(),
            start_word,
            end_word,
            label: format!("{annotation_id} label"),
            include,
            strikes: strikes.to_vec(),
        }
    }

    /// The composer's state as comparable identities: (annotation id,
    /// include, strikes) per clip, in composer order.
    fn reel_clip_identities(viewer: &MediaViewer) -> Vec<(String, bool, Vec<(usize, usize)>)> {
        viewer
            .reel_clips
            .iter()
            .map(|clip| {
                (
                    clip.annotation_id.clone(),
                    clip.include,
                    clip.strikes.clone(),
                )
            })
            .collect()
    }

    /// dcterms:identifier: `MediaViewer::rematch_reel_clips`
    /// expect: Composer state — order, inclusion, and per-clip in-reel
    /// strikes — survives every workspace reload, keyed to highlight
    /// identity (annotation id + source range): a fresh load composes every
    /// researcher highlight included in transcript order, an unrelated
    /// annotation edit reloads the workspace and the deltas survive the
    /// re-match, new researcher highlights join included at the tail,
    /// deleted highlights drop their clips, and a rename refreshes the
    /// clip's label. [P1]
    #[gpui::test]
    fn reel_composer_rebuilds_reorders_and_survives_reload(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());

        // Load 1: three researcher highlights compose [a, b, c], all
        // included (the landed fresh-workspace semantics).
        viewer.update(cx, |viewer, cx| {
            let epoch = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-reel".into()))
                .expect("the first load owns its request");
            let mut state = pane_state("t-reel");
            state.highlights = vec![
                reel_highlight("layer-a", 0, 3),
                reel_highlight("layer-b", 5, 8),
                reel_highlight("layer-c", 10, 12),
            ];
            viewer.apply_transcript_result(epoch, Ok(state), cx);
        });
        viewer.update(cx, |viewer, cx| {
            assert_eq!(
                reel_clip_identities(viewer),
                vec![
                    ("layer-a".into(), true, vec![]),
                    ("layer-b".into(), true, vec![]),
                    ("layer-c".into(), true, vec![]),
                ]
            );
            // Composer deltas: b before a, c excluded, b's tail struck in-reel.
            viewer.move_reel_clip(1, true, cx);
            viewer.toggle_reel_clip_include("layer-c", 10, 12, cx);
            viewer.selection_popover = Some((5, 7));
            viewer.popover_strike_in_reel(cx);
        });

        // Reload 2 — an unrelated annotation edit mid-compose: the same
        // highlights with a renamed label, an unrelated strike elsewhere, a
        // model-pass row, and a NEW researcher highlight.
        viewer.update(cx, |viewer, cx| {
            let epoch = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-reel".into()))
                .expect("the reload owns its request");
            let mut state = pane_state("t-reel");
            state.highlights = vec![
                PaneHighlight {
                    label: "renamed a".into(),
                    ..reel_highlight("layer-a", 0, 3)
                },
                reel_highlight("layer-b", 5, 8),
                reel_highlight("layer-c", 10, 12),
                reel_highlight("layer-d", 30, 32),
                PaneHighlight {
                    researcher_supplied: false,
                    ..reel_highlight("layer-model", 100, 110)
                },
            ];
            state.strikes = vec![PaneStrike {
                start_word: 40,
                end_word: 41,
                annotation_id: "unrelated-strike".into(),
            }];
            viewer.apply_transcript_result(epoch, Ok(state), cx);
        });
        viewer.update(cx, |viewer, _| {
            // The composer deltas survived the reload; d joined included at
            // the tail; the model row never composes.
            assert_eq!(
                reel_clip_identities(viewer),
                vec![
                    ("layer-b".into(), true, vec![(5, 7)]),
                    ("layer-a".into(), true, vec![]),
                    ("layer-c".into(), false, vec![]),
                    ("layer-d".into(), true, vec![]),
                ],
                "order, inclusion, and strikes survive the reload"
            );
            assert_eq!(
                viewer.reel_clips[1].label, "renamed a",
                "the re-match refreshes labels from the reloaded highlights"
            );
        });

        // Reload 3 — deleting a highlight drops its clip; the rest survives.
        viewer.update(cx, |viewer, cx| {
            let epoch = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-reel".into()))
                .expect("the reload owns its request");
            let mut state = pane_state("t-reel");
            state.highlights = vec![
                reel_highlight("layer-a", 0, 3),
                reel_highlight("layer-b", 5, 8),
                reel_highlight("layer-d", 30, 32),
            ];
            viewer.apply_transcript_result(epoch, Ok(state), cx);
        });
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                reel_clip_identities(viewer),
                vec![
                    ("layer-b".into(), true, vec![(5, 7)]),
                    ("layer-a".into(), true, vec![]),
                    ("layer-d".into(), true, vec![]),
                ],
                "the deleted highlight's clip drops; the deltas survive"
            );
        });
    }

    /// dcterms:identifier: `composed_keep_ranges`
    /// expect: Reorder changes the composed op order — the composed clip
    /// list emits its Keep ops in composer order, and a later clip that
    /// overlaps an earlier one repeats only its unplayed remainder (the
    /// selection algebra admits each source span once — overlapping Keep
    /// ops are rejected — so [b(5,15), a(0,10)] composes keep(5,15) then
    /// keep(0,4), and the reverse order composes keep(0,10) then
    /// keep(11,15)). [P1]
    #[test]
    fn reorder_changes_composed_op_order() {
        let state = pane_state("t-order");
        let b_first = [
            reel_clip("layer-b", 5, 15, true, &[]),
            reel_clip("layer-a", 0, 10, true, &[]),
        ];
        let params =
            compose_reel_params(&state, &b_first, ReelComposeMode::Assemble).expect("compose");
        let ops = params["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0]["op"], serde_json::json!("keep"));
        assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(5));
        assert_eq!(ops[0]["range"]["end_word"], serde_json::json!(15));
        assert_eq!(ops[1]["range"]["start_word"], serde_json::json!(0));
        assert_eq!(
            ops[1]["range"]["end_word"],
            serde_json::json!(4),
            "the later clip repeats only its unplayed remainder"
        );

        let a_first = [
            reel_clip("layer-a", 0, 10, true, &[]),
            reel_clip("layer-b", 5, 15, true, &[]),
        ];
        let params =
            compose_reel_params(&state, &a_first, ReelComposeMode::Assemble).expect("compose");
        let ops = params["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0]["range"]["start_word"], serde_json::json!(0));
        assert_eq!(ops[0]["range"]["end_word"], serde_json::json!(10));
        assert_eq!(ops[1]["range"]["start_word"], serde_json::json!(11));
        assert_eq!(ops[1]["range"]["end_word"], serde_json::json!(15));
    }

    /// dcterms:identifier: `compose_reel_params` (S6 ordered form)
    /// expect: An in-reel strike bakes into the clip's Keep op through the
    /// server's published difference helper — the composer EDL carries
    /// ONLY Keep ops (never a Cut op: the algebra unions ALL cuts globally,
    /// so a per-clip strike as a Cut would strike every clip overlapping
    /// the range), the struck sub-range splits the clip into its flanking
    /// pieces, excluded clips compose nothing, and the pane_reel_compose
    /// provenance keeps the ordered form outside the pane strike identity.
    /// [P1]
    #[test]
    fn in_reel_strikes_bake_into_keeps_not_cuts() {
        let state = pane_state("t-bake");
        let clips = [
            reel_clip("layer-a", 0, 10, true, &[(3, 4)]),
            reel_clip("layer-b", 20, 24, false, &[]),
        ];
        let params =
            compose_reel_params(&state, &clips, ReelComposeMode::Assemble).expect("compose");
        assert_eq!(
            params["layer"]["provenance"]["prompt_template"],
            serde_json::json!("pane_reel_compose")
        );
        let ops = params["layer"]["ops"].as_array().expect("ops");
        assert_eq!(ops.len(), 2, "the strike splits the one clip's Keep op");
        assert!(
            ops.iter().all(|op| op["op"] == serde_json::json!("keep")),
            "a composer EDL carries no Cut ops — strikes are baked keeps"
        );
        assert_eq!(
            ops[0]["range"],
            serde_json::json!({"start_word": 0, "end_word": 2})
        );
        assert_eq!(
            ops[1]["range"],
            serde_json::json!({"start_word": 5, "end_word": 10})
        );

        // The ordered identity extension: the composed layer re-parsed as
        // a transcript layer renders no strike row (the pane strike
        // identity needs researcher_annotation + exactly one Cut op).
        let payload = serde_json::json!({
            "summary": {"id": "t-bake", "media_path": "/m", "words_count": 11, "has_word_timings": true},
            "working_transcript": {"text": "a b c", "alignment": "original", "words": [
                {"word": "a", "start_ms": 0, "end_ms": 100}
            ]},
            "layers": [
                {"id": "compose-ordered", "created_at": "c1", "layer": params["layer"].clone()}
            ],
            "exports": [], "renders": []
        });
        let parsed = parse_transcript_payload(&payload).expect("parses");
        assert!(
            parsed.strikes.is_empty(),
            "an ordered multi-Keep composer EDL is not a researcher strike"
        );
        assert_eq!(parsed.layers.len(), 1, "it stays in the Layers strip");
    }

    /// dcterms:identifier: `reel_preview_plan`
    /// expect: The preview's word→time mapping reads the stored SOURCE
    /// bundle words — the render path's own input — never the working
    /// view's timings: over a re_anchored fixture whose working timings are
    /// interpolated estimates over different indices, every preview span
    /// equals `edl_to_clip_plan`'s output for the same composed EDL over
    /// the source words, and differs from the working-view mapping. [P1]
    #[test]
    fn reel_preview_plan_reads_source_bundle_words() {
        let mut state = pane_state("t-preview");
        state.alignment = "re_anchored".into();
        // The source bundle words: six words, 0..=5, timings 0..1800ms.
        state.source_words = (0..6)
            .map(|index| PaneWord {
                word: format!("w{index}"),
                start_ms: index * 300,
                end_ms: index * 300 + 300,
            })
            .collect();
        // The re_anchored working view: interpolated estimates over
        // DIFFERENT indices — ten times the source timings.
        state.words = Some(
            (0..6)
                .map(|index| PaneWord {
                    word: format!("w{index}"),
                    start_ms: index * 3000,
                    end_ms: index * 3000 + 3000,
                })
                .collect(),
        );
        let clips = [
            reel_clip("layer-b", 2, 5, true, &[]),
            reel_clip("layer-a", 0, 1, true, &[]),
        ];
        let spans = reel_preview_plan(&state, &clips).expect("plan");

        // The render path's own plan over the same composed EDL and the
        // SOURCE bundle words — the equality the preview must hold.
        let keep_ranges = composed_keep_ranges(&clips);
        let edl = hkask_mcp_media::transcript_select::Edl {
            ops: keep_ranges
                .iter()
                .map(|range| hkask_mcp_media::transcript_select::EdlEntry {
                    range: *range,
                    op: hkask_mcp_media::transcript_select::EdlOp::Keep,
                })
                .collect(),
        };
        let timed = |words: &[PaneWord]| -> Vec<hkask_mcp_media::transcript::TimedWord> {
            words
                .iter()
                .map(|word| hkask_mcp_media::transcript::TimedWord {
                    word: word.word.clone(),
                    start_ms: word.start_ms,
                    end_ms: word.end_ms,
                    confidence: None,
                })
                .collect()
        };
        let source_words = timed(&state.source_words);
        let expected = hkask_mcp_media::transcript_select::edl_to_clip_plan(&source_words, &edl)
            .expect("the render plan maps");
        assert_eq!(spans, expected, "preview spans equal the render plan");
        assert_eq!(
            spans,
            vec![(600, 1800), (0, 600)],
            "the spans follow the composed order over the SOURCE timings"
        );
        let working_words = timed(
            state
                .words
                .as_deref()
                .expect("the fixture carries working words"),
        );
        let working_plan =
            hkask_mcp_media::transcript_select::edl_to_clip_plan(&working_words, &edl)
                .expect("the working-view plan maps");
        assert_ne!(
            spans, working_plan,
            "the preview never reads the working view's timings"
        );
    }

    /// The committed preview fixture (a 3-second WAV, the widget crate's
    /// playback-seek fixture copied in) — the real audio the scripted-widget
    /// preview test loads, seeks, and reads positions from.
    const REEL_PREVIEW_FIXTURE: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/reel-preview.wav");

    fn assert_player_position_near(
        cx: &gpui::TestAppContext,
        widget: &gpui::Entity<hkask_media_widget::MediaWidget>,
        expected_ms: f64,
    ) {
        let position = widget
            .read_with(cx, |widget, _| widget.transport_position_secs())
            .expect("the player reports a position");
        let expected_secs = expected_ms / 1000.0;
        assert!(
            (position - expected_secs).abs() < 0.05,
            "the player position {position}s must sit at the seek target {expected_secs}s"
        );
    }

    /// dcterms:identifier: `MediaViewer::toggle_reel_preview`
    /// expect: The preview plays the composed order on the shared player
    /// without rendering: starting primes a seek to the first span's start
    /// (observable as the real player's position landing off 0), the walk
    /// holds while playback is paused (the playing gate), crossing a
    /// span's end seeks the next span's start through the bound widget (a
    /// backward jump natural playback never makes — the landing is the
    /// seek's proof), and the last span's end finishes the preview. [P1]
    #[gpui::test]
    fn reel_preview_plays_spans_in_composed_order(cx: &mut gpui::TestAppContext) {
        hkask_media_widget::approve_gallery_media_path(std::path::Path::new(REEL_PREVIEW_FIXTURE))
            .expect("the committed preview fixture must be admittable");
        let widget = cx.new(|cx| {
            hkask_media_widget::MediaWidget::new(
                hkask_media_widget::MediaRef::new(
                    REEL_PREVIEW_FIXTURE.into(),
                    hkask_media_widget::MediaKind::Audio,
                ),
                cx,
            )
        });
        // Drive the widget's own pub load path (the production entry after
        // construction), then let the async file read + decode complete —
        // the error gate below is only meaningful once a load has run.
        widget.update(cx, |widget, cx| widget.load(cx));
        cx.run_until_parked();
        assert!(
            widget.read_with(cx, |widget, _| widget.error_message().is_none()),
            "the fixture must load — a seek on a failed (or never-run) load is a vacuous green"
        );

        // The workspace: source words over the 3-second fixture, the clips
        // composed [b(900..1500ms), a(0..600ms)].
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, _| {
            let mut state = pane_state("t-preview-order");
            state.source_words = (0..6)
                .map(|index| PaneWord {
                    word: format!("w{index}"),
                    start_ms: index * 300,
                    end_ms: index * 300 + 300,
                })
                .collect();
            viewer.transcript = Some(state);
            viewer.reel_clips = vec![
                reel_clip("layer-b", 3, 4, true, &[]),
                reel_clip("layer-a", 0, 1, true, &[]),
            ];
            viewer.media_widget = Some(widget.clone());
            viewer.media_widget_body = Some("fixture".into());
            viewer.active_tab = ViewerTab::Workspace;
        });
        viewer.update(cx, |viewer, cx| viewer.toggle_reel_preview(cx));
        viewer.update(cx, |viewer, _| {
            let preview = viewer.reel_preview.as_ref().expect("the preview armed");
            assert_eq!(
                preview.spans,
                vec![(900, 1500), (0, 600)],
                "the span plan follows the composed order"
            );
        });
        // The prime seek landed on the real player: the position reads the
        // first span's start, off the 0.0 the fixture loads paused at.
        assert_player_position_near(cx, &widget, 900.);

        // Paused: the walk holds (the playing gate — the tick keeps polling
        // without advancing).
        viewer.update(cx, |viewer, cx| {
            assert!(
                viewer.reel_preview_tick(cx),
                "the tick keeps polling while paused"
            );
            assert_eq!(
                viewer.reel_preview.as_ref().expect("held").index,
                0,
                "the walk never advances a paused player"
            );
        });

        // The walk, scripted through the production step: crossing span 1's
        // end seeks span 2's start through the bound widget — the position
        // lands at 0, a backward jump natural playback never makes.
        viewer.update(cx, |viewer, cx| viewer.step_reel_preview(1_500, cx));
        assert_player_position_near(cx, &widget, 0.);

        // Crossing the last span's end finishes the preview.
        viewer.update(cx, |viewer, cx| {
            viewer.step_reel_preview(600, cx);
            assert!(viewer.reel_preview.is_none(), "the preview finished");
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("finished")),
                "the finish surfaces: {:?}",
                viewer.status
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::stop_reel_preview`
    /// expect: The preview joins the sub-surface's teardown sites — leaving
    /// the workspace, a fresh open, closing the transcript, and every reload
    /// stop it and clear the span plan (the composer state itself survives
    /// reloads; only the preview tears down), and the tick exits on
    /// widget-None (hidden media clears the widget mid-preview). [P1]
    #[gpui::test]
    fn reel_preview_teardown_joins_the_surface_lifecycle(cx: &mut gpui::TestAppContext) {
        // Tab switches and opens dispatch reads through the shared
        // process-global invoker — the lock prevents cross-talk into a
        // concurrent lock-holder's scripted invoker.
        let _invoker_lock = INVOKER_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let viewer = cx.new(|_| MediaViewer::new());
        let arm_preview = |viewer: &mut MediaViewer, cx: &mut gpui::Context<MediaViewer>| {
            let mut state = pane_state("t-preview-teardown");
            state.source_words = vec![PaneWord {
                word: "a".into(),
                start_ms: 0,
                end_ms: 300,
            }];
            viewer.transcript = Some(state);
            viewer.reel_clips = vec![reel_clip("layer-a", 0, 0, true, &[])];
            viewer.active_tab = ViewerTab::Workspace;
            viewer.toggle_reel_preview(cx);
            assert!(viewer.reel_preview.is_some(), "the preview armed");
            assert!(viewer.reel_preview_task.is_some());
        };

        // Leaving the workspace tears the preview down.
        viewer.update(cx, |viewer, cx| {
            arm_preview(viewer, cx);
            viewer.activate_tab(ViewerTab::Library, cx);
        });
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer.reel_preview.is_none(),
                "leaving the workspace stops the preview"
            );
            assert!(viewer.reel_preview_task.is_none());
        });

        // A fresh open tears it down (open_transcript resets the surface).
        viewer.update(cx, |viewer, cx| {
            arm_preview(viewer, cx);
            viewer.open_transcript("t-other".into(), cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer.reel_preview.is_none(),
                "a fresh open stops the preview"
            );
            assert!(viewer.reel_preview_task.is_none());
        });

        // The reload path tears it down — while the composer state re-matches.
        viewer.update(cx, |viewer, cx| {
            arm_preview(viewer, cx);
            let epoch = viewer
                .transcript_request
                .begin(RequestOwner::Transcript("t-preview-teardown".into()))
                .expect("the reload owns its request");
            let mut state = pane_state("t-preview-teardown");
            state.source_words = vec![PaneWord {
                word: "a".into(),
                start_ms: 0,
                end_ms: 300,
            }];
            state.highlights = vec![reel_highlight("layer-a", 0, 0)];
            viewer.apply_transcript_result(epoch, Ok(state), cx);
            assert!(
                viewer.reel_preview.is_none(),
                "the reload stops the preview"
            );
            assert!(viewer.reel_preview_task.is_none());
            assert_eq!(
                reel_clip_identities(viewer),
                vec![("layer-a".into(), true, vec![])],
                "the composer state re-matched across the same reload"
            );
        });

        // widget-None: the tick exits and clears (hidden media suspends or
        // the selection changes mid-preview).
        viewer.update(cx, |viewer, cx| {
            arm_preview(viewer, cx);
            viewer.clear_media_widget(cx);
            assert!(
                !viewer.reel_preview_tick(cx),
                "the tick exits on widget-None"
            );
            assert!(viewer.reel_preview.is_none());
            assert!(viewer.reel_preview_task.is_none());
        });

        // Closing the transcript tears it down.
        viewer.update(cx, |viewer, cx| {
            arm_preview(viewer, cx);
            viewer.close_transcript_workspace(cx);
        });
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer.reel_preview.is_none(),
                "closing the transcript stops the preview"
            );
        });
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

    /// dcterms:identifier: `resolved_highlight_runs` (search composition)
    /// expect: Search marks compose with the researcher overlays field-wise
    /// — a match pushed after a highlight keeps its own background (the
    /// match stays visible inside the researcher's range), a struck match
    /// keeps the line-through, and the current match paints over the plain
    /// match (the render's push order: highlight, strike, match, current).
    /// [P1] Motivating: search must find words inside highlights and cuts —
    /// an overlay-erased match is a false negative on screen (S3 AC3).
    #[test]
    fn search_marks_compose_with_highlight_and_strike_overlays() {
        let range = 2..6;
        let highlight = HighlightStyle {
            background_color: Some(highlight_label_color("thesis")),
            ..Default::default()
        };
        let strike = HighlightStyle {
            strikethrough: Some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            }),
            ..Default::default()
        };
        let match_style = HighlightStyle {
            background_color: Some(gpui::white()),
            ..Default::default()
        };
        let current_style = HighlightStyle {
            background_color: Some(gpui::black()),
            ..Default::default()
        };
        let runs = resolved_highlight_runs(vec![
            (range.clone(), highlight),
            (range.clone(), strike),
            (range.clone(), match_style),
        ]);
        let merged = runs.first().expect("the marked range resolves");
        assert_eq!(merged.0, range, "the composed run keeps the word range");
        assert_eq!(
            merged.1.background_color,
            Some(gpui::white()),
            "the match's background wins over the highlight's — a match inside a highlight stays visible"
        );
        assert!(
            merged.1.strikethrough.is_some(),
            "a struck match keeps its line-through"
        );

        let runs =
            resolved_highlight_runs(vec![(range.clone(), match_style), (range, current_style)]);
        let merged = runs.first().expect("the marked range resolves");
        assert_eq!(
            merged.1.background_color,
            Some(gpui::black()),
            "the current match paints over the plain match"
        );
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
            viewer.reel_clips = vec![reel_clip("layer-0", 0, 2, true, &[])];
            viewer.compose_workspace_reel(ReelComposeMode::Assemble, cx);
            assert!(
                matches!(viewer.edit_request.state, ResourceState::Failed(_)),
                "the unwired compose refusal fails the edit request"
            );
            assert!(
                viewer.edit_progress.is_none(),
                "the refusal cleared the compose progress"
            );
            // Nothing included: the assemble empty status surfaces
            // without admitting an edit.
            viewer.edit_request.state = ResourceState::Idle;
            viewer.reel_clips = vec![reel_clip("layer-0", 0, 2, false, &[])];
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
            gallery_asset_id: None,
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
            correction_edits: Vec::new(),
            source_words: Vec::new(),
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
            gallery_asset_id: None,
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
            correction_edits: Vec::new(),
            source_words: Vec::new(),
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
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
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

    // ── Visual-transcript surface (S5) ───────────────────────────────────

    /// A canned `layers_transcript` response over two events: a shot row
    /// (no box) and an object row carrying the box that arms a redaction
    /// target.
    fn visual_events_response() -> serde_json::Value {
        serde_json::json!({
            "status": "completed",
            "media_path": "/rec/one.mp4",
            "events": [
                {
                    "start_sec": 0.0, "end_sec": 4.2, "kind": "shot",
                    "layer_id": "layer_shot", "item_id": "item_s0",
                    "label": null, "searchable_text": "shot 0",
                    "payload": {"shot_index": 0},
                    "frame_asset_ids": [],
                    "provenance": {"model": "ffmpeg:scdet", "prompt_template": "ffmpeg:scdet", "created_at": "2026-10-30T00:00:00Z"},
                    "evidence": ["seen"]
                },
                {
                    "start_sec": 3.5, "end_sec": 9.0, "kind": "object",
                    "layer_id": "layer_obj", "item_id": "item_a",
                    "label": "Grounded pass", "searchable_text": "object: person",
                    "payload": {"label": "person", "confidence": 0.9, "box": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4}, "frame_index": 2},
                    "frame_asset_ids": ["frame-1"],
                    "provenance": {"model": "qwen3-vl", "prompt_template": "layers_semantic_pass_grounded", "created_at": "2026-10-30T00:00:00Z"},
                    "evidence": ["heard", "seen"]
                }
            ],
            "events_total": 2, "events_page": 2,
            "kinds_present": ["shot", "object"],
            "layers_included": ["layer_shot", "layer_obj"],
            "resume_from": 0, "next_from": null,
            "time_basis": "source_pts_seconds"
        })
    }

    /// A viewer with an open transcript workspace over `/rec/one.mp4` —
    /// the events-surface tests' shared fixture.
    fn events_workspace_viewer() -> MediaViewer {
        let mut viewer = MediaViewer::new();
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-1".into(),
            media_path: "/rec/one.mp4".into(),
            gallery_asset_id: Some("ga-1".into()),
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
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        viewer
    }

    /// A scripted invoker for the S5 dispatches: records every (tool, args)
    /// call and answers from a queue of canned response texts.
    struct ScriptedInvoker {
        calls: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
        responses: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    }

    impl hkask_tool_invoker::ToolInvoker for ScriptedInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: Value,
        ) -> gpui::Task<Result<String, hkask_tool_invoker::InvokeError>> {
            assert_eq!(server, MEDIA_SERVER);
            self.calls
                .lock()
                .expect("calls lock")
                .push((tool.to_string(), args));
            let response = self
                .responses
                .lock()
                .expect("responses lock")
                .pop_front()
                .unwrap_or_else(|| "{}".to_string());
            gpui::Task::ready(Ok(response))
        }
    }

    /// Install a scripted invoker, returning (recorded-calls, restore). The
    /// caller holds INVOKER_TEST_LOCK for the test's lifetime — the same
    /// serialization discipline as the direct-install invoker tests.
    struct InvokerRestore(Option<std::sync::Arc<dyn hkask_tool_invoker::ToolInvoker>>);
    impl Drop for InvokerRestore {
        fn drop(&mut self) {
            hkask_tool_invoker::set_tool_invoker(self.0.take());
        }
    }

    fn install_scripted_invoker(
        responses: Vec<String>,
    ) -> (
        std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
        InvokerRestore,
    ) {
        let restore = InvokerRestore(hkask_tool_invoker::shared_tool_invoker());
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(ScriptedInvoker {
            calls: calls.clone(),
            responses: std::sync::Arc::new(std::sync::Mutex::new(responses.into_iter().collect())),
        })));
        (calls, restore)
    }

    /// dcterms:identifier: `parse_visual_events`
    /// expect: The event log parses strictly or rejects, never degrading to
    /// a partial success; the no_layers state is its own named status.
    #[test]
    fn parse_visual_events_is_strict() {
        let parsed =
            parse_visual_events(&visual_events_response()).expect("the completed response parses");
        assert_eq!(parsed.status, "completed");
        assert_eq!(parsed.events_total, 2);
        assert_eq!(
            parsed.kinds_present,
            vec!["shot".to_string(), "object".to_string()]
        );
        assert_eq!(parsed.events.len(), 2);
        let object = &parsed.events[1];
        assert_eq!(object.box_norm, Some([0.1, 0.2, 0.3, 0.4]));
        assert_eq!(object.object_label.as_deref(), Some("person"));
        assert_eq!(object.source_ref(), "layer_obj/item_a");
        assert_eq!(object.model, "qwen3-vl");
        let shot = &parsed.events[0];
        assert_eq!(shot.box_norm, None);
        assert_eq!(shot.end_or_start(), 4.2);

        let no_layers = serde_json::json!({
            "status": "no_layers", "media_path": "/rec/one.mp4", "events": [],
            "note": "no stored layers for this recording"
        });
        let parsed = parse_visual_events(&no_layers).expect("no_layers parses");
        assert_eq!(parsed.status, "no_layers");
        assert!(parsed.events.is_empty());
        assert_eq!(
            parsed.note.as_deref(),
            Some("no stored layers for this recording")
        );

        let object_event = &visual_events_response()["events"][1];
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"status": "pending", "events": []}),
            serde_json::json!({"status": "completed"}),
            serde_json::json!({"status": "no_layers", "events": [object_event.clone()]}),
            serde_json::json!({"status": "completed", "events": [serde_json::json!({
                "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
                "searchable_text": "x", "provenance": {"model": "m"}
            })]}),
            serde_json::json!({"status": "completed", "events": [serde_json::json!({
                "start_sec": -1.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
                "searchable_text": "x", "provenance": {"model": "m"}
            })]}),
            serde_json::json!({"status": "completed", "events": [serde_json::json!({
                "start_sec": 5.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
                "searchable_text": "x", "provenance": {"model": "m"}
            })]}),
            serde_json::json!({"status": "completed", "events": [serde_json::json!({
                "start_sec": 0.0, "end_sec": 4.0, "kind": "  ", "layer_id": "l", "item_id": "i",
                "searchable_text": "x", "provenance": {"model": "m"}
            })]}),
            serde_json::json!({"status": "completed", "events": [serde_json::json!({
                "start_sec": 0.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
                "searchable_text": "  ", "provenance": {"model": "m"}
            })]}),
            serde_json::json!({"status": "completed", "events": [serde_json::json!({
                "start_sec": 0.0, "end_sec": 4.0, "kind": "shot", "layer_id": "l", "item_id": "i",
                "searchable_text": "x"
            })]}),
            serde_json::json!({"status": "completed", "events": [object_event.clone()],
                "events_total": 1}),
        ] {
            let err = parse_visual_events(&invalid)
                .expect_err("malformed payloads must reject, never partially parse");
            assert!(!err.is_empty(), "rejection names the failure");
        }
    }

    /// dcterms:identifier: `VisualEventsState::truncation_note`
    /// expect: A truncated page's note names the page-vs-total gap — the
    /// count the operator reads never silently claims completeness; a
    /// full page is not truncated and renders no note.
    #[test]
    fn truncation_note_names_page_and_total() {
        let mut state = parse_visual_events(&visual_events_response()).expect("fixture parses");
        assert!(!state.truncated(), "a full page is not truncated");
        state.events_total = 1200;
        assert!(state.truncated());
        assert_eq!(
            state.truncation_note(),
            "showing 2 of 1200 events — refine your search or window"
        );
    }

    /// dcterms:identifier: `parse_redaction_render`
    /// expect: The S3 render result parses into the review/lineage summary
    /// or rejects — segments and applied items carry their full detail.
    #[test]
    fn parse_redaction_render_is_strict() {
        let render = serde_json::json!({
            "status": "rendered",
            "result": {"output": "/published/redacted.mp4", "kind": "video"},
            "decision_layer_id": "layer-decision",
            "media_path": "/rec/one.mp4",
            "segments": [
                {"mode": "stream_copy", "start_sec": 0.0, "end_sec": 12.0, "items": []},
                {"mode": "reencode", "start_sec": 12.0, "end_sec": 18.0, "items": ["item_d1"]},
                {"mode": "stream_copy", "start_sec": 18.0, "end_sec": 20.0, "items": []}
            ],
            "reencoded_segments": 1,
            "stream_copied_segments": 2,
            "decisions_rendered": 1,
            "applied_items": [{
                "item_id": "item_d1", "effect": "blur",
                "pixel_x": 16, "pixel_y": 12, "pixel_w": 32, "pixel_h": 24,
                "local_start_sec": 0.4, "local_end_sec": 5.6, "delogo_edge_inset": false
            }]
        });
        let parsed = parse_redaction_render(&render).expect("the rendered result parses");
        assert_eq!(parsed.decision_layer_id, "layer-decision");
        assert_eq!(parsed.decisions_rendered, 1);
        assert_eq!(parsed.reencoded_segments, 1);
        assert_eq!(parsed.stream_copied_segments, 2);
        assert_eq!(parsed.segments.len(), 3);
        assert_eq!(
            parsed.segments[1],
            PaneRenderSegment {
                mode: "reencode".into(),
                start_sec: 12.0,
                end_sec: 18.0,
                items: 1,
            }
        );
        assert_eq!(parsed.applied.len(), 1);
        assert_eq!(parsed.applied[0].pixel_w, 32);
        assert_eq!(parsed.applied[0].local_end_sec, 5.6);

        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"status": "failed"}),
            serde_json::json!({"status": "rendered"}),
            serde_json::json!({"status": "rendered", "decision_layer_id": "l"}),
            serde_json::json!({"status": "rendered", "decision_layer_id": "l", "decisions_rendered": 1,
                "reencoded_segments": 1, "stream_copied_segments": 1}),
            serde_json::json!({"status": "rendered", "decision_layer_id": "l", "decisions_rendered": 1,
                "reencoded_segments": 1, "stream_copied_segments": 1,
                "segments": [{"mode": "teleport", "start_sec": 0.0, "end_sec": 1.0, "items": []}]}),
            serde_json::json!({"status": "rendered", "decision_layer_id": "l", "decisions_rendered": 1,
                "reencoded_segments": 1, "stream_copied_segments": 1,
                "segments": [{"mode": "reencode", "start_sec": 0.0, "end_sec": 1.0, "items": []}],
                "applied_items": [{"item_id": "i", "effect": "blur", "pixel_x": 1}]}),
        ] {
            assert!(
                parse_redaction_render(&invalid).is_err(),
                "malformed render results must reject"
            );
        }
    }

    /// dcterms:identifier: `MediaViewer::handle_event_click`
    /// expect: The event-click state machine mirrors the word pane's —
    /// seek outside select mode, anchor → span inside it, inert while the
    /// editor is open — with span bounds over source-PTS ranges and a
    /// named refusal for degenerate spans.
    #[gpui::test]
    fn event_click_state_machine_and_span_bounds(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| {
            let mut viewer = events_workspace_viewer();
            viewer.visual_events = Some(VisualEventsState {
                status: "completed".into(),
                events: vec![
                    PaneEventRow {
                        start_sec: 3.5,
                        end_sec: Some(9.0),
                        kind: "object".into(),
                        layer_id: "layer_obj".into(),
                        item_id: "item_a".into(),
                        searchable_text: "object: person".into(),
                        model: "qwen3-vl".into(),
                        box_norm: Some([0.1, 0.2, 0.3, 0.4]),
                        object_label: Some("person".into()),
                        evidence: vec!["seen".into()],
                    },
                    PaneEventRow {
                        start_sec: 12.4,
                        end_sec: Some(17.6),
                        kind: "researcher".into(),
                        layer_id: "layer_dec".into(),
                        item_id: "item_r1".into(),
                        searchable_text: "redact: person (blur)".into(),
                        model: "user".into(),
                        box_norm: None,
                        object_label: None,
                        evidence: vec!["authored".into()],
                    },
                    PaneEventRow {
                        start_sec: 5.0,
                        end_sec: None,
                        kind: "audio_event".into(),
                        layer_id: "layer_a".into(),
                        item_id: "item_p".into(),
                        searchable_text: "sound: clap".into(),
                        model: "user".into(),
                        box_norm: None,
                        object_label: None,
                        evidence: vec!["heard".into()],
                    },
                    PaneEventRow {
                        start_sec: 5.0,
                        end_sec: None,
                        kind: "audio_event".into(),
                        layer_id: "layer_a".into(),
                        item_id: "item_q".into(),
                        searchable_text: "sound: clap".into(),
                        model: "user".into(),
                        box_norm: None,
                        object_label: None,
                        evidence: vec!["heard".into()],
                    },
                ],
                events_total: 4,
                kinds_present: vec!["object".into(), "researcher".into(), "audio_event".into()],
                note: None,
            });
            viewer
        });
        // Outside select mode every click seeks (criterion 1's routing).
        viewer.update(cx, |viewer, cx| {
            assert_eq!(viewer.handle_event_click(0), EventClickOutcome::SeekOnly);
            viewer.selection_mode = true;
            assert_eq!(
                viewer.handle_event_click(0),
                EventClickOutcome::AnchorSet(0)
            );
            viewer.event_selection_anchor = Some(0);
            assert_eq!(
                viewer.handle_event_click(0),
                EventClickOutcome::AnchorCleared
            );
            // The span covers from the earlier start to the later end,
            // regardless of click order.
            viewer.event_selection_anchor = Some(0);
            assert_eq!(
                viewer.handle_event_click(1),
                EventClickOutcome::SpanSelected {
                    start_sec: 3.5,
                    end_sec: 17.6
                }
            );
            viewer.event_selection_anchor = Some(1);
            assert_eq!(
                viewer.handle_event_click(0),
                EventClickOutcome::SpanSelected {
                    start_sec: 3.5,
                    end_sec: 17.6
                }
            );
            // A point event's end falls back to its start.
            viewer.event_selection_anchor = Some(2);
            assert_eq!(
                viewer.handle_event_click(0),
                EventClickOutcome::SpanSelected {
                    start_sec: 3.5,
                    end_sec: 9.0
                }
            );
            // Two same-instant point events cover no time — the named refusal.
            viewer.event_selection_anchor = Some(2);
            assert_eq!(
                viewer.handle_event_click(3),
                EventClickOutcome::SpanRejected
            );
            // The label editor is modal to event clicks.
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::EventTag {
                    start_sec: 1.0,
                    end_sec: 2.0,
                },
                buffer: LabelBuffer::default(),
            });
            assert_eq!(viewer.handle_event_click(0), EventClickOutcome::EditorOpen);
            // The degenerate-span refusal clears the anchor with a named status.
            viewer.highlight_edit = None;
            viewer.event_selection_anchor = Some(2);
            viewer.event_selection_span = None;
            viewer.event_clicked(3, 5.0, cx);
            assert_eq!(viewer.event_selection_anchor, None);
            assert_eq!(viewer.event_selection_span, None);
            let status = viewer.status.clone().expect("refusal surfaces a status");
            assert!(status.contains("span is empty"), "got: {status}");
            // The completed span routes through with its own status.
            viewer.event_selection_anchor = Some(0);
            viewer.event_clicked(1, 12.4, cx);
            assert_eq!(viewer.event_selection_span, Some((3.5, 17.6)));
            assert_eq!(viewer.event_selection_anchor, None);
        });
    }

    /// dcterms:identifier: `PaneRedactionEffect::next`
    /// expect: The effect chip cycles the card's tier-1 vocabulary.
    #[test]
    fn redaction_effect_cycles_the_card_vocabulary() {
        assert_eq!(
            PaneRedactionEffect::Blur.next(),
            PaneRedactionEffect::Pixelate
        );
        assert_eq!(
            PaneRedactionEffect::Pixelate.next(),
            PaneRedactionEffect::Black
        );
        assert_eq!(PaneRedactionEffect::Black.next(), PaneRedactionEffect::Blur);
        assert_eq!(PaneRedactionEffect::Blur.as_str(), "blur");
        assert_eq!(PaneRedactionEffect::Pixelate.as_str(), "pixelate");
        assert_eq!(PaneRedactionEffect::Black.as_str(), "black");
    }

    /// dcterms:identifier: `MediaViewer::toggle_transcript_surface`
    /// expect: Switching to Events dispatches exactly one pinned
    /// `layers_transcript` call for the open recording; retained state means
    /// switching back and forth re-dispatches nothing; the switch clears
    /// pending surface-local selections.
    #[gpui::test]
    fn surface_toggle_switches_and_loads_events_once(cx: &mut gpui::TestAppContext) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (calls, _restore) = install_scripted_invoker(vec![
            serde_json::json!({
                "content": visual_events_response()
            })
            .to_string(),
        ]);
        let viewer = cx.new(|_| {
            let mut viewer = events_workspace_viewer();
            viewer.selection_mode = true;
            viewer.event_selection_anchor = Some(0);
            viewer.event_selection_span = Some((1.0, 2.0));
            viewer
        });
        assert_eq!(
            viewer.read_with(cx, |viewer, _| viewer.transcript_surface),
            TranscriptSurface::Words
        );
        viewer.update(cx, |viewer, cx| {
            viewer.toggle_transcript_surface(TranscriptSurface::Events, cx)
        });
        assert_eq!(
            viewer.read_with(cx, |viewer, _| viewer.transcript_surface),
            TranscriptSurface::Events
        );
        // The switch cleared the pending selections that belonged to the
        // words surface's select mode.
        viewer.read_with(cx, |viewer, _| {
            assert_eq!(viewer.selection_anchor, None);
            assert_eq!(viewer.event_selection_anchor, None);
            assert_eq!(viewer.event_selection_span, None);
        });
        cx.run_until_parked();
        assert_eq!(
            viewer.read_with(cx, |viewer, _| viewer.visual_events_request.state.clone()),
            ResourceState::Ready
        );
        {
            let recorded = calls.lock().expect("calls lock");
            assert_eq!(recorded.len(), 1, "exactly one layers_transcript dispatch");
            assert_eq!(recorded[0].0, "layers_transcript");
            assert_eq!(
                recorded[0].1,
                serde_json::json!({ "media_path": "/rec/one.mp4", "limit": 5000 }),
                "the reading surface dispatches the pinned payload — the \
                 5000-event maximum page, never the 500-event default prefix"
            );
        }
        // Retained state: back to words and again dispatches nothing.
        viewer.update(cx, |viewer, cx| {
            viewer.toggle_transcript_surface(TranscriptSurface::Words, cx)
        });
        viewer.update(cx, |viewer, cx| {
            viewer.toggle_transcript_surface(TranscriptSurface::Events, cx)
        });
        cx.run_until_parked();
        assert_eq!(calls.lock().expect("calls lock").len(), 1);
    }

    /// dcterms:identifier: `MediaViewer::create_event_tag`
    /// expect: Selecting a span and confirming the label editor authors a
    /// researcher tag through `layers_store` with the exact human-act
    /// payload (kind researcher, model "user", one item over the span),
    /// then reloads the event log.
    #[gpui::test]
    fn select_to_author_tag_dispatches_strict_researcher_payload(cx: &mut gpui::TestAppContext) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store_response = serde_json::json!({
            "content": {
                "id": "layer-tag-1", "media_path": "/rec/one.mp4",
                "gallery_asset_id": null, "detached_at": null,
                "kind": "researcher", "label": "hot moment",
                "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author",
                    "created_at": "2026-10-30T00:00:00Z", "parameters": null},
                "items_count": 1, "created_at": "2026-10-30T00:00:00Z",
                "probed_duration_secs": 20.0, "time_basis": "source_pts_seconds"
            }
        })
        .to_string();
        let (calls, _restore) = install_scripted_invoker(vec![
            store_response,
            serde_json::json!({"content": visual_events_response()}).to_string(),
        ]);
        let viewer = cx.new(|_| {
            let mut viewer = events_workspace_viewer();
            viewer.event_selection_span = Some((3.5, 9.0));
            viewer.highlight_edit = Some(HighlightEdit {
                kind: HighlightEditKind::EventTag {
                    start_sec: 3.5,
                    end_sec: 9.0,
                },
                buffer: LabelBuffer::from_str("hot moment"),
            });
            viewer
        });
        viewer.update(cx, |viewer, cx| viewer.confirm_label_edit(cx));
        cx.run_until_parked();
        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 2, "store then reload");
        assert_eq!(calls[0].0, "layers_store");
        assert_eq!(
            calls[0].1,
            serde_json::json!({
                "media_path": "/rec/one.mp4",
                "gallery_asset_id": "ga-1",
                "kind": "researcher",
                "label": "hot moment",
                "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author"},
                "items": [{
                    "start_sec": 3.5,
                    "end_sec": 9.0,
                    "searchable_text": "tag: hot moment",
                    "payload": {"label": "hot moment"},
                }],
            }),
            "select-to-author dispatches the exact researcher payload"
        );
        assert_eq!(calls[1].0, "layers_transcript");
        drop(calls);
        viewer.read_with(cx, |viewer, _| {
            assert_eq!(
                viewer.event_selection_span, None,
                "the consumed span clears"
            );
            assert!(viewer.highlight_edit.is_none(), "the editor closed");
            let status = viewer.status.as_deref().expect("success surfaces a status");
            assert!(status.contains("tag stored"), "got: {status}");
        });
    }

    /// dcterms:identifier: `MediaViewer::arm_redaction_target`
    /// expect: Only box-carrying events arm the redaction target, and the
    /// armed target carries the OMC lineage source ref.
    #[gpui::test]
    fn target_arming_requires_a_box(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| {
            let mut viewer = events_workspace_viewer();
            viewer.visual_events =
                Some(parse_visual_events(&visual_events_response()).expect("fixture parses"));
            viewer
        });
        viewer.update(cx, |viewer, cx| {
            viewer.arm_redaction_target(0, cx);
            assert_eq!(viewer.redaction_target, None, "the shot row has no box");
            let status = viewer.status.as_deref().expect("the refusal surfaces");
            assert!(status.contains("box"), "got: {status}");
            viewer.status = None;
            viewer.arm_redaction_target(1, cx);
            assert_eq!(
                viewer.redaction_target,
                Some(RedactionTarget {
                    label: "person".into(),
                    box_norm: [0.1, 0.2, 0.3, 0.4],
                    source_ref: "layer_obj/item_a".into(),
                })
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::dispatch_redaction_render`
    /// expect: With target and span armed, the dispatch runs the pinned
    /// two-invoke chain — an approved decision layer through `layers_store`,
    /// then `layers_render_redactions` with the STORED layer id — commits
    /// the segments/applied-items summary, and reloads the event log.
    #[gpui::test]
    fn redaction_chain_pins_store_and_render_payloads(cx: &mut gpui::TestAppContext) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let decision_store_response = serde_json::json!({
            "content": {
                "id": "layer-decision", "media_path": "/rec/one.mp4",
                "gallery_asset_id": null, "detached_at": null,
                "kind": "researcher", "label": "redaction decisions",
                "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author",
                    "created_at": "2026-10-30T00:00:00Z", "parameters": null},
                "items_count": 1, "created_at": "2026-10-30T00:00:00Z",
                "probed_duration_secs": 20.0, "time_basis": "source_pts_seconds"
            }
        })
        .to_string();
        let render_response = serde_json::json!({
            "content": {
                "status": "rendered",
                "result": {"output": "/published/redacted.mp4", "kind": "video"},
                "decision_layer_id": "layer-decision",
                "media_path": "/rec/one.mp4",
                "segments": [
                    {"mode": "stream_copy", "start_sec": 0.0, "end_sec": 12.0, "items": []},
                    {"mode": "reencode", "start_sec": 12.0, "end_sec": 18.0, "items": ["item-1"]},
                    {"mode": "stream_copy", "start_sec": 18.0, "end_sec": 20.0, "items": []}
                ],
                "reencoded_segments": 1, "stream_copied_segments": 2, "decisions_rendered": 1,
                "applied_items": [{
                    "item_id": "item-1", "effect": "blur",
                    "pixel_x": 16, "pixel_y": 12, "pixel_w": 32, "pixel_h": 24,
                    "local_start_sec": 0.4, "local_end_sec": 5.6, "delogo_edge_inset": false
                }]
            }
        })
        .to_string();
        let (calls, _restore) = install_scripted_invoker(vec![
            decision_store_response,
            render_response,
            serde_json::json!({"content": visual_events_response()}).to_string(),
        ]);
        let viewer = cx.new(|_| {
            let mut viewer = events_workspace_viewer();
            viewer.visual_events =
                Some(parse_visual_events(&visual_events_response()).expect("fixture parses"));
            viewer.event_selection_span = Some((3.5, 9.0));
            viewer
        });
        viewer.update(cx, |viewer, cx| {
            viewer.arm_redaction_target(1, cx);
            viewer.dispatch_redaction_render(cx);
        });
        cx.run_until_parked();
        let calls = calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 3, "store, render, then the event-log reload");
        assert_eq!(calls[0].0, "layers_store");
        assert_eq!(
            calls[0].1,
            serde_json::json!({
                "media_path": "/rec/one.mp4",
                "gallery_asset_id": "ga-1",
                "kind": "researcher",
                "label": "redaction decisions",
                "provenance": {"model": "user", "prompt_template": "media-panel:select-to-author"},
                "items": [{
                    "start_sec": 3.5,
                    "end_sec": 9.0,
                    "searchable_text": "redact: person (blur)",
                    "payload": {
                        "op": "redact",
                        "status": "approved",
                        "effect": "blur",
                        "target": {"kind": "box", "box": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4}},
                        "source_tag_refs": ["layer_obj/item_a"],
                    },
                }],
            }),
            "the decision layer carries the exact S3 redaction vocabulary"
        );
        assert_eq!(calls[1].0, "layers_render_redactions");
        assert_eq!(
            calls[1].1,
            serde_json::json!({
                "media_path": "/rec/one.mp4",
                "layer_id": "layer-decision",
            }),
            "the render dispatches the STORED decision layer id (required)"
        );
        assert_eq!(calls[2].0, "layers_transcript");
        drop(calls);
        viewer.read_with(cx, |viewer, _| {
            let summary = viewer
                .redaction_render
                .as_ref()
                .expect("the render summary commits for review/lineage");
            assert_eq!(summary.decision_layer_id, "layer-decision");
            assert_eq!(summary.decisions_rendered, 1);
            assert_eq!(summary.reencoded_segments, 1);
            assert_eq!(summary.stream_copied_segments, 2);
            assert_eq!(summary.segments.len(), 3);
            assert_eq!(summary.applied[0].pixel_w, 32);
            assert_eq!(
                viewer.event_selection_span, None,
                "the consumed span clears"
            );
            let status = viewer.status.as_deref().expect("success surfaces a status");
            assert!(status.contains("Redaction rendered"), "got: {status}");
        });
    }

    /// dcterms:identifier: `MediaViewer::dispatch_redaction_render`
    /// expect: Missing target or span refuses with a named status and
    /// dispatches nothing — the authoring gate runs before any tool call.
    #[gpui::test]
    fn redaction_refuses_without_target_or_span(cx: &mut gpui::TestAppContext) {
        let _invoker_guard = INVOKER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (calls, _restore) = install_scripted_invoker(vec![]);
        let viewer = cx.new(|_| {
            let mut viewer = events_workspace_viewer();
            viewer.visual_events =
                Some(parse_visual_events(&visual_events_response()).expect("fixture parses"));
            viewer.event_selection_span = Some((3.5, 9.0));
            viewer
        });
        viewer.update(cx, |viewer, cx| {
            viewer.dispatch_redaction_render(cx);
            let status = viewer.status.as_deref().expect("the refusal surfaces");
            assert!(status.contains("Arm a target"), "got: {status}");
            viewer.arm_redaction_target(1, cx);
            viewer.event_selection_span = None;
            viewer.dispatch_redaction_render(cx);
            let status = viewer.status.as_deref().expect("the refusal surfaces");
            assert!(status.contains("Select a span"), "got: {status}");
        });
        assert_eq!(
            calls.lock().expect("calls lock").len(),
            0,
            "refusals dispatch nothing"
        );
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
    use crate::test_fixtures::scheduled_job;
    use gpui::{DefiniteLength, TestAppContext, px, size};

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/viewer-layout.mp4");

    /// Admit the committed fixture through the widget's locator-validation
    /// boundary (`approve_gallery_media_path`) — the same authority the
    /// viewer's ingest path uses for gallery-published media. The widget's
    /// cfg(test) approved root covers only its OWN crate's `test_data`; this
    /// crate's fixture lives outside both it and the artifacts dir, so
    /// without explicit admission the shared widget refuses to resolve the
    /// path and the video area never renders (the layout pins then fail).
    fn admit_fixture() {
        hkask_media_widget::approve_gallery_media_path(std::path::Path::new(FIXTURE))
            .expect("the committed layout fixture must be admittable");
    }

    /// Layout ground truth at the VIEWER level — the full production chain
    /// (tab bar → tab content → toolbar → player), not the widget in
    /// isolation. The widget-level test passed while the app was broken,
    /// so the break is in this chain. The property: the video area's
    /// laid-out height tracks window height exactly.
    #[gpui::test]
    fn viewer_video_area_scales_with_window_size(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        admit_fixture();

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

        // This pin is the PLAYER's chain: the video area must track window
        // height with the inspector pane closed — the open pane is a
        // legitimate height share (the fraction contract), not a squash.
        // The fixture asset is surfaced (unindexed), so the toggle
        // dispatches nothing.
        viewer.update(cx, |viewer, cx| viewer.toggle_inspector(cx));

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
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            // The open path's post-state (T3): open_transcript selects the
            // Workspace and yields the inspector's share to the reading
            // surface.
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        // The sub-surface's chrome (purpose line + header row, T3) sits
        // above the list, so the 140px pinned player row and the header
        // rows fill the first frame — scroll the paragraph into view
        // before measuring. The wrap contract is width-containment at the
        // definite pane width, verifiable at any scroll offset.
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(ListOffset {
                item_ix: WORKSPACE_HEADER_ITEMS,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
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
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            // The open path's post-state (T3).
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
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
    /// inspector pane, or tab bar — may exceed the pane's available width
    /// at any pane width, with the video's natural width (640px) larger
    /// than the pane. Historically the pane div lacked `min_w_0`, so the
    /// viewer's min-content width (untruncated header src + seven inline
    /// toolbar buttons ≈ 685px) inflated the pane and the video rendered
    /// clipped at the dock edge, unviewable. This test pins the fix in the
    /// current production embedding (top/bottom split): the pane is a
    /// flex-column child and still carries `min_w_0`. The final iteration
    /// resizes back — the no-thrash probe.
    #[gpui::test]
    fn viewer_content_fits_narrow_pane(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        admit_fixture();

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

    /// THE close-transcript overlap pin (S1, operator report 2026-11-04):
    /// closing the transcript sub-surface must never leave the shared
    /// player painting past the workspace's content area — the artifact
    /// the operator saw as transport buttons superimposed over the Steer
    /// director below. The transcript's 140px player row clips, which
    /// masked the player's intrinsic usable-height floor; closing the
    /// transcript was the first unclipped render of the same shared
    /// widget. The invariant: the player's laid-out bounds stay inside
    /// the content container at every pane height — the floor belongs to
    /// the content-flow embedder (`block_renderer`), and this container
    /// clips as the backstop.
    #[gpui::test]
    fn closing_transcript_leaves_widget_inside_pane(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        admit_fixture();

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

        fn pinned_pane_state() -> TranscriptPaneState {
            TranscriptPaneState {
                transcript_id: "t-close-pin".into(),
                media_path: FIXTURE.into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(vec![PaneWord {
                    word: "pin".into(),
                    start_ms: 0,
                    end_ms: 100,
                }]),
                working_text: "pin".into(),
                paragraph_breaks: None,
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            }
        }

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });

        // A short pane (the host halves the window: the viewer area is
        // ~159px at 320px, far below the player's former 240px floor) and
        // a tall one — the invariant holds at every height, exercised over
        // the open→close transition each time.
        for window_height in [px(320.), px(900.)] {
            cx.simulate_resize(size(px(480.), window_height));
            cx.run_until_parked();

            // Open the sub-surface — the player rides the clipped 140px
            // header row — then close: the transition the operator
            // reported.
            viewer.update(cx, |viewer, cx| {
                viewer.transcript = Some(pinned_pane_state());
                viewer.active_tab = ViewerTab::Workspace;
                cx.notify();
            });
            cx.run_until_parked();
            viewer.update(cx, |viewer, cx| viewer.close_transcript_workspace(cx));
            cx.run_until_parked();

            let content = cx
                .debug_bounds("media-viewer-content")
                .expect("the workspace content container lays out after close");
            let widget = cx
                .debug_bounds("media-widget")
                .expect("the shared player lays out after close");
            assert!(
                widget.top() >= content.top() - px(1.),
                "the player starts inside the content area (window height {window_height:?}): \
                 widget {widget:?} vs content {content:?}"
            );
            assert!(
                widget.bottom() <= content.bottom() + px(1.),
                "the player must not paint past the content area after closing the transcript \
                 (window height {window_height:?}): widget {widget:?} vs content {content:?}"
            );
        }
    }

    /// dcterms:identifier: `render_transcript_workspace` — the pinned
    /// dcterms:identifier: `render_transcript_workspace` — the pinned
    /// interaction band
    /// expect: The player, the surface chips, and the search input pin
    /// ABOVE the scrolling transcript list: a deep follow-along reveal
    /// (the operator's far-forward seek) scrolls the marked block into
    /// view WITHOUT displacing the player — the synced-recording contract
    /// keeps media and text addressing each other, both visible. The
    /// shipped construction rendered the player as list item 0 (operator
    /// report 2026-11-07: the text scroll "pushes or blocks the video from
    /// the view"), so the reveal's own scroll virtualized the primary
    /// media object out of the pane. The band caps at 55% of the
    /// workspace (the player row is its flexible member), so the reading
    /// area keeps the majority at every pane height — the composed
    /// height-budget answer to the one-list design's rejected concern.
    /// [P1] Motivating: the synced recording — watch while reading along.
    /// pre: a word-timed pane state with 30 blocks (far taller than the
    /// pane); the follow reveal armed by a late current word.
    /// post: the media widget's bounds remain laid out INSIDE the band
    /// after the reveal — a virtualized-away row lays out no bounds,
    /// which is the shipped construction's red.
    #[gpui::test]
    fn follow_scroll_keeps_the_pinned_player_in_view(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        admit_fixture();

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

        // 30 blocks over 60 words: far taller than the test pane, so the
        // reveal must scroll.
        let deep_state = TranscriptPaneState {
            transcript_id: "t-follow-pin".into(),
            media_path: FIXTURE.into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..60)
                    .map(|index| PaneWord {
                        word: format!("follow-word-{index}"),
                        start_ms: index * 100,
                        end_ms: index * 100 + 100,
                    })
                    .collect(),
            ),
            working_text: "deep transcript".into(),
            paragraph_breaks: Some((0..30).map(|index| index * 2 + 1).collect()),
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        };

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(deep_state);
            viewer.active_tab = ViewerTab::Workspace;
            // The mark: word 50 — block 25, far past the first viewport.
            viewer.current_word = Some(50);
            viewer.follow_viewport = true;
            cx.notify();
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, cx| {
            viewer.follow_marked_block_into_view();
            cx.notify();
        });
        cx.run_until_parked();

        let band = cx
            .debug_bounds("media-transcript-band")
            .expect("the pinned interaction band lays out under the sub-surface");
        let widget = cx
            .debug_bounds("media-widget")
            .expect("the pinned player stays laid out after the deep reveal");
        assert!(
            widget.top() >= band.top() - px(1.),
            "the pinned player starts inside the band after the reveal: \
             widget {widget:?} vs band {band:?}"
        );
        assert!(
            widget.bottom() <= band.bottom() + px(1.),
            "the pinned player must not leave the band after the reveal: \
             widget {widget:?} vs band {band:?}"
        );
    }

    /// The selection popover (S2) opens for BOTH selection gestures on the
    /// completed range — press-and-release drag and the two-click fallback —
    /// pinned above the list like the label editor (never virtualized
    /// away), carrying the plan's S2 acceptance 1-2. A fresh completion
    /// replaces the tray's range; the completed range owns the pane (no
    /// anchor lingers).
    #[gpui::test]
    fn selection_popover_opens_for_drag_and_two_click_ranges(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-popover-gestures".into(),
                media_path: "/generated/popover.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..30)
                        .map(|index| PaneWord {
                            word: format!("popover-word-{index}"),
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        use gpui::VisualContext as _;
        // Drag completion (S2 core → tray): press on word 2, hover word 5,
        // release — the ordered union range, never inverted.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.word_drag_anchor = Some(2);
            viewer.drag_last_word = Some(5);
            viewer.word_drag_released(window, cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.selection_popover,
                Some((2, 5)),
                "the drag completion opens the tray on the ordered union range"
            );
        });
        assert!(
            cx.debug_bounds("media-transcript-selection-popover")
                .is_some(),
            "the selection popover renders pinned above the list"
        );

        // Two-click completion (the fallback gesture): anchor then last
        // word in select mode — the same range semantics, the same tray.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.selection_mode = true;
            viewer.word_clicked(10, 1_000, window, cx);
            viewer.word_clicked(14, 1_400, window, cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.selection_popover,
                Some((10, 14)),
                "the two-click completion opens the tray on the same union range"
            );
            assert!(
                viewer.selection_anchor.is_none(),
                "an open tray owns the completed range; no anchor lingers"
            );
        });
    }

    /// The tray's keyboard shortcuts (S2): Enter opens the label editor over
    /// the tray's range, Delete cuts through the strike path, Escape
    /// dismisses — the plan's S2 objective's Enter/Delete keys. The tray
    /// holds focus while open, so the keys bind on it and never on the
    /// label input (Delete there is text editing).
    #[gpui::test]
    fn selection_popover_keys_highlight_cut_and_dismiss(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        let blank_modifiers = || gpui::Modifiers {
            control: false,
            alt: false,
            shift: false,
            platform: false,
            function: false,
        };
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-popover-keys".into(),
                media_path: "/generated/popover-keys.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..30)
                        .map(|index| PaneWord {
                            word: format!("keys-word-{index}"),
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        use gpui::VisualContext as _;
        // Enter routes to Highlight: the label editor opens over the tray's
        // range and the tray closes (the editor supersedes it).
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.selection_popover = Some((3, 9));
            viewer.handle_selection_popover_key(
                &Keystroke {
                    modifiers: blank_modifiers(),
                    key: "enter".into(),
                    key_char: None,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                matches!(
                    &viewer.highlight_edit,
                    Some(HighlightEdit {
                        kind: HighlightEditKind::Create {
                            start_word: 3,
                            end_word: 9
                        },
                        ..
                    })
                ),
                "Enter opens the label editor over the tray's range"
            );
            assert!(
                viewer.selection_popover.is_none(),
                "the editor supersedes the tray"
            );
        });

        // Delete routes to the strike path — the admission refusal surfaces
        // (an in-flight edit), proving the cut arm was taken.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.highlight_edit = None;
            viewer.selection_popover = Some((3, 9));
            viewer.edit_request.state = ResourceState::Loading;
            viewer.handle_selection_popover_key(
                &Keystroke {
                    modifiers: blank_modifiers(),
                    key: "delete".into(),
                    key_char: None,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer.selection_popover.is_none(),
                "the cut consumes the tray"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("Another media edit")),
                "Delete reached the strike path and surfaced the admission refusal: {:?}",
                viewer.status
            );
        });

        // Escape dismisses without acting.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.edit_request.state = ResourceState::Idle;
            viewer.selection_popover = Some((3, 9));
            viewer.handle_selection_popover_key(
                &Keystroke {
                    modifiers: blank_modifiers(),
                    key: "escape".into(),
                    key_char: None,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer.selection_popover.is_none(),
                "Escape dismisses the tray"
            );
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("dismissed")),
                "the dismissal surfaces: {:?}",
                viewer.status
            );
        });
    }

    /// dcterms:identifier: `MediaViewer::word_drag_released`
    /// expect: A completed drag on a re_anchored view opens the CORRECTION
    /// tray (S5, D5 v2) — Enter opens the correction editor, Delete stays
    /// inert (cut needs an index-identical view), the index-identical-only
    /// actions refuse with the honest reason even when reached directly,
    /// and Escape dismisses. [P1] Motivating: the Dunning recording is
    /// re_anchored — the in-panel correction path needs a selection
    /// surface there, and working indices must never anchor source-word
    /// tools (the 2026-10-25 overshoot trap).
    #[gpui::test]
    fn reanchored_selection_tray_corrects_only(cx: &mut gpui::TestAppContext) {
        init_layout_test_globals(cx);
        // No dispatch in this pin's flow: the tray opens, Enter opens the
        // editor, Delete is inert, and the index-identical-only actions
        // refuse at the view gate — before the edit admission and its
        // invoker read — so it needs neither the lock nor a scripted
        // invoker.
        let blank_modifiers = || gpui::Modifiers {
            control: false,
            alt: false,
            shift: false,
            platform: false,
            function: false,
        };
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-reanchored-tray".into(),
                media_path: "/m.wav".into(),
                gallery_asset_id: None,
                alignment: "re_anchored".into(),
                alignment_note: None,
                words: Some(
                    ["alpha", "bravo", "charlie"]
                        .into_iter()
                        .enumerate()
                        .map(|(index, word)| PaneWord {
                            word: word.into(),
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        use gpui::VisualContext as _;
        // A completed drag opens the correction tray on a re_anchored view
        // (the S2 gate refused selection here).
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.word_drag_anchor = Some(0);
            viewer.drag_last_word = Some(2);
            viewer.word_drag_released(window, cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(viewer.selection_popover, Some((0, 2)), "the tray opens");
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("Suggest correction")),
                "the correction tray's hint: {:?}",
                viewer.status
            );
        });

        // Enter opens the correction editor over the tray's range.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.handle_selection_popover_key(
                &Keystroke {
                    modifiers: blank_modifiers(),
                    key: "enter".into(),
                    key_char: None,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                matches!(
                    &viewer.highlight_edit,
                    Some(HighlightEdit {
                        kind: HighlightEditKind::Correction {
                            start_word: 0,
                            end_word: 2
                        },
                        ..
                    })
                ),
                "Enter opens the correction editor: {:?}",
                viewer.highlight_edit
            );
            assert!(viewer.selection_popover.is_none());
            viewer.highlight_edit = None;
            viewer.selection_popover = Some((0, 2));
        });

        // Delete stays inert on the correction tray — cut needs an
        // index-identical view.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.handle_selection_popover_key(
                &Keystroke {
                    modifiers: blank_modifiers(),
                    key: "delete".into(),
                    key_char: None,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.selection_popover,
                Some((0, 2)),
                "Delete is inert on the correction tray"
            );
            assert_eq!(viewer.edit_request.state, ResourceState::Idle);
        });

        // The index-identical-only actions refuse with the honest reason
        // even when reached directly — a stale tray or a mis-routed key
        // must never anchor working indices to source-word tools.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.popover_highlight(window, cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("index-identical")),
                "the highlight refusal names the view gate: {:?}",
                viewer.status
            );
            assert!(viewer.highlight_edit.is_none());
            viewer.selection_popover = Some((0, 2));
        });
        cx.update_window_entity(&viewer, |viewer, _window, cx| {
            viewer.popover_cut(cx);
            viewer.selection_popover = Some((0, 2));
            viewer.popover_toggle_reel_include(cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("index-identical")),
                "the cut/include refusals name the view gate: {:?}",
                viewer.status
            );
            assert_eq!(viewer.edit_request.state, ResourceState::Idle);
        });

        // Escape dismisses.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            viewer.handle_selection_popover_key(
                &Keystroke {
                    modifiers: blank_modifiers(),
                    key: "escape".into(),
                    key_char: None,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(viewer.selection_popover.is_none(), "Escape dismisses");
        });
    }

    /// A words pane state for the search tests: `total` words in 5-word
    /// blocks, "dunning" at each index in `matches_at`.
    fn search_pane_state(total: usize, dunning_at: &[usize]) -> TranscriptPaneState {
        TranscriptPaneState {
            transcript_id: "t-search".into(),
            media_path: "/generated/search.wav".into(),
            gallery_asset_id: None,
            alignment: "original".into(),
            alignment_note: None,
            words: Some(
                (0..total)
                    .map(|index| PaneWord {
                        word: if dunning_at.contains(&index) {
                            "dunning".into()
                        } else {
                            format!("w{index}")
                        },
                        start_ms: index as u64 * 100,
                        end_ms: index as u64 * 100 + 90,
                    })
                    .collect(),
            ),
            working_text: String::new(),
            paragraph_breaks: Some((1..(total / 5)).map(|block| block * 5 - 1).collect()),
            speaker_spans: None,
            review: None,
            layers: Vec::new(),
            highlights: Vec::new(),
            strikes: Vec::new(),
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        }
    }

    /// The blank-modifier keystroke helper for the search tests.
    fn search_key(key: &str, shift: bool) -> Keystroke {
        Keystroke {
            modifiers: gpui::Modifiers {
                control: false,
                alt: false,
                shift,
                platform: false,
                function: false,
            },
            key: key.into(),
            key_char: None,
        }
    }

    /// Whether the block at `block_ix` is fully inside the list's viewport
    /// — the same visibility read `follow_marked_block_into_view` performs.
    fn search_block_in_view(
        viewer: &gpui::Entity<MediaViewer>,
        cx: &mut gpui::VisualTestContext,
        block_ix: usize,
    ) -> bool {
        viewer.update(cx, |viewer, _| {
            let item_ix = WORKSPACE_HEADER_ITEMS + block_ix;
            viewer
                .transcript_list
                .bounds_for_item(item_ix)
                .is_some_and(|item| {
                    let viewport = viewer.transcript_list.viewport_bounds();
                    item.top() >= viewport.top() && item.bottom() <= viewport.bottom()
                })
        })
    }

    /// The in-transcript search (S3): typing into the header input finds
    /// every match of the query over the loaded transcript, the stabilized
    /// query lands the first match in view, Enter cycles forward and
    /// Shift-Enter back (wrapping), each cycled match's block scrolls into
    /// view, and a query with no results shows the honest no-matches state
    /// — the plan's S3 acceptance 1 and 2.
    #[gpui::test]
    fn transcript_search_finds_cycles_and_lands_the_current_match(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        // 120 words in 5-word blocks, "dunning" at 25, 85, 115 — three
        // matches, the later two far below the first fold.
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(search_pane_state(120, &[25, 85, 115]));
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        // A taller window than the other layout tests: the search row sits
        // BELOW the 140px player row, so the first fold must cover the whole
        // header for the input to paint — and its focus path to register
        // (focus events dispatch from the painted frame's focus path).
        cx.simulate_resize(size(px(480.), px(900.)));
        cx.run_until_parked();

        // The search input renders in the header from the first paint.
        assert!(
            cx.debug_bounds("media-transcript-search-input").is_some(),
            "the search input renders in the header"
        );

        // Type the query; the debounced rescan runs once the burst settles.
        // The paint before the rescan carries the typed query and the
        // honest interim status ("No matches" — the rescan has not run).
        viewer.update(cx, |viewer, cx| {
            for character in "dunning".chars() {
                viewer.handle_transcript_search_key(&search_key(&character.to_string(), false), cx);
            }
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-transcript-search-status").is_some(),
            "the match count renders beside the input"
        );
        cx.executor().advance_clock(TRANSCRIPT_SEARCH_DEBOUNCE);
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.transcript_search.matches,
                vec![(25, 25), (85, 85), (115, 115)],
                "typing finds all the matches"
            );
            assert_eq!(viewer.transcript_search.current, 0);
            assert_eq!(
                transcript_search_status(&viewer.transcript_search, true).as_deref(),
                Some("1/3")
            );
        });
        // The rescan's auto-land scrolled the first match's block into view
        // — the header rows (input included) are above the viewport now, so
        // the input's rendering is pinned by the PRE-rescan bounds asserts
        // above; here the reveal is the observable.
        assert!(
            search_block_in_view(&viewer, cx, 5),
            "the stabilized query lands the first match (word 25, block 5) in view"
        );

        // Enter cycles forward (wrapping) and lands each cycled match in
        // view; Shift-Enter cycles back.
        for (expected_current, block) in [(1usize, 17usize), (2, 23), (0, 5)] {
            viewer.update(cx, |viewer, cx| {
                viewer.handle_transcript_search_key(&search_key("enter", false), cx);
            });
            cx.run_until_parked();
            viewer.update(cx, |viewer, _| {
                assert_eq!(
                    viewer.transcript_search.current, expected_current,
                    "Enter cycles and wraps"
                );
            });
            assert!(
                search_block_in_view(&viewer, cx, block),
                "the cycled match's block scrolls into view"
            );
        }
        viewer.update(cx, |viewer, cx| {
            viewer.handle_transcript_search_key(&search_key("enter", true), cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.transcript_search.current, 2,
                "Shift-Enter cycles back"
            );
        });

        // A query with no results shows the honest no-matches state.
        viewer.update(cx, |viewer, cx| {
            viewer.handle_transcript_search_key(&search_key("escape", false), cx);
            for character in "absent".chars() {
                viewer.handle_transcript_search_key(&search_key(&character.to_string(), false), cx);
            }
        });
        cx.executor().advance_clock(TRANSCRIPT_SEARCH_DEBOUNCE);
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(viewer.transcript_search.matches.is_empty());
            assert_eq!(
                transcript_search_status(&viewer.transcript_search, true).as_deref(),
                Some("No matches"),
                "zero matches surface, never silence"
            );
        });
    }

    /// The search rescan is debounced (S3): a burst of keystrokes rescans
    /// once, 150 ms after the LAST edit — never per keystroke, never before
    /// the burst settles.
    #[gpui::test]
    fn transcript_search_rescan_debounces_the_burst(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(search_pane_state(30, &[5]));
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        viewer.update(cx, |viewer, cx| {
            for character in "dun".chars() {
                viewer.handle_transcript_search_key(&search_key(&character.to_string(), false), cx);
            }
        });
        // 149 ms after the last keystroke: the rescan has not run.
        cx.executor()
            .advance_clock(TRANSCRIPT_SEARCH_DEBOUNCE - Duration::from_millis(1));
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                viewer.transcript_search.matches.is_empty(),
                "the rescan waits out the burst"
            );
        });
        // One millisecond later it fires, once, on the final query.
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.transcript_search.matches,
                vec![(5, 5)],
                "the settled burst rescans once, on the final query"
            );
            assert!(
                viewer.transcript_search_task.is_none(),
                "the rescan task completes"
            );
        });
    }

    /// Focusing the search input suspends viewport tracking (the plan's S3
    /// design: typing and the auto-landed matches must not fight the
    /// follow-along); blur restores tracking only when the search took it —
    /// a manually suspended follow stays suspended.
    #[gpui::test]
    fn transcript_search_focus_suspends_follow_and_blur_restores(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(search_pane_state(30, &[5]));
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        // The search row sits below the 140px player row — the fold must
        // cover the header for the input to paint and register its focus
        // path (focus events dispatch from the painted frame's focus path,
        // so an off-screen input never gains focus events).
        cx.simulate_resize(size(px(480.), px(900.)));
        cx.run_until_parked();

        use gpui::VisualContext as _;
        // The test platform's windows start INACTIVE: the focus-change event
        // carries empty focus paths for an inactive window
        // (`current_window_active` gates them), so the listeners would never
        // fire. Activate first — the production window is active the moment
        // the operator clicks anything.
        cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
        cx.run_until_parked();
        // The initial paint registered the focus listeners (the workspace
        // render path); the drive only focuses. The dispatch needs an
        // active window (test windows start INACTIVE — the focus event's
        // paths stay empty until activate_window) and a following draw, so
        // the drive also notifies.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            let focus = viewer
                .transcript_search_focus
                .clone()
                .expect("rendering the header created the focus handle");
            window.focus(&focus, cx);
            cx.notify();
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(!viewer.follow_viewport, "focus suspends tracking");
            assert!(
                viewer.transcript_search_suspended_follow,
                "the search holds the follow custody it took"
            );
        });

        // Blur (focus the label input): custody restores the tracking.
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            let focus = viewer
                .label_input_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            window.focus(&focus, cx);
            cx.notify();
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(viewer.follow_viewport, "blur restores the follow it took");
            assert!(!viewer.transcript_search_suspended_follow);
        });

        // A manually suspended follow is not the search's to restore.
        viewer.update(cx, |viewer, _| {
            viewer.follow_viewport = false;
        });
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            let focus = viewer
                .transcript_search_focus
                .clone()
                .expect("the search focus handle exists");
            window.focus(&focus, cx);
            cx.notify();
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                !viewer.transcript_search_suspended_follow,
                "the search never takes custody of an already-suspended follow"
            );
        });
        cx.update_window_entity(&viewer, |viewer, window, cx| {
            let focus = viewer
                .label_input_focus
                .clone()
                .expect("the label focus handle exists");
            window.focus(&focus, cx);
            cx.notify();
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(
                !viewer.follow_viewport,
                "a manually suspended follow stays suspended after blur"
            );
        });
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
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            // The open path's post-state (T3).
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            viewer.media_widget = Some(widget);
            viewer.media_widget_body = Some(transcript_media_body(media_path));
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        // 480x600, not the old 480x320: the composed two-pane fix (2026-11-07)
        // pins the interaction band (player + chips + search) above the list,
        // capped at 55% of the workspace so the reading area keeps the
        // majority — at the old 320px-window geometry the band leaves the
        // list a sub-padding sliver and the reachability contract is
        // exercised against the band instead (the pinned close row and
        // select chip are now reachable WITHOUT scrolling — strictly
        // stronger than the old scroll-to-reach). The harness halves the
        // window, so the viewer area here is ~300px.
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        // The close affordance rides in the sub-surface's header row above
        // the list — always visible, never virtualized away, reachable
        // without scrolling even on the tightest pane (the harness halves
        // the window, so the viewer area here is ~300px).
        let close = cx
            .debug_bounds("media-transcript-close")
            .expect("Close remains laid out during media failure");
        assert!(
            close.bottom() <= px(320.),
            "Close remains reachable during media failure: {close:?}"
        );
        // The select-mode chip is PINNED in the interaction band (the
        // composed two-pane fix): it is never a list item, so it must
        // render WITHOUT any scrolling — the strict form of the old
        // scroll-to-reach contract.
        let select_mode = cx
            .debug_bounds("media-transcript-select-mode")
            .expect("the pinned select chip renders without scrolling during media failure");
        assert!(
            select_mode.bottom() <= px(320.),
            "the pinned select chip remains reachable during media failure: {select_mode:?}"
        );
        // The paragraph (block 0) remains a list item under the band —
        // scrolling renders it, no dead end.
        let selector = "media-transcript-paragraph";
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(ListOffset {
                item_ix: WORKSPACE_HEADER_ITEMS,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} renders when scrolled to during media failure"));
        assert!(
            bounds.bottom() <= px(320.),
            "{selector} remains reachable during media failure: {bounds:?}"
        );
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
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            // The open path's post-state (T3).
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
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
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            // The open path's post-state (T3).
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
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

    /// The per-recording transcription status (R2/R5): a running transcribe
    /// job whose source is the recording shows Transcribing; a stored
    /// transcript linked to the recording shows Transcribed; otherwise
    /// Not transcribed. [P1] Motivating: the recordings browser is the
    /// recordings CRUD the reference model's capability 3 names.
    #[test]
    fn recordings_rows_carry_transcription_status() {
        let mut viewer = MediaViewer::new();
        viewer.assets = vec![
            MediaAsset {
                body: r#"{"kind":"video","src":"/talk.mp4"}"#.into(),
                src: "/talk.mp4".into(),
                kind: "video".into(),
                tool: "gallery".into(),
                gallery_index: Some(0),
                gallery_asset_id: Some("asset-talk".into()),
            },
            MediaAsset {
                body: r#"{"kind":"audio","src":"/note.wav"}"#.into(),
                src: "/note.wav".into(),
                kind: "audio".into(),
                tool: "gallery".into(),
                gallery_index: Some(1),
                gallery_asset_id: Some("asset-note".into()),
            },
        ];
        viewer.transcript_listing = Some(vec![TranscriptSummaryRow {
            id: "t-talk".into(),
            media_path: "/talk.mp4".into(),
            words_count: 42,
            duration_secs: 30,
            has_word_timings: true,
            alignment: "original".into(),
            gallery_asset_id: Some("asset-talk".into()),
        }]);
        viewer.live_jobs = vec![JobRecord {
            id: "job-transcribe".into(),
            op: "transcribe".into(),
            status: "running".into(),
            created_at: "2026-10-28T00:00:00Z".into(),
            completed_at: None,
            result: None,
            error: None,
            source: Some("/note.wav".into()),
        }];
        assert_eq!(
            viewer.recording_transcription_status(&viewer.assets[0]),
            RecordingTranscriptionStatus::Transcribed,
            "a linked transcript shows Transcribed"
        );
        assert_eq!(
            viewer.recording_transcription_status(&viewer.assets[1]),
            RecordingTranscriptionStatus::Transcribing,
            "a running transcribe job against the recording shows Transcribing"
        );
        // A gallery-backed recording with no transcript and no job shows
        // Not transcribed — the operator can take it to its transcript.
        viewer.assets.push(MediaAsset {
            body: r#"{"kind":"video","src":"/later.mp4"}"#.into(),
            src: "/later.mp4".into(),
            kind: "video".into(),
            tool: "gallery".into(),
            gallery_index: Some(2),
            gallery_asset_id: Some("asset-later".into()),
        });
        let later = viewer.assets.last().unwrap();
        assert_eq!(
            viewer.recording_transcription_status(later),
            RecordingTranscriptionStatus::NotTranscribed,
            "a recording with neither transcript nor job shows Not transcribed"
        );
    }

    /// The Transcribe affordance (R5, T3) renders on a not-transcribed
    /// recording's Library row and dispatches the async transcribe job
    /// against the recording's source; the unwired-invoker degradation
    /// surfaces a status (tests run without a shared tool invoker).
    #[gpui::test]
    fn transcribe_affordance_renders_and_degrades_honestly(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.assets = vec![MediaAsset {
                body: r#"{"kind":"video","src":"/later.mp4"}"#.into(),
                src: "/later.mp4".into(),
                kind: "video".into(),
                tool: "gallery".into(),
                gallery_index: Some(0),
                gallery_asset_id: Some("asset-later".into()),
            }];
            viewer.gallery_total = Some(1);
            viewer.transcript_listing = Some(vec![]);
            viewer.active_tab = ViewerTab::Library;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(320.)));
        cx.run_until_parked();

        // The Transcribe affordance renders on the not-transcribed
        // recording's Library row; the operator can take the recording to
        // its transcript without a chat turn.
        assert!(
            cx.debug_bounds("media-recording-transcribe").is_some(),
            "the Transcribe affordance must render on the recording's Library row"
        );
        assert!(
            cx.debug_bounds("media-transcript-delete").is_none(),
            "no transcript rows render for a not-transcribed recording"
        );

        // The dispatch surfaces a status even on the unwired degradation
        // path — never silence (D5).
        viewer.update(cx, |viewer, cx| {
            viewer.transcribe_recording("asset-later".into(), "/later.mp4".into(), cx);
        });
        cx.run_until_parked();
        viewer.update(cx, |viewer, _cx| {
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| status.contains("cannot transcribe")),
                "the unwired-invoker degradation must surface a status, got {:?}",
                viewer.status
            );
        });
    }

    /// The Library's transcript rows' two-click delete (T3): the Delete
    /// affordance renders per row — nested under its recording, or in the
    /// unlinked section — arming swaps it for Confirm delete, and the
    /// dispatch closes the edit lifecycle through the unwired-invoker
    /// degradation path (tests run without a shared tool invoker).
    #[gpui::test]
    fn library_transcript_rows_delete_confirms_and_closes_the_edit(cx: &mut TestAppContext) {
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
                gallery_asset_id: None,
            }]);
            viewer.gallery_total = Some(0);
            viewer.active_tab = ViewerTab::Library;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(320.)));
        cx.run_until_parked();

        // The Delete affordance is laid out on the row, alongside the open
        // affordance — the operator can always remove a stored transcript;
        // the row renders in the unlinked section below the (empty) asset
        // rows, reachable, never hidden.
        assert!(
            cx.debug_bounds("media-transcript-delete").is_some(),
            "the Delete affordance must render on the Library's transcript row"
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

    /// The activity drawer (T2) renders on demand over any mode: the
    /// observed live rows first with Cancel for queued/running jobs, the
    /// recent history after them (terminal — no Cancel), and the
    /// process-local history disclosure in the footer. The toolbar
    /// toggle opens and closes it.
    #[gpui::test]
    fn activity_drawer_renders_rows_cancel_and_disclosure(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.active_tab = ViewerTab::Library;
            viewer.gallery_total = Some(0);
            viewer.jobs_total = Some(2);
            viewer.jobs_limit = hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT;
            viewer.jobs_has_more = false;
            let mut live =
                scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
            live.source = Some("/talk.wav".into());
            viewer.live_jobs = vec![live];
            viewer.jobs = vec![scheduled_job(
                "job-done",
                "video_clip",
                "completed",
                "2026-09-06T00:00:00Z",
            )];
            viewer.jobs_history_scope =
                Some(hkask_mcp_media::tools::jobs::JOB_HISTORY_SCOPE.into());
            viewer.jobs_restart_behavior =
                Some(hkask_mcp_media::tools::jobs::JOB_RESTART_BEHAVIOR.into());
            viewer.activity_open = true;
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(320.)));
        cx.run_until_parked();

        assert!(
            cx.debug_bounds("media-activity-toggle").is_some(),
            "the toolbar's Activity toggle renders"
        );
        assert!(
            cx.debug_bounds("media-activity-drawer").is_some(),
            "the drawer renders open over any mode"
        );
        assert!(
            cx.debug_bounds("media-activity-row-job-live").is_some(),
            "the observed live row renders"
        );
        assert!(
            cx.debug_bounds("media-activity-cancel-job-live").is_some(),
            "a queued/running row carries Cancel"
        );
        assert!(
            cx.debug_bounds("media-activity-row-job-done").is_some(),
            "the recent-history row renders"
        );
        assert!(
            cx.debug_bounds("media-activity-cancel-job-done").is_none(),
            "a terminal history row carries no Cancel"
        );
        assert!(
            cx.debug_bounds("media-activity-disclosure").is_some(),
            "the footer carries the process-local history disclosure"
        );

        // The toggle closes the drawer — ambient state, never a
        // destination.
        viewer.update(cx, |viewer, cx| viewer.toggle_activity(cx));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-activity-drawer").is_none(),
            "the toggle closes the drawer"
        );
    }

    /// The ambient activity badge (T2): a live research job whose primary
    /// source is the asset badges its Library row — transient (it clears
    /// when the job is no longer live), never error-styled.
    #[gpui::test]
    fn library_rows_badge_live_jobs(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.active_tab = ViewerTab::Library;
            viewer.gallery_total = Some(2);
            viewer.assets = vec![
                MediaAsset {
                    body: r#"{"kind":"audio","src":"/talk.wav"}"#.into(),
                    src: "/talk.wav".into(),
                    kind: "audio".into(),
                    tool: "gallery".into(),
                    gallery_index: Some(0),
                    gallery_asset_id: Some("asset-talk".into()),
                },
                MediaAsset {
                    body: r#"{"kind":"video","src":"/other.mp4"}"#.into(),
                    src: "/other.mp4".into(),
                    kind: "video".into(),
                    tool: "gallery".into(),
                    gallery_index: Some(1),
                    gallery_asset_id: Some("asset-other".into()),
                },
            ];
            let mut live =
                scheduled_job("job-live", "transcribe", "running", "2026-09-05T00:00:00Z");
            live.source = Some("/talk.wav".into());
            viewer.live_jobs = vec![live];
            cx.notify();
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(320.)));
        cx.run_until_parked();

        assert!(
            cx.debug_bounds("media-library-activity-0").is_some(),
            "the affected asset's Library row carries the badge"
        );
        assert!(
            cx.debug_bounds("media-library-activity-1").is_none(),
            "an unaffected asset carries no badge"
        );
        // The badge is transient: it clears when the job is no longer
        // live (HIG Progress indicators — present only while ongoing).
        viewer.update(cx, |viewer, cx| {
            viewer.live_jobs.clear();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-library-activity-0").is_none(),
            "the badge clears when the job is no longer live"
        );
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
        // The inspector pane (T1): the pane and all seven of its
        // pane-internal tabs render inside the same dock — the tab row
        // wraps instead of inflating the pane's min-content.
        let inspector_bounds = cx
            .debug_bounds("media-inspector")
            .expect("inspector laid out with debug bounds");
        assert!(
            inspector_bounds.right() <= dock_width + px(1.),
            "the inspector must fit inside the {dock_width:?} dock: right {:?}",
            inspector_bounds.right()
        );
        for tab in InspectorTab::ALL {
            cx.debug_bounds(tab.element_id())
                .unwrap_or_else(|| panic!("inspector tab {:?} laid out", tab));
        }
    }

    /// The events surface (S5) renders inside the same narrow-dock
    /// workspace: the surface toggle chips, the time-ordered event rows,
    /// and the authoring section all lay out within the pane — the toggle
    /// is a reading-surface switch, not a new workspace.
    #[gpui::test]
    fn events_surface_renders_rows_and_authoring_section(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| {
            let mut viewer = MediaViewer::new();
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-events".into(),
                media_path: "/generated/one.mp4".into(),
                gallery_asset_id: None,
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
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.visual_events = Some(VisualEventsState {
                status: "completed".into(),
                events: vec![
                    PaneEventRow {
                        start_sec: 0.0,
                        end_sec: Some(4.2),
                        kind: "shot".into(),
                        layer_id: "layer_shot".into(),
                        item_id: "item_s0".into(),
                        searchable_text: "shot 0".into(),
                        model: "ffmpeg:scdet".into(),
                        box_norm: None,
                        object_label: None,
                        evidence: vec!["seen".into()],
                    },
                    PaneEventRow {
                        start_sec: 3.5,
                        end_sec: Some(9.0),
                        kind: "object".into(),
                        layer_id: "layer_obj".into(),
                        item_id: "item_a".into(),
                        searchable_text: "object: person".into(),
                        model: "qwen3-vl".into(),
                        box_norm: Some([0.1, 0.2, 0.3, 0.4]),
                        object_label: Some("person".into()),
                        evidence: vec!["seen".into()],
                    },
                ],
                events_total: 2,
                kinds_present: vec!["shot".into(), "object".into()],
                note: None,
            });
            viewer.transcript_surface = TranscriptSurface::Events;
            // The open path's post-state (T3).
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            viewer
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        // The list paints items only within its viewport (virtualized):
        // the toggle chips are what the first frame guarantees in view
        // (T3: the sub-surface's purpose line and header row sit above the
        // list, so the pinned 140px player row and the chips fill the
        // first frame); the event rows render when scrolled to — the
        // reachability contract under virtualization.
        let bounds = cx
            .debug_bounds("media-transcript-surface-events")
            .expect("the surface toggle chip must lay out");
        assert!(
            bounds.right() <= px(480.),
            "the surface toggle chip fits the dock: {bounds:?}"
        );
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: WORKSPACE_HEADER_ITEMS,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        let bounds = cx
            .debug_bounds("media-event-row-0")
            .expect("the first event row must lay out when scrolled to");
        assert!(
            bounds.right() <= px(480.),
            "the first event row fits the dock: {bounds:?}"
        );

        // Scrolling the virtualized list to the tail brings the authoring
        // section into view — the section renders as a real laid-out row,
        // not a phantom.
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: viewer.transcript_list_count.saturating_sub(1),
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        let bounds = cx
            .debug_bounds("media-events-authoring-section")
            .expect("the authoring section must lay out");
        assert!(
            bounds.right() <= px(480.),
            "the authoring section fits the dock: {bounds:?}"
        );
    }

    /// The events-surface layout fixture: the two-row event log (a shot
    /// row and a box-carrying object row) with a caller-set total — a
    /// total above the row count makes the state a truncated page.
    fn events_state(events_total: usize) -> VisualEventsState {
        VisualEventsState {
            status: "completed".into(),
            events: vec![
                PaneEventRow {
                    start_sec: 0.0,
                    end_sec: Some(4.2),
                    kind: "shot".into(),
                    layer_id: "layer_shot".into(),
                    item_id: "item_s0".into(),
                    searchable_text: "shot 0".into(),
                    model: "ffmpeg:scdet".into(),
                    box_norm: None,
                    object_label: None,
                    evidence: vec!["seen".into()],
                },
                PaneEventRow {
                    start_sec: 3.5,
                    end_sec: Some(9.0),
                    kind: "object".into(),
                    layer_id: "layer_obj".into(),
                    item_id: "item_a".into(),
                    searchable_text: "object: person".into(),
                    model: "qwen3-vl".into(),
                    box_norm: Some([0.1, 0.2, 0.3, 0.4]),
                    object_label: Some("person".into()),
                    evidence: vec!["seen".into()],
                },
            ],
            events_total,
            kinds_present: vec!["shot".into(), "object".into()],
            note: None,
        }
    }

    /// An events-surface workspace over `/generated/one.mp4` with the
    /// given event log — the truncation tests' shared viewer.
    fn events_surface_viewer(state: VisualEventsState) -> MediaViewer {
        let mut viewer = MediaViewer::new();
        viewer.transcript = Some(TranscriptPaneState {
            transcript_id: "t-events".into(),
            media_path: "/generated/one.mp4".into(),
            gallery_asset_id: None,
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
            correction_edits: Vec::new(),
            source_words: Vec::new(),
        });
        viewer.visual_events = Some(state);
        viewer.transcript_surface = TranscriptSurface::Events;
        // The open path's post-state (T3).
        viewer.active_tab = ViewerTab::Workspace;
        viewer.inspector_open = false;
        viewer
    }

    /// A page shorter than the recording's total events surfaces the
    /// truncation note at the bottom of the event list: the list carries
    /// one extra tail item, the note lays out under the last event row,
    /// and the authoring section still lays out below it.
    #[gpui::test]
    fn events_surface_truncation_note_renders_below_the_event_rows(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| events_surface_viewer(events_state(1200)));

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        // The truncated page carries one extra tail item: the fixed
        // header, the two event rows, the truncation note, then the
        // authoring section.
        viewer.read_with(cx, |viewer, _| {
            assert_eq!(
                viewer.transcript_list_count,
                WORKSPACE_HEADER_ITEMS + 2 + WORKSPACE_EVENTS_TAIL_ITEMS + 1
            );
        });

        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: WORKSPACE_HEADER_ITEMS + 2,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        let bounds = cx
            .debug_bounds("media-events-truncation-note")
            .expect("the truncation note must lay out");
        assert!(
            bounds.right() <= px(480.),
            "the truncation note fits the dock: {bounds:?}"
        );

        // The authoring section still lays out below the note.
        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: viewer.transcript_list_count.saturating_sub(1),
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        let bounds = cx
            .debug_bounds("media-events-authoring-section")
            .expect("the authoring section must lay out");
        assert!(
            bounds.right() <= px(480.),
            "the authoring section fits the dock: {bounds:?}"
        );
    }

    /// A full page is not truncated: the tail stays exactly the authoring
    /// section — no phantom truncation note — so the note's presence
    /// itself is the truncation signal.
    #[gpui::test]
    fn events_surface_full_page_renders_no_truncation_note(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| events_surface_viewer(events_state(2)));

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        viewer.read_with(cx, |viewer, _| {
            assert_eq!(
                viewer.transcript_list_count,
                WORKSPACE_HEADER_ITEMS + 2 + WORKSPACE_EVENTS_TAIL_ITEMS
            );
        });

        viewer.update(cx, |viewer, cx| {
            viewer.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: WORKSPACE_HEADER_ITEMS + 2,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-events-truncation-note").is_none(),
            "a full page renders no truncation note"
        );
        cx.debug_bounds("media-events-authoring-section")
            .expect("the authoring section still lays out");
    }

    /// A real pointer two-click on the rendered words reaches the
    /// selection path — the click hitboxes map window coordinates to the
    /// word ranges, and the completed range opens the selection popover.
    /// [P1] Motivating: the interaction must work through the painted
    /// surface, not only through the handler methods the unit pins call.
    #[gpui::test]
    fn pointer_two_click_selection_opens_the_tray(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-diag-click".into(),
                media_path: "/generated/diag.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();

        // Test windows start inactive — activate before the pointer path
        // can dispatch (the production window is active on first click).
        cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
        cx.run_until_parked();
        let paragraph = cx
            .debug_bounds("media-transcript-paragraph")
            .expect("block 0 laid out");
        // Select mode on, then two clicks: anchor word 0, complete on the
        // word under the second click.
        cx.update_window_entity(&viewer, |viewer, _, cx| {
            viewer.selection_mode = true;
            cx.notify();
        });
        cx.run_until_parked();
        let first = gpui::Point {
            x: paragraph.left() + gpui::px(4.),
            y: paragraph.center().y,
        };
        let second = gpui::Point {
            x: paragraph.left() + gpui::px(120.),
            y: paragraph.center().y,
        };
        cx.simulate_mouse_down(first, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_up(first, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(second, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_up(second, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();

        viewer.read_with(cx, |viewer, _| {
            assert!(
                viewer.selection_anchor.is_none(),
                "the completed range clears the anchor"
            );
            assert_eq!(
                viewer.selection_popover,
                Some((0, 1)),
                "the two pointer clicks complete the ordered range"
            );
        });
        assert!(
            cx.debug_bounds("media-transcript-selection-popover")
                .is_some(),
            "the completed range opens the selection popover"
        );
    }

    /// dcterms:identifier: `MediaViewer::context_menu_range`
    /// expect: an open selection popover owns its range; otherwise a
    /// pending two-click anchor completes to the hovered word; a bare
    /// right click over a word selects that word alone; a right click
    /// outside the words (no hovered word, no anchor) has no range.
    #[test]
    fn context_menu_range_prefers_the_open_tray() {
        let mut viewer = MediaViewer::new();
        viewer.selection_popover = Some((3, 9));
        viewer.selection_anchor = Some(1);
        viewer.context_menu_fallback = Some(7);
        assert_eq!(
            viewer.context_menu_range(),
            Some((3, 9)),
            "the open tray's range wins"
        );
        viewer.selection_popover = None;
        assert_eq!(
            viewer.context_menu_range(),
            Some((1, 7)),
            "a pending anchor completes to the right-click target"
        );
        viewer.selection_anchor = None;
        assert_eq!(
            viewer.context_menu_range(),
            Some((7, 7)),
            "a bare right click selects the right-click target word"
        );
        viewer.context_menu_fallback = None;
        assert_eq!(
            viewer.context_menu_range(),
            None,
            "a right click outside the words has no range"
        );
    }

    /// A right click over the rendered words opens the context menu with
    /// the Highlight and Strike out entries — the pointer alternative to
    /// the select-then-tray gesture. [P1] Motivating: the operator's
    /// "select text, then right-click to choose highlight or strike" is
    /// reachable through the painted surface.
    #[gpui::test]
    fn right_click_opens_context_menu_with_highlight_and_strike(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-context-menu".into(),
                media_path: "/generated/context-menu.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();
        cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
        cx.run_until_parked();
        let paragraph = cx
            .debug_bounds("media-transcript-paragraph")
            .expect("block 0 laid out");
        let point = gpui::Point {
            x: paragraph.left() + gpui::px(4.),
            y: paragraph.center().y,
        };

        // The pointer rests on a word first (hover tracks it), then the
        // right click opens the menu for that word.
        cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-transcript-context-menu").is_some(),
            "the context menu renders"
        );
        assert!(
            cx.debug_bounds("media-transcript-context-menu-highlight")
                .is_some(),
            "the context menu offers Highlight"
        );
        assert!(
            cx.debug_bounds("media-transcript-context-menu-strike")
                .is_some(),
            "the context menu offers Strike out"
        );

        // Choosing Highlight opens the label editor over the range.
        let highlight = cx
            .debug_bounds("media-transcript-context-menu-highlight")
            .expect("Highlight entry laid out");
        cx.simulate_click(highlight.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        viewer.read_with(cx, |viewer, _| {
            assert!(
                matches!(
                    &viewer.highlight_edit,
                    Some(HighlightEdit {
                        kind: HighlightEditKind::Create {
                            start_word: 0,
                            end_word: 0
                        },
                        ..
                    })
                ),
                "Highlight opens the label editor over the right-clicked word"
            );
        });
    }

    /// Escape dismisses the open context menu without acting — the menu's
    /// dismissal key, bound on the menu while it holds focus.
    #[gpui::test]
    fn context_menu_escape_dismisses(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-context-escape".into(),
                media_path: "/generated/context-escape.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();
        cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
        cx.run_until_parked();
        let paragraph = cx
            .debug_bounds("media-transcript-paragraph")
            .expect("block 0 laid out");
        let point = gpui::Point {
            x: paragraph.left() + gpui::px(4.),
            y: paragraph.center().y,
        };
        cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-transcript-context-menu").is_some(),
            "the context menu opens"
        );
        // Escape dismisses the open menu (the menu holds focus).
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        viewer.read_with(cx, |viewer, _| {
            assert!(
                viewer.context_menu.is_none(),
                "Escape dismisses the context menu"
            );
        });
    }

    /// A completed selection followed by a right click opens the context
    /// menu over that selection, and its Strike out entry dispatches the
    /// strike path — the operator's "select text, then right-click to
    /// choose highlight or strike" for the cut half of the pair.
    #[gpui::test]
    fn right_click_after_selection_strikes_the_selection(cx: &mut TestAppContext) {
        init_layout_test_globals(cx);
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, cx| {
            viewer.transcript = Some(TranscriptPaneState {
                transcript_id: "t-context-strike".into(),
                media_path: "/generated/context-strike.wav".into(),
                gallery_asset_id: None,
                alignment: "original".into(),
                alignment_note: None,
                words: Some(
                    (0..12)
                        .map(|index| PaneWord {
                            word: format!("word{index:02}"),
                            start_ms: index as u64 * 100,
                            end_ms: index as u64 * 100 + 90,
                        })
                        .collect(),
                ),
                working_text: "word00 word01 word02 word03 word04 word05 word06 word07 word08 word09 word10 word11".into(),
                paragraph_breaks: Some(vec![5, 11]),
                speaker_spans: None,
                review: None,
                layers: Vec::new(),
                highlights: Vec::new(),
                strikes: Vec::new(),
                correction_edits: Vec::new(),
                source_words: Vec::new(),
            });
            viewer.active_tab = ViewerTab::Workspace;
            viewer.inspector_open = false;
            cx.notify();
        });
        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(480.), px(600.)));
        cx.run_until_parked();
        cx.update_window_entity(&viewer, |_, window, _| window.activate_window());
        cx.run_until_parked();
        let paragraph = cx
            .debug_bounds("media-transcript-paragraph")
            .expect("block 0 laid out");
        // Select words 3–9 with the two-click gesture, completing the
        // selection popover, then right-click to reach the context menu.
        cx.update_window_entity(&viewer, |viewer, _, cx| {
            viewer.selection_mode = true;
            cx.notify();
        });
        cx.run_until_parked();
        let first = gpui::Point {
            x: paragraph.left() + gpui::px(4.),
            y: paragraph.center().y,
        };
        let second = gpui::Point {
            x: paragraph.left() + gpui::px(120.),
            y: paragraph.center().y,
        };
        cx.simulate_mouse_down(first, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_up(first, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(second, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_up(second, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(
            viewer.read_with(cx, |viewer, _| viewer.selection_popover.is_some()),
            "the two clicks complete the selection popover"
        );
        // The pointer rests on a word, then the right click opens the menu
        // over the completed selection.
        cx.simulate_mouse_move(first, None, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(first, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.simulate_mouse_up(first, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("media-transcript-context-menu").is_some(),
            "the context menu opens over the completed selection"
        );

        // Strike out dispatches the strike over the selection's range.
        let strike = cx
            .debug_bounds("media-transcript-context-menu-strike")
            .expect("Strike out entry laid out");
        cx.simulate_click(strike.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        viewer.read_with(cx, |viewer, _| {
            assert!(
                viewer
                    .status
                    .as_deref()
                    .is_some_and(|status| { status.contains("Tool invoker not wired") }),
                "Strike out reached the strike dispatcher: {:?}",
                viewer.status
            );
            assert!(
                matches!(viewer.edit_request.state, ResourceState::Failed(_)),
                "the strike refusal failed the edit admission"
            );
        });
    }
}
