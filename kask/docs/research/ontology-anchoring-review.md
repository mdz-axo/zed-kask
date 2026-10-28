---
title: "Ontological Anchoring — Hierarchy, Methodology, and Multi-Domain Chunk Linking (Review)"
audience: [architects, developers, operators, agents]
last_updated: 2026-10-13
version: "1.0.0"
status: "Active"
domain: "Ontology"
mds_categories: [domain, composition, trust]
---

# Ontological Anchoring — Hierarchy, Methodology, and Multi-Domain Chunk Linking (Review)

## 1. Purpose and scope

Two questions, both operator-posed:

1. **Hierarchy and methodology** — how does ontological anchoring work across
   the different domain spaces zed-kask serves, and which tier serves which
   domain, why?
2. **Multi-domain linking** — when a data item or chunk touches several
   domains, how does it anchor into a position in *each* of them? What does
   the current design carry, and what does it not?

This is a review: the findings land as a decision-ready proposal, and every
adoption decision stays with the operator. Grounding is the tree (cited
file:line) plus live `onto_anchor` probes against the running binary
(2026-10-13, post-rung-2 rebuild). Companions: `reference/native-ontologies.md`
(the inventory), `reference/ontology-bridge.md` (the API), and
`research/software-ontology-adoption.md` (the adoption trail).

## 2. The hierarchy: two distinct anchoring mechanisms

The system has **two** anchoring mechanisms, and conflating them is the
review's first finding. They answer different questions and are consumed by
different paths.

### 2.1 Term resolution — the chunk/content path (multi-domain by design)

`term_resolution::resolve_term` walks the fallback ladder once per term:
domain supplements (in a fixed namespace order) → derived rulings → SUMO →
schema.org → the DC/BIBO/CiTO state axis → the 5W1H core
(`kask/crates/hkask-bridge-ontology/src/term_resolution.rs:115-131,190-238`).
The first matching sense is primary; every other sense found is listed in
`alternatives` with source and definition. Matching is exact — concept ID,
local name, or published label under normalization (lowercase alphanumeric
only, `kask/crates/hkask-bridge-ontology/src/published.rs:52-57,147-157`) —
never fuzzy, because a false specific anchor is worse than the real but
coarse ground.

`canonicalize_terms` (`kask/crates/hkask-bridge-ontology/src/term_resolution.rs:242-280`)
resolves **each candidate term independently** and aggregates:

- `candidate_terms` — the raw descriptive terms, preserved as-is (coarse
  resolution never erases them);
- `ontology_tags: HashMap<namespace, Vec<concept>>` — the resolver-derived
  map, keyed by lowercase namespace, carrying **every namespace hit**;
- `concepts` — the first-seen union of `ontology_tags` values, for salience
  and retrieval.

**This is the multi-domain record.** A chunk whose candidate terms are
`["inference", "model", "storage duration"]` anchors into three domains at
once: CSO (`cso:inference`), ML-Schema (`mls:Model`), and DPV
(`dpv:StorageDuration`) — each term through its own ladder walk, all three
namespaces present in one chunk's `ontology_tags`. The classifier (LLM)
never assigns namespaces; it supplies raw terms only, and the shared bridge
resolver owns all authority (`kask/crates/hkask-types/src/corpus.rs:265-267,325-338`).

### 2.2 Axis selection — the artifact/tool path (single-pick)

`axis::select_ontology_anchor(domain_hint)` picks **one** anchor for an
artifact or tool-derived context by keyword-matching the hint: a domain
supplement (RDF Data Cube, SEPIO, GOLEM, ML-Schema, OMC), a dual-axis
anchor (PKO process or DC+BIBO state), SUMO fallback, or the core
(`kask/crates/hkask-bridge-ontology/src/axis.rs:207-390`). The dual-axis
invariant (P5.4): one axis is always Dublin Core or PKO, so every artifact
has a common mapping regardless of domain
(`kask/crates/hkask-bridge-ontology/src/axis.rs:1-16`).

**Finding A (corrected same-day) — the axis selector has one production
consumer: the condenser.** An earlier draft of this review claimed zero
consumers, read from a paginated grep whose first page happened to contain
only `axis.rs` matches — the truncated-sweep trap. The full count finds
`CondenserEngine::compress` in `kask/crates/hkask-condenser/src/engine.rs`
calling `select_ontology_anchor(tool_name)` on every tool-result compression
(the anchor's tier label feeds telemetry and its density expectation feeds
saliency within the selected algorithm, not category-to-algorithm dispatch),
with its test at `kask/crates/hkask-condenser/src/algorithms.rs`,
`tests::derive_and_compress_with_correct_anchor`, and type imports at
`types.rs:20`. The portfolio widget remains a second,
independent anchoring path — its own per-metric ladder walk (D54) — that
does not go through the selector. The error is recorded here, not smoothed
over.

**Finding B — the axis selector's namespace set predates the rung-1 pins
(live via the condenser).** `OntologyNamespace`
(`kask/crates/hkask-bridge-ontology/src/axis.rs:49-68`) carries SEPIO,
GOLEM, ML-Schema, Data Cube, OMC, SUMO — **no DPV, no CSO**. The
condenser routes every tool name through this enum, so a tool name hinting
at privacy or research-topic vocabulary falls to the SUMO arm instead of
the vocabularies that now own those domains. Practical impact today is
small — the anchor feeds compression density expectations, not
correctness (`axis.rs:125-147`), and few tool names hint privacy or
cso topics — but the gap is live, not latent. (The module doc's
illustrative supplement list also lagged the enum; corrected in the same
pass as this review.)

## 3. Methodology per domain space

Which tier serves which domain, and why — the hierarchy's working map:

| Domain space | Primary tier | Vocabulary | Why this tier |
|---|---|---|---|
| Machine learning | domain_supplement + derived | ML-Schema; CSO topics; the fine_tuning ruling | ML-Schema owns the experiment/process vocabulary; CSO owns the research-area taxonomy; composed rulings (bayesian network, entropy) sit derived |
| Computer-science research topics | domain_supplement | CSO 3.5 | The Klink-2 taxonomy of CS research areas — `inference`, `fine-tuning learning` |
| Data governance / privacy | domain_supplement | DPV 2.3 | `dpv:StorageDuration`, `dpv:Audit`, `dpv:ServiceLevelAgreement` — the personal-data processing vocabulary |
| Media production | domain_supplement | OMC 2.8 | Creative works, assets, capture-to-post workflows |
| Narrative / fiction | domain_supplement | GOLEM (extending CIDOC-CRM + LRMoo) | Narrative events, characters, settings — domain-general, not work-specific |
| Bibliographic (WEMI) | domain_supplement | LRMoo 1.0 | Work/Expression/Manifestation/Item — `lrmoo:F1_Work` |
| Cultural heritage | domain_supplement | CIDOC-CRM 7.1.3 | `crm:E31_Document` and the curated-event vocabulary |
| Scientific evidence | domain_supplement | SEPIO | Propositions, evidence lines, support/dispute |
| Plans / steps / provenance | domain_supplement | P-Plan 1.3, PROV 2013, PKO 2.0.0 | `pplan:Step`, `prov:Agent`, `pko:Procedure` — the process axis |
| Statistics (RDF) | domain_supplement | RDF Data Cube | `qb:Observation`, `qb:DataSet` — the spreadsheet wire contracts' published ground |
| Cross-domain formal | upper | SUMO | The universal formal fallback — `sumo:evidence`, `sumo:Game` |
| General web | general_vocabulary | schema.org 30.1 | After SUMO so a formal category is preferred |
| Artifact typing | state_axis | Dublin Core, BIBO, CiTO | Visible as alternatives, never outranking a formal category (the 2026-09-10 BIBO ruling) |
| Unmatched | core | 5W1H | Real but coarse, carrying the ruling-request note — never a private definition |

The methodology invariants, each pinned by tests: exact match only;
every sense listed in `alternatives`; nothing ever untagged (the ladder
always terminates); derived constituents must themselves resolve through
the ladder (never a fabricated anchor); every derived entry cites its
authority.

## 4. Multi-domain chunk linking — what the current design carries

### 4.1 What is carried

A `TaggedChunk` (`kask/crates/hkask-types/src/corpus.rs:277-338`) carries,
per chunk:

- up to **5** candidate terms (`MAX_CANDIDATE_TERMS`,
  `kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs:32-35`);
- the **multi-namespace** `ontology_tags` map — the chunk's position in
  every domain its terms anchor into;
- the flat `concepts` union, consumed by salience and retrieval;
- `dc_subject` (the normalized candidate terms) and `dc_type` — the
  dual-axis record (`ChunkOntology`,
  `kask/crates/hkask-types/src/corpus.rs:226-236`);
- the `published-term-resolution-v2` protocol stamp, with reconciliation
  required before QA generation (`has_current_canonical_terms`,
  `kask/crates/hkask-types/src/corpus.rs:367-371`).

Downstream, the multi-namespace map is read where domains matter:
`read_ontology_tags` renders per-namespace summaries and
`read_ontology_namespaces` lists the touched domains per chunk
(`kask/mcp-servers/hkask-mcp-corpus/src/tools/semantic/ontology_io.rs:27-60`).
Consolidation merges cluster members' concepts and re-canonicalizes
(`kask/mcp-servers/hkask-mcp-corpus/src/services/consolidation.rs`, `ConsolidationService::consolidate`).
Salience deliberately uses the **raw** `candidate_terms`, not the canonical
concepts, because canonical anchors are too coarse for the graph when exact
resolution correctly reaches the shared 5W1H core
(`kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs:187-202`).

### 4.2 What is not carried — the gaps

**Finding C — per-term alternatives are dropped at persistence.** The
`onto_anchor` tool returns every sense (`service` → `omc:Service` primary
with DPV, SUMO, schema.org and Dublin Core alternatives live-probed), but
`canonicalize_terms` keeps only each term's **primary** resolution; the
alternatives never reach `ontology_tags`. A chunk whose term "service"
resolves OMC-primary loses its DPV and SUMO positions at rest. The
multi-domain record is therefore **primary-sense-per-term**, not
every-sense-per-term.

**Finding D — `concepts` is a flat union.** The namespace structure survives
in `ontology_tags` but the retrieval/salience union (`concepts`) is flat:
a consumer of `concepts` cannot tell which domain a concept came from
without re-reading the map.

**Finding E — the 5-term cap bounds domain breadth.** A chunk touching six
domains needs six well-chosen terms; the classifier emits at most five
(3–5 validated), so breadth is bounded by design. This is a deliberate
precision-over-breadth trade, not an accident — but it is a real ceiling on
how many domain positions one chunk can record.

**Finding F — the two mechanisms do not compose.** The artifact-level
selector picks one ontology per artifact; the term-level resolver picks one
primary sense per term across many ontologies. There is no path today that
says "this artifact is an OMC artifact, and this chunk within it also holds
a DPV position" — the artifact anchor (the condenser's tool-name-derived
pick) and the chunk's term anchors live on separate surfaces. This is a
design observation, not a live loss: no consumer today needs both on one
record.

### 4.3 What a multi-domain anchoring would need (decision-ready)

Three options, with functional consequences. **No adoption is made by this
review** — each stays with the operator:

- **Option 1 — record the gaps, change nothing.** The primary-sense map
  already answers "which domains does this chunk touch." Consequence: a
  chunk's *secondary* positions (the dropped alternatives) are recoverable
  only by re-resolving its terms — cheap (the resolver is in-process and
  deterministic) but not queryable at rest. Recommended **unless** a
  retrieval or QA consumer manifests a need.
- **Option 2 — persist per-term alternatives.** Extend `TaggedChunk` with
  per-term alternative senses (or a per-domain position map). Consequence:
  a schema change on the chunk record — a `published-term-resolution`
  protocol bump and re-tagging of existing corpora, plus storage growth
  proportional to sense count. Benefit: a chunk's position in every touched
  domain survives persistence and becomes queryable. Worth it only when a
  named consumer (e.g. domain-filtered retrieval) exists.
- **Option 3 — extend the axis selector (DPV/CSO) and compose it with term
  resolution.** Consequence: `OntologyNamespace` gains variants; keyword
  routing for privacy (`dpv`) and research-topic (`cso`) hints; the
  artifact anchor and chunk anchors could then share a vocabulary. The
  selector is consumed (the condenser, Finding A), so an extension is not
  speculative generality — but it is a design decision: density factors to
  set per variant (`axis.rs:125-147`) and keyword arms to choose. Worth
carding as an operator decision on Finding B, not landing unprompted.

**Recommendation:** Option 1 now (this document is the record); Option 2
carded awaiting a named consumer; Option 3 carded as an operator decision
on Finding B (the selector is consumed, so the extension is live design
work, not speculative). The cheap exception already taken: the stale axis
module doc corrected in the same pass (a comment fix, no behavior change).

## 5. Open register

- Finding B (DPV/CSO missing from `OntologyNamespace`) is live via the
  condenser; the extension — with density factors per variant — is an
  operator decision, carded as Option 3.
- Options 2 and 3 await their named consumers; adoption is the operator's.
- The salience design (raw terms over canonical concepts) is deliberate and
  correct while coarse-core resolution is common; revisit only if the
  derived registry grows enough that canonical concepts become reliably
  specific (the salience comment's own condition).