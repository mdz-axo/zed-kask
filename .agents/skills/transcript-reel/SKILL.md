---
name: transcript-reel
description: "Distill a recording into a highlight reel over the media server's educt layer system: record and transcribe with word-level timings, run correction and speaker passes, select highlights semantically, compose and render an EDL, and export captions or corpus text. Reifies the capture-post-distribution discipline (MovieLabs OMC) over the educt transcript tools."
---

# Transcript Reel

Turn a meeting, interview, or lecture recording into a short highlight
reel with a corrected, searchable transcript. The educt tool family
stores an immutable word-level transcript and derives everything else
as validated layers (correction, paragraph, speaker, highlight, EDL) —
timings are never touched, so every reel is reproducible from its
layers.

## When to Use

- The operator wants "the key 2 minutes" from a long recording.
- A meeting/interview was recorded and needs a corrected transcript,
  speaker attribution, and a shareable clip.
- Building a searchable archive of recordings (transcripts feed corpus
  ingestion via educt_export).

## When NOT to Use

- Live transcription only, with no media file to keep — plain
  `record_and_transcribe` without storing is enough.
- The source is already text (a document) — use the corpus pipeline
  instead.

## Instructions

### Phase 1 — Capture and store

1. If recording now: call `record_and_transcribe` with the duration.
   It returns the audio path and a synchronized TranscriptBundle.
   If the media already exists: call `transcribe_bundle` with the
   audio/video URL.
2. Call `educt_store_transcript` with the bundle (and the gallery
   asset id if the media is indexed). If the summary notes missing
   word timings, stop — layers cannot anchor to a timing-free
   transcript; re-transcribe with `transcribe_bundle`.

### Phase 2 — Correct and structure (the passes)

3. Call `educt_correction_pass` with the transcript id. It proposes
   word-range replacements for likely speech-to-text errors. Review the
   pass stats; then call `educt_apply_corrections` to get the corrected
   text view. The original words stay immutable — corrections are a
   derived view.
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
   render losslessly via the audio path; video via stream copy.

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

11. Gate — call `lisp_eval` with:
    - form: `(and (eq reel_exists 1) (> clip_count 0) (< rejection_rate 0.3))`
    - env: `{ "reel_exists": <1 if render_edl returned a media path>,
              "clip_count": <EDL keep-op count>,
              "rejection_rate": <paragraph pass rejection_rate> }`
    If the reel is empty or the passes rejected heavily, re-enter at
    the failing phase (sharper highlight request, or corrections
    first) rather than shipping a broken reel.

## Constraints

- Never edit word timings — layers anchor to word indices; the
  immutable transcript is the ground truth every layer derives from.
- `educt_locate` is the citation gate: no quote goes into a summary
  without a located word range.
- A zero-text OCR/transcription result is an error, never a success —
  surface it.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "transcript-reel", the tool name, and the error;
  continue with the best available information.
