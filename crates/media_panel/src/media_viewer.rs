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
use hkask_media::{
    jobs_contract::parse_job_list_response,
    jobs_contract::{JobListPayload, JobRecord},
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
            jobs_limit: hkask_media::media_limits::DEFAULT_JOB_LIST_LIMIT,
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
            self.selection_popover = None;
            self.context_menu = None;
            self.highlight_edit = None;
            self.word_drag_anchor = None;
            self.drag_last_word = None;
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

mod activity;

mod gallery;

mod inspector;

mod transcript;

mod wire;

use wire::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod viewer_layout_tests;
