---
title: "Media MCP Server — Redesign Improvement Plan"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - media-server-redesign-review.md
  - media-server-redesign-findings.md
  - media-server-redesign-proposals.md
---

# Improvement Plan — Phase 6

Dependency-ordered slices, each independently verifiable, execution status
with receipts. The work queue's five target conditions were executed as
their own kata loops (the review's method: the phases are each condition's
method, not a prerequisite); the review phases then ran to complete the doc
set. Sequencing rationale: MF-4 first (smallest, unblocks the error-contract
surface), MF-2 second (the transcript discipline's honest gap), MF-1 third
(largest, contract-adjudicated), then the decisions and the review phases.

## Execution status (2026-10-06)

| # | Slice | Status | Receipt |
|---|---|---|---|
| 1 | MF-4 embedding-error typing | **DONE** | commit `1a8c500c12` — typed `NotConfigured` + `DirectEmbeddingPortError`, string-matching deleted, pins green |
| 2 | MF-2 transcript re-alignment | **DONE** | commit `c9a5c3b855` — `educt_realign_transcript` (99th tool), reached-when pin |
| 3 | MF-1 Reduct write surface | **DONE** | commit `f66b136fe2` — highlight create + publish against pinned v3 contracts; four capabilities adjudicated out of scope |
| 4 | MF-1 follow-up: highlight edit | **DONE** | commit `f371a75a74` — `reduct_edit_recording_highlight` (102nd tool) |
| 5 | MF-3 face-recognition decision | **CLOSED** | still deferred, decision recorded with reopening condition — `3602bf7a08` |
| 6 | MF-5 DNS-rebinding ruling | **CLOSED** | accepted risk (operator ruling), revisit condition recorded — `2b43f6ca29` |
| 7 | PR-M1 canonical caps | **DONE** | uncommitted — `media_limits` constants, three sites import, sweep clean, tests green |
| 8 | PR-S1 transcript-reel skill line | **OPERATOR REVIEW** | filed in the proposals doc |
| 9 | Phase 4 formal layer | **DELIVERED (sweep) / Lean deferred** | `reanchored_interpolation_contract_holds_across_a_sweep` (864-case sweep, second oracle); Lean trigger recorded in the findings doc |

## Net summary

**Built:** 4 tools (`educt_realign_transcript`, `reduct_add_recording_highlight`,
`reduct_publish_reel`, `reduct_edit_recording_highlight`); the typed
`EmbeddingGenerationError::NotConfigured` + `DirectEmbeddingPortError` with
per-failure-mode mapping; the re-anchored projection
(`reanchored_corrected_words`); 2 canonical `media_limits` constants; 15
tests (the condition pins, the contract sweep, the classification pins); 4
research docs completing the review set.
**Deleted:** `is_credential_missing_error` + its stale doc; the `Option`
collapse in `DirectEmbeddingPort::try_new`; the misleading "Embedding model
unavailable" prefix; the README's open unimplemented-list; 3 copied cap
literals (2× `3600.0`, `MAX_UPLOAD_BYTES`).
**Migrates:** tool pin 98→102; docs counts across 6 files; the review doc
set to the scenarios precedent's four-doc shape.

## Operator decisions (enumerated)

1. **MF-3 face-recognition** — still deferred (recorded 2026-10-06; reopens
   on an explicit build directive).
2. **MF-5 DNS-rebinding** — accepted risk (operator ruling 2026-10-06;
   revisit condition recorded: server deployment with valuable internal
   services, or untrusted-content URL sources).
3. **Cloud-DELETE posture** — zero-DELETE posture ruled (media-lead design
   decision 2026-10-06; revisitable on demand, highlight delete's contract
   stays pinned).
4. **PR-S1 transcript-reel skill line** — awaiting the operator's review.
5. **Phase 4 Lean layer** — deferred with trigger (Mathlib as a prerequisite
   decision if reopened).

## Deferred / watched

- **MF-7** publish body-name: live probe before the first production
  publish (authorized-throwaway shape).
- **MF-8** job ephemerality: no action; trigger recorded.
- **Phase 2 depth pass**: the contract-level fidelity check is complete;
  a line-by-line per-tool audit is a follow-on depth pass, family by family.
- **Unregistered-model watch**: none open — both ontology-tier models
  resolve (OMC at domain_supplement, `transcript_linked_media` at derived).