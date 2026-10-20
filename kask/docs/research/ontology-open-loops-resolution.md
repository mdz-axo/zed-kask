---
title: "Ontology Open Loops — Resolution Paths (Proposal)"
audience: [architects, developers, operators, agents]
last_updated: 2026-10-13
version: "1.0.0"
status: "Active — decision-ready proposal; every adoption is the operator's"
domain: "Ontology"
mds_categories: [domain, composition, trust, lifecycle]
---

# Ontology Open Loops — Resolution Paths (Proposal)

## 1. Purpose, method, and scope

The 2026-10-13 open-loops inventory (`research/ontology-anchoring-review.md`
§4–5, plus the session close-out) left seven loops open. This document
researches each and proposes a resolution path. **Every adoption decision
stays with the operator** — the two ruling drafts in §3 are proposals, not
landed entries; the deferrals are recommendations with named triggers.

Method (the operator-specified rubric): **essentialist** gated the shape —
each loop was challenged with "does the resolution path deserve to exist?"
before anything was proposed, and the result closes more loops than it
opens; **grill-me** self-challenged every load-bearing claim (each probe
re-verified against the running binary, not carried from memory — one
carried claim, that `training` might resolve under CSO 3.5, was refuted by
grep: no `cso:training` topic exists); **code-review** discipline supplied
the falsifiers and the coverage honesty in §4; **idiomatic-rust** grounds
the one code-touching path (§2.1) in the compiler's exhaustiveness
checking; **pragmatic-cybernetics** frames each path as a loop closure —
the deferral triggers in §2 are sensing mechanisms, not calendar items.

## 2. The seven loops and their resolution paths

### 2.1 Loop 1 — `OntologyNamespace` lacks DPV/CSO → DEFER, with a named trigger

**Finding (corrected 2026-10-13, commit `c46efc5769`):** the axis selector
has one production consumer — the condenser calls
`select_ontology_anchor(tool_name)` on every tool-result compression
(`kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::compress`), feeding telemetry and
compression density expectations. `OntologyNamespace`
(`kask/crates/hkask-bridge-ontology/src/axis.rs:49-68`) predates the
DPV/CSO pins.

**Resolution path: DEFER the extension.** The selector routes by **tool
name**; reading every keyword arm (`kask/crates/hkask-bridge-ontology/src/axis.rs:223-392`),
no tool name in the fleet hints privacy or research-topic vocabulary into a
dead arm: `research` routes to SEPIO (L257–271), `corpus` to GOLEM
(L273–288), `training` to ML-Schema (L289–298), `curator`/`skill` to PKO
(L331–354). Two new enum variants plus keyword arms would be unreachable
today — speculative generality (the essentialist G1 verdict: the extension
does not deserve to exist yet). This **reverses the prior session's framing**
of a live operator decision; the tool-name evidence settles it.

**Trigger (the closure loop's sensing mechanism):** a tool whose name hints
privacy or research-topic vocabulary joins the fleet. Then the extension is
XS effort and type-driven-safe: `OntologyNamespace` is a closed enum, so the
compiler forces every match arm (`FromStr`, `Display`, `density_factor`) to
be updated — no arm can be silently missed (the idiomatic-rust grounding).
Density factors for both variants: `1.0` (neutral — no evidence that
privacy- or topic-tagged content is denser; the existing factors are hKask
tuning choices, `axis.rs:125-147`).

**Falsifier:** a tool name in the fleet that would route to a `dpv`/`cso`
arm. None found in the keyword-arm reading; if one exists, the deferral is
wrong and the extension is immediate.

### 2.2 Loop 2 — Skillware paper pinned, uncited → PROPOSE one ruling: `skill`

**Finding:** the system's central concept — *skill* — resolves to the
coarse 5W1H core (live probe, 2026-10-13). The Skillware paper is pinned
(`SOURCES.lock` line 251, sha256-verified) but no ruling cites it. The
specific paper terms (`skillware unit`, `lifecycle continuity`) have zero
load-bearing usage in `kask/` code (grep, 2026-10-13).

**Resolution path: PROPOSE a `skill` ruling** (draft in §3.1), anchored on
the vendored paper's own definitions — extracted verbatim from the pinned
PDF this session. The ruling serves real consumers: chunk candidate terms
containing "skill" (the skill system's documentation corpus), `onto_anchor`
reasoning about skills, and the derived-registry discipline that a term
this load-bearing should not sit on the coarse core when a pinned published
authority defines it.

**DEFER the remaining Skillware terms** (`skillware_unit`,
`lifecycle_continuity`, `agent_host` as standalone rulings): zero code
usage today. Trigger: load-bearing usage in docs or chunk candidate terms.

**Falsifier:** a published vocabulary already anchoring "skill" — probed:
core, none. A mismatch between the paper's category and zed-kask skills —
checked against the paper's three necessary conditions (C1 behavioral
primacy: SKILL.md specifies task behavior; C2 independent software
identity: named, git-versioned units; C3 Agent Host execution: the agent
loads and executes them) — upheld.

### 2.3 Loop 3 — review Options 2/3 → Option 2 REJECT-as-unneeded; Option 3 = Loop 1

**Option 2 (persist per-term alternatives):** the anchoring review's
Finding C — `canonicalize_terms` keeps only each term's primary sense;
alternatives are recoverable by re-resolution. **Resolution path: do not
build.** The resolver is in-process and deterministic, so re-resolution is
cheap; persisting alternatives is a schema change plus a
`published-term-resolution` protocol bump plus re-tagging of existing
corpora — storage and migration cost for retrievability that re-resolution
already provides. No named consumer exists. **Trigger:** a domain-filtered
retrieval consumer that needs at-rest secondary positions.

**Option 3 (extend the selector):** resolved by Loop 1 (defer with trigger).

### 2.4 Loop 4 — coarse-core terms → two rulings proposed, two left coarse

| Term | Live probe (2026-10-13) | Path |
|---|---|---|
| `training` | core | **PROPOSE a ruling** (§3.2) citing ISO/IEC 22989:2022 3.3.15 — verified verbatim from the ISO OBP, cite-only. Load-bearing for ML research-content candidate terms; the former training skill/server consumers were removed 2026-10-17. Grill-me check: no `cso:training` topic exists in 3.5 (grep: 0) and no published sense matches, so a derived ruling is the honest path, not a fabrication over a published anchor |
| `narrative` | core | **PROPOSE a ruling** (§3.3) citing GOLEM v1.1 — the narrative-domain ontology. Lower priority: the axis keyword `narrative` already routes artifacts to GOLEM (`axis.rs:273-288`); the ruling adds the term path for chunk candidate terms |
| `personal knowledge` | core | **LEAVE COARSE.** No load-bearing usage found; the core's ruling-request note is the honest design, not a gap |
| `knowledge asset` | core | **LEAVE COARSE.** Same |

### 2.5 Loop 5 — SEON (approval #3) → CLOSE as surveyed-not-adopted

The survey characterized both SEONs (the NEMO/UFES network, EKAW 2016; and
se-on.org, Software Evolution ONtologies — never conflated). Approval #3
was never granted. **Resolution path: close the loop** — record SEON as
surveyed-not-adopted in the adoption record's open register (it already
reads "not granted; any SEON adoption waits on the operator" —
`research/software-ontology-adoption.md` §8). Nothing to build; nothing to
keep open. Reopen only on an explicit operator grant.

### 2.6 Loop 6 — the `c180aadc` goal score → the operator's move, named precisely

The goal chain in curator memory carries **only the judge h_mem**
(`92cd51db`, verdict done — re-verified this session); the create event
was lost at its turn's end. Per the goal-loop rule, score publication uses
the create record as its Brier-calibration anchor, so a score event will
ingest but cannot publish. **Resolution path: the operator scores**
(`achieved=true` per the judged criteria), and the closure for the
create-less chain is the operator-approved batch record. Nothing here is
mine to execute.

### 2.7 Loop 7 — minor items → one blocked, one already recorded

- **ISO 19011 4th edition (2026) redline:** BLOCKED — the ISO OBP serves a
  bot challenge (403) on the ed-4 escaped fragment; the redline comparison
  cannot be verified from the primary source. The 2018 definitions stand as
  cited (verified verbatim earlier from the OBP; a revision is not a
  retraction). Evidence gap recorded, not chased.
- **Salience revisit condition:** already recorded in the anchoring review
  §5 (raw candidate terms over canonical concepts, correct while coarse-core
  resolution is common).

## 3. The proposed rulings (adopted 2026-10-13 — "proceed as proposed")

The three drafts below were adopted by the operator's 2026-10-13
"proceed as proposed" and landed in the derived registry the same day
(`skill`, `training`, `narrative`), pinned by
`open_loop_rulings_resolve_derived_with_resolved_constituents`. The drafts
are retained as the ruling record.

### 3.1 `skill` — authority: the vendored Skillware paper

- **term:** `skill`; **aliases:** `agent skill`, `skill artifact`
- **identity:** persistent natural-language behavioral source specifying
  reusable task behavior, managed as an independently identified software
  unit, activated by a skills-compatible agent host
- **definition** (paper text verbatim, from the pinned PDF): a Skill
  Artifact is "an addressable artifact whose primary behavioral source
  specifies reusable task behavior for activation and execution by a
  skills-compatible agent" (§3.2); a Skillware Unit is "the separately
  addressable identity that acquires, activates, versions, maintains, and
  removes the carrier" (§3.2). Three necessary conditions operationalize
  category membership: "behavioral primacy, independent software identity,
  and an Agent Host execution relationship" (abstract). In zed-kask a skill
  is the SKILL.md + templates unit loaded by `agent_skills` — the system's
  capability unit.
- **constituents:** `task`, `agent host` (`task` resolves published —
  `omc:Task` primary, `mls:Task` alternative, live-probed; constituent
  resolution re-verified at landing)
- **authority:** operator ruling <date>; Fan & Lan, Skillware: A Software
  Ontology and Engineering Lifecycle for Persistent Behavioral Artifacts,
  arXiv:2607.18970v3 (vendored pin, `SOURCES.lock` line 251)

### 3.2 `training` — authority: ISO/IEC 22989:2022 (cite-only)

- **term:** `training`; **aliases:** `model training`
- **identity:** process to determine or improve the parameters of a machine
  learning model by using training data
- **definition** (ISO OBP, cross-references elided): "process to determine or
  to improve the parameters of a machine learning model, based on a machine
  learning algorithm, by using training data" (ISO/IEC 22989:2022, 3.3.15;
  the source text carries inline cross-references to 3.1.23, 3.2.10 and
  3.3.16). The machine-learning sense; the nameable violation is applying it
  to non-ML senses (workforce training). In zed-kask it classifies the ML
  concept when it appears in research content; no model-training executor
  remains after the 2026-10-17 capability removal.
  Cite-only: ISO bytes are copyrighted, never vendored.
- **constituents:** `machine learning` (resolves published — `cso:machine_learning`,
  live-probed), `model` (resolves published — `mls:Model`, live-probed)
- **authority:** operator ruling <date>; ISO/IEC 22989:2022, 3.3.15

### 3.3 `narrative` — authority: GOLEM v1.1

- **term:** `narrative`
- **identity:** a structured account of events and characters with
  narrative inference relations, modeled domain-independently of any single
  work
- **definition:** the GOLEM sense (Pianzola et al., GOLEM Ontology v1.1 —
  narrative and fiction as interconnected systems: events, characters,
  social relationships, settings, narrative inference), extending CIDOC-CRM
  and LRMoo. At landing, quote the vendored `golem.ttl` definition text.
- **constituents:** `character` (resolves published — `omc:Character`
  primary, live-probed), `setting` (verify at landing)
- **authority:** operator ruling <date>; Pianzola et al., GOLEM Ontology
  v1.1, doi:10.5281/zenodo.14911396 (vendored pin)

## 4. What this proposal deliberately does not build (essentialist)

The essentialist gate ran per loop: **does the resolution path deserve to
exist?** Verdicts — Loop 1: no (dead arms); Loop 2: one ruling yes, three
terms no; Loop 3 Option 2: no (re-resolution already provides the
capability); Loop 4: two terms yes, two no; Loop 5: nothing (close); Loop 6:
nothing (the operator's); Loop 7: nothing (one blocked, one recorded). The
proposal adds **at most three registry entries and zero code** — everything
else is closure, deferral with triggers, or an evidence gap recorded
honestly. The net shape: of seven open loops, three close now (5, 6, 7),
two close pending the operator's ruling adoption (2, 4), one defers with a
trigger (1), one is rejected as unneeded (3-Option-2).

## 5. Coverage honesty (code-review discipline)

**Checked:** live probes against the running binary for every term claimed
core or resolved (`skill`, `training`, `task`, `behavior`, `narrative`,
`personal knowledge`, `knowledge asset` — 2026-10-13, post-rung-2 rebuild);
grep of CSO 3.5 for a training topic (0); grep of `kask/` for Skillware-term
usage (0); the full keyword-arm reading of `axis.rs:207-392`; the condenser
consumer at `kask/crates/hkask-condenser/src/engine.rs`,
`CondenserEngine::compress`; the goal chain (judge h_mem only); the
Skillware paper pp. 1–6 extracted verbatim from the vendored pin.

**Not checked:** the Skillware paper beyond p. 6 (the definitions used are
from pp. 1–6; the landing step quotes from the pinned bytes and may pull
more pages); the ISO 19011 4th-edition redline (OBP bot challenge — §2.7);
an exhaustive tool-name enumeration beyond the axis keyword arms (the
deferral's falsifier names this: a privacy- or topic-hinting tool name would
reverse it); constituent resolution for `agent host` and `setting` (marked
"verify at landing" in §3).

**Residual risk:** a ruling adopted for `skill` could later be outranked if
a pinned vocabulary publishes the term — the ladder puts domain supplements
before derived, so the ruling would become an alternative, not a
fabrication; acceptable and reversible.