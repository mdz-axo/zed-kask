---
title: "Native Ontologies — Comprehensive Inventory"
audience: [developers, operators, agents]
last_updated: 2026-10-13
version: "1.0.0"
status: "Active"
domain: "Ontology"
mds_categories: [domain, trust, lifecycle]
---

# Native Ontologies — Comprehensive Inventory

## 1. What this inventory is

Every ontology vocabulary available natively in zed-kask, what each covers,
its license, and what resolving a term through it lets the operator and
agents do. The bytes live under
`kask/crates/hkask-bridge-ontology/sources/`, each file pinned in
`SOURCES.lock` (path, upstream URL, version, sha256, license) with
`verify_lock` refusing the build on sha256 drift
(`kask/crates/hkask-bridge-ontology/build.rs:35-97`). Resolution is exact,
never fuzzy: a term matches a concept ID, a local name, or a published label
under normalization (lowercase alphanumeric only,
`kask/crates/hkask-bridge-ontology/src/published.rs:52-57,147-157`).

Companions: `ontology-bridge.md` is the crate's API reference (modules, tag
shape, the axis selector); `research/software-ontology-adoption.md` is the
decision trail for what was adopted and why. This document is the inventory.

Current counts: **248 pinned files across 20 source directories** — 19
indexed vocabularies plus one non-RDF authority pin — and **94 derived
rulings** in the registry.

## 2. The resolution ladder in functional terms

`resolve_term` walks the rungs in a fixed order
(`kask/crates/hkask-bridge-ontology/src/term_resolution.rs:115-131,190-238`);
the first sense found is the resolution and every other sense found is
listed in `alternatives` — never dropped.

| Tier | What it is | What resolving here lets you do |
|---|---|---|
| `domain_supplement` | A pinned published domain vocabulary (RDF Data Cube, OMC, PKO, P-Plan, PROV, SEPIO, GOLEM, CIDOC-CRM, LRMoo, ML-Schema, RDF, RDFS, DPV, CSO) | Cite the published source's own definition — the claim is checkable against sha256-pinned bytes, and the resolution names the source file and version |
| `derived` | A recorded composition over ladder-resolved constituents, with identity and authority (`kask/crates/hkask-bridge-ontology/src/derived.rs:47-913`) | Use an operator ruling or a published standard as the term's authority; the identity is machine-checkable and the violation nameable |
| `upper` | The full SUMO distribution (65 pinned files) | Fall back on a formal upper-ontology category with SUMO's own documentation text |
| `general_vocabulary` | The full schema.org 30.1 release | Fall back on the general web vocabulary with its definitions |
| `state_axis` | Dublin Core, BIBO, CiTO | Artifact-typing senses stay visible as alternatives but never outrank a formal category |
| `core` | `5w1h_core`, the interrogative ground | A guaranteed real but coarse anchor carrying a ruling-request note — never a private definition |

Domain order within `domain_supplement` is fixed (DPV and CSO
last so no earlier resolution shifts); DPV and CSO claim only terms no
earlier vocabulary already anchors.

## 3. The vendored vocabularies

| Vocabulary | Prefix(es) | Version | License | Domain |
|---|---|---|---|---|
| SUMO | `sumo` | `ontologyportal/sumo@77f8fdd2` | GPL (README); IEEE file headers | Formal upper ontology |
| schema.org | `schema` | 30.1 (2026-09-16) | CC BY-SA 3.0 | General web vocabulary |
| RDF Data Cube | `qb` | 2014 Recommendation | PDDL 1.0 | Statistical data |
| OMC | `omc`, `omcT`, `cw` | 2.8 | Apache-2.0 | Media production |
| PKO | `pko` | 2.0.0 | CC BY 4.0 | Procedural knowledge |
| P-Plan | `pplan` | 1.3 | CC BY 4.0 | Plans and steps |
| PROV | `prov` | 2013-04-30 Recommendation | W3C Software and Document License | Provenance |
| SEPIO | `SEPIO` | 2023-06-13 release | CC BY 3.0 | Scientific evidence |
| GOLEM | `gc` | 1.1 | CC BY 4.0 | Narrative and fiction |
| CIDOC-CRM | `crm` | 7.1.3 | CC BY 4.0 | Cultural heritage |
| LRMoo | `lrmoo` | 1.0 | CC BY 4.0 | Bibliographic (WEMI) |
| ML-Schema | `mls` | 1.0 (2016-10-17) | W3C CCLA | Machine learning |
| RDF / RDFS | `rdf`, `rdfs` | 2019-12-16 / 2024-12-18 | W3C Software and Document License | Web-of-data core |
| Dublin Core | `dcterms`, `dcmitype`, `dc`, `dcam` | 2020-01-20 | CC BY 4.0 | Metadata and artifact typing |
| BIBO | `bibo` | 1.3 | CC BY 4.0 | Bibliography |
| CiTO | `cito` | 2026-09-03 | CC BY 4.0 | Citation typing |
| DPV | `dpv` | 2.3 | W3C Software and Document License | Data privacy |
| CSO | `cso` | 3.5 (2025-07-29) | CC BY 4.0 | Computer-science research topics |
| Skillware (paper) | — (not RDF-indexed) | arXiv:2607.18970v3 | CC BY 4.0 | Software-process authority |

Namespace IRIs and directory bindings are declared in
`RDF_VOCABULARIES` (`kask/crates/hkask-bridge-ontology/src/published_sources.rs:523-656`).

### What each resolves, with live-probed examples

- **SUMO** — the universal formal fallback. `evidence` resolves
  `sumo:evidence` (Law.kif); `Game` resolves `sumo:Game` (test-pinned).
- **schema.org** — general terms with definitions. `citation` resolves
  `schema:citation` as primary — a good illustration that the general
  vocabulary outranks the state axis (CiTO's sense appears as the
  alternative).
- **SUMO** — formal upper ontology. `corporation` resolves
  `sumo:Corporation` ("An Organization that has a special legal status..."),
  the anchor for corporation terms since the FIBO rung was removed
  (2026-10-18). `quantity` resolves `sumo:Quantity`.
- **RDF Data Cube** — statistical shapes. `observation` resolves
  `qb:Observation`.
- **OMC** — media production. `service` resolves `omc:Service`;
  `character` resolves `omc:Character` ("a sentient entity... whose
  specific identity is consequential to the narrative").
- **PKO** — procedures and executions. `procedure` resolves
  `pko:Procedure` ("a sequence of actions to be executed in order to
  achieve a desired outcome").
- **P-Plan** — plans and steps. `plan` resolves `pplan:Plan`; `activity`
  resolves `pplan:Activity` (the execution of a planned step) — the
  process axis the kanban and edit-transaction rulings build on.
- **PROV** — provenance. `agent` resolves `prov:Agent` ("something that
  bears some form of responsibility for an activity taking place...").
- **SEPIO** — scientific evidence. `proposition` resolves
  `SEPIO:0000000` ("the sharable meaning of what is expressed in a
  particular assertion") — the grounding-verify evidence discipline's
  vocabulary.
- **GOLEM** — narrative and fiction, extending CIDOC-CRM and LRMoo.
  The bare term `narrative` resolves through the derived `narrative`
  ruling (operator ruling 2026-10-13), anchored on GOLEM's own ontology
  description — GOLEM's topics are `gc:`-prefixed and none matches the
  bare word under exact normalization, so the ruling is the honest path,
  never a fabricated published anchor.
- **CIDOC-CRM** — cultural heritage. `document` resolves
  `crm:E31_Document` ("identifiable immaterial items that make
  propositions about reality"); `activity` lists `crm:E7_Activity` as an
  alternative under P-Plan's primary.
- **LRMoo** — the bibliographic Work/Expression/Manifestation/Item stack.
  `work` resolves `lrmoo:F1_Work` ("distinct intellectual ideas conveyed
  in artistic and intellectual creations").
- **ML-Schema** — machine learning. `model` resolves `mls:Model`;
  `process` resolves `mls:Process`. The fine-tuning ruling's constituents
  resolve here and in CSO.
- **RDF / RDFS** — the web-of-data core. `class` resolves `rdfs:Class`
  ("the class of classes").
- **Dublin Core** — metadata and artifact typing. `agent` lists
  `dcterms:Agent`; `service` lists `dcmitype:Service` — state-axis
  alternatives, never primary over a domain sense.
- **BIBO** — bibliography. `document` lists `bibo:Document` as an
  alternative under CIDOC-CRM's primary.
- **CiTO** — citation typing. `citation` lists `cito:Citation` (the
  conceptual directional link from a citing to a cited entity) as the
  alternative under schema.org's primary.
- **DPV** — data privacy. `storage duration` resolves
  `dpv:StorageDuration`; `audit` resolves `dpv:Audit`; `service level
  agreement` resolves `dpv:ServiceLevelAgreement` — the rung-1 pin that
  resolved three of the five coarse-anchor rulings.
- **CSO** — computer-science research topics. `inference` resolves
  `cso:inference`; `machine learning` resolves `cso:machine_learning`;
  `fine-tuning learning` resolves `cso:fine-tuning_learning` (CSO 3.5).
- **Skillware paper** — a pinned authority, not an indexed vocabulary:
  `read_rdf` skips non-RDF extensions
  (`kask/crates/hkask-bridge-ontology/build.rs:100-140`), so the PDF is
  sha256-verified provenance for future software-process rulings (skill
  artifact, skillware unit, lifecycle continuity) — none cites it yet.

## 4. The derived registry

Ninety-four rulings (`kask/crates/hkask-bridge-ontology/src/derived.rs:47-938`),
each a `DerivedConcept` with: canonical `term`, `aliases`, a
machine-checkable `identity`, a `definition` stating load-bearing
semantics so a violation is nameable, `constituents` that must themselves
resolve through the ladder (never a fabricated anchor), and an `authority`
— an operator ruling with its date, a published standard, or both. The
`every_entry_cites_authority` test pins the no-fabricated-definitions
discipline; resolution is exact under the same normalization
(`resolve_derived`, `kask/crates/hkask-bridge-ontology/src/derived.rs:1993`).

Grouped by the domains they serve: finance (the DuPont family, valuation,
expectations investing), forecasting and calibration (Brier, calibration,
superforecasting, EQM), software engineering (test harness, property-based
testing, Fagan inspection, deep module, strangler fig), cybernetics (VSM,
algedonic signal, feedback loop), the kata system (improvement kata,
coaching kata, gemba walk, PDCA), documentation (Diataxis, minimalism),
computation theory (entropy, determinism, syntax/semantics, the D/P
routing terms), maintenance (IEC
60050-192 preventive/condition-based), media (transcript-linked media),
and the ontology self-rulings (PKO, GOLEM, OMC, SEPIO as derived concepts
alongside their published vocabularies).

## 5. Honest coverage notes

- **Exact match only.** A term resolves only when it equals a concept ID,
  local name, or published label under normalization. Bare phrases that
  look like they should resolve but do not — `personal knowledge`,
  `knowledge asset` — stay on the coarse core with a ruling-request note,
  never a private definition. (`narrative` and `training` left this list
  2026-10-13 via derived rulings — GOLEM and ISO/IEC 22989:2022
  respectively.) That is the design: a false specific anchor is worse than
  the real but coarse ground.
- **Every sense is listed.** One word, several published senses: the
  ladder picks the primary by rung order and lists the rest in
  `alternatives` with source and definition — `agent` returns PROV
  primary with DPV, SUMO, schema.org and Dublin Core senses visible.
- **The index is the vocabulary.** The module constants are named consumer
  selections; every term in every pinned file is in the compiled index.
  Coverage claims in this document were probed live against the running
  binary (2026-10-13, post-rung-2 rebuild).

## Statistical interpretation warning — goal/forecast cutover

The live derived Brier anchor still supplies the correct binary formula but
calls raw loss calibration and constant .5 a universal baseline. This gloss is
not statistical authority. Murphy (1973), DOI
`10.1175/1520-0450(1973)012<0595:ANVPOT>2.0.CO;2`, separates reliability,
resolution and uncertainty; compare matched references and disclose populations.
See the approved goal-closure plan §2 for accessed-source limits and P6. The
ontology implementation is outside this docs cutover; the remaining warning is
routed to `skill_use_issue:onto-anchor` for the ontology owner, not silently
rewritten as a private definition or claimed fixed here.
