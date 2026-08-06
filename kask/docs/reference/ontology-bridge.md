---
title: "Ontology Bridge — API Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-05
version: "0.33.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, curation]
---

# Ontology Bridge — API Reference

**Crate:** `hkask-bridge-ontology` (`kask/crates/hkask-bridge-ontology/`)

The single source of truth for ontology vocabulary and the dual-axis
domain-selection logic in hKask. No ontology vocabulary lives inside any MCP
server; every server that does tagging depends on this crate.

## Modules

### `dc_bibo` — Dublin Core + BIBO + CiTO (state axis, universal)

Canonical URI constants for bibliographic metadata, resource typing, and
citation relationships. The universal "what is this" axis.

| Constant | URI |
|----------|-----|
| `TITLE` | `dcterms:title` |
| `CREATOR` | `dcterms:creator` |
| `DATE` | `dcterms:date` |
| `DESCRIPTION` | `dcterms:description` |
| `IDENTIFIER` | `dcterms:identifier` |
| `SUBJECT` | `dcterms:subject` |
| `TYPE` | `dcterms:type` |
| `TEXT` | `dcterms:Text` |
| `DATASET` | `dcterms:Dataset` |
| `COLLECTION` | `dcterms:Collection` |
| `ARTICLE` | `bibo:Article` |
| `DOCUMENT` | `bibo:Document` |
| `PREPRINT` | `bibo:Preprint` |
| `CITES` | `cito:cites` |
| `SUPPORTS` | `cito:supports` |
| `REFUTES` | `cito:refutes` |

Full list: `kask/crates/hkask-bridge-ontology/src/dc_bibo.rs`

**Helpers:**
- `mime_to_dc_type(mime: &str) -> Option<DcConcept>` — MIME → DC type
- `kind_to_bibo(kind: &str) -> Option<DcConcept>` — informal label → BIBO type

### `pko` — Procedural Knowledge Ontology (process axis, universal)

Canonical URI constants for procedures, steps, executions, and verification.
The universal "how did this come to be" axis.

| Constant | URI |
|----------|-----|
| `PROCEDURE` | `pko:Procedure` |
| `STEP` | `pko:Step` |
| `STEP_EXECUTION` | `pko:StepExecution` |
| `PROCEDURE_EXECUTION` | `pko:ProcedureExecution` |
| `FUNCTION` | `pko:Function` |
| `STEP_VERIFICATION` | `pko:StepVerification` |
| `ACTION` | `pko:Action` |

Full list: `kask/crates/hkask-bridge-ontology/src/pko.rs`

**Helpers:**
- `kanban_status_to_pko_execution(status: &str) -> Option<PkoConcept>`
- `corpus_stage_to_pko_step(stage: &str) -> Option<PkoConcept>`
- `research_stage_to_pko(stage: &str) -> Option<PkoConcept>`
- `task_breakdown_field_to_pko(field: &str) -> Option<PkoConcept>`

### `fibo` — Financial Industry Business Ontology (financial domain)

Canonical URI constants for financial and business analysis. This module is
the union of the two former non-overlapping FIBO subsets (companies-server
financial ratios + corpus-server competitive-advantage concepts).

| Constant | URI |
|----------|-----|
| `CORPORATION` | `fibo-be-le-corp:Corporation` |
| `MARKET_CAPITALIZATION` | `fibo-fbc-fct-ra:MarketCapitalization` |
| `RETURN_ON_INVESTED_CAPITAL` | `fibo-fbc-fct-ra:ReturnOnInvestedCapital` |
| `DISCOUNT_RATE` | `fibo-fbc-fct-ra:DiscountRate` |
| `FREE_CASH_FLOW` | `fibo-fbc-fct-ra:FreeCashFlow` |
| `PORTFOLIO` | `fibo-sec-sec-ast:Portfolio` |
| `WEIGHTED_AVERAGE` | `fibo-ind-ind-ind:WeightedAverage` |
| `COMPETITIVE_ADVANTAGE` | `fibo:hasCompetitiveAdvantage` |
| `ECONOMIC_PROFIT` | `fibo:economicProfit` |

Full list: `kask/crates/hkask-bridge-ontology/src/fibo.rs`

### `eso` — Epistemic Science Ontology (scientific domain)

Canonical predicate URIs for epistemic and scientific reasoning.

| Constant | URI |
|----------|-----|
| `HAS_HYPOTHESIS` | `eso:hasHypothesis` |
| `HAS_EVIDENCE` | `eso:hasEvidence` |
| `FALSIFIED_BY` | `eso:falsifiedBy` |
| `CORROBORATED_BY` | `eso:corroboratedBy` |
| `HAS_UNCERTAINTY` | `eso:hasUncertainty` |

Full list: `kask/crates/hkask-bridge-ontology/src/eso.rs`

### `golem` — GOLEM narrative ontology (narrative domain)

Canonical predicate URIs for narrative concepts.

| Constant | URI |
|----------|-----|
| `CHARACTER` | `golem:G1_Character` |
| `EVENT` | `golem:G1_Event` |
| `HAS_THEME` | `golem:hasTheme` |
| `METAPHOR_FOR` | `golem:metaphorFor` |

Full list: `kask/crates/hkask-bridge-ontology/src/golem.rs`

### `omc` — MovieLabs Ontology for Media Creation (media domain)

Canonical concept URIs for media-production workflows.

| Constant | URI |
|----------|-----|
| `CREATIVE_WORK` | `omc:CreativeWork` |
| `SCENE` | `omc:Scene` |
| `SHOT` | `omc:Shot` |
| `SEQUENCE` | `omc:Sequence` |
| `ASSET` | `omc:Asset` |
| `VERSION` | `omc:Version` |

Full list: `kask/crates/hkask-bridge-ontology/src/omc.rs`

### `mlschema` — ML-Schema (ML training domain)

Canonical concept URIs for machine-learning experiments.

| Constant | URI |
|----------|-----|
| `MODEL` | `mls:Model` |
| `RUN` | `mls:Run` |
| `DATA` | `mls:Data` |
| `HYPER_PARAMETER` | `mls:HyperParameter` |
| `EVALUATION` | `mls:Evaluation` |

Full list: `kask/crates/hkask-bridge-ontology/src/mlschema.rs`

### `axis` — Domain-selection logic

The core of the system: maps a domain hint to its axis anchoring.

**Types:**

| Type | Description |
|------|-------------|
| `OntologyAxis` | `Pko` or `DcBibo` — which axis of the dual-axis framework |
| `OntologyNamespace` | `Fibo`, `Eso`, `Golem`, `Sumo`, `MlSchema`, `Omc` — which domain supplement |
| `OntologyAnchor` | `Core`, `DualAxis { axis, concept }`, or `DomainSupplement { namespace, concept }` — the 3-tier ontology tier |

**Functions:**

| Function | Signature | Description |
|----------|-----------|-------------|
| `select_ontology_anchor` | `(domain: &str) -> OntologyAnchor` | Select the ontology anchoring for a domain. State axis always DC; process axis is the domain ontology or PKO; unknown → Core fallback. |
| `OntologyNamespace::dc_concept` | `(&self) -> DcConcept` | Map namespace to its canonical DC concept. |
| `OntologyNamespace::pko_concept` | `(&self) -> PkoConcept` | Map namespace to its canonical PKO concept. |
| `OntologyAnchor::confidence_modifier` | `(&self) -> f64` | Confidence modifier for saliency weighting. |
| `OntologyAnchor::density_factor` | `(&self) -> f64` | Information density expectation. |
| `OntologyAnchor::axis` | `(&self) -> Option<OntologyAxis>` | Which axis this anchor belongs to. |
| `OntologyAnchor::tier_label` | `(&self) -> &str` | Human-readable tier label. |

## Domain → ontology mapping

| Domain hint keywords | Namespace | State axis | Process axis |
|---------------------|-----------|------------|--------------|
| `finance`, `company`, `stock`, `portfolio`, `dcf`, `prediction-markets` | FIBO | DC | FIBO |
| `science`, `research`, `hypothesis`, `evidence` | ESO | DC | ESO |
| `narrative`, `corpus`, `persona`, `author` | GOLEM | DC | GOLEM |
| `media`, `image`, `video`, `generate`, `face` | OMC | DC | OMC |
| `training`, `ml`, `adapter`, `lora` | ML-Schema | DC | ML-Schema |
| `memory`, `cognitive`, `episodic` | SUMO | DC | SUMO |
| `kanban`, `task`, `spec`, `skill`, `curator` | (PKO) | DC | PKO |
| `file`, `web`, `registry`, `wallet` | (DC+BIBO) | DC | DC+BIBO |
| (unknown) | (Core) | DC | PKO |

## Dependencies

The crate has no dependencies beyond `serde` and `schemars` (for the
`OntologyAnchor`/`OntologyAxis`/`OntologyNamespace` derives). It is pure
vocabulary + selection logic — no reasoners, no OWL parsing, no graph
databases.

## See also

- [Ontology Bridge Architecture](../diagrams/architecture-ontology-bridge.md) — the architecture diagram.
- [Using the Ontology Bridge](../diataxis/hkask-bridge-ontology/how-to.md) — a how-to guide for servers.
- [PRINCIPLES.md P5.4/P8.1](../architecture/core/PRINCIPLES.md) — the dual-axis framework principles.
