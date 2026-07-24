# hkask-mcp-media

Media generation MCP server — image, video, audio, and 3D generation via fal.ai and other providers.

## Tools (36)

| Tool | Description |
|------|-------------|
| `gallery_organize` | Organize a photo gallery. Point at a folder — the system creates the index, scans for images, and returns status. Use gallery_search to find photos by content. |
| `gallery_status` | Get gallery status: path, mode, image count, and total size |
| `gallery_search` | Search your gallery by describing what you're looking for. Fuzzy-matches against AI-generated tags (objects, faces, colors, composition) |
| `gallery_find_similar` | Find gallery images similar to a text description or to another image. Uses AI caption embeddings for semantic similarity (requires gallery_analyze to have been run first) |
| `gallery_refresh` | Refresh the gallery: scan for new/removed images, then update all AI metadata (objects, colors, composition, scene descriptions). Face detection is OFF by default; when include_faces=true, also scans the face reference folder (~/.hkask/faces/ by default) for new reference faces, then auto-matches detected faces against the face_registry |
| `describe_image` | Describe an image in detail. Choose a style: descriptive (full scene), artistic (poetic), technical (photographic analysis), or alt_text (accessibility) |
| `gallery_analyze` | Analyze gallery images with AI: detect faces, objects, colors, composition, and generate scene descriptions. Tags are persisted and become searchable |
| `gallery_name_face` | Name a face group from gallery_analyze. Provide either a free-text name or a face_id from the face registry |
| `face_validate` | Validate a gallery image as a face reference for facial recognition. Checks: exactly 1 face, face coverage ≥15%, frontal pose, good lighting, no occlusion, sharp focus |
| `face_register` | Register a face reference with a person's name. Auto-validates against 6 criteria. Pass --force to skip validation. Stored in the face_registry for automatic matching during gallery_refresh |
| `face_scan_folder` | Scan a folder of reference face images and register each one in the face_registry. Each image must have a YAML sidecar (e.g. `alice.jpg.yaml`) with `first_name`, `last_name`, and optional `notes`. Default folder: `~/.hkask/faces/` |
| `face_list` | List all registered faces in the face registry. Optionally filter by status: valid, rejected, or pending |
| `face_remove` | Remove a face from the registry by its ID |
| `extract_object` | Extract a specific object from an image using AI segmentation. Returns the isolated object as a new image |
| `gallery_timeline` | Organize gallery images by time period using EXIF dates. Returns images grouped by year, month, or decade |
| `image_remove_background` | Remove background from a gallery image. Delegates to DeepInfra Bria RMBG 2.0 |
| `image_apply_style` | Apply style transfer to a gallery image. Delegates to fal.ai Flux dev img2img |
| `image_create_collage` | Create a collage from multiple gallery images. Local composition using image crate. Three modes: search_terms, similar_to_index, or image_indices |
| `video_clip` | Trim a video to specified start/end times using local ffmpeg |
| `video_to_gif` | Convert a video segment to GIF format using local ffmpeg |
| `image_to_video` | Animate a gallery image into a short video clip. Delegates to fal.ai Seedance 2.0 |
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

Provider API keys configured via standard environment variables (`DI_API_KEY`, `FA_API_KEY`, `TG_API_KEY`, `OR_API_KEY`, `KC_API_KEY`).

| Variable | Default | Description |
|---|---|---|
| `HKASK_MEDIA_TTS_MODEL` | `FA/qwen-3-tts` | Text-to-speech (Qwen3-TTS, Apache 2.0) |
| `HKASK_MEDIA_STT_MODEL` | `FA/wizper` | Speech-to-text (Whisper v3 Large, MIT) |
| `HKASK_MEDIA_VISION_MODEL` | `KC/qwen/qwen3-vl-235b-a22b-instruct` | Vision model (Qwen3-VL, Apache 2.0) |
| `HKASK_MEDIA_IMAGE_GEN_MODEL` | `FA/flux-2` | Image generation (FLUX.2 [dev], open-source) |

All models are open-weight. Provider prefixes (`FA/`, `KC/`, etc.) route to the appropriate inference backend.

## Quick Start

```bash
export FA_API_KEY="your-fal-ai-key"
# The server starts automatically with kask
kask chat
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
