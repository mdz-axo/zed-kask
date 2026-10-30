---
title: "Media-model selection report — STT, audio events, visual-event tagging, object redaction"
audience: [developers, architects, operators]
last_updated: 2026-10-29
version: "1.0.0"
status: "Proposed — model review for operator decision; adoptions and outcomes recorded in dated brackets"
domain: "Media / hkask-mcp-media"
mds_categories: [domain, research]
---

# Media-model selection report — STT, audio events, visual-event tagging, object redaction

Status: **Committed 2026-10-29 (`1a41dcfbc9`) as grounding for goal `a6c6fdf3`; post-review outcomes are recorded in dated brackets below.** The audio pass adopted `OpenRouter/qwen/qwen3.8-omni-flash` (card M2, probe-verified) and the Nemotron STT flip is prepared, pinned, and HELD (card M1); remaining recommendations await the operator decisions in the last section.
Date: 2026-10-29 · Author: Z-K agent (glm-5.3:cloud), composed for the principal investigator.
Research ledger: run `29f97aa1cac697b3` — 9 run-scoped web searches (3 per role, at the brief's cap), 9 `resolve_paper` records, 39 agent source annotations. Line citations were verified against the tree as read during this run at HEAD `f95cb5356e` (2026-10-29); the tree has since advanced (t8/t9/t10 landed; M1/M2 adoption commit `1a41dcfbc9`; HEAD `ddea5f7416`), the companion task-breakdown proposal was executed and removed per the house lifecycle (`bc0a6c95d4` — outcomes on the Visual-transcript board cards M1/M2/S1-S5 under goal `a6c6fdf3`), and the citations drifted by those commits were re-verified and fixed 2026-10-29 at `ddea5f7416`.

Ontology anchors: *automatic speech recognition* → `cso:automatic_speech_recognition` (CSO 3.5); *object detection* → `cso:object_detection` (CSO 3.5); *image segmentation* → `cso:image_segmentation` (CSO 3.5); *annotation* → `omc:Annotation` (OMC 2.8, MovieLabs — the vocabulary the media server already carries). *Inpainting* anchors only on the coarse 5W1H core with the operator ruling pending — defined below operationally from cited sources, never from an invented definition.

## Method and claim tiers

- **run-verified** — claim read from a source the research server recorded under this run (`verified` annotation).
- **partial** — paper identity resolved via `resolve_paper` (canonical URL recorded; full text not extracted).
- **code-cited** — claim verified against the tree (file:line).
- **recalled-prior-thread** — carried from the 2026-10-29 curator thread `dcc599f2-7df4-4553-b6f8-64a5b6301829` (conversation-recalled, not ledger-backed); used only where labeled.

Termination: every recommendation carries ≥2 independent run-verified sources and a provider-availability verdict. Gaps are named in the last section, not dropped.

## Configured baseline (code-cited, verified 2026-10-29)

> Baseline as reviewed. Later on 2026-10-29 the audio pass adopted
> `OpenRouter/qwen/qwen3.8-omni-flash` (board card M2, goal `a6c6fdf3`),
> and the STT flip to `DeepInfra/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B`
> was prepared and pinned but HELD (board card M1) — the DeepInfra endpoint
> returned word timestamps once and then segment-only across 7 same-day
> requests. The citations below refer to the pre-work tree.

| Role | Configured model | Citation |
|---|---|---|
| STT | `OpenRouter/openai/whisper-large-v3-turbo` | `kask/crates/hkask-inference/src/model_constants.rs:200-206` (62-min talk transcribes in one call, ~$0.04, 8.5K word timings, verified 2026-09-04; "a segments-only STT response cannot back a reel"); `model_list` confirms it as the served, default `transcribe` model |
| Vision/pass default | `OpenRouter/openai/gpt-4o-mini` | `kask/crates/hkask-inference/src/model_constants.rs:216-222` (non-reasoning is a hard requirement — reasoning-mandatory models reject the passes' non-reasoning requests) |
| Pass-model chain | per-call override → `HKASK_MEDIA_PASS_MODEL` → `HKASK_MEDIA_VISION_MODEL`; unset fails visibly, no hidden constant | `kask/mcp-servers/hkask-mcp-media/src/layer_pass.rs:62-76`, `kask/mcp-servers/hkask-mcp-media/src/tools/layers.rs:303-325` |
| Audio-input pass | `chat_audio` — OpenRouter-only; DeepInfra refuses with a typed error naming `OpenRouter/mistralai/voxtral-small-24b-2507` | `kask/mcp-servers/hkask-mcp-media/src/layer_pass.rs:282-333`; `kask/crates/hkask-inference/src/media_providers.rs:459-465` |
| STT wire format | base64 `input_audio`, `response_format: verbose_json`, `timestamp_granularities: ["word","segment"]` → `TimedWord` | `kask/crates/hkask-inference/src/media_providers.rs:586-609`; `kask/mcp-servers/hkask-mcp-media/src/transcript.rs:17-27` |
| Capture | 16 kHz mono WAV (capture constants; the former Whisper-specific capture description was swept in `1a41dcfbc9`) | `kask/mcp-servers/hkask-mcp-media/src/tools/audio.rs:4-5` |
| Embeddings | `OpenRouter/qwen/qwen3-embedding-8b` @ 1024 dims — unchanged by this review | `kask/crates/hkask-inference/src/model_constants.rs:231-243` |

Registry-confirmed availability (empirical `model_list`, 2026-10-29): the catalog serves the Qwen3-VL family (`Qwen3 VL 8B Instruct`, `Qwen3 VL 30B A3B Instruct/Thinking`, `Qwen3 VL 235B A22B Instruct`, `Qwen2.5 VL 72B Instruct`), `Qwen3.8 Omni Flash`, Moondream 3, plus ~40 other vision models across openrouter/kilocode/ollama/bedrock/runpod (runpod carries `kask-ocr`). No ASR model other than whisper-large-v3-turbo carried `transcribe` capability at run time.

## R1 — ASR and word-timed transcription (anchor: `cso:automatic_speech_recognition`)

| Model | Word timestamps | Languages | Long-audio behavior | Speaker attribution | Cost (wired path) | Availability verdict |
|---|---|---|---|---|---|---|
| `openai/whisper-large-v3-turbo` (current) | verbose_json word+segment granularities (code-cited) | ~99 (run-verified) | 30-s receptive field, chunked long-form; **hallucination during silence** (run-verified); transducer-free decoder forces words in pauses | none native (run-verified) | ≈$0.04/62 min (code-cited) | **AVAILABLE** — configured default |
| `nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B` (released 2026-10-28) | verbose_json word granularities via DeepInfra (run-verified) | 40 transcribe; translate 31; **speaker labels in 13** (run-verified ×3) | **60-minute single request**; offline/batch (run-verified) | built-in, ≤8 speakers (run-verified) | $0.002/min = $0.12/hr DeepInfra (run-verified) | **AVAILABLE** — DeepInfra is a wired registry provider; the `DeepInfraMediaProvider::transcribe` multipart arm exists (`media_providers.rs:289-293`); **[outcome 2026-10-29]** the pin test landed — the arm sends `chunk_level: "word"` and the parse accepts both response shapes (`1a41dcfbc9`) — but the flip is HELD: the endpoint returned word timestamps once and then segment-only across 7 same-day requests; the committed `live_stt_nemotron_returns_word_timings` probe gates any re-flip (card M1) |
| `Qwen3-ASR-1.7B / 0.6B / Flash` | word-level timestamps (run-verified, OpenRouter listing) | 30 languages + **22 Chinese dialects** (run-verified) | offline + streaming (run-verified) | — via `Qwen3-ForcedAligner-0.6B` companion (run-verified) | 1.7B: $0.00045/min DeepInfra; 0.6B: $0.000003/s OpenRouter (run-verified) | **AVAILABLE** — listed on OpenRouter (new since the prior thread's review) and DeepInfra; response-shape pin test required if routed through the OpenRouter transcribe arm |
| Voxtral Small 24B | — | — | — | — | $0.24/hr (one indirect run-verified source) | **UNVERIFIED at listing level** — the code's named audio-chat fallback (`media_providers.rs:459-465`); **[outcome 2026-10-29]** the M2 probe verified Omni Flash instead (adopted, `1a41dcfbc9`) — this listing stays unprobed as the named fallback |
| `nvidia/parakeet-tdt-0.6b-v3` | word+segment timestamps (run-verified) | 25 EU languages (run-verified) | transducer **emits blanks instead of forcing words in pauses** — anti-hallucination by design (run-verified) | none | CC-BY-4.0; not hosted | **AVAILABLE locally-run only** — not on any configured hosted provider; RunPod self-host would be a new runner |

Measured standing: Parakeet 6.32 vs Whisper-Turbo 7.83 English Open-ASR mean (run-verified); Nemotron English WER 4.71 / AA-WER 2.8, tied first open-weight (run-verified); Qwen3-ASR-1.7B GigaSpeech 8.45 vs Whisper 9.76, KeSpeech 5.10 vs 28.79 (run-verified).

**Recommendation R1.** PRIMARY — adopt `deepinfra/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B` (existing backlog card M1) **[outcome 2026-10-29: prepared and pinned, flip HELD on the flaky endpoint — Whisper stays the default until `live_stt_nemotron_returns_word_timings` passes]**: best measured WER, native 1-hour single-request context (matches the 62-minute production precedent), built-in speaker attribution, and a serving path that is already wired end-to-end. FALLBACK — keep `OpenRouter/openai/whisper-large-v3-turbo`: wider language coverage (~99 vs 40) at ~3x lower cost, already production-verified; it stays the documented fallback for languages outside Nemotron's 40. Qwen3-ASR is named as the now-available challenger — strongest for Chinese-dialect-heavy recordings and the cheapest hosted ASR on the wired surface — adoptable later behind its own pin test.

**One model for both transcription and the audio-event pass (t5)? No.** The two seams require different protocols: the STT seam requires the transcriptions-endpoint response (`verbose_json` word granularities → `TimedWord`), while the audio-event pass requires `chat_audio` support. Nemotron carries transcription and audio-QA in one checkpoint, but DeepInfra refuses `chat_audio` (`media_providers.rs:459-465`) — it cannot serve t5. Qwen3.8 Omni Flash can serve t5 via OpenRouter `chat_audio`, but chat-model transcription does not produce word timings through the STT arm, and "a segments-only STT response cannot back a reel" (`model_constants.rs:202-203`). Keep the roles separate: M1 (STT) + M2 (audio pass) is the pairing.

## R2 — Vision passes and open-vocabulary tagging (anchor: `cso:object_detection`)

Baseline fact: the delivered t4 vision pass (`layers_semantic_pass`, `kask/mcp-servers/hkask-mcp-media/src/tools/layers.rs:698-1185`) already produces per-chunk semantic/object/OCR items with normalized 0..1 boxes, confidences, and frame asset refs (`tools/layers.rs:980-1010`), gated deterministically (`validate_box` `tools/layers.rs:330-350`; `validate_confidence` `:356-367`). The model question is grounding quality and cost, not item shape.

| Model | Literature standing | ComfyUI / fal production use | Availability | Output → layer payload |
|---|---|---|---|---|
| `Qwen3-VL-8B-Instruct` (Apache-2.0, run-verified) | text–timestamp alignment for video temporal grounding; OCR 32 languages, robust in low light/blur/tilt (run-verified HF + GitHub); Qwen3-VL paper released 2025-11-27 (run-verified) | the tagging canon: Qwen-VL Caption node in production ComfyUI use ("half of r/comfyui is running Qwen3-VL"); included in the July 2026 four-way captioner comparison (Florence-2/WD14/JoyCaption/Qwen3-VL) — all run-verified | **AVAILABLE** — registry-confirmed (`model_list`: openrouter, KiloCode, ollama `qwen3-vl:8b`, amazon-bedrock listings) | grounding boxes → `object` item `payload.box` (the pass template already demands normalized boxes, `templates.rs:98-102`); per-call override fits with zero code change |
| `gpt-4o-mini` (current default) | — no native box grounding worth the name | wired, proven v1 schema-in-prompt mode | **AVAILABLE** — configured default | captions/events without reliable boxes |
| **SAM 3** (shared candidate — cross-referenced once with R3) | Promptable Concept Segmentation; paper arXiv 2511.16719 (partial) | SAM2/SAM3 segmentation nodes in the maintained `ComfyUI_VLM_nodes` pack (run-verified); fal `sam-3/video-rle` $0.005/16 frames, text/point/box/mask prompts, RLE masks + per-frame box overlays, **commercial use permitted** (run-verified) | **BLOCKED as a dependency** — fal is a data-services credential (a new serving path, not a registry inference provider); HF weights are `license: other` and gated (run-verified) | mask tracks → future tier-2 redaction anchors; not layer-item payloads in v1 |
| Grounding DINO | ECCV 2024; arXiv 2303.05499 (partial); 52.5 AP COCO zero-shot [recalled-prior-thread] | `VLMOpenVocabularyDetection` exposes Grounding DINO Tiny/Base, OWLv2, OmDet (run-verified) | **NOT available** — local ComfyUI-class Python runner; excluded under the kask/ no-Python rule | boxes+labels → `object` items if ever adopted |
| Florence-2 | dedicated lightweight tagger: caption-to-phrase grounding, region captions, detection (run-verified tutorial/node evidence) | KJ and spacepxl node packs; MiaoshouAI tagger fine-tunes [recalled-prior-thread] | **NOT available** — same local-runner exclusion | labels/boxes → `object`/`semantic` items if ever adopted |
| RAM / YOLO-World | no new capability signal found within this run's bound (the prior thread cut them at its bound too) | not surfaced | **EVIDENCE GAP** — not recommended | — |

**Recommendation R2.** PRIMARY — `openrouter/Qwen3 VL 8B Instruct` as the per-call override for grounded tagging passes: registry-confirmed, non-reasoning Instruct edition (fits the pass contract — avoid Thinking editions), native grounding and 32-language OCR, and the strongest production evidence of any candidate. FALLBACK — keep `gpt-4o-mini` as the cheap default pass model for caption-heavy passes; whether to registry-confirm Qwen3-VL-8B as the new default is the operator's cost call (per-call overrides leave the default untouched). SAM 3 stays tier-2 mask evidence (R3), not a v1 dependency.

## R3 — Image modification and object redaction (inpainting: coarse 5W1H anchor, operator ruling pending)

Operational definition from cited sources only: VOID "removes objects from videos along with all interactions they induce on the scene — not just secondary effects like shadows and reflections, but physical interactions" (run-verified, `netflix/void-model`); Generative Omnimatte "trains a video diffusion model to identify and remove scene effects caused by a specific object" (run-verified, CVPR 2025); LaMa performs mask-based region inpainting for object removal (arXiv 2109.07161, partial; ComfyUI node run-verified); FLUX.1 [pro] Fill performs "targeted modifications, replacing objects, filling gaps" (run-verified, fal). The anchor ruling remains requested (see open questions).

| Tier | Model / path | Evidence | License | Production use | Availability |
|---|---|---|---|---|---|
| 1 — ffmpeg-native | `boxblur` / `pixelize` / `drawbox` / `delogo` / `gblur` / `avgblur` over a time-ranged box | verified present on the local ffmpeg 8.0.1 (`ffmpeg -filters`, run 2026-10-29); `FfmpegRunner` already builds filter graphs (`add_caption` drawtext, `kask/mcp-servers/hkask-mcp-media/src/video/ffmpeg.rs:504-554`) and does lossless clip/concat (`:422-461`, `:690-716`) | LGPL (ffmpeg) | the existing reel-render primitive is the same segment-clip + concat shape | **ALWAYS AVAILABLE** — local, deterministic, zero new models, zero new serving paths |
| 2 — mask + removal | **VOID** (Netflix) — two checkpoints (base + optical-flow-refined pass) | paper arXiv 2604.02296 (partial); interaction-aware removal | **Apache-2.0 (run-verified — resolves the prior thread's UNVERIFIED flag)** | official ComfyUI tutorial, runcomfy workflows, two-pass tutorials with SAM 3.1 masking — all run-verified | **GATED** — RunPod is a wired registry provider, but the runner is a Python/GPU stack excluded under the kask/ no-Python rule; deferred per operator decision C |
| 2 — mask + removal (alternative) | **Generative Omnimatte** (CVPR 2025, pp. 12522-12532, run-verified) | removes soft shadows/reflections that ProPainter-class models miss (run-verified) | **Apache-2.0 (run-verified)** | — | same GATED verdict |
| 2 — masks | **SAM 3** (shared with R2) | see R2 row | fal commercial use permitted; HF weights `license: other`, gated | see R2 | **GATED** — fal = new serving path |
| 2 — excluded | ProPainter (ICCV 2023; arXiv 2309.13321, partial) | Replicate listing exists | **license "Other" — non-commercial** (run-verified mirror) | — | **EXCLUDED** for production redaction regardless of serving path |
| image-tier fill | LaMa (arXiv 2109.07161, partial); FLUX.1 [pro] Fill ($0.05/MP, run-verified; FLUX.1-Fill-dev ComfyUI workflows run-verified) | single-frame fill | LaMa Apache-2.0 [recalled-prior-thread] | ComfyUI nodes run-verified | **GATED** — fal new serving path / local runner; image-domain only, later |
| excluded | Kling O3 Edit (fal) | $0.126/s, 3–15 s clips [recalled-prior-thread; not re-verified this run] | — | — | demo tier — skip for redaction |

**Recommendation R3.** TIER 1 (adopt now): ffmpeg-native redaction over an approved decision layer's time-range + box — blur (`boxblur`/`gblur`), pixelate (`pixelize`), black-box (`drawbox`), `delogo` — all verified locally; deterministic, reversible, audit-friendly. TIER 2 (deferred per operator decision C): SAM 3 mask tracks → VOID or Generative Omnimatte on a RunPod runner. **Quality floor, stated honestly:** tier 1 obscures pixels — the object remains as a blurred/blocked region; it does not remove the object or its interactions. What tier 2 adds is exactly interaction-aware removal (shadows, reflections, physical causality), at GPU cost per redacted minute and a new serving path. VOID is the primary tier-2 candidate now that its license is verified Apache-2.0.

## Evidence gaps (named, not dropped)

1. **Voxtral Small 24B** — its own provider listing was not retrieved (one indirect pricing source); **[outcome 2026-10-29]** the M2 probe verified Omni Flash instead (adopted, `1a41dcfbc9`) — Voxtral stays the code-named fallback, unprobed.
2. **Qwen3.8 Omni Flash `input_audio` — RESOLVED 2026-10-29**: the M2 probe ran `layers_audio_pass` on a real 15 s recording (one chunk, one inference call) and produced labeled `audio_event` items; adopted as the audio passes' per-call model (`1a41dcfbc9`; card M2 verbatim outcome).
3. **SAM 3 open weights** — `license: other` + gated on HF (run-verified); self-host licensing needs the operator's read of the license text.
4. **OpenRouter per-token prices** for Qwen3-VL tiers were not retrieved; the cost comparison for a default change is deferred to adoption time (per-call overrides keep the default unchanged).
5. **RAM / YOLO-World** — no signal within the bound; not recommended.
6. **DiffuEraser** (Alibaba open-source video inpainting, ComfyUI pack) surfaced in one R3 result — recorded in the ledger, not researched (outside the bound).

## Citations (run ledger `29f97aa1cac697b3`; states as annotated)

R1 — verified: cekura.ai/blogs/nemotron-3-5-transcribe · deepinfra.com/blog/nvidia-nemotron-3-5-transcribe-release · research.nvidia.com/labs/nemotron/NVIDIA-Nemotron-3.5-Transcribe/ · huggingface.co/blog/nvidia/nemotron-transcribe-granary · openrouter.ai/qwen/qwen3-asr-1.7b · huggingface.co/Qwen/Qwen3-ASR-1.7B · deepinfra.com/Qwen/Qwen3-ASR-1.7B/api · convertaudiototext.com/blog/whisper-large-v3-explained · northflank.com/blog/best-open-source-speech-to-text-stt-model-in-2026-benchmarks · whispernotes.app/blog/parakeet-v3-default-mac-model · loronote.com/en/blog/parakeet-v3-vs-whisper-large-v3; partial: arxiv.org/abs/2212.04356 (Whisper).
R2 — verified: huggingface.co/Qwen/Qwen3-VL-8B-Instruct · github.com/qwenlm/qwen3-vl · fal.ai/models/fal-ai/sam-3/video-rle · fal.ai/models/fal-ai/sam-3/video · huggingface.co/facebook/sam3/blob/refs%2Fpr%2F10/README.md (encoded form as recorded) · github.com/gokayfem/ComfyUI_VLM_nodes · comfy.icu/node/QwenVLCaption · vietnamwhite.hatenablog.com/entry/2026/07/28/073000 · openrouter.ai/qwen; partial: arxiv.org/abs/2511.16719 (SAM 3) · arxiv.org/abs/2303.05499 (Grounding DINO) · arxiv.org/abs/2502.13923 (Qwen2.5-VL) · arxiv.org/abs/2509.17765 (Qwen3-Omni).
R3 — verified: docs.comfy.org/tutorials/utility/void-video-inpainting · github.com/netflix/void-model · blog.comfy.org/p/new-open-source-models-now-in-comfyui · github.com/gen-omnimatte/gen-omnimatte-public · gen-omnimatte.github.io · openaccess.thecvf.com (Lee et al., CVPR 2025) · runcomfy.com/comfyui-nodes/comfyui-art-venture/LaMaInpaint · fal.ai/models/fal-ai/flux-pro/v1/fill · youtube.com/watch?v=pTfLEPFjgxc · github.com/deep-info/ProPainter; partial: arxiv.org/abs/2604.02296 (VOID) · arxiv.org/abs/2411.16683 (Generative Omnimatte) · arxiv.org/abs/2109.07161 (LaMa) · arxiv.org/abs/2309.13321 (ProPainter).

## Open questions for the operator

1. M2: ratify the executed Omni Flash adoption (probe passed, wired in `1a41dcfbc9`; Voxtral stays the named fallback). M1: decide with the held state — the Nemotron flip is prepared and pinned but HELD on the flaky endpoint (word timestamps once, then segment-only across 7 same-day requests); keep Whisper default and re-evaluate via the committed `live_stt_nemotron_returns_word_timings` probe, or re-flip when it passes.
2. Registry-confirm `Qwen3 VL 8B Instruct` as the grounded-tagging per-call model (or as the new pass default — the cost trade is yours).
3. The inpainting ontology-anchor ruling (coarse 5W1H today; this report defines the term operationally from cited sources in the meantime).
4. Tier-2 serving-path decision (deferred per decision C): fal for SAM 3 masks vs RunPod self-host, and the Python-runner allowlist exception the tier requires.