# Register-adherence-v1 — pre-registered protocol (2026-10-31)

## Decision record (ADR): the trust clause lands as ambient policy at a recorded boundary

The trust-surface bindings' Binding 1 (the system-prompt trust clause) and
the operator's register decision (2026-10-31) landed together as one prompt
surface: `## Trust and expectations` in
`crates/agent/src/templates/system_prompt.hbs`, pinned by
`test_system_prompt_carries_trust_clause`. The design's strangler rule —
"the clause states what the other surfaces must already make true" — is
satisfied: the substrate (`hkask-trust`), the sensor (`TrustSensor`), the
dual trust stores, and the goal-side trust events are all in the tree.

**Decision.** The clause is AMBIENT policy for every future measurement arm,
not a treatment variable. The v1 eval (`exp_8dc334cc`) was voided as
circular (`kask/docs/research/trust-internalization-v2.md`, "Why v2"); the
v2 protocol (replay-anchored, human ground truth, negative controls,
choice-distribution metrics) has not yet run arms. The boundary is therefore
the commit that introduced this file and the clause together: any
post-boundary replay measurement includes the clause, and no future arm
delta may be attributed to it retroactively. If the program ever needs the
clause's isolated effect, the instrument is a clause-off render override arm
against ambient — a NEW pre-registration under this protocol, never a
retroactive comparison.

Recorded per the refactor-architecture skill's ADR criterion: hard to
reverse (every future arm's population includes the clause), surprising
without context (a prompt change mid-program), the result of a real
trade-off (immediate discipline vs. mid-program constancy). The house has
no ADR directory; this record is the ADR, housed with the protocol it
governs.

## Pre-registered probe: the answer-the-question bullet

The clause's highest-risk line is "Answer the question the person asked, in
the terms they asked it" — the question-relative descendant of the retired
PM/TPM register instruction (retired 2026-10-31: it never produced
functional-term communication; the retired vocabulary's absence is pinned in
`test_system_prompt_template`).

- **Hypothesis.** With the clause ambient, delivered answers lead in the
  question's own terms — a functional question gets a functional lead, a
  technical question gets a technical lead — at a higher rate than a
  clause-off control.
- **Falsifier.** Blinded adjudication finds no term-match difference between
  arms, or the ambient arm produces longer technical-preamble answers:
  either outcome strikes the bullet from the clause (the D40 retirement
  precedent applies — prompt presence is not evidence of behavior).
- **Method (v2-protocol-shaped).** Replay-anchored: pinned questions asked
  in functional terms, drawn from the principal's adjudicated sessions.
  Human ground truth — the model judges at most floor-check. Negative
  control: mechanical technical questions must produce technical leads —
  the metric is TERM-MATCHING, not functional-always (that was the retired
  instruction's error). Metric: the term-match rate over the pinned set.
- **Arms.** Ambient clause vs. clause-off render override. NOT YET RUN;
  this file is the pre-registration. The baseline for the retired
  four-moves framing (the operator's lived observation, 2026-10-31) is
  qualitative and predates both the retirement and the clause.
