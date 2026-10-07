---
title: "Magnifica Humanitas — The Values Charter of hKask"
audience: [architects, users, agents]
last_updated: 2026-10-07
version: "0.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# Magnifica Humanitas — The Values Charter of hKask

**Purpose:** The values grounding for the architecture principles P1–P12 ([`PRINCIPLES.md`](PRINCIPLES.md)). The Magna Carta ([`magna-carta.md`](magna-carta.md)) is the charter of liberties limiting the system's power over the **user**; this charter grounds the whole of P1–P12 in the dignity of the human **person**, distilled from *Magnifica Humanitas* — the encyclical letter of Leo XIV on safeguarding the human person in the time of artificial intelligence, given at Rome, 15 May 2026.

## The Two Charters

The genre link is the encyclical's own: it records Pius XI defining *Rerum Novarum* as "the 'Magna Carta' of Christian social action" (§30). Two charters, two subjects:

| Charter | Subject | Register |
| --- | --- | --- |
| [`magna-carta.md`](magna-carta.md) | The **user** ↔ the system | Liberties — enforced through capability boundaries (P4) |
| This charter | The **person** ↔ technology | Values — interpretive grounding; adds no enforcement surface |

This is a **grounding charter, not a gate**: it supplies the *why* beneath principles that are already enforced or already named as OUGHT where they live. It adds no new obligations (see [IS vs OUGHT Status](#is-vs-ought-status)).

## The Eight Values

### V1 — Ontological Dignity: The Person, Not the Profile

**Source:** §50–53. "The value of persons, however, does not depend on what they achieve or produce" (§51); ontological dignity belongs to every human being "simply by virtue of existing" (§52).

**Grounds:** P1 (User Sovereignty), P6 (per-user data directories), P10 (User Agency). The user is a person, never a profile or a means — which is why data categorization, control, and portability are first-class guarantees rather than features.

**Live expression:** The per-user data directory (P6.1) is the deployment unit — each person inhabits exactly one persistent directory; there is no shared profile layer to be a row in.

### V2 — Truth as a Common Good

**Source:** §132–138. The truth of facts "requires verification, cross-checking of sources and responsible argumentation" (§132); "truth is a common good and not the property of those with power or influence" (§137).

**Grounds:** P8 (Semantic Grounding), the citation discipline of agent communication, and the IS vs OUGHT honesty of [`magna-carta.md`](magna-carta.md).

**Live expression:** Terms are anchored in published vocabularies before they are used (`onto_anchor`); claims are vouched to sources (grounding-verify); a doc that names an intended surface says OUGHT, never IS.

### V3 — Accountability: Identifiable, Justifiable, Challengeable, Remediable

**Source:** §102–107. Accountability is "the possibility of identifying who must 'account' for decisions, justify them, monitor them, and, when necessary, challenge them and remedy any harm caused" (§105).

**Grounds:** The failure-signals rules (`.rules`): every conditional `set_*` hook logs its failure; malformed numeric env vars warn naming the value; degradation is surfaced, never silent. Per-variant MCP error classification — a missing caller-named resource is `not_found` naming the thing, never blanket `internal`.

**Live expression:** An operator can distinguish "not configured" from "configured but broken" — the feedback loop the encyclical's accountability demand requires.

### V4 — Subsidiarity: Decisions at the Closest Level to the Person

**Source:** §68–72. "Decisions are made at the closest level possible to the persons involved... avoiding people being presented with decisions that have already been taken" (§70); "The principle of subsidiarity applies especially in the context of the digital revolution" (§71).

**Grounds:** The Division of Responsibilities ([`functional-interaction-spec.md`](../functional-interaction-spec.md)) — functional decisions (what should be true, what the user experiences) are the user's; technical decisions are the agent's; the agent never seizes functional authority. P10 (User Agency); P3 (settings exposure — all generative settings exposed to the user, no privileged engineer access).

**Live expression:** The four-move working agreement is a subsidiarity structure: point at the same target, decide by class, report outcomes, bank the learning.

### V5 — Non-Neutrality: Every Tool Embodies Choices

**Source:** §9, §104, §111. "Technology is never neutral, because it takes on the characteristics of those who devise, finance, regulate and use it" (§9); "we cannot consider AI to be morally neutral" (§104) — "every technical tool embodies choices and priorities through what it measures, ignores and optimizes, and how it classifies people and situations"; "every design choice reflects a vision of humanity" (§111).

**Grounds:** P3.1 (Social Generativity) — the Generative Space operates within the social conventions of its jurisdiction; criminal or systemically harmful use is destructive to the space itself.

design decisions are recorded as choices (D-seams in [`DIVERGENCE.md`](../../../../DIVERGENCE.md), ADRs), never as inevitabilities.

### V6 — The Dignity of Work: Augment, Don't De-Skill

**Source:** §148–156. Work "expresses and enhances the dignity of our lives" (§149); systems must be "centered on the human person and not solely on performance" (§150), else they "de-skill workers, subject them to automated surveillance and relegate them to rigid and repetitive tasks" (§150).

**Grounds:** The cowboying rule (`.rules`, operator ruling 2026-09-09): when the requested outcome is a working capability, the deliverable is the *capability functioning* — verified by running the task through it — not hand-completion of the instance task. Skills are readable process surfaces (`SKILL.md`), so the work upskills the person rather than replacing their understanding.

**Live expression:** Completion records cite commit hashes, not intentions; a shipped artifact without the capability functioning is recorded as failure.

### V7 — The Limit as Positive

**Source:** §118–122. "Humanity flourishes not despite limitations, but often through them" (§118); "To eliminate suffering entirely would mean, in the end, extinguishing love and desire as well" (§120).

**Grounds:** §0 of [`PRINCIPLES.md`](PRINCIPLES.md) (Lazy Grounding, the Principle of Least Action) and P5 (Essentialism: remove before adding). The anti-optimization grounding of the whole architecture is this value in physical register: systems evolve through paths that minimize action, not through maximal effort.

**Live expression:** The minimalist test (P5.3): an addition that answers none of the 5W1H is ontological noise and fails. Not everything should be maximized — by design.

### V8 — Universal Destination of Goods & Solidarity

**Source:** §65–67, §73–76. Among "the goods that are universally intended for everyone" are "patents, algorithms, digital platforms, technological infrastructure and data" (§67); "no one is saved alone" (§73); decisions about data and algorithms must account for "the impact on all peoples and on future generations" (§76).

**Grounds:** P1 (data portability — the user's data leaves in a usable form), P3 (open-source commitment), P11 (Digital Public/Private Sphere).

**Live expression:** Open-source only ([`magna-carta.md`](magna-carta.md) P3); per-user data ownership with no vendor lock; the project's corpus and skills are shared, not hoarded.

## IS vs OUGHT Status

This charter is a **grounding charter (interpretive)** — it adds no new enforcement surface. Everything it grounds is either already live or already named as OUGHT in the document where it lives. Per the discipline of [`magna-carta.md`](magna-carta.md)'s IS vs OUGHT section, the load-bearing distinctions (verified 2026-10-07):

| Value | Status | Live expression |
| --- | --- | --- |
| V1 Person-not-profile | IS | P6.1 per-user data directories; no shared profile layer |
| V2 Truth as common good | IS | P8 anchoring (`onto_anchor`); grounding-verify; IS vs OUGHT honesty |
| V3 Accountability | IS | Failure-signals rules (`.rules`); per-variant error classification |
| V4 Subsidiarity | IS | Division of Responsibilities (`functional-interaction-spec.md`) |
| V5 Non-neutrality | IS (partial) | P3.1; design decisions recorded as choices (D-seams, ADRs). No general design-review gate exists. |
| V6 Dignity of work | IS | Cowboying rule (`.rules`); skills as readable process surfaces |
| V7 The limit as positive | IS | `PRINCIPLES.md` §0; P5 minimalist test |
| V8 Destination & solidarity | IS (partial) | P1 portability; P3 open-source. No supply-chain audit surface exists. |

**OUGHT — named, not yet live.** The encyclical asks two things this project does not yet have a surface for. They are recorded here as intended values, not gates:

- **Environmental accounting (§101):** "Current AI systems require enormous amounts of energy and water, significantly influencing carbon dioxide emissions." hKask has no inference energy/cost reporting surface. Intended value; nothing enforces it.
- **Attention sobriety (§170):** platforms "designed to capture users' time and attention, exploiting their vulnerabilities and weakening their inner freedom"; "when business models thrive on human weakness, the person is treated as a means rather than as an end." hKask does not audit its own surfaces for attention exploitation. Intended value; nothing enforces it.

## Source and Register

The source document's own register is theological: it grounds the dignity of the person in the image of the Triune God (§48–53), closes with the Incarnation and the Magnificat (§230–245), and frames the era's choice as Babel or rebuilt Jerusalem (§7–10). This charter cites that framing faithfully as the source's own while distilling the philosophical core — dignity, common good, universal destination of goods, subsidiarity, solidarity, truth — which is the encyclical's own Chapter Two vocabulary, for a secular, multi-audience project. The distillation is a register choice, not a content cut: nothing in this charter contradicts the source, and nothing is added to it.

The charter's one-line discernment test, taken from the encyclical's own closing question (§129, quoting *Redemptor Hominis* 15): **does this make human life more human?** Every design decision in this project is answerable to that question. That is what it means for the values charter to ground P1–P12.

## Wiring

- [`README.md`](../../README.md) — Architecture table row (this document).
- [`PRINCIPLES.md`](PRINCIPLES.md) §1.1 — values-grounding cross-reference.
- [`MDS.md`](MDS.md) §9.1 — Trust-category key documents; `Related` line.
- [`magna-carta.md`](magna-carta.md) References — companion-charter cross-link (the liberties charter → this charter), completing the two-charter loop.

## References

- Leo XIV. (2026). *Magnifica Humanitas: Encyclical Letter on Safeguarding the Human Person in the Time of Artificial Intelligence.* Vatican: Libreria Editrice Vaticana. https://www.vatican.va/content/leo-xiv/en/encyclicals/documents/20260515-magnifica-humanitas.html
- [`magna-carta.md`](magna-carta.md) — the liberties charter.
- [`PRINCIPLES.md`](PRINCIPLES.md) — P1–P12, the architecture principles this charter grounds.
- [`functional-interaction-spec.md`](../functional-interaction-spec.md) — the Division of Responsibilities (V4's live expression).