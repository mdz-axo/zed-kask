---
title: "Ontology Bridge — API Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-28
version: "0.39.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, curation]
---

# Ontology Bridge — API Reference

**Crate:** `hkask-bridge-ontology` (`kask/crates/hkask-bridge-ontology/`)

The single source of truth for ontology vocabulary and the dual-axis
domain-selection logic in hKask. Nine ontologies across ten modules: two
universal axes, one upper ontology, and six domain supplements
(`kask/crates/hkask-bridge-ontology/src/hkask_bridge_ontology.rs:4-6`).
No ontology vocabulary lives inside any MCP server; every server that does
tagging depends on this crate (user directive 2026-08-05, recorded at
`hkask_bridge_ontology.rs:36-39`).

## Modules

Declared at `hkask_bridge_ontology.rs:58-67`: `axis`, `dc_bibo`, `eso`,
`fibo`, `golem`, `ml_schema`, `omc`, `pko`, `sdmx`, `sumo`.

### `dc_bibo` — Dublin Core + BIBO + CiTO (state axis, universal)

Canonical URI constants for bibliographic metadata, resource typing, and
citation relationships. The universal "what is this" axis.

| Constant | URI |
|----------|-----|
| `TITLE` | `dcterms:title` |
| `CREATOR` | `dcterms:creator` |
| `DATE` | `dcterms:date` |
| `IDENTIFIER` | `dcterms:identifier` |
| `TEXT` | `dcterms:Text` |
| `DATASET` | `dcterms:Dataset` |
| `ARTICLE` | `bibo:Article` |
| `CITES` | `cito:cites` |
| `SUPPORTS` | `cito:supports` |
| `REFUTES` | `cito:refutes` |

Full list: `kask/crates/hkask-bridge-ontology/src/dc_bibo.rs`

**Helpers:** `mime_to_dc_type(mime: &str) -> Option<DcConcept>`
(`dc_bibo.rs:79`).

### `pko` — Procedural Knowledge Ontology (process axis, universal)

Canonical URI constants for procedures, steps, executions, and verification.
The universal "how did this come to be" axis.

| Constant | URI |
|----------|-----|
| `PROCEDURE` | `pko:Procedure` |
| `STEP` | `pko:Step` |
| `STEP_EXECUTION` | `pko:StepExecution` |
| `PROCEDURE_EXECUTION` | `pko:ProcedureExecution` |
| `STEP_VERIFICATION` | `pko:StepVerification` |
| `ACTION` | `pko:Action` |

Full list: `kask/crates/hkask-bridge-ontology/src/pko.rs`

**Helpers:** `kanban_status_to_pko_execution` (`pko.rs:106`),
`corpus_stage_to_pko_step` (`pko.rs:127`), `research_stage_to_pko`
(`pko.rs:156`).

### `fibo` — Financial Industry Business Ontology (financial domain)

Canonical URI constants for financial and business analysis. The union of
the two former non-overlapping FIBO subsets (companies-server financial
ratios + corpus-server competitive-advantage concepts).

| Constant | URI |
|----------|-----|
| `CORPORATION` | `fibo-be-le-corp:Corporation` |
| `MARKET_CAPITALIZATION` | `fibo-fbc-fct-ra:MarketCapitalization` |
| `RETURN_ON_INVESTED_CAPITAL` | `fibo-fbc-fct-ra:ReturnOnInvestedCapital` |
| `DISCOUNT_RATE` | `fibo-fbc-fct-ra:DiscountRate` |
| `FREE_CASH_FLOW` | `fibo-fbc-fct-ra:FreeCashFlow` |
| `PORTFOLIO` | `fibo-sec-sec-ast:Portfolio` |
| `COMPETITIVE_ADVANTAGE` | `fibo:hasCompetitiveAdvantage` |
| `ECONOMIC_PROFIT` | `fibo:economicProfit` |

Full list: `kask/crates/hkask-bridge-ontology/src/fibo.rs`

### `eso` — Epistemic Science Ontology (scientific domain)

Canonical predicate URIs for epistemic and scientific reasoning:
`HAS_HYPOTHESIS` (`eso:hasHypothesis`), `HAS_EVIDENCE` (`eso:hasEvidence`),
`FALSIFIED_BY` (`eso:falsifiedBy`), `CORROBORATED_BY` (`eso:corroboratedBy`),
`HAS_UNCERTAINTY` (`eso:hasUncertainty`).

Full list: `kask/crates/hkask-bridge-ontology/src/eso.rs`

### `golem` — GOLEM narrative ontology (narrative domain)

Canonical URIs from the official GOLEM v1.1 vocabulary (Pianzola et al.,
GOLEM Lab 2024, <https://ontology.golemlab.eu/> — IRI
<https://w3id.org/golem/ontology>, preferred prefix `gc:`). GOLEM extends
CIDOC-CRM and LRMoo and reuses their terms, so the module also carries
`crm:`, `dlp:` (DOLCE-Lite-Plus), and `lrmoo:` URIs: `WORK`
(`lrmoo:F1_Work`), `CHARACTER` (`gc:G1_Character`), `HAS_CHARACTER`
(`gc:GP1i_has_Character`), `HAS_SETTING` (`dlp:setting`), `REFERS_TO`
(`crm:P67_refers_to`). Every term is pinned against the checked-in
official term list `kask/crates/hkask-bridge-ontology/fixtures/golem-v1.1-terms.txt`
by the `all_terms_are_official` test — a URI not in the published ontology
fails the build.

Full list: `kask/crates/hkask-bridge-ontology/src/golem.rs`

### `ml_schema` — ML-Schema (ML training domain)

Canonical concept URIs for machine-learning experiments. The module is
`ml_schema` (snake_case; the crate re-exports it and servers alias it as
`mlschema`, e.g. `kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs:315`).

| Constant | URI |
|----------|-----|
| `MODEL` | `mls:Model` |
| `RUN` | `mls:Run` |
| `DATA` | `mls:Data` |
| `HYPER_PARAMETER` | `mls:HyperParameter` |
| `HYPER_PARAMETER_SETTING` | `mls:HyperParameterSetting` |
| `EVALUATION` | `mls:Evaluation` |
| `EVALUATION_MEASURE` | `mls:EvaluationMeasure` |
| `WAS_DERIVED_FROM` | `mls:wasDerivedFrom` |
| `IMPLEMENTED_BY` | `mls:implementedBy` |
| `HAS_DATA` | `mls:hasData` |

Full list: `kask/crates/hkask-bridge-ontology/src/ml_schema.rs:21-48`

### `sdmx` — SDMX (statistical data domain)

Statistical Data and Metadata eXchange — statistical data from FRED,
DBnomics, World Bank, IMF, OECD, ECB, INSEE
(`hkask_bridge_ontology.rs:29-30`).

| Constant | URI |
|----------|-----|
| `DATASET` | `sdmx:DataSet` |
| `DATA_FLOW` | `sdmx:DataFlow` |
| `DATA_STRUCTURE` | `sdmx:DataStructureDefinition` |
| `TIME_SERIES` | `sdmx:TimeSeries` |
| `OBSERVATION` | `sdmx:Observation` |
| `CATEGORY` | `sdmx:Category` |
| `DATA_PROVIDER` | `sdmx:DataProvider` |

Full list: `kask/crates/hkask-bridge-ontology/src/sdmx.rs:23-40`

### `omc` — MovieLabs OMC (media production domain)

Media production workflows (capture → post → distribution)
(`hkask_bridge_ontology.rs:31-32`).

| Constant | URI |
|----------|-----|
| `CREATIVE_WORK` | `omc:CreativeWork` |
| `SCENE` | `omc:Scene` |
| `SHOT` | `omc:Shot` |
| `SEQUENCE` | `omc:Sequence` |
| `PARTICIPANT` | `omc:Participant` |
| `MEDIA_SOURCE` | `omc:MediaSource` |
| `ASSET` | `omc:Asset` |
| `TASK` | `omc:Task` |
| `VERSION` | `omc:Version` |

Full list: `kask/crates/hkask-bridge-ontology/src/omc.rs:29-53`

### `sumo` — SUMO upper ontology (universal fallback)

The Suggested Upper Merged Ontology — the general-purpose fallback for
domains that don't map to a specific supplement. Provides foundational
categories that all domain supplements specialize. Unknown domains route to
SUMO rather than the bare 5W1H core, so they get formal categorization
(`hkask_bridge_ontology.rs:19-24`).

| Concept | URI |
|--------|-----|
| `ENTITY` | `sumo:Entity` |
| `OBJECT` | `sumo:Object` |
| `PROCESS` | `sumo:Process` |
| `AGENT` | `sumo:Agent` |
| `RELATION` | `sumo:Relation` |

Full list: `kask/crates/hkask-bridge-ontology/src/sumo.rs:32-48`

> **Deleted surface:** there is no `five_w_one_h` module. The 5W1H
> interrogative survives only as the `Core` anchor tier (label
> `"5w1h_core"`, `kask/crates/hkask-bridge-ontology/src/axis.rs:192`) — the
> ground for artifacts with an empty domain hint.

### `axis` — Domain-selection logic

The core of the system: maps a domain hint to its axis anchoring.

**Types** (`kask/crates/hkask-bridge-ontology/src/axis.rs`):

| Type | Description |
|------|-------------|
| `OntologyAxis` | `Pko` or `DcBibo` — which axis of the dual-axis framework (`axis.rs:33`) |
| `OntologyNamespace` | `Fibo`, `Eso`, `Golem`, `MlSchema`, `Sdmx`, `Sumo` — which domain supplement (`axis.rs:47-63`) |
| `OntologyAnchor` | `Core`, `DualAxis { axis, concept }`, or `DomainSupplement { namespace, concept }` — the 3-tier anchoring (`axis.rs:126-138`) |

**Functions:**

| Function | Signature | Description |
|----------|-----------|-------------|
| `select_ontology_anchor` | `(domain: &str) -> OntologyAnchor` (`axis.rs:210`) | Select the ontology anchoring for a domain. State axis always DC; process axis is the domain ontology or PKO; unknown → SUMO; empty → Core. |
| `OntologyNamespace::dc_concept` | `(&self) -> DcConcept` (`axis.rs:67`) | Map namespace to its canonical DC concept. |
| `OntologyNamespace::pko_concept` | `(&self) -> PkoConcept` (`axis.rs:79`) | Map namespace to its canonical PKO concept. |
| `OntologyAnchor::confidence_modifier` | `(&self) -> f64` (`axis.rs:149`) | Confidence modifier for saliency weighting. |
| `OntologyAnchor::density_factor` | `(&self) -> f64` (`axis.rs:162`) | Information density expectation. |
| `OntologyAnchor::axis` | `(&self) -> Option<OntologyAxis>` (`axis.rs:181`) | Which axis this anchor belongs to. |
| `OntologyAnchor::tier_label` | `(&self) -> &str` (`axis.rs:190`) | Human-readable tier label. |

Keyword matching is token-aware (`axis.rs:212-221`): the hint must equal the
keyword, start with it, or contain it preceded by `_` or space — so
`company_profile` matches `company` but `logistics` does not match `log`.

## Domain → ontology mapping

Verified against `select_ontology_anchor` (`axis.rs:210-347`):

| Domain hint keywords | Namespace | State axis | Process axis |
|---------------------|-----------|------------|--------------|
| `economic`, `fred`, `dbnomics`, `worldbank`, `indicator`, `timeseries` | SDMX | DC | SDMX |
| `finance`, `company`, `stock`, `portfolio`, `dcf`, `screener`, `forecast`, `scenario`, `prediction-markets` | FIBO | DC | FIBO |
| `science`, `research`, `hypothesis`, `evidence` | ESO | DC | ESO |
| `narrative`, `literature`, `persona`, `author`, `corpus` | GOLEM | DC | GOLEM |
| `training`, `ml`, `adapter`, `sweep`, `lora` | ML-Schema | DC | ML-Schema |
| `kanban`, `board`, `task`, `spec`, `skill`, `docproc`, `curator`, `kata`, `condenser` | (PKO) | DC | PKO |
| `file`, `web`, `registry`, `wallet` | (DC+BIBO) | DC | DC+BIBO |
| (empty) | (Core) | DC | PKO |
| (unknown, non-empty) | SUMO | DC | SUMO |

## Unified ontology tag shape

MCP servers emit a single top-level `"ontology"` key in tool output JSON,
carrying a concept URI string (e.g. `"pko:Step"`, `"fibo:Portfolio"`,
`"omc:CreativeWork"`). Verified emitters:

| Server | JSON key | Value example | Evidence |
|---|---|---|---|
| companies | `"ontology"` | `"fibo:Portfolio"` | `kask/mcp-servers/hkask-mcp-companies/src/fibo.rs:149-161` |
| curator | `"ontology"` | per-template | `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:599` |
| media | `"ontology"` | `"omc:CreativeWork"` | `kask/mcp-servers/hkask-mcp-media/src/media_block.rs:19-25` |
| portfolio | `"ontology"` | per-tool | `kask/mcp-servers/hkask-mcp-portfolio/src/server.rs:54` |
| scenarios | `"ontology"` | per-tool | `kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs` |
| training | span tag via `ToolSpanGuard::with_ontology` | `mls:Data`/`mls:Run`/`mls:Model` | `kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs:314-326` |

The companies server also emits a `"fibo": {...}` map for per-field display
metadata — a separate concern (display vocabulary, not dispatch metadata).

### OMC-bounded affordances (`explain_tool_for`)

The crate root of `omc` exports `explain_tool_for(omc: &str) -> &'static str`
(`kask/crates/hkask-bridge-ontology/src/omc.rs:76-82`) — the unified dispatch
function mapping an OMC concept to the explain tool a media widget should
invoke:

| Concept | Explain tool |
|---|---|
| `omc:Scene` / `omc:Asset` | `gallery_analyze` |
| others / empty / unknown | `describe_image` (general vision fallback) |

This is OMC-specific dispatch. There is no crate-level fibo/pko/dcterms →
explain-tool mapping; the portfolio widget's "Explain" uses provenance-based
dispatch (server → tool), with the ontology tag carried in the compose-back
body for agent correlation.

## Dependencies

The crate is pure vocabulary + selection logic — no reasoners, no OWL
parsing, no graph databases (`hkask_bridge_ontology.rs:41-43` describes the
orthogonality invariant).

## See also

- [Architecture diagrams](../diagrams/architecture.md) — the ontology-bridge
  architecture and domain-selection flow (consolidated 2026-08-28).
- [Using the Ontology Bridge](../diataxis/hkask-bridge-ontology/how-to.md) —
  a how-to guide for servers.
- [PRINCIPLES.md P5.4/P8.1](../architecture/core/PRINCIPLES.md) — the
  dual-axis framework principles.
