---
title: "Visual-event tagging and object redaction — the non-word transcript (design)"
audience: [developers, architects, operators]
last_updated: 2026-10-29
version: "1.0.0"
status: "Proposed design — unbuilt parts await operator decisions in the final section"
domain: "Media / hkask-mcp-media"
mds_categories: [domain, research]
---

# Visual-event tagging and object redaction — the non-word transcript (design)

Status: **Committed 2026-10-29 (`1a41dcfbc9`) as grounding for goal `a6c6fdf3`; the design below remains a proposal for its unbuilt parts** — cards S1/S2/S3/S5 on the Visual-transcript board carry the build, and the M1/M2 model adoptions are executing per the model report's dated brackets. Operator decisions: §10.
Date: 2026-10-29 · Author: Z-K agent (glm-5.3:cloud), composed for the principal investigator.
Companion deliverables: `kask/docs/research/media-model-selection-report.md` (model evidence, ledger run `29f97aa1cac697b3`); the task-breakdown companion was executed and removed per the house lifecycle (`bc0a6c95d4`) — outcomes on the Visual-transcript board cards M1/M2/S1-S5 under goal `a6c6fdf3`.
Line citations were verified against the tree as read during this run at HEAD `f95cb5356e` (2026-10-29); the tree has since advanced (t8 compose `27b0829862`, t9 render `3a11fa570d`, t10 docs `bcce7f1d42`, M1/M2 adoption `1a41dcfbc9`; HEAD `ddea5f7416`), and the citations drifted by those commits were re-verified and fixed 2026-10-29 at `ddea5f7416`.

## 1. Goal

The principal investigator's words: after transcription and editing, add "tagging capabilities and tagging things in the video stream at points in time or modifying and 'redacting' or removing an object from a video stream" — tags should be "like a layer of the transcript but which isn't composed of words but is composed of a 'transcript' or image events or edits or tags."

Formally: extend the transcript-linked-media discipline from word series to visual-event series. Published reference model (zk-ref-open, reduct-transcript-model): "Transcript-linked media couples a time-aligned transcript with its source recording so that text and media address each other." The visual-event layer must preserve that bidirectional addressability — **a tag resolves to a time range (and its frame assets); a time range resolves to tags (unified search) and to media (clips, renders)**; a redaction decision resolves to a rendered asset with lineage. Model passes propose; the human reviews, approves, and composes. Nothing fully automatic (co-research charter, `kask/docs/architecture/core/co-research-environment.md`).

## 2. What exists today (all claims code-verified 2026-10-29)

| Capability | Where (file:line) |
|---|---|
| Layer store: `media_layers` / `media_layer_items`, closed kind set `shot\|semantic\|object\|ocr\|audio_event\|researcher\|edl`; unknown kinds rejected | `kask/mcp-servers/hkask-mcp-media/src/layer_store.rs:89-137` (enum, `VALID_VALUES`, `parse` rejects unknown) |
| Item contract: `start_sec` (f64), `end_sec` optional (None = point-in-time), `searchable_text` never blank, `payload` arbitrary JSON, `frame_asset_ids` optional | `kask/mcp-servers/hkask-mcp-media/src/layer_store.rs:171-182` |
| Validation: whole gate runs before any write (a rejected layer is never partially stored); trim-empty text; finite/≥0/≤duration bounds; reversed-range rejection; frame refs must exist in the gallery | `kask/mcp-servers/hkask-mcp-media/src/layer_store.rs:525-588` (store never shells out; the caller supplies the ffprobe-anchored `duration_secs`, `:184-186`) |
| Provenance envelope: `model` ("user" for researcher-authored), `prompt_template` (producer-scoped labels like `ffmpeg:scdet`), `created_at`, `parameters` | `kask/mcp-servers/hkask-mcp-media/src/layer_store.rs:139-160` — **this is the machine-vs-human distinction, at the data layer** |
| Pass core: model chain per-call override → `HKASK_MEDIA_PASS_MODEL` → `HKASK_MEDIA_VISION_MODEL`, unset fails visibly (`NotConfigured` names the env); bounded chunk plan; resume cursor; one inference call per chunk | `kask/mcp-servers/hkask-mcp-media/src/layer_pass.rs:62-76`, `:92-114`, `:146-210` |
| Vision pass (t4, delivered): `layers_semantic_pass` — exactly one of `time_range` or `shot_layer_id`; shot mode chunks per stored shot; no-video-stream = `failed_precondition` naming the requirement; frames sampled and published as gallery assets with source PTS; per-chunk prompt carries the schema; object items carry `{label, confidence, box{x,y,w,h}, frame_index}`, `searchable_text "object: {label}"`, time range = chunk range | `kask/mcp-servers/hkask-mcp-media/src/tools/layers.rs:698-1185` (bounds `:752-757`, `:828-830`; shot mode `:793-827`; degradation `:738-748`, `:901-915`; object items `:980-1010`), template `kask/mcp-servers/hkask-mcp-media/src/templates.rs:85-109` |
| Deterministic gates on model output: boxes within 0..1 (`x+w<=1`, `y+h<=1`), confidences within 0..1, frame indices within attached frames — a violation rejects the whole pass, nothing stored | `kask/mcp-servers/hkask-mcp-media/src/tools/layers.rs:327-381` |
| Audio-event pass (t5, commit `b603570832`): `layers_audio_pass` via `chat_audio` (OpenRouter-only; DeepInfra refuses with a typed error naming the audio-chat route) | `kask/mcp-servers/hkask-mcp-media/src/layer_pass.rs:282-333`; `kask/crates/hkask-inference/src/media_providers.rs:459-465` |
| Unified lexical search (t6, commit `4960ec126a`): `layers_search` — item `searchable_text` joined with the transcript's working text by media path; kind filter; ASCII case-insensitive substring; 4 MiB budget; resume offsets; `scope_notes` name what was skipped, never silent | `kask/mcp-servers/hkask-mcp-media/src/tools/layers.rs:1467-1560` |
| Corpus handoff (t7, commit `f95cb5356e`): `layers_export` corpus-text — one passage per item with stable id, time range, kind, label, confidence; ids resolve back via `layers_get` | `kask/mcp-servers/hkask-mcp-media/src/tools/layers.rs:2046-2049` (fn head; shifted by the t8/t9 insertions) |
| Render primitives: `FfmpegRunner` builds filter graphs (`add_caption` drawtext, `-c:a copy`), lossless `clip`, `concat`, `probe` (`has_video_stream`/`has_audio_stream`) | `kask/mcp-servers/hkask-mcp-media/src/video/ffmpeg.rs:504-554`, `:422-461`, `:690-716`, `:316-400` |
| Word-timed STT: `TimedWord` (word, start_ms, end_ms, confidence); 16 kHz mono capture | `kask/mcp-servers/hkask-mcp-media/src/transcript.rs:17-27`; `kask/mcp-servers/hkask-mcp-media/src/tools/audio.rs:4-5` |

The visual-event transcript is therefore mostly **built**: time-anchored tag items with provenance exist (t4/t5), are searchable like transcript words (t6), and are semantically searchable through the corpus handoff (t7). The design below adds the deltas: grounded model adoption, track identity, the redaction decision + render, and the human review gates.

## 3. Layer contracts — decision: NO new kind

Full reuse of the closed set (`layer_store.rs:102-110`). The richness lives in `payload`, which is arbitrary JSON (`layer_store.rs:178-179`); the closed set already carries every content class this design needs:

| Content | Kind | Provenance `model` | Payload convention |
|---|---|---|---|
| Machine grounded tags | `object` | inference label | `{label, confidence, box{x,y,w,h}, frame_index}` (delivered) + `track_id` (proposed, §4.3) |
| Machine event/caption descriptions | `semantic` | inference label | `{kind: "caption"\|"event", label?, confidence}` (delivered) |
| On-screen text | `ocr` | inference label | delivered |
| Audio events | `audio_event` | inference label | delivered |
| Human review (approve/reject/correct) | `researcher` | `"user"` | `{decision, target_item_id, note?, corrected_label?}` |
| Human redaction decisions | `researcher` | `"user"` | redaction vocabulary (§6) |
| Composed edits/reels | `edl` | producer label | landed t8/t9 — `layers_edl_compose` + `layers_render_edl` (`27b0829862`, `3a11fa570d`) |

Rationale: a dedicated `tag` or `redaction` kind would require the full closed-set sweep — `MediaLayerKind` enum + `VALID_VALUES` + `parse` (`layer_store.rs:89-137`), the request schemas and filters in `tools/layers.rs`, the `layers_search` kind filter (`:1527`), `layers_export`, docs/live-tool instructions, and every test pinning the vocabulary — in exchange for zero contract gain. **Proposed against; the operator can overrule.**

## 4. The tag pass

The delivered `layers_semantic_pass` IS the tag pass. Four deltas:

1. **Per-call model adoption (no code default change).** Grounded tagging runs with the per-call `model` override `openrouter/Qwen3 VL 8B Instruct` — registry-confirmed in the catalog (`model_list`), non-reasoning Instruct edition (pass requests are non-reasoning; reasoning-mandatory models reject them — the rationale at `kask/crates/hkask-inference/src/model_constants.rs:213-219`). The chain already accepts per-call overrides (`layer_pass.rs:66-76`). Model evidence and the Qwen-vs-field comparison: see the model report, R2.
2. **A grounded template mode (optional prompt variant).** The current template elicits caption + events + objects + OCR with normalized boxes (`templates.rs:85-109`). A grounded variant raises box density and label specificity while keeping the identical `SemanticPassOutput` schema — zero store/schema change. The template name rides provenance (`layer_store.rs:149-152`), so the modes stay distinguishable.
3. **Deterministic track stitching (new).** The pass emits per-chunk, per-frame detections with no temporal identity (`frame_index` maps to `frames.pts_sec` at pass time, `tools/layers.rs:882-886`). A stitcher links same-label detections across consecutive chunks by box IoU over time and stores a **derived `object`-kind layer** whose items carry `track_id` in payload, with provenance `model: "deterministic:track-stitcher"`, a versioned algorithm label in `prompt_template`, and `parameters: {iou_threshold, max_gap, cross_shot: false}` (`layer_store.rs:155-159` carries parameters without schema change). Default: **no cross-shot merge** (shot boundaries are hard cuts by construction). This rides `layers_store` — no parallel annotation store. Surfaced stats: linked tracks, unlinked detections, gaps, shot-fragmentation counts.
4. **(Refinement, optional)** Tighter item time ranges: today an object item spans its whole chunk range (`tools/layers.rs:996-998`); the stitcher's derived items can tighten to the source frame's PTS ± half the sampling interval — sharper bidirectional addressability for search and redaction.

Degradation discipline (delivered, extended to the stitcher): no video stream → `failed_precondition` naming the requirement (`:738-748`); no attachable frames → `no_frames` named degradation with coverage, nothing stored (`:901-915`); unparseable model output → typed `unavailable` (`:303-307`); out-of-range boxes/confidences/frame indices → whole-pass rejection (`:327-381`); beyond `max_chunks` → resume cursor (`:839-846`). Stitcher additions: empty source → named `no_detections` note, nothing stored; malformed source layer → typed `invalid_argument` naming the layer.

## 5. Searchability and composition

- **Lexical** — t6 `layers_search` matches `searchable_text` across kinds joined with the transcript (`:1467-1560`): tags are searchable like transcript words today.
- **Semantic** — t7 `layers_export` emits one passage per item whose stable id resolves back via `layers_get` (`:2046-2049`): the corpus handoff makes visual events semantically searchable the way word-transcript exports are.
- **Visual-transcript view** (proposed; board card S1) — one time-ordered event list across all stored kinds per recording: the read surface. S1's export half is already landed as t7; S1's scope narrows to the view tool.
- **Composition** — researcher selections compose into EDL Keep ops via `layers_edl_compose` (landed t8, `27b0829862`) exactly the way transcript highlights compose into reels; redaction decisions (§6) compose through the same researcher-layer surface (`layers_store`, kind `researcher`).

## 6. Redaction as a composed edit

- **The decision** is a researcher-authored layer (kind `researcher`, provenance `model: "user"` — a human act, never machine-authored) whose items carry the redaction vocabulary:

```json
{
  "start_sec": 12.4, "end_sec": 17.6,
  "searchable_text": "redact: person (blur)",
  "payload": {
    "op": "redact", "tier": "ffmpeg", "effect": "blur|pixelate|black|delogo",
    "target": { "kind": "box", "box": {"x":0.12,"y":0.33,"w":0.41,"h":0.52} },
    "source_tag_refs": ["layer_abc/item_9f2c"],
    "status": "approved"
  }
}
```

  `target.kind: "mask_track"` (referencing a stitcher `track_id`) is **reserved vocabulary for tier 2** — representable now, renderable once tier 2 exists, so no store migration is owed later. `source_tag_refs` carries the originating tags' provenance (the OMC lineage discipline; annotation anchors on `omc:Annotation`, OMC 2.8, the vocabulary `omc.rs` already carries).
- **The tier-1 render** (proposed; slots with the landed t9 renderer `layers_render_edl`, `3a11fa570d`): takes an **approved decision layer id** (never free parameters), builds the ffmpeg filter graph per decision — time-ranged via `enable='between(t,start,end)'` over the item's range, spatial over the normalized box scaled to the probed frame dimensions; effects `boxblur`/`gblur` (blur), `pixelize` (pixelate), `drawbox` filled black, `delogo` — all verified present on the local ffmpeg 8.0.1 (empirical `ffmpeg -filters`, 2026-10-29). Only affected segments re-encode; untouched segments stream-copy — the reel-render primitive (`clip` + `concat`, `video/ffmpeg.rs:422-461`, `:690-716`) re-used; pinned by test (existing card S3's AC).
- **Tier 2** (deferred per operator decision C): mask-track targets + a configured mask/removal provider (SAM 3 masks → VOID or Generative Omnimatte; model report R3). Requesting tier 2 without a configured provider fails as `permission_denied` naming the serving path — the `NotConfigured` precedent (`tools/layers.rs:317-320`) and the project's missing-credential contract.
- **Human gate (hard):** no redaction renders without an approved researcher decision layer; unknown/empty/unapproved → typed `not_found`/`failed_precondition` naming the layer. Source media bytes are never modified — layers are database rows only (the store's contract, `layer_store.rs:162-165` and its test suite).

## 7. Human enablement

- **Review**: researcher items approve/reject/correct machine tags via `target_item_id`; machine items are immutable rows, so corrections are new researcher items — distinguishable by provenance at the data layer (`layer_store.rs:146-148`).
- **Select and compose**: selections over time ranges or search hits compose into EDLs via `layers_edl_compose` (landed t8, `27b0829862`); tags compose the way highlights do.
- **Redact**: select a tag span → author the decision item → dispatch the render; panel parity later (board card S5).

## 8. Capability edge cases (each degrades visibly, never silently)

| Edge case | Visible degradation | Enforced where |
|---|---|---|
| Short-duration tracks (object visible in only a few frames) | No track is invented: the detection stays unlinked (no `track_id`), still searchable; redaction falls back to the per-item box over its frame range; stitcher stats name the unlinked count | Stitcher stats + decision render fallback |
| Occlusion (object leaves and returns) | The track's spans stay disjoint — the gap is visible in the span list and counted in stats, never smoothed into false continuity | Stitcher span emission + stats |
| Shot-boundary fragmentation | Cross-shot merge is off by default (a named stitcher parameter); shot-mode passes already chunk per shot (`tools/layers.rs:793-827`); fragmented track segments are visible per track | Stitcher `cross_shot: false` default |
| Fast motion | Missed inter-sample events are bounded by sampling and **reported**: the coverage block exposes plan-vs-observed (the `no_frames` precedent `:901-915`); guidance: raise `frames_per_chunk` (1..=12, `:709-715`) for fast-motion ranges; confidences carry the uncertainty (the template instructs it, `templates.rs:105-107`) | Pass coverage output + template contract |
| Model hallucinated boxes | Whole-pass rejection with a named error — never a silent clamp | `validate_box` `:327-350` (delivered); extended to stitcher-derived items |

## 9. Structural validation record (machine-checked)

`lisp_eval`, run 2026-10-29. The proposed item JSON shapes were checked against the documented `layers_store` item contract and closed kind set (`layer_store.rs:171-182`, `:102-110`) plus the pass-layer box rule (`tools/layers.rs:330-350`), with a simulated ffprobe duration of 120.0 s. **Nine cases, all matching expectation** — the validator discriminates (not vacuous):

- Accepted: machine object tag (label/confidence/box/frame_index/track_id + frame refs); researcher approval (decision/target_item_id); redaction decision (op/tier/effect/target box/source_tag_refs/status).
- Correctly rejected: kind `"redaction"` (unknown — the closed set holds), blank `searchable_text`, reversed range (5.0→4.0), end beyond duration (300 > 120), box outside frame (x+w = 1.3), missing `payload`.

Not machine-checked: the ffprobe duration bound is simulated (the store enforces the real anchor at validation, `layer_store.rs:527-588`); the store's whitespace-trim rule (`:532` — trims before the empty check) is stricter than this validator and is code-cited.

## 10. Open questions for the operator

1. The **inpainting ontology-anchor ruling** (coarse 5W1H today; the model report defines the term operationally from cited sources in the meantime — never a private definition).
2. Do redaction decisions ride the landed t8 surface — researcher items via `layers_store`, composed by `layers_edl_compose` (one surface, redaction as a second payload vocabulary) — or a dedicated compose path? Proposal: ride t8.
3. Registry-confirm `Qwen3 VL 8B Instruct` as the grounded-tagging model (per-call override vs new pass default — the cost trade is yours).
4. Tier-2 timing (decision C stands; the serving-path decision — fal for masks vs RunPod runner, and the Python-runner allowlist exception — is the follow-up when taken).
5. Track stitcher: default-on after S2, or opt-in per recording? Proposal: opt-in, like every other layer generator.