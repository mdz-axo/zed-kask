# Dual-Mode Video Analysis: Educt (Local) + Reduct (Cloud)

Status: research scaffold — architecture outline for a two-mode video analysis
capability: a sovereign local mode (Educt — our own Reduct-analog pipeline) and a cloud
mode (a Reduct API client), with bridge operations between them. Researched
2026-08-30. Companion to `tasks/reference-model-video-editor.md`, which
established Reduct as the interaction model (transcript-as-EDL); this document
covers the integration architecture that one deliberately deferred.

## Purpose

The directive this scaffold answers: a local video analysis mode analogous to
Reduct, plus a cloud video analysis mode that is an API connection to the
Reduct service — the same shape as ABW (cloud swarm) paired with the local
swarm capability.

Functional goal, in user terms: **analyze video locally by default** —
transcribe, search, highlight, assemble, ask questions — with zero credentials
and zero data egress; and **optionally connect a Reduct account** for their
hosted strengths (human transcription, redaction, published sharing, 90+
languages at scale), moving transcripts and highlights between the two worlds.

## Naming: Educt (local) / Reduct (cloud)

The local mode is **Educt** — *Reduct without the R*. The name is parallel by
construction: same `-duct` family, both real words (re-ducere "lead back",
e-ducere "draw out"), and it shows locality by construction — the cloud
service's initial is literally removed. It also names the role: an educt is
*that which is drawn out*, and video analysis is drawing meaning out of
footage. Tool prefixes make the duality legible at a glance: `educt_*`
(local-mode tools) vs `reduct_*` (cloud-API tools), one letter apart.

Alternatives considered: **Deduct** (names the subtractive/strikethrough
editing paradigm; the common word of the family) and **Induct** (in-process,
ingestion-first framing). Educt is the name — confirmed 2026-08-30 (the user delegated the
choice: consistency and structure matter, not the specific word).

## Companion documents (the reconciled plan)

This scaffold is the canonical, reconciled plan — it incorporates the
parallel agent's research and implementation design. The companion files
remain as records:

- `tasks/reduct-video-analysis-scaffold.md` — the parallel agent's research
  scaffold (capability matrix, cloud-skeleton evidence, mode-seam detail,
  eight open questions). Its verified cloud facts are folded into the
  Reduct API surface section below; its open questions are tracked in
  Decisions.
- `tasks/transcript-store-continuation-prompt.md` — the gap-1 implementation
  handoff (transcript store + selection algebra, LLM-maximal design). Its
  thesis and slices are folded into the Educt section below; its deliverable
  `tasks/transcript-store-design.md` will carry the investigation findings.
- `tasks/reference-model-video-editor.md` — the interaction-model anchor
  (Reduct as transcript-as-EDL paradigm).

Deliberate divergence from the companion scaffold: its per-upload
fail-closed consent gate is **not adopted** — decision 2 below (the user's
call: minimal, low-impedance; key presence is the consent). Its mode-seam
surfacing discipline (mode named in every response; never a silent local
fallback for cloud-only capabilities) **is** adopted, as the Mode-selection
seam section.

## The pattern being composed: ABW cloud/local, mapped to Reduct

The swarm server already solved this exact architectural shape. The mapping
below is the load-bearing artifact of this document — every cloud-mode design
decision inherits from a verified row.

| ABW cloud/local concept (verified in code) | Reduct integration counterpart (proposed) |
|---|---|
| `abw_client.rs` `SwarmClient` — thin reqwest seam isolating base URL, auth header, error mapping; "the panel, settings, and tool handlers never construct raw requests" | `reduct_client.rs` `ReductClient` — same seam discipline inside `hkask-mcp-media` |
| API key in keychain `kask://credentials/<key>`; presence IS the toggle; missing key = `permission_denied` naming the key | `kask://credentials/reduct_api_key` |
| `require_auth()` gate before any cloud call (`abw_client.rs:44-49`) | identical |
| Typed per-variant errors: `Auth` (401/403), `PaymentRequired` (402), `RateLimited` (429), `AgentNotFunded`, `Unavailable`; parse failures = `ApiVersionMismatch` (`abw_client.rs:74-106`) | `Auth`, `PlanGated` (API is Professional/Enterprise-only), `RateLimited`, `Unavailable` — per-variant, never blanket `internal` |
| `cloud_swarm_tools.rs` — "All 27 tools here talk to the ABW REST API; none touch the local registry or local ledger" | `reduct_tools.rs` — cloud tools touch only the Reduct API, never the local transcript/highlight stores |
| `local_tools.rs` — "All operate on the local registry/runtime; no ABW round-trips" | the existing media tools (`transcribe_bundle`, `video_*`, `gallery_*`) ARE the Educt mode's substrate — no new local tools needed for the core loop; new mode-specific tools take the `educt_` prefix |
| `swarm_request_consent` → single-use consent token for a credit spend | **not adopted** — minimal by design: the key's presence in the keychain is the consent; no tokens, no per-upload prompts |
| `wallet_balance()` algedonic signal riding tool responses (`abw_client.rs:160-190`) | transcription-minutes usage signal on Reduct tool responses (OUGHT — see gates) |
| `swarm_pull_swarm_to_local` / `swarm_push_local_swarm` bridge tools | `reduct_pull_transcript` / `reduct_push_highlights` |
| unauthenticated = catalogue-only mode (degraded cloud) | **inverted, deliberately**: unauthenticated = full local capability |

The last row is the one place we improve on the analogy rather than copy it.
In the swarm server, local mode was built as the analog of the cloud; the
cloud is the default frame. For video analysis the Magna Carta inverts the
default: **the local mode is the sovereign baseline that works with zero
credentials; the cloud mode is a pure opt-in extension** (Magna Carta
Principles 1–2: user sovereignty, affirmative consent / default deny). No
`*_enabled` settings toggle — the key's presence in the keychain is the only
credential toggle, and egress consent is per-operation, never a blanket grant.

## What Reduct is (grounded summary)

From the product site and prior research (`tasks/reference-model-video-editor.md` §5):

- **Core loop**: ingest (upload/URL import/live capture) → transcribe (AI
  immediate, human overnight at 99%) → interactive transcript (click word →
  jump to moment; selecting text selects video) → repository-wide fuzzy/NLP
  search ("Ctrl+F for video") → highlight + label (label groups) → reel
  (drag highlights into an EDL; strikethrough text = subtractive cut) →
  share/export (published reels, MP4, SRT/Word/PDF, FCP XML, Premiere).
- **Analysis surface**: AI summaries with clickable timestamps, ask-a-question
  with cited source jump, videoboard (2D canvas), speaker separation, 90+
  languages, redaction (PII names/faces/screen content), timeline/multicam.
- **Audience**: legal/public defense, qualitative research, filmmaking —
  i.e., exactly the sensitive-footage cases where local-first matters.

## The Reduct API surface (publicly documented)

From `help.reduct.video/en/articles/api-access` (fetched 2026-08-30):

- RESTful, **version 3**, "hundreds of accessible endpoints", more planned.
- **Objects** with create/edit/retrieve, org-wide or per-object: Projects,
  Recordings, Media, Redactions (+ redaction motion), Highlights, Comments,
  Reels, and blocks/strikethroughs/comments within reels.
- **Operations**: retrieve transcripts and transcription statuses, upload or
  import new media files, publish/unpublish reels; adjacent properties
  (recording languages, workspace members per project).
- **Plan gate**: Enterprise or Professional plans only.
- Full endpoint documentation and API-key generation are **behind account
  login**.

GitHub org `reduct-inc` (fetched 2026-08-30): 6 repositories — utility forks
(pyflame, material-table, filefy), `reduct-wordpress-plugin` (embeds Reduct
Share URLs), two status pages. **No public SDK, no API client library.** The
integration surface is the REST API alone.

**Verified skeleton** (from the companion scaffold's research — Pipedream
integration examples, pricing and security pages; folded in 2026-08-30):

- Base URL: `https://app.reduct.video/api/v3/`; one concrete endpoint known:
  `GET /api/v3/project`.
- Auth: API key via `x-auth-key` header; keys generated in-app after login.
- Plan gate: Professional ($40/editor/mo) or Enterprise; the 14-day trial
  includes Professional features — i.e., API access — plus 5 hrs AI
  transcription.
- Security posture (for upload transparency copy): SOC 2 Type II, HIPAA BAA
  available, transcription models self-hosted in Reduct's GCP, no
  subprocessor LLMs. The media still leaves the machine — which is what the
  keychain toggle governs.

**Remaining obstacle (typed, external):** the full endpoint catalog, rate
limits, pagination, upload caps, and billing mechanics are behind the
account-login docs. The client is designed against the *documented object
model* (above), not guessed paths; the endpoint-discovery probe suite
(account confirmed, 2026-08-30) pins the rest — the swarm server's
live-probe pattern (self-contained probes, `--test-threads=1`) — starting
from the known `GET /api/v3/project`.

## Educt — the local mode (the sovereign Reduct-analog)

### Already exists (verified this session)

| Capability | Reduct analog | Where (verified) |
|---|---|---|
| Word-level timed transcription | interactive transcript | `hkask-mcp-media/src/transcript.rs` — `TranscriptBundle` (`hkask-transcript-v1`), `TimedWord`, `word_at_ms()` click-to-seek; tools `transcribe` / `transcribe_bundle` |
| Keyframes + vision-LLM description | — (Reduct has no visual analysis; we exceed) | `video_caption` tool |
| Frame extraction into searchable gallery | — | `video_extract_frames` |
| Face/object/color/composition tagging | redaction-adjacent detection | `gallery_analyze`, `src/faces.rs` |
| Stream-copy clip + concat | reel render | `video_clip`, `video_concat` (`src/video/ffmpeg.rs`, `-c copy`) |
| URL import | web import | `video_fetch` |
| Async generation jobs | background transcription | `job_submit` / `job_status` (in-memory by design — `src/jobs.rs:1-6` names the split: ephemeral jobs, persistent gallery lineage) |
| Persistent workflow documents | project persistence | `workflow_save` / `workflow_load` |
| Albums, tags, lineage | label groups | `gallery_*` tools |
| Record + transcribe in one call | live capture | `record_and_transcribe` |
| Repository-wide semantic search + cited QA | "Ctrl+F for video", ask-a-question | `hkask-mcp-corpus` — `corpus_convert` → `corpus_chunk` → `corpus_embed` → `corpus_query` (with `generate_answer`) — the pipeline exists, transcripts don't flow through it yet |

### Gaps (verified absent by grep — no `highlight`/`edl`/`reduct` hits in media src)

1. **Transcript persistence** — bundles are per-call artifacts; there is no
   stored transcript repository per gallery asset.
2. **Repository-wide transcript search** — the corpus pipeline exists but no
   transcript-to-corpus path; "find where he talks about X" across all
   recordings is not yet one operation.
3. **Highlights/labels store** — no `(asset, start_ms, end_ms, text, labels)`
   record type; gallery albums group whole assets, not moments.
4. **EDL/reel document** — `workflow_save` can hold a serialized graph, but
   there is no typed EDL (sequence of `(asset, in_ms, out_ms)` triples — the
   MLT playlist lesson from the reference doc).
5. **Subtractive editing** — classify unwanted passages → render the
   complement (reference doc improvement target #3; the agent is the
   classification engine).
6. **Speaker separation** — the STT backend yields words, not speakers.
   Resolution (decided 2026-08-30): speaker-attributed transcription via
   audio-capable local LLMs through the inference port — the capability is
   embedded in the model, not a separate diarization pipeline. A dedicated
   diarization model is the fallback only if LLM attribution proves
   insufficient.
7. **Exports** — SRT/Word/CSV are trivial projections of `TranscriptBundle`
   and a highlights store; absent today.
8. **Video redaction** (from the companion scaffold) — face *detection*
   exists (`gallery_analyze`, `src/faces.rs`) and image *inpainting* exists
   (`image_edit_region`), but there is no time-varying face blur/pixelate
   in video. ffmpeg filter chains (boxblur/delogo with tracked regions)
   over detected face boxes would be the local approximation of Reduct's
   redaction — the hardest local gap; until closed, redaction is a
   cloud-escalation operation.

*Honesty note* (from the companion scaffold): "local" means **no Reduct
dependency**, not *no network* — STT/vision ride cloud-defaulted inference
providers today. A fully-offline variant is optional hardening via provider
overrides (`HKASK_MEDIA_STT_MODEL`-style); no local default exists.

### Build order (TDD slices, reconciled with the continuation prompt)

1. **Selection algebra** (pure, no LLM, no storage): `word_range →
   (start_ms, end_ms)`; text selection → word range (ambiguity surfaces
   all candidates, never a guess); EDL → keep-ranges (complement);
   keep-ranges → clip plan for `video_clip`/`video_concat`. Property
   tests: round-trips, boundary indices, empty/degenerate EDLs.
   **Landed 2026-08-30** — `src/transcript_select.rs` (29 tests green);
   design record: `tasks/transcript-store-design.md`.
2. **Transcript persistence**: store/load `TranscriptBundle` + layers with
   provenance; recall round-trips cover the JOIN (layer ↔ transcript ↔
   asset); empty-`words` transcripts report a named degradation, never
   empty-success.
   **Landed 2026-08-30** — `src/transcript_layers.rs` (typed layer
   contracts + validation) + `src/transcript_store.rs` (SQLite tables,
   cascade delete, orphan surfacing) + six `educt_*` tools; tool surface
   68 → 74; 145 crate tests green.
3. **First LLM pass — paragraphing** (lowest risk: no speaker inference,
   no text mutation): the full v1 pipeline below; measure the validation
   failure rate — that number decides the v2 spike.
4. **Speaker pass, then correction pass**: speaker labels from
   audio-capable local LLMs (decision 3) with the text-cue pass as
   fallback; corrections are proposals over word ranges — `words` stays
   immutable, corrected `full_text` is a derived view.
5. **Semantic selection → EDL → render**: natural-language request →
   `HighlightLayer` entries → `EdlLayer` → deterministic clip plan →
   existing `video_clip`/`video_concat`. Closes the agent-as-selection-
   engine loop (reference doc improvement target 2).
6. *(Optional)* v2 structured-outputs spike, only if slice 3's measured
   failure rate warrants it.
7. **Exports** (SRT from `TimedWord`, CSV highlights) and **repository
   search wiring** (corpus composition, per decision 8).
8. **Redaction** (gap 8, hardest): time-varying face blur from existing
   face detection + ffmpeg filter chains; until then, cloud escalation.

### The gap-1 design: word-index anchoring (incorporated from the continuation prompt)

The transcript store + selection algebra (gaps 1, 3, 4, 5) follow one
binding thesis: **LLM passes never emit timestamps — they emit indices
into the immutable `words` array; the deterministic layer owns the only
index→time mapping.** Timestamp hallucination is structurally impossible
(the schema the models return has no time fields); validation is cheap and
total (index-in-range, non-overlapping, monotonic, tiling — all checkable
deterministically); every layer is replayable against the `words` it
references. Offload all semantics, offload zero time.

The LLM/deterministic boundary:

| Concern | Owner |
|---|---|
| Word timings (`TimedWord`) | ASR via the STT provider — ground truth, already produced |
| Index→time mapping, text-match selection, EDL complement, render | Deterministic (index/set arithmetic; an LLM here adds hallucination risk for zero capability) |
| Speaker labeling, paragraph breaks, corrections, highlight extraction, filler/tangent classification | **LLM passes** — typed, validated, provenance-carrying |
| Layer validation + storage + provenance | Deterministic (the trust boundary) |

Typed layer contracts (full Rust definitions in the continuation prompt;
the design doc finalizes them): `SpeakerLayer`, `ParagraphLayer`,
`CorrectionLayer`, `HighlightLayer`, `EdlLayer` — every layer carries
`LayerProvenance { model, prompt_template, created_at }`, anchors to word
indices, and must pass per-layer deterministic invariants (ranges in
bounds, spans sorted and non-overlapping, EDL ops tile the transcript or
explicitly mark the remainder, corrections disjoint). A layer that fails
validation is **rejected with the named failing invariant** — never
partially applied.

Mechanism: **v1 first** — schemars-derived schema in the prompt → model
call → `extract_json_from_response` (`hkask-types/src/json_extract.rs`;
array-correctness pinned by tests) → schema-validate → typed
deserialize → invariant check → store with provenance. Works with every
catalog model today; the schema in the prompt is a contract, the hard
validation gate after parsing is the enforcement. **v2 is a timeboxed
spike only**: provider-enforced `response_format: json_schema` passthrough
(verified absent in `kask/` — zero grep hits, re-verified 2026-08-30) is
adopted only if slice 3's measured failure rate justifies it; the
validation gate stays either way.

## Cloud mode — the Reduct seam

### Client (`src/reduct_client.rs`, following `abw_client.rs` discipline)

- Config: base URL + API key resolved from `kask://credentials/reduct_api_key`.
- `is_authenticated()` / `require_auth()` → missing key returns
  `permission_denied` **naming the key** (canonical pattern:
  `abw_client.rs:44-49`; a missing credential is an authorization failure,
  never `unavailable` or a silent fallback).
- `send()` maps status AND body to typed variants: 401/403 → `Auth` (or
  `PlanGated` when the body says so), 402 → `PaymentRequired` (plan gate),
  429 → `RateLimited`, parse failure → `ApiVersionMismatch`, else
  `Unavailable`. No silent fallbacks; a failed measurement is never a
  measured zero.
- Tool inputs accepting arbitrary JSON use `AnyJsonValue`, not
  `serde_json::Value` (schemars renders `Value` as bare `true`).

### Tools (`src/reduct_tools.rs` — zero local-store touch, the `cloud_swarm_tools.rs` rule)

Read: `reduct_list_projects`, `reduct_list_recordings`,
`reduct_get_transcript` (+ transcription status), `reduct_get_highlights`,
`reduct_get_reel`.
Write: `reduct_upload_media` (its description states the media is
transferred to Reduct — transparency, no friction), `reduct_create_highlight`,
`reduct_create_reel` (blocks/strikethroughs), `reduct_publish_reel`,
`reduct_redact`.
Each maps 1:1 to a documented API object; nothing is invented beyond the
object model above.

### Gates (minimal by design — decided 2026-08-30)

Low impedance is the requirement: no consent tokens, no per-operation
confirmation prompts. The key's presence in the keychain IS the affirmative
consent — the platform's single-toggle credential pattern, satisfied by the
deliberate act of putting the key there.

1. **Credential gate.** Key presence in the keychain is the only toggle (no
   `*_enabled` setting). Missing key → `permission_denied` naming
   `reduct_api_key`. The key is added to the media server's env/credential
   allowlist, aligned with the actual env read. Settings-UI writes call
   `nudge_mcp_servers` so the running server picks up the key.
2. **Egress transparency, not egress friction.** Upload tools state plainly
   in their description that the media is transferred to Reduct; beyond
   that, the flow is unimpeded.
3. **Plan gate surfaced.** The API is Professional/Enterprise-only;
   `PaymentRequired`/`PlanGated` errors name the plan requirement rather
   than collapsing to `unavailable` (the operator must be able to
   distinguish "not configured" from "configured but plan-gated").
4. **Usage signal (OUGHT).** Reduct bills by transcription time; attach a
   usage reading to tool responses the way `wallet_balance()` rides ABW
   responses (`abw_client.rs:160-190`) — an algedonic sense input, not a
   decoration. Not enforceable until the probe suite discovers the
   usage endpoint.

## Mode-selection seam

How a request resolves between Educt and Reduct (from the companion
scaffold's mode-seam section, consent mechanics simplified per decision 2):

1. **Default local, escalate explicitly.** Educt serves every operation it
   can (privacy default, zero marginal cost). Cloud is chosen per
   operation, not per session — the differentiators (human transcription,
   redaction, multicam, 90+ languages at scale, a team's shared library)
   are occasional and priced.
2. **One resolution function at the media-server boundary**, not a
   heuristic scattered across tools:
   - Operation in the local capability set → local.
   - Operation needs a cloud-only capability AND the key is present →
     cloud.
   - Operation needs a cloud-only capability AND the key is absent →
     `permission_denied` naming `reduct_api_key` and the plan requirement
     — never a silent local fallback that quietly returns a lesser result
     (the operator must distinguish "not configured" from "can't do this
     locally").
   - Cloud call fails transiently → the classified error surfaces; the
     user decides retry-cloud vs accept-local.
3. **Every response names its mode** (`mode: local | cloud` in the
   envelope). Degradation tests assert the mode is surfaced — a test
   asserting an empty/degraded result equals success enforces the broken
   behavior as spec.

## Bridge (the pull/push analog)

- `reduct_pull_transcript` — Reduct recording → local `TranscriptBundle`
  (normalized to `hkask-transcript-v1`), stored in the L1 transcript store.
  The `swarm_pull_swarm_to_local` analog: cloud → local, sovereign copy.
- `reduct_push_highlights` — local highlights → Reduct highlights. The
  `swarm_push_local_swarm` analog: local → cloud.
- Round-trip story: local rough cut (L3 EDL) → `reduct_create_reel` →
  `reduct_publish_reel` for hosted sharing with Reduct's interactive
  transcript player — rough cut sovereign, publication optional.

## Divergence surface

All new code lands in `kask/mcp-servers/hkask-mcp-media/` (new modules
`reduct_client.rs`, `reduct_tools.rs`, plus L1/L2 store modules). No upstream
Zed files are touched — no D-seam entries required. New tools register in the
media server's router; the Reduct key joins that server's existing credential
allowlist.

## Decisions (recorded 2026-08-30)

1. **Naming**: the local mode is **Educt** — *Reduct without the R* (see
   Naming above). The mode split is legible via tool prefixes: `educt_*`
   local, `reduct_*` cloud. Alternatives considered: Deduct, Induct.
   **Confirmed 2026-08-30** — the user delegated the choice (consistency and
   structure matter, not the specific word); Educt stands.
2. **Gates**: minimal — no consent tokens, no per-upload confirmations;
   key presence in the keychain is the consent. Low-impedance flows.
   (Supersedes the companion scaffold's per-upload consent gate.)
3. **Speaker separation**: local LLM audio models via the inference port
   (the capability is embedded in the model); a dedicated diarization
   model only as fallback.
4. **Probe access**: confirmed — a Reduct account with API access exists.
   First cloud-mode step: the endpoint-discovery probe suite.
5. **Word-index anchoring** (incorporated from the continuation prompt):
   LLM layers emit word indices, never timestamps; the deterministic layer
   owns index→time. Structurally prevents timestamp hallucination;
   validation is total and replayable.
6. **Layer contracts**: Speaker/Paragraph/Correction/Highlight/EDL layers —
   provenance envelopes, word-index anchors, per-layer deterministic
   invariants, reject-with-named-invariant, never partial application.
7. **Speaker sources ranked** (reconciles decision 3 with the
   continuation prompt's text-cue `SpeakerLayer`): audio-capable local
   LLM (primary) → text-cue LLM pass (works with any model, approximate,
   honest about it) → dedicated diarization model (fallback). All three
   produce the same `SpeakerLayer` record; provenance distinguishes.
8. **Store location** (resolves the companion scaffold's open question 7
   and the continuation prompt's investigation item 3): hybrid —
   media-server-local for the bundle + layers (ground truth and typed
   records live together, eliminating the orphan-JOIN trap), corpus as a
   derived, rebuildable search index. Confirmable by the design doc's
   investigation.
9. **EDL representation**: word-index anchored ops (`Keep | Cut` over word
   ranges) per the continuation prompt; time-ranges are the derived
   projection the render consumes. (Supersedes this scaffold's earlier
   `(asset, in_ms, out_ms)` triple sketch.)
10. **LLM→JSON mechanism**: v1 schema-in-prompt + `extract_json_from_response`
    + hard validation gate, first; v2 `response_format` passthrough only
    as a timeboxed spike if measured failure rates warrant (verified
    absent in `kask/` today).

**Open items** — the companion scaffold's
questions that resolve by probe or benchmark: API billing beyond the
subscription (its Q2), direct upload vs import-by-URL (Q3), offline STT
speed benchmark (Q4), local redaction feasibility (Q6), enterprise-tier
distinctness (Q8). Its Q1 (endpoint catalog) resolves via the probe suite;
Q5 (speaker) and Q7 (store) are decided above.

## Sources

- Reduct product site (fetched 2026-08-30): https://reduct.video/product/
- Reduct API Access (fetched 2026-08-30):
  https://help.reduct.video/en/articles/api-access
- reduct-inc GitHub org (fetched 2026-08-30): https://github.com/reduct-inc
- Prior session: `tasks/reference-model-video-editor.md` (Reduct §5, gap
  table, improvement targets)
- Verified in code this session: `kask/mcp-servers/hkask-mcp-swarm/src/abw_client.rs`,
  `.../cloud_swarm_tools.rs`, `.../local_tools.rs`;
  `kask/mcp-servers/hkask-mcp-media/src/transcript.rs`, `src/jobs.rs`
- Invariants: `kask/docs/architecture/core/magna-carta.md` (Principles 1–2,
  IS/OUGHT discipline)
- Companion scaffold (incorporated 2026-08-30):
  `tasks/reduct-video-analysis-scaffold.md` — cloud skeleton via Pipedream
  integration examples, Reduct pricing and security pages; mode-seam
  design; capability matrix
- Continuation prompt (incorporated 2026-08-30):
  `tasks/transcript-store-continuation-prompt.md` — word-index anchoring,
  layer contracts, offload map, TDD slices
- Re-verified in code this session (2026-08-30): `MediaOp` has exactly 8
  variants, no text-generation/vision-chat op
  (`kask/crates/hkask-inference/src/provider.rs:24-33`); `response_format`
  has zero hits in `kask/`; corpus tagging reference pattern exists at
  `kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs`
