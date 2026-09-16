# hkask-mcp-media

Media generation MCP server — image, video, and audio generation via the configured media providers.

## Tools (81)

The full surface is pinned end-to-end by `tool_surface_is_exactly_80_registered_tools` (`src/hkask_mcp_media.rs`) and documented per-tool in [`kask/docs/reference/mcp-servers/media.md`](../../docs/reference/mcp-servers/media.md). The table below is a partial quick-reference.

| Tool | Description |
|------|-------------|
| `gallery_organize` | Organize a photo gallery. Point at a folder — the system creates the index, scans for images, and returns status. Use gallery_search to find photos by content. |
| `gallery_status` | Get gallery status: path, mode, image count, and total size |
| `gallery_search` | Search your gallery by describing what you're looking for. Mode `tags` (default) fuzzy-matches AI-generated tags; mode `semantic` matches by caption-embedding similarity (the former `gallery_find_similar`, requires `gallery_analyze` first) |
| `gallery_add_media` | Import a video or audio file into the gallery index (`media_type` selects the kind); path-identity upsert with SHA-256 change detection. The former `gallery_add_video`/`gallery_add_audio` pair, merged |
| `gallery_refresh` | Refresh the gallery: scan for new/removed images, then update all AI metadata (objects, colors, composition, scene descriptions). Face detection is OFF by default; when include_faces=true, also scans the face reference folder (mcp/media/faces/ by default) for new reference faces, then auto-matches detected faces against the face_registry |
| `describe_image` | Describe an image in detail. Choose a style: descriptive (full scene), artistic (poetic), technical (photographic analysis), or alt_text (accessibility) |
| `gallery_analyze` | Analyze gallery images with AI: detect faces, objects, colors, composition, and generate scene descriptions. Tags are persisted and become searchable |
| `gallery_name_face` | Name a face group from gallery_analyze. Provide either a free-text name or a face_id from the face registry |
| `face_validate` | Validate a gallery image as a face reference for facial recognition. Checks: exactly 1 face, face coverage ≥15%, frontal pose, good lighting, no occlusion, sharp focus |
| `face_register` | Register a face reference with a person's name. Auto-validates against 6 criteria. Pass --force to skip validation. Stored in the face_registry for automatic matching during gallery_refresh |
| `face_scan_folder` | Scan a folder of reference face images and register each one in the face_registry. Each image must have a YAML sidecar (e.g. `alice.jpg.yaml`) with `first_name`, `last_name`, and optional `notes`. Default folder: `mcp/media/faces/` |
| `face_list` | List all registered faces in the face registry. Optionally filter by status: valid, rejected, or pending |
| `face_remove` | Remove a face from the registry by its ID |
| `gallery_timeline` | Organize gallery images by time period using EXIF dates. Returns images grouped by year, month, or decade |
| `image_remove_background` | Remove background from a gallery image. Delegates to the configured background-removal provider |
| `image_apply_style` | Apply style transfer to a gallery image. Delegates to the configured style-transfer provider |
| `image_create_collage` | Create a collage from multiple gallery images. Local composition using image crate. Three modes: search_terms, similar_to_index, or image_indices |
| `youtube_search` | Search YouTube through SerpApi for structured metadata (views, duration, channel, publication date, provider extensions, URL); does not invoke yt-dlp or download media |
| `video_fetch` | Download a selected YouTube/Vimeo/platform URL with yt-dlp and publish it as a durable local gallery asset |
| `video_clip` | Trim a video to specified start/end times using local ffmpeg |
| `video_to_gif` | Convert a video segment to GIF format using local ffmpeg |
| `image_to_video` | Animate a gallery image into a short video clip. Delegates to the configured video-generation provider |
| `video_add_caption` | Add text caption overlay to a video using local ffmpeg |
| `video_remix` | Generate a video remix: clip, add caption, convert to GIF |
| `video_from_images` | Create a video or GIF from a sequence of gallery images using ffmpeg |
| `video_concat` | Concatenate multiple video clips into one using ffmpeg |
| `video_caption` | Generate a description of video content by extracting keyframes and analyzing them with a vision LLM |
| `video_meme` | Create a meme video from a gallery image with text overlay and camera motion. Composes text rendering + AI motion generation |
| `voice_design` | Design a synthetic voice profile from a character description. Returns a VoiceDesign JSON for use with generate_speech |
| `generate_speech` | Generate speech audio from text using a voice design. Publishes and returns a durable local audio Asset; provider payloads do not enter the result |
| `transcribe_bundle` | Transcribe audio and return a synchronized TranscriptBundle with word-level timings (the former bare `transcribe` tool, merged) |
| `audio_capture` | Capture audio from the default system microphone and publish a canonical durable WAV asset optimized for Whisper transcription (16kHz mono) |
| `record_and_transcribe` | Record audio from microphone and transcribe it in one call. Returns linked audio file path and transcript |
| `generate_image` | Generate an image from a text prompt. Describe what you want to see |
| `transform_image` | Transform an existing image with a text prompt. Describe the change you want |
| `upscale_image` | Upscale an image to higher resolution |
| `generate_video` | Generate a short video from a text prompt. Describe the scene you want to see in motion |

## Tool-result contracts

The approved media wire cleanup (2026-09-05) keeps `job_list`'s existing
`{"content": [JobRecord, ...]}` response. The Queue calls
`tools::jobs::parse_job_list_response` and uses the existing `JobRecord` type:
empty arrays are valid; missing fields, malformed responses, and tool errors
surface a panel status instead of silently clearing the queue.

`display_hint` / `display_hints` contain JSON-serialized fenced media blocks.
Every hint for an indexed Asset carries its stable `gallery_asset_id`; inline
conversation and Media-panel bodies therefore resolve one shared player key.
The viewer consumes structured raw outputs directly through
`hkask_types::tool_response::display_hints_from_output_value`; text transports
use `display_hints_from_output_text`. Both viewer and widget validate bodies
with the widget's `MediaBlockBody` parser and media-kind resolver. `src` is
required. Ontology and provenance are intentional optional metadata; the
viewer retains the original JSON body, including dimensions and other fields.
Quotes, backslashes, newlines, and Unicode in paths are serialized, not
interpolated into JSON. No provider routing or GPUI layout changes accompany
this contract repair.

Pins: `media_blocks_round_trip_escaped_paths`,
`job_list_response_round_trips_through_client_decoder`, and the media viewer's
`ingest_tool_result_accepts_structured_and_text_transports`,
`load_jobs_surfaces_array_rows_and_response_failures`, and
`server_hint_round_trips_through_viewer_and_widget` tests.

## Resource and lifecycle bounds

Cardinality limits live in `hkask_types::media_limits`; server and panel callers
import them rather than copying values. Direct heavy provider, FFmpeg/ffprobe,
yt-dlp, analysis, capture/transcription, and transcript-pass RPCs share one
four-slot fail-fast authority with background jobs. The fifth combined operation
fails before external work; it is never queued, clamped, or silently truncated.
Job list/status/cancel and cheap gallery reads remain available under load.
Concat accepts at most 64 audio
or video inputs, image-sequence rendering accepts 256 images, keyframe
extraction accepts 256 frames, and image generation accepts 10 variants.
Workflow graphs are limited to 1 MiB. Cap violations fail before work begins;
requests are never clamped or silently truncated.

Multi-variant generation reports `completed`, `partial`, or `failed`, preserves
each valid published variant, and returns a causal failure for every missing
variant. Keyframe extraction owns one scratch batch, removes it on success or
failure, removes failed durable copies, aggregates failures instead of warning
once per frame, and reports partial completion explicitly.

`job_list` defaults to 20 rows, accepts at most 256, and returns `total` plus
`has_more`; the panel displays the subset rather than presenting it as the
whole history. Job history is process-local: restart loss and bounded terminal
retention are disclosed at list/status/cancel boundaries. Workflow listing is
bounded to summaries (default 100, maximum 256); `workflow_load` is the only
list/load surface that returns the full graph.

The media panel applies one latest-request ownership rule to Library, Queue,
Detail, and edits. Every success and failure is epoch-gated, malformed results
preserve the complete last-good snapshot, pagination is reachable, and loading,
ready/empty, degraded, and failed states are distinct. Queue polling uses a
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
valid results. Generated assets capture
the gallery at operation admission, before the first inference await: the snapshot
travels immutably through inference, downloads, and every variant, so a root switch
mid-flight never retargets an in-flight generation (background jobs capture at
submission). Provider-generated variants, job completions, and canonically
published local media share one rollback-armed publication aggregate. It commits
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

**Forward schema update:** add `missing`/`metadata_stale`, canonicalize existing
identities, enforce unique `(gallery_id, absolute_path)`, and remove cached gallery
count/size columns in favor of active-row aggregates. Existing annotations and
relations are preserved. Ambiguous duplicate identities stop opening with an
explicit conflict; no automatic record merge or DB deletion occurs. There are no
legacy read paths or API aliases. `GalleryStore::open` replaces `create`, and
insertion derives the relative path rather than accepting a second path identity.

Pins: `gallery_reopen_restores_requested_root`, `gallery_rescan_preserves_identity`,
`gallery_lifecycle_tests` (including `generate_image_tool_entry_binds_admission_gallery`
and `invalid_analysis_outputs_retain_staleness`), and storage's
`path_upsert_retains_annotations_and_deterministic_positions`,
`absent_alias_path_resolves_existing_ancestors_and_keeps_identity`,
`conflicting_absent_alias_spellings_fail_explicitly`,
and `forward_schema_preserves_data_and_refuses_duplicate_identity`.

## Transcript-as-timeline reference model

The local educt system follows Reduct.video's published interaction model rather
than integrating or imitating its private cloud API. The current official product
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
silently using stale source text or inventing timestamps. Reduct can re-align such
edits server-side; local re-transcription/re-alignment remains the honest capability
gap. The immutable source bundle remains available for audit, and there is no
hidden Reduct upload, credential, fallback, or cloud mode.

## Configuration

Media generation is child-local: `LazyInferencePort::media_generate` calls the media process's `MediaRouter` using its env-injected `DEEPINFRA_API_KEY` / `OPENROUTER_API_KEY` (D35). Vision/chat/embed calls use the inference IPC bridge to zed's `LanguageModelRegistry`; media generation does not make that IPC round-trip.

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
| `HKASK_MEDIA_IMAGE_GEN_MODEL` | unset | image generation and image-to-image |
| `HKASK_MEDIA_VIDEO_MODEL` | unset | text-to-video and image-to-video |
| `HKASK_MEDIA_TTS_MODEL` | unset | speech (DeepInfra only) |
| `HKASK_MEDIA_STT_MODEL` | `OpenRouter/openai/whisper-large-v3-turbo` | transcription, the educt transcript passes, and `voice_design` |

Settings inject resolved defaults into the child; standalone calls need env
configuration or an explicit override. Background removal and upscale use
fixed DeepInfra native models, require `DEEPINFRA_API_KEY`, need no model
env var, and reject model overrides. Missing selected credentials/config
and HTTP 401/403 are `permission_denied`; invalid model/provider selection
is `invalid_argument`, never a retryable outage. Image-to-image is
DeepInfra-only in the current adapters. Vision/chat/embed are unchanged.

**Migration:** qualify bare/short-alias media model settings and saved replay
overrides with a full provider name; configure that provider's key. Remove
model overrides for background removal/upscale. See the
[inference routing contract](../../crates/hkask-inference/README.md#media-routing-policy--operator-decision-2026-09-06).

## Face recognition — design decision

Face recognition relies on vision-LLM calls, not local code. The implementation surface is the minijinja (j2) prompt templates — `validate_face_ref` (reference validation) and `match_faces` (two-image same-person comparison) in `src/templates.rs` — dispatched through the inference port, the same pattern as every other vision capability in this server. There is no local embedding model and no local geometric matching; a previous LLM-produced-"embedding" cosine path was removed because LLMs cannot emit geometrically consistent vectors, and its store column was dropped with it (the forward schema update removes `face_registry.embedding` from pre-existing DBs). Full build-out of the face-recognition feature is **deferred** — the current templates are the working core, and any future expansion (e.g. better matching prompts, multi-reference voting) stays on the LLM-template surface.

## Quick Start

```bash
# The server starts automatically with kask
the zed-kask editor
# Or standalone:
hkask-mcp-media
```

## Usage

```
"Generate an image of a sunset over mountains"  → generate_image
"Search my gallery for cat photos"              → gallery_search
"Convert this video to GIF"                      → video_to_gif
"Transcribe this audio recording"                → transcribe_bundle
```
