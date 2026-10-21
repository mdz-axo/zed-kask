# hkask-inference

Multi-provider inference router for hKask — OpenRouter, Ollama, RunPod.

## Entry points

- **Inference ports** — `resolve_inference_port` and `InferenceIpcClient` connect callers to the configured host; the explicit direct-path policy is described below.
- **Provider configuration** — `InferenceConfig`, `ProviderId` and `model_constants` resolve visible provider/model choices; they are not a study budget or evidence-retention contract.
- **Offline packet-study instrument** — `packet-study` prepares evidence-conditioned judgment requests, replays digest-bound responses, and audits durable reservation journals. It does not dispatch live inference.

## Offline packet-study instrument

Study A compares completion judgments over supplied evidence, not autonomous tool choice or code repair. The instrument lives in this standalone crate: it adds no editor, registry-execution or generic agent framework dependency.

```bash
cargo run -p hkask-inference --bin packet-study -- verify --output /absolute/new-directory
cargo run -p hkask-inference --bin packet-study -- audit --output /absolute/new-directory
```

`verify` runs deterministic contract controls and scripted responses through the real preparation, parser, reservation, persistence and paired-report path. It creates a private `.replay-inputs` sidecar next to the requested output for its scripted source bodies; both locations must be new. Every output is labeled `instrument_verification_replay`: the displayed model name and usage/cost arithmetic are fixture declarations, not model observations or provider charges. Controls are not a pilot dataset or heldout scientific evidence.

The resulting private directory contains `protocol.json`, `private-cases.json`, `replay-records.json`, per-attempt request/raw-response/receipt files, an fsynced hash-chained `budget.jsonl`, and `report.json`. The output directory must be new; files are never overwritten or automatically resumed. On Unix the directory/files are created with 0700/0600 permissions. Hash consistency is not producer authentication.

```bash
cargo run -p hkask-inference --bin packet-study -- prepare \
  --protocol protocol.json --cases cases.json --output /absolute/new-preparation
cargo run -p hkask-inference --bin packet-study -- replay \
  --protocol protocol.json --cases cases.json --responses replay-records.json \
  --output /absolute/new-replay
```

Use the verification output as a documented schema example. Protocol, case and replay inputs are bounded JSON files. The replay index contains metadata and relative `response_file` basenames with `response_sha256`, not inline bodies. Body files are opened only after admission and synced reservation/request persistence; the retained index contains only acquired responses. Duplicate IDs/digests and invalid case/checker classifications fail. Enum values must be strings; object-shaped encodings such as a `verdict` of `{"complete":null}` are rejected rather than scored. Request fingerprints bind protocol, attempt, stage and exact body, so identical bodies from different attempts cannot reuse one receipt. Initial model history is shared across methods; final requests contain only their own branch's review. Hidden contract truth and primary-checker health labels do not enter request bodies. Weak checker receipts come from actually executed deterministic predicates, not copied oracle labels.

Reference contract checks cover JSON-container preservation, checked integer sums and constrained readiness claims. They share a trusted JSON parsing kernel; reference agreement and smoke controls are not independent scientific validation. Malformed output is not abstention. Missing primary-pair members remain bounded unknowns, rather than disappearing from the denominator; reported bounds are not confidence intervals or a treatment conclusion.

Reservation arithmetic uses checked integer micro-USD, not floating-point balances. Every accepted reservation is synced before a replay response is accessed. Missing/partial usage, cap violations and failed receipts retain liability and stop further admission; reasoning tokens are a subset of completion tokens and are not charged twice. The journal root binds the canonical protocol digest, so substituting cheaper tariffs or different limits is rejected. Audit reconstructs the validated complete prefix after an interrupted write: a torn tail is surfaced as degraded/incomplete with known outstanding liability, while a corrupt complete entry remains an error. Neither result authorizes a retry or automatic resume. Replay input-token bounds are explicit fixture declarations, **not validated live token measurements**.

**Live execution is unavailable.** No supported CLI mode resolves provider credentials or invokes an inference provider. An `execute` request fails visibly. The selected service's tokenizer is not established by its catalog label `Other`; byte limits are storage limits, not an 8,192-token proof. The [Fugu Max model page](https://openrouter.ai/sakana/fugu-max) describes learned multi-agent orchestration and bills orchestration tokens as input/output. Consequently a single fixed-model interpretation and the draft's simple per-request cost envelope need explicit revision or provider evidence; the earlier conditional $6 proposal is not a live spending guarantee. A bounded live transport, validated model-specific input counting, provider cap/route evidence, independent study-case calibration and a separate PI execution approval remain required before any paid pilot. Do not turn offline verification into scientific capability acceptance.

## QA transport contracts

- QA callers resolve `model_constants::resolve_qa_generation_model` before
  dispatch: explicit tool model > `HKASK_QA_GENERATION_MODEL` (host setting
  `kask.models.qa_generation_model`), otherwise a visible configuration error.
  The setting defaults empty. Malformed values fail rather than falling through;
  registry/provider lookup checks availability. Chat/default and training base
  models are never inputs to this QA resolver. Non-QA inference retains its
  own defaults below.
- QA disables thinking: synchronous direct HTTP and bridge OpenRouter requests
  serialize `reasoning.effort: "none"`. The provider-batch formatter does too
  (its sole production caller is corpus QA); `exclude` is not a disable flag.
- `LazyInferencePort` tries IPC first. If the bridge cannot be reached, direct
  chat uses the explicit model or `HKASK_DEFAULT_MODEL`; an unset default is
  `NotConfigured`, not a hidden replacement model. A response error from a
  connected bridge is returned, not retried through another provider.
- Direct chat preserves the supplied `ChatMessage` array (roles, order, and
  content). String prompts delegate to that same HTTP implementation as one
  user message. Direct tool-definition forwarding remains unsupported.
- Raw provider batch output is normalized by `batch::parse_batch_results`:
  duplicate `custom_id`s fail permanently, and a response accompanied by an
  error fails as malformed. Provider errors and no-choice failures remain
  errors. Counts reflect unique terminal map entries, not JSONL line counts.
  The map contains only returned identities; callers still detect missing or
  unsolicited IDs against their original prompts.
- The bridge maps normalized failures to `BatchResultEntry { text: None,
  error: Some(...), total_tokens: 0, .. }`. Corpus QA treats these as failed
  prompts, never successful QA rows. Parser/IPC-contract tests live in
  `src/batch.rs`; corpus consumption is pinned by
  `batch_response_failures_have_truthful_totals`. Offline HTTP capture tests
  in `src/hkask_inference.rs` exercise both direct and lazy fallback paths in
  an env-isolated subprocess with a loopback proxy, without live providers.

## Configuration

| Variable | Description |
|----------|-------------|
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `RUNPOD_API_KEY` | RunPod API key (endpoint discovery and OCR inference) |

| `HKASK_DEFAULT_MODEL` | Default model for non-QA callers (e.g., `OR/z-ai/glm-5.2`) |
| `HKASK_QA_GENERATION_MODEL` | Required dedicated QA generator unless explicitly supplied; full `Provider/model-id`, no default |
| `HKASK_DEFAULT_PROVIDER` | Default provider code (OR, OM, RP; default: OR) |

## Media routing policy — operator decision 2026-09-06

The selected model is authoritative: `params.model` when present, otherwise
that operation's configured environment model. Selectable operations require
`OpenRouter/<provider-local-model>` or `DeepInfra/<provider-local-model>`.
Full provider names are ASCII case-insensitive; the remainder may contain a
vendor slash and is preserved exactly. Bare models, short aliases (`OR/`,
`DI/`), blank overrides, and unknown providers are invalid.

**URL-safety clarification (2026-09-06):** provider-local model components
must be nonempty and cannot be `.` or `..`. Whitespace, control characters,
`?`, `#`, backslash, and `%` (including URL escapes) are rejected as `Model`
before any request, for explicit and env-selected models alike. Ordinary
dots, hyphens, underscores, version colons, and Unicode names remain valid.
The registry and direct adapters share this validation; native DeepInfra
URLs append model identifiers as URL path segments, preserving the
configured host/base path instead of parsing model text as URL syntax.

| Operation | Environment model when no override is supplied |
|---|---|
| `generate_image`, `image_to_image` | `HKASK_MEDIA_IMAGE_GEN_MODEL` |
| `generate_speech` | `HKASK_MEDIA_TTS_MODEL` |
| `transcribe`, `chat_audio`, `chat_json` | `HKASK_MEDIA_STT_MODEL` |
| `generate_video`, `image_to_video` | `HKASK_MEDIA_VIDEO_MODEL` |

`ProviderRegistry::execute` resolves once, verifies the named provider is
registered and supports the operation, and calls exactly that provider with
only the provider-local model. No scoring, registration-order selection,
automatic cross-provider retry, or hidden default substitution remains.
The selected provider's errors return unchanged. Missing configuration or a
selected key names its environment variable (`NotConfigured`); invalid
selection is `Model`. Media MCP maps those to `permission_denied` and
`invalid_argument`, respectively; provider 401/403 remains `Auth` →
`permission_denied`.

`remove_background` and `upscale` use the fixed DeepInfra native endpoints
`Bria/remove_background` and `latentconsistency/upscale`. They need
`DEEPINFRA_API_KEY`, not a model env var, and reject model overrides.
OpenRouter does not serve image-to-image or TTS in these adapters;
DeepInfra does not serve `chat_audio` or `chat_json`.

This decision supersedes `d660f3b754` (scoring/automatic fallback) and
`8cc79c797e` (registration-order routing), not `f86cf19a70` (child-local
media). `LazyInferencePort::media_generate` still uses the child-local
`MediaRouter`; foreground media behavior and chat/vision/embed/IPC routes
are unchanged. The router is now media-only, with one inherent
`media_generate` entry point rather than a fake chat `InferencePort` impl.

**Migration:** qualify existing bare/short-alias media model settings and
replay overrides with the intended full provider name; install that
provider's key. Remove overrides from fixed-model operations. Settings
remain the source of defaults; standalone callers must configure the env
model or pass an override. The current STT settings default remains
`OpenRouter/openai/whisper-large-v3-turbo`; image, video, and TTS settings
have no default. No compatibility fallback is provided.

Offline pins: `media_routing_tests` exercises all eight selectable ops,
invalid selections, absent keys, fixed ops, typed failures, real adapter
HTTP payloads, and child-local `LazyInferencePort` with zero IPC calls.
`hkask-mcp-media/src/error.rs` pins real router 401/403 → tool
`permission_denied` and invalid model → `invalid_argument`.
