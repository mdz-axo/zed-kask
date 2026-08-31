# Video Analysis: Local + Cloud Duality — Research-Grounded Architecture Scaffold

Status: research scaffold — proposes, does not implement. No code changes.
Researched: 2026-08-30, bounded 4-source protocol (Reduct product surface → GitHub
org → API-existence gate → local-stack inventory), wrapped in a Toyota Improvement
Kata cycle. Companion to `tasks/reference-model-video-editor.md`, which captured
Reduct's *interaction model* (transcript-as-EDL); this document adds the *service
and capability layer* and the cloud/local duality that the interaction model plugs
into.

---

## Kata record (process discipline)

### 1. Current condition (grasped before research)

zed-kask already ships a substantial video-analysis surface in the media server
(`kask/mcp-servers/hkask-mcp-media/`), exposed to the agent conversation via the
media panel steer prompt (`crates/media_panel/src/media_panel.rs:231`):

- **Transcription**: `transcribe` (plain text), `transcribe_bundle`
  (TranscriptBundle with word-level `TimedWord` timings for click-to-seek;
  `src/transcript.rs:56`), `record_and_transcribe`, `audio_capture`
  (16 kHz mono WAV, Whisper-optimized; `src/video/ffmpeg.rs:361`).
- **Video processing** (local ffmpeg): `video_clip` (stream-copy `-c copy`),
  `video_concat`, `video_extract_frames` (keyframes → searchable gallery assets
  with lineage; `src/tools/processing.rs:786`), `video_caption` (keyframes +
  vision LLM), `video_info` (ffprobe), `video_to_gif`, `video_remix`,
  `video_add_caption`, `video_fetch` (yt-dlp).
- **Gallery indexing**: `gallery_organize` / `gallery_analyze` (faces, objects,
  colors, composition, scene descriptions — tags persisted and searchable),
  `gallery_search`, `gallery_find_similar`, `gallery_timeline`, face registry.
- **Persistence/async**: `workflow_save`/`workflow_load` (JSON graph documents —
  the EDL-persistence candidate named by the reference-model doc),
  `job_submit`/`job_status`.

The critical caveat: **the inference layer is provider-based and cloud-defaulted.**
`kask/crates/hkask-inference/src/model_constants.rs` — STT
`DeepInfra/whisper-large-v3` (env `HKASK_MEDIA_STT_MODEL`), vision
`OpenRouter/Qwen/Qwen3-VL-235B-A22B-Instruct`, OCR `RunPod/kask-ocr`. The only
local-model experiment on record (Ollama embeddings) was reverted for being
"impractically slow on CPU" (`model_constants.rs:33-35`). So "local mode" today
means *local media processing + configured inference providers*, not fully
offline AI.

### 2. Target condition

One scaffold document with every section grounded in a cited source, the
cloud/local duality made explicit, and the API-existence gate answered before any
cloud design.

### 3. Predictions (written before research) and gap measurement

| Prediction (before) | Evidence (after) | Gap = learning |
|---|---|---|
| Core capability: transcript-as-timeline, highlights/Reels, semantic search; enterprise/research users | Confirmed exactly (homepage) | Small — prior reference-model work had already anchored this |
| Partial public API docs exist | API **exists** (REST v3, "hundreds of endpoints") but **all endpoint documentation is behind account login**; only the object list is public (help center) + one endpoint shape via a third party (Pipedream) | I overestimated public documentation. The gate passes on *existence*, fails on *public verifiability* |
| GitHub org mostly private, maybe an SDK | 6 public repos: 3 dependency forks, a WordPress embed plugin, 2 status pages. **Zero API/SDK code** | Larger gap than expected — Reduct is not developer-surface-first; the org is a dead end for integration evidence |
| Seat/usage hybrid pricing | Confirmed: per-editor subscription + pooled annual transcription hours + per-minute overage | Small |
| (Not predicted) API is a **paid-tier feature** — Professional ($40/editor/mo) or Enterprise only | Pricing page + help center | New fact: cloud mode has a minimum monthly cost before any usage |
| (Not predicted) Reduct self-hosts its transcription/diarization/alignment models in GCP; "no language models from subprocessors are required" | Security page | Reframes the privacy comparison: cloud mode sends media to Reduct, but not to third-party LLM vendors |
| (Not predicted) Our own "local" mode is not offline | `model_constants.rs` defaults | The duality's local pole needs an honest definition or a local-STT workstream |

### 4. Section grounding scores (measured after research)

| Section | Score |
|---|---|
| 1. Reduct capability matrix | Evidence-grounded (every row cites a fetched source) |
| 2. Duality mapping | Evidence-grounded (swarm side from code/rules; Reduct side from fetched sources) |
| 3. Local mode design | Evidence-grounded inventory; Partially-grounded pipeline proposal (gaps named) |
| 4. Cloud mode design | **Partially-grounded** — API existence/auth/plan-gate verified; endpoint surface beyond the verified skeleton is Speculative |
| 5. Mode-selection seam | Evidence-grounded (existing consent/credential patterns cited) |
| 6. Open questions | Evidence-grounded (each names its resolving evidence) |

---

## 1. Reduct capability matrix

What Reduct actually does, per feature, each row citing the source it came from
(all Reduct sources fetched 2026-08-30).

| Capability | What it is | Source |
|---|---|---|
| Core positioning | "Ctrl+F for video" — transcribe hours of audio/video; find, annotate, edit, redact, share | reduct.video homepage |
| Transcription (AI) | 94.92% average accuracy across 6 audio types (standard interview 97.85%, bodycam 92.37%, news 98.04%, accented 87.64%, multi-speaker 94.58%, medical 99.04%); benchmark updated Jan 2025; speaker identification: yes | reduct.video/transcribe/benchmark |
| Transcription (human) | Overnight (within 24 hrs), "up to 99% accuracy", better on noise/crosstalk/accents, additional cost | reduct.video/pricing |
| Languages | 90+ languages | homepage; benchmark table |
| Silence-aware billing | Long silences detected and excluded from transcription billing (one public-defender client: ~36% of bodycam audio silent, uncharged) | reduct.video/pricing |
| Input formats | "mp4, mp3, mov, wav, aac, and beyond"; unlimited storage; per-file cap 4 GB (Personal) / 75 GB (Professional/Enterprise) | homepage; pricing |
| Interactive transcript | Click a word → playback jumps to that moment; selecting text selects video | homepage |
| Transcript correction | Select error → correct; replace-all across a recording | homepage |
| Search | Repository-wide: exact match via quotes + "NLP-powered fuzzy-search" for ideas/phrases; per-recording or across all | homepage |
| Highlights & labels | Highlight transcript passages, label/tag by theme, organize across recordings | homepage |
| Reels (highlight extraction) | Assemble highlights into captioned Reels; export 720p/2K/4K by tier | homepage; pricing |
| Strikethrough editing | In a Reel, select text → "cut" → passage skipped in playback (subtractive editing) | homepage |
| Redaction | PII redaction in video and audio (names, faces, screen-share content); Professional/Enterprise feature | homepage; pricing |
| Multicam / timeline | Sync footage from multiple sources, watch all angles at once (bodycam review); Professional/Enterprise | homepage; pricing |
| Live capture | Live transcription of Zoom/Meet/Teams calls with team highlighting; clips/reels available at call end | homepage |
| Import | Google Drive, Dropbox, Usertesting, Vimeo by link; Zoom cloud recordings via login | homepage |
| Documents | PDFs, Word, spreadsheets, presentations, images, text become searchable/highlightable; Enterprise | homepage |
| Sharing | "Link to selection" — shareable URL to an exact transcript moment; publish/unpublish reels; WordPress embed plugin | homepage; github.com/reduct-inc/reduct-wordpress-plugin |
| NLE round-trip | Premiere Pro extension (original high-res sources, multicam sequences); Final Cut Pro; DaVinci Resolve; time-coded media | homepage; help.reduct.video section 8 |
| Collaboration | Real-time (browser-synced), commenters (10 free on Personal, 50 on Professional), videoboard 2D canvas | homepage; pricing |
| AI summaries / Q&A | "Summarizer" product page; help-center articles "AI summaries", "Ask recording a question" | site footer; help.reduct.video nav |
| Translation | Product page + help-center article; security page: translation/LLM features use Google-provided models | site footer; security page |
| Target users | Public defense & legal (bodycam, interrogations, jail calls, 9-1-1), qualitative research, marketing, education, filmmaking | homepage |
| Security posture | SOC 2 Type II, GDPR (DPA), HIPAA (BAA, added cost), GCP hosting, TLS 1.2 in transit, 256-bit at rest, SSO (SAML/Google), enterprise RBAC; transcription/diarization/alignment models self-hosted in GCP; "No language models from subprocessors are required to use Reduct" | reduct.video/security |
| Pricing | Personal $12/editor/mo (120 pooled hrs/yr, 720p, 4 GB); Professional $40/editor/mo (300 pooled hrs/yr, 2K, 75 GB, redaction, multicam, **API access**); Enterprise from $75/editor/mo (4K, SSO, SOC2, DPA, MSA/SLA); per-minute overage; 14-day trial with all Professional features + 5 hrs AI transcription | reduct.video/pricing |
| Public API | REST, version 3, "hundreds of accessible endpoints"; objects: Projects, Recordings, Media, Redactions (+ redaction motion), Highlights, Comments, Reels, blocks/strikethroughs/comments within reels; retrieve transcripts + transcription statuses; upload/import media; publish/unpublish reels. **Professional/Enterprise plans only; full docs + API-key generation behind account login** | help.reduct.video/en/articles/api-access |
| API auth (third-party evidence) | API keys; example call `GET https://app.reduct.video/api/v3/project` with `x-auth-key: <api_key>` header | pipedream.com/apps/reduct-video |
| GitHub org | 6 public repos, none exposing an API/SDK: `pyflame` (C++ fork), `material-table` (JS fork), `filefy` (TS fork), `reduct-wordpress-plugin` (PHP), `reduct-status`, `dev-status` (Markdown). No public members | github.com/reduct-inc |

Dead ends recorded: `reduct.video/features` → 404 (feature content lives on the
homepage and per-feature pages); the Pipedream app page exposes only the single
test endpoint above, not an endpoint catalog; the GitHub org is a dead end for
integration evidence.

---

## 2. The duality mapping

The structural pattern this scaffold follows is the one the swarm system already
implements: ABW cloud workspaces vs local swarms in `agents/local/curated/`
(local: no consent gate, no per-use cost, runs offline; cloud: external
credentials, per-use cost, consent/capability gates). Mapping it onto video
analysis across the six dimensions:

| Dimension | ABW cloud swarm (existing) | Reduct cloud mode (proposed) | Local swarm (existing) | Local video mode (existing + proposed) |
|---|---|---|---|---|
| **Auth** | ABW API key in zed keychain (`kask://credentials/…`); missing key → `permission_denied` naming the env var (`hkask-mcp-swarm/src/abw_client.rs` `require_auth`) | Reduct API key (`x-auth-key` header) in the same single keychain namespace; **additionally gated by a Reduct Professional/Enterprise subscription** — the key cannot exist without a paid plan | None | None for media operations (ffmpeg/gallery are local). Inference providers (DeepInfra/OpenRouter/RunPod) carry their own existing keys |
| **Cost model** | Credits per delegation (`credits_authorized`, rJoule budgets) | Subscription floor ($40/editor/mo Professional) + pooled annual transcription hours + per-minute overage; silence excluded from billing | No per-use cost (compute only) | No per-use cost for ffmpeg/gallery; STT/vision billed by the configured inference providers |
| **Consent gate** | `swarm_request_consent` → single-use consent token required by `swarm_hire`/`swarm_delegate` | **Data Sovereignty Boundary** (Magna Carta P1/P2): uploading user video to Reduct's servers crosses the boundary → affirmative, fail-closed consent per upload operation, not just per key configuration | No consent gate | No consent gate for local ops; inference calls governed by existing provider consent |
| **Capability parity** | Catalogue agents vs local curated agents; push/pull sync tools keep them aligned | Asymmetric: cloud has redaction, human transcription (99%/24h), multicam sync, 90+ languages, live capture, collaboration, NLE round-trip. Local has transcription, frame extraction, captioning, clip/concat, gallery search — no redaction, no diarization, no human tier | Local agents are the replica baseline | Local is the baseline; gaps named in §3 |
| **Fallback behavior** | ABW unavailable → local swarm still runs (`swarm_pull_swarm_to_local` replicates) | Reduct unreachable/key missing → local mode serves the request; the response must **surface which mode served it** (degradation surfaced, never silent) | Always available | Always available for media ops; STT/vision fail if providers unreachable |
| **Offline behavior** | Local swarms run offline | Cloud mode impossible offline (by definition) | Fully offline | ffmpeg/gallery fully offline; **STT/vision are NOT offline today** — cloud-defaulted providers; a fully-offline variant requires a local STT/vision provider via `HKASK_MEDIA_STT_MODEL`-style overrides (architecture supports it; no default exists, and the local-embedding precedent warns about CPU speed) |

The one place the analogy bends: for swarms, local is a *replica* of the cloud
thing. For video analysis, local is the *primary* and cloud is an *escalation* —
Reduct's differentiators (human transcription, redaction, multicam) are exactly
the operations worth paying and consenting for. The seam should therefore
default local and treat cloud as opt-in per operation.

---

## 3. Local mode design

Pipeline architecture composing existing zed-kask media tools, with gaps named.
This is the Reduct paradigm from `tasks/reference-model-video-editor.md` made
concrete: `transcribe (word timestamps) → search/select text → (start, end) →
video_clip → concat queue = the Reel`.

```
 ingest                index                  analyze                 produce
 ───────              ──────                 ───────                 ───────
 video_fetch   ──►    transcribe_bundle ──►  agent selection over    video_clip
 (yt-dlp)            (word-level TimedWord)  transcript text         (stream-copy)
 local file           gallery_organize        (semantic search)      video_concat
                     gallery_analyze         video_caption          video_to_gif
                     (frames→tags, faces)    (vision LLM scene      video_add_caption
                     video_extract_frames     description)           workflow_save
                     (keyframes→assets)                             (EDL persistence)
```

**Exists today (no new code):**

1. **Ingest**: `video_fetch` (URL → local file + gallery index), local files
   directly.
2. **Transcript-as-timeline**: `transcribe_bundle` returns a TranscriptBundle
   whose `words: Vec<TimedWord>` carry word-level timings
   (`src/transcript.rs:56`) — the data structure Reduct's interactive
   transcript is built on. Click-to-seek is already a stated frontend purpose.
3. **Frame-level indexing**: `video_extract_frames` turns keyframes into
   gallery assets with lineage; `gallery_analyze` persists face/object/color/
   composition/scene tags; `gallery_search` / `gallery_find_similar` query
   them.
4. **Scene understanding**: `video_caption` (keyframes + vision LLM) and
   `describe_image`.
5. **Production**: `video_clip` (lossless stream-copy), `video_concat`,
   `video_to_gif`, `video_add_caption`, `video_remix`.
6. **EDL persistence**: `workflow_save`/`workflow_load` — a transcript-selection
   list or segment list is a workflow document (the reference-model doc already
   identified this).
7. **Async**: `job_submit`/`job_status` for long renders.

**Gaps requiring new code (each is a proposal, not implemented):**

1. **Transcript store + selection algebra.** Transcripts are currently returned
   to the conversation but not persisted as queryable objects keyed to the
   media asset. Reduct's core move — a text range IS a media range — needs a
   stored transcript with (asset, word, start, end) so "select the passage
   about X" maps to `(start, end)` for `video_clip`. Small schema, large
   leverage.
2. **Semantic search over the transcript repository.** Reduct's "NLP-powered
   fuzzy-search" across all recordings. Our corpus server already has the
   embedding + KNN machinery (`corpus_embed`/`corpus_query`); wiring
   transcripts into it (or a media-server-local embedding index) is the
   composition step. Cost caveat: embeddings default to DeepInfra.
3. **Speaker diarization.** Reduct has speaker identification (benchmark
   table); Whisper large-v3 transcription alone does not diarize. A local
   diarization path (e.g. pyannote-style) is a genuine new dependency.
4. **Video redaction.** We have face *detection* (`gallery_analyze`, face
   registry) and image *inpainting* (`image_edit_region`) but no time-varying
   face blur/pixelate in video. ffmpeg filter chains (boxblur/delogo with
   tracked regions) plus the existing face detection would be the local
   approximation of Reduct's redaction — the hardest local gap.
5. **Subtractive editing as an operation.** Strikethrough = transcribe →
   classify unwanted ranges → render the complement → concat. All primitives
   exist; the missing piece is the complement-render orchestration (the
   reference-model doc names this an agent-classifiable operation).
6. **Fully-offline STT/vision (optional hardening).** Override
   `HKASK_MEDIA_STT_MODEL` to a local provider. The architecture supports it
   (env → provider resolution), but no local default exists and the
   local-embedding precedent (`model_constants.rs:33`) warns about CPU
   throughput. Until then, "local mode" honestly means *no Reduct dependency*,
   not *no network*.

---

## 4. Cloud mode design

**API-gate status, stated first: the gate partially passes.** A public API
verifiably exists — REST, version 3, "hundreds of accessible endpoints", API-key
auth (`x-auth-key`), covering Projects, Recordings, Media, Redactions,
Highlights, Comments, Reels, transcripts/transcription statuses, media
upload/import, and reel publishing (help.reduct.video API Access article;
Pipedream integration). But **the full endpoint documentation is only reachable
behind a Reduct account login**, and API access requires a **paid
Professional/Enterprise plan**. Everything below the verified skeleton is
therefore marked Speculative, and no endpoint beyond
`GET https://app.reduct.video/api/v3/project` is invented here.

**Verified skeleton (Evidence-grounded):**

- Base URL: `https://app.reduct.video/api/v3/` (Pipedream example).
- Auth: API key via `x-auth-key` header (Pipedream: "Reduct.Video uses API keys
  for authentication"); keys generated in-app after login (help center).
- Object surface: Projects, Recordings, Media, Redactions (+ redaction motion),
  Highlights, Comments, Reels, blocks/strikethroughs/comments within reels;
  transcript retrieval + transcription status; media upload/import; reel
  publish/unpublish (help center).
- Plan gate: Professional ($40/editor/mo) or Enterprise (pricing page).
- Privacy posture for the consent copy: SOC 2 Type II, HIPAA BAA available,
  transcription models self-hosted in Reduct's GCP, no subprocessor LLMs
  required (security page) — the data still leaves the machine, which is what
  the sovereignty boundary governs.

**Proposed integration shape (Speculative beyond the skeleton):**

1. **Credential**: `REDUCT_API_KEY` stored under the single keychain namespace
   `kask://credentials/reduct_api_key` — key presence IS the availability
   toggle (no `*_enabled` setting, per the settings rules); writes call
   `nudge_mcp_servers` so a running server picks it up.
2. **Client surface** (new, in `kask/mcp-servers/hkask-mcp-media/` or a sibling
   leaf crate, following the `abw_client.rs` reference pattern): per-variant
   error classification; missing key → `permission_denied` naming
   `REDUCT_API_KEY`; `{"content": …}` envelopes via `unwrap_tool_envelope`;
   arbitrary-JSON inputs as `AnyJsonValue`.
3. **Operations worth escalating to cloud** (the parity asymmetry from §2):
   human transcription (99%/24h), redaction, multicam sync, 90+-language
   transcription at scale, repository search over a *team's* shared library.
   Each is an upload operation → each crosses the Data Sovereignty Boundary →
   each requires affirmative, fail-closed consent at call time, with the
   security-page posture quoted in the consent copy.
4. **What evidence would unlock the full design**: a Reduct account (the 14-day
   trial includes all Professional features — i.e. API access — plus 5 hrs AI
   transcription, per the pricing page). Login → capture the endpoint catalog,
   auth details, rate limits, upload constraints (does API upload honor the
   75 GB web cap?), pagination, and webhook/event surface. Until that account
   exists, endpoint-level design must not proceed — anything more detailed than
   the skeleton above would be fabrication.

---

## 5. Mode-selection seam

How a user picks or falls back between modes — analogous to local vs cloud
delegation (`swarm_delegate_local` vs `swarm_delegate`). Proposal:

1. **Default local, escalate explicitly.** Local mode serves every operation it
   can (privacy default, zero marginal cost). Cloud is chosen per operation,
   not per session — because the differentiators (human transcription,
   redaction) are occasional, priced, and consent-bearing.
2. **Resolution rule at the media-server boundary** (one function, not a
   heuristic scattered across tools):
   - Operation in local capability set → local.
   - Operation needs a cloud-only capability AND `REDUCT_API_KEY` present AND
     user grants upload consent → cloud.
   - Operation needs a cloud-only capability AND key absent →
     `permission_denied` naming `REDUCT_API_KEY` and the Reduct plan
     requirement — **never a silent local fallback that quietly returns a
     lesser result** (the operator must be able to distinguish "not
     configured" from "can't do this locally").
   - Cloud call fails transiently → error surfaces with classification; the
     user decides whether to retry cloud or accept local; the response states
     which mode produced it.
3. **Surfacing, not silence.** Every result names its mode (a `mode: local |
   cloud` field in the response envelope). Degradation tests must assert the
   degradation is *surfaced* — a test asserting an empty/degraded result equals
   success enforces the broken behavior as spec (test-protocol trap).
4. **Consent copy** for cloud uploads quotes the verified posture: SOC 2 Type
   II, HIPAA BAA on request, self-hosted transcription models, no subprocessor
   LLMs — and states plainly that the media leaves this machine and lands on
   Reduct's GCP infrastructure.

---

## 6. Open questions

Each with the evidence that would resolve it.

| # | Question | Resolving evidence |
|---|---|---|
| 1 | Full API endpoint catalog, rate limits, pagination, upload size caps for API (vs the 75 GB web cap), webhook/event surface | Log in to the gated API docs with a Professional-plan account (14-day trial includes Professional features) |
| 2 | Is API usage billed beyond the subscription (per-call, per-transcription-hour drawdown)? | The gated docs + a trial account's billing/usage page (`help.reduct.video` lists "Create itemized billing with usage metrics") |
| 3 | Does the API accept direct media upload, or only import-by-URL (Drive/Dropbox/Zoom style)? | Gated docs; the help article says "upload or import new media files" without specifying the API's mechanism |
| 4 | Is a fully-offline STT path viable at acceptable speed (local Whisper via provider override), given the CPU-slowness precedent for local embeddings? | Benchmark `HKASK_MEDIA_STT_MODEL` pointed at a local provider on target hardware |
| 5 | Local speaker diarization options and their cost | Evaluate a diarization model against `transcribe_bundle` output; Reduct's benchmark confirms diarization is table-stakes for this product class |
| 6 | Local video redaction feasibility (time-varying face blur from existing face detection + ffmpeg filters) | Prototype against `gallery_analyze` face boxes + `video/ffmpeg.rs` filter chains; compare to Reduct's redaction scope (names, faces, screen content) |
| 7 | Where should the transcript store live — media-server-local (alongside gallery index) or corpus-server (reusing embeddings/KNN)? | A design decision once Q4's cost profile is known; the corpus path reuses machinery but couples servers |
| 8 | Does Reduct offer an SLA'd enterprise API tier distinct from Professional's (the pricing page lists Enterprise SLAs but the help article groups both tiers)? | The gated docs or sales contact (`sales@reduct.video`) |

---

## Sources

Fetched 2026-08-30:

- Reduct homepage — https://www.reduct.video
- Reduct pricing — https://www.reduct.video/pricing
- Reduct security — https://www.reduct.video/security
- Reduct transcription benchmark — https://www.reduct.video/transcribe/benchmark
- Reduct help center, API Access — https://help.reduct.video/en/articles/api-access
- Reduct GitHub org — https://github.com/reduct-inc
- Pipedream Reduct.Video integration — https://pipedream.com/apps/reduct-video
- Dead ends: `reduct.video/features` (404); Pipedream page exposes no endpoint
  catalog; GitHub org has no API/SDK repos.

Local-stack evidence (read from the tree, 2026-08-30):

- `kask/mcp-servers/hkask-mcp-media/src/transcript.rs` (TranscriptBundle,
  word-level `TimedWord`)
- `kask/mcp-servers/hkask-mcp-media/src/tools/processing.rs:786`
  (`video_extract_frames`)
- `kask/mcp-servers/hkask-mcp-media/src/tools/audio.rs:140`
  (`transcribe_bundle`), `:227` (`audio_capture`)
- `kask/mcp-servers/hkask-mcp-media/src/video/ffmpeg.rs:361` (16 kHz mono
  capture), stream-copy trim (per `tasks/reference-model-video-editor.md`)
- `kask/crates/hkask-inference/src/model_constants.rs` (all model defaults and
  env overrides; local-embedding precedent at lines 33-35)
- `kask/crates/hkask-inference/src/media_providers.rs` (DeepInfra Whisper
  multipart STT; OpenRouter `/v1/audio/transcriptions` with word-level
  timestamps)
- `crates/media_panel/src/media_panel.rs:231` (steer prompt tool surface)
- `tasks/reference-model-video-editor.md` (prior Reduct interaction-model
  research, 2026-08-29)
- `kask/docs/architecture/core/magna-carta.md` (Data Sovereignty Boundary,
  fail-closed affirmative consent — the invariant governing §4 and §5)