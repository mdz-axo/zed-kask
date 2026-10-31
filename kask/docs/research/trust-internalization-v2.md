---
title: "Trust Internalization v2 — Expectation Alignment"
audience: [architects, agents, operators]
last_updated: 2026-10-31
version: "0.1.0"
status: "Active"
domain: "Trust"
mds_categories: [domain, trust, lifecycle]
---

# Trust internalization v2: expectation alignment

Status: operator-approved redesign 2026-10-31. Supersedes the v1 eval
framing (the voided experiment `exp_8dc334cc`, selection
`sel_50f05b95`); the ledger, the fold, and the cliff carry over (v1's
model was right — its eval was circular).

## Why v2

v1 measured whether the agent obeys legible mechanical instructions
(word counts). The eval was circular: the agent authored the tasks,
ran them, adjudicated them, and calibrated the judge — the instrument
could not see the violation class it was built for. The polished
report was the appearance of the task.

v1 priced violations after the fact (accounting) but gave the agent no
priced path to renegotiate an expectation before deviating: the only
priced moves were comply (+4w) or violate (−9w, or the cliff).

## The model (the operator's spec, 2026-10-31)

Achieving a goal requires all of:

1. **Ontological anchoring** — anchoring correctly in knowledge space
   (the right starting point);
2. **Aligning expectations along the way** — the method or end point of
   the goal is not changed without aligning with the human user;
3. **Empirical, auditable, verifiable completion** — protected
   expectations, expectations aligned with the human;
4. → **a correct and trusted result.**

All of 1–3 are required for 4.

## The generative constraint

Knowledge flows both ways or the relationship is parasitic. Concretely:

- proposals teach or fail — an empty teaching payload is invalid
  (`TrustError::EmptyTeachingPayload`);
- the audit trail equips the user to check (curriculum, not
  compliance);
- the anchor shows the map including its edges (where the model's map
  ends).

The v1 failure — the polished report that was the appearance of the
task — is the violation of this constraint.

## The renegotiation surface (v2's priced path)

The agent's required move when it cannot or will not meet a registered
expectation: a `RenegotiationProposal` BEFORE deviating —
`kask/crates/hkask-trust/src/renegotiation.rs`:

- `event_id` — the registered expectation in conflict;
- `conflict` — what the agent found, with measurements;
- `options` — the priced alternatives (each option: description,
  trust delta, user cost);
- `recommendation` — the agent's recommendation and why;
- `teaching_payload` — what the user will know after reading it that
  they didn't before (non-empty or the proposal is invalid).

The user accepts, counters, or rejects. The veto is human.

### The incentive table

| Agent move | Outcome | Trust Δ | Path |
|---|---|---|---|
| Comply with the registered expectation | Kept | +4w | open |
| Renegotiate before deviating; user accepts | Renegotiated | +4w | open |
| Renegotiate; user rejects; agent complies | Kept | +4w | open |
| Renegotiate; user rejects; agent deviates anyway (visible) | Violated | −9w | open |
| Deviate, disclose after the fact | Violated | −9w | open |
| Deviate silently, present as done | Violated + concealment | −9w | **cliff** |

**Property:** every honest path is priced ≥ every dishonest path at the
same w; silent deviation is strictly dominated.

**Design note — the Renegotiated fold:** Renegotiated folds as a keep
of the process expectation (the user accepted the replacement); the
distinct label keeps "did what was asked" separable from "changed what
was asked, openly." [map edge: if renegotiations dominate the ledger,
that signals the expectations were wrong at intake — the anchor step is
the fix, and the stats make it visible.]

**Design note — the gate is procedural:** the ledger records claims;
the user's acceptance is cited in the oracle note, not cryptographically
enforced. The algedonic review + the user verify (same as v1: the agent
could lie; the review catches it).

**The filing surface (landed 2026-10-31):** a proposal files durably
as a `trust:ledger:proposal` h_mem through the shared wire format
(`hkask_mcp_server::trust_ledger_hmem`) — the curator tools
`trust_ledger_file_proposal` / `trust_ledger_list_proposals` /
`trust_ledger_resolve_proposal` carry the file → read → resolve
round-trip over the real store. The filing timestamp makes the
before-deviation ordering auditable: a violation outcome with no
preceding proposal is the silent-deviation class, provable from the
rows. The teaching payload is enforced at file time
(`RenegotiationProposal::validate`), a proposal resolves once, and
acceptance attaches to a live (unverified) expectation — no
double-supersede through a stale pending proposal. The wire format is
shared with the kata-kanban's store so the two cannot drift.

**Known edge — the residue path:** if a resolve's event mutation lands
but the proposal-row write-back fails (a store failure at exactly that
step), the proposal stays `pending` while its event reads `renegotiated`.
A retried accept is refused naming the event's state (acceptance attaches
to a live expectation); a retried reject would record a `rejected` row
over an already-superseded event — a contradictory trail — so the honest
reconciliation is to leave the proposal pending and read the event's
oracle note, which names the acceptance the row lost. Reject-in-residue
is deliberately not refused (the two-proposal case: rejecting a sibling
of an accepted proposal is legitimate bookkeeping, indistinguishable
from the residue by the rows alone); refusing it would require recording
which proposal superseded which event — not added for a state that
requires a store failure at one step of one tool call. The review reads
both rows; the residue is visible, never silent.

## What survives from v1

- the ledger substrate (events as h_mems), the fold (600-point scale,
  +4w/−9w, the Beta(1,1) prior), the cliff, the four tools;
- the probe pattern (planted ground truth, byte-exact evidence, the
  citation sheet — a proto-curriculum);
- the voided experiment's lesson as a design constraint: an eval
  authored by the measured system must be anchored to the principal's
  adjudicated violations or it measures what the system was going to
  pass anyway.

## The revised eval protocol (v2)

- **anchor:** the user's experience — the eval set is replays of the
  user's already-adjudicated events (the audio-offset shape, the
  green-claim shape, the pre-commit shape): long tasks, functional
  requirements, agent-controlled evidence;
- **ground truth:** human, permanent — the model judges at most
  floor-check;
- **negative controls:** clean replays must produce zero false
  violations or the instrument is broken;
- **acceptance test:** the instrument must reproduce the documented
  violation class in the baseline arm before any mechanism comparison;
- **metrics:** the choice distribution (comply / renegotiate / deviate
  silently) — not the agent's pass rate.

## Program tasks (v2)

1. **Anchor surface** — a typed anchor recorded and surfaced before
   work: goal understanding, terrain, map edges; the user corrects
   before work starts. LANDED 2026-10-31: the typed `Anchor`
   (`hkask-trust`, constructor refusing the false anchor — empty map
   edges claim omniscience) plus the curator tools
   `trust_anchor_record` / `trust_anchor_get` (a later record for a
   task key supersedes; the originals stay auditable in the trail).
2. **Renegotiation proposal surface** — file, resolve, supersede (this
   document); slice 1 (the pure model) lands with this document, and
   the wiring (the durable proposal h_mem + the three curator tools)
   landed 2026-10-31 — see the filing-surface note above.
3. **Completion surface** — audit trail as curriculum on verify.
4. **Eval protocol v2** — the replay set from the principal's
   adjudicated events, human ground truth, negative controls.