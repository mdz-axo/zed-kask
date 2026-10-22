# hkask-inference

Multi-provider inference router for hKask — OpenRouter, Ollama, RunPod.

## Entry points

- **Inference ports** — `resolve_inference_port` and `InferenceIpcClient` connect callers to the configured host; the explicit direct-path policy is described below.
- **Provider configuration** — `InferenceConfig`, `ProviderId` and `model_constants` resolve visible provider/model choices; they are not a study budget or evidence-retention contract.
- **Packet-study instrument** — `packet-study` prepares evidence-conditioned judgments, executes native OpenAI count-first HTTP, replays digest-bound responses, and audits durable all-in admission journals. The standalone executor does not use the editor's inference router.

## Packet-study instrument

Study A compares completion judgments over supplied evidence, not autonomous tool choice or code repair. The instrument lives in this standalone crate: it adds no editor, registry-execution or generic agent framework dependency.

```bash
cargo run -p hkask-inference --bin packet-study -- verify --output /absolute/new-directory
cargo run -p hkask-inference --bin packet-study -- audit --output /absolute/new-directory
```

`verify` runs deterministic contract controls and scripted responses through the real preparation, parser, reservation, persistence and paired-report path. It creates a private `.replay-inputs` sidecar next to the requested output for its scripted source bodies; both locations must be new. Every output is labeled `instrument_verification_replay`: the displayed model name and usage/cost arithmetic are fixture declarations, not model observations or provider charges. Controls are not a pilot dataset or heldout scientific evidence.

The resulting private directory contains `protocol.json`, `private-cases.json`, `replay-records.json`, per-attempt count/generation request/raw-response/receipt files, count-only `*.count.replay.json` metadata, an fsynced hash-chained `budget.jsonl`, and `report.json`. The output directory must be new; files are never overwritten or automatically resumed. On Unix the directory/files are created with 0700/0600 permissions. Hash consistency is not producer authentication.

```bash
cargo run -p hkask-inference --bin packet-study -- prepare \
  --protocol protocol.json --cases cases.json --output /absolute/new-preparation
cargo run -p hkask-inference --bin packet-study -- replay \
  --protocol protocol.json --cases cases.json --responses replay-records.json \
  --output /absolute/new-replay
```

Use the verification output as a documented **mock-only** schema example. The `packet-study-instrument-v3` protocol adds count maximum-fee provisions and tariff provenance. v1/v2 protocols, unprovisioned count events, whole-per-token tariff fields and caller-declared input bounds are rejected, never silently reinterpreted. Protocol, case and replay inputs are bounded JSON files. Each replay entry has a native counter (`count`) and generation (`response`) with metadata and relative `response_file` basenames/digests, not inline bodies. The counter's raw `response.input_tokens` result must pass before generation admission; its request fingerprint binds the documented input-shaping projection, the entire generation body and the attempt identity. Unknown settings/server conversation state, count failures, oversized counts and payload drift block generation. Generation bodies are opened only after admission and synced reservation/request persistence; the retained generation index contains only acquired responses. Count-only failures retain their raw bytes, receipts and `*.count.replay.json` separately without copying an unadmitted generation reference. Duplicate IDs/digests and invalid case/checker classifications fail. Enum values must be strings; object-shaped encodings such as a `verdict` of `{"complete":null}` are rejected rather than scored. Request fingerprints bind protocol, attempt, stage and exact body, so identical bodies from different attempts cannot reuse one receipt. Initial model history is shared across methods; final requests contain only their own branch's review. Hidden contract truth and primary-checker health labels do not enter request bodies. Weak checker receipts come from actually executed deterministic predicates, not copied oracle labels.

Reference contract checks cover JSON-container preservation, checked integer sums and constrained readiness claims. They share a trusted JSON parsing kernel; reference agreement and smoke controls are not independent scientific validation. Malformed output is not abstention. Missing primary-pair members remain bounded unknowns, rather than disappearing from the denominator; reported bounds are not confidence intervals or a treatment conclusion.

Generation reservation arithmetic remains exact integer micro-USD per million tokens (`prompt_microusd_per_million_tokens`, `completion_microusd_per_million_tokens`), checked `u128` intermediates, and one upward rounding of the combined request cost. Admission includes incurred generation cost, outstanding generation reserves, **all outstanding count maximum-fee provisions**, and the next provision. `limits.count_request_max_fee_microusd` is a conservative per-count maximum in whole micro-USD, not a per-token rate. Unknown (`null`) refuses before HTTP. Count fees are never inferred free from a successful token count: their full provisions stay outstanding, including on success, because this endpoint supplies no settled bill. Reports/audits expose count and generation liability separately. A count event carrying its maximum fee is synced before count HTTP; a generation reserve is synced before generation HTTP. Cap checks reserve `provider_output_max=128000` while still requesting only1024; ordinary requests reserve the requested cap. Missing/partial usage, count/usage drift, cap violations and HTTP/native errors retain liability and stop further admission. Reasoning tokens are a subset of completion tokens, not an extra charge.

The journal root binds the canonical protocol digest; count-event fees must match that protocol. `audit` is labeled `journal_accounting_audit`, not a provider observation: it reconstructs the validated complete prefix, surfaces a torn tail as degraded/incomplete, and rejects a corrupt complete entry. Neither audit nor a stopped report permits retry or resume. Native response model/status/error/output/usage remain in raw files; tools, refusals and unexpected output shapes fail closed. Ordinary incomplete output is missing judgment; only cap checks permit `status=incomplete` with `incomplete_details.reason=max_output_tokens`. Deterministic replay/mock results do not prove live cap compliance.

### Native execution

```bash
cargo run --offline --locked -p hkask-inference --bin packet-study -- execute \
  --protocol protocol.json --cases cases.json --output /absolute/new-live-run
```

Only `execute` resolves credentials, after financial/policy validation: a configured `OPENAI_API_KEY` takes precedence; **only absence** falls back to the existing native OpenAI provider Settings credential at the canonical keychain URL `https://api.openai.com/v1`. Empty, whitespace-padded, non-ASCII or malformed environment values fail visibly without keychain fallback. The existing `hkask_keystore::Keychain.retrieve_by_url` runs on a CLI-only worker with a five-second receive deadline; a wedged worker may linger after timeout, but cannot stall CLI admission. Not-found, platform-inaccessible and timeout failures remain distinct, with diagnostic payloads redacted. No secret is written and no new keychain location is created. There is no proxy, alternate URL, provider/model fallback, credential logging, or editor dependency. No key belongs in a command line, protocol, report, or README. The CLI owns a Tokio current-thread runtime; do not invoke it from GPUI foreground code or an existing runtime. All other modes remain credential-free.

Live policy pins direct OpenAI `gpt-5.4-mini-2026-03-17`, at most170 **each** count and generation attempts (at most340 total HTTP), zero retries, an all-in ceiling at most10000000micro-USD ($10), input at most8192, requested output at most1024, and provider maximum128000. Lower attempt/spend/token ceilings are accepted. HTTP execution requires generation tariffs at least750000input/4500000output micro-USD per million tokens, the published Standard-rate floors recorded in Study A's `single-model-v1.1.0/model-profile-v1.1.0.json`. Exact-floor and higher conservative rates are accepted; this is a study-specific safeguard, not a general model default or current-price authentication. Replay arithmetic may still use custom tariffs. The native codec retains text-only public history with `reasoning.effort=none`, `store=false`, Standard/global service, foreground/non-streaming requests and no tools/server conversation state. No model code execution is possible through this dispatcher.

A protocol must explicitly supply `limits.count_request_max_fee_microusd` and `count_tariff: {"basis":"supplied_tariff","source":"…","recorded_at":"…"}`. `source` and `recorded_at` must be nonempty. This records **caller-supplied tariff provenance, not an authenticated vendor promise**. The supplied generation rates and count maximum must conservatively cover applicable known fees/taxes within the all-in ceiling. Undocumented count charges remain unknown: do not substitute zero or invent a source to unlock execution. `verify` uses an explicit `Some(0)` mock fee and `basis=mock_only`; that tariff is refused by live execution. No price default or caller `ready`/capability-acceptance flag grants admission.

Each HTTP attempt has a60-second timeout and at most1MiB retained response bytes. Redirects, automatic HTTP/2 stream retries, idle connection reuse and replayable request bodies are disabled. Failed/timed-out attempts consume their count/generation allowance; no fallback or resume follows. Raw response prefixes, status, transport-failure classification, available native response ID and allowlisted response headers (request ID, rate limits, retry-after, date, content type, processing time) are persisted. Authorization/request headers are never written to receipts or reports. Transport diagnostic text is deliberately classified rather than exposing URL/credential-bearing diagnostics. Byte limits remain storage limits, not token-count proofs.

Reports use `live_packet_study` only after HTTP was attempted, `live_not_dispatched` before any attempt, `local_mock_http` for loopback qualification, and `instrument_verification_replay` for file fixtures. Acquired metadata can be replayed offline without relabeling it as a fresh live observation. Output paths must be new.

Every budget report and accounting audit separates `execution_provenance` from financial evidence: `cost_basis` is always `supplied_tariff_estimate_not_provider_billing`, `costs_are_tariff_estimates_not_provider_charges` is always true, and `provider_billed_microusd` is always null. Live HTTP/usage does not establish a provider bill; even `incurred_microusd` is tariff-derived arithmetic, and the native codec supplies no reported monetary cost. Paired bounds always carry `treatment_conclusion=not_generated_by_runner_requires_prespecified_analysis`: neither live execution nor this runner generates a scientific treatment conclusion.

The [native counting contract](https://developers.openai.com/api/docs/guides/token-counting) documents input formatting and an output cap covering non-visible tokens; the [count reference](https://developers.openai.com/api/reference/resources/responses/subresources/input_tokens/methods/count) defines the projection/discriminator. Socket-side local HTTP tests exercise the dispatcher and the same study controller, including durable count-first payload binding, full workflow, budget boundaries, failures, retained raw prefixes and no retries/fallback. They use fake credentials and loopback only; they are not authenticated provider measurements.

The existing nine deterministic controls are not independently calibrated scientific cases or a pilot dataset. Scientific case/oracle calibration, frozen protocol/allocation, retention/access/source checks and lifecycle governance remain the PI/main orchestration's responsibility, outside this CLI. Capability acceptance is not an execution prerequisite. `store=false` is not ZDR; standard retention remains applicable unless independently established otherwise. No editor setting changes or provider-adoption claim follows.

Reproduce local qualification without provider access or credentials:

```bash
bash kask/scripts/cargo-test-nonzero.sh --offline --locked -p hkask-inference
./script/clippy --offline --locked -p hkask-inference
```

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
