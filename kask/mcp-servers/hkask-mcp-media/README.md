# hkask-mcp-media

Media generation MCP server — image, video, and audio generation via the configured media providers.

## Tools (80)

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
| `generate_speech` | Generate speech audio from text using a voice design. Returns audio as base64 data URI |
| `transcribe_bundle` | Transcribe audio and return a synchronized TranscriptBundle with word-level timings (the former bare `transcribe` tool, merged) |
| `audio_capture` | Capture audio from the default system microphone. Records to a WAV file optimized for Whisper transcription (16kHz mono) |
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
captured before a root switch can never act on the new gallery.

Reconciliation and analysis writes use real SQLite transactions. Analysis captures
records before awaiting vision, commits only against the same stored hash, and
only a successful complete pipeline clears staleness (including faces when face
annotations exist). Partial analysis does not certify all retained metadata, and
structurally invalid vision output — a missing colors array, an empty composition
object, or a blank caption — surfaces as an analysis error that retains staleness;
legitimate empty face/object detections are valid results. Generated assets capture
the gallery at operation admission, before the first inference await: the snapshot
travels immutably through inference, downloads, and every variant, so a root switch
mid-flight never retargets an in-flight generation (background jobs capture at
submission). Keyframes are copied into durable artifacts before indexing, not
indexed as soon-to-be-deleted extraction scratch files.

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
| `HKASK_MEDIA_STT_MODEL` | `OpenRouter/openai/whisper-large-v3-turbo` | transcription |
| `HKASK_MEDIA_AUDIO_CHAT_MODEL` | unset | audio chat (OpenRouter only) |
| `HKASK_MEDIA_STRUCTURED_PASS_MODEL` | unset | structured chat (OpenRouter only) |

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
