# Continuation Prompt: Transcript Store + Selection Algebra — LLM-Maximal Design

Hand this prompt to the agent session that will implement local-mode gap 1 from
the video-analysis scaffold. It is self-contained: required reading, verified
findings, the design thesis, the investigation checklist, the implementation
slices, and the binding invariants. Everything stated as *verified* was checked
against the tree on 2026-08-30; everything else is a proposal for you to test.

---

## Required reading (before any design or edit)

1. `tasks/reduct-video-analysis-scaffold.md` — the research scaffold this
   continues. §3 gap 1 is your target; §2 defines the local/cloud duality your
   work sits inside (local mode is the primary; cloud escalation is out of
   scope for this continuation).
2. `tasks/reference-model-video-editor.md` — Reduct's transcript-as-EDL
   interaction model and the improvement targets (agent-as-selection-engine,
   subtractive editing). Your work makes §"Improvement targets" 2 and 3
   implementable.
3. `kask/docs/architecture/core/magna-carta.md` — name one invariant from it
   that constrains your first edit before you make that edit (project rule).
   The likely candidate: system types preserve semantic identity and are
   provenance-aware — which is exactly why the LLM layers you add must carry
   model + prompt provenance.

## Findings summary (verified — do not re-research)

**The gap.** `transcribe_bundle` returns a `TranscriptBundle`
(`kask/mcp-servers/hkask-mcp-media/src/transcript.rs`) whose components are:

```rust
TimedWord        { word: String, start_ms: u64, end_ms: u64, confidence: Option<Confidence> }
TranscriptSegment{ text: String, start_ms: u64, end_ms: u64 }
TranscriptBundle { format: "hkask-transcript-v1", audio_path, audio_duration_secs,
                   full_text, words: Vec<TimedWord>, segments: Vec<TranscriptSegment>,
                   language, model, repl_chat_ref }
```

But the bundle is returned to the conversation and dropped: no persistence, no
query surface, and no selection algebra — nothing maps "this text passage" to
"(start_ms, end_ms)" for `video_clip`. That mapping is Reduct's core move
(selecting text selects video) and it is pure index arithmetic over `words`.

**The LLM→JSON machinery that already exists:**

- `hkask-types/src/json_extract.rs` — `extract_json_from_response`:
  balanced-delimiter extraction of the first top-level JSON value (object or
  array), fence-stripping, reasoning-preamble-safe, security-documented
  (OWASP LLM02:2025). Array-correctness is pinned by tests. Its doc contract
  says: *callers must schema-validate the result*.
- The corpus tagging pipeline is the reference pattern for "LLM returns a
  typed structure from inside an MCP server":
  `kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs` imports
  `hkask_inference::model_constants::classifier_model`,
  `render_docproc_template`, `schemars::JsonSchema`, and `TaggedChunk` —
  template-rendered prompt → model call → JSON extraction → typed
  deserialization.
- Model resolution: `kask/crates/hkask-inference/src/model_constants.rs` —
  classifier `OpenRouter/z-ai/glm-5.2`, STT `DeepInfra/whisper-large-v3`
  (env `HKASK_MEDIA_STT_MODEL`), vision `OpenRouter/Qwen/Qwen3-VL-…`. Every
  model has an env override; never re-declare model literals at call sites.

**Two verified negatives that shape the design:**

1. There is **no `response_format` / `json_schema` passthrough anywhere in
   `kask/`** (zero grep matches; the only `ResponseFormat` in the repo is
   upstream Zed's deepseek client). Provider-enforced structured outputs would
   be new surface.
2. `MediaOp` (`kask/crates/hkask-inference/src/provider.rs:24`) has 8 variants —
   image/video/speech/transcribe — and **no text-generation or vision-chat
   variant**. Yet `video_caption` and `describe_image` reach a vision LLM
   somehow. That call path exists but is untraced; find it before adding a
   second one.

**Context from the scaffold (one paragraph).** Reduct is a transcript-as-
timeline platform (94.92% avg AI transcription accuracy, 90+ languages,
speaker identification, highlights/labels, strikethrough subtractive editing,
correction, paragraph breaks — all cited in the scaffold's capability matrix).
Its public API exists but is plan-gated and login-documented, so cloud mode is
deferred; local mode is the path, and this work is its foundation. The
features you are about to build map one-to-one onto Reduct features — use that
mapping as the product spec.

---

## The goal (target condition)

A persisted, queryable transcript layer over the media server where:

1. Every transcript is stored with its immutable word-timing ground truth.
2. LLM passes enrich stored transcripts with typed, validated, provenance-
   carrying layers: speaker labels, paragraph breaks, corrections, highlights,
   and edit-decision lists.
3. A deterministic selection algebra maps text/word-index selections to
   `(start_ms, end_ms)` ranges and EDLs to `video_clip`/`video_concat` plans.
4. The agent conversation can do, end to end and offline-of-Reduct: "find
   where he explains X, cut the filler, and make it a clip."

## The core design thesis: word-index anchoring

**LLM passes never emit timestamps. They emit indices into the `words` array.**
The deterministic layer owns the only index→time mapping. Consequences:

- Timestamp hallucination is structurally impossible — an LLM cannot invent a
  `start_ms` because the schema it returns has no time fields.
- Validation is cheap and total: index-in-range, ranges non-overlapping,
  monotonic, coverage — all checkable deterministically (and expressible as
  `lisp_eval` invariants if you want them model-checkable).
- LLM output is replayable: every layer is verifiable against the immutable
  `words` array it references.
- The types the LLMs return are small, flat, schema-friendly — ideal for
  structured outputs.

This is also the honest answer to "offload as much complexity as possible to
LLMs": offload *all semantics*, offload *zero time*.

## The LLM/deterministic boundary (offload map)

| Concern | Owner | Why |
|---|---|---|
| Word timings (`TimedWord`) | ASR (Whisper via STT provider) | Ground truth; already produced |
| Index→time mapping | Deterministic | Index arithmetic; an LLM here adds hallucination risk for zero capability |
| Text-match selection (exact passage → word range) | Deterministic | Substring resolution over `words`; ambiguity (multiple matches) must be surfaced, not guessed |
| Speaker labeling | **LLM pass** | Judgment over text cues + timing gaps (Reduct: "Renaming speakers") |
| Paragraph breaks | **LLM pass** | Discourse structure (Reduct: "Paragraph breaks") |
| Transcript correction | **LLM pass** | Proposes replacements anchored to word ranges (Reduct: "Correcting AI transcripts"); timings untouched |
| Highlight/label extraction ("find where he explains X") | **LLM pass** | Semantic selection — the agent-as-selection-engine improvement target |
| Filler/tangent classification (strikethrough) | **LLM pass** | Reduct's subtractive editing; classification is what LLMs are good at |
| EDL complement (cut-ranges → keep-ranges) | Deterministic | Set arithmetic |
| Render (`video_clip`, `video_concat`) | Deterministic (existing ffmpeg) | Already ships |
| Layer validation + storage + provenance | Deterministic | Trust boundary |

## The typed contracts (what the LLMs return)

All layers share a provenance envelope and anchor to word indices. Define them
as Rust structs deriving `Serialize, Deserialize, JsonSchema` in the media
server (sibling to `transcript.rs`), so the same type is: (a) the schema handed
to the model, (b) the validation target, (c) the stored record.

```rust
// Shared provenance — every LLM layer carries it (Magna Carta: types are
// provenance-aware).
LayerProvenance { model: String, prompt_template: String, created_at: String }

// 1. Speaker layer (diarization-lite; honest caveat: text-cue-based, approximate)
SpeakerLayer  { provenance: LayerProvenance,
                spans: Vec<{ start_word: usize, end_word: usize,
                             speaker: String, confidence: f64 }> }

// 2. Paragraph layer
ParagraphLayer{ provenance: LayerProvenance,
                breaks_after: Vec<usize> }   // word indices after which a break occurs

// 3. Correction layer — proposes text replacements, never timings
CorrectionLayer{ provenance: LayerProvenance,
                 edits: Vec<{ start_word: usize, end_word: usize,
                              replacement: String, reason: String }> }

// 4. Highlight layer (Reduct's highlights + labels)
HighlightLayer { provenance: LayerProvenance,
                 highlights: Vec<{ start_word: usize, end_word: usize,
                                   label: String, note: String }> }

// 5. EDL layer (the Reel) — op-level subtractive/additive plan
EdlLayer       { provenance: LayerProvenance,
                 ops: Vec<{ start_word: usize, end_word: usize,
                            op: Keep | Cut }> }
```

Per-layer invariants (deterministic validation, reject-with-named-reason):
`start_word <= end_word < words.len()`; spans sorted and non-overlapping
(merge or reject per layer semantics); `EdlLayer` ops tile the transcript or
explicitly mark the unopposed remainder; `CorrectionLayer` edits disjoint.
A layer that fails validation is **rejected with the failing invariant named**,
never partially applied.

## Mechanisms for getting typed JSON from LLMs (investigate, then choose)

**v1 — guaranteed-available (build this first).** The corpus pattern:
render a prompt (Jinja2 via the existing template machinery) that embeds the
`schemars`-derived JSON schema of the target layer + the relevant transcript
slice → model call → `extract_json_from_response` → schema-validate →
deserialize into the typed struct → run per-layer invariants → store with
provenance. Works with every model in the catalog today. The schema in the
prompt is a *contract*, not an enforcement — hence the hard validation gate
after parsing.

**v2 — provider-enforced (spike, don't block on it).** OpenRouter and DeepInfra
support `response_format: {type: "json_schema"}` on capable models, which makes
the provider enforce the schema. Verified negative: no `response_format`
passthrough exists in `kask/` today, so this is new inference surface.
Investigate: which catalog models advertise structured-output support; what
the passthrough would touch in the inference request path; whether strict-schema
providers choke on any schemars output (recall the repo rule: schemars renders
`serde_json::Value` as bare `true` — use `AnyJsonValue` for arbitrary JSON).
Adopt only if the spike shows it meaningfully reduces validation failures vs
v1; the validation gate stays either way.

**Validation layering.** Schema conformance (schemars) is necessary but not
sufficient — the domain invariants (index ranges, tiling, disjointness) are
where the real guarantees live. Follow the swarm precedent
(`hkask-mcp-swarm/src/schema_validate.rs`): unsupported schema keywords are
`UnsupportedSchema`, never a silent pass.

## Investigation checklist (verify before designing; cite file:line in the design doc)

1. **Trace the corpus LLM call path end-to-end**: from
   `hkask-mcp-corpus/src/tools/tagging/ops.rs` through the actual model
   invocation to `extract_json_from_response` and typed storage. This is the
   pattern you will replicate; know its error classification and its batching
   behavior (recall the `tag_batch_size` array-parsing trap).
2. **Trace how `video_caption` / `describe_image` reach the vision model
   today.** `MediaOp` has no chat/vision variant, so a separate call path
   exists. Reuse it (or the corpus path) for the transcript passes — do not
   invent a third.
3. **Decide the store location**: media-server-local (alongside the gallery
   index) vs corpus server (reuses embeddings/KNN for semantic search —
   scaffold open question 7). Weigh: server coupling, the recall-path JOIN
   trap (a stored layer whose transcript is gone is an orphan the recall path
   must not silently drop), and the embedding-provider cost caveat.
4. **Check STT segment production**: are `TranscriptSegment`s produced from
   provider `verbose_json` today, and are they word-aligned? If segments carry
   no word-index linkage, the LLM layers should still anchor to `words` only —
   segments become a derived view, not a second ground truth.
5. **Spike `response_format` passthrough** (v2 above) — timeboxed; report
   feasibility, do not land it unless v1 validation failure rates justify it.

## Implementation slices (TDD — one vertical slice at a time, red-green-refactor)

1. **Selection algebra (pure, no LLM, no storage).**
   `word_range → (start_ms, end_ms)`; `text selection → word range` (exact
   match over `words`, ambiguity surfaced as all candidate ranges); `EDL →
   keep-ranges` (complement); `keep-ranges → clip plan` (list of
   `(start_ms, end_ms)` for `video_clip`/`video_concat`). Property-style
   tests: round-trips, boundary indices, empty/degenerate EDLs.
2. **Transcript persistence.** Store/load a `TranscriptBundle` + layers with
   provenance; recall round-trip tests covering the JOIN (layer ↔ transcript ↔
   asset); degradation surfaced when `words` is empty (the existing documented
   case — "Empty if word-level timestamps not available") must produce a named
   unavailability status, never empty-success.
3. **First LLM pass: paragraphing** (lowest risk — no speaker inference, no
   text mutation). Full pipeline: schema-in-prompt → call → extract → validate
   → typed store. Measure validation failure rate; that number decides
   whether the v2 spike is worth it.
4. **Speaker pass, then correction pass.** Correction edits are proposals:
   applying one produces a new `full_text` view while `words` stays immutable
   (corrections carry the reason; Reduct's replace-all becomes a multi-edit
   proposal).
5. **Semantic selection → EDL → render.** `transcript_select`-style operation:
   natural-language request → `HighlightLayer` entries → user/agent composes
   `EdlLayer` → deterministic clip plan → existing `video_clip`/`video_concat`
   dispatch. This closes the loop on the reference-model doc's improvement
   target: Reduct's paradigm with the reading automated.
6. *(Optional)* v2 structured-outputs spike, only if slice 3's failure rate
   warrants it.

## Binding invariants and traps (from the project rules — these are not suggestions)

- Name an architecture doc + invariant before your first edit (session rule).
- `words` is immutable ground truth. Layers are additive, versioned, and never
  mutate it. Corrections produce derived views.
- Every LLM layer carries provenance (model, prompt, timestamp).
- Degradation is surfaced, never silent: empty `words`, unavailable model,
  failed validation — each returns a named status. A test asserting
  `count == 0` on a degraded path enforces the broken behavior as spec.
- MCP tests must construct the server with the capability under test; recall
  tests must cover the JOIN, not just the write.
- Anti-gaming (LLM-scored-target rule): ground every pass in the real `words`
  array; validate against external ground truth — e.g. round-trip checks
  (the transcript of a rendered clip must equal the selected passage) and
  spot-checks against human-verified samples for quality claims; preserve
  load-bearing properties (timings, coverage).
- No `unwrap()`; propagate errors with `?` or classify per-variant
  `McpToolError`s; arbitrary-JSON tool inputs use `AnyJsonValue`; tool
  responses use the `{"content": …}` envelope via `unwrap_tool_envelope`.
- `extract_json_from_response` array behavior is pinned by tests — if your
  layer type is a top-level array, the parser returns the full array; don't
  work around it, rely on it.
- No `mod.rs`; new crates declare `[lib] path`; build with `./script/clippy`;
  model names resolve via `hkask_inference::model_constants` + env overrides,
  never re-declared literals.
- Don't leave the tree half-broken between sessions — each slice compiles and
  tests green before you stop.

## Acceptance criteria (assessable)

1. Selection algebra: `word_range → time range` exact at word boundaries;
   text-selection ambiguity surfaces all candidates; EDL complement is exact.
2. Every LLM layer that validates is stored with provenance; every layer that
   fails is rejected with the named failing invariant and nothing persisted.
3. Round-trip: rendering an EDL and transcribing the result yields text
   contained in the selected passages (automated external ground truth).
4. Empty-`words` transcripts report a named degradation; no layer pass
   silently succeeds on them.
5. Recall round-trip: stored layers are retrievable by transcript and by
   asset; orphaned layers are surfaced, not dropped.
6. `./script/clippy` clean; all new tests green; the design doc cites
   file:line for every verified claim.

## Deliverables

1. `tasks/transcript-store-design.md` — the design doc: investigation
   findings (file:line-cited), the chosen store location with rationale, the
   final type contracts, the v1-vs-v2 mechanism decision with the measured
   validation failure rate from slice 3.
2. The slices above, each with tests, landed in
   `kask/mcp-servers/hkask-mcp-media/` (and `hkask-inference` only if the v2
   spike lands).
3. An update to `tasks/reduct-video-analysis-scaffold.md` §3 marking gap 1
   closed (or scoped down, with the residue named) and open question 7
   answered.