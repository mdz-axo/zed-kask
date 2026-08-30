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

## The fallback ladder (P8.3)

Ontology anchoring is a scope-broadening walk, never a single pick. When a
concept has no fit in the narrowest applicable ontology, the anchor falls
to progressively broader scopes until one fits:

1. **Domain supplement** — the domain's specific ontology (FIBO, SEPIO,
   GOLEM, ML-Schema, SDMX, OMC), when the concept exists in that
   ontology's *published* vocabulary. Never force a concept into an
   ontology that has no place for it in its graph.
2. **Universal axes** — Dublin Core + BIBO (the state axis: what the
   artifact *is*) and PKO (the process axis: how it came to be). Always
   applicable to artifacts and processes. Implemented as
   `OntologyNamespace::dc_concept` / `pko_concept` (`axis.rs`).
3. **Upper ontology** — SUMO (Entity, Process, Quantity, Proposition):
   formal categorization when no domain or axis concept fits — e.g. a
   financial metric with no FIBO term is a `sumo:Quantity`.
4. **Interrogative ground** — the 5W1H core: the guaranteed final rung.

The invariant: **nothing is ever untagged.** SUMO and the 5W1H core exist
precisely so the ladder always terminates on a real anchor. Skipping rungs
to force a fit, or stopping above a rung that fits (emitting no tag), both
violate the ladder. `select_ontology_anchor` implements the ladder in
dispatch form (rungs named in its doc comment);
`fallback_ladder_terminates_on_a_real_anchor` (`axis.rs` tests) pins it.

## Modules

Declared at `hkask_bridge_ontology.rs:58-67`: `axis`, `dc_bibo`, `sepio`,
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

Canonical URI constants for procedures, steps, executions, and verification,
from the official PKO v2.0.0 (Carriero et al., <https://w3id.org/pko>),
fixture-pinned by `fixtures/pko-2.0.0-terms.txt`. The universal "how did
this come to be" axis. PKO reuses P-Plan, PROV-O, and Dublin Core terms;
reused terms keep their canonical prefixes (`pplan:Step`, `prov:Agent`,
`dcterms:references`) — never re-prefixed under `pko:`.

| Constant | URI |
|----------|-----|
| `PROCEDURE` | `pko:Procedure` |
| `STEP` | `pplan:Step` |
| `MULTI_STEP` | `pplan:MultiStep` |
| `STEP_EXECUTION` | `pko:StepExecution` |
| `PROCEDURE_EXECUTION` | `pko:ProcedureExecution` |
| `STEP_VERIFICATION` | `pko:StepVerification` |
| `ACTION` | `pko:Action` |
| `AGENT` | `prov:Agent` |

> History: verification (2026-08-29, PKO v2.0.0 OWL) corrected five
> mis-prefixed terms (`pko:Step` → `pplan:Step`, `pko:MultiStep` →
> `pplan:MultiStep`, `pko:Agent` → `prov:Agent`, `pko:references` →
> `dcterms:references`), removed the fabricated `pko:ProcedureTarget`,
> and remapped `kanban_status_to_pko_execution` onto PKO's four published
> execution-status individuals (`pko:InProgress`, `pko:Completed`,
> `pko:Paused`, `pko:Cancelled`) — the former
> `pko:ProcedureExecutionStatus/queued|verifying|blocked` path-suffixed
> URIs were fabricated. Statuses with no published individual omit the
> annotation rather than force one.

Full list: `kask/crates/hkask-bridge-ontology/src/pko.rs`

**Helpers:** `kanban_status_to_pko_execution`,
`corpus_stage_to_pko_step`, `research_stage_to_pko` (all in `pko.rs`).

### `fibo` — Financial Industry Business Ontology (financial domain)

Canonical URIs from the official FIBO (EDM Council / OMG,
<https://spec.edmcouncil.org/fibo/>), each mechanically verified against the
FIBO master ontology and pinned by `fixtures/fibo-verified-terms.txt`:
`CORPORATION` (`fibo-be-le-cb:Corporation`), `TICKER_SYMBOL`
(`fibo-sec-sec-id:TickerSymbol`), `PORTFOLIO` (`fibo-sec-sec-ast:Portfolio`),
`MARKET_CAPITALIZATION` (`fibo-ind-mkt-bas:MarketCapitalization`),
`INTERNAL_RATE_OF_RETURN` (`fibo-fbc-fi-ip:InternalRateOfReturn`), plus the
economic-indicator terms (`CONSUMER_PRICE_INDEX`, `PRODUCER_PRICE_INDEX`,
`GROSS_DOMESTIC_PRODUCT`, `ECONOMIC_INDICATOR`, `REFERENCE_INDEX`,
`REFERENCE_INTEREST_RATE`, `INTEREST_RATE_BENCHMARK`).

> History: verification (2026-08-29, FIBO master) found 63 of the 70 terms
> formerly carried here were fabricated — the `fibo-fbc-fct-ra` "Financial
> Ratios" module never existed in FIBO, and FIBO publishes no terms for
> financial ratios, DCF line items, valuation methods, or portfolio
> transactions. Per the operator decision, concepts with no real FIBO
> equivalent anchor on Dublin Core at the consumer (analysis outputs →
`bibo:Report`, data outputs → `dcterms:Dataset`); the companies server's
> metric cache and financial model use plain hKask-internal metric
> identifiers, not ontology URIs.

Full list: `kask/crates/hkask-bridge-ontology/src/fibo.rs`

### `sepio` — SEPIO (scientific evidence and provenance domain)

Canonical URIs for epistemic and evidential reasoning, from the
Monarch Initiative's SEPIO (namespace `http://purl.obolibrary.org/obo/SEPIO_`):
`ASSERTION` (`SEPIO:0000001` — the state-axis type for extracted assertion
h_mems), `OBJECTIVE_SPECIFICATION` (`IAO:0000005` — IAO reuse, defined in
the SEPIO OWL; the anchor for target conditions / goals),
`ASSERTS_PROPOSITION` (`SEPIO:0000030`), `WAS_SPECIFIED_BY` (`SEPIO:0000041`),
`HAS_DISPUTING_EVIDENCE_LINE` (`SEPIO:0000008`), `CONTRADICTS` (`SEPIO:0000101`),
`HAS_CONFIDENCE_LEVEL` (`SEPIO:0000167`), `HAS_EVIDENCE` (`SEPIO:0000189`),
`HAS_SUPPORTING_EVIDENCE` (`SEPIO:0000440`), `HAS_DISPUTING_EVIDENCE`
(`SEPIO:0000441`). Every term is pinned by
`fixtures/sepio-2023-06-13-terms.txt` (official OWL release 2023-06-13).

Full list: `kask/crates/hkask-bridge-ontology/src/sepio.rs`

> History: this module replaces the former `eso` module ("Epistemic Science
> Ontology"), which was a fabrication — no such ontology was ever published.
> Only former ESO functions with a real SEPIO equivalent survived; the rest
> were dropped.

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
| `CAPTURE` | `omc:Capture` |
| `ASSET` | `omc:Asset` |
| `TASK` | `omc:Task` |
| `VERSION_INFO` | `omc:VersionInfo` |

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
| `finance`, `company`, `stock`, `portfolio`, `dcf`, `screener` | FIBO | DC | FIBO |
| `forecast`, `scenario` | (PKO) | DC | PKO |
| `prediction-markets` | (DC+BIBO) | DC | DC+BIBO |
| `science`, `research`, `hypothesis`, `evidence` | SEPIO | DC | SEPIO |
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
