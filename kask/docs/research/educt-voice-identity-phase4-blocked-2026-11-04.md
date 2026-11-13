---
title: "educt voice-identity Phase 4 measurement — blocked: the frozen eval set does not exist yet (2026-11-04)"
audience: [developers, operators]
last_updated: 2026-11-04
version: "1.0.0"
status: "Active"
domain: "Media / educt"
mds_categories: [lifecycle, trust]
---

# educt voice-identity Phase 4 measurement — blocked: the frozen eval set does not exist yet (2026-11-04)

Experiment `exp_1d2d6140796a481eba7deb63fbfaf1f2` (layer scientific_hypothesis,
status running, zero fitness records since registration 2026-10-28), baseline
variant `var_2caa24f72efd4059adbf10c70b22f3de` (CAM++ threshold+margin open-set
matching, suggest-only). Operator ruling 2026-11-04 (algedonic review, card
`bfba0e8d-bbdd-4f9e-aad7-f839ea8db36d`): *resume — try encoding dunning's
voice*. This report is the Phase 4 measurement attempt that ruling asked for.

**Result:** the enrollment half of the frozen protocol is now real and verified;
the measurement half cannot run, because the frozen eval set's probe material
does not exist in the operator's stored media. Per the mission boundary and the
plan's completion gate ("No accuracy claim about `educt` voice identity may
exist anywhere before this phase reports", plan §7 Phase 4), this report
records the exact gap instead of forcing a pass. No accuracy claim is made; the
registered prediction remains untested, not refuted.

## 1. The frozen protocol (what a pass requires)

Plan §8 (`kask/docs/plans/educt-speaker-identity-plan-2026-10-27.md`, deleted
from the tree 2026-11-01 in commit `68324a5941`; readable at
`git show 68324a5941^:kask/docs/plans/educt-speaker-identity-plan-2026-10-27.md`)
plus the experiment's `eval_set`, frozen 2026-10-27 before any measurement:

| Requirement (frozen) | Measured 2026-11-04 |
|---|---|
| ≥2 enrolled speakers, each with human-confirmed word ranges | **1 of 2** (Dunning) |
| Multiple human-confirmed word ranges per speaker | **1 range** (the enrollment authorization itself) |
| ≥2 probe recordings disjoint from the enrollment session | **0 of 2** |
| ≥1 non-enrolled unknown speaker as probes | **0 of 1** |
| EER/minDCF operating-point selection; per-condition breakdowns (duration buckets, quality, channel); overlap spans scored unknown/mixed; session-level 95% bootstrap CI, ties within CI | **not computable** — no genuine/impostor score distributions exist |

## 2. Measured live state (all verified 2026-11-04)

### Enrollment — done, real

- Profile `9100bf24-0b01-48b1-8104-478fe2077194`, speaker "David Dunning",
  operator-authorized 2026-11-04T17:16:35Z over transcript
  `5509bb94-6a9e-41b3-afb9-512e4db618e1`, source words 2889–2961
  (audio span 1040.430–1064.069 s of `generated/dunning-2017-audio.mp3`,
  23.64 s) — the authorization is the consent-record human act (plan §5.5).
- Encoder fingerprint (live `voice_list`): model_family
  `wespeaker-voxceleb-campp`, weights_sha256
  `c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef`,
  512-dim, preprocessing `16khz-mono-s16le;kaldi-fbank-80;25ms-10ms;dither-0;snip-edges-false;int16-scale`.
  Weights re-verified on disk this pass:
  `~/Documents/zk-data/media-mcp/models/wespeaker_en_voxceleb_CAM++.onnx`
  sha256 `c46fad10…` — matches the live fingerprint and the sherpa-onnx
  release checksum (plan §12, 2026-10-28).

### Probe and unknown material — the gap

- **The Dunning material in the store is ONE session**: transcript
  `5509bb94` (14,270 words, 4,963.71 s) over `generated/dunning-2017-audio.mp3`,
  sourced from the 4,963.72 s video `5c4bfbb5-c0c2-42dd-a315-769a6477cefe.mp4`
  (identical duration; gallery asset
  `80f804a6-f5da-4119-bcfc-292403b3dd61`, `dunning-knowing-yourself-2017.mp4`).
  **Finding (anticipated by the mission): Dunning cannot participate in the
  identification test without violating session-disjointness.** Any probe
  drawn from the lecture shares the enrollment session; the frozen protocol
  forbids it, and the protocol was not bent.
- Stored transcripts: **2 total**. (a) the Dunning lecture; (b) the 15 s mic
  smoke WAV `generated/167bd383-2aa8-4e66-a362-c43bc5249c06.wav` (7 words;
  energy re-measured this pass: mean −29.3 dB, max 0.0 dB — near-silence, the
  documented STT-fabrication specimen; its only researcher annotation is the
  "smoke-leg" reel highlight, not speaker ground truth). Neither provides
  probe material.
- **Human-confirmed speaker ground truth system-wide: exactly one range** —
  the Dunning enrollment authorization above. `educt_annotation_list` on the
  Dunning transcript returns 0 annotations. Its two speaker layers
  (`389da7f3`, `80c54988`) are model passes (claude-sonnet-5.5, span
  confidences 0.3–0.95) and are correctly excluded as ground truth: the thing
  under test cannot be its own oracle.
- Other stored audio: ~20 untranscribed files in `generated/` (largest: the
  62-min lecture `08d8d962-91bd-4ff3-a2b2-e9599987a487.mp4`, real speech energy
  — mean −25.5 dB at the 25-min sample — presenter unidentified; no
  transcript, no word timings, no operator-confirmed speaker). None can serve
  as probes or unknowns without transcription plus operator confirmation.

## 3. Why no scorecard and no harness were produced

- The fitness_fn ("suggest-only operating point scorecard": top-1
  identification accuracy at the false-accept operating point selected from
  EER/minDCF analysis, unknown-rejection rate at the same operating point,
  per-condition breakdown, session-level 95% bootstrap CI with ties within
  CI) has no computable inputs. With 1 enrolled speaker, 0 session-disjoint
  probes, and 0 unknown speakers there are no genuine or impostor score
  distributions, the majority-speaker baseline is degenerate at N=1, and the
  margin-over-runner-up rule has no second enrolled voice to separate.
- The offline §5.3/§8 harness (CAM++ embeddings for enrollment and probe
  segments, cosine similarity, threshold+margin matching, EER/minDCF
  selection, per-condition breakdowns, bootstrap CI) was **not** built in this
  pass: with zero probe material it would be unverifiable surface — no
  runnable path exists to exercise EER/minDCF against real data, and a
  capability is the capability functioning (operator ruling 2026-09-09). The
  encoder seam it would reuse is live and verified (§2). The harness is
  specified and unblocked the moment material exists.
- Threshold provenance as it stands: no local operating point exists. The
  §5.3 starting parameters (cosine distance threshold ≈0.25–0.30, margin
  ≈0.05 over runner-up, minimum probe duration unset) remain uncalibrated
  starting points, not chosen operating points; the encoder fingerprint is
  the only threshold-provenance element that is real today.

## 4. What unblocks the measurement (operator acts, in protocol order)

1. **A second enrolled speaker**: `voice_enroll` is operator-authorized
   enrollment — a human-confirmed word range on any stored recording of a
   second speaker (the operator's own voice included), ideally several ranges
   per speaker per the frozen enrollment structure.
2. **Session-disjoint Dunning probes**: any Dunning recording from a
   different session (e.g., another Dunning lecture), then transcribe it
   (word timings) and have the operator confirm Dunning spans for probe
   segmentation and ground truth.
3. **Unknown probes**: operator-confirmed speech of a non-enrolled speaker
   (a third voice, or any confirmed non-Dunning speech).

The unidentified 62-min lecture (`08d8d962`) is the only substantial
untranscribed speech in the store; if the operator identifies its presenter,
it becomes candidate material for (2) or (3) after transcription and
confirmation.

## 5. Statuses recorded alongside the gap

- **suggest-only preserved**: the deployed `voice_identify` emits a distinct
  identification layer (matched profile id, speaker name, cosine similarity,
  top-1-minus-runner-up margin) and has no auto-apply path — no match is
  auto-confirmed and human-confirmed layers are never mutated (tool contract,
  `kask/mcp-servers/hkask-mcp-media/src/tools/voice.rs` L296–300; Phase 2/3
  landed in commits `9a62f6a0bc`, `41c95ecd07`, `77fffe2131`, `207466f90c`).
- **Goal link dangling**: the experiment's `linked_goal_id`
  `2294d470-63be-4fe1-8dfe-3aa61076ce97` no longer resolves — the goal store
  returns empty (goal created 2026-10-27 per plan §12, since deleted). The
  goal loop's judgment/Brier chain is inert for this experiment until a goal
  exists. Not repaired here: goal intake requires the operator's confirmed
  source, and none was invented.
- **Seal: none.** P2 requires a measured baseline pass rate to attest
  headroom; a void measurement cannot attest anything, so the experiment
  stays unsealed — consistent with the board's recorded position that the
  seal lands only with the first real measurement.
- **Rubric status** (the Phase 4 report contract): `operating_point_scorecard`
  blocked (no computable metrics); `threshold_provenance_recorded` blocked
  beyond the encoder fingerprint (no chosen threshold/margin/min-duration
  exists); `suggest_only_preserved` verified true; `ledger_grounded_report`
  satisfied — this report cites ledger run `e99d0ff4045bfb64` and the plan §8
  protocol it executed.
- **v2-design lineage**: the board's Phase 3 prerequisite task (`5ffe9726`)
  recorded a corrected evaluation design
  (`educt-speaker-identity-evaluation-design-v2-2026-10-28.md`, also deleted
  from the tree; its registry attempt failed 2026-10-29 with
  `registry_schema_mismatch` — the manifest/executor do not exist). The
  2026-11-04 operator ruling resumed *this* experiment and its frozen §8
  contract; the v2 lineage remains available to the review if it prefers
  re-registration over measuring on the frozen contract.

## 6. Ledger grounding

Research ledger run `e99d0ff4045bfb64` (completed 2026-10-28) — the plan's
source study; every §11 reference it rests on is recorded server-side with
excerpt copies: LocalAI's threshold/margin starting points and
encoder-fingerprint discipline [10], the meetily CAM++ pipeline and the
resample-before-fbank invariant [15], NIST SRE metrics practice [19][20], and
the sherpa-onnx CAM++ release [11]. This report grounds its protocol claims
in plan §8 (frozen at registration) and that ledger run.

## 7. What was recorded on the experiment

One fitness record (run_count 1, seal_id null, budget 1 of 12) referencing
this report as the readiness-measurement artifact, with scores naming the
measured readiness (1/2 enrolled speakers, 0/2 session-disjoint probes, 0/1
unknown speakers) and every protocol metric explicitly `not_computable`. No
other records were made. The resolution verdict (`selection_record` /
`scientific_outcome` — e.g. `measurement_void`, rejection with reasons, or
extension with new material) is governance: the operator's or the algedonic
review's, never this worker's.