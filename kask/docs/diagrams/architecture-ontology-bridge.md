---
title: "Ontology Bridge Architecture"
audience: [developers, architects, agents]
last_updated: 2026-08-05
version: "0.33.5"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, curation]
---

# Ontology Bridge Architecture

The ontology bridge system is a single shared crate (`hkask-bridge-ontology`)
that owns all ontology vocabulary and the dual-axis domain-selection logic.
No ontology vocabulary lives inside any MCP server; every server that does
tagging depends on this crate. The architecture enforces the orthogonality
of domain maps (ontologies) and functional-area maps (MCP servers).

```mermaid
graph TD
    subgraph shared["hkask-bridge-ontology (shared crate)"]
        direction TB
        axis["axis.rs<br/>domain-selection logic"]
        dc_bibo["dc_bibo.rs<br/>DC + BIBO + CiTO"]
        pko["pko.rs<br/>PKO"]
        fibo["fibo.rs<br/>FIBO (union)"]
        eso["eso.rs<br/>ESO"]
        golem["golem.rs<br/>GOLEM"]
        omc["omc.rs<br/>OMC"]
        mlschema["mlschema.rs<br/>ML-Schema"]
        axis --> dc_bibo
        axis --> pko
    end

    subgraph servers["MCP servers (functional areas)"]
        condenser["hkask-condenser<br/>re-exports axis types"]
        corpus["hkask-mcp-corpus<br/>tagging + triples"]
        media["hkask-mcp-media<br/>omc dispatch + ontology tag"]
        companies["hkask-mcp-companies<br/>fibo dispatch + ontology tag"]
        training["hkask-mcp-training<br/>mlschema dispatch only"]
        pm["hkask-mcp-prediction-markets<br/>FIBO-anchored CMP"]
    end

    condenser -->|"depends on"| shared
    corpus -->|"depends on"| shared
    media -->|"depends on"| shared
    companies -->|"depends on"| shared
    training -->|"depends on"| shared
    pm -->|"depends on"| shared

    subgraph deleted["Deleted (rip-and-replace)"]
        old_dc["~~hkask-bridge-dublincore~~"]
        old_fibo_co["~~companies/fibo.rs~~"]
        old_fibo_cu["~~corpus/bridge/fibo.rs~~"]
        old_eso["~~corpus/bridge/eso.rs~~"]
        old_golem["~~corpus/bridge/golem.rs~~"]
        old_omc["~~media/omc.rs (original, dead surface)~~"]
        old_ml["~~training/mlschema.rs~~"]
    end
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ONT-001
verified_date: 2026-08-05
verified_against: kask/crates/hkask-bridge-ontology/src/hkask_bridge_ontology.rs; kask/crates/hkask-bridge-ontology/src/axis.rs
status: VERIFIED
-->

## The dual-axis invariant

State axis is always Dublin Core. Process axis is the domain ontology when
one applies, PKO otherwise. One axis is always DC or PKO, so every artifact
has a common mapping in process or state space regardless of domain.

```mermaid
flowchart LR
    domain["domain hint<br/>(from server or call)"]
    select["select_ontology_anchor"]
    domain --> select
    select -->|"finance/company"| fibo_anchor["FIBO + DC"]
    select -->|"science/research"| eso_anchor["ESO + DC"]
    select -->|"narrative/corpus"| golem_anchor["GOLEM + DC"]
    select -->|"media/generate"| omc_anchor["OMC + DC"]
    select -->|"training/ml"| ml_anchor["ML-Schema + DC"]
    select -->|"memory/cognitive"| sumo_anchor["SUMO + DC"]
    select -->|"kanban/task/process"| pko_anchor["PKO + DC"]
    select -->|"file/web/registry"| dc_anchor["DC + BIBO"]
    select -->|"unknown"| core_anchor["5W1H Core<br/>(DC + PKO fallback)"]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ONT-002
verified_date: 2026-08-05
verified_against: kask/crates/hkask-bridge-ontology/src/axis.rs:select_ontology_anchor
status: VERIFIED
-->

## See also

- [PRINCIPLES.md P5.4/P8.1](../architecture/core/PRINCIPLES.md) — the dual-axis framework and bridging principles.
- [Ontology Bridge Reference](../reference/ontology-bridge.md) — the API reference for the crate, including the unified tag shape and `explain_tool_for`.
- [Using the Ontology Bridge](../diataxis/hkask-bridge-ontology/how-to.md) — a how-to guide for servers.
