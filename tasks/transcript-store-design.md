# Transcript Store + Selection Algebra — Design

Status: design record for Educt gap 1 (transcript store + selection algebra),
the implementation track defined by `tasks/transcript-store-continuation-prompt.md`
and reconciled into `tasks/reduct-dual-mode-video-analysis.md`. Investigation
performed and slice 1 landed 2026-08-30. Every *verified* claim below was
checked against the tree this session; file:line citations are from those reads.

## 1. Investigation findings

### 1.1 The corpus LLM→JSON pattern (the v1 reference)

`kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs` is the pattern
the transcript layers replicate:

- **Typed target struct** (`OntologyTags`, ops.rs:71-90) with `serde(default)`
  on every field and a field-level coercion deserializer
  (`coerce_string_or_array`, ops.rs:43-68) that absorbs model shape drift
  (`{"fibo": "x"}` vs `{"fibo": ["x"]}`).
- **Prompt** via `render_docproc_template` (Jinja2); **model** via
  `hkask_inference::model_constants::classifier_model()` (ops.rs:14) — never a
  re-declared literal.
- **Extraction** via `extract_json_from_response` (`hkask-types/src/json_extract.rs`)
  — balanced-delimiter, fence-stripping, array-correct (pinned by tests).
- **Validation boundary**: `validate_ontology_tags` (ops.rs:149+) —
  allowlist filtering, normalization, length/count caps; its doc comment
  names the invariant exactly: *"the single point where LLM-produced strings
  become trusted corpus tags."* This is the discipline the layer invariants
  follow.
- **Batching** via `crate::batch::{BatchOutcome, MAX_RETRIES, retry_with_backoff}`
  (ops.rs:8) — retry with backoff around each model call.

### 1.2 The media server's LLM call paths (no third path needed)

`MediaServer` holds `Arc<dyn InferencePort>` (`hkask-types/src/ports/inference_port.rs`).
The trait already exposes everything the transcript passes need:

- `generate(prompt, parameters, tools)` (inference_port.rs:158) and
  `generate_with_model` — **the text path the transcript layers will use**.
- `generate_vision` (inference_port.rs:269) — the vision path
  (`src/gallery/vision.rs:81-93`: minijinja template via
  `crate::templates::render` → `inference.generate_vision(...)` → strict
  JSON parse with a named error carrying the raw prefix, vision.rs:98-104).
- `media_generate("transcribe", &MediaGenerateParams)` — the STT path
  (`src/tools/audio.rs:158-162`).

`faces.rs:73-77` states the house rule: *"dispatched through the inference
port, the same pattern as every other vision capability in this server."*
The transcript layers follow it — `generate`/`generate_with_model` through
the port the server already holds. `MediaOp` (8 variants, no text/vision-chat
op — `hkask-inference/src/provider.rs:24-33`) is not touched.

### 1.3 STT segment production (item 4 resolved)

`transcribe_bundle` (`src/tools/audio.rs:143-225`) parses the provider's
verbose-JSON response into `words` (audio.rs:174-192) and `segments`
(audio.rs:193-207) as **independent arrays** — there is no word-index
linkage between a segment and the words it covers. Consequence, as the
continuation prompt predicted: **LLM layers anchor to `words` only;
`segments` are a derived view, never a second ground truth.**

### 1.4 Store location (item 3 resolved — hybrid confirmed)

The media server's persistence substrate: `GalleryStore` over
`SqliteDriver` (`hkask-storage`), durable file DB at
`{kask_data_dir}/mcp/media/gallery.db` (`HKASK_MEDIA_DB` override), and the
server **refuses to start with an ephemeral in-memory fallback**
(`src/hkask_mcp_media.rs:448-503`). Ad-hoc SQL through the same driver is an
established in-server precedent (`src/images.rs:76-86`:
`gallery_store.driver().query(...)`).

Decision (recorded in the scaffold, decision 8): **transcripts + layers are
media-server-local SQLite** — new tables (`transcripts`, `transcript_layers`)
in the same DB via the existing driver, so ground truth and typed records
live together (eliminating the orphan-JOIN trap: a layer cannot outlive its
transcript in a different store). **The corpus server is a derived,
rebuildable search index** — transcripts are exported/chunked/embedded there
for repository-wide semantic search; if the index is lost, it is rebuilt from
the media store, never the reverse.

### 1.5 v2 structured outputs (item 5 — verified negative, deferred)

`response_format` has **zero grep hits in `kask/`** (re-verified this
session). Provider-enforced structured outputs would be new inference
surface. Decision: **v1 ships; v2 is a timeboxed spike only if slice 3's
measured validation failure rate justifies it.** The hard validation gate
after parsing stays either way.

### 1.6 Two process findings (method, not design)

- **Stale comment fixed**: `src/types.rs:4-5` claimed the transcript types
  are "imported from `hkask_types`" — `hkask-types` carries no such types
  (zero grep hits); they are defined in this server's `src/transcript.rs`.
  Corrected in this slice (stale comments are active misinformation).
- **Grep include-pattern trap**: `kask/`-prefixed include patterns silently
  return "no matches"; `**/`-prefixed globs work. The scaffold's earlier
  "verified absent by grep" gap claims were re-verified with corrected
  patterns — they hold (the only `highlight|reduct|edl` hits are doc-comment
  "highlighting" and the substring "edl" inside "unexpectedly").

## 2. The thesis (binding)

**LLM passes never emit timestamps — they emit indices into the immutable
`words` array; the deterministic layer owns the only index→time mapping.**
Timestamp hallucination is structurally impossible (no time fields in the
layer schemas); validation is total and cheap; every layer is replayable
against its `words`. Offload all semantics, offload zero time.

## 3. Type contracts (final)

Slice 1 landed the pure algebra in `src/transcript_select.rs`:
`WordRange` (inclusive `[start_word, end_word]`), `EdlOp` (`Keep | Cut`),
`EdlEntry`, `Edl`, `SelectionError` (named variants: `NoWordTimings`,
`WordIndexOutOfBounds`, `ReversedRange`, `OverlappingKeepOps`), and:

- `word_range_to_time_range(words, range) -> (start_ms, end_ms)` — the only
  index→time mapping.
- `text_to_word_ranges(words, text) -> Vec<WordRange>` — exact match over
  the rendered text (words joined by single spaces), word-boundary aligned,
  **all** candidates on ambiguity, empty = no match (caller surfaces).
- `edl_to_keep_ranges(words_len, edl)` — uniform semantics:
  `keep = (Keep ops in EDL order, or the full transcript when no Keep op
  exists) minus (Cut ops)`. Deterministic for every shape: no ops → full;
  all-Cut → complement (strikethrough); all-Keep → reel (EDL order
  preserved — the reorderable EDL); both → reel with strikethroughs inside.
  Keep ops must be pairwise disjoint (Cut ops may overlap — union; a Cut
  outside every Keep is a harmless no-op).
- `keep_ranges_to_clip_plan(words, keep_ranges)` — merges list-adjacent
  ranges that are also word-adjacent (contiguous media is one clip),
  preserves reel order, maps to `(start_ms, end_ms)` for
  `video_clip`/`video_concat`.
- `edl_to_clip_plan(words, edl)` — the composed slice-5 render path.

Slices 2+ add `src/transcript_layers.rs` (sibling to `transcript.rs`), each
layer deriving `Serialize, Deserialize, JsonSchema` (schemars is already a
media-crate dependency — `Cargo.toml:24`) and embedding the slice-1 types:

```rust
LayerProvenance { model: String, prompt_template: String, created_at: String }
SpeakerLayer   { provenance, spans: Vec<{ start_word, end_word, speaker, confidence }> }
ParagraphLayer { provenance, breaks_after: Vec<usize> }
CorrectionLayer{ provenance, edits: Vec<{ start_word, end_word, replacement, reason }> }
HighlightLayer { provenance, highlights: Vec<{ start_word, end_word, label, note }> }
EdlLayer       { provenance, ops: Vec<EdlEntry> }   // reuses the slice-1 types
```

Per-layer invariants (deterministic, reject-with-named-variant, never
partial application): ranges in bounds and non-reversed; spans sorted and
non-overlapping (merge or reject per layer semantics); corrections disjoint;
`EdlLayer` inherits the slice-1 EDL validation. Speaker provenance records
the producing source — audio-capable local LLM (primary, scaffold decision
3/7), text-cue pass (fallback), or dedicated model (last resort).

## 4. Mechanism decision

**v1** (chosen; works with every catalog model today): schemars-derived
schema embedded in a minijinja prompt (the `templates.rs` house pattern) →
`InferencePort::generate_with_model` → `extract_json_from_response` →
schema-validate → typed deserialize → per-layer invariants → store with
`LayerProvenance`. Model resolution via `hkask_inference::model_constants`
+ env overrides (the lib-root `models` module pattern,
`src/hkask_mcp_media.rs:69+`). Errors classify per-variant through the
`classify_inference_error` precedent (`src/error.rs:183-188`:
`NotConfigured` → `permission_denied`, else `unavailable`).

**v2** (deferred, timeboxed spike): `response_format: json_schema`
passthrough — new inference surface (verified absent). Gated on slice 3's
measured failure rate; the validation gate stays either way.

## 5. Slice status

| Slice | Status |
|---|---|
| 1 — selection algebra (pure) | **landed 2026-08-30** (`src/transcript_select.rs`, tests in-module) |
| 2 — transcript persistence (SQLite tables + JOIN round-trips) | **landed 2026-08-30** (`src/transcript_layers.rs`, `src/transcript_store.rs`, six `educt_*` tools in `src/tools/educt.rs`; tool surface 68 → 74; 145 crate tests green) |
| 3 — paragraph pass (first LLM layer; measures v1 failure rate) | **landed 2026-08-30** (`src/transcript_pass.rs` + `educt_paragraph_pass` tool; tool surface 74 → 75; the attempts/rejections counters ride every pass response — the v1 rate accumulates in live use, and the v2 spike decision reads it) |
| 4 — speaker + correction passes | **landed 2026-08-30, extended same day** (`educt_speaker_pass` with `source: "audio"` (default) \| `"text"`, `educt_correction_pass`, `educt_apply_corrections`; tool surface 75 → 78. The audio source routes through `MediaOp::ChatAudio` — child-local provider keys, OpenAI `input_audio` content parts on `/v1/chat/completions` — NOT an `InferencePort` trait method; provenance's `prompt_template` distinguishes the source, per decision 7) |
| 5 — semantic selection → EDL → render | **landed 2026-08-30** (`educt_highlight_pass` — the semantic selection; `educt_edl_from_highlights` — deterministic union-merged composition; `educt_render_edl` — the slice-1 algebra driving ffmpeg stream-copy renders, audio and video paths; tool surface 78 → 81. The closing loop: "find where he explains X and cut it to a clip" works end to end, proven against real media by a live-render test) |
| 6 — v2 spike (conditional) | deferred |
| 7 — exports + corpus search wiring | planned |
| 8 — redaction (hardest local gap) | planned, last |

## 6. Inference input-modality matrix (verified 2026-08-30)

The media server's inference surface, audited end-to-end per input modality:

| Input | Path | Status |
|---|---|---|
| **Text** | `InferencePort::generate`/`generate_with_model` → IPC bridge → zed `LanguageModelRegistry` | ✓ — the transcript passes (paragraph, speaker text-cue, correction) |
| **Images** | `InferencePort::generate_vision` → IPC bridge → `MessageContent::Image` parts (`kask_bridge/src/inference_chat.rs:734-763`) | ✓ — gallery vision (`gallery/vision.rs`), `video_caption` |
| **Audio → text (STT)** | `MediaOp::Transcribe` → child-local MediaRouter → `/v1/audio/transcriptions` (OpenRouter `input_audio` JSON / DeepInfra multipart) | ✓ — `transcribe_bundle` (word timings) |
| **Audio → LLM reasoning** | `MediaOp::ChatAudio` → child-local MediaRouter → `/v1/chat/completions` with `input_audio` content parts (OpenRouter; `media_providers.rs` `chat_audio`) | ✓ — landed 2026-08-30; the speaker pass's primary source. Local audio paths (recordings) read from disk via the shared `download_audio_bytes` helper |
| **Video** | `video_caption` → ffmpeg keyframe extraction → images → `generate_vision` (`tools/processing.rs:715-759`) | ✓ — keyframe-derived by design. Native video content parts are provider-specific (Gemini's API), not OpenAI-standard; named as a deferred gap, not built speculatively |

Why audio-chat lives in the media-provider layer (not the `InferencePort` trait):
the IPC bridge routes through zed's `LanguageModelRequest`, whose content
model has Image parts but no Audio parts — an audio trait method would
require upstream zed surface changes (DIVERGENCE). The child-local
provider path uses the same env-injected keys and the same OpenAI wire
format the STT endpoint already speaks (`input_audio`), following the
`generate_image` chat-completions precedent in the same module. Deep-module
note: `chat_audio` deepens `media_providers.rs` (more behavior behind the
unchanged `execute(op, params)` interface); the deletion test passes —
without it, the speaker pass would reconstruct HTTP auth, b64 encoding,
format detection, and response parsing at its call site.

## 7. Sources

- `tasks/transcript-store-continuation-prompt.md` (the handoff this design
  answers), `tasks/reduct-dual-mode-video-analysis.md` (decisions 5-10)
- Code read this session: corpus `tools/tagging/ops.rs`;
  `hkask-types/src/ports/inference_port.rs`; media `src/gallery/vision.rs`,
  `src/tools/audio.rs`, `src/hkask_mcp_media.rs`, `src/error.rs`,
  `src/images.rs`, `src/types.rs`, `src/transcript.rs`, `Cargo.toml`
- Verified negatives: `response_format` (zero hits in `kask/`),
  `TranscriptBundle` in `hkask-types` (zero hits), `highlight|edl|reduct`
  in media src (no real hits — see §1.6)
