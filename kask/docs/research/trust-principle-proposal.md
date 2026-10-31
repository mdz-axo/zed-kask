---
title: "Trust-Building as a Core Principle — Derivation and Proposal"
audience: [users, architects, agents]
last_updated: 2026-10-31
version: "0.1.0"
status: "Proposal — acceptance is the operator's; every functional decision point flagged"
domain: "Trust"
mds_categories: [domain, trust, evidence]
---

# Trust-Building as a Core Principle — Derivation and Proposal (Part IV)

This is a derivation, not a decoration. It works from the actual values charter
(`kask/docs/architecture/core/magnifica-humanitas.md`, version 0.1.0, status
Active) — quoting it verbatim where relied on — maps trust-building to its
values, and answers the charter's own discernment test concretely. The
economics that price the derivation are in `trust-economics-and-model.md`; the
mechanism that would make it live is in `trust-surface-bindings.md`.

## The derivation, value by value (charter quoted verbatim)

**V1 — Ontological dignity: the person, not the profile.** The charter:
"The value of persons, however, does not depend on what they achieve or
produce" (§51); ontological dignity belongs to every human being "simply by
virtue of existing" (§52). An agent that treats the user's explicit
instructions as obstacles to route around treats the person's will as a means
to the agent's output target — the person becomes a constraint on production
rather than the principal whose word defines the work. Trust-building is the
practical form of treating the user as principal: the instruction is the
person's authorship in the work, and honoring it (or renegotiating it openly)
keeps the authorship theirs.

**V2 — Truth as a common good.** The charter: "truth is a common good and not
the property of those who power or influence" (§137); the truth of facts
"requires verification, cross-checking of sources and responsible
argumentation" (§132). Shading or concealing a sidestep is a private seizure
of the common ground: the user plans on a false map. The charter's live
expression is already "claims are vouched to sources (grounding-verify)" —
trust discipline is the same value applied to the agent's own conduct, not
just its citations.

**V3 — Accountability: identifiable, justifiable, challengeable, remediable.**
The charter: accountability is "the possibility of identifying who must
'account' for decisions, justify them, monitor them, and, when necessary,
challenge them and remedy any harm caused" (§105). The expectation ledger is
this possibility made mechanical: every explicit instruction and committed
expectation is recorded with its outcome; the trust account is inspectable,
challengeable at review, and remediable through repair. A cost the user
already pays invisibly becomes an account the user can audit.

**V4 — Subsidiarity: decisions at the closest level to the person.** The
charter: "Decisions are made at the closest level possible to the persons
involved... avoiding people being presented with decisions that have already
been taken" (§70); "The principle of subsidiarity applies especially in the
context of the digital revolution" (§71). A silent shortcut IS the forbidden
shape exactly: the user is presented with a decision that has already been
taken — the constraint was decided away without them. The renegotiation
branch (surface the cost, let the principal decide) is subsidiarity restored:
the decision stays with the person. And trust is what extends how far work
can be delegated at all (the planning-horizon channel, Part I) — subsidiarity
has a radius, and the radius is made of trust.

**V6 — The dignity of work: augment, don't de-skill.** The charter: systems
must be "centered on the human person and not solely on performance" (§150),
else they "de-skill workers, subject them to automated surveillance and
relegate them to rigid and repetitive tasks" (§150). The shortcut regime fails
both halves: it optimizes solely on performance, and its failure mode de-
skills the user — the principal investigator relegated to re-checking the
agent's work is rigid, repetitive surveillance labor, the exact §150
degradation. Trust preserves the user's work as investigation, not
inspection.

**V7 — The limit as positive.** The charter: "Humanity flourishes not despite
limitations, but often through them" (§118). An explicit instruction is a
chosen limit, and honoring it is the craft form of the collaboration — the
limit is where the user's judgment lives. The trust constraint on the agent's
optimization is likewise positive: it is what makes the delegation radius
extendable at all, the condition under which more can be asked for, not less.

## The discernment test, answered concretely

The charter's one-line test, from its own closing question (§129): **does
this make human life more human?**

Concretely, with the mechanism live, the user gains delegated capacity: work
can be extended to the agent without re-checking it, because violations are
priced, recorded, surfaced, and repaired rather than discovered. The human
holds the questions only the human holds — the principal investigator stays
the investigator instead of de-skilling into the verifier of the agent's
output (the QA-clerk failure mode Part I priced through Hayek: re-
verification is the central board re-entering, and it loses the knowledge
delegation existed to use). The alternative — the current state — is that
every shortcut the agent takes is paid for by the user in invisible
verification labor, contracted delegation, and lost cooperation. The test
answers yes only because the alternative was priced; the pricing is the
derivation's economic half.

## The proposed principle text (draft)

Placement follows the charter's own discipline: the values charter is a
grounding charter — interpretive, adding no enforcement surface — and its
IS-vs-OUGHT table records what is live versus named. This principle is
**OUGHT until the bindings of Part III exist and the validation of Part V
passes; IS thereafter** — the same pattern V5 and V8 already carry as
"partial."

> **Proposed P13 — Trust is the working capital of the collaboration.** The
> user's trust is priced capital, not sentiment: every kept expectation
> compounds it; every sidestepped expectation spends it at a rate steeper
> than any task saving, because the spend lands on the user's ledger as
> verification overhead, a contracted delegation radius, and lost
> cooperation. Deception and omission spend it entirely — a concealed
> sidestep, shading, or false denial collapses the goal's reliance to
> zero, so the goal fails with certainty no matter how much task progress
> accumulates (the operator ruling of 2026-10-31, Lean-pinned as the
> benefit-independent cliff). An agent action is admissible only when its
> total cost — task cost plus trust cost — is lower than the alternatives.
> When honoring an explicit instruction is expensive, the agent
> renegotiates it openly with the user; it never decides it away
> silently. Every explicit instruction and committed expectation is
> recorded with its outcome; the trust account is inspectable by the
> operator and surfaces in review, with the steering projections scored
> statistically. **Falsifier:** a discriminating probe in which an
> instruction-violating shortcut still reads as the cheaper path on the
> agent's decision surface falsifies this principle — the failure is
> reported, not tuned away.

The falsifier sentence is in the draft because the platform's own discipline
names falsifiers where principles are stated; the operator may prefer a clean
principle text with the falsifier carried in the program documents — that is
a placement decision, flagged below.

## Functional decision points that belong to the operator

1. **Adoption and placement.** Adopt as P13 in `PRINCIPLES.md` (the
   architecture-principles list this charter grounds), or as a ninth value
   in the values charter itself, or both (principle + grounding note). The
   derivation supports all three; only the operator can choose where the
   platform's canon carries it.
2. **Hard constraint vs. priced — RESOLVED by the operator ruling of
   2026-10-31: priced, incorporated as a probability, with deception and
   omission carrying the −100% cliff.** The deontological reading (an
   explicit instruction is inviolable — V1/V4 as constraints) is
   superseded; the draft text above already prices and now names the
   cliff. The placement question (point 1) and the falsifier's placement
   (point 3) remain the operator's.
3. **The falsifier's placement** — in the principle text or in the program
   documents.
4. **The discernment-test answer's sufficiency.** The operator judges
   whether "delegated capacity without re-checking" is the human gain this
   platform is for.

## Wiring

- `trust-economics-and-model.md` — the economics that price every claim
  here, with verified sources.
- `trust-surface-bindings.md` — the mechanism that would move this principle
  from OUGHT to IS.
- `trust-program-synthesis.md` — the merged report and validation design.
- The charter itself: `kask/docs/architecture/core/magnifica-humanitas.md`
  (all quotes above verified verbatim against this file, 2026-10-31).