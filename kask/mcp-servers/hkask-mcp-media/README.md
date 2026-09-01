# hkask-mcp-media

Media generation MCP server — image, video, audio, and 3D generation via the configured media providers.

## Tools (36)

| Tool | Description |
|------|-------------|
| `gallery_organize` | Organize a photo gallery. Point at a folder — the system creates the index, scans for images, and returns status. Use gallery_search to find photos by content. |
| `gallery_status` | Get gallery status: path, mode, image count, and total size |
| `gallery_search` | Search your gallery by describing what you're looking for. Fuzzy-matches against AI-generated tags (objects, faces, colors, composition) |
| `gallery_find_similar` | Find gallery images similar to a text description or to another image. Uses AI caption embeddings for semantic similarity (requires gallery_analyze to have been run first) |
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
| `transcribe` | Transcribe speech audio to text. Returns transcribed text for REPL injection |
| `transcribe_bundle` | Transcribe audio and return a synchronized TranscriptBundle with word-level timings |
| `audio_capture` | Capture audio from the default system microphone. Records to a WAV file optimized for Whisper transcription (16kHz mono) |
| `record_and_transcribe` | Record audio from microphone and transcribe it in one call. Returns linked audio file path and transcript |
| `generate_image` | Generate an image from a text prompt. Describe what you want to see |
| `transform_image` | Transform an existing image with a text prompt. Describe the change you want |
| `upscale_image` | Upscale an image to higher resolution |
| `generate_video` | Generate a short video from a text prompt. Describe the scene you want to see in motion |

## Configuration

This server routes media generation through the inference IPC bridge to the zed process's `MediaRouter`. Vision-LLM calls (describe, analyze, face validation) also route through the inference IPC bridge to zed's `LanguageModelRegistry` — the media process does not read `OPENROUTER_API_KEY` directly (that is read by the zed process).

| Variable | Default | Description |
|---|---|---|
| `HKASK_MEDIA_TTS_MODEL` | `DeepInfra/hexgrad/Kokoro-82M` | Text-to-speech (Kokoro via DeepInfra) |
| `HKASK_MEDIA_STT_MODEL` | `DeepInfra/openai/whisper-large-v3` | Speech-to-text (Whisper Large v3 via DeepInfra) |
| `HKASK_MEDIA_VISION_MODEL` | `OpenRouter/Qwen/Qwen3-VL-235B-A22B-Instruct` | Vision model (Qwen3-VL via OpenRouter) |
| `HKASK_MEDIA_IMAGE_GEN_MODEL` | `DeepInfra/black-forest-labs/FLUX-2-klein-4b` | Image generation model override |

All models are open-weight. Provider prefixes (`OR/`, `KC/`, etc.) route to the appropriate inference backend.

## Face recognition — design decision

Face recognition relies on vision-LLM calls, not local code. The implementation surface is the minijinja (j2) prompt templates — `validate_face_ref` (reference validation) and `match_faces` (two-image same-person comparison) in `src/templates.rs` — dispatched through the inference port, the same pattern as every other vision capability in this server. There is no local embedding model and no local geometric matching; a previous LLM-produced-"embedding" cosine path was removed because LLMs cannot emit geometrically consistent vectors. Full build-out of the face-recognition feature is **deferred** — the current templates are the working core, and any future expansion (e.g. better matching prompts, multi-reference voting) stays on the LLM-template surface. The store's nullable `embedding` column is legacy from the removed path, is unused, and is not part of this design.

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
"Transcribe this audio recording"                → transcribe
```
