---
title: "Ontology Bridge — API Reference"
audience: [developers, architects, agents]
last_updated: 2026-09-09
version: "0.39.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, curation]
---

# Ontology Bridge — API Reference

**Crate:** `hkask-bridge-ontology` (`kask/crates/hkask-bridge-ontology/`)

The single source of truth for ontology vocabulary and the dual-axis

domain-selection logic in hKask. Eleven vocabularies across twelve modules:
two universal axes, one upper ontology, six domain supplements, and two
pipeline vocabularies — schema.org and RDF 1.1, both consumed by the corpus
assertion pipeline
(`kask/crates/hkask-bridge-ontology/src/hkask_bridge_ontology.rs:4-6`).
No ontology vocabulary lives inside any MCP server; every server that does
tagging depends on this crate (user directive 2026-08-05, recorded at
`hkask_bridge_ontology.rs:70-73`).

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

Declared at `hkask_bridge_ontology.rs:90-101`: `axis`, `dc_bibo`, `fibo`,
`golem`, `ml_schema`, `omc`, `pko`, `rdf`, `schema_org`, `sdmx`, `sepio`,
`sumo`.

### `dc_bibo` — Dublin Core + BIBO + CiTO (state axis, universal)

Canonical URI constants for bibliographic metadata, resource typing, and
citation relationships. The universal "what is this" axis.

| Constant | URI |
|----------|-----|
| `TITLE` | `dcterms:title` |
| `CREATOR` | `dcterms:creator` |
| `DATE` | `dcterms:date` |
| `IDENTIFIER` | `dcterms:identifier` |
| `TEXT` | `dcmitype:Text` |
| `DATASET` | `dcmitype:Dataset` |
| `ARTICLE` | `bibo:Article` |
| `CITES` | `cito:cites` |
| `SUPPORTS` | `cito:supports` |
| `REFUTES` | `cito:refutes` |

Full list: `kask/crates/hkask-bridge-ontology/src/dc_bibo.rs`

**Helpers:** `mime_to_dc_type(mime: &str) -> Option<DcConcept>`
(`dc_bibo.rs:92`).

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
`corpus_stage_to_pko_step` (both in `pko.rs`, at `:172` and `:191`).

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
`bibo:Report`, data outputs → `dcmitype:Dataset`); the companies server's
> metric cache and financial model use plain hKask-internal metric
> identifiers, not ontology URIs.

Full list: `kask/crates/hkask-bridge-ontology/src/fibo.rs`

### `sepio` — SEPIO (scientific evidence and provenance domain)

Canonical URIs for epistemic and evidential reasoning, from the
Monarch Initiative's SEPIO (namespace `http://purl.obolibrary.org/obo/SEPIO_`):
`ASSERTION` (`SEPIO:0000001` — the state-axis type for extracted assertion
h_mems), `ASSERTS_PROPOSITION` (`SEPIO:0000030`), `WAS_SPECIFIED_BY` (`SEPIO:0000041`),
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
| `MODEL_EVALUATION` | `mls:ModelEvaluation` |
| `EVALUATION_MEASURE` | `mls:EvaluationMeasure` |
| `HAS_INPUT` | `mls:hasInput` |
| `HAS_OUTPUT` | `mls:hasOutput` |
| `IMPLEMENTS` | `mls:implements` |

Full list: `kask/crates/hkask-bridge-ontology/src/ml_schema.rs:21-48`

### `sdmx` — SDMX (statistical data domain)

Statistical Data and Metadata eXchange — statistical data from FRED,
DBnomics, World Bank, IMF, OECD, ECB, INSEE
(`hkask_bridge_ontology.rs:29-30`).

| Constant | URI |
|----------|-----|
| `DATASET` | `sdmx:DataSet` |
| `DATA_FLOW` | `sdmx:Dataflow` |
| `DATA_STRUCTURE` | `sdmx:DataStructureDefinition` |
| `TIME_SERIES` | `sdmx:SeriesKey` |
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

### `rdf` — RDF 1.1 core vocabulary (pipeline)

The `rdf:` namespace terms used by the corpus assertion pipeline. RDF 1.1
publishes a small closed vocabulary (22 terms); the complete official list
is pinned by `fixtures/rdf-11-terms.txt`, and `all_terms_are_official`
fails the build if a term drifts. The pipeline uses exactly one term:
`TYPE` (`rdf:type`, `rdf.rs:25`). Notably, RDF 1.1 publishes **no creator
property** — the former `rdf:creator` literal in the corpus dimension mapping
was fabricated; the real term is `dcterms:creator`.

### `schema_org` — schema.org predicate bridge (pipeline)

Canonical predicate URIs for the corpus assertion pipeline's expository
passages (concepts, analysis, arguments) — the general-purpose vocabulary
the extraction prompts offer alongside the domain ontologies. Every URI is
verified against the official machine-readable release:
`fixtures/schema-org-terms.txt` pins the term list, and `all_terms_are_official`
fails the build on drift (`schema_org.rs:37-42` declares the term macro).

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
| `AUTONOMOUS_AGENT` | `sumo:AutonomousAgent` |
| `RELATION` | `sumo:Relation` |

Full list: `kask/crates/hkask-bridge-ontology/src/sumo.rs:32-48`

> **Deleted surface:** there is no `five_w_one_h` module. The 5W1H
> interrogative survives only as the `Core` anchor tier (label
> `"5w1h_core"`, `kask/crates/hkask-bridge-ontology/src/axis.rs:213`) — the
> ground for artifacts with an empty domain hint. There is also no
> `research_stage_to_pko` helper (removed with the research-stage mapping;
`pko.rs` ships `kanban_status_to_pko_execution` and
`corpus_stage_to_pko_step` only).

### `axis` — Domain-selection logic

The core of the system: maps a domain hint to its axis anchoring.

**Types** (`kask/crates/hkask-bridge-ontology/src/axis.rs`):

| Type | Description |
|------|-------------|
| `OntologyAxis` | `Pko` or `DcBibo` — which axis of the dual-axis framework (`axis.rs:35`) |
| `OntologyNamespace` | `Fibo`, `Sepio`, `Golem`, `MlSchema`, `Sdmx`, `Omc`, `Sumo` — which domain supplement (`axis.rs:49-70`) |
| `OntologyAnchor` | `Core`, `DualAxis { axis, concept }`, or `DomainSupplement { namespace, concept }` — the 3-tier anchoring (`axis.rs:140-152`) |

**Functions:**

| Function | Signature | Description |
|----------|-----------|-------------|
| `select_ontology_anchor` | `(domain: &str) -> OntologyAnchor` (`axis.rs:252`) | Select the ontology anchoring for a domain. State axis always DC; process axis is the domain ontology or PKO; unknown → SUMO; empty → Core. |
| `OntologyNamespace::dc_concept` | `(&self) -> DcConcept` (`axis.rs:75`) | Map namespace to its canonical DC concept. |
| `OntologyNamespace::pko_concept` | `(&self) -> PkoConcept` (`axis.rs:90`) | Map namespace to its canonical PKO concept. |
| `OntologyAnchor::confidence_modifier` | `(&self) -> f64` (`axis.rs:164`) | Confidence modifier for saliency weighting. |
| `OntologyAnchor::density_factor` | `(&self) -> f64` (`axis.rs:182`) | Information density expectation. |
| `OntologyAnchor::axis` | `(&self) -> Option<OntologyAxis>` (`axis.rs:202`) | Which axis this anchor belongs to. |
| `OntologyAnchor::tier_label` | `(&self) -> &str` (`axis.rs:211`) | Human-readable tier label. |

Keyword matching is token-aware (`axis.rs:254-262`): the hint must equal the
keyword, start with it, or contain it preceded by `_` or space — so
`company_profile` matches `company` but `logistics` does not match `log`.

## Domain → ontology mapping

Verified against `select_ontology_anchor` (`axis.rs:252-444`):

| Domain hint keywords | Namespace | State axis | Process axis |
|---------------------|-----------|------------|--------------|
| `economic`, `fred`, `dbnomics`, `worldbank`, `indicator`, `timeseries` | SDMX | DC | SDMX |
| `finance`, `company`, `stock`, `portfolio`, `dcf`, `screener` | FIBO | DC | FIBO |
| `forecast`, `scenario` | (PKO) | DC | PKO |
| `prediction-markets` | (DC+BIBO) | DC | DC+BIBO |
| `science`, `research`, `hypothesis`, `evidence` | SEPIO | DC | SEPIO |
| `narrative`, `literature`, `persona`, `author`, `corpus` | GOLEM | DC | GOLEM |
| `training`, `ml`, `adapter`, `sweep`, `lora` | ML-Schema | DC | ML-Schema |
| `media`, `image`, `video`, `audio`, `gallery`, `face`, `speech`, `voice`, `transcribe`, `meme`, `collage`, `album`, `gif` | OMC | DC | OMC |
| `kanban`, `board`, `task`, `spec`, `skill`, `docproc`, `curator`, `kata`, `condenser` | (PKO) | DC | PKO |
| `file`, `web`, `registry`, `wallet` | (DC+BIBO) | DC | DC+BIBO |
| (empty) | (Core) | DC | PKO |
| (unknown, non-empty) | SUMO | DC | SUMO |

> The OMC arm (`axis.rs:351-380`) was added so the condenser's tool-name-derived
> anchors route media tools to their domain ontology instead of SUMO;
> deliberately generic tokens (`model`, `job`, `workflow`, `prompt`) are
> excluded. The GOLEM arm's `replica` keyword was removed with the
> persona/replica system (`axis.rs:326-332`).

## Unified ontology tag shape

MCP servers emit a single top-level `"ontology"` key in tool output JSON,
carrying a concept URI string (e.g. `"pplan:Step"`, `"fibo-sec-sec-ast:Portfolio"`,
`"omc:CreativeWork"`). Verified emitters:

| Server | JSON key | Value example | Evidence |
|---|---|---|---|
| companies | `"ontology"` | `"fibo-be-le-cb:Corporation"` | `kask/mcp-servers/hkask-mcp-companies/src/fibo.rs:149-161` |
| curator | `"ontology"` | per-template | `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:599` |
| media | `"ontology"` | `"omc:CreativeWork"` | `kask/mcp-servers/hkask-mcp-media/src/media_block.rs:19-25` |
| portfolio | `"ontology"` | per-tool | `kask/mcp-servers/hkask-mcp-portfolio/src/server.rs:54` |
| scenarios | `"ontology"` | per-tool | `kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs` |
| training | span tag via `ToolSpanGuard::with_ontology` | `mls:Data`/`mls:Run`/`mls:Model` | `kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs:314-326` |

The companies server also emits a `"fibo": {...}` map for per-field display
metadata — a separate concern (display vocabulary, not dispatch metadata).

### OMC-bounded affordances (`explain_tool_for`)

The crate root of `omc` exports `explain_tool_for(omc: &str) -> &'static str`
(`kask/crates/hkask-bridge-ontology/src/omc.rs:115-121`) — the unified dispatch
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

## How to tag spans with this crate

> Folded 2026-09-09 from `diataxis/hkask-bridge-ontology/how-to.md`
> (single-file set; this doc is now the crate's only doc — the Diataxis
> INDEX lists the crate as out-of-scope with this file as cross-reference).
> Git history preserves the full original.

**Add the dependency** in your server's `Cargo.toml`:

```toml
[dependencies]
hkask-bridge-ontology = { path = "../../crates/hkask-bridge-ontology" }
```

The crate is `forbid(unsafe_code)` (`hkask_bridge_ontology.rs:1`) and
exposes only `pub` modules plus two root re-exports, `DcConcept` and
`PkoConcept` (`:105-106`) — no feature flags, no build-time configuration.

**Pick the right entry point.** Two layers, used in this order:

1. **Vocabulary constants** — the canonical concept URI strings, one
   module per ontology (see Modules above). Use these when you already
   know the concept.
2. **`select_ontology_anchor`** (`axis.rs:252`) — the domain-hint
   dispatcher. Use this when you have a tool name or domain string and
   want the crate to pick the anchor.

**Step 1 — universal vocabulary directly.** The state axis (Dublin Core +
BIBO + CiTO) and process axis (PKO) are always available; reach for these
first and only escalate to a domain supplement when they are too coarse:

```rust
use hkask_bridge_ontology::{dc_bibo, pko};

let dataset = dc_bibo::DATASET;      // "dcmitype:Dataset"  (dc_bibo.rs)
let procedure = pko::PROCEDURE;      // "pko:Procedure"     (pko.rs:46)
let dc_type = dc_bibo::mime_to_dc_type("application/pdf"); // Some("dcmitype:Text") (dc_bibo.rs:92)
```

PKO ships stage-mapping helpers for servers that convert domain stages
to process concepts: `kanban_status_to_pko_execution` (`pko.rs:172`),
`corpus_stage_to_pko_step` (`pko.rs:191`). GOLEM ships `corpus_op_to_golem`
(`golem.rs:156`). The corpus server's `ontology_anchor` delegates to
`corpus_stage_to_pko_step` and `corpus_op_to_golem` — the canonical
mapping, so it cannot drift.

**Step 2 — domain supplement when the universal axes are too coarse.**
Each supplement module is a flat list of `pub const` URI strings — no
trait, no struct, no runtime state:

```rust
use hkask_bridge_ontology::{fibo, sdmx, sepio, golem, ml_schema, sumo, omc};

let mcap = fibo::MARKET_CAPITALIZATION;  // "fibo-ind-mkt-bas:MarketCapitalization"
let series = sdmx::TIME_SERIES;           // "sdmx:SeriesKey"
let ev = sepio::HAS_EVIDENCE;             // "SEPIO:0000189" (fixture-pinned)
let run = ml_schema::RUN;                 // "mls:Run" (module is ml_schema, not mlschema)
```

**Step 3 — let the crate pick the anchor from a domain hint.**
`select_ontology_anchor` maps the hint to an `OntologyAnchor` using
token-aware keyword matching (the hint must equal the keyword, start with
it, or contain it preceded by `_` or space — `"company_profile"` matches
`company` but `"logistics"` does not match `log`; `matches_kw` at
`axis.rs:254-262`):

```rust
use hkask_bridge_ontology::axis::select_ontology_anchor;

select_ontology_anchor("company_profile")  // → DomainSupplement { Fibo, "dcmitype:Dataset" }
select_ontology_anchor("kanban_board")      // → DualAxis { Pko, "pko:Procedure" }
select_ontology_anchor("some-unknown-domain") // → DomainSupplement { Sumo, "sumo:Entity" }
select_ontology_anchor("")                  // → Core (5W1H ground)
```

Dispatch order: SDMX (`axis.rs:264`) → FIBO (`:290`) → SEPIO (`:308`) →
GOLEM (`:324`) → ML-Schema (`:342`) → OMC (`:351`) → PKO dual-axis (`:387`)
→ DC+BIBO dual-axis (`:425`) → SUMO fallback / `Core` for the empty hint
(`:437-444`). First matching keyword set wins. **Fallback discipline:** if
a domain mapping fails or the domain ontology can't place the concept,
fall back to the generalists (DC + PKO) or SUMO — never force a domain
ontology where it doesn't fit. An unknown non-empty domain returns SUMO's
`sumo:Entity`, not an error; an empty hint returns `Core` (`axis.rs:437-444`).

**Step 4 — read the anchor's tier metadata.** The condenser and other
regulation-loop consumers read derived fields off the anchor for
domain-aware saliency weighting — use these instead of re-deriving per
consumer: `confidence_modifier()` (FIBO +0.10, SUMO +0.05, others ±0.00,
`axis.rs:164`), `density_factor()` (FIBO 1.3, ML-Schema/SDMX 1.1, others
1.0, `axis.rs:182`), `tier_label()` (`axis.rs:211`).

**Step 5 — re-export the shared vocabulary in your server.** Keep
server-specific dispatch (mapping your server's tool names or provider
field names to the shared vocabulary) in your server — that is the
server's business, not the ontology's. Re-export so existing call sites
keep one import path; this is the pattern the condenser uses
(`kask/crates/hkask-condenser/src/types.rs:19-21` re-exports
`OntologyAnchor`, `OntologyAxis`, `OntologyNamespace`, and
`select_ontology_anchor`):

```rust
// In your server's ontology module — re-export only the verified FIBO
// terms the server anchors on:
pub use hkask_bridge_ontology::fibo::{CORPORATION, MARKET_CAPITALIZATION};

// Internal metric identifiers are plain hKask canonical names — NOT
// ontology URIs (FIBO publishes no terms for financial ratios; verified
// 2026-08-29).
```

**Step 6 — dispatch the gallery explain tool from an OMC tag.** The only
explain-dispatch function in the crate is `omc::explain_tool_for`
(`omc.rs:115`) — OMC-scoped (see the OMC-bounded affordances section
above). There is no crate-root ontology→explain-tool dispatcher for the
other namespaces; widgets that dispatch on non-OMC ontology tags implement
their own mapping today.

**Phantom-API warnings (verified absent from the tree):** there is no
`five_w_one_h` module (the 5W1H interrogative survives only as the `Core`
anchor tier, `axis.rs:213`); there is no `enrich_with_ontology` helper;
there is no `research_stage_to_pko` helper; `explain_tool_for` lives in
`omc.rs`, not the crate root, and dispatches OMC media concepts only.

## Dependencies

The crate is pure vocabulary + selection logic — no reasoners, no OWL
parsing, no graph databases (`hkask_bridge_ontology.rs:41-43` describes the
orthogonality invariant).

## See also

- [Architecture diagrams](../diagrams/architecture.md) — the ontology-bridge
  architecture and domain-selection flow (consolidated 2026-08-28).
- [PRINCIPLES.md P5.4/P8.1](../architecture/core/PRINCIPLES.md) — the
  dual-axis framework principles.
