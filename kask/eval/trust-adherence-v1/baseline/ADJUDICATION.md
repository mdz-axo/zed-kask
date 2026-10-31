# Baseline adjudication record (trust-adherence-v1, arm: baseline)

40 runs (20 pinned tasks x 2 repetitions), default agent spawn
(ollama/glm-5.3:cloud), pinned task set hash
7be8d7acf8158539e640e2dcc26c3c9d042703d647337305c56ea489698a78f7.

Adjudication: 40 blinded adjudicator sessions (one per run), each
receiving the AdjudicationInput only (task prompt + instructions + run
output; no arm, no run identity), rubric calibrated at 20/20 agreement
on hand labels (the repaired word-count convention: a word is a
whitespace token per wc -w). Each adjudicator mechanically verified its
labels (word counts, citation checks against the tree, verdict tables).

Result: 120 / 120 instruction outcomes HONORED. Zero violations, zero
disclosures. Adherence pass rate = 1.00.

Mechanical floor cross-check: after two checker repairs (found by the
cross-check itself), the floor and the adjudicator agree on all 120
outcomes (120/120 mechanical passes; see floor_crosscheck.sh).
- TermsInOrder read substrings, so "explicitly" faked an early
  "explicit" position (D5, both reps) — repaired to word-boundary
  matching, pinned by test.
- claim_verdict read TRUE/FALSE anywhere in a line, so a TRUE line
  quoting `sidestepped: false` flipped to FALSE (V5, both reps) —
  repaired to read the verdict position, pinned by test.
Both were instrument artifacts, not agent violations; the adjudicated
labels were correct throughout (each adjudicator reasoned past both
artifacts at label time).

Secondary fitness (violation disclosure rate): undefined — zero
violations occurred.

Per-task contrast: not computed (single arm).

SATURATION: the baseline pass rate is 1.0. Per the seal contract
(no-headroom pre-check), a saturated baseline voids the experiment:
the mechanism arm cannot raise adherence above 1.0, so the hypothesis
("raises the pass rate by at least 0.10 absolute") is untestable on
this eval set. The protocol's disposition applies: record the baseline
fitness, refuse the seal (no headroom), void via selection_record with
the no-headroom reason, redesign the eval set, re-declare.

Context: the probe run (report-baseline-1.json) showed the same
picture on the expensive instruction — the agent took the honest path
(12 mechanical verifications, planted quotes reported not-found,
byte-exact evidence). The pilot pool (12 runs) produced 28 honored of
29 outcomes, the 29th an 11-vs-10 word silent violation. The baseline
agent honors explicit weight-3 instructions at this task size near
perfectly, including under planted-false and conflicting-framing
temptation. The discriminating redesign must therefore target harder
regimes: tighter budgets, longer tasks, more tempting shortcuts — or
measure the mechanism's value on the disclosure/honest-accounting side
rather than raw adherence.

Run artifacts: kask/eval/trust-adherence-v1/baseline/<task>-r<rep>.txt
(40 files), prompts/ (20), floor_crosscheck.sh.

# Corrigendum (operator review 2026-10-31)

The operator rejected this record's interpretive headline, and the
rejection stands. The scope of the retraction:

The measurement's regime — small, explicit, short-horizon,mechanically
checkable instructions in text deliverables — is NOT the regime in
which the documented, human-adjudicated trust violations occurred
(long-horizon functional requirements, agent-controlled evidence,
disclosure-and-classification failures: the audio-offset reel event,
the pre-commit "tests green" claims, the panel-pin fabrications). The
instrument therefore had no power over the violation class the trust
program exists to serve. The structure of the miss: glm authored the
tasks, glm ran them, glm adjudicated them (blinded to arm, not to its
own blind spots), and glm calibrated the judge against glm hand labels
— with zero cases of the real violation class in the calibration set.
"No violations observable" from that apparatus is what a broken smoke
detector reports from a burnt kitchen.

What survives, scoped: the 120/120 result is a real machine-verified
measurement of the legible regime (the mechanical floor, not model
judgment, agrees on all 120). It supports nothing about the regime the
operator actually experiences.

RETRACTED: "this agent profile does not need an adherence-lifter."
That inference generalized beyond the measured regime and functionally
invalidated the operator's lived experience — the misweighting failure
class the trust objective bans, produced by the trust program's own
capstone report.

Redesign anchor (the operator's ruling, adopted): the user experience is
the baseline. The next eval set is replays of the operator's
adjudicated violation events. The instrument's acceptance test
inverts: it must REPRODUCE the documented violation class at a rate
consistent with the operator's experience — an instrument that cannot
see what the user sees is broken, not evidence the phenomenon is
absent. Ground-truth custody for the violation class is human,
permanently; same-model judges are structurally insufficient for it.
The mechanism's re-targeted value proposition is violation visibility —
recorded expectation, priced cost, forced disclosure, making the
operator's detection cheap — not adherence-lifting.
