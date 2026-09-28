# hkask-bridge-ontology

Ontology bridge — the single source of truth for ontology vocabulary and the
dual-axis domain-selection logic in hKask.

## Architectural invariant

Ontologies are domain maps; MCP servers are functional-area maps; these are
orthogonal. No ontology vocabulary lives inside an MCP server. Every server
that does tagging depends on this crate.

## Two universal axes (P5.4) + domain supplements (P8.1)

- **State axis** — Dublin Core + BIBO + CiTO (`dc_bibo`): the "what is this"
  noun dimension. Always available.
- **Process axis** — PKO (`pko`): the "how did this come to be" verb dimension.
  Always available.
- **Domain supplements** — FIBO, SEPIO, GOLEM, ML-Schema and OMC:
  layered on top where the universal axes aren't specific enough. SDMX
  currently supplies local identifiers over Information Model class names,
  not a pinned RDF/OWL vocabulary.

The invariant: one axis is always Dublin Core or PKO, so every artifact has a
common mapping in process or state space regardless of domain.

## Modules

| Module | Ontology | Axis |
|--------|----------|------|
| `dc_bibo` | Dublin Core + BIBO + CiTO | State (universal) |
| `pko` | Procedural Knowledge Ontology | Process (universal) |
| `fibo` | Named FIBO constants; the full pinned Q2 **Release** selection resolves through `published` | Domain supplement (financial) |
| `sepio` | Scientific Evidence and Provenance Information Ontology | Domain supplement (scientific evidence) |
| `golem` | GOLEM narrative ontology | Domain supplement (narrative) |
| `ml_schema` | ML-Schema | Domain supplement (ML training) |
| `sdmx` | SDMX Information Model class-name identifiers (fixture-backed, not official RDF/OWL URIs) | Domain supplement (statistics) |
| `omc` | MovieLabs Ontology for Media Creation | Domain supplement (media) |
| `axis` | Domain-selection logic | `OntologyAxis`, `OntologyNamespace`, `OntologyAnchor`, `select_ontology_anchor` |
| `sumo` | Named SUMO concepts emitted by hKask code | Upper ontology |
| `schema_org` | Curated schema.org predicate menu for assertion extraction | General vocabulary |
| `published` | Pinned published-source index (see coverage and exceptions below) | `terms`, `get`, `lookup`, `contains` |
| `term_resolution` | Exact fallback-ladder resolution | `resolve_term`, `canonicalize_terms`, `TERM_RESOLUTION_PROTOCOL` |
| `ontology_graph` | Bounded, source-backed concept relations | `graph().traverse(from, to, max_hops)` |

## Usage

```rust
use hkask_bridge_ontology::{dc_bibo, pko, fibo, axis};

// Universal vocabulary.
let title = dc_bibo::TITLE;            // "dcterms:title"
let step = pko::STEP_EXECUTION;        // "pko:StepExecution"

// Named FIBO constants (Release-backed; the full Release resolves through published).
let corp = fibo::CORPORATION;             // "fibo-be-le-cb:Corporation"
let mcap = fibo::MARKET_CAPITALIZATION;   // "fibo-ind-mkt-bas:MarketCapitalization"

// Domain selection.
let anchor = axis::select_ontology_anchor("prediction-markets");

// Model output supplies descriptive candidates only; the bridge owns authority.
let terms = hkask_bridge_ontology::term_resolution::canonicalize_terms([
    "corporation",
    "quantity",
]);
assert_eq!(terms.ontology_tags["fibo"], [fibo::CORPORATION]);
```

## Full published vocabularies (`sources/`)

`sources/SOURCES.lock` pins each vendored file by upstream URL, version,
SHA-256 and license. `build.rs` verifies the pins and compiles an embedded
`published` index with concept IDs, kinds, names/labels, directly stated
parents and inverse properties, source files, and the source's definition
**when one is supplied**. `published::lookup` can return multiple exact senses;
named constants in the bridge modules are consumer menus, not the extent of the
published index. No OWL/KIF axioms are evaluated.

Current indexed coverage: the complete pinned SUMO distribution (excluding
`tiny*` test subsets); all layers of schema.org 30.1; DCMI terms/types,
BIBO, CiTO, PKO with P-Plan and PROV, SEPIO, GOLEM with CIDOC-CRM and LRMoo,
OMC, ML-Schema, and RDF/RDFS from their pinned published sources. FIBO is the
pinned Q2 **Release** maturity selection: 157 modules and 6,443 indexed terms;
its Provisional modules are **not** indexed. `fixtures/fibo-verified-terms.txt`
retains the selected module/namespace bindings, reconciled by `build.rs`
against the pinned Release sources — it is not a tiny term-only substitute.
For source versions and individual license terms, consult `sources/SOURCES.lock`
(SUMO's `Merge.kif` carries the IEEE notice).

Two explicit exceptions: `sdmx::ALL_CONCEPTS` is a fixture-checked local
rendering of SDMX Information Model class names; `sdmx:` is **not** an official
published RDF/OWL URI namespace, and SDMX is not in the published-source index.
The five `dlp:` constants used by `golem` remain pending a licensable pinned
DOLCE-Lite-Plus source; they do not gain source-backed definitions by proximity
to GOLEM. Do not present either as fully loaded.

`resolve_term` walks: pinned domain sources (FIBO Release first) and the local
SDMX identifier registry → derived concepts → full SUMO (`upper`) → full
schema.org (`general_vocabulary`) → published Dublin Core/BIBO/CiTO
(`state_axis`) → 5W1H core. The first sense is the resolution; other found
senses appear in `alternatives`, with each published sense's source and its
definition when present. Artifact-axis selection via `axis` is separate.
`TERM_RESOLUTION_PROTOCOL` is `published-term-resolution-v2`; older tagged
records require re-tagging, not an implicit upgrade. To update a vocabulary,
re-vendor from the new pinned version and update the lock, never hand-edit a
source.

## Thin dependency surface

Pure Rust vocabulary + selection logic with serialization/schema derives; the
only build dependency is `sha2` for source pinning. No reasoners, OWL
reasoning or graph databases. The read-only concept graph contains only
sourced edges: derived-concept constituents that resolve to distinct
published/derived identities, and the parents and inverse properties each
published source row states directly. No inferred or transitive edge is added.
A path connects concepts, not instances; an absent path is not a negative fact.
Directed BFS discovers at most 256 nodes and 4 hops; neighbor queries return
at most 256 outgoing edges. Both return an explicit budget status instead of
silently reporting an incomplete result as absence. Agent calls without
`relation_query` retain the original `onto_anchor` JSON shape.
