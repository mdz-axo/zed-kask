---
title: "educt speaker identity — evaluation DesignSpec v2 (declaration-blocked)"
audience: [operator, architects, developers]
last_updated: 2026-10-28
version: "2.0.0"
status: "Proposed — design only, no measurements, registry declaration, seal, or accuracy claim"
domain: "Media / transcript capabilities"
mds_categories: [domain, trust]
---

# educt speaker identity — evaluation DesignSpec v2 (declaration-blocked)

Status: **design only; no measurements, registry declaration, seal, or accuracy claim**.
Research cutoff inherited from the source assessment: 2026-10-27. This revision was
authored 2026-10-28 without inspecting confirmatory trial outcomes.

## 1. Lineage and correction reason

- Goal: `2294d470-63be-4fe1-8dfe-3aa61076ce97`.
- Prior unsealed experiment: `exp_1d2d6140796a481eba7deb63fbfaf1f2`.
- HypothesisSpec: `educt-voice-identity-hspec-v2` (`hypothesis-spec-v1`).
- DesignSpec: `educt-voice-identity-design-v2` (`design-spec-v1`).
- Source candidates: `three_session_confirmatory` (selected),
  `single_arm_calibration`, and `same_session_cross_validation`.
- Evidence: research run `e99d0ff4045bfb64`,
  `educt-speaker-identity-plan-2026-10-27.md`, and
  `educt-research-design-assessment-2026-10-27.md` §7 and §9.

This revision corrects two defects in the prior registration: overlap-flagged
trials must not disappear from identification errors, and a development-only
report with CAM++ as the sole arm cannot test comparative transport. The prior
experiment remains unsealed with zero fitness records; this document does not
resolve it. Its disposition belongs to the algedonic review after a replacement
is declaration-ready.

## 2. Hypothesis traceability

| Hypothesis or rival | Distinct predicted observation | Falsifying or limiting observation |
|---|---|---|
| H1: local CAM++-class threshold+margin matching is useful for suggest-only identification | On frozen final sessions C1..Cn, CAM++ beats the majority baseline and attains unknown-probe false-accept <10% with true-match retention ≥80% | The joint criterion is contradicted with adequate precision |
| H0 / majority rival | Majority-speaker assignment performs at least as well as encoder matching | CAM++ exceeds majority under the frozen final analysis |
| Domain-shift rival | Development gains fail to transport to disjoint sessions | Per-session final results retain the operating criterion |
| Overlap-contamination rival | Overlap changes end-to-end outcomes and coverage materially | All-trial and clean diagnostic results show no material difference |

The negative control is within-session label permutation. A material departure
from its expected null behavior triggers a leakage/measurement audit before any
scientific interpretation; it is not treated as a second scientific claim.

## 3. Selected design

### 3.1 Units, partitions, and arms

- Unit of independence: **recording session**, not an individual clip.
- Enrollment, development, and final probes are disjoint by session. Clips from
  one continuous recording may not be split across partitions.
- Required population at declaration: at least two development sessions and at
  least three independent final sessions, at least two enrolled speakers, and
  at least one non-enrolled unknown speaker.
- Feedback/development partition: sessions A/B only, for encoder,
  threshold/margin, duration-floor, and diagnostic selection.
- Selection/final partition: sessions C1..Cn (n≥3), evaluated once after the
  complete configuration is frozen. No final outcomes have been inspected.
- Predeclared arms: deterministic majority-speaker baseline (baseline), pinned
  WeSpeaker CAM++ ONNX threshold+margin (primary), and pinned short-duration
  ERes2NetV2 ONNX threshold+margin (deployable challenger). Exact weights and
  preprocessing fingerprints must be recorded before declaration.

### 3.2 Measurement model and overlap policy

Each probe receives an identity outcome or `unknown` plus model/threshold
provenance. Reference labels carry session, speaker, exact span, duration,
channel/quality condition, and multi-label overlap.

**All trials remain in end-to-end scoring.** Overlap is an explicit stratum and
a clean/non-overlap subset is diagnostic only:

- a mated/target abstention or wrong enrolled identity counts against target
  retention and top-1 accuracy;
- a non-mated/unknown probe assigned any enrolled identity counts as a false
  accept;
- overlap abstentions reduce coverage and count as target misses where mated;
- extraction failures and missing reference labels remain visible outcomes or
  protocol deviations, never silent exclusions;
- no clean-subset result may be presented as the all-trial result.

Report top-1 accuracy, target false-accept/false-reject or FNIR/FPIR as defined
in the scoring harness, unknown false-accept/rejection, coverage, and per-
condition outcomes for duration, overlap, noise, and channel. Diarization DER or
transcript WDER/cpWER are separate component metrics if those stages are added;
they are not identity metrics.

### 3.3 Identification contrast and auxiliary failures

The identifying contrast is the same frozen final probes scored by majority,
CAM++, and ERes2NetV2 under their development-frozen configurations. This is a
computational benchmark, not a causal estimate about people or populations.
Transport is limited to the declared languages, channels, speakers, and
recording conditions.

The auxiliary ledger covers encoder calibration, label leakage, reference-label
error, overlap segmentation, model misspecification, and session/channel shift.
A failed negative control or manifest-integrity check localizes a measurement
problem; it does not exonerate the focal hypothesis.

### 3.4 Information justification and preregistered analysis

Use a fixed-seed, 10,000-resample session-cluster percentile bootstrap over the
pooled final sessions, resampling whole sessions and recording the seed, for
confidence intervals only. CAM++ versus majority is primary; ERes2NetV2 versus
majority is secondary.

For each arm, compute per-session `Δs = top-1 arm,s − top-1 majority,s` and the
paired statistic `T = Σs nsΔs`, where `ns` is the session's final-probe count.
The paired null is zero arm effect: conditional on each session's absolute
contribution, its sign is independently equiprobable because paired outcomes
are sign-exchangeable within that session. This is an explicit assumption, not
a consequence of the majority rival; dependence that violates it invalidates
the p-value analysis and yields `measurement_void`, while descriptive outcomes
remain reportable.

Test the positive alternative with a sign-flip permutation of each session's
`nsΔs`. Enumerate all `2^n` sign assignments when n≤12; otherwise draw 10,000
assignments with the recorded fixed seed. For observed `Tobs`, the one-sided
p-value is `(1 + count(Tperm ≥ Tobs)) / (1 + N)`, with ties included in the
numerator and `N` the number of sign assignments. Holm-adjust the two arm
p-values. Use one-sided 95% bootstrap bounds for the operating criteria and
report two-sided 95% intervals plus every per-session result.

Precision is adequate only when at least three final sessions are present and
the two-sided 95% interval widths are ≤0.10 for the CAM++−majority top-1
difference, unknown-probe false-accept, and true-match retention. Fewer sessions,
wider intervals, a failed resampling contract, or missing final strata is
`inconclusive`, never support or refutation. No power claim is made before the
real manifest exists.

With adequate precision, the joint H1 criterion is:

1. Holm-adjusted one-sided p<0.05 for CAM++ improving top-1 over majority;
2. the one-sided 95% upper bound for unknown-probe false-accept is <0.10; and
3. the one-sided 95% lower bound for true-match retention is ≥0.80.

`corroborated` requires all three. `rejected` requires a contrary bound with
adequate precision: the CAM++−majority upper bound ≤0, false-accept lower bound
≥0.10, or retention upper bound <0.80. Every other adequately executed outcome
is `inconclusive`. Development-selected thresholds and exploratory findings are
not confirmatory.

### 3.5 Stopping, deviations, and outcome rule

Fixed maximum: **12 offline measurement passes** across all arms and partitions.
One pass means one arm over one complete partition (all sessions and probes in
that partition), never one pass per session or probe. The minimum three-arm,
two-partition protocol therefore requires six passes; development alternatives
and the negative control share the remaining ceiling. Development stops at the
frozen configuration. Each final session runs once as part of the single pooled
final-partition pass per arm; no optional stopping or repeated final peeking.
`executor_unavailable`, `protocol_deviation`, and `measurement_failed` are
operational states, not scientific refutations. A valid observation with no
decisive conclusion is `scientific_non_result` / `inconclusive`.

### 3.6 Executor binding — currently unavailable

No frozen authorized reference manifest or deterministic scoring executor exists
at design time. Therefore the design is **not declaration-ready** and no
experiment write is authorized by this review.

Before rerunning registry compatibility, supply a real Rust executor and typed
`educt-voice-identity-report-v1` reports with:

- input: content-addressed session-disjoint manifest, media hashes, exact spans,
  reference and overlap labels, enrollment/dev/final assignments, conditions,
  and model fingerprints;
- output: per-trial identity/unknown outcomes, all metrics above, thresholds,
  margins, coverage, overlap stratum, intervals, exclusions/deviations, and a
  manifest/report reproducibility digest;
- typed failures: `executor_unavailable`, `protocol_deviation`,
  `measurement_failed`, and `scientific_non_result`.

The actual executable path and report references must exist before declaration;
they are deliberately not invented here.

### 3.7 Safety and reproducibility

Use only authorized public research material or operator-authorized local media.
Keep enrollment voiceprints in protected storage and out of reports, exports,
and corpus handoff. Publish only the manifest metadata and report data required
for reproduction, with license and consent constraints intact. Suggest-only
operation and explicit `unknown` remain mandatory; no auto-apply arm exists.

The reproduction package must include the frozen manifest digest, model and
preprocessing fingerprints, source/media hashes, configuration, executable
identity, per-trial outcomes, scoring code, and report digests.

## 4. Candidate comparison

| Candidate | Disposition | Reason |
|---|---|---|
| `three_session_confirmatory` | Selected, declaration-blocked | Directly tests comparative session-disjoint transport, preserves overlap in scoring, and predeclares all deployable arms |
| `single_arm_calibration` | Strongest rejected alternative | Useful for CAM++-only calibration, but cannot test the comparative H1 or choose a deployable alternative |
| `same_session_cross_validation` | Rejected for confirmation | Cheaper exploratory screening, but within-session leakage cannot estimate the required transport |

## 5. Registry gate result

The registry-adapter evidence was classified with `meaningful_baseline=true`,
`rubric_expressible=true`, `positive_max_runs=true`,
`design_spec_versioned=true`, but `partitions_valid=false` and
`executor_bound=false` because the reference partitions and executor do not yet
exist. HT-R returned **`registry_schema_mismatch`**.

Consequences:

- no `experiment_propose`, variant, fitness, seal, final evaluation, or selection
  call was made for this revision;
- no HT-RM declaration mapping was attempted;
- the next useful action is to build the reference manifest and deterministic
  executor, then rerun HT-R on the real artifacts;
- any replacement declaration must name this design and the prior experiment as
  lineage, and the prior experiment's disposition remains a human review decision.
