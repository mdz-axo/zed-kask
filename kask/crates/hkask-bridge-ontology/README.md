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
| `term_resolution` | Exact fallback-ladder resolution | `resolve_term`, `canonicalize_terms`, `TERM_RESOLUTION_PROTOCOL` |

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

## Thin dependency surface

Pure Rust vocabulary + selection logic with serialization/schema derives. No
reasoners, OWL parsing or graph databases. Bridges are thin vocabulary layers,
not ontology engines.
