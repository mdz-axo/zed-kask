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
- **Domain supplements** — FIBO, ESO, GOLEM, ML-Schema: layered on top
  where the universal axes aren't specific enough for a domain.

The invariant: one axis is always Dublin Core or PKO, so every artifact has a
common mapping in process or state space regardless of domain.

## Modules

| Module | Ontology | Axis |
|--------|----------|------|
| `dc_bibo` | Dublin Core + BIBO + CiTO | State (universal) |
| `pko` | Procedural Knowledge Ontology | Process (universal) |
| `fibo` | Financial Industry Business Ontology | Domain supplement (financial) |
| `eso` | Epistemic Science Ontology | Domain supplement (scientific) |
| `golem` | GOLEM narrative ontology | Domain supplement (narrative) |
| `mlschema` | ML-Schema | Domain supplement (ML training) |
| `axis` | Domain-selection logic | `OntologyAxis`, `OntologyNamespace`, `OntologyAnchor`, `select_ontology_anchor` |

## Usage

```rust
use hkask_bridge_ontology::{dc_bibo, pko, fibo, axis};

// Universal vocabulary.
let title = dc_bibo::TITLE;            // "dcterms:title"
let step = pko::STEP_EXECUTION;        // "pko:StepExecution"

// Domain vocabulary.
let roic = fibo::RETURN_ON_INVESTED_CAPITAL;  // "fibo-fbc-fct-ra:ReturnOnInvestedCapital"

// Domain selection.
let anchor = axis::select_ontology_anchor("prediction-markets");
// → OntologyAnchor::DomainSupplement { namespace: Fibo, concept: "dcterms:Dataset" }
```

## No dependencies

Pure Rust vocabulary + selection logic. No reasoners, no OWL parsing, no graph
databases. Bridges are thin vocabulary layers, not ontology engines.
