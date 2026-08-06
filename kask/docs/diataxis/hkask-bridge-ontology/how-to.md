---
title: "Using the Ontology Bridge — How-to Guide"
audience: [developers, agents]
last_updated: 2026-08-05
version: "0.33.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, curation]
---

# Using the Ontology Bridge

This guide shows MCP servers how to use the shared `hkask-bridge-ontology`
crate for ontology vocabulary and domain selection. Every server that does
tagging, classification, or ontology anchoring depends on this crate; no
ontology vocabulary lives inside a server.

## Add the dependency

In your server's `Cargo.toml`:

```toml
[dependencies]
hkask-bridge-ontology = { path = "../../crates/hkask-bridge-ontology" }
```

## Use the universal vocabulary

The state axis (Dublin Core + BIBO + CiTO) and process axis (PKO) are
always available:

```rust
use hkask_bridge_ontology::{dc_bibo, pko};

// State axis — what is this?
let title = dc_bibo::TITLE;           // "dcterms:title"
let dataset = dc_bibo::DATASET;       // "dcterms:Dataset"
let article = dc_bibo::ARTICLE;       // "bibo:Article"

// Process axis — how did this come to be?
let procedure = pko::PROCEDURE;       // "pko:Procedure"
let step = pko::STEP_EXECUTION;       // "pko:StepExecution"
```

## Use a domain supplement

Domain ontologies (FIBO, ESO, GOLEM, OMC, ML-Schema) are available when the
universal axes aren't specific enough:

```rust
use hkask_bridge_ontology::fibo;
use hkask_bridge_ontology::omc;

// Financial domain
let roic = fibo::RETURN_ON_INVESTED_CAPITAL;  // "fibo-fbc-fct-ra:ReturnOnInvestedCapital"
let dcf = fibo::DCF_VALUATION;                // "fibo:dcfValuation"

// Media domain
let creative_work = omc::CREATIVE_WORK;      // "omc:CreativeWork"
let scene = omc::SCENE;                       // "omc:Scene"
```

## Select the ontology anchor for a domain

The `select_ontology_anchor` function maps a domain hint to its axis
anchoring. State axis is always Dublin Core; process axis is the domain
ontology when one applies, PKO otherwise:

```rust
use hkask_bridge_ontology::axis::{select_ontology_anchor, OntologyAnchor};

// From a tool name (the condenser pattern)
let anchor = select_ontology_anchor("company_profile");
// → OntologyAnchor::DomainSupplement { namespace: Fibo, concept: "dcterms:Dataset" }

// From a bare domain hint
let anchor = select_ontology_anchor("media");
// → OntologyAnchor::DomainSupplement { namespace: Omc, concept: "dcterms:Collection" }

// Unknown domain → fallback to the generalists (DC + PKO)
let anchor = select_ontology_anchor("some-unknown-domain");
// → OntologyAnchor::Core
```

The invariant: one axis is always DC or PKO, so every artifact has a common
mapping in process or state space regardless of domain.

## Keep server-specific dispatch in the server

The ontology vocabulary lives in the shared crate. Server-specific dispatch
— mapping your server's tool names or provider field names to the shared
vocabulary — stays in your server. That is the server's business, not the
ontology's.

```rust
// In your server (e.g. companies server's fibo.rs):
// Re-export the shared vocabulary so existing call sites keep working.
pub use hkask_bridge_ontology::fibo::{
    RETURN_ON_INVESTED_CAPITAL, PRICE_EARNINGS_RATIO, /* ... */
};

// Keep the server-specific mapping here.
pub fn fmp_field_to_fibo(field: &str) -> Option<FiboConcept> {
    match field {
        "roic" => Some(RETURN_ON_INVESTED_CAPITAL),
        "peRatio" => Some(PRICE_EARNINGS_RATIO),
        _ => None,
    }
}
```

## The dual-axis invariant

When you build an `OntologyAnchor`, the state axis is always Dublin Core. The
process axis is the domain ontology when one applies, PKO otherwise. This
ensures every artifact has a common mapping in process or state space:

```rust
use hkask_bridge_ontology::axis::{OntologyAnchor, OntologyNamespace, OntologyAxis};
use hkask_bridge_ontology::dc_bibo;

// Domain supplement: FIBO for a financial document
let anchor = OntologyAnchor::DomainSupplement {
    namespace: OntologyNamespace::Fibo,
    concept: dc_bibo::DATASET.to_string(),
};

// Dual-axis: PKO for a process document
let anchor = OntologyAnchor::DualAxis {
    axis: OntologyAxis::Pko,
    concept: hkask_bridge_ontology::pko::PROCEDURE.to_string(),
};

// Core: the 5W1H fallback when no domain applies
let anchor = OntologyAnchor::Core;
```

## Fallback discipline

If a domain mapping fails or the domain ontology can't place the concept,
fall back to the generalists (DC + PKO). Never force a domain ontology where
it doesn't fit — the generalists are always valid:

```rust
let anchor = select_ontology_anchor("some-unknown-domain");
// → OntologyAnchor::Core (the 5W1H fallback)
// This is correct behavior, not an error.
```

## See also

- [Ontology Bridge API Reference](../../reference/ontology-bridge.md) — the full API.
- [Ontology Bridge Architecture](../../diagrams/architecture-ontology-bridge.md) — the architecture diagram.
- [PRINCIPLES.md P5.4/P8.1](../../architecture/core/PRINCIPLES.md) — the dual-axis framework principles.
