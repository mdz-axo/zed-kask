# Educt Scenario Tests — Using the Local Video Analysis Mode

Status: runnable playbook — simulated scenarios for building, editing,
and excerpting transcripts in the Educt local mode, verified against the
live server 2026-08-31. Companion to `tasks/reduct-dual-mode-video-analysis.md`
(the architecture) and `tasks/transcript-store-design.md` (the design).

## Live verification findings (2026-08-31)

### Cycle 1 — server bring-up (before this playbook was written)

- **All 15 Educt tools registered and callable** (`educt_store_transcript`
  … `educt_locate`).
- **STT works live**: `transcribe_bundle` on the whisper.cpp JFK sample
  returned a real word-timed bundle. The DeepInfra 401 (below) did not
  block it — the provider registry's fallback chain served it.
- **TTS is down**: `generate_speech` fails — DeepInfra returns
  `401 Unauthorized` (the key is rejected; OpenRouter serves no TTS).
  Affects TTS only; surfaced cleanly per the failure-signal design.
  Still down as of cycle 2 — the key is the operator's to rotate.
- **Transport-stringification bug found and fixed**: the MCP transport
  serialized object-valued `AnyJsonValue` params (transcript/layer JSON)
  into their string form, so `educt_store_transcript` and
  `educt_store_layer` rejected every agent caller. Both tools now accept
  the object OR the JSON-string form (`parse_json_value`); pinned by a
  test. **Live-verified in cycle 2** — object-form store and layer calls
  are both accepted by the running server.
- **`educt_locate` added**: the deterministic quote→word-range→time-range
  mapping — the mechanical step that turns a verified citation (the
  listening skill's evidence quotes) into a media range with no model in
  the loop. Tool surface 82 → 83. **Live-verified in cycle 2** — exact
  quote → word range → time range; `no_match` surfaced for a fabricated
  dropped-"not" inversion and for non-word-aligned forms.
- Two rebuild-session clippy blockers fixed to keep the repo gate green
  (curator `sort_by` → `sort_by_key`; the media test env-lock moved to an
  async-aware tokio mutex — a std guard across an await is an error).

### Cycle 2 — Scenarios 1 & 2 verified live (binary from `df4897d5d3`)

The complete loop ran on real media, on both the audio and video paths:

- **Scenario 1 verified**: `transcribe_bundle` → `educt_store_transcript`
  (object JSON — the transport fix, live) → paragraph/speaker/correction
  passes → `educt_apply_corrections` corrected view → SRT export.
- **Scenario 2 verified end-to-end, reel rendered**: speaker spans →
  `{speaker, text}` chunks → `render_template`
  (`listening/apply-template`) → verbatim citations → mechanical substring
  verification → `educt_locate` → highlight layer →
  `educt_edl_from_highlights` (union-merged) → `educt_render_edl` →
  **rendered reel**, on the wav-keyed and mp4-keyed transcripts both.
- **Two fixes shipped in `df4897d5d3`**, both live-verified and pinned by
  tests:
  1. **Every video-path ffmpeg call was SIGPIPE-dead** — tokio's
     `status()` drops piped read-ends, so the child died on its banner
     write ("exit code: None", empty output). Fixed with `output()`
     (which also puts ffmpeg's stderr into the error). The suite had
     zero real-ffmpeg coverage — that is how it shipped.
  2. **Whisper's separator-prefixed tokens** (`" not"` vs `"not"`)
     broke the rendered-form contract; trimmed at ingestion now.
- **Precision finding (open operator decision)**: stream-copy render is
  keyframe-bound. The JFK mp4 has keyframes only at 0s/10s, so the
  lossless reel physically starts ~0.01s, not at the cited 2.279s — the
  semantic `clip_plan` is exact, the physical clip is not (four ffmpeg
  flag variants tested; none precise on this source). Options: (a) keep
  lossless-with-GOP-slack and document it, (b) add a re-encode mode (an
  8.080s frame-accurate demo exists), or (c) a per-render precision flag.
  Whichever is chosen also updates the "Stream-copy clip + concat" row
  in `tasks/reduct-dual-mode-video-analysis.md`.
- **Store state** (persists across restarts, SQLite at
  `~/.local/share/zed-kask/mcp/media/gallery.db`): T1 `375f01bf-…`
  (wav-keyed), T2 `50a64355-…` (mp4-keyed; carries the Scenario-2
  highlight + EDL layers), T3 `00e30125-…` (pre-token-fix verbatim
  store — live evidence of the token gap; delete when it has served).
- **Durable artifacts** in `~/Documents/zk-data/media-mcp/generated/`:
  the SRT, the highlights-CSV manifest, `educt-50a64355-reel.mp4` (the
  stream-copy render), `educt-50a64355-reel-frame-accurate.mp4` (the
  re-encode demo).
- **Minor backlog**: the SRT cue splitter broke mid-phrase
  ("…you, Ask | what…") rather than at punctuation — worth a look when
  next touching the export.

## The video-ingest pattern (read first)

`transcribe_bundle` validates its input as an HTTP(S) URL, and the STT
providers want audio. The realistic video pipeline:

1. `video_fetch` (or a local file) → the video.
2. Extract the audio track: `ffmpeg -i video.mp4 -vn audio.wav`
   (terminal, or `audio_capture`-style tooling).
3. `transcribe_bundle(audio_url)` — for a local WAV, host it at any
   reachable URL, or point at a public sample URL.
4. `educt_store_transcript` with the bundle's `audio_path` **edited to the
   video file's path** — the word timings were measured on the video's
   audio track, so they transfer exactly; keying the transcript to the
   video makes `educt_render_edl` produce **video** excerpts (the render
   picks the audio/video path by extension).
5. The speaker pass's audio source reads `audio_path` — for an
   mp4-keyed transcript it receives mp4 bytes (format-sniffed by the
   provider); if the audio model rejects them, run the speaker pass
   before re-keying, or keep a wav-keyed twin transcript for the passes.

---

## Scenario 1 — Ingest & annotate (the baseline loop)

**Goal**: a media file becomes a stored, paragraph-structured,
speaker-attributed, corrected transcript with captions.

| Step | Call | Expected |
|---|---|---|
| 1 | `transcribe_bundle(audio_url)` | `hkask-transcript-v1` bundle with `words[]` (timings). If `words` is empty → the degradation is surfaced; layers will refuse (Scenario 5) |
| 2 | `educt_store_transcript { transcript }` (optionally re-key `audio_path` to the video — the pattern above) | summary with `id`, `words_count`, `has_word_timings: true` |
| 3 | `educt_paragraph_pass { transcript_id }` | stored `paragraph` layer; `pass_stats` carries the v1 rate and the `structured` A/B |
| 4 | `educt_speaker_pass { transcript_id }` (default source `audio`) | stored `speaker` layer; provenance `prompt_template: educt_speaker_audio_pass`. On audio-model failure: the error surfaces — retry with `source: "text"` |
| 5 | `educt_correction_pass { transcript_id }` | stored `correction` layer (proposals; words untouched) |
| 6 | `educt_apply_corrections { transcript_id }` | `corrected_text` — the derived view |
| 7 | `educt_export { transcript_id, format: "srt" }` | `.srt` file path + cue count; cues split at sentence punctuation |
| 8 | `educt_get_transcript { transcript_id }` | summary + bundle + all layers with provenance |

**Verifies**: persistence, all four passes, the derived correction view,
SRT export, recall.

## Scenario 2 — Listening-driven excerpt reel (the centerpiece)

**Goal**: the listening skill's retrieve-cite-verify process identifies
segments of interest; each verified citation becomes a media range
mechanically; the ranges become a rendered reel.

The composition: the listening skill's evidence fields are **verbatim
substrings** of the transcript — and `educt_locate` matches exactly the
rendered form (words joined by single spaces). A verified citation
resolves to a word range with **no model in the loop** — the
no-fabrication invariant extends all the way to the clip.

| Step | Call | Expected |
|---|---|---|
| 1 | Scenario 1 steps 1-4 (a stored, speaker-attributed transcript) | — |
| 2 | Build chunks from the `speaker` layer: each span → `{speaker, text}` where text is the rendered words of the span's range (the listening template's `transcript_chunks` context) | chunks in speaker-turn order |
| 3 | `render_template` with `listening/apply-template`, context `{transcript_chunks, company_symbol}` | per-section verdicts, each evidence field a verbatim substring |
| 4 | **Verify each citation mechanically** (the skill's step 4): substring-check every evidence quote against the chunk it cites — fabricated quotes are rejected here | only verified quotes proceed |
| 5 | For each verified quote: `educt_locate { transcript_id, text: <quote> }` | `located` with word + time ranges; `no_match` means the quote isn't word-aligned (re-quote the rendered form); multiple ranges = ambiguity, pick by the chunk's span |
| 6 | `educt_store_layer { transcript_id, layer: {kind: "highlight", provenance: {model: "listening", prompt_template: "listening/apply-template", created_at: …}, highlights: [{start_word, end_word, label: <section>, note: <verdict>}]} }` | stored highlight layer — the listening verdicts as annotations |
| 7 | `educt_edl_from_highlights { transcript_id }` | stored `edl` layer — union-merged Keep ops |
| 8 | `educt_render_edl { transcript_id }` | the excerpt reel — lossless stream-copy clips, concatenated; `clip_plan` shows each range in seconds |
| 9 | `educt_export { format: "highlights_csv" }` | the reel's manifest as CSV (time ranges + labels) |

**Verifies**: the full listening→citation→range→clip loop; the
deterministic mapping (step 5) is the anti-fabrication extension — the
clip can only contain passages that verifiably exist in the transcript.

**Live-run note**: steps 1-2 and 5-9 are mechanical and verified; step 3's
template is earnings-call-shaped — point it at an earnings-call
transcript for real verdicts (the JFK smoke test exercises the mechanics,
not the sections).

## Scenario 3 — Subtractive editing (cut the filler)

**Goal**: Reduct's strikethrough — remove the unwanted, render the
complement.

| Step | Call | Expected |
|---|---|---|
| 1 | A stored transcript (Scenario 1) | — |
| 2 | Identify the unwanted passages: `educt_highlight_pass { request: "the filler words and throat-clearing" }` → the model labels them; or the agent reads `educt_get_transcript` and marks ranges itself | word ranges for the filler |
| 3 | `educt_store_layer { layer: {kind: "edl", ops: [{range, op: "cut"}, …]} }` — Cut ops over the unwanted ranges | stored `edl` layer (Cut ops may overlap — union semantics) |
| 4 | `educt_render_edl { transcript_id }` | the complement clip — everything EXCEPT the cuts, stream-copied |
| 5 | Inspect `clip_plan` in the response | the keep-ranges tile around the cuts exactly |

**Verifies**: the uniform EDL semantics (all-Cut → complement), the
render's complement path.

## Scenario 4 — Cross-recording search ("Ctrl+F for video")

**Goal**: find a passage across many recordings, clip it from the right
one.

| Step | Call | Expected |
|---|---|---|
| 1 | For each recording: Scenario 1 ingest, then `educt_export { format: "corpus_text" }` | `educt-transcript-{id}.txt` files — the rendered form |
| 2 | `corpus_convert` → `corpus_chunk` → `corpus_embed` on each exported file (the corpus server's pipeline; the agent is the composer) | the derived, rebuildable search index |
| 3 | `corpus_query { query: "where he explains the Cinderella curve" }` | the hit's chunk text — a verbatim substring of one recording's rendered form |
| 4 | Identify the recording (the chunk's source path carries the transcript ID) → `educt_locate { transcript_id, text: <chunk passage> }` | word + time ranges in that recording |
| 5 | `educt_store_layer` a highlight/EDL over the range → `educt_render_edl` | the clip from the right recording |

**Verifies**: decision 8's hybrid — media owns artifacts, corpus owns the
index, and the rendered-form contract makes hits map back exactly.

## Scenario 5 — Degradations & error paths (the honesty checks)

Each must surface a NAMED status/error — never silent, never empty-success:

| Probe | Expected |
|---|---|
| Store a transcript whose `words` is empty (STT produced no timings) | stored with `has_word_timings: false` + a surfaced degradation note |
| Run any pass on that transcript | `invalid_argument` naming `NoWordTimings` |
| `educt_speaker_pass` with `source: "bogus"` | `invalid_argument`: source must be "audio" or "text" |
| `educt_speaker_pass` with `structured: true` + audio source | `invalid_argument`: structured outputs apply to the text passes |
| `educt_export` with `format: "bogus"` | `invalid_argument`: format must be srt/highlights_csv/corpus_text |
| `educt_locate` a quote that isn't in the transcript | `status: "no_match"` (surfaced, not an error) |
| `educt_locate` a repeated passage | ALL candidate ranges (ambiguity surfaced, never a guess) |
| Any tool with an unknown `transcript_id` | `not_found` naming the ID |
| `educt_store_layer` with an out-of-bounds range | `invalid_argument` naming the invariant; nothing persisted |
| `educt_edl_from_highlights` with overlapping highlights | union-merged (one Keep op) — `ranges_merged` in the response |
| `educt_render_edl` on an EDL that cuts everything | `invalid_argument`: nothing to render |
| Delete a transcript with layers | cascade — layers removed first; counts returned |

## Model & env notes

- Pass model: `HKASK_MEDIA_PASS_MODEL` → classifier default.
- Audio speaker pass: `HKASK_MEDIA_AUDIO_CHAT_MODEL` → Voxtral (live-verified
  default).
- Structured mode: `HKASK_MEDIA_STRUCTURED_PASS_MODEL` → gpt-4o-mini family.
- STT: `HKASK_MEDIA_STT_MODEL` → Whisper; the registry falls back across
  providers on failure (observed live).
- The v1/structured A/B: run passes both ways over real transcripts; the
  `pass_stats.structured` sub-object vs the per-pass totals is the
  adoption evidence.
