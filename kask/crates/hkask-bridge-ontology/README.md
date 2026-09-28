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
- **Domain supplements** — FIBO, SEPIO, GOLEM, ML-Schema, SDMX and OMC:
  layered on top where the universal axes aren't specific enough.

The invariant: one axis is always Dublin Core or PKO, so every artifact has a
common mapping in process or state space regardless of domain.

## Modules

| Module | Ontology | Axis |
|--------|----------|------|
| `dc_bibo` | Dublin Core + BIBO + CiTO | State (universal) |
| `pko` | Procedural Knowledge Ontology | Process (universal) |
| `fibo` | Financial Industry Business Ontology | Domain supplement (financial) |
| `sepio` | Scientific Evidence and Provenance Information Ontology | Domain supplement (scientific evidence) |
| `golem` | GOLEM narrative ontology | Domain supplement (narrative) |
| `ml_schema` | ML-Schema | Domain supplement (ML training) |
| `sdmx` | Statistical Data and Metadata eXchange | Domain supplement (statistics) |
| `omc` | MovieLabs Ontology for Media Creation | Domain supplement (media) |
| `axis` | Domain-selection logic | `OntologyAxis`, `OntologyNamespace`, `OntologyAnchor`, `select_ontology_anchor` |
| `sumo` | Named SUMO concepts emitted by hKask code | Upper ontology |
| `schema_org` | Curated schema.org predicate menu for assertion extraction | General vocabulary |
| `published` | The full SUMO distribution and schema.org release, compiled from `sources/` | `terms`, `get`, `lookup`, `contains` |
| `term_resolution` | Exact fallback-ladder resolution | `resolve_term`, `canonicalize_terms`, `TERM_RESOLUTION_PROTOCOL` |
| `ontology_graph` | Bounded, source-backed concept relations | `graph().traverse(from, to, max_hops)` |

## Usage

```rust
use hkask_bridge_ontology::{dc_bibo, pko, fibo, axis};

// Universal vocabulary.
let title = dc_bibo::TITLE;            // "dcterms:title"
let step = pko::STEP_EXECUTION;        // "pko:StepExecution"

// Domain vocabulary (verified FIBO terms, fixture-pinned).
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

The complete SUMO distribution (every ontology file of the pinned commit;
the `tiny*` test subsets excluded) and the complete schema.org release (every
layer) are vendored under `sources/` and pinned file-by-file in
`sources/SOURCES.lock` with upstream URL, version, sha256 and license.
`build.rs` refuses to build on drift, an unlisted file or a missing file, and
compiles the sources into an embedded index read by `published`: every term
with its labels, direct parents, inverse properties, published definition and
source file. To update a vocabulary, re-vendor the files from the new pinned
version and regenerate the lock rows — never edit a source by hand. Licenses:
SUMO is GPL (`Merge.kif` carries the IEEE notice); schema.org is CC BY-SA 3.0
(https://schema.org/docs/terms.html).

`resolve_term` walks: domain supplements → derived concepts → full SUMO
(`upper`) → full schema.org (`general_vocabulary`) → 5W1H core. The first
sense is the resolution, with the source's `definition` and `source`; every
other sense found is listed in `alternatives`.

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
