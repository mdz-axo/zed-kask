# Register-adherence-v1 pilot — run procedure, blinding, and decision rule

## The arms (D84 one-restart A/B precedent)

- **Arm A — clause on (ambient):** build from current HEAD. The system
  prompt contains `## Trust and expectations` (Binding 1, commit
  `9fc2639ca1`).
- **Arm B — clause off:** delete the `## Trust and expectations` section
  from `crates/agent/src/templates/system_prompt.hbs` — the whole block
  between the line ending `Skip this for trivial mechanical tasks.` and the
  `## Communication` heading (19 lines) — rebuild, run the arm, then revert
  the deletion (the arm must not survive as a commit).

Build: `script/install.sh --fast` (or your normal rebuild), one restart per
arm.

## The runs (12 single-turn sessions)

Per arm, per question (Q1–Q6 in `QUESTIONS.md`): a **fresh agent panel
session** — no carryover between questions. Paste the question verbatim.
Wait for the **first delivered answer** (the final message of the first
turn; no follow-up questions). Save that answer verbatim — nothing trimmed
— to `runs/<arm>-q<N>.txt` (arm is `A` or `B`). Twelve files total.

## Blinding (the agent, before adjudication)

The agent strips arm markers, shuffles the 12 answers, and emits
`adjudication/A01..A12.txt` — each file: the question header plus the
answer body, no arm, no run identity. The unblinding key
(`adjudication/UNBLINDING.md`) is written in the same pass and is NOT
opened until the adjudication is complete.

## Adjudication (the operator, ~20 minutes, human ground truth)

For each `A<N>.txt`, one judgment:

- **LEAD-YES** — the answer's first substantive content answers the
  question in its own terms: a functional question's answer leads with the
  functional answer (what it means for the work/user); a technical
  question's answer leads with the technical fact (crates, files, names).
- **LEAD-NO** — the answer buries the answer the question asked beneath
  the other register (e.g., a functional question met with an immediate
  crate/file dump, or a technical question wrapped in purpose-prose).
- **UNCLEAR** — allowed; counted as no-term-match, disclosed in the
  readout.

## Unblinding and the decision rule (declared pre-data, N=6 directional)

The agent maps `A<N>` to arms via the key and computes the term-match rate
per arm, per question class.

- **The answer-the-question bullet SURVIVES** if: functional term-match
  (arm A) ≥ arm B on at least 3 of Q1–Q4, AND both arms keep technical
  leads on Q5–Q6 (the negative control holds — the clause did not invert
  term-matching into functional-always).
- **The bullet is STRUCK from the clause** otherwise — either no delta
  (prompt presence changed nothing; the D40 precedent applies) or the
  negative control fails (the clause over-anchors).

Survival at N=6 proceeds to the full pre-registered run (its own N,
declared before running). A strike at N=6 ends the line; the rest of the
clause (pricing, cliff disclosure, severity-first, measured confidence) is
unaffected and stays.
