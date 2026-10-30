# hkask-mcp-media

Research media MCP server — ingest, organize, annotate, search, transcribe,
and extract existing evidence. Source-linked excerpts and reels are derived
outputs, not synthetic evidence. Scanning never changes source bytes.
Historical media, lineage, and OMC provenance remain readable.

## Tools

The canonical inventory is the build-generated `TOOL_NAMES`, checked against
`MediaServer::combined_router()` by `tool_names_match_live_router`. Do not copy
a historical tool count into clients. See the [reference](../../docs/reference/mcp-servers/media.md)
for research operations, request contracts, and the retained presentation boundary.
The table below is a partial quick-reference.

| Tool | Description |
|------|-------------|
| `gallery_organize` | Organize a photo gallery. Point at a folder — the system creates the index, scans for images, and returns status. Use gallery_search to find photos by content. |
| `gallery_status` | Get gallery status: path, mode, image count, and total size |
| `gallery_search` | Search your gallery by describing what you're looking for. Mode `tags` (default) fuzzy-matches AI-generated tags; mode `semantic` matches by caption-embedding similarity (the former `gallery_find_similar`, requires `gallery_analyze` first) |
| `gallery_add_media` | Import a video or audio file into the gallery whose folder contains it, not the active gallery (`media_type` selects the kind); path-identity upsert with SHA-256 change detection. The former `gallery_add_video`/`gallery_add_audio` pair, merged |
| `gallery_refresh` | Refresh the gallery: scan for new/removed images, then update all AI metadata (objects, colors, composition, scene descriptions). Face detection is OFF by default; when include_faces=true, also scans the face reference folder (mcp/media/faces/ by default) for new reference faces, then auto-matches detected faces against the face_registry |
| `describe_image` | Describe source content; distinguish observations from researcher interpretation |
| `gallery_analyze` | Analyze source objects, colors, composition, and scenes; faces require explicit pipeline selection. Model-derived metadata stays separate from researcher annotations |
| `gallery_name_face` | Name a face group from gallery_analyze. Provide either a free-text name or a face_id from the face registry |
| `face_validate` | Validate a gallery image as a face reference for facial recognition. Checks: exactly 1 face, face coverage â¥15%, frontal pose, good lighting, no occlusion, sharp focus |
| `face_register` | Register a face reference with a person's name. Auto-validates against 6 criteria. Pass --force to skip validation. Stored in the face_registry for automatic matching during gallery_refresh |
| `face_scan_folder` | Scan a folder of reference face images and register each one in the face_registry. Each image must have a YAML sidecar (e.g. `alice.jpg.yaml`) with `first_name`, `last_name`, and optional `notes`. Default folder: `mcp/media/faces/` |
| `face_list` | List all registered faces in the face registry. Optionally filter by status: valid, rejected, or pending |
| `face_remove` | Remove a face from the registry by its ID |
| `gallery_timeline` | Organize gallery images by time period using EXIF dates. Returns images grouped by year, month, or decade |
| `image_create_collage` | Create a collage from multiple gallery images. Local composition using image crate. Three modes: search_terms, similar_to_index, or image_indices |
| `youtube_search` | Search YouTube through SerpApi for structured metadata (views, duration, channel, publication date, provider extensions, URL); does not invoke yt-dlp or download media |
| `video_fetch` | Download a selected YouTube/Vimeo/platform URL with yt-dlp and publish it as a durable local gallery asset |
| `video_clip` | Trim a video to specified start/end times using local ffmpeg |
| `video_to_gif` | Convert a video segment to GIF format using local ffmpeg |
| `video_add_caption` | Add text caption overlay to a video using local ffmpeg |
| `video_concat` | Concatenate multiple video clips into one using ffmpeg |
| `video_caption` | Generate a description of video content by extracting keyframes and analyzing them with a vision LLM |
| `transcribe_bundle` | Transcribe audio and return a synchronized TranscriptBundle with word-level timings (the former bare `transcribe` tool, merged) |
| `audio_capture` | Capture audio from the default system microphone and publish a canonical durable WAV asset (16kHz mono) |
| `record_and_transcribe` | Record audio from microphone and transcribe it in one call. Returns linked audio file path and transcript |

## Research annotations and recording retrieval

The source inventory is 90 tools, pinned against the real router and request schemas.
A running editor must rebuild/restart its media child to expose this inventory.

| Tool | Required request fields |
|---|---|
| `gallery_annotation_create` | `gallery_asset_id`, `kind` (`tag` or `note`), `value` |
| `gallery_annotation_list` | `gallery_asset_id` |
| `gallery_annotation_update` | `gallery_asset_id`, `annotation_id`, replacement `value` |
| `gallery_annotation_delete` | `gallery_asset_id`, `annotation_id` |
| `educt_annotation_create` | `transcript_id`, inclusive source `start_word`, `end_word`, `label`, `note` |
| `educt_annotation_list` | `transcript_id` |
| `educt_annotation_update` | `transcript_id`, `annotation_id`, all replacement range/label/note fields |
| `educt_annotation_delete` | `transcript_id`, `annotation_id` |
| `educt_search_transcripts` | `query`, `mode: "lexical"` |

Annotations are supplied research observations, not authenticated human identity
or verified evidence. They remain separate from model metadata. Use stable Asset
IDs, not gallery positions; transcript annotation ranges index immutable source
words, not corrected-text tokens. The annotation ID is a researcher highlight
layer ID; select it explicitly for `educt_edl_from_highlights` with `layer_id`.

Recording search is literal substring matching (ASCII case-insensitive,
non-ASCII exact), not semantic retrieval. Optional `limit` is 1–100 (default 20),
`recording_limit` 1–500 (default 100); `recording_offset`, `start_byte`, and
`gallery_asset_id` scope/resume the query. Nonblank queries are capped at 256
UTF-8 bytes and matching at 4 MiB per call (not a bound on one recording's
load/projection). Keep each exact excerpt's source/transcript IDs and available
word/time ranges, including missing/unaligned timing disclosures. `has_more`,
`search_complete`, result/byte-limit flags and scan counts disclose bounded
coverage. Resume with both `next_recording_offset` and `next_start_byte`; these
refer to current order/working text, not a frozen snapshot. Export corpus text
for semantic search. Search hits and annotations do not certify source truth.

Creative generation/TTS/workflow execution is retired. Historical Asset records,
lineage, OMC graphs, transcripts, and source-derived excerpts/reels remain
readable. `gallery_lineage` also returns the saved historical workflow graph
when one is referenced; no workflow replay or new creative execution is exposed.
Face identification is opt-in, never implied by generic analysis.

## Source-linked visual evidence

`video_caption` explicitly analyzes sampled visual frames, not the complete
recording or its audio. Optional `interval_sec` (default 2 seconds) and
`max_frames` (default 10, bounded by the extraction limit) control sampling.
Responses distinguish the requested sample from observed source-frame timestamps
and disclose missing-frame failures and visual-only limitations. Transcribe
speech separately rather than treating a visual caption as spoken evidence.

`video_extract_frames` publishes each selected frame through the atomic asset
publisher. Lineage records source identity, the SHA-256 of the private snapshot
actually decoded, a matching indexed parent when available, and actual source
PTS observed from FFmpeg. Frame ordinals are not timestamps. Originals remain
unchanged; failures do not leave half-published assets.

## Tool-result contracts

`job_list` has one strict object response consumed through
`tools::jobs::parse_job_list_response`: `jobs`, `total`, `limit`, `has_more`,
`history_scope`, and `restart_behavior` are required and count-coherent. Legacy
arrays, incomplete objects, unknown statuses, malformed responses, and tool
errors surface a panel status instead of silently clearing the queue.

`display_hint` / `display_hints` contain JSON-serialized fenced media blocks.
Every hint for an indexed Asset carries its stable `gallery_asset_id`; indexed
publication results also carry `gallery_changed: true`. The panel derives
Library invalidation from that completed-result fact rather than tool names,
and read-only gallery hints do not cause refreshes.
The viewer consumes structured raw outputs directly through
`hkask_types::tool_response::display_hints_from_output_value`; text transports
use `display_hints_from_output_text`. Both viewer and widget validate bodies
with the widget's `MediaBlockBody` parser and media-kind resolver. `src` is
required. Ontology and provenance are intentional optional metadata; the
viewer retains the original JSON body, including dimensions and other fields.
Quotes, backslashes, newlines, and Unicode in paths are serialized, not
interpolated into JSON. No provider routing or GPUI layout changes accompany
this contract repair.

`video_fetch` validates the public source before starting `yt-dlp`. Its download is temporary input to the canonical
rollback-armed local publisher, so file, Asset row, stable identity, lineage,
and OMC graph commit together or leave no residue. Successful yt-dlp warnings
such as a missing JavaScript runtime remain visible; authorization, unavailable
video, extractor, missing-binary, and unknown failures retain distinct classes.

The media widget treats every block locator as untrusted. Local media must be
under the artifacts root or be an exact path observed through a successful
gallery result; symlink escapes fail. Inline data is MIME-checked and capped at
32 MiB decoded. Public image/audio fetches validate DNS, reject redirects, and
read through the cap. Platform inputs and yt-dlp stream outputs are validated
before FFmpeg. **DNS-rebinding scope and decision state (verified 2026-10-06,
MF-5):** exactly two subprocess paths make outbound connections in this
server's usage — yt-dlp (`video_fetch`) and ffprobe (`video_info` on a
remote URL; ffprobe is bundled with FFmpeg, and FFmpeg's transcode
invocations receive only local, preflight-validated paths). Both paths run
`validate_tool_url_with_dns` at preflight; the residual gap is the
connect-time TOCTOU window between the preflight's validated resolution and
the subprocess's own DNS lookup. The mitigation is priced out with current
tooling: neither yt-dlp nor ffprobe exposes a resolve-style flag, URL-to-IP
rewriting breaks TLS certificate validation (no SNI override), and
hosts-file mutation is global system state — a pin would require upstream
tool support or sandbox-grade networking. **Ruling (operator, 2026-10-06):
accepted risk.** The named TOCTOU window is accepted as a residual risk on
the media-lead's recommendation: the exposure is narrow (two
preflight-validated subprocess paths, agent-mediated callers, a targeted
timing attack against this toolchain), the blast radius is small (a
low-grade SSRF on a workstation — yt-dlp's output is media-validated before
FFmpeg, the gallery is local, ffprobe leaks at most response-shape hints),
and the only implementable mitigation is refusing remote URLs in those two
tools, which would amputate the media-ingestion workflow. **Revisit
condition:** a server deployment with valuable internal services, or a
workflow feeding URLs from untrusted content without agent mediation, voids
this acceptance and reopens the mitigation question (then worth the
sandbox-grade cost).

`youtube_search` performs exactly one paid SerpApi request and reads one provider
page. `max_results` caps that page; it does not trigger paid continuation calls.
The response discloses `requested_max_results`, `count`,
`more_results_available`, `provider_pages_fetched: 1`, and
`provider_request_limit: 1` without returning continuation secrets.

Pins: `media_blocks_round_trip_escaped_paths`,
`job_list_response_round_trips_through_client_decoder`, `video_fetch_*`, the
widget locator/redirect/yt-dlp tests, and the media viewer's
`ingest_tool_result_accepts_structured_and_text_transports`,
`load_jobs_surfaces_strict_rows_and_response_failures`, and
`server_hint_round_trips_through_viewer_and_widget` tests.

## Resource and lifecycle bounds

Cardinality limits live in `hkask_types::media_limits`; server and panel callers
import them rather than copying values. Direct heavy provider, FFmpeg/ffprobe,
yt-dlp, analysis, capture/transcription, and transcript-pass RPCs share one
four-slot fail-fast authority with background jobs. The fifth combined operation
fails before external work; it is never queued, clamped, or silently truncated.
Job list/status/cancel and cheap gallery reads remain available under load.
Concat accepts at most 64 audio
or video inputs; keyframe extraction accepts 256 frames.
Cap violations fail before work begins;
requests are never clamped or silently truncated.

Keyframe extraction owns one scratch batch, removes it on success or
failure, removes failed durable copies, aggregates failures instead of warning
once per frame, and reports partial completion explicitly.

`job_list` defaults to 20 rows, accepts at most 256, and returns `total` plus
`has_more`; the panel displays the subset rather than presenting it as the
whole history. Job history is process-local: restart loss and bounded terminal
retention are disclosed at list/status/cancel boundaries. The retained queue
accepts `transcribe`, `audio_trim`, `audio_concat`, `video_clip`, and `video_concat`.
`params` is serialized JSON matching that operation's request, not a generation
request. Transcription jobs return a source-linked bundle; store it explicitly
if durable transcript retrieval is needed.

The media panel applies latest-request ownership to Library, Queue, and Detail.
Direct trim/concat edits are serialized: while one side-effecting edit is active,
conflicting controls are disabled and a second dispatch is rejected visibly.
Every success and failure is epoch-gated, malformed results preserve the
complete last-good snapshot, pagination is reachable, and loading, ready/empty,
degraded, and failed states are distinct. Queue polling uses a
single GPUI-native timer only while the Queue tab is active and a nonterminal
job exists; hiding the tab, reaching terminal state, or failure cancels it.

## Gallery lifecycle — operator decision 2026-09-06

`gallery_organize(path)` validates and canonicalizes the directory before opening
its durable record. It activates that requested gallery, including after restart;
startup itself does not choose a gallery. Reopening preserves the stored mode.
The response includes `requested_mode`, effective `mode`, and `mode_preserved`
when they differ. A failed open or reconciliation leaves prior activation intact.

Identity is **gallery + canonical absolute path**, not content hash. Unchanged
rescans keep the asset ID, original `added_at`, annotations, albums, and lineage.
Equal-content files at different paths remain distinct. Changed bytes update
physical metadata and set `metadata_stale`; source bytes are never modified by
scanning, in any gallery mode.

The approved missing policy is **retain and mark missing**. Complete scans mark
absent supported images only within their recursive/one-level scope. Missing
records retain all metadata but disappear from normal counts, positional lists,
search, timeline, and album positions. Returning paths recover the same ID.
External generated/imported assets and audio/video are outside image-scan
absence inference. Decode/read/walk errors or skipped symlinks produce a degraded
scan and prohibit absence inference for the entire scan. `.hkask-gallery`
directories are excluded.

`gallery_list_assets` includes stable `id` and `metadata_stale` fields and
carries the listing's `gallery_id` — the consumer-side identity boundary. The
media panel reconciles against it: a response for a different gallery clears
the previous gallery's indexed rows and pending selection/deletion actions,
superseded listing responses (an older request epoch) are dropped, and
terminal pages reconcile their exhausted tails against the payload's `total`.
Inspect a retained missing record with `gallery_asset_detail(image_id=...)`;
provide exactly one of `image_id` or active `image_index`. The existing Detail
inspector displays `missing` and `metadata_stale` in its Record section.
Positional indices use the same `(added_at, id)` order across listing, lookup,
and album positions; positions are not durable identities. Panel detail and
delete actions address assets by stable `image_id` (`gallery_delete_image`
accepts exactly one of `image_id` or `image_index`) — a positional index
captured before a root switch can never act on the new gallery. Asset deletion
preserves linked transcripts and atomically detaches their live gallery link;
the immutable source Asset ID remains queryable. Index-only deletion leaves a
still-present source file usable. Requested file deletion happens before the
gallery row is removed, and a filesystem failure surfaces with its cause rather
than warning and falsely reporting success. Missing local paths are classified by
locator syntax rather than current existence, so tools preserve the downstream
filesystem/FFmpeg cause instead of misreporting a deleted file as a malformed URL.

Reconciliation and analysis writes use real SQLite transactions. New records begin
analysis-pending. Refresh targets the exact added/changed/restored records returned
by reconciliation rather than guessing positional indices. Analysis captures
records before awaiting vision, commits only against the same stored hash, and
only a successful complete pipeline clears staleness (including faces when face
annotations exist). Partial analysis remains retryable, reports `partial`, and
atomically replaces prior model-derived metadata for the requested pipelines while
preserving user-authored tags. Invalid modes, pipelines, bounds, or selection
indices fail before inference. Structurally invalid vision output — a missing
colors array, an empty composition object, or a blank caption — surfaces as an
analysis error that retains staleness; legitimate empty face/object detections are
valid results. Every produced file is
filed under the gallery whose folder contains it — `media-mcp/generated/` — whatever
gallery is active. File-producing research jobs and canonically published local
media share one rollback-armed publication aggregate. It commits
the gallery Asset, generation lineage, effective parameters, and OMC v2.8
creation graph in one SQLite transaction; coupled files and rows roll back on
failure, and cancellation arbitrates before terminal job publication. Each graph:
the output Asset links to its creation Task and Provenance; the Task links to its
completed State/StateDescriptor; and an OMC Role links that Task to the responsible
hkask media Service/Participant. `gallery_asset_detail` returns this structured
`omc_creation_graph` plus any typed transcript-render origin. Deleting the Asset
cascades its graph, lineage, gallery metadata, and render relationship in the same
parent-row statement. Keyframes are copied
into durable artifacts before indexing, not indexed as soon-to-be-deleted extraction
scratch files.

An absent file's canonicalization resolves its existing symlink ancestors (the
deepest existing prefix is canonicalized, the absent remainder appended
lexically), so `/alias/clip.mp4` and `/real/clip.mp4` denote one identity even
while the file is offline — a missing file cannot split an asset record across
two spellings.

Schema DDL creates `missing` and `metadata_stale` directly and enforces unique `(gallery_id, absolute_path)`; gallery count/size derive from active-row aggregates, with no legacy read paths, API aliases, or migration shims. `GalleryStore::open` opens or creates the canonical root, and insertion derives the relative path from the canonical absolute path.

Pins: `gallery_reopen_restores_requested_root`, `gallery_rescan_preserves_identity`,
`gallery_lifecycle_tests` (including `invalid_analysis_outputs_retain_staleness`),
and `gallery::store::tests`
(`path_upsert_retains_annotations_and_deterministic_positions` and
`absent_alias_path_resolves_existing_ancestors_and_keeps_identity`).

## Transcript-as-timeline reference model

The local educt system follows Reduct.video's published interaction model rather
than calling its private cloud API. The current official product
surface describes selecting transcript text to create video highlights, arranging
highlights into a Reel, deleting text to skip media, correcting transcript text,
and exporting captions or finished video:

- <https://reduct.video/product/edit-video/>
- <https://help.reduct.video/en/articles/2528101-how-do-i-correct-a-transcript>

The implemented local model preserves the load-bearing structure: immutable
word-level time anchors; text selections as media ranges; overlapping labeled
highlights; ordered Keep ranges as a Reel; Cut ranges as strikethrough/subtractive
editing; deterministic FFmpeg rendering; and durable SRT, highlight CSV, corpus
text, and rendered-media outputs. Transcript deletion removes editable layers in
one database transition but preserves exports and rendered media with immutable
source transcript/layer IDs and nullable live links. Gallery Asset deletion keeps
transcripts and marks their source relationship detached instead of destroying
editorial work.

The latest correction layer is the working transcript used by inspection,
`educt_locate`, semantic highlighting, SRT, and corpus-text export. A pure
projection replaces text one-for-one over cloned `TimedWord`s, retaining every
source timestamp. If a correction changes token cardinality, its corrected text
remains readable and corpus-exportable, but timing-dependent navigation,
highlighting, and SRT fail with an explicit unaligned precondition instead of
silently using stale source text or inventing timestamps. The local re-alignment
closes that gap (2026-10-06): `educt_realign_transcript` stores a realignment
layer recording the decision to project the correction with re-anchored
timings — each cardinality-changing edit's replacement tokens slice its source
range's span equally, interpolated tokens carry no STT confidence, and the
immutable source bundle and its timings are never modified. After the
realignment layer exists, the working transcript is `re_anchored` and every
timed consumer (locate, highlighting, SRT) works again; the unaligned
precondition is retained as the failure mode for the cases re-anchoring
genuinely cannot handle (an untimed source, or a cardinality-changing
correction with no realignment layer). Reduct can re-align such edits
server-side too; the local path no longer depends on it. The immutable source
bundle remains available for audit. Local educt never uploads to or falls
back to a cloud service.

## Configuration

Transcription is child-local: `LazyInferencePort::media_generate` uses the
media process's `MediaRouter` and the selected provider's env-injected
`DEEPINFRA_API_KEY` / `OPENROUTER_API_KEY`. Vision/chat/embed use the IPC bridge
to zed's `LanguageModelRegistry`. Shared inference generation support is outside
this server's research-tool boundary.

Routing policy was ratified on **2026-09-06**, superseding `d660f3b754`
(scoring/automatic fallback) and `8cc79c797e` (registration order), while
preserving `f86cf19a70` (child-local media). Explicit `params.model` wins;
otherwise the operation's env model applies. Use full `OpenRouter/...` or
`DeepInfra/...` model names (ASCII case-insensitive provider names); bare
models, short aliases, and blank/whitespace-invalid values are rejected.
Only the selected provider is called, with its local model ID. There is no
automatic cross-provider retry on any error.

| Variable | Settings default | Operations |
|---|---|---|
| `HKASK_MEDIA_STT_MODEL` | `DEFAULT_MEDIA_STT_MODEL` | transcription and educt transcript passes |
| `HKASK_MEDIA_VISION_MODEL` | `DEFAULT_MEDIA_VISION_MODEL` | source-image and video analysis |

Settings inject resolved defaults into the child; standalone calls need env
configuration or an explicit override. Explicitly empty settings clear a
default; there is no hidden server fallback. Missing selected credentials/config
and HTTP 401/403 are `permission_denied`; invalid model/provider selection
is `invalid_argument`, never a retryable outage. `model_list` / `model_info`
discover configured STT and registry-confirmed vision models only; registry
failures surface rather than masquerading as empty discovery.

**Migration:** configure only research STT/vision model settings with a full
provider name and the selected provider's key; remove retired TTS/image/video
model settings. Historical lineage does not authorize replaying creative tools. See the
[inference routing contract](../../crates/hkask-inference/README.md#media-routing-policy--operator-decision-2026-09-06).

## Face recognition — design decision

Face detection and identification are optional, explicitly selected operations,
not implied by generic gallery analysis or auto-analysis. Face recognition relies on vision-LLM calls, not local code. The implementation surface is the minijinja (j2) prompt templates — `validate_face_ref` (reference validation) and `match_faces` (two-image same-person comparison) in `src/templates.rs` — dispatched through the inference port, the same pattern as every other vision capability in this server. There is no local embedding model and no local geometric matching; a previous LLM-produced-"embedding" cosine path was removed because LLMs cannot emit geometrically consistent vectors. `run_face_matching` (`src/faces.rs`) groups valid `face_registry` entries by `(first_name, last_name)` so multiple reference portraits for the same person evaluate as a best-of set (early-exiting at confidence ≥ 0.85), replaces unnamed `face` tags in place via `GalleryStore::replace_tag` while preserving `bbox`, skips already-identified `face` tags on subsequent refreshes, and surfaces crop errors rather than falling back to uncropped full images. Full build-out beyond this vision-LLM surface remains **deferred — decision recorded 2026-10-06**.

## Quick Start

```bash
# The server starts automatically with kask
the zed-kask editor
# Or standalone:
hkask-mcp-media
```

## Usage

```
"Index this research recording folder"         → gallery_organize
"Find images of the instrument setup"           → gallery_search
"Locate this phrase in stored interviews"       → educt_search_transcripts
"Transcribe and store this interview"           → transcribe_and_store
```
