---
title: "educt transcript/video research and speaker-identity design assessment"
audience: [operator, architects, developers]
last_updated: 2026-10-27
version: "1.0.0"
status: "Research assessment and updated-plan review; no implementation or measurements"
domain: "Media / transcript capabilities"
mds_categories: [domain, research]
---

# educt transcript/video research and speaker-identity design assessment

## 0. Scope, cutoff, and evidence discipline

**Research cutoff:** 2026-10-27. This is a bounded review, not a living survey.
Primary ledger: research run `258f6fd09a8285aa` (completed). The updated plan's
independent earlier ledger is `e99d0ff4045bfb64`; its source claims were used only
where the artifact was inspected or is explicitly downgraded below.

Evidence labels:

- **[tree]** — current working tree inspected. Uncommitted work is identified and is
  not called delivered or validated.
- **[inspected]** — underlying paper, official model card, repository, or official
  documentation inspected, not merely a search snippet.
- **[metadata]** — bibliographic identity or official abstract only.
- **[vendor]** — official self-report; useful evidence of intended behavior, not
  independent validation.
- **[inference]** — synthesis from cited evidence.
- **[proposal]** — design or experiment not yet run.

No tests, diarization benchmarks, speaker-identification trials, accuracy
measurements, or implementation work were performed for this assessment. A ledger
`verified` annotation records source inspection, not proof that a vendor's result is
true or transferable.

## 1. Executive summary

1. **Current `educt` has a strong transcript-linked editorial base, not an acoustic
   speaker-identity system.** It persists word-timed bundles and immutable source
   words; applies provenance-bearing correction, speaker, paragraph, highlight, EDL,
   and realignment layers; composes/renders reels; exports; searches lexically; and
   links transcripts to gallery assets [tree]. The current audio/text speaker passes
   produce model-attributed spans, but no demonstrated DER/JER evaluation, acoustic
   diarizer, voice-profile store, canonical person entity, or cross-recording voice
   identity exists [tree].
2. **Diarization is not identification.** The practical stack is: acoustic speech
   segmentation/diarization → recording-local clusters → speaker embeddings →
   open-set comparison against explicitly enrolled profiles → `unknown` or a
   reviewable name suggestion. A text label, an LLM confidence, or a same-recording
   cluster is not evidence of a real-world name [S1][S15][S21].
3. **There is no defensible universal diarization winner.** The independent ETH
   benchmark favored pyannoteAI Precision-2 overall and DiariZen among its tested
   open systems, under one out-of-box protocol [S13]. It did not compare Nemotron 3,
   Community-1, or Precision-3. NVIDIA and pyannote results use different reference
   labels, collars, datasets, and reporting conditions [S9][S11][S14].
4. **The most relevant new local diarization candidate is Nemotron 3 via its native
   C++ runtime.** Its card documents offline/streaming inference, up to eight
   speakers, 100M parameters, and a NeMo-Speech.cpp path [S11]. This is a research
   candidate, not an `educt` feasibility claim: target-hardware throughput, OpenMDW
   1.1 terms, overlap behavior, and session-disjoint accuracy remain unmeasured.
5. **For cross-recording identity, an ONNX ECAPA/CAM++/ResNet-class encoder is a
   practical candidate, not a proven choice.** SpeechBrain supplies an inspectable
   local ECAPA-TDNN card; sherpa-onnx supplies local deployment artifacts; meetily
   PR #511 demonstrates a Rust/`ort` prototype pattern [S16][S17][S20]. Embeddings
   must be namespaced by exact model, weights, and preprocessing fingerprint. No
   cross-model interoperability should be assumed.
6. **LLMs are most credible as constrained orchestrators.** DiarizationLM supports
   transcript-preserving speaker reassignment; unconstrained zero/one-shot correction
   degraded text/attribution in the inspected experiments [S23]. VideoAgent and Deep
   Video Discovery support iterative retrieval and question answering, not acoustic
   identity or autonomous editorial truth [S29][S30].
7. **The updated plan's D1–D4 product decisions can stand, but four specifications
   must be corrected before Phase 2 or sealing:** the unencrypted media DB cannot be
   described as SQLCipher-protected; an agent-writable annotation is not a human or
   data-subject consent receipt; overlap cannot be excluded from identification
   errors; and threshold selection needs an independent final test set and a real
   comparator. Details are in §9.

## 2. Terminology and ontology status

| Term | Research-grounded meaning |
|---|---|
| Speaker diarization | Determine “who spoke when”: speech/speaker segmentation and recording-local labels. It does **not by itself** establish a person's name [S1][S13]. Overlap requires multi-speaker activity, not one forced label per interval. |
| Speaker clustering | Group segment/frame embeddings into recording-local speaker clusters, often agglomerative or Bayesian HMM clustering [S3][S8]. |
| Closed-set identification | Choose one of N enrolled identities; every probe is forced to a gallery identity. |
| Open-set identification | Compare against enrolled identities while permitting non-enrolled probes; return `unknown` when evidence is insufficient. This is the appropriate `educt` setting [S21][S22]. |
| Speaker verification | Decide a 1:1 same-speaker claim. Verification scores can support identification but are not automatically calibrated open-set probabilities [S15][S21]. |
| Voice/speaker embedding | Fixed-dimensional, model-produced speaker-discriminative representation. Dimension, geometry, preprocessing, and score distribution are model/artifact-specific [S15][S16][S17]. |
| Enrollment | Creation of reference voice evidence linked to a person and exact source spans, with quality, consent/authority, media, and model provenance. NIST evaluations vary enrollment and probe duration explicitly [S21]. |

Local `speaker diarization` and `speaker embedding` anchors remain coarse 5W1H
anchors. `speaker identification` resolves through CSO in the prior review. The
ontology follow-up should attach published definitions (Park et al. for diarization;
ECAPA/x-vector literature for embeddings) rather than create a local taxonomy. No
new local ruling is asserted here.

## 3. State of the art: acoustic diarization and voice identity

### 3.1 Architecture and deployment comparison

| Approach/system | Type / modality | Deployment and compute evidence | License / maintenance evidence | Privacy and principal limits |
|---|---|---|---|---|
| x-vector + PLDA + AHC | Modular audio embeddings, backend scoring, agglomerative clustering | Local-capable; mature CPU/GPU implementations; multiple stages and VAD dependency [S2] | Implementation/model terms vary; established reproducible baseline | No names without enrollment; weak overlap handling without additional segmentation; module errors propagate. |
| VBx | Bayesian HMM clustering over x-vector sequences | Local/offline; research artifacts available [S3] | Artifact-specific; established baseline, not a current identity model | Does not itself resolve overlap; depends on frontend VAD/segmentation and embedding domain. |
| EEND / EDA-EEND | End-to-end frame-level multi-label audio activity; attractor variants address variable speakers | Local research inference; attention/long-form cost and output/channel constraints matter [S4][S5] | Research lineage; artifact-specific | Overlap-native formulation, but finite output capacity and long-form/generalization constraints remain. |
| TS-VAD | Per-speaker frame activity conditioned on initial speaker representations | Local research; multi-stage and comparatively heavy [S6] | Interspeech 2020 artifacts; implementation-specific | Strong overlap-oriented design; depends on initial diarization/enrollment-like representations and is not cross-recording identification. |
| MSDD | Multi-scale diarization with dynamic scale weighting | Local/NeMo research path [S7] | Research artifact | Addresses temporal-resolution/embedding-fidelity tradeoff; does not solve names or every overlap/channel condition. |
| pyannote 3.1 / Community-1 | Hybrid local segmentation + speaker embeddings + clustering; Community-1 uses powerset segmentation + WeSpeaker + VBx | Local-capable; Community-1 documents offline/local and CPU-default use; throughput is machine-specific [S8][S9] | pyannote pipeline/community model artifacts have distinct terms; Community-1 card states CC-BY-4.0 and gated access [S9] | Recording-local labels only. Community-1 reports much higher DER on AVA-AVD/Ego4D than on cleaner benchmarks; no universal language guarantee. |
| DiariZen / cVBx | Hybrid WavLM/EEND local activity + global clustering | Local research; independent benchmark ran RTX A6000; inference-only RTF reported there [S10][S13] | Described as open source in the independent study; exact artifact/model terms must be checked | Strong tested high-speaker/open alternative [S13]; Python research integration conflicts with kask's no-Python implementation rule unless externally evaluated or reimplemented. |
| Sortformer v1/v2 | End-to-end arrival-ordered transformer diarization | Local NeMo; v1/v2 tested up to four speakers; long-form chunk/cache strategies [S11][S13] | Version-specific cards: noncommercial/open terms differ by release; check the exact checkpoint | Fast in the ETH inference-only GPU protocol, but 5+ speaker degradation and four-speaker envelope are material [S13]. |
| Nemotron 3 Diarization | 31-layer transformer, 10 ms activity, AOSC/FIFO streaming; up to eight speakers | **Local-capable/hybrid research candidate:** NeMo-Speech.cpp native path plus Python/Transformers; 100M parameters. Card speed results are NVIDIA RTX PRO 5000 BF16, not local CPU evidence [S11] | OpenMDW 1.1; release 2026-09-23; current official card and native runtime [S11] | Vendor-reported results depend on exact RTTMs/collars. “Unlimited when chunked” is not proof of drift-free identity over arbitrary sessions. Audio stays local only in a local runtime. |
| LS-EEND | Causal encoder + online attractor extraction for long-form streaming EEND | Research local path; source reports linear temporal complexity [S12] | IEEE/author artifacts; exact deployment terms not established here | Relevant long-form architecture, but not independently compared with the newest local candidates in this review. |
| pyannoteAI Precision-2 / Precision-3 | Commercial cloud diarization | **Cloud-only**; Precision-2 independently tested in ETH; Precision-3 internals/compute opaque [S13][S14] | Commercial vendor service; Precision-3 current vendor materials | Audio leaves the machine. Precision-3's 14.35 average DER/10.4% improvement is **[vendor]**, not an independent matched-protocol result. |

### 3.2 Evaluation conditions, not leaderboard folklore

| Evidence | Actual conditions | What it supports — and what it does not |
|---|---|---|
| ETH *Benchmarking Diarization Models* [S13] | Five systems; 196.6 h; CALLHOME/VoxConverse/AMI/AliMeeting; English/Mandarin/German/Japanese/Spanish; 0.25 s collar; overlap included; out-of-box models; RTX A6000 for local systems | Supports condition-specific comparisons among those five systems. Missed speech dominated, with roughly 350 ms average missed durations; boundary precision mattered more than wholesale missed short segments. It does not rank Nemotron 3, Community-1, or Precision-3. RTF excludes data/model loading. |
| Community-1 model card [S9] | No-collar, no-skip DER across DIHARD-3, VoxConverse, AMI, CALLHOME, AliMeeting, AISHELL-4, MSDWild, REPERE, AVA-AVD, Ego4D | Shows large domain/channel degradation and provides an exclusive-diarization output useful for ASR reconciliation. Numbers are not directly comparable with ETH's collar/protocol. |
| Nemotron 3 model card [S11] | 901 condition-specific recordings; up to eight speakers; exact original vs forced-aligned RTTMs; overlap included; collars 0 s except CALLHOME 0.25 s; offline/streaming configurations | Shows vendor-reported improvement over its four-speaker predecessor under the card's exact protocol. Different RTTMs constitute different evaluations; no independent local `educt` conclusion follows. |
| DIHARD III [S36] | Multi-domain, variable equipment/noise/speaker conditions; DER/JER evaluation plans with explicit overlap/collar rules | Appropriate robustness reference and scoring discipline, not a single universal threshold or leaderboard. |
| NIST SRE24 [S21] | Approximately 10/30/60 s enrollment and 5–60 s probes; audio/visual tracks; cross-source/duration and low-false-alarm operating conditions | Appropriate identity-enrollment/probe structure and calibrated detection-cost practice. It is not a diarization benchmark and does not supply an `educt` threshold. |

**Overlap:** EEND, TS-VAD, powerset segmentation, and overlap-aware resegmentation
are the relevant model-level responses. A single VAD segment with two speakers is
not a valid single-speaker embedding probe. Current disjoint `SpeakerLayer` spans
also lack typed multi-speaker overlap semantics [tree].

**Short turns:** two different problems must not be conflated: diarization boundary
misses [S13] and degraded embeddings from short speech [S23]. Short probes need
aggregation within reliable recording-local clusters, explicit duration strata, and
abstention—not simply a lower threshold.

**Multilingual speech:** acoustic embeddings can transfer across language, but the
complete stack may not: ASR, forced-alignment models, training domain, code
switching, and reference labels all matter [S13][S26]. No inspected evidence justifies
calling any candidate language-universal.

**Variable recording quality:** DIHARD, AVA-AVD/Ego4D results, and SRE conditions
show why channel/session shift must be measured explicitly [S9][S21][S36]. A threshold
from VoxCeleb or another application is only an initialization.

**Long-form media:** chunking, attractor/cache mechanisms, and hybrid local/global
clustering address different scaling problems. None proves stable identity across
arbitrary sessions; measure speaker drift and cross-chunk label consistency.

### 3.3 Local voice-embedding candidates

| Candidate | Deployment / enrollment | Evaluation and license conditions | Limitations for `educt` |
|---|---|---|---|
| ECAPA-TDNN / SpeechBrain `spkrec-ecapa-voxceleb` | Local Python/PyTorch; 16 kHz mono; cosine scoring; extract per-clip embeddings and aggregate enrolled clips [S15][S16] | Card states Apache-2.0, VoxCeleb 1+2 training, VoxCeleb1-test results; explicitly gives no warranty on other datasets [S16] | Strong reproducible candidate, not a multilingual/open-set guarantee. Python runtime is unsuitable for kask-owned implementation, though ONNX/export alternatives can be investigated. |
| WeSpeaker CAM++ / ResNet via sherpa-onnx | Local ONNX/CPU deployment; per-segment enrollment; practical Rust `ort` route [S17][S20] | WeSpeaker/sherpa artifacts are available; runtime and exact weights/data terms must be checked separately rather than inferred from a PR sentence | Similar voices, short/overlapping probes, and channel shift remain. No locally calibrated threshold exists. |
| TitaNet-Large | Local NeMo speaker representation; 23M-parameter en-US card [S18] | Card states CC-BY-4.0; NeMo research/deployment ecosystem | en-US and NeMo/Python constraints; not the first kask-native candidate. |
| ERes2NetV2 | Short-duration-oriented speaker verification candidate [S40] | Interspeech 2024 identity established; model-specific evidence needed before deployment | Promising research alternative, not evidence of better local open-set identification. |
| LocalAI voice-detect | Local C++/GGUF-style stack with several encoders, thresholds, margin, and model fingerprinting [S19] | Official docs updated 2026-10-22; useful deployment reference | In-memory registry, unresolved overlap, no anti-spoofing head; published thresholds are starting points only. |
| meetily PR #511 | Rust `ort` + pure-Rust fbank + CAM++ prototype; persistent profiles; approximately 28 MB model [S20] | PR conversation and code-description evidence, not production validation | Initially unchecked manual multi-speaker validation, maintainer label problem, only macOS/aarch64 build claimed, and later overlap commits. Treat as a prototype seam, not “proven SOTA.” |

## 4. LLM and agentic-video landscape

### 4.1 Acoustic/model-level advances vs orchestration

**Acoustic/model level:** x-vector/ECAPA/WeSpeaker embeddings, EEND/powerset
activity, TS-VAD, multi-scale/hybrid clustering, Sortformer/AOSC, and audiovisual
active-speaker models produce time/speaker evidence. They do not gain identity merely
because an LLM names their output [S1]–[S18].

**LLM orchestration and tool use:**

- **DiarizationLM [S23]:** converts ASR+diarization output to compact speaker-tagged
  text, uses an LLM for reassignment, then Transcript-Preserving Speaker Transfer
  reattaches labels to the original words. In the peer-reviewed 2024 English
  two-speaker telephone setting, fine-tuning improved WDER; zero/one-shot variants
  degraded outputs. The later v13 preprint extends the analysis to more domains and
  shows long-monologue/multi-speaker failure modes. These versions and conditions
  must not be collapsed into one claim.
- **ASR correction [S24][S25]:** constrained N-best/rescoring and bounded edits are
  safer than unconstrained rewriting. The inspected ASR-EC paper reports mixed
  outcomes and deletion/contamination concerns. CoVoGER supplies multilingual
  benchmark scope, but only its official abstract/metadata was inspected here; its
  detailed numbers are not used.
- **WhisperX [S26]:** not an agent; a specialized VAD/ASR/forced-alignment pipeline
  that complements transcript-linked media with better word timing. Alignment model
  availability and language constraints matter.
- **SpeakerLM [S39]:** a directional multimodal end-to-end diarization/named-
  recognition research result [metadata]. Artifact availability and deployment were
  not established, so it is not a deployable recommendation.
- **TalkNet / AVA-ActiveSpeaker [S27][S28]:** audiovisual evidence of whether a
  visible face is speaking. This can complement acoustic overlap attribution; it is
  not face/voice identity and fails for off-screen or occluded speakers.

### 4.2 Agentic video analysis

VideoAgent [S29] and Deep Video Discovery [S30] demonstrate iterative model-driven
search, tool use, temporal grounding, and question answering over long video. DVD's
official repository supplies reproduction materials and a lighter mode. Their evidence
supports **cited retrieval and analysis loops**, not autonomous editing, acoustic
diarization, or open-set cross-recording identity.

Recommended `educt` pattern **[inference][proposal]**:

1. Specialized models extract acoustic/visual evidence.
2. Immutable transcript words/timings remain the editorial anchor.
3. LLM passes make bounded, source-cited suggestions.
4. Agents retrieve across transcript, clip, and frame levels with explicit budgets.
5. Every consequential label/edit remains a separate, reviewable provenance-bearing
   assertion. Agent confidence is not calibrated truth.

Current `video_caption` samples frames and explicitly reports visual-only limits;
current lexical transcript search is not semantic retrieval. Corpus export is the
existing semantic handoff [tree].

## 5. Reduct baseline, current fit, and complementary references

### 5.1 Demonstrated current `educt` capabilities

- Immutable timed source words (`start_ms`/`end_ms`, optional STT confidence) and a
  media-linked `TranscriptBundle`: `transcript.rs:17-27,46-60`.
- Persistent transcripts/layers/exports/renders and source-link detachment:
  `transcript_store.rs:36-71,104-113`.
- Six layer kinds with model/template/time provenance; disjoint speaker-span
  validation: `transcript_layers.rs:27-65,149-165,310-312`.
- Audio/text speaker passes, correction/apply/realignment, paragraphs, semantic
  highlights, researcher highlight/strike annotations, EDL composition/rendering,
  SRT/highlight/corpus exports, bounded lexical search, and gallery/OMC linkage:
  `tools/educt.rs`, `transcript_select.rs`, `transcript_export.rs`, `omc.rs`.
- Researcher annotations are explicitly **not authenticated human receipts**:
  `tools/educt.rs:659-697`.
- The media gallery DB is durable but explicitly unencrypted:
  `hkask_mcp_media.rs:536-546`.
- The face registry uses vision-LLM matching and intentionally stores no local face
  embeddings: `faces.rs:72-82`; it is not a modality-independent person registry.
- Default managed STT remains `OpenRouter/openai/whisper-large-v3-turbo` through
  settings: `hkask-inference/src/model_constants.rs:200-209`. Local voice embedding
  would not make the existing complete transcription pipeline local.

The tests inspected for speaker passes use mocked model outputs; they validate
storage/contracts, not acoustic attribution accuracy. A `SpeakerLayer` comment
calling audio attribution “exact” is not evaluation evidence and should not be used
as a capability claim.

**Working-tree caveat:** uncommitted Phase 1a review-flag code is present
(`transcript_review.rs`, `transcript_export.rs`, `tools/educt.rs`, and
`hkask_mcp_media.rs` modifications). It is work in progress, not a demonstrated
completed feature; no tests were run in this assessment.

### 5.2 Reduct baseline and the class boundary

Reduct and the `zk-ref-open` Reduct model establish transcript-linked navigation and
editing, per-recording state, labeled highlights, and reel composition [S37][S38].
Official Reduct correction materials additionally demonstrate voice-based Speaker
1/2 grouping, rename propagation, paragraph correction, review recommendations, and
section re-transcription [S37]. Vendor accuracy/language claims are not independently
validated here.

The complementary references below add capabilities Reduct does not define for this
study; they should not replace the transcript-as-timeline model.

| Complementary model | Capability and evidence | `educt` seam | Dependencies/conflicts | Do **not** copy |
|---|---|---|---|---|
| WhisperX [S26] | VAD cut/merge + forced phoneme alignment for word timing | Transcription/alignment stage feeding `TimedWord`; candidate realignment comparisons | Language-specific aligners; Python reference conflicts with kask implementation rule | Unbounded ASR rewriting or treating diarization labels as names |
| DIHARD III + NIST SRE24 + NISTIR 8271 [S36][S21][S22] | Condition-specific diarization, variable-duration speaker evaluation, and open-set mated/non-mated methodology | Frozen local evaluation contract and report | Licensed/consented data; exact protocol declaration | Face accuracy as voice evidence; EER alone as open-set product calibration |
| DiarizationLM [S23] | Transcript-preserving speaker reassignment and locality gating | New derived speaker-suggestion layer over immutable words | Task/model-specific fine-tuning and local WDER/cpWER validation | Free-form transcript regeneration or LLM-generated voice embeddings |
| TalkNet + AVA-ActiveSpeaker [S27][S28] | Visible-face active-speaker evidence | Time-aligned audiovisual evidence layer; joins transcript spans only with provenance | Frames, face tracks, domain shift, off-screen speakers | “Visible and speaking” as identity or as always-available ground truth |
| MovieLabs OMC v2.8 [S31] | Media creation graph, identifiers, relationships, schema versioning | Extend existing OMC graph to diarizer/encoder/threshold/model migrations | Avoid conflating production participants, appearing people, and biometric profiles | Treating an OMC person reference as proof of identity |
| VideoAgent + Deep Video Discovery [S29][S30] | Iterative cited retrieval over long media | Bounded agent over lexical/corpus transcript search, clips, and extracted frames | Cost/latency, loop detection, citation correctness, visual-only gaps | QA confidence as fact or autonomous destructive editing |
| ASVspoof 5 [S32] | Spoof/deepfake/adversarial evaluation conditions | Adversarial test track and explicit threat boundary | Suitable lawful synthetic/replay fixtures; separate countermeasure model | Claiming spoof resistance without a measured detector |

## 6. Cross-recording speaker-identification design

### 6.1 Enrollment

1. A host-authorized human selects exact immutable source-word ranges and confirms a
   display name plus the purpose “local cross-recording voice suggestions.” Record
   this as an **operator authorization/confirmation**, not automatically as the
   speaker's legal consent. Whether separate data-subject consent is required is a
   deployment/jurisdiction question [S33].
2. Resolve those words to exact media spans. Retain transcript id, media path and
   content hash where available, gallery/source asset ids, word indices, working vs
   immutable basis, correction/realignment ids, STT model, extraction parameters,
   sample rate/channels, and confirmation/provenance timestamps.
3. Decode and resample before feature extraction. Reject or flag clipping, excessive
   noise/reverberation, codec damage, overlap, and insufficient speech. Use several
   clean clips where possible; derive minimum duration from the development set,
   guided—not fixed—by SRE duration conditions [S21].
4. Candidate non-enrolled audio should be transient by default. Do not silently turn
   every analyzed speaker into a persistent biometric profile.

### 6.2 Voice profiles

Store a stable local profile id, per-segment embeddings, quality/duration metadata,
and optional aggregate only after aggregation is evaluated. Every vector carries:

`model_family + model_version + weights_sha256 + preprocessing_sha256 + dimension +
similarity_metric + extraction_config + calibration_id`.

- Use a **new protected SQLCipher store** or an explicit encrypted migration; the
  existing gallery DB is not encrypted. Resolve credentials through the canonical
  passphrase chain and update the server credential allowlist when implemented.
- Never compare vectors across fingerprints. A model upgrade creates a new namespace
  and requires re-embedding authorized retained source spans plus recalibration; it
  does not inherently require a human to rename every person again.
- Keep candidate embeddings, enrolled embeddings, identity assertions, and human
  confirmation state separate.
- At tens/hundreds of profiles, exact linear scan is a reasonable first hypothesis;
  add ANN indexing only after measured scale/latency need **[inference]**.

### 6.3 Matching

1. Diarize or otherwise obtain reliable recording-local speaker clusters. Existing
   speaker layers may be noisy input; plain VAD spans are insufficient when they can
   contain two speakers.
2. Aggregate enough clean speech within a cluster; compute embeddings with the exact
   enrollment fingerprint. Do not embed isolated words as identity probes.
3. Compare with the model-appropriate metric (cosine for the inspected candidates).
   Require top-1 threshold, runner-up margin, minimum duration, and quality gates.
   Similarity is not a probability; calibrate separately before displaying one.
4. Return visible `unknown` for weak, ambiguous, mixed, overlapped, or unsupported
   evidence. A later overlap-aware model may emit multiple activities, but identity
   should still abstain when the mixed embedding is unreliable.
5. Treat channel/language shift, similar voices, short turns, replay, and voice
   conversion as explicit conditions. No anti-spoofing assurance is made [S32].
6. **Do not auto-enroll every confirmed suggestion.** Name confirmation and adding
   new voice evidence to a profile must be separate actions; otherwise one false
   accept can contaminate a profile and amplify future errors.

Suggested mode (adopted D2): produce a distinct identification-suggestion layer with
transcript/cluster/word-range ids, profile id, score, margin, fingerprint,
calibration id, evidence quality, and `suggested` state. Human confirmation creates
a new researcher/host-authorized assertion; it does not rewrite the acoustic layer.
Automatic assignment would be a separate functional mode that writes names into the
working view at a stricter measured operating point, remains reversible, and is not
adopted under D2.

### 6.4 Correction, deletion, retention, and risk propagation

- **Mistaken name:** support correction at this-span, this-recording, and profile
  scopes. Mark old assertions superseded; preserve the audit trail.
- **Re-enrollment:** add/replace authorized clips, reject outliers, and recalibrate.
  Never overwrite history silently.
- **Deletion:** remove profile id links where required, embeddings, aggregates,
  transient candidates, and derived suggestions in one transaction/workflow; verify
  exports/corpus indexes contain no embeddings. Names in exported prose can still be
  personal data even when they are not voice templates, so export deletion scope is
  a separate product decision.
- **Retention:** D3's memory decay + recall refresh is a useful lifecycle pattern,
  but it is not an absolute retention limit. Add an operator-set maximum age/purpose
  review if required; recurring matches should not make retention indefinite by
  default without that decision.
- **Security/privacy:** purpose limitation, local protected storage, transparency,
  access control, audit, and bystander handling remain necessary. EDPB guidance
  explicitly addresses voice identification, non-registered users, retention, and
  accuracy/bias [S33]. NIST SP 800-63B says voice biometric comparison must not be
  used for authentication; this feature is media analysis, never login or identity
  proofing [S34].

Failure chain to test: diarization merge → mixed embedding → false accept → plausible
name suggestion → human confirmation → contaminated enrollment → more false accepts.
The reverse failure is false rejection/`unknown`, which reduces utility but is safer;
it needs per-condition measurement rather than anecdotes.

## 7. Evaluation plan (proposal; not run)

### 7.1 Dataset and splits

Build a consented local set with recording-session ids, media hashes, reference
transcripts, multi-label overlap, and exact word timings. Include multiple enrolled
speakers and non-enrolled unknowns; near/far microphones, different rooms/devices,
noise/reverberation, codecs, long-form recordings, short turns, high overlap,
multilingual/code-switched speech, similar voices, and lawful replay/conversion
fixtures where feasible.

- **Enrollment:** recording session A.
- **Development/threshold/model selection:** session B, disjoint from A.
- **Final test:** session C, disjoint from A and B, evaluated once after freezing the
  complete configuration. Add sessions rather than splitting clips from one continuous
  recording across these partitions.

### 7.2 Metrics and threshold selection

| Layer | Metrics and protocol |
|---|---|
| Diarization | DER with missed/false-alarm/confusion decomposition; JER where relevant; exact collar/reference-RTTM/overlap policy; per-condition and per-speaker-count results [S36]. |
| Attributed transcript | WDER and cpWER; tcpWER when its concatenation assumptions fit the output. Keep DER distinct from word attribution [S35]. |
| Correction | WER before/after, deletion/insertion rate, word-alignment preservation, long-monologue regressions, and human review burden. |
| Open-set identity | Separate mated/non-mated trials; FNIR/FPIR and DET-style curves; unknown rejection; suggestion coverage and precision among surfaced suggestions; closed-set top-1 only as a secondary diagnostic [S22]. |
| Calibration | Reliability/Brier/ECE only if a probability is emitted; cosine threshold/margin/min-duration are frozen from development data. EER/minDCF may inform verification analysis but are insufficient alone for open-set product selection [S21][S22]. |
| Agent retrieval | Citation correctness, source-word/time alignment, unsupported-answer rate, retrieval loops, latency/cost—not just QA answer similarity [S29][S30]. |

Report **all-trial end-to-end results** and a clean/non-overlap diagnostic subset.
An overlap abstention reduces coverage and counts appropriately in mated-trial FNIR;
it must not disappear from scoring. Use session-level bootstrap confidence intervals
and predeclare the false-accept/false-reject cost tradeoff. No accuracy or feasibility
claim is made until this protocol runs.

Difficult/adversarial strata: `<1 s`, 1–3 s, 3–10 s, `>10 s` speech; multiple
overlap levels; unseen channels; same voice across sessions; unseen speakers;
codec/resampling perturbations including 48→16 kHz; similar voices; replay and voice
conversion. Keep model extraction failures and `unknown` visible as outcomes.

## 8. Prioritized adjacent-feature roadmap

### 8.1 Quick research prototypes / prerequisites

| Priority | Candidate | User value / evidence / fit | Effort, dependency, risk | Relation to identity |
|---|---|---|---|---|
| 0 | Correct governance/storage/evaluation specifications | Prevents biometric, consent, and inflated-result failures; highest evidence strength | New protected store decision; corrected preregistration before seal | **Blocking** |
| 0 | Acoustic diarization bake-off: Nemotron 3 native C++ vs CAM++/cluster candidate; external pyannote/DiariZen only as research comparators | Supplies the recording-local clusters identity needs; tests overlap/language/long-form directly | Target-hardware RTF/memory and DER/JER unknown; model terms review | **Blocking measurement** |
| 1 | Review-confidence markers, labeled uncalibrated | Reduct-grounded review value; low integration effort; Phase 1a WIP exists | STT/LLM confidence may be uninformative; calibrate before numeric presentation | Accompanies enrollment quality review |
| 1 | Bounded short-span speaker post-pass | DiarizationLM evidence; preserves immutable text | Replace one-recording spot-check with frozen WDER/cpWER comparison and long-monologue guard | Cleans clusters before embedding |
| 1 | Section re-transcribe → candidate correction/realignment | Reduct-grounded; uses existing correction seam | Conflicts/timing reconciliation; no source mutation | Improves transcript without voice identity |
| 1 | Typed overlap/evidence representation | Prevents silent mixed-speaker attribution | Current single-layer spans are disjoint; multi-label semantics and migration needed | **Prerequisite for honest overlap handling** |
| 2 | Session-disjoint embedding/open-set probe | Directly measures the central unknown before full UI/storage build | Exact model fingerprints, unknown speakers, threshold/margin/dev/test protocol | **Go/no-go for identity** |
| 2 | Cited cross-recording transcript/frame retrieval | Strong agentic-video fit and user value without biometrics | Corpus/lexical seams, citation oracle, budget/loop controls | Can precede identity independently |

### 8.2 Longer-term candidates

1. Audiovisual active-speaker evidence (TalkNet/AVA) for visible-speaker disambiguation;
   not identity and not always available.
2. Long-form drift tracking, cross-chunk speaker reconciliation, and streaming
   diarization with measured latency/identity stability.
3. Spoof/replay/voice-conversion detection as a separately evaluated countermeasure.
4. A canonical person entity joining human-confirmed voice and face evidence; never
   infer that a `face_id` and voice profile are the same person automatically.
5. Versioned model migration/re-embedding, calibration rollback, and retention
   administration.

## 9. Re-review of updated `educt-speaker-identity-plan-2026-10-27.md`

D1 adopt, D2 suggest-only, D3 voice-as-memory lifecycle, and D4 cheap-wins ordering
are recorded operator decisions; this review does not silently reopen them. The
following corrections are required for factual/technical integrity.

| Severity | Finding and evidence | Required disposition |
|---|---|---|
| **Critical** | Plan §5.2/T3 calls tables beside `face_registry` SQLCipher-protected. Current media DB is explicitly unencrypted and deliberately does not receive `HKASK_DB_PASSPHRASE` (`hkask_mcp_media.rs:536-546`). The registered variant repeats the false storage premise. | Specify a new protected store or encrypted migration and credential-path change before Phase 2. Correct the experiment genotype before any seal. |
| **Critical** | Plan §0/§5.1/§5.5 and goal criteria equate annotation/naming with human/data-subject consent. `educt_annotation_create` explicitly says researcher-supplied, agent-mediated, not authenticated human identity (`tools/educt.rs:659-697`). | Keep D3 lifecycle, but call this an operator authorization record. Decide the actual consent/authority UX by deployment; do not claim legal/ethical compliance from the existing tool. |
| **High** | Registered eval excludes overlap-flagged spans from identification errors. This hides a required hard condition and inflates end-to-end performance. | Score all trials; report clean diagnostic separately; count abstention/coverage correctly. |
| **High** | Plan §8 uses a dev set for threshold pinning/reporting and lacks a frozen final test. CAM++ is the only registered “baseline”; no majority or simpler comparator/fitness exists. | Add session-disjoint dev/test and a real comparator, or redesign honestly as single-arm calibration with scientific headroom. No seal yet. |
| **High** | “Existing annotation and speaker-label tools” do not provide typed suggestion confirmation/rejection, and one `SpeakerLayer` rejects overlap. | Add distinct suggestion/confirmation semantics and explicit overlap/evidence representation; do not mutate human/acoustic layers. |
| **High** | Correcting a suggestion “may enroll” automatically (§5.3), enabling false-accept profile contamination. | Separate name confirmation from enrollment/profile updates; require an explicit authorized enrollment action. |
| **Medium** | Meetily is called “proven”/the practical SOTA path. The PR is useful prototype evidence with self-reported build/tests, an initial unchecked manual-validation item, a maintainer-reported label problem, and later overlap commits [S20]. | Downgrade to prototype reference; evaluate independently. Do not generalize its initial VAD limitation to its later commits without qualification. |
| **Medium** | “A model change means re-enrollment” is unnecessarily broad and cross-version incomparability is absolute. | Require fingerprint separation and recalibration; re-embed retained authorized spans where possible. Compatibility may only be claimed with artifact evidence and local validation. |
| **Medium** | Memory decay is not an absolute retention limit; derived name/score layers are called “non-biometric,” obscuring personal-data/export questions [S33]. | Add explicit maximum-retention and deletion-scope decisions; call suggestion layers non-template personal data, not risk-free data. |
| **Medium** | Plan status says no implementation code, but uncommitted Phase 1a files now exist. No validation was run here. Section 1c task id also differs from the live board: live `bf891388-20ea-457b-b7fb-e424c57a9c4a`. | Refresh execution state without counting WIP as delivered; correct the id. If the plan changes, use goal supersession rather than silently mutating the immutable spec. |
| **Low** | Cloud alternatives are excluded by “D2's local-first consequence,” but D2 is suggest-only; local-first comes from the goal/platform constraint. | Correct the cross-reference. |
| **Evidence update** | §10 still lists SRE24 full text and ECAPA as open gaps. The newer ledger inspected the official SRE24 plan and ECAPA paper; NISTIR 8271, ASVspoof 5, and the 2025 multi-talker WER paper also now supply stronger evaluation evidence. | Refresh references/gaps; retain SpeakerLM deployment and CoVoGER full-text limitations honestly. |

Phase 0 existence checks succeeded: the goal and experiment records exist; the
experiment has zero fitness records and no seal. The live board shows review flags
in progress and the other Phase 1 tasks backlog. These are execution facts, not
feature acceptance.

## 10. Limitations, inaccessible sources, conflicts, and open decisions

- No independent matched-protocol head-to-head covers Nemotron 3, Community-1,
  DiariZen/cVBx, LS-EEND, and Precision-3. Vendor/model-card figures remain
  condition-specific and are not universal rankings.
- CoVoGER's official abstract/metadata was inspected, not its full paper. The PMC
  short-duration article yielded only a cookie shell in one extraction path. A
  supplied PLOS DOI was topically unrelated (facial/humerus robotics rather than
  speaker recognition) and was excluded. These are coverage gaps, not claims of
  absence.
- Community-1's raw model-card URL now requires authentication (HTTP 401); its public
  model page had previously been inspected. Gating itself is an adoption cost.
- Multilingual/code-switching open-set identity, very short probes, and long-form
  speaker drift remain thin evidence areas for the exact local candidate stack.
- Meetily's PR contains evolving overlap work; static summaries of its first revision
  are not a current limitation census.
- NISTIR 8271 is a **face** identification report used solely for open-set evaluation
  practice, never as voice accuracy evidence.
- No local runtime throughput, model-size-in-memory, license compatibility, accuracy,
  calibration, or anti-spoofing result has been measured for `educt`.

Open functional decisions for the operator:

1. Does enrollment record only operator authorization, or must the UX capture
   speaker/data-subject consent for the deployment context?
2. What absolute retention limit and bystander policy accompany memory-style decay?
3. Does deletion cover only templates/suggestions, or also names in exports, reels,
   corpus derivatives, and backups?
4. May audio used for enrollment pass through the existing cloud STT/audio speaker
   passes, or must the complete identity pipeline be local?
5. Which false-accept versus false-reject cost is acceptable for suggestions, and
   after what measured evidence could D2 ever be revisited?
6. Are OpenMDW 1.1 and the exact chosen encoder weight/data terms acceptable?

## 11. Bibliography

All entries below are cited above. “Inspected” means the underlying artifact was
read; “metadata” and “vendor” retain their limits.

- **[S1]** T. J. Park, N. Kanda, D. Dimitriadis, K. J. Han, S. Watanabe, S. Narayanan (2022). *A Review of Speaker Diarization: Recent Advances with Deep Learning*. Computer Speech & Language 72. https://arxiv.org/abs/2101.09624 — peer-reviewed review; inspected.
- **[S2]** D. Snyder, D. Garcia-Romero, G. Sell, D. Povey, S. Khudanpur (2018). *X-Vectors: Robust DNN Embeddings for Speaker Recognition*. ICASSP. https://doi.org/10.1109/ICASSP.2018.8461375 — paper/DOI; inspected.
- **[S3]** F. Landini, J. Profant, M. Díez, L. Burget (2021). *Bayesian HMM clustering of x-vector sequences (VBx) in speaker diarization*. Computer Speech & Language. https://doi.org/10.1016/j.csl.2021.101254 — paper/DOI; inspected.
- **[S4]** Y. Fujita, N. Kanda, S. Horiguchi, Y. Xue, K. Nagamatsu, S. Watanabe (2019). *End-to-End Neural Diarization*. https://arxiv.org/abs/1909.05952 and https://arxiv.org/abs/2003.02966 — research papers; inspected.
- **[S5]** S. Horiguchi, Y. Fujita, S. Watanabe, Y. Xue, K. Nagamatsu (2021/2022). *Encoder-Decoder Based Attractors for End-to-End Neural Diarization*. https://arxiv.org/abs/2106.10654 — research paper; inspected.
- **[S6]** I. Medennikov et al. (2020). *Target-Speaker Voice Activity Detection: a Novel Approach for Multi-Speaker Diarization in a Dinner Party Scenario*. Interspeech. https://www.isca-archive.org/interspeech_2020/medennikov20_interspeech.html — peer-reviewed paper; inspected.
- **[S7]** T. J. Park, N. R. Koluguri, J. Balam, B. Ginsburg (2022). *Multi-scale Speaker Diarization with Dynamic Scale Weighting*. https://arxiv.org/abs/2203.15974 — research paper; inspected.
- **[S8]** H. Bredin (2023). *pyannote.audio 2.1 speaker diarization pipeline: principle, benchmark, and recipe*. Interspeech. https://doi.org/10.21437/Interspeech.2023-105 — peer-reviewed paper/official artifacts; inspected.
- **[S9]** pyannote / H. Bredin (2025). *speaker-diarization-community-1 model card*. https://huggingface.co/pyannote/speaker-diarization-community-1 — official model card; inspected; gated raw access disclosed.
- **[S10]** BUT Speech@FIT maintainers (2025–2026). *DiariZen repository and tutorial*. https://github.com/BUTSpeechFIT/DiariZen and https://arxiv.org/abs/2604.21507 — official repository/paper; inspected.
- **[S11]** NVIDIA (release 2026-09-23). *Nemotron 3 Diarization model card*. https://huggingface.co/nvidia/nemotron-3-diarization — official model card; inspected. T. Park et al., *Sortformer*, https://proceedings.mlr.press/v267/park25h.html — peer-reviewed background; inspected.
- **[S12]** D. Liang, X. Li (2024/2025). *LS-EEND: Long-Form Streaming End-to-End Neural Diarization with Online Attractor Extraction*. https://arxiv.org/abs/2410.06670 and https://doi.org/10.1109/TASLPRO.2025.3597446 — paper/DOI; inspected.
- **[S13]** L. A. Lanzendörfer, F. Grötschla, C. Blaser, R. Wattenhofer (2025-09-30). *Benchmarking Diarization Models*. https://arxiv.org/abs/2509.26177 — independent research preprint; full text inspected.
- **[S14]** pyannote.ai (accessed 2026-10-27). *Precision-3*. https://www.pyannote.ai/precision-3 and https://www.pyannote.ai/blog/precision-3 — official vendor claims; inspected, not independently validated.
- **[S15]** B. Desplanques, J. Thienpondt, K. Demuynck (2020). *ECAPA-TDNN: Emphasized Channel Attention, Propagation and Aggregation in TDNN Based Speaker Verification*. Interspeech. https://arxiv.org/abs/2005.07143 — peer-reviewed paper; inspected.
- **[S16]** SpeechBrain maintainers (accessed 2026-10-27). *spkrec-ecapa-voxceleb model card*. https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb — official model card; inspected.
- **[S17]** H. Wang et al. (2023). *WeSpeaker: A Research and Production Oriented Speaker Embedding Learning Toolkit*. ICASSP; https://github.com/wenet-e2e/wespeaker. k2-fsa maintainers, *sherpa-onnx Speaker Identification*, https://k2-fsa.github.io/sherpa/onnx/speaker-identification/index.html — official toolkit/docs; inspected.
- **[S18]** N. R. Koluguri, T. Park, J. Balam, B. Ginsburg (2021). *TitaNet*. https://arxiv.org/abs/2110.04410; NVIDIA, *TitaNet-Large model card*, https://huggingface.co/nvidia/speakerverification_en_titanet_large — paper/model card; inspected.
- **[S19]** E. Di Giacinto / LocalAI (updated 2026-10-22). *Voice Recognition documentation*. https://localai.io/docs/features/voice-recognition/index.html — official documentation; inspected.
- **[S20]** KrtinShet / Zackriya Solutions (2026-06, accessed 2026-10-27). *meetily PR #511: local speaker identification with persistent voice profiles*. https://github.com/Zackriya-Solutions/meetily/pull/511 — official prototype PR; inspected.
- **[S21]** NIST ITL/IAD Multimodal Information Group (2024-10-07). *NIST 2024 Speaker Recognition Evaluation Plan*. https://www.nist.gov/document/sre24-evaluation-plan — official evaluation plan; inspected.
- **[S22]** P. Grother, M. Ngan, K. Hanaoka (2019-09). *NISTIR 8271: FRVT Part 2, Identification*. https://doi.org/10.6028/NIST.IR.8271 — official evaluation methodology; inspected; cross-modality use only.
- **[S23]** Q. Wang, Y. Huang, G. Zhao, E. Clark, W. Xia, H. Liao (2024; v13 2026-10-11). *DiarizationLM: Speaker Diarization Post-Processing with Large Language Models*. Interspeech 2024 / arXiv. https://doi.org/10.21437/Interspeech.2024-209 and https://arxiv.org/abs/2401.03506 — peer-reviewed paper and later preprint; inspected separately.
- **[S24]** R. Ma, M. Qian, M. Gales, K. Knill (2024). *ASR Error Correction using Large Language Models*. https://arxiv.org/abs/2409.09554 — research paper; inspected.
- **[S25]** Z. Yang, Z. Wan, S. Li, C.-H. H. Yang, C. Chu (2025). *CoVoGER: A Multilingual Multitask Benchmark for Speech-to-text Generative Error Correction with Large Language Models*. EMNLP. https://doi.org/10.18653/v1/2025.emnlp-main.320 — official abstract/metadata only.
- **[S26]** M. Bain, J. Huh, T. Han, A. Zisserman (2023; repository accessed 2026). *WhisperX: Time-Accurate Speech Transcription of Long-Form Audio*. https://arxiv.org/abs/2303.00747 and https://github.com/m-bain/whisperX — paper/official repository; inspected.
- **[S27]** R. Tao, Z. Pan, R. K. Das, X. Qian, M. Z. Shou, H. Li (2021). *Is Someone Speaking? Exploring Long-term Temporal Features for Audio-visual Active Speaker Detection*. ACM MM. https://doi.org/10.1145/3474085.3475587 and https://github.com/TaoRuijie/TalkNet-ASD — paper/repository; inspected.
- **[S28]** Google Research (accessed 2026-10-27). *AVA ActiveSpeaker download/documentation*. https://research.google.com/ava/download.html#ava_active_speaker_download — official dataset documentation; inspected.
- **[S29]** X. Wang, Y. Zhang, O. Zohar et al. (2024). *VideoAgent: Long-form Video Understanding with Large Language Model as Agent*. https://arxiv.org/abs/2403.10517 — research paper; inspected.
- **[S30]** X. Zhang, Z. Jia, Z. Guo, J. Li, B. Li, H. Li, Y. Lu (2025). *Deep Video Discovery: Agentic Search with Tool Use for Long-form Video Understanding*. NeurIPS 2025. https://arxiv.org/abs/2505.18079 and https://github.com/microsoft/DeepVideoDiscovery — paper/official repository; inspected.
- **[S31]** MovieLabs (2025). *Ontology for Media Creation (OMC), v2.8*. https://github.com/MovieLabs/OMC — official ontology/repository; inspected.
- **[S32]** X. Wang et al. (2026-01-07). *ASVspoof 5: Evaluation of Spoofing, Deepfake, and Adversarial Attack Detection Using Crowdsourced Speech*. https://arxiv.org/abs/2601.03944 — research paper; inspected.
- **[S33]** European Data Protection Board (adopted 2021-07-07). *Guidelines 02/2021 on virtual voice assistants, v2.0*. https://www.edpb.europa.eu/documents/guideline/guidelines-022021-on-virtual-voice-assistants_en — official guidance; inspected.
- **[S34]** NIST (2025-08-26 page). *SP 800-63B-4: Authentication and Authenticator Management*. https://pages.nist.gov/800-63-4/sp800-63b.html — official guideline; inspected.
- **[S35]** T. von Neumann, C. Boeddeker, M. Delcroix, R. Haeb-Umbach (2025-08-04). *Word Error Rate Definitions and Algorithms for Long-Form Multi-Talker Speech Recognition*. https://arxiv.org/abs/2508.02112 — research paper; inspected.
- **[S36]** N. Ryant, K. Church, C. Cieri, J. Du, S. Ganapathy, M. Liberman (2020). *Third DIHARD Challenge Evaluation Plan*. https://arxiv.org/abs/2006.05815 and https://dihardchallenge.github.io/dihard3/ — official evaluation plan/challenge; inspected.
- **[S37]** Reduct.Video (accessed 2026-10-27). *Correcting AI Transcripts*, *Video Text Interface*, and *Product & Features*. https://help.reduct.video/en/articles/correct-ai-transcript, https://help.reduct.video/en/articles/video-text-interface, https://reduct.video/product/ — official product/help artifacts; inspected; vendor claims bounded.
- **[S38]** `zk-ref-open` (local sealed corpus, accessed 2026-10-27). *Reduct transcript model*, entity `corpus:zk-ref-open::fine:utf8-7265647563742d7472616e7363726970742d6d6f64656c2e6d64` — inspected corpus reference model.
- **[S39]** H. Yin et al. (2026). *SpeakerLM: End-to-End Versatile Speaker Diarization and Recognition with Multimodal Large Language Models*. AAAI-26. https://doi.org/10.1609/aaai.v40i40.40745 — bibliographic identity only; artifact not established.
- **[S40]** Y. Chen et al. (2024). *ERes2NetV2: Boosting Short-Duration Speaker Verification Performance with Computational Efficiency*. Interspeech. https://arxiv.org/abs/2406.02167 — metadata/official paper identity; no deployment claim.

## 12. Explicit recommendation

Proceed with the **measurement and governance prerequisites**, not with identity UI
as the first build. Preserve Reduct's transcript-linked model; add acoustic evidence,
constrained suggestions, and open-set evaluation as separate layers. Correct the
updated plan/goal/experiment premises listed in §9 before Phase 2 or sealing. The
central unknown is not whether an embedding can be computed locally—it plainly can
in external artifacts—but whether a chosen diarizer+encoder+threshold pipeline is
useful on the operator's session-disjoint media with acceptable open-set errors,
retention, and review burden. That remains unmeasured.
