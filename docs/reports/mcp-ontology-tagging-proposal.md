# MCP server ontology tagging — standardization proposal

## Status

Implemented — all five steps complete. Tests and clippy pass.

## Decisions (per operator review 2026-08-13)

1. **Span tag is canonical; output field is optional complement.** Every
   server calls `execute_tool_semantic` with a real per-tool anchor. Output-field
   enrichment stays per-widget (e.g., portfolio's "Explain" affordance).

2. **Economic-data tools get real ontology anchors, not `None`.** FRED,
   DBnomics, and World Bank all use SDMX (Statistical Data and Metadata
   eXchange) as their underlying data model. DBnomics is explicitly a
   multi-provider SDMX aggregator (IMF, OECD, ECB, INSEE, World Bank, FRED
   mirrors — see `economic_data/dbnomics.rs` L4-6). The bridge crate needs a
   new SDMX module to host these anchors.

3. **One PR.** Companies (largest divergence), prediction-markets (stub fix +
   new SDMX anchors), scenarios (`&'static str` → `Option<&'static str>`
   migration), portfolio (already conformant — reference implementation).

4. **Prediction-markets fallback is `dcterms:Dataset` (Dublin Core), not
   `None`.** When no domain ontology is obvious for a prediction-markets tool,
   anchor on Dublin Core. This keeps the existing `"dublin-core"` behavior for
   tools where it's genuinely the right answer, but makes it a real per-tool
   decision instead of a constant.

## Problem

The four domain MCP servers (`prediction-markets`, `scenarios`, `portfolio`,
`companies`) apply ontology tagging in three different ways. The divergence is
in substance, not style: ontology lives in different places depending on the
server, and one server calls the semantic API as theater.

### Current state

| Server               | `execute_tool_semantic`?     | `ontology_anchor` fn?                                  | Output-field enrichment?                  |
| -------------------- | ---------------------------- | ------------------------------------------------------ | ----------------------------------------- |
| `prediction-markets` | yes, all tools               | yes — **stub**: returns `"dublin-core"` for every tool | no                                        |
| `scenarios`          | yes, all tools               | yes — real PKO/DC split per tool                       | yes (inline in some outputs)              |
| `portfolio`          | yes, some tools              | yes — real FIBO split per tool                         | no                                        |
| `companies`          | **no** — bare `execute_tool` | **no** fn                                              | yes — `fibo::enrich_with_ontology` inline |

### The theater case

`prediction-markets` calls `execute_tool_semantic` on every tool, but its
`ontology_anchor` is:

```rust
fn ontology_anchor(_tool: &str) -> &'static str {
    "dublin-core"
}
```

Every tool gets the same concept. The semantic call adds a span tag, but the
tag carries no per-tool information — it's a constant. The `reg.tool` span's
`ontology` field is indistinguishable across `market_lookup`, `market_cmp`,
and `fred_search_series`. The call site looks like it's doing ontology routing;
the implementation isn't.

## Implementation plan

### Step 1 — Add SDMX module to `hkask-bridge-ontology`

New file `kask/crates/hkask-bridge-ontology/src/sdmx.rs` with SDMX concept
constants. SDMX is the ISO standard (ISO 17369) for statistical data exchange;
all three economic-data providers in `prediction-markets` use it as their
data model. The RDF/OWL SDMX ontology defines the relevant concepts.

Proposed constants (initial set — expand as needed):

```rust
pub type SdmxConcept = &'static str;

/// A statistical dataset (FRED series, DBnomics dataset, WB indicator).
pub const DATASET: SdmxConcept = "sdmx:DataSet";
/// A data flow — the publication channel for a dataset (FRED release, WB topic).
pub const DATA_FLOW: SdmxConcept = "sdmx:DataFlow";
/// A data structure definition — the schema/dimensions of a dataset.
pub const DATA_STRUCTURE: SdmxConcept = "sdmx:DataStructureDefinition";
/// A time series — the per-series observation sequence.
pub const TIME_SERIES: SdmxConcept = "sdmx:TimeSeries";
/// A single observation (period + value).
pub const OBSERVATION: SdmxConcept = "sdmx:Observation";
/// A category in the SDMX category scheme (FRED category tree, WB topics).
pub const CATEGORY: SdmxConcept = "sdmx:Category";
/// A data provider (IMF, OECD, ECB, INSEE, FRED, World Bank).
pub const DATA_PROVIDER: SdmxConcept = "sdmx:DataProvider";
```

Add `Sdmx` variant to `OntologyNamespace` in `axis.rs` with:

- `dc_concept()` → `dc_bibo::DATASET` (statistical data is a dataset)
- `pko_concept()` → `pko::PROCEDURE` (data retrieval is a procedure)
- `FromStr` / `Display` for `"sdmx"`

### Step 2 — Prediction-markets: real per-tool anchors

Replace the stub in `hkask_mcp_prediction_markets.rs`:

```rust
fn ontology_anchor(tool: &str) -> Option<&'static str> {
    use hkask_bridge_ontology::{dc_bibo, fibo, sdmx};
    match tool {
        // Economic-data tools — SDMX
        "fred_search_series" | "fred_get_series_info"
        | "fred_list_categories" | "fred_get_release" => Some(sdmx::DATASET),
        "fred_get_observations" => Some(sdmx::TIME_SERIES),
        "wb_search_indicators" | "wb_list_topics" | "wb_get_indicator_info" => Some(sdmx::DATASET),
        "wb_get_observations" => Some(sdmx::TIME_SERIES),
        "wb_list_countries" => Some(sdmx::CATEGORY),
        "dbnomics_search" | "dbnomics_get_dataset" => Some(sdmx::DATASET),
        "dbnomics_list_providers" => Some(sdmx::DATA_PROVIDER),
        "dbnomics_get_series" => Some(sdmx::TIME_SERIES),

        // Market tools — Dublin Core fallback (no prediction-market ontology yet)
        "prediction_markets_status" | "market_lookup" | "market_match"
        | "market_ontology_map" | "market_calibration" | "market_record_resolution"
        | "market_subscribe_resolutions" | "market_ladder" | "market_cmp"
        | "market_residual" | "market_check_resolutions" | "market_history"
        | "market_cmp_index" | "market_volatility" | "market_cmp_index_store"
        | "market_cmp_portfolio_store" | "market_cmp_context_suggest" => {
            Some(dc_bibo::DATASET)
        }

        // EQM — Dublin Core (rationale is a dataset)
        "market_score_rationale" => Some(dc_bibo::DATASET),

        _ => Some(dc_bibo::DATASET), // Dublin Core fallback per decision 4
    }
}
```

Signature migrates from `&'static str` to `Option<&'static str>`. Call sites
change from `Some(Self::ontology_anchor(...))` to `Self::ontology_anchor(...)`
(the `Option` is now in the anchor fn itself, matching portfolio's pattern).

### Step 3 — Companies: adopt `execute_tool_semantic` + anchor fn

Add a shared `ontology_anchor` fn in `hkask_mcp_companies.rs`:

```rust
fn ontology_anchor(tool: &str) -> Option<&'static str> {
    use hkask_bridge_ontology::{dc_bibo, fibo};
    match tool {
        // Financial data
        "company_profile" => Some(fibo::CORPORATION),
        "stock_quote" | "historical_price" => Some(fibo::MARKET_CAPITALIZATION),
        "income_statement" | "balance_sheet" | "cash_flow_statement"
        | "key_metrics" => Some(dc_bibo::DATASET),
        "symbol_search" => Some(dc_bibo::IDENTIFIER),

        // MAIA analysis
        "moat_check" => Some(fibo::COMPETITIVE_ADVANTAGE),
        "management_scorecard" => Some(fibo::CAPITAL_ALLOCATION),
        "working_capital_cycle" => Some(fibo::NET_WORKING_CAPITAL),

        // Valuation
        "dcf_valuation" => Some(fibo::DCF_VALUATION),
        "reverse_dcf" => Some(fibo::DCF_VALUATION),
        "ep_valuation" => Some(fibo::ECONOMIC_PROFIT),
        "comparable_analysis" => Some(fibo::COMPARABLE_COMPANY_ANALYSIS),
        "scenario_analysis" => Some(fibo::SCENARIO_PROBABILITY),
        "sensitivity_analysis" => Some(fibo::SENSITIVITY_ANALYSIS),
        "monte_carlo_dcf" => Some(fibo::MONTE_CARLO_DCF),
        "scenario_impact_valuation" => Some(fibo::SCENARIO_PROBABILITY),
        "calibrate_forecast" => Some(fibo::BRIER_SCORE),
        "forecast_record" => Some(fibo::FORECAST_ID),

        // Research & screening
        "research_search" => Some(dc_bibo::DATASET),
        "stock_screener" => Some(fibo::STOCK_SCREENER),
        "expectations_gap" => Some(fibo::REVENUE_GROWTH_RATE),

        // Portfolio
        "ledger_import" | "ledger_export" | "portfolio_list" | "portfolio_delete"
        | "transaction_note_append" => Some(fibo::TRANSACTION_LEDGER),
        "note_add" | "note_list" | "note_delete" => Some(dc_bibo::DESCRIPTION),
        "file_attach" | "file_list" | "file_delete" => Some(dc_bibo::TYPE),
        "portfolio_attribution" => Some(fibo::ATTRIBUTION_ANALYSIS),
        "portfolio_characteristics" => Some(fibo::WEIGHTED_AVERAGE),
        "portfolio_comparison" => Some(fibo::COMPARABLE_COMPANY_ANALYSIS),
        "portfolio_returns" => Some(fibo::TIME_WEIGHTED_RETURN),

        // Transcript
        _ => Some(dc_bibo::TEXT),
    }
}
```

Then migrate all ~44 call sites in `tools/*.rs` from `execute_tool(self, "name", async {...})`
to `execute_tool_semantic(self, "name", Self::ontology_anchor("name"), async {...})`.

The existing `fibo::enrich_with_ontology` calls in `financial_data.rs` stay —
they enrich the _output JSON_ for widget consumption, which is the optional
complement layer (decision 1).

### Step 4 — Scenarios: `&'static str` → `Option<&'static str>`

Migrate `ontology_anchor` to return `Option<&'static str>`:

```rust
fn ontology_anchor(tool: &str) -> Option<&'static str> {
    match tool {
        "scenario_frame" | "scenario_brainstorm" | "scenario_build" => Some("pko:Procedure"),
        _ => Some("dcterms:Dataset"),
    }
}
```

Call sites change from `Some(Self::ontology_anchor(...))` to
`Self::ontology_anchor(...)`. No tools currently return `None`, but the
`Option` admits future tools with no natural anchor.

### Step 5 — Portfolio: no change (reference implementation)

Portfolio already uses `Option<&'static str>` and `execute_tool_semantic`
correctly. It's the reference implementation for the other three to match.

## Scope and risk

- **Companies is the largest change**: ~44 tool call sites across 8 submodules
  switch from `execute_tool` to `execute_tool_semantic` + an anchor fn. This is
  mechanical but wide. The anchor fn is shared in the root module to avoid
  duplication across submodules.
- **Prediction-markets** needs the new SDMX module + `OntologyNamespace`
  variant, plus the stub replacement. The anchor fn is the design work; the
  call-site migration is mechanical (drop the `Some(...)` wrapper).
- **Scenarios** is a small signature migration.
- **Portfolio** is conformant — no change.
- **Bridge crate** gains a new module (`sdmx.rs`) and a new
  `OntologyNamespace::Sdmx` variant. The `dc_concept` / `pko_concept` /
  `FromStr` / `Display` impls must be extended.

## Verification

- Existing pin tests (`tool_surface_is_exactly_*`) must still pass — the
  migration doesn't add or remove tools, only changes how they're dispatched.
- New test in `hkask-bridge-ontology`: assert `OntologyNamespace::Sdmx`
  resolves correctly and `sdmx::DATASET` is a valid `&'static str`.
- New test in `prediction-markets`: assert `ontology_anchor` returns distinct
  concepts for at least two economic-data tools and two market tools (catches
  future stub regressions).
- `./script/clippy` clean across all four server crates + the bridge crate.

## Non-goals

- Standardizing `called_tools` / `<server>_status` — only meaningful where a
  server has session state worth introspecting. Portfolio (stateless ledger)
  and companies (stateless fetcher) would get dead surface.
- Standardizing `check_sequence` / `expected_predecessor` — scenarios-specific
  pipeline discipline, not a general pattern.
- Standardizing `combined_router` — only needed when a server splits tools
  across files. Forcing it on single-file servers is ceremony.
- A prediction-market-specific ontology (e.g., for `market_cmp`, `market_volatility`).
  Dublin Core is the fallback per decision 4; a dedicated PM ontology is a
  future proposal if the span routing needs finer granularity.
