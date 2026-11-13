---
name: transcript-reel
description: "Distill a recording into a highlight reel over the media server's educt layer system: record and transcribe with word-level timings, run correction and speaker passes, select highlights semantically, compose and render an EDL, and export captions or corpus text. Reifies the Reduct.video transcript-linked-media discipline (operator ruling 2026-09-25) over the educt transcript tools, with MovieLabs OMC provenance vocabulary."
audience: user
---

# Transcript Reel

Turn a meeting, interview, or lecture recording into a short highlight
reel with a corrected, searchable transcript. The educt tool family
stores an immutable word-level transcript and derives everything else
as validated layers (correction, paragraph, speaker, highlight, EDL) —
timings are never touched, so every reel is reproducible from its
layers.

## Reference model and labels

Reduct.video — the transcript bundled with its video as one linked artifact: correct the transcript, highlight passages, and edit the video by selecting transcript ranges, as defined by the Reduct API and user interface (`onto_anchor` → derived `transcript_linked_media`, operator ruling 2026-09-25; text-based video editing per Berthouzoz, Li & Agrawala, 2012 `doi:10.1145/2185520.2185563` → `onto_anchor` `derived:edit_decision_list`). The educt layers are the local form of that model: corrections, speakers, highlights and the EDL are layers over an immutable word-level transcript, so the reel is cut from the text and the timings never move. OMC (`omc:Provenance`) names only the provenance vocabulary.

- **Initial condition:** a stored transcript with word timings (Phase 1).
- **Target condition:** the Convergence gate passes and every quote in the summary resolved with `educt_locate`.
- **D/P labelling:** transcription, correction, paragraph, speaker and highlight passes are P (model passes), critiqued by each pass's validated layer stats and rejection rate, and by the operator's review of the selection. Layer validation, EDL composition, rendering, `educt_locate` and the gate are D.

## When to Use

- The operator wants "the key 2 minutes" from a long recording.
- A meeting/interview was recorded and needs a corrected transcript,
  speaker attribution, and a shareable clip.
- Building a searchable archive of recordings: bounded lexical search uses
  `educt_search_transcripts`; semantic retrieval hands off via `educt_export`
  to the corpus pipeline. Search-only work does not require rendering a reel.

## When NOT to Use

- Live transcription only, with no media file to keep — plain
  `record_and_transcribe` without storing is enough.
- The source is already text (a document) — use the corpus pipeline
  instead.

## Instructions

### Phase 1 — Capture and store

1. If recording now: call `record_and_transcribe` with the duration.
   It publishes the recording as a gallery asset (`output`,
   `gallery_asset_id`) and returns the synchronized TranscriptBundle under
   `transcript` — then call `educt_store_transcript` with that bundle and
   the `gallery_asset_id` to obtain the transcript id the later phases
   anchor to (record_and_transcribe does not store it itself). A response
   with `status: partial` kept the recording but has no transcript: call
   `transcribe_and_store` on its `output`.
   If the media already exists: call `transcribe_and_store` with the
   audio/video path or URL (plus the gallery asset id if the media is
   indexed). It transcribes and stores the bundle server-side in one
   call, returning only the summary — for long media this is the only
   sane path: an hour-long bundle is ~550KB of JSON, and relaying it
   through the model context (`transcribe_bundle` then
   `educt_store_transcript`) costs ~140K tokens each way. If the
   summary notes missing word timings, stop — layers cannot anchor to
   a timing-free transcript; re-transcribe. (`transcribe_bundle` →
   `educt_store_transcript` remains available for short media when the
   caller needs the bundle in hand.)

### Research retrieval and annotations (when requested)

- Search existing recordings with `educt_search_transcripts`, `mode: "lexical"`
  and a literal `query`. This is ASCII case-insensitive substring matching
  (non-ASCII exact), not embeddings or semantic retrieval. Respect `limit`
  and `recording_limit`; when `has_more` is true, continue with both returned
  `next_recording_offset` and `next_start_byte`. Keep the returned source IDs,
  excerpt, word/time ranges, and alignment/degradation alongside every citation.
  A match with unavailable timing must not be presented as a timed quote.
- For manually supplied research highlights use `educt_annotation_create`
  with `transcript_id`, inclusive immutable-source `start_word` / `end_word`,
  `label`, and `note`. List with `educt_annotation_list`; update with the same
  transcript ID plus `annotation_id` and all replacement range/label/note
  fields; delete with transcript ID plus annotation ID. Do not reuse corrected
  text indices as source indices. These annotations are `researcher_supplied`,
  not authenticated human identity or verified evidence; model suggestions stay
  distinct. Select the returned highlight layer explicitly when composing a reel
  from that annotation (`educt_edl_from_highlights`, `layer_id: annotation_id`).
- For asset-level notes/tags use `gallery_annotation_create` with stable
  `gallery_asset_id`, `kind` (`tag` or `note`), and `value`. List by asset ID;
  update with asset ID, `annotation_id`, and replacement `value`; delete with
  both IDs. Never substitute a positional gallery index for stable identity.
- For search/annotation-only work, report the saved IDs and exact returned
  citations, or surfaced missing timing/partial search; stop here. The reel
  convergence gate applies only when a reel is requested.

### Phase 1b — Bounded cross-check (long-form or platform-sourced assets)

For assets ≥20 minutes, operator-flagged as important, or platform videos
with a public transcript — the base transcript is one model's claim about
the audio; this step buys independent evidence at bounded cost. Skip it
for short informal recordings. Budget ~$0.02 per intake for the
   cross-check legs (≤3 slices; measured 2026-11-02: platform
   transcript fetch ~$0.004 + Nemotron slices $0.002/min ≈ $0.012 for
   3 slices); record per-leg receipts in a cost ledger and stop and
   report if any paid call exceeds its estimate.

1. Pick ≤3 contested or representative regions (the opening, a name-dense
   middle span, or spans the correction pass has flagged) — locate each
   in the working view before slicing (measured 2026-11-02: a span
   believed to be in the opening was at ~16:52). `audio_trim` each from
   the stored media (free, local).
2. Platform leg (YouTube sources): recover the video ID from the asset's
   `video_fetch` lineage (`source_url`) before paying for a search —
   the lineage makes an ID search unnecessary (measured 2026-11-02).
   Then `web_search` with `provider: serpapi` and the watch URL — the
   `youtube_video_transcript` engine returns the platform transcript as
   text (this fetch is the ~$0.004 paid call). Treat it as
   independent evidence, never ground truth:
   auto-captions are ASR output with their own proper-noun error modes.
3. Second-model leg: `transcribe_bundle` each slice WITHOUT storing it.
   Use the per-call `model` override (restart-free; measured 2026-11-02) —
   the settings switch (`kask.media.stt_model`, restart, `model_list`
   receipt, restore) is the fallback. Resolve each fleet model's current
   carrier from the provider's live catalog BEFORE firing: dated snapshot
   slugs retire, and a model in the chat catalog may not be served at the
   transcriptions endpoint (measured 2026-11-02: Qwen3-ASR withdrawn from
   OpenRouter — dated slug 404, omni models 400 at /audio/transcriptions —
   the Qwen family carried on DeepInfra/Qwen/Qwen3-ASR-1.7B, $0.0000075/s
   with word timings; Nemotron via DeepInfra $0.002/min, segments-only).
   Retry a failing leg ≤3 (a carrier or arm switch is the retry), then
   mark the leg failed, adjudicate on the remaining families, and report
   degraded coverage. Text-level output is what content adjudication
   needs; word timings are welcome but never assumed.
4. Compare each slice against the STORED bundle's region — never against
   a fresh slice of the base model (measured 2026-11-02: a whisper slice
   pass dropped a 5-second passage the stored full-file bundle contains).
   Record each leg's model family (intake STT, platform ASR, slice STT)
   and check independence before applying the corroboration rule —
   same-family agreement counts as single-source. Adjudicate per span:
   - ≥2 independent families agree on a reading → CORROBORATED:
     store the correction edit at SOURCE indices, provenance naming
     the sources.
   - Readings all differ but agree on the sound → CONTESTED: route to
     external verification (literature record; disclose its cost
     outside the leg bound). If the literature resolves the
     orthography, store the correction with the literature provenance
     (measured 2026-11-02: "Rosucci" /
     "rasuu Chi" / "Rusuchi" → Risucci, Tortolani & Ward 1989, Surg
     Gynecol Obstet 169(6):519-26, PMID 2814768 — the described study
     resolved the orthography where no two ASR strings matched);
     if not, keep the prior reading labeled unverified in the edit's
     reason.
   - Contested or single-source spans carry a researcher annotation
     recording the competing readings and state — never a silent text
     edit (measured 2026-11-02: "Dutty and Kruger" / "dirty Krueger" /
     "Dunning Kruger").
   - All agree → confirmed; no action.
5. Store adjudicated edits as a correction layer at source indices
   (cross-check provenance), realign, then probe the rendered view by
   lexical search — old form gone, new form present — before passing
   the unresolved list to the Phase 2 correction pass.

### Phase 2 — Correct and structure (the passes)

3. Call `educt_correction_pass` with the transcript id. It proposes
   word-range replacements for likely speech-to-text errors. Review the
   pass stats; then call `educt_apply_corrections` to get the corrected
   text view. The original words stay immutable — corrections are a
   derived view. If the alignment comes back `unaligned` (a correction
   changed word cardinality), call `educt_realign_transcript` to store
   the realignment layer — it re-anchors the corrected text onto the
   source timeline so the timed phases below (highlight pass, locate,
   SRT) work; without it they fail with the unaligned precondition. Verify timing and realignment readiness in `lisp_eval` (`TR-1`): `(and has_word_timings (> words_count 0) (if (string= alignment "unaligned") realignment_stored true))`.
4. Call `educt_paragraph_pass` for paragraph boundaries. Check the
   rejection_rate in the pass stats — a high rate means the model
   struggled; consider re-running after corrections.
5. If multiple speakers: call `educt_speaker_pass` (source "audio" is
   the primary, more accurate path; "text" works with every model).

### Phase 3 — Select and render

6. Call `educt_highlight_pass` with a natural-language request naming
   what matters, e.g. "the three key decisions and any commitments with
   dates". It returns word ranges with theme labels. If the selection
   misses the point, re-run with a sharper request — highlights are
   cheap to redo (layers, not edits).
7. Call `educt_edl_from_highlights` to compose the EDL (overlapping
   selections are union-merged automatically).
8. Call `educt_render_edl` to render the reel media file. Audio sources
   render as a PCM WAV cut on the sample (a PCM source keeps its bit depth);
   video renders as MP4, stream-copied where the cut falls on keyframes.

### Phase 4 — Verify and export

9. Verify before citing: any quote you plan to put in the summary must
   resolve with `educt_locate` (quote the rendered form exactly). A
   no_match means the quote is not in the transcript — fix the quote,
   never the transcript.
10. Export what the operator needs: `educt_export` with format "srt"
    (captions), "highlights_csv" (the highlight list), or "corpus_text"
    (for corpus ingestion — run corpus_convert → corpus_chunk →
    corpus_embed on the exported file).

### Convergence

11. Gate — call `lisp_eval` with (`TR-2`):
    - form: `(and (eq reel_exists 1) (> clip_count 0) (< rejection_rate 0.3) (= unlocated_summary_quotes 0))`
    - env: `{ "reel_exists": <1 if render_edl returned a media path>,
              "clip_count": <EDL keep-op count>,
              "rejection_rate": <paragraph pass rejection_rate>,
              "unlocated_summary_quotes": <count of summary quotes that failed educt_locate> }`
    If the reel is empty or the passes rejected heavily, re-enter at
    the failing phase (sharper highlight request, or corrections
    first) rather than delivering a broken reel. Bound: at most 2
    re-entries per failing phase; a third failure delivers the best
    available reel with the failure noted.

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-14 audit),
over the Convergence form:

- Green: `{reel_exists: 1, clip_count: 5, rejection_rate: 0.1}` → `true` —
  the reel rendered with clips and the passes accepted.
- Empty reel: `clip_count: 0` → `false` — a highlight selection with no
  keep-ops re-enters the highlight phase (a sharper request); in practice
  `educt_render_edl` rejects an empty clip plan outright, so this state
  fails at the first conjunct — the receipt pins the form's behavior
  regardless.
- Heavy rejection: `rejection_rate: 0.45` (the paragraph pass rejected
  heavily) → `false` — re-enter at Phase 2 (corrections first), never
  deliver a broken reel.

The skill's form is executed at use time, never anchored in code.

## Registry Templates

This skill owns no registry templates — the pipeline is the media server's
educt tool family plus the convergence form above.

## Constraints

- The stored base bundle is the only index and timing ground truth: every
  cross-check output lands as a correction layer at SOURCE indices with
  evidence provenance; slices and platform transcripts are evidence,
  never the base, and timings never change.
- Never edit source word timings — layers anchor to word indices; the
  immutable source transcript remains auditable. Re-anchored timings are
  derived estimates, not newly observed STT timing or confidence.
- `educt_locate` is the citation gate: no quote goes into a summary
  without a located word range.
- Local educt is self-contained: it never uploads to, transfers media to, or
  falls back to a cloud service. No cloud path is part of this skill.
- Face identification is opt-in; generic media curation never authorizes it.
- A zero-text OCR/transcription result is an error, never a success —
  surface it.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "transcript-reel", the tool name, and the error;
  continue with the best available information.
