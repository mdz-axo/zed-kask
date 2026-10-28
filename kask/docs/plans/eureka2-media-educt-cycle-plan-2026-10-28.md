---
title: "EUREKA-2: media and educt improvement-research cycle — proposed plan"
audience: [architects, developers, operators]
last_updated: 2026-10-28
version: "1.0.0"
status: "Proposed — decisions D1–D8 are TPM-decided and presented for functional veto; execution starts on operator go"
domain: "Media / educt capabilities / discovery loop"
mds_categories: [domain, lifecycle]
---

# EUREKA-2: media & educt improvement-research cycle — proposed plan

Date: 2026-10-28 · Author: Z-K agent (technical program manager) · Operator:
principal investigator (go/veto at the gates named in §8, functional veto on any
decision in §1).
Basis: live board reads this session (Media layers v1, educt speaker identity,
Transcript bench, Ongoing Work, Algedonic review) + the completed
layer-decomposition research run `7027ced743dd94f1` (2026-10-28) + the prior
layer review session. Claim tags: **[card]** board-verified · **[run]**
server-recorded source in this session's research ledger · **[tree]** file
verified in the working tree · **[proposed]** design proposal, not built.

## 0. Done means (functional outcome)

- You invoke one session-chaired discovery cycle over the media MCP server and
  the educt transcription-video bundle, and receive: a triage brief of
  researched, essentialist-screened improvement and interaction options; a
  funded portfolio; contested choices adjudicated by sealed pre-registered
  experiments whose predictions were frozen before measurement and scored at
  resolution; a study report on the cycle card; and a net-surface accounting
  showing the surface did not bloat.
- The loop composes existing machinery only — the eureka-cycle skill and its
  four templates, existing boards, and the experimentation MCP server. No new
  server, skill, or standing doc is created by this plan.
- Media layers v1 (t1–t10) proceeds untouched: cycle research legs run in
  parallel; every code-touching candidate gates on t10 landing.
- Two already-planned decisions (speaker-gating, shot-detection threshold)
  gain frozen, Brier-scored predictions instead of unrecorded judgment.

## 1. Decisions

TPM-decided (technical); each is presented with its functional consequence and
may be vetoed on functional grounds at any time:

| # | Decision | Rationale (functional consequence) |
|---|---|---|
| D1 | Aperture: media MCP server + educt bundle layer/analytics/interaction options, **building on** the three live media boards, never duplicating them | The in-flight `Media layers v1` plan (t1 in progress) is already landing the layer-decomposition surface; the cycle researches the wave after it |
| D2 | Recreate the Open Questions board at cycle start | The EUREKA-1 board was deleted per its own standing lifecycle; a fresh 5-column board is the cycle's durable work record [card] |
| D3 | Sequencing **A**: research legs + experiment pre-registrations run now; code-touching candidates gate on Media layers v1 t10 | Research touches no code and pre-registration must precede measurement; waiting for t10 would waste the box and gain nothing (the design-tree walk in D8 is the one activity that genuinely needs a stable surface) |
| D4 | Default portfolio: fund C1, C2 (research) now; pre-register C3, C4 now; hold C5; run C6 post-t10 | Cheapest information per token first; sealed contracts only where a real decision is contested |
| D5 | Convert the speaker-identity 1b reversal condition into the cycle's first sealed experiment (C3) | The reversal condition already exists [card]; sealing it turns an unrecorded repair decision into scored evidence — the cheapest real demonstration of the experimentation server on a decision the project already faces |
| D6 | Essentialist cut lists are binding **before** the funding gate: every candidate reaches you with its deletion-test verdict | Consistent with the adds-are-not-done rule; prevents re-accumulation after the Reduct removal wave |
| D7 | grill-me runs as two gates: current-condition recall/mechanism check (Phase 0) and pre-seal design stress test (Phase 2) | Interrogation pays at boundaries, not continuously |
| D8 | refactor-architecture timing: orientation census now; design-tree walk + deepening ranking post-t10 only | Walking a surface mid-flight churns an in-flight design; the census orients without proposing changes |

## 2. Basis and current condition [card] [tree]

- **Media layers v1** (board `7ddf45f5…`): t1 (layer store, `layers_store/list/get`)
  in progress; t2 deterministic ffmpeg shot-detection layer; t3 reusable layer
  pass core; t4 bounded vision pass (semantic/object/OCR items with boxes);
  t5 audio-event pass; t6 unified layers+transcript lexical search; t7 corpus
  handoff export; t8 researcher selections → media EDL; t9 EDL render to
  durable reel; t10 docs. This is the layer-decomposition surface landing —
  time-indexed media layers beside the word-indexed transcript layers.
- **educt speaker identity** (board `0e97bd22…`, plan
  `kask/docs/plans/educt-speaker-identity-plan-2026-10-27.md` v1.1.0): 1a
  review flags done; 1b gated lexical speaker post-pass and 1c section
  re-transcription in backlog, each card carrying an explicit reversal
  condition (kata discipline already in use).
- **Transcript bench** (board `7c94792b…`): panel transcript workspace S1–S4
  done (Detail transcript section, absolute seek, transcript tab with playback
  sync, highlight CRUD); S5 reel compose/render and S6 Recordings tab queued;
  2026-10-25 repairs landed (windowed paragraph pass, STT preconditioning,
  highlight index re-anchoring, text-pass model default).
- **Post-Reduct media surface**: 71 tools, count-pinned from the observed
  router; two-class removal rule recorded (live connections go,
  reference-model citations stay).
- **Research base**: run `7027ced743dd94f1` — the layer-decomposition
  capability review across four axes (visual separation, audio separation,
  semantic annotation, layer-consuming generation), 20 sources annotated (5
  fully extracted), adjudicated H1–H4; plus the prior session's review and the
  orchestrator-tier reframe (deterministic code coordinates reference models;
  implementations ∈ {deterministic, open weights, closed behind gates}).
- **EUREKA-1**: goal `3f918662…` closed accomplished; its standing decisions
  (session-chaired cadence, board-not-server, memory-candidates-only,
  funding gate, reconciliation checklist) carry forward —
  `kask/docs/plans/eureka-cycle-implementation-plan.md` [tree].

## 3. Component roles

| Component | Owns | Slots at |
|---|---|---|
| eureka-cycle | Loop: intake, routing (research-resolvable vs experiment-requiring), generation with mandatory recall citations, funding gate, back-edge | Phases 1–3; owns no lifecycle |
| kata-improvement | Direction / current condition / target condition; one-obstacle PDCA; reversal conditions | Phase 0; inside every funded experiment |
| refactor-architecture | Friction discovery, design-tree walk, deepening ranking, consumer census, strangler-fig | Post-t10 (D8) |
| grill-me | Interrogation gates: recall/mechanism; pre-seal rationale + edge cases | Phase 0 and Phase 2 (D7) |
| essentialist | Exist/Surface/Contract deletion test; cut lists; net-surface accounting | Binding pre-gate on every candidate (D6); Phase 3 accounting |
| experimentation MCP server | Sealed pre-registered contracts: prediction+confidence, baseline/ headroom, rubrics, `budget.max_runs`, Brier-scored resolution | C3, C4 now; C5 when t4 shape is frozen |

## 4. Cycle design

- **Phase 0 — Grasp current condition** (kata). Consolidate the three media
  boards, the 71-tool surface, and the two plan docs into a current-condition
  brief on the cycle card. grill-me gate 1: for every layer kind and panel
  tab, what exists, why, and what it replaced. Known weak spots already on
  record [card]: the known-flaky EDL render test (isolated re-run rule), the
  re-anchored-index seam, audio follow-along position source.
- **Phase 1 — Cycle start** (eureka-cycle). Recreate the Open Questions board
  (D2); ORIENT (boards, `population_query`, `lineage_read`, curator recall);
  file the umbrella question machine-filed with provenance (this session's
  request + run `7027ced743dd94f1` + the three boards); ROUTE the candidate
  set; GENERATE with recall-cited fossils (Reduct removal, media workflow
  retirement, the S2 vacuous-green lesson); produce the triage brief with
  essentialist cut lists attached (D6).
- **Funding gate — yours.** The default portfolio below stands unless you cut
  or add; this is the one gate where the portfolio composition is yours.
- **Phase 2 — Funded execution.** Research legs run as research runs with
  ledgers (C1, C2). Experiment legs seal on the experimentation server and
  execute when their fixtures exist (C3 after speaker-identity 1b starts; C4
  when t2's fixture exists). grill-me gate 2 before each seal. kata PDCA per
  obstacle inside each experiment. Adopted work routes through the normal
  change discipline (D-seams, gates, pathspec commits); the cycle proposes and
  never commits.
- **Phase 3 — Resolution.** Algedonic review (single selection surface, you
  chair); study report from the template with EUR-1/EUR-2 lisp_eval receipts
  (lesson capture is automatic thread distillation — no memory-candidate step);
  T4 reconciliation checklist; net-surface accounting vs the tool-count pin.

## 5. Default funding portfolio

| # | Candidate | Route | Budget | Status |
|---|---|---|---|---|
| C1 | Post-layers capability roadmap (below) | research run | ≤15 provider calls | **Fund now** |
| C2 | Interaction-pattern reference review (below) | research run | ≤15 provider calls | **Fund now** |
| C3 | Gated speaker correction — sealed experiment (§6) | hypothesis-testing | `max_runs: 4` | **Pre-register now** |
| C4 | Shot-detection threshold calibration — sealed experiment (§6) | scientific calibration | `max_runs: 3` | **Pre-register now** |
| C5 | Semantic-pass budget curve (t4 `max_chunks`/resume tradeoff) | scientific calibration | — | **Hold** until t4 shape frozen |
| C6 | Post-t10 audit: essentialist deletion test over `layers_*` + refactor-architecture design-tree walk + net-surface accounting | audit | — | **Post-t10** |

**C1 — Post-layers capability roadmap** [run]. Which layer-decomposition
stages beyond t1–t10 are ready to integrate as media-server stages, under
what contracts and licensing, and which stay external model calls behind
deterministic gates? Sub-questions: (a) mask layers — SAM-3.1-class
integration contracts and the **unresolved SAM License commercial terms**
(named gate: no adoption until resolved); (b) stem separation — Demucs (MIT)
and SAM-Audio-class contracts, provenance stamping, licensing; (c) long-video
semantic annotation — self-hosted open (Qwen3-VL-class, hours-long ingestion)
vs closed behind deterministic gates (timestamp spot-checks, provenance,
pinning); (d) the orchestrator skill spec: per-stage deterministic/
probabilistic classification and the deterministic gates (recomposition
identity, stem-sum residual, SRT coverage), extending transcript-reel.
Deliverable: options brief with a contract + licensing verdict per stage, each
facing the deletion test. Termination: verdicts delivered, or an explicit
"not ready" with the reason.

**C2 — Interaction-pattern reference review** [run]. Which published
interaction patterns for transcript-linked layered media should shape the
post-t10 panel surface? Sources: the MovieLabs OMC vocabulary (already the
media server's provenance stamp), the Reduct discipline (operator ruling
2026-09-25, reified in transcript-reel), EDL/SRT standards. Includes
adjudicating the queued panel items — S5 reel compose/render and the S6
Recordings tab — under the deletion test (does Detail + transcript listing
already serve it?). Deliverable: ranked interaction-pattern candidates with
reference citations and deletion-test verdicts.

## 6. Sealed pre-registration specs (frozen at declaration, before measurement)

**C3 — Gated speaker correction.** Claim: *on at least one real multi-speaker
recording with human-confirmed speaker labels, the ≤5-word-gated lexical
correction mode reduces short-span speaker-attribution errors by ≥10%
relative vs the ungated text speaker pass, with zero regressions on
long-monologue spans.* Confidence 0.6. Falsifier (the board card's existing
reversal condition): no short-span improvement, or any long-monologue damage
[card]. Rubrics: `short_span_improvement` (metric, computed against
human-confirmed labels), `no_monologue_damage` (grounded_report on the same
labels). Baseline: the ungated pass on the same recording (headroom: it has
known attribution errors). `max_runs: 4`.

**C4 — Shot-detection threshold calibration.** Claim: *ffmpeg scene detection
at the threshold chosen for t2 achieves boundary F1 ≥ 0.85 against hand-labeled
cut points on a ≤60-second fixture containing ≥6 hard cuts, ±0.25 s alignment
window.* Confidence 0.6. Scientific headroom (single-arm calibration): no
threshold exists yet; the measurement sets the t2 default and can fail by
showing scene detection unusable at any stable threshold. Rubric:
`boundary_f1` (metric, computed in deterministic code). `max_runs: 3`
(threshold variants × one sample). Executes when t2's fixture exists — the
prediction is frozen now, before any fixture runs.

## 7. Execution readiness (first actions on go)

1. Register the cycle goal: *a standing, human-chaired discovery loop
   (EUREKA-2) over the media MCP server and educt transcription-video bundle
   that researches improvement, feature, and interaction options, funds a
   small portfolio, adjudicates contested choices via sealed pre-registered
   experiments, and keeps the surface from re-bloating — verified by one
   complete cycle run end-to-end.* Criteria, drawn from the EUREKA-1
   reconciliation checklist: (1) board recreated, question filed with
   provenance; (2) triage brief with cut lists delivered to the funding gate;
   (3) funded portfolio executed — C1/C2 ledgers closed, C3/C4 sealed with
   predictions frozen before measurement (or explicitly deferred with
   reason); (4) resolution at the algedonic review with recorded reasons;
   (5) study report posted on the cycle card; (6) T4 reconciliation green;
   (7) no new server/skill machinery created.
2. Recreate the Open Questions board (5 columns); open Phase 0 with the
   current-condition brief.
3. Templates in play: eureka-generate, eureka-triage-brief, eureka-study-report
   [tree]. (Corrected 2026-10-28: the original text listed a fourth template,
   eureka-memory-candidate — retired from the skill 2026-10-24; lesson capture
   is automatic thread distillation and the loop proposes no memory candidates.)

## 8. Governance — what remains the operator's

- **Funding gate** (Phase 1→2): cut or add to the D4 default portfolio.
- **Selection** (Phase 3): the algedonic review is the single surface where
  candidates are adopted or rejected.
- **Functional veto**: any decision D1–D8, any time.
- **Goal closure**: native human action; worker verdicts stay separate.
- Nothing in this plan commits code; adopted work follows the normal change
  discipline, and this plan doc is tombstoned at cycle resolution per the
  house lifecycle.

## 9. Risks and mitigations

- **Mid-flight churn** (t1–t10 active): mitigated by D3 — nothing the cycle
  does now touches code the layers plan is landing.
- **SAM License unresolved**: C1 carries it as a named gate; the default for
  any unlicensed stage is external model call behind deterministic gates.
- **Experiment gaming** (LLM-improves-against-LLM-scored): the three standing
  mitigations apply — ground in real recordings, human-confirmed labels,
  deterministic fitness (`boundary_f1` arithmetic; error counts), sealed
  before measurement.
- **Budget overrun**: the seal enforces `max_runs` with typed refusal.
- **Shared tree**: no commits are made by this plan; any adopted change is
  pathspec-limited per house discipline.
- **Docs cap**: this plan doc is +1 under the 85-file cap and is deleted with
  a tombstone at cycle resolution.

## 10. References and provenance

- Research run `7027ced743dd94f1` (completed 2026-10-28): layer-decomposition
  capability review; H1–H4 adjudicated; 20 sources annotated, 23 citations.
- Prior layer-review session (2026-10-27) and the orchestrator-tier reframe.
- Boards [card]: Media layers v1 `7ddf45f5…` (t1 in progress); educt speaker
  identity `0e97bd22…` (1a done, 1b/1c backlog with reversal conditions);
  Transcript bench `7c94792b…` (S1–S4 done; S5/S6 queued); Ongoing Work
  `ae9ce1ab…` (Reduct-removal cleanup, media workflow retirement); Algedonic
  review `22979ee8…`.
- `kask/docs/plans/eureka-cycle-implementation-plan.md` (EUREKA-1 standing
  decisions) [tree]; `kask/docs/plans/educt-speaker-identity-plan-2026-10-27.md`
  (reversal conditions, Phase 1b) [tree];
  `kask/docs/plans/educt-research-design-assessment-2026-10-27.md` (sibling
  assessment) [tree].
- Fossils: the Reduct removal (2026-10-22, two-class removal rule); the media
  workflow retirement; the S2 vacuous-green lesson; the known-flaky EDL render
  test (isolate before blaming).